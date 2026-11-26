//! The check of a rules file (`docs/spec/references.md`, REF-14; D39): what [`Rules::parse`]
//! refuses, and what it takes in silence and rarely means: unknown keys, keys given twice,
//! rules that match nothing in a document.

use std::fmt;

use memchr::memmem;
use serde_json::Value;

use super::{
    Context, Invalid, RefKind, Rules, RulesError, Selection, references, select, split_context,
};
use crate::Document;
use crate::problems::Severity;
use xml_atlas_xml::{NodeId, NodeKind, ROOT};

const TOP: &[&str] = &[
    "$schema",
    "includes",
    "attribute_references",
    "contexts",
    "runtime_contexts",
    "variables",
    "repeat",
];
const INCLUDE: &[&str] = &["element", "attribute", "fallback", "skip_when", "enabled"];
const ATTRIBUTES: &[&str] = &["skip_names_with", "enabled"];
const CONTEXT: &[&str] = &["path", "nearest", "up_to", "enabled"];
const REPEAT: &[&str] = &[
    "count",
    "start",
    "suffix",
    "relative",
    "index",
    "relative_index",
    "enabled",
];
/// A key this many changes from a known one is taken for that one misspelt.
const NEAR: usize = 2;

/// Something to look at in a rules file. `line` and `column` count from 1, in characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub line: usize,
    pub column: usize,
    pub message: Remark,
}

/// What a [`Finding`] says. Its words (`Display`) are the English the CLI gives; the window
/// words each case in its own language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remark {
    /// The JSON library's message, without its line and column.
    NotJson(String),
    Invalid(Invalid),
    /// Another reason the rules cannot be read, in the engine's words.
    Other(String),
    NoReferences,
    /// A key, as `includes[0].element`.
    GivenTwice(String),
    /// `meant` is the known key it may be a slip for.
    UnknownKey {
        key: String,
        meant: Option<&'static str>,
    },
    /// An include for an element of this name after the first.
    SecondInclude(String),
    MatchesNothing(RuleName),
}

/// A rule, as the check names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleName {
    Include { element: String, attribute: String },
    AttributeReferences,
    Context(String),
    RunTimeContext(String),
    Variable(String),
    Repeat,
}

/// How many places of the document a rule matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
    pub rule: RuleName,
    pub places: usize,
}

/// What [`check`] found, in the file's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Per rule; empty without a document.
    pub counts: Vec<Count>,
}

/// A key or a value in an array, where it is.
struct Place {
    path: Vec<Part>,
    at: usize,
    /// For a key, where its object starts: the same key twice is twice in one object.
    object: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Key(String),
    Item(usize),
}

impl fmt::Display for Remark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(words) | Self::Other(words) => f.write_str(words),
            Self::Invalid(what) => what.fmt(f),
            Self::NoReferences => f.write_str(
                "these rules name no references: there is neither includes, \
                 attribute_references nor repeat",
            ),
            Self::GivenTwice(key) => write!(f, "{key} is given twice: only the last counts"),
            Self::UnknownKey { key, meant } => {
                write!(f, "unknown key {key:?}, which is ignored")?;
                match meant {
                    Some(meant) => write!(f, "; is it {meant:?}?"),
                    None => Ok(()),
                }
            }
            Self::SecondInclude(element) => write!(
                f,
                "a second include for <{element}> is never used: only the first counts"
            ),
            Self::MatchesNothing(rule) => write!(f, "{rule} matches nothing in the document"),
        }
    }
}

impl fmt::Display for RuleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Include { element, attribute } => write!(f, "include <{element} {attribute}>"),
            Self::AttributeReferences => f.write_str("attribute_references"),
            Self::Context(name) => write!(f, "context {name}"),
            Self::RunTimeContext(name) => write!(f, "run-time context {name}"),
            Self::Variable(name) => write!(f, "variable {name}"),
            Self::Repeat => f.write_str("repeat"),
        }
    }
}

impl Report {
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }
}

/// Checks the text of a rules file, and with `document` how often each rule matches in it.
/// A byte order mark is accepted.
pub fn check(text: &str, document: Option<&Document>) -> Report {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut report = Report::default();
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(e) => {
            let mut message = e.to_string();
            if let Some(at) = message.rfind(" at line ") {
                message.truncate(at);
            }
            let line = e.line().max(1);
            // serde_json counts the column in bytes.
            let bytes = e.column().saturating_sub(1);
            let chars = text.split('\n').nth(line - 1).map_or(0, |l| {
                l.char_indices().take_while(|(i, _)| *i < bytes).count()
            });
            report.findings.push(Finding {
                severity: Severity::Error,
                line,
                column: chars + 1,
                message: Remark::NotJson(message),
            });
            return report;
        }
    };
    let places = places(text);
    let mut found: Vec<(usize, Severity, Remark)> = Vec::new();
    unknown_and_repeated(&places, &mut found);
    match Rules::parse(text) {
        Err(RulesError::Invalid(what)) => {
            let at = place_of_message(&places, &what.to_string());
            found.push((at, Severity::Error, Remark::Invalid(what)));
        }
        Err(e) => found.push((0, Severity::Error, Remark::Other(e.to_string()))),
        Ok(rules) => {
            if rules.is_empty() && rules.repeat.is_none() {
                found.push((0, Severity::Warning, Remark::NoReferences));
            }
            repeated_includes(&places, &value, &rules, &mut found);
            if let Some(doc) = document {
                for (rule, path, count) in matches(doc, &value, &rules) {
                    if count == 0 {
                        let at = place(&places, &path);
                        let message = Remark::MatchesNothing(rule.clone());
                        found.push((at, Severity::Warning, message));
                    }
                    report.counts.push(Count {
                        rule,
                        places: count,
                    });
                }
            }
        }
    }
    found.sort_by_key(|(at, ..)| *at);
    report.findings = found
        .into_iter()
        .map(|(at, severity, message)| {
            let before = &text[..at];
            let start = before.rfind('\n').map_or(0, |i| i + 1);
            Finding {
                severity,
                line: before.matches('\n').count() + 1,
                column: before[start..].chars().count() + 1,
                message,
            }
        })
        .collect();
    report
}

fn unknown_and_repeated(places: &[Place], found: &mut Vec<(usize, Severity, Remark)>) {
    let mut seen = std::collections::HashSet::new();
    for p in places.iter().filter(|p| p.object.is_some()) {
        let Some((Part::Key(key), parent)) = p.path.split_last() else {
            continue;
        };
        if !seen.insert((p.object, key.as_str())) {
            let message = Remark::GivenTwice(name_of(&p.path));
            found.push((p.at, Severity::Warning, message));
        }
        let known = match parent {
            [] => TOP,
            [Part::Key(list), Part::Item(_)] if list == "includes" => INCLUDE,
            [Part::Key(k)] if k == "attribute_references" => ATTRIBUTES,
            [Part::Key(k), Part::Key(_)] if k == "contexts" => CONTEXT,
            [Part::Key(k)] if k == "repeat" => REPEAT,
            // Names the user chooses.
            _ => continue,
        };
        if known.contains(&key.as_str()) {
            continue;
        }
        let near = known
            .iter()
            .map(|k| (distance(k, key), *k))
            .min()
            .filter(|(d, _)| *d <= NEAR);
        let message = Remark::UnknownKey {
            key: key.clone(),
            meant: near.map(|(_, k)| k),
        };
        found.push((p.at, Severity::Warning, message));
    }
}

fn repeated_includes(
    places: &[Place],
    value: &Value,
    rules: &Rules,
    found: &mut Vec<(usize, Severity, Remark)>,
) {
    let at = included(value);
    for (i, inc) in rules.includes.iter().enumerate() {
        let before = rules.includes[..i].iter();
        if before.filter(|b| b.element == inc.element).count() > 0 {
            let path = [Part::Key("includes".into()), Part::Item(at[i])];
            let message = Remark::SecondInclude(inc.element.clone());
            found.push((place(places, &path), Severity::Warning, message));
        }
    }
}

/// Per rule: its name, its place in the file, and how many places of `doc` it matches.
fn matches(doc: &Document, value: &Value, rules: &Rules) -> Vec<(RuleName, Vec<Part>, usize)> {
    let idx = &doc.index;
    let Some(root) = idx.root_element() else {
        return Vec::new();
    };
    let key = |k: &str| Part::Key(k.to_owned());
    let found = references(doc, rules, root);
    let mut out = Vec::new();

    for (inc, i) in rules.includes.iter().zip(included(value)) {
        let count = found
            .iter()
            .filter(|r| {
                r.kind == RefKind::Include && idx.name(&doc.src, r.site) == inc.element.as_bytes()
            })
            .count();
        let name = RuleName::Include {
            element: inc.element.clone(),
            attribute: inc.attribute.clone(),
        };
        out.push((name, vec![key("includes"), Part::Item(i)], count));
    }
    if rules.attributes.is_some() {
        let count = found.iter().filter(|r| r.kind != RefKind::Include).count();
        let path = vec![key("attribute_references")];
        out.push((RuleName::AttributeReferences, path, count));
    }

    // The names `nearest` looks for and the repeat's attribute, in one walk.
    let wanted: Vec<&str> = rules
        .contexts
        .iter()
        .filter_map(|(_, c)| match c {
            Context::Nearest { name, .. } => Some(name.as_str()),
            Context::Path(_) => None,
        })
        .collect();
    let mut named = vec![0usize; wanted.len()];
    let repeat = rules.repeat.as_ref().filter(|r| r.enabled);
    let count_attribute = repeat.map(|r| memmem::Finder::new(r.count.as_bytes()));
    let mut counted = 0;
    if !wanted.is_empty() || repeat.is_some() {
        for id in 0..idx.len() as NodeId {
            let node = idx.node(id);
            if node.kind != NodeKind::Element {
                continue;
            }
            let name = idx.name(&doc.src, id);
            for (n, w) in named.iter_mut().zip(&wanted) {
                *n += usize::from(w.as_bytes() == name);
            }
            if let (Some(finder), Some(rule)) = (&count_attribute, repeat) {
                let start = node.start as usize;
                let head = &doc.src[start..start + node.head_len as usize];
                if finder.find(head).is_some() {
                    let attrs = idx.attributes(&doc.src, id);
                    counted += usize::from(attrs.iter().any(|(k, _)| *k == rule.count));
                }
            }
        }
    }
    let mut named = named.into_iter();
    for (name, context) in &rules.contexts {
        let count = match context {
            Context::Path(path) => match select(doc, ROOT, path) {
                Ok(Selection::Nodes(nodes)) => nodes.len(),
                Ok(Selection::Value(_)) | Err(_) => 0,
            },
            Context::Nearest { .. } => named.next().unwrap_or(0),
        };
        out.push((
            RuleName::Context(name.clone()),
            vec![key("contexts"), key(name)],
            count,
        ));
    }
    for (i, name) in rules.runtime.iter().enumerate() {
        let count = found
            .iter()
            .filter(|r| {
                let expr = super::bracketed(&r.expr).unwrap_or(&r.expr);
                split_context(expr).0 == Some(name.as_str())
            })
            .count();
        let path = vec![key("runtime_contexts"), Part::Item(i)];
        out.push((RuleName::RunTimeContext(name.clone()), path, count));
    }
    for (name, _) in &rules.variables {
        let count = found
            .iter()
            .filter(|r| r.expr.contains(name.as_str()))
            .count();
        out.push((
            RuleName::Variable(name.clone()),
            vec![key("variables"), key(name)],
            count,
        ));
    }
    if repeat.is_some() {
        out.push((RuleName::Repeat, vec![key("repeat")], counted));
    }
    out
}

/// The item in `includes` of each include that is switched on, as [`Rules::parse`] has them.
fn included(value: &Value) -> Vec<usize> {
    let items = value.get("includes").and_then(Value::as_array);
    items
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, item)| item.get("enabled").and_then(Value::as_bool) != Some(false))
        .map(|(i, _)| i)
        .collect()
}

/// Where `path` is last given, as that is the one that counts; else the file's start.
fn place(places: &[Place], path: &[Part]) -> usize {
    places
        .iter()
        .rev()
        .find(|p| p.path == path)
        .map_or(0, |p| p.at)
}

/// Where the parser's `message` is about: it starts with the path.
fn place_of_message(places: &[Place], message: &str) -> usize {
    places
        .iter()
        .filter_map(|p| {
            let name = name_of(&p.path);
            let rest = message.strip_prefix(&name)?;
            rest.starts_with(['.', ' ', ':', '['])
                .then_some((name.len(), p.at))
        })
        .max()
        .map_or(0, |(_, at)| at)
}

/// A path as the parser's messages write it: `includes[0].element`.
fn name_of(path: &[Part]) -> String {
    let mut out = String::new();
    for part in path {
        match part {
            Part::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            Part::Item(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// Where each key and array value of `json` is. `json` is valid: serde_json, which keeps no
/// positions, has read it, and has bounded how deep it nests.
fn places(json: &str) -> Vec<Place> {
    /// An object, waiting for a key or past it, or an array at an item.
    enum In {
        Object { key: bool, start: usize },
        Array { item: usize, fresh: bool },
    }
    let bytes = json.as_bytes();
    let mut out = Vec::new();
    let mut path: Vec<Part> = Vec::new();
    let mut stack: Vec<In> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if !matches!(c, b']' | b',')
            && let Some(In::Array { item, fresh }) = stack.last_mut()
            && *fresh
        {
            *fresh = false;
            path.push(Part::Item(*item));
            out.push(Place {
                path: path.clone(),
                at: i,
                object: None,
            });
        }
        match c {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                if let Some(In::Object { key, start: object }) = stack.last_mut()
                    && *key
                {
                    *key = false;
                    let name = serde_json::from_str(&json[start..=i]).unwrap_or_default();
                    path.push(Part::Key(name));
                    out.push(Place {
                        path: path.clone(),
                        at: start,
                        object: Some(*object),
                    });
                }
            }
            b'{' => stack.push(In::Object {
                key: true,
                start: i,
            }),
            b'[' => stack.push(In::Array {
                item: 0,
                fresh: true,
            }),
            b'}' | b']' => {
                // An empty one has put nothing on the path.
                let part = match stack.pop() {
                    Some(In::Object { key, .. }) => !key,
                    Some(In::Array { fresh, .. }) => !fresh,
                    None => false,
                };
                if part {
                    path.pop();
                }
            }
            b',' => {
                path.pop();
                match stack.last_mut() {
                    Some(In::Object { key, .. }) => *key = true,
                    Some(In::Array { item, fresh }) => {
                        *item += 1;
                        *fresh = true;
                    }
                    None => {}
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// How many changes of one character turn `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.chars().enumerate() {
        let mut corner = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == *y {
                corner
            } else {
                1 + corner.min(above).min(row[j])
            };
            corner = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document with non-ASCII names, a byte order mark and CRLF.
    fn doc() -> Document {
        let text = "\u{feff}<Doc>\r\n<Shared><Größe/></Shared>\r\n<Var/>\r\n\
                    <Include src=\"Shared:Größe\"/>\r\n<Box N=\"[Live:X]\" Count=\"2\"/>\r\n</Doc>";
        Document::from_bytes(text.as_bytes().to_vec())
    }

    fn found(text: &str, doc: Option<&Document>) -> Vec<(Severity, usize, usize, String)> {
        check(text, doc)
            .findings
            .into_iter()
            .map(|f| (f.severity, f.line, f.column, f.message.to_string()))
            .collect()
    }

    /// The one finding in `text`.
    fn only(text: &str, doc: Option<&Document>) -> (Severity, usize, usize, String) {
        let all = found(text, doc);
        assert_eq!(all.len(), 1, "{all:?}");
        all.into_iter().next().unwrap()
    }

    /// D39's table, each row in a file with a byte order mark, CRLF and non-ASCII names.
    #[test]
    fn each_silent_mistake_is_said() {
        let head = "\u{feff}{\r\n";
        let good = "  \"includes\": [{\"element\": \"Include\", \"attribute\": \"src\"}]";

        let f = found(&format!("{head}  \"include\": []\r\n}}"), None);
        assert!(
            f.iter().any(|(_, line, col, m)| {
                (*line, *col) == (2, 3) && m.contains("\"include\"") && m.contains("\"includes\"?")
            }),
            "{f:?}"
        );

        let misspelt = format!(
            "{head}  \"includes\": [{{\"element\": \"Inklüde\", \"attribute\": \"src\"}}]\r\n}}"
        );
        let (severity, line, col, m) = only(&misspelt, Some(&doc()));
        assert_eq!((severity, line, col), (Severity::Warning, 2, 16), "{m}");
        assert!(
            m.contains("Inklüde") && m.contains("matches nothing"),
            "{m}"
        );

        let f = found(&format!("{head}  \"name\": \"Größe\"\r\n}}"), None);
        assert!(
            f.iter().any(|f| f.3.contains("name no references")),
            "{f:?}"
        );

        let (_, line, col, m) = only(&format!("{head}{good},\r\n{good}\r\n}}"), None);
        assert_eq!((line, col), (3, 3));
        assert!(m.contains("given twice"), "{m}");

        let typo = format!(
            "{head}{good},\r\n  \"contexts\": {{\"Größe\": {{\"path\": \"/Doc\", \"enabeld\": false}}}}\r\n}}"
        );
        let (_, line, col, m) = only(&typo, None);
        assert_eq!((line, col), (3, 42), "columns are characters, not bytes");
        assert!(m.contains("\"enabled\"?"), "{m}");

        assert!(found(&format!("{head}{good}\r\n}}\r\n"), Some(&doc())).is_empty());

        let (severity, line, col, m) =
            only(&format!("{head}  \"Größe\": [1,],\r\n{good}\r\n}}"), None);
        assert_eq!((severity, line, col), (Severity::Error, 2, 15), "{m}");
        assert_eq!(m, "trailing comma");
    }

    #[test]
    fn a_refused_rule_is_shown_where_it_is() {
        let text = "{\n \"includes\": [\n  {\"element\": \"A\", \"attribute\": \"a\"},\n  {\"element\": \"B\"}\n ]\n}";
        let (severity, line, col, m) = only(text, None);
        assert_eq!((severity, line, col), (Severity::Error, 4, 3), "{m}");
        assert!(m.contains("includes[1] needs attribute"), "{m}");
        let (severity, line, _, _) =
            only("{\n\"contexts\": {\n \"X\": {\"path\": \"rel\"}}}", None);
        assert_eq!((severity, line), (Severity::Error, 3));
    }

    #[test]
    fn counts_each_rule_in_the_document() {
        let text = r#"{
            "includes": [{"element": "Off", "attribute": "a", "enabled": false},
                         {"element": "Include", "attribute": "src"}],
            "attribute_references": true,
            "contexts": {"Shared": {"path": "/Doc/Shared"},
                         "Var": {"nearest": "Var", "up_to": "Doc"}},
            "runtime_contexts": ["Live"],
            "repeat": {"count": "Count", "index": ["[I]", "$I"]}
        }"#;
        let report = check(text, Some(&doc()));
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        let counts: Vec<(String, usize)> = report
            .counts
            .iter()
            .map(|c| (c.rule.to_string(), c.places))
            .collect();
        let counts: Vec<(&str, usize)> = counts.iter().map(|(r, n)| (r.as_str(), *n)).collect();
        assert_eq!(
            counts,
            [
                ("include <Include src>", 1),
                ("attribute_references", 1),
                ("context Shared", 1),
                ("context Var", 1),
                ("run-time context Live", 1),
                ("repeat", 1),
            ]
        );
        assert!(check(text, None).counts.is_empty());
    }

    #[test]
    fn a_rule_without_a_match_is_marked_at_its_own_place() {
        let text = "{\"includes\": [{\"element\": \"Include\", \"attribute\": \"src\"}],\n\"contexts\": {\n\"Da\": {\"path\": \"/Doc/Shared\"},\n\"Fehlt\": {\"path\": \"/Doc/Fehlt\"}}}";
        let (_, line, col, m) = only(text, Some(&doc()));
        assert_eq!((line, col), (4, 1));
        assert!(m.contains("Fehlt"), "{m}");
    }

    #[test]
    fn a_second_include_for_an_element_is_marked() {
        let text = "{\"includes\": [\n{\"element\": \"A\", \"attribute\": \"a\"},\n{\"element\": \"A\", \"attribute\": \"b\"}]}";
        let (_, line, _, m) = only(text, None);
        assert_eq!(line, 3);
        assert!(m.contains("never used"), "{m}");
    }

    /// Strings with brackets, quotes and commas, empty containers and equal names in other
    /// objects do not confuse the places.
    #[test]
    fn places_follow_the_structure() {
        let text = r#"{"a\"{[,": [{}, [], "],{", 3], "b": {"x": 1}, "c": {"x": [true]}}"#;
        let names: Vec<String> = places(text).iter().map(|p| name_of(&p.path)).collect();
        assert_eq!(
            names,
            [
                "a\"{[,",
                "a\"{[,[0]",
                "a\"{[,[1]",
                "a\"{[,[2]",
                "a\"{[,[3]",
                "b",
                "b.x",
                "c",
                "c.x",
                "c.x[0]"
            ]
        );
    }
}
