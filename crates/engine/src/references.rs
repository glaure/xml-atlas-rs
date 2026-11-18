//! References inside a document: elements that stand for other elements, and attribute
//! values that stand for values (`docs/spec/references.md`, REF-1 to REF-7; D32).
//!
//! Nothing here changes the document. A rules file next to the document, or in a folder
//! above it, says which elements and attributes are references and where their named
//! contexts are; this module finds the references, resolves them for a preview and lists
//! those that find nothing. The syntax is `Context:path` (or `[Context:path]` in an
//! attribute), where the path is a small part of XPath: child names, `*`, `.`, `..`, `//`,
//! `@name`, `text()` and the predicates `[@a='v']`, `[@a]` and `[n]`.

use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::Value;
use xml_atlas_xml::{NodeId, NodeKind, ROOT};

use crate::Document;
use crate::format::newline_style;
use crate::problems::{Problem, Severity};

/// Name of the rules file, looked for next to the document and in each folder above it.
pub const RULES_FILE: &str = "xml-atlas-references.json";

/// Imports inside imports are followed this deep; deeper ones are left as they are.
pub const MAX_DEPTH: usize = 10;

/// How much text a preview makes before it stops.
pub const PREVIEW_LIMIT: usize = 16 * 1024 * 1024;

/// Marks what the preview adds to the text, so it cannot be taken for the document's own.
const NOTE: &str = "XML Atlas:";

/// What the rules file says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules {
    includes: Vec<Include>,
    attributes: Option<AttributeRule>,
    contexts: Vec<(String, Context)>,
    runtime: Vec<String>,
    variables: Vec<(String, Variable)>,
}

/// An element that stands for what its attribute refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Include {
    element: String,
    attribute: String,
    /// Tried when `attribute` finds nothing.
    fallback: Option<String>,
    /// Attribute values (case ignored) that leave the element as it is.
    skip_when: Vec<(String, String)>,
}

/// Attribute values written `[Context:path]` stand for the value found there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AttributeRule {
    /// Attributes whose name contains this are left as they are.
    skip_names_with: Option<String>,
}

/// Where a named context is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Context {
    /// One element, by an absolute path.
    Path(String),
    /// The nearest element called `name`: a child of an ancestor of the reference, up to
    /// the ancestor called `up_to`, whose own descendants called `name` come last.
    Nearest { name: String, up_to: String },
}

/// What a `$NAME` in a path stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variable {
    /// The name of the reference's parent element.
    ParentName,
}

#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("cannot read {path}: {error}")]
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

/// A reference in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// The element that is the reference, or that has the attribute that is.
    pub site: NodeId,
    pub kind: RefKind,
    /// The reference as written, unescaped.
    pub expr: String,
    /// Where it is in the source: the element, or the attribute's value between its quotes.
    pub span: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Include,
    /// The attribute at this index of [`xml_atlas_xml::Index::attributes`].
    Attribute(usize),
}

/// What a reference stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Elements, in document order.
    Nodes(Vec<NodeId>),
    /// A value: text or attribute values.
    Value(String),
    /// Left as it is by the rules (`skip_when`).
    Skipped,
    /// Known only when the program that reads the file runs.
    Runtime(String),
    /// Depends on where it is used: a context that is looked for around the reference
    /// (`nearest`) has nothing around it here.
    Elsewhere(String),
    /// Finds nothing, or cannot be read.
    Unresolved(String),
}

/// A reference resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub resolution: Resolution,
    /// The element a `nearest` context was taken from.
    pub via: Option<NodeId>,
}

/// The selected element with its references resolved, as text (REF-5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    /// Byte ranges of `text` that were imported.
    pub imported: Vec<Range<usize>>,
    /// Byte ranges of `text` the preview added or that did not resolve.
    pub notes: Vec<Range<usize>>,
    /// References met, and how many of them did not resolve.
    pub references: usize,
    pub unresolved: usize,
    /// The text reached the limit and stops there.
    pub cut: bool,
}

impl Rules {
    /// Reads rules from the JSON text of a rules file.
    pub fn parse(text: &str) -> Result<Self, RulesError> {
        let v: Value = serde_json::from_str(text)?;
        let top = v
            .as_object()
            .ok_or_else(|| invalid("the file holds no JSON object"))?;
        let mut rules = Rules::default();
        for (i, inc) in list(top.get("includes"), "includes")?.iter().enumerate() {
            let at = format!("includes[{i}]");
            let o = inc
                .as_object()
                .ok_or_else(|| invalid(format!("{at} is not an object")))?;
            let mut skip_when = Vec::new();
            if let Some(s) = o.get("skip_when") {
                let s = s
                    .as_object()
                    .ok_or_else(|| invalid(format!("{at}.skip_when is not an object")))?;
                for (k, v) in s {
                    skip_when.push((k.clone(), text_of(v, &format!("{at}.skip_when.{k}"))?));
                }
            }
            rules.includes.push(Include {
                element: field(o, "element", &at)?,
                attribute: field(o, "attribute", &at)?,
                fallback: o
                    .get("fallback")
                    .map(|v| text_of(v, &format!("{at}.fallback")))
                    .transpose()?,
                skip_when,
            });
        }
        match top.get("attribute_references") {
            None | Some(Value::Bool(false)) => {}
            Some(Value::Bool(true)) => {
                rules.attributes = Some(AttributeRule {
                    skip_names_with: None,
                })
            }
            Some(Value::Object(o)) => {
                rules.attributes = Some(AttributeRule {
                    skip_names_with: o
                        .get("skip_names_with")
                        .map(|v| text_of(v, "attribute_references.skip_names_with"))
                        .transpose()?,
                })
            }
            Some(_) => {
                return Err(invalid(
                    "attribute_references is not true, false or an object",
                ));
            }
        }
        if let Some(c) = top.get("contexts") {
            let c = c
                .as_object()
                .ok_or_else(|| invalid("contexts is not an object"))?;
            for (name, def) in c {
                let at = format!("contexts.{name}");
                let o = def
                    .as_object()
                    .ok_or_else(|| invalid(format!("{at} is not an object")))?;
                let context = match (o.get("path"), o.get("nearest")) {
                    (Some(p), None) => {
                        let p = text_of(p, &format!("{at}.path"))?;
                        if !p.starts_with('/') {
                            return Err(invalid(format!("{at}.path must start with /")));
                        }
                        Context::Path(p)
                    }
                    (None, Some(n)) => Context::Nearest {
                        name: text_of(n, &format!("{at}.nearest"))?,
                        up_to: field(o, "up_to", &at)?,
                    },
                    _ => return Err(invalid(format!("{at} needs either path or nearest"))),
                };
                rules.contexts.push((name.clone(), context));
            }
        }
        for (i, r) in list(top.get("runtime_contexts"), "runtime_contexts")?
            .iter()
            .enumerate()
        {
            rules
                .runtime
                .push(text_of(r, &format!("runtime_contexts[{i}]"))?);
        }
        if let Some(vars) = top.get("variables") {
            let vars = vars
                .as_object()
                .ok_or_else(|| invalid("variables is not an object"))?;
            for (name, kind) in vars {
                let at = format!("variables.{name}");
                if !name.starts_with('$') {
                    return Err(invalid(format!("{at}: a variable starts with $")));
                }
                let kind = match text_of(kind, &at)?.as_str() {
                    "parent_name" => Variable::ParentName,
                    other => return Err(invalid(format!("{at}: unknown kind {other:?}"))),
                };
                rules.variables.push((name.clone(), kind));
            }
        }
        Ok(rules)
    }

    /// Reads a rules file.
    pub fn read(path: &Path) -> Result<Self, RulesError> {
        let text = std::fs::read_to_string(path).map_err(|error| RulesError::Read {
            path: path.to_owned(),
            error,
        })?;
        Self::parse(&text)
    }

    /// The rules file for a document: [`RULES_FILE`] next to it or in the nearest folder
    /// above it.
    pub fn find(document: &Path) -> Option<PathBuf> {
        let start = document.parent()?;
        start
            .ancestors()
            .map(|dir| dir.join(RULES_FILE))
            .find(|p| p.is_file())
    }

    /// Whether the rules name any reference at all.
    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.attributes.is_none()
    }
}

/// Every reference inside `within` (its own tags included), in document order.
pub fn references(doc: &Document, rules: &Rules, within: NodeId) -> Vec<Reference> {
    let idx = &doc.index;
    let mut out = Vec::new();
    if rules.is_empty() {
        return out;
    }
    let last = within as usize + idx.node(within).descendants as usize;
    for id in within as usize..=last.min(idx.len().saturating_sub(1)) {
        let id = id as NodeId;
        if idx.node(id).kind != NodeKind::Element {
            continue;
        }
        if let Some(r) = include_at(doc, rules, id) {
            out.push(r);
        } else if rules.attributes.is_some() {
            attribute_references(doc, rules, id, &mut out);
        }
    }
    out
}

/// The reference element `id` is, if the rules make it one.
pub fn include_at(doc: &Document, rules: &Rules, id: NodeId) -> Option<Reference> {
    let idx = &doc.index;
    let name = idx.name(&doc.src, id);
    let inc = rules
        .includes
        .iter()
        .find(|i| i.element.as_bytes() == name)?;
    let expr = idx
        .attributes(&doc.src, id)
        .into_iter()
        .find(|(k, _)| *k == inc.attribute)
        .map(|(_, v)| v)?;
    Some(Reference {
        site: id,
        kind: RefKind::Include,
        expr,
        span: idx.node(id).span(),
    })
}

/// Resolves `r`. `anchor` is where it is used: the reference itself, or for one inside
/// imported elements, the reference that imported them, which a `nearest` context is
/// looked for around.
pub fn resolve(doc: &Document, rules: &Rules, r: &Reference, anchor: NodeId) -> Resolved {
    let idx = &doc.index;
    if r.kind == RefKind::Include
        && let Some(inc) = include_rule(doc, rules, r.site)
    {
        let attrs = idx.attributes(&doc.src, r.site);
        let skip = inc.skip_when.iter().any(|(k, v)| {
            attrs
                .iter()
                .any(|(n, val)| n == k && val.eq_ignore_ascii_case(v))
        });
        if skip {
            return Resolved {
                resolution: Resolution::Skipped,
                via: None,
            };
        }
        // `[Context:path]` reads the same as `Context:path` here.
        let expr = bracketed(&r.expr).unwrap_or(&r.expr);
        let first = resolve_expr(doc, rules, r.site, expr, anchor);
        if let (Resolution::Unresolved(_), Some(fallback)) = (&first.resolution, &inc.fallback)
            && let Some((_, expr)) = attrs.iter().find(|(k, _)| k == fallback)
        {
            let expr = bracketed(expr).unwrap_or(expr);
            let second = resolve_expr(doc, rules, r.site, expr, anchor);
            if !matches!(second.resolution, Resolution::Unresolved(_)) {
                return second;
            }
        }
        return first;
    }
    let expr = match bracketed(&r.expr) {
        Some(inner) => inner,
        None => r.expr.as_str(),
    };
    if let Some(value) = whole_variable(doc, rules, r.site, expr) {
        return Resolved {
            resolution: Resolution::Value(value),
            via: None,
        };
    }
    // `${Context:path}` as the whole value: the text found there.
    if let Some(inner) = expr.strip_prefix("${").and_then(|e| e.strip_suffix('}')) {
        let mut resolved = resolve_expr(doc, rules, r.site, inner, anchor);
        if let Resolution::Nodes(nodes) = &resolved.resolution {
            resolved.resolution =
                Resolution::Value(doc.index.element_text(&doc.src, nodes[0]).into_owned());
        }
        return resolved;
    }
    let mut resolved = resolve_expr(doc, rules, r.site, expr, anchor);
    // An attribute takes the text of the elements found.
    if let Resolution::Nodes(nodes) = &resolved.resolution {
        let text: String = nodes
            .iter()
            .map(|&n| idx.element_text(&doc.src, n))
            .collect();
        resolved.resolution = Resolution::Value(text);
    }
    resolved
}

/// The references that find nothing, as warnings (REF-4). References that resolve only
/// when the file is read by its program, or only where they are imported, are not listed.
pub fn problems(doc: &Document, rules: &Rules) -> Vec<Problem> {
    let Some(root) = doc.index.root_element() else {
        return Vec::new();
    };
    references(doc, rules, root)
        .iter()
        .filter_map(|r| match resolve(doc, rules, r, r.site).resolution {
            Resolution::Unresolved(why) => Some(Problem {
                severity: Severity::Warning,
                offset: r.span.start as u64,
                message: format!("Reference {:?} {why}", r.expr),
            }),
            _ => None,
        })
        .collect()
}

/// Element `id` as text with its references resolved (REF-5): an import is replaced by the
/// elements it refers to, re-indented to where it stands, or by the value; an attribute
/// reference by its value. Imports inside imported elements are resolved too, up to
/// [`MAX_DEPTH`]; a reference that loops is left as it is. What does not resolve stays and
/// is followed by a comment saying why. Stops after about `limit` bytes.
pub fn preview(doc: &Document, rules: &Rules, id: NodeId, limit: usize) -> Preview {
    let mut p = Previewer {
        doc,
        rules,
        newline: std::str::from_utf8(newline_style(&doc.src)).unwrap_or("\n"),
        limit,
        stack: Vec::new(),
        out: Preview::default(),
    };
    // Lines lose the element's own indentation, as its first line has none.
    let strip = source_indent(&doc.src, doc.index.node(id).start as usize);
    p.element(id, None, strip, "", 0);
    p.out
}

struct Previewer<'a> {
    doc: &'a Document,
    rules: &'a Rules,
    newline: &'static str,
    limit: usize,
    /// Elements being imported, outermost first: one met again is a loop.
    stack: Vec<NodeId>,
    out: Preview,
}

impl Previewer<'_> {
    /// Writes element `id`. Each line after its first loses `strip` and gains `indent`.
    /// `anchor` is the import it came in through, if any.
    fn element(
        &mut self,
        id: NodeId,
        anchor: Option<NodeId>,
        strip: &[u8],
        indent: &str,
        depth: usize,
    ) {
        let doc = self.doc;
        let span = doc.index.node(id).span();
        let refs = references(doc, self.rules, id);
        let mut at = span.start;
        for r in refs {
            if self.out.cut {
                return;
            }
            // An include inside an include's span (its own attributes) is not separate.
            if r.span.start < at {
                continue;
            }
            self.copy(at..r.span.start, strip, indent);
            self.reference(&r, anchor.unwrap_or(r.site), depth);
            at = r.span.end;
        }
        self.copy(at..span.end, strip, indent);
    }

    fn reference(&mut self, r: &Reference, anchor: NodeId, depth: usize) {
        let doc = self.doc;
        self.out.references += 1;
        let resolved = resolve(doc, self.rules, r, anchor);
        let original = &doc.src[r.span.clone()];
        match (r.kind, resolved.resolution) {
            (RefKind::Include, Resolution::Nodes(nodes)) => {
                if depth >= MAX_DEPTH {
                    return self
                        .unresolved_include(original, &format!("deeper than {MAX_DEPTH} imports"));
                }
                if let Some(&looped) = nodes.iter().find(|n| self.stack.contains(n)) {
                    let path = doc.index.path(&doc.src, looped);
                    return self
                        .unresolved_include(original, &format!("imports {path} again, a loop"));
                }
                let indent = self.line_indent();
                let start = self.out.text.len();
                for (i, &n) in nodes.iter().enumerate() {
                    if i > 0 {
                        self.push(self.newline);
                        self.push(&indent);
                    }
                    let strip = source_indent(&doc.src, doc.index.node(n).start as usize);
                    self.stack.push(n);
                    self.element(n, Some(anchor), strip, &indent, depth + 1);
                    self.stack.pop();
                    if self.out.cut {
                        break;
                    }
                }
                let end = self.out.text.len();
                self.out.imported.push(start..end);
                if let Some(via) = resolved.via {
                    self.note(&format!(
                        " <!-- {NOTE} {} from {} -->",
                        r.expr,
                        doc.index.path(&doc.src, via)
                    ));
                }
            }
            (RefKind::Include, Resolution::Value(v)) => {
                let start = self.out.text.len();
                self.push(&escape(&v, false));
                let end = self.out.text.len();
                self.out.imported.push(start..end);
            }
            (RefKind::Include, Resolution::Skipped) => self.push_bytes(original),
            (RefKind::Include, Resolution::Runtime(why) | Resolution::Elsewhere(why)) => {
                self.push_bytes(original);
                self.note(&format!(" <!-- {NOTE} {why} -->"));
            }
            (RefKind::Include, Resolution::Unresolved(why)) => {
                self.unresolved_include(original, &why)
            }
            (RefKind::Attribute(_), Resolution::Value(v)) => {
                let start = self.out.text.len();
                self.push(&escape(&v, true));
                let end = self.out.text.len();
                self.out.imported.push(start..end);
            }
            (RefKind::Attribute(_), resolution) => {
                if matches!(resolution, Resolution::Unresolved(_)) {
                    self.out.unresolved += 1;
                }
                let start = self.out.text.len();
                self.push_bytes(original);
                let end = self.out.text.len();
                self.out.notes.push(start..end);
            }
        }
    }

    fn unresolved_include(&mut self, original: &[u8], why: &str) {
        self.out.unresolved += 1;
        self.push_bytes(original);
        self.note(&format!(" <!-- {NOTE} {why} -->"));
    }

    fn note(&mut self, text: &str) {
        let start = self.out.text.len();
        self.push(text);
        let end = self.out.text.len();
        self.out.notes.push(start..end);
    }

    /// Copies source bytes; after each line break, `strip` is taken off the line's start
    /// when it is there, and `indent` put in its place.
    fn copy(&mut self, range: Range<usize>, strip: &[u8], indent: &str) {
        let bytes = &self.doc.src[range];
        if strip.is_empty() && indent.is_empty() {
            return self.push_bytes(bytes);
        }
        let mut lines = bytes.split_inclusive(|&b| b == b'\n');
        if let Some(first) = lines.next() {
            self.push_bytes(first);
        }
        for line in lines {
            self.push(indent);
            self.push_bytes(line.strip_prefix(strip).unwrap_or(line));
        }
    }

    /// The white space the current output line starts with.
    fn line_indent(&self) -> String {
        let text = &self.out.text;
        let line = &text[text.rfind('\n').map_or(0, |i| i + 1)..];
        line[..line.len() - line.trim_start_matches([' ', '\t']).len()].to_string()
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        self.push(&String::from_utf8_lossy(bytes));
    }

    fn push(&mut self, s: &str) {
        if self.out.cut {
            return;
        }
        self.out.text.push_str(s);
        if self.out.text.len() >= self.limit {
            self.out.cut = true;
            let _ = write!(
                self.out.text,
                "{}<!-- {NOTE} cut after {} bytes -->",
                self.newline, self.limit
            );
        }
    }
}

// Private helpers

fn include_rule<'r>(doc: &Document, rules: &'r Rules, id: NodeId) -> Option<&'r Include> {
    let name = doc.index.name(&doc.src, id);
    rules.includes.iter().find(|i| i.element.as_bytes() == name)
}

/// Pushes the attribute references of element `id` onto `out`.
fn attribute_references(doc: &Document, rules: &Rules, id: NodeId, out: &mut Vec<Reference>) {
    let Some(rule) = &rules.attributes else {
        return;
    };
    let idx = &doc.index;
    let head = idx.node(id);
    let head = &doc.src[head.start as usize..head.start as usize + head.head_len as usize];
    // Quick check before decoding anything: most elements have no reference.
    if !head.contains(&b'[') && !head.contains(&b'$') {
        return;
    }
    let attrs = idx.attributes(&doc.src, id);
    let spans = idx.attribute_spans(&doc.src, id);
    for (i, ((name, value), span)) in attrs.into_iter().zip(spans).enumerate() {
        if rule
            .skip_names_with
            .as_ref()
            .is_some_and(|s| name.contains(s.as_str()))
        {
            continue;
        }
        let is_ref = bracketed(&value).is_some()
            || (value.starts_with("${") && value.ends_with('}'))
            || rules.variables.iter().any(|(v, _)| *v == value);
        if !is_ref {
            continue;
        }
        let Some(span) = value_span(&doc.src, span) else {
            continue;
        };
        out.push(Reference {
            site: id,
            kind: RefKind::Attribute(i),
            expr: value,
            span,
        });
    }
}

/// The value of an attribute between its quotes, from the span of the whole attribute.
fn value_span(src: &[u8], attr: Range<usize>) -> Option<Range<usize>> {
    let a = &src[attr.clone()];
    let q = a.iter().position(|&b| b == b'"' || b == b'\'')?;
    let end = a.iter().rposition(|&b| b == a[q])?;
    (end > q).then(|| attr.start + q + 1..attr.start + end)
}

/// `inner` of a value written `[inner]`.
fn bracketed(value: &str) -> Option<&str> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    (!inner.is_empty()).then_some(inner)
}

/// A value that is only a variable, such as `$CURRENT_PARENT`.
fn whole_variable(doc: &Document, rules: &Rules, site: NodeId, expr: &str) -> Option<String> {
    let (_, kind) = rules.variables.iter().find(|(v, _)| v == expr)?;
    Some(variable(doc, site, *kind))
}

fn variable(doc: &Document, site: NodeId, kind: Variable) -> String {
    match kind {
        Variable::ParentName => {
            let parent = doc.index.node(site).parent;
            String::from_utf8_lossy(doc.index.name(&doc.src, parent)).into_owned()
        }
    }
}

/// Resolves `Context:path`, or a path without a context from `site`.
fn resolve_expr(
    doc: &Document,
    rules: &Rules,
    site: NodeId,
    expr: &str,
    anchor: NodeId,
) -> Resolved {
    let unresolved = |why: String| Resolved {
        resolution: Resolution::Unresolved(why),
        via: None,
    };
    let (context, path) = split_context(expr);
    // `${Context:path}` inside the path: the text found there.
    let path = match expand_dynamic(doc, rules, site, path, anchor) {
        Ok(p) => p,
        Err(r) => {
            return Resolved {
                resolution: r,
                via: None,
            };
        }
    };
    let path = match expand_variables(doc, rules, site, &path) {
        Ok(p) => p,
        Err(r) => {
            return Resolved {
                resolution: r,
                via: None,
            };
        }
    };
    let candidates = match context_nodes(doc, rules, context, site, anchor) {
        Ok(c) => c,
        Err(r) => {
            return Resolved {
                resolution: r,
                via: None,
            };
        }
    };
    let mut last_error = None;
    for (from, via) in candidates {
        match select(doc, from, &path) {
            Ok(Selection::Nodes(n)) if !n.is_empty() => {
                return Resolved {
                    resolution: Resolution::Nodes(n),
                    via,
                };
            }
            Ok(Selection::Value(v)) if !v.is_empty() => {
                return Resolved {
                    resolution: Resolution::Value(v),
                    via,
                };
            }
            Ok(_) => {}
            Err(e) => last_error = Some(e),
        }
    }
    unresolved(last_error.unwrap_or_else(|| "finds nothing".to_string()))
}

/// `(Some("Ctx"), "path")` for `Ctx:path`, else `(None, expr)`. The context ends at the
/// first colon.
fn split_context(expr: &str) -> (Option<&str>, &str) {
    match expr.find(':') {
        Some(i) if i > 0 && i + 1 < expr.len() && !expr[..i].contains(['/', '[', '$', '@']) => {
            (Some(&expr[..i]), &expr[i + 1..])
        }
        _ => (None, expr),
    }
}

/// Where a path is evaluated from, with the element a `nearest` context was taken from.
fn context_nodes(
    doc: &Document,
    rules: &Rules,
    context: Option<&str>,
    site: NodeId,
    anchor: NodeId,
) -> Result<Vec<(NodeId, Option<NodeId>)>, Resolution> {
    let Some(name) = context else {
        return Ok(vec![(site, None)]);
    };
    if rules.runtime.iter().any(|r| r == name) {
        return Err(Resolution::Runtime(format!(
            "{name} is known only at run time"
        )));
    }
    let Some((_, ctx)) = rules.contexts.iter().find(|(n, _)| n == name) else {
        return Err(Resolution::Unresolved(format!(
            "uses the context {name}, which the rules file does not name"
        )));
    };
    match ctx {
        Context::Path(p) => match select(doc, ROOT, p) {
            Ok(Selection::Nodes(n)) if !n.is_empty() => Ok(vec![(n[0], None)]),
            _ => Err(Resolution::Unresolved(format!(
                "uses the context {name}, but nothing is at {p}"
            ))),
        },
        Context::Nearest {
            name: wanted,
            up_to,
        } => {
            let c = nearest(doc, anchor, wanted, up_to);
            if c.is_empty() {
                return Err(Resolution::Elsewhere(format!(
                    "{name} is looked for inside a <{up_to}>: resolved where this is imported"
                )));
            }
            Ok(c.into_iter().map(|n| (n, Some(n))).collect())
        }
    }
}

/// Elements called `wanted` around `from`: children of each ancestor, nearest first, up to
/// the ancestor called `up_to`, then that one's other descendants called `wanted`.
fn nearest(doc: &Document, from: NodeId, wanted: &str, up_to: &str) -> Vec<NodeId> {
    let idx = &doc.index;
    let named = |id: NodeId, n: &str| {
        idx.node(id).kind == NodeKind::Element && idx.name(&doc.src, id) == n.as_bytes()
    };
    let mut out = Vec::new();
    let mut cur = idx.node(from).parent;
    while cur != ROOT && cur != xml_atlas_xml::NONE {
        for c in idx.children(cur) {
            if named(c, wanted) && !out.contains(&c) {
                out.push(c);
            }
        }
        if named(cur, up_to) {
            let last = cur as usize + idx.node(cur).descendants as usize;
            for d in cur as usize + 1..=last {
                let d = d as NodeId;
                if named(d, wanted) && !out.contains(&d) {
                    out.push(d);
                }
            }
            return out;
        }
        cur = idx.node(cur).parent;
    }
    Vec::new()
}

/// Replaces `${Context:path}` in `path` with the text found there.
fn expand_dynamic(
    doc: &Document,
    rules: &Rules,
    site: NodeId,
    path: &str,
    anchor: NodeId,
) -> Result<String, Resolution> {
    let Some(start) = path.find("${") else {
        return Ok(path.to_string());
    };
    let Some(len) = path[start..].find('}') else {
        return Err(Resolution::Unresolved("has ${ without }".into()));
    };
    let inner = &path[start + 2..start + len];
    let found = resolve_expr(doc, rules, site, inner, anchor);
    let text = match found.resolution {
        Resolution::Nodes(n) => doc.index.element_text(&doc.src, n[0]).into_owned(),
        Resolution::Value(v) => v,
        other => return Err(other),
    };
    let rest = expand_dynamic(doc, rules, site, &path[start + len + 1..], anchor)?;
    Ok(format!("{}{}{}", &path[..start], text, rest))
}

/// Replaces the variables the rules name; a `$NAME` left over is known only at run time.
fn expand_variables(
    doc: &Document,
    rules: &Rules,
    site: NodeId,
    path: &str,
) -> Result<String, Resolution> {
    let mut out = path.to_string();
    for (name, kind) in &rules.variables {
        if out.contains(name.as_str()) {
            out = out.replace(name.as_str(), &variable(doc, site, *kind));
        }
    }
    if let Some(i) = out.find('$') {
        let name: String = out[i..]
            .chars()
            .take_while(|&c| c == '$' || c.is_ascii_alphanumeric() || c == '_')
            .collect();
        return Err(Resolution::Runtime(format!(
            "{name} is known only at run time"
        )));
    }
    Ok(out)
}

/// What a path selects.
#[derive(Debug, PartialEq, Eq)]
enum Selection {
    Nodes(Vec<NodeId>),
    Value(String),
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// `/` at the start.
    Root,
    /// `//`: the elements and all their descendant elements.
    Descendants,
    SelfNode,
    Parent,
    /// A child element by name, or any (`*`), with predicates.
    Child(Option<String>, Vec<Predicate>),
    Attribute(Option<String>),
    Text,
}

#[derive(Debug, PartialEq, Eq)]
enum Predicate {
    Has(String),
    Equals(String, String),
    Position(usize),
}

/// Evaluates `path` from element `from` (or the document node).
fn select(doc: &Document, from: NodeId, path: &str) -> Result<Selection, String> {
    let steps = parse_path(path)?;
    let idx = &doc.index;
    let mut set = vec![from];
    let last = steps.len().saturating_sub(1);
    for (i, step) in steps.iter().enumerate() {
        match step {
            Step::Root => set = vec![ROOT],
            Step::SelfNode => {}
            Step::Parent => {
                set = set
                    .iter()
                    .map(|&n| idx.node(n).parent)
                    .filter(|&p| p != xml_atlas_xml::NONE)
                    .collect();
            }
            Step::Descendants => {
                let mut all = Vec::new();
                for &n in &set {
                    all.push(n);
                    let end = n as usize + idx.node(n).descendants as usize;
                    all.extend(
                        (n as usize + 1..=end)
                            .map(|d| d as NodeId)
                            .filter(|&d| idx.node(d).kind == NodeKind::Element),
                    );
                }
                set = all;
            }
            Step::Child(name, preds) => {
                let mut next = Vec::new();
                for &n in &set {
                    let start = next.len();
                    next.extend(idx.children(n).filter(|&c| {
                        idx.node(c).kind == NodeKind::Element
                            && name
                                .as_ref()
                                .is_none_or(|w| idx.name(&doc.src, c) == w.as_bytes())
                    }));
                    for p in preds {
                        let mut k = 0;
                        let kept: Vec<NodeId> = next[start..]
                            .iter()
                            .copied()
                            .filter(|&c| {
                                k += 1;
                                predicate(doc, c, p, k)
                            })
                            .collect();
                        next.truncate(start);
                        next.extend(kept);
                    }
                }
                set = next;
            }
            Step::Attribute(name) if i == last => {
                let mut value = String::new();
                for &n in &set {
                    for (k, v) in idx.attributes(&doc.src, n) {
                        if name.as_ref().is_none_or(|w| *w == k) {
                            value.push_str(&v);
                        }
                    }
                }
                return Ok(Selection::Value(value));
            }
            Step::Text if i == last => {
                let value: String = set.iter().map(|&n| idx.element_text(&doc.src, n)).collect();
                return Ok(Selection::Value(value));
            }
            Step::Attribute(_) | Step::Text => {
                return Err(
                    "has @ or text() before its last step, which the preview cannot follow".into(),
                );
            }
        }
        set.sort_unstable();
        set.dedup();
    }
    set.retain(|&n| n != ROOT);
    Ok(Selection::Nodes(set))
}

fn predicate(doc: &Document, id: NodeId, p: &Predicate, position: usize) -> bool {
    match p {
        Predicate::Position(n) => position == *n,
        Predicate::Has(a) => doc
            .index
            .attributes(&doc.src, id)
            .iter()
            .any(|(k, _)| k == a),
        Predicate::Equals(a, v) => doc
            .index
            .attributes(&doc.src, id)
            .iter()
            .any(|(k, val)| k == a && val == v),
    }
}

fn parse_path(path: &str) -> Result<Vec<Step>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("has an empty path".into());
    }
    let mut steps = Vec::new();
    let mut rest = path;
    if let Some(r) = rest.strip_prefix('/') {
        steps.push(Step::Root);
        rest = r;
        if let Some(r) = rest.strip_prefix('/') {
            steps.push(Step::Descendants);
            rest = r;
        }
    }
    let parts = split_steps(rest)?;
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        if part.is_empty() {
            // `a//b`: the empty step between the slashes.
            if i == 0 || i + 1 == parts.len() {
                return Err(format!("has a path {path:?} the preview cannot read"));
            }
            steps.push(Step::Descendants);
            continue;
        }
        steps.push(parse_step(part).map_err(|e| format!("has {e} in {path:?}"))?);
    }
    Ok(steps)
}

/// Splits at `/` outside brackets and quotes.
fn split_steps(path: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0usize, None, 0);
    for (i, c) in path.char_indices() {
        match (c, quote) {
            ('\'' | '"', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            (_, Some(_)) => {}
            ('[', None) => depth += 1,
            (']', None) => depth = depth.checked_sub(1).ok_or("a ] without [")?,
            ('/', None) if depth == 0 => {
                out.push(&path[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || quote.is_some() {
        return Err(format!("has an unclosed [ or quote in {path:?}"));
    }
    out.push(&path[start..]);
    Ok(out)
}

fn parse_step(s: &str) -> Result<Step, String> {
    match s {
        "." => return Ok(Step::SelfNode),
        ".." => return Ok(Step::Parent),
        "text()" => return Ok(Step::Text),
        "@*" => return Ok(Step::Attribute(None)),
        _ => {}
    }
    if let Some(name) = s.strip_prefix('@') {
        return if is_name(name) {
            Ok(Step::Attribute(Some(name.to_string())))
        } else {
            Err(format!("the attribute step {s:?}"))
        };
    }
    let (name, mut rest) = match s.find('[') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    let name = match name.trim() {
        "*" => None,
        n if is_name(n) => Some(n.to_string()),
        _ => return Err(format!("the step {s:?}")),
    };
    let mut preds = Vec::new();
    while let Some(r) = rest.strip_prefix('[') {
        let end = r.find(']').ok_or_else(|| format!("the step {s:?}"))?;
        preds.push(
            parse_predicate(r[..end].trim())
                .ok_or_else(|| format!("the condition [{}]", &r[..end]))?,
        );
        rest = r[end + 1..].trim_start();
    }
    if !rest.is_empty() {
        return Err(format!("the step {s:?}"));
    }
    Ok(Step::Child(name, preds))
}

fn parse_predicate(p: &str) -> Option<Predicate> {
    if let Ok(n) = p.parse::<usize>() {
        return (n > 0).then_some(Predicate::Position(n));
    }
    let a = p.strip_prefix('@')?;
    match a.split_once('=') {
        None => is_name(a.trim()).then(|| Predicate::Has(a.trim().to_string())),
        Some((name, value)) => {
            let name = name.trim();
            let value = value.trim();
            let q = value.chars().next()?;
            let inner = value.strip_prefix(['\'', '"'])?.strip_suffix(q)?;
            is_name(name).then(|| Predicate::Equals(name.to_string(), inner.to_string()))
        }
    }
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

/// The white space before byte `at` on its line, if only white space is there.
fn source_indent(src: &[u8], at: usize) -> &[u8] {
    let line_start = src[..at]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let before = &src[line_start..at];
    if before.iter().all(|&b| b == b' ' || b == b'\t') {
        before
    } else {
        &[]
    }
}

fn escape(s: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn invalid(msg: impl Into<String>) -> RulesError {
    RulesError::Invalid(msg.into())
}

fn list<'v>(v: Option<&'v Value>, at: &str) -> Result<&'v [Value], RulesError> {
    match v {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(invalid(format!("{at} is not a list"))),
    }
}

fn field(o: &serde_json::Map<String, Value>, key: &str, at: &str) -> Result<String, RulesError> {
    let v = o
        .get(key)
        .ok_or_else(|| invalid(format!("{at} needs {key}")))?;
    text_of(v, &format!("{at}.{key}"))
}

fn text_of(v: &Value, at: &str) -> Result<String, RulesError> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| invalid(format!("{at} is not a string")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"{
        "includes": [{"element": "Include", "attribute": "src", "fallback": "default-src",
                      "skip_when": {"Later": "true"}}],
        "attribute_references": {"skip_names_with": "-Later"},
        "contexts": {
            "Shared": {"path": "/Doc/Shared"},
            "Var": {"nearest": "Var", "up_to": "Unit"}
        },
        "runtime_contexts": ["Live"],
        "variables": {"$PARENT": "parent_name"}
    }"#;

    /// A byte order mark, CRLF and non-ASCII names, as the owner's files have.
    fn doc() -> Document {
        let src = "\u{feff}<Doc>\r\n\
                   \x20 <Shared>\r\n\
                   \x20   <Größe><Wert>5 °C</Wert></Größe>\r\n\
                   \x20   <Parts Kind=\"a\">\r\n\
                   \x20     <P1/>\r\n\
                   \x20     <P2/>\r\n\
                   \x20   </Parts>\r\n\
                   \x20   <Parts Kind=\"b\"><Q/></Parts>\r\n\
                   \x20   <Loop><Include src=\"Shared:Loop\"/></Loop>\r\n\
                   \x20   <Uses><Include src=\"Var:Ch/Count\"/></Uses>\r\n\
                   \x20 </Shared>\r\n\
                   \x20 <Unit>\r\n\
                   \x20   <General>\r\n\
                   \x20     <Var><Ch><Count>8</Count></Ch></Var>\r\n\
                   \x20     <Box N=\"[Var:Ch/Count]\" Where=\"$PARENT\" M-Later=\"[Var:x]\">\r\n\
                   \x20       <Include src=\"Shared:Parts[@Kind='a']/*\"/>\r\n\
                   \x20       <Include src=\"Shared:Uses/*\"/>\r\n\
                   \x20       <Include src=\"Shared:Größe/Wert/text()\"/>\r\n\
                   \x20       <Include src=\"Shared:Missing\" default-src=\"Shared:Parts[2]/Q\"/>\r\n\
                   \x20       <Include src=\"Shared:Missing\"/>\r\n\
                   \x20       <Include src=\"Live:Name\"/>\r\n\
                   \x20       <Include src=\"Var:Only/In/Sub\"/>\r\n\
                   \x20       <Include src=\"Shared:Loop/*\"/>\r\n\
                   \x20       <Include src=\"Shared:Parts\" Later=\"TRUE\"/>\r\n\
                   \x20     </Box>\r\n\
                   \x20   </General>\r\n\
                   \x20   <Sub><Var><Only><In><Sub>s</Sub></In></Only></Var></Sub>\r\n\
                   \x20 </Unit>\r\n\
                   </Doc>\r\n";
        Document::from_bytes(src.as_bytes().to_vec())
    }

    fn rules() -> Rules {
        Rules::parse(RULES).unwrap()
    }

    fn element(doc: &Document, path: &str) -> NodeId {
        crate::command::resolve(doc, path).unwrap()
    }

    fn resolution(doc: &Document, expr: &str) -> Resolution {
        let rules = rules();
        let r = references(doc, &rules, doc.index.root_element().unwrap())
            .into_iter()
            .find(|r| r.expr == expr)
            .unwrap_or_else(|| panic!("no reference {expr}"));
        resolve(doc, &rules, &r, r.site).resolution
    }

    fn names(doc: &Document, r: Resolution) -> Vec<String> {
        match r {
            Resolution::Nodes(n) => n.iter().map(|&n| doc.index.path(&doc.src, n)).collect(),
            other => panic!("not nodes: {other:?}"),
        }
    }

    #[test]
    fn rules_are_checked() {
        assert!(rules().includes.len() == 1 && !rules().is_empty());
        for bad in [
            "[]",
            r#"{"includes": [{"element": "I"}]}"#,
            r#"{"contexts": {"C": {"path": "no/slash"}}}"#,
            r#"{"contexts": {"C": {}}}"#,
            r#"{"variables": {"X": "parent_name"}}"#,
            r#"{"variables": {"$X": "colour"}}"#,
            "{",
        ] {
            assert!(Rules::parse(bad).is_err(), "{bad}");
        }
        assert!(Rules::parse("{}").unwrap().is_empty());
    }

    #[test]
    fn finds_includes_and_attribute_references_in_order() {
        let d = doc();
        let rules = rules();
        let all = references(&d, &rules, d.index.root_element().unwrap());
        let exprs: Vec<&str> = all.iter().map(|r| r.expr.as_str()).collect();
        assert_eq!(
            &exprs[..4],
            ["Shared:Loop", "Var:Ch/Count", "[Var:Ch/Count]", "$PARENT"]
        );
        // `M-Later` is skipped by name; 2 in Shared, 2 attributes, 9 in Box.
        assert_eq!(all.len(), 13);
        // An attribute's span is its value between the quotes.
        let n = &all[2];
        assert_eq!(&d.src[n.span.clone()], b"[Var:Ch/Count]");
    }

    #[test]
    fn resolves_paths_values_and_contexts() {
        let d = doc();
        assert_eq!(
            names(&d, resolution(&d, "Shared:Parts[@Kind='a']/*")),
            ["/Doc/Shared/Parts[1]/P1", "/Doc/Shared/Parts[1]/P2"]
        );
        assert_eq!(
            resolution(&d, "Shared:Größe/Wert/text()"),
            Resolution::Value("5 °C".into())
        );
        assert_eq!(
            resolution(&d, "[Var:Ch/Count]"),
            Resolution::Value("8".into())
        );
        assert_eq!(
            resolution(&d, "$PARENT"),
            Resolution::Value("General".into())
        );
        // The nearest Var is under General; one only under Sub is found there next.
        assert_eq!(
            names(&d, resolution(&d, "Var:Only/In/Sub")),
            ["/Doc/Unit/Sub/Var/Only/In/Sub"]
        );
        // The fallback when the first finds nothing.
        assert_eq!(
            names(&d, resolution(&d, "Shared:Missing")),
            ["/Doc/Shared/Parts[2]/Q"],
            "the first Shared:Missing has a default-src"
        );
        assert!(matches!(
            resolution(&d, "Live:Name"),
            Resolution::Runtime(_)
        ));
        assert_eq!(resolution(&d, "Shared:Parts"), Resolution::Skipped);
        // Outside a Unit, Var has nothing around it.
        assert!(matches!(
            resolution(&d, "Var:Ch/Count"),
            Resolution::Elsewhere(_)
        ));
    }

    #[test]
    fn select_reads_the_path_subset() {
        let d = doc();
        let shared = element(&d, "/Doc/Shared");
        let nodes = |p: &str| match select(&d, shared, p).unwrap() {
            Selection::Nodes(n) => n.len(),
            Selection::Value(v) => panic!("value {v}"),
        };
        assert_eq!(nodes("Parts"), 2);
        assert_eq!(nodes("Parts[2]/*"), 1);
        assert_eq!(nodes("Parts[@Kind]"), 2);
        assert_eq!(nodes("Parts[@Kind=\"b\"]/Q"), 1);
        assert_eq!(nodes("./Parts/../Parts"), 2);
        assert_eq!(nodes("/Doc/Unit"), 1);
        // `//` at the start is the whole document; `.//` below here.
        assert_eq!(nodes("//Include"), 11);
        assert_eq!(nodes(".//Include"), 2);
        assert_eq!(nodes("Parts//P2"), 1);
        assert_eq!(
            select(&d, shared, "Parts/@Kind").unwrap(),
            Selection::Value("ab".into())
        );
        for bad in [
            "",
            "Parts/@Kind/x",
            "Parts[name()='x']",
            "Parts[",
            "a b",
            "Parts/text()/x",
        ] {
            assert!(select(&d, shared, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn problems_list_only_what_finds_nothing() {
        let d = doc();
        let p = problems(&d, &rules());
        let messages: Vec<&str> = p.iter().map(|p| p.message.as_str()).collect();
        // The second Shared:Missing has no fallback; run time, skipped and the Var in
        // Shared (resolved where it is imported) are not problems.
        assert_eq!(messages, ["Reference \"Shared:Missing\" finds nothing"]);
        assert_eq!(p[0].severity, Severity::Warning);
        let line = d.line_of(p[0].offset);
        assert_eq!(line, 19);
    }

    #[test]
    fn preview_resolves_reindents_and_marks() {
        let d = doc();
        let rules = rules();
        let p = preview(
            &d,
            &rules,
            element(&d, "/Doc/Unit/General/Box"),
            PREVIEW_LIMIT,
        );
        let text = &p.text;
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "<Box N=\"8\" Where=\"General\" M-Later=\"[Var:x]\">"
        );
        // The import's lines take its indentation, and keep their own below it.
        assert_eq!(lines[1], "  <P1/>");
        assert_eq!(lines[2], "  <P2/>");
        // `Uses/*` is an import itself: it resolves where it was imported, in the Unit.
        assert!(
            lines[3].starts_with(
                "  <Count>8</Count> <!-- XML Atlas: Var:Ch/Count from /Doc/Unit/General/Var"
            ),
            "{}",
            lines[3]
        );
        assert_eq!(lines[4], "  5 °C");
        assert_eq!(lines[5], "  <Q/>");
        assert!(
            lines[6]
                .contains("<Include src=\"Shared:Missing\"/> <!-- XML Atlas: finds nothing -->")
        );
        assert!(lines[7].contains("Live is known only at run time"));
        assert!(
            lines[8].starts_with(
                "  <Sub>s</Sub> <!-- XML Atlas: Var:Only/In/Sub from /Doc/Unit/Sub/Var"
            )
        );
        assert!(
            lines[9].contains("imports /Doc/Shared/Loop again, a loop"),
            "{}",
            lines[9]
        );
        assert!(lines[10].contains("Later=\"TRUE\"/>"));
        assert_eq!(lines[11], "</Box>");
        // Box has 11; the imported `Uses` and `Loop` bring 1 and 2 more (the loop is met twice).
        assert_eq!((p.references, p.unresolved, p.cut), (14, 2, false));
        // Marked ranges lie in the text.
        assert!(
            p.imported
                .iter()
                .chain(&p.notes)
                .all(|r| r.end <= text.len())
        );
        assert!(p.imported.iter().any(|r| &text[r.clone()] == "8"));
    }

    #[test]
    fn preview_stops_at_the_limit() {
        let d = doc();
        let p = preview(&d, &rules(), d.index.root_element().unwrap(), 64);
        assert!(p.cut);
        assert!(p.text.ends_with("<!-- XML Atlas: cut after 64 bytes -->"));
    }

    #[test]
    fn rules_file_is_found_next_to_the_document_or_above() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-refs-{}", std::process::id()));
        let deep = dir.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(Rules::find(&deep.join("f.xml")), None);
        std::fs::write(dir.join(RULES_FILE), "{}").unwrap();
        assert_eq!(Rules::find(&deep.join("f.xml")), Some(dir.join(RULES_FILE)));
        std::fs::write(deep.join(RULES_FILE), "{}").unwrap();
        assert_eq!(
            Rules::find(&deep.join("f.xml")),
            Some(deep.join(RULES_FILE))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
