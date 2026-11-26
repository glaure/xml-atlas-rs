// Turns the language files into Rust (`docs/spec/language.md`, LANG-10 to LANG-14). The
// build script runs it; the tests include it too, to check what it refuses.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// A plural's forms, in the order the code takes them.
const FORMS: [&str; 2] = ["one", "other"];
/// The placeholder that holds a plural's number.
const COUNT: &str = "count";

/// The code, and what the build is to warn about.
pub struct Generated {
    /// `impl Lang`, with a method per text.
    pub code: String,
    /// The texts a language lacks (LANG-12).
    pub warnings: Vec<String>,
}

/// A text as a file gives it.
#[derive(Clone)]
enum Text {
    Single(String),
    /// Its forms, as [`FORMS`] lists them.
    Plural([String; 2]),
}

/// A language's file: its code, such as `de`, and its content.
pub struct File<'a> {
    pub code: &'a str,
    pub source: &'a str,
}

/// Reads the files, `english` first among them, and writes the code. `Err` says which file,
/// which text and why (LANG-13).
pub fn generate(english: &File<'_>, others: &[File<'_>]) -> Result<Generated, String> {
    let (en, lacking) = read(english)?;
    if let Some(name) = lacking.first() {
        return Err(format!(
            "{}.toml: `{name}` is empty or lacks a form",
            english.code
        ));
    }
    // Each English text's placeholders, which every other language has to have too.
    let mut wanted = BTreeMap::new();
    for (name, text) in &en {
        wanted.insert(name.clone(), placeholders(english.code, name, text, None)?);
    }
    let mut warnings = Vec::new();
    let mut languages = Vec::new();
    for file in others {
        let (texts, _) = read(file)?;
        for (name, text) in &texts {
            let at = || format!("{}.toml: `{name}`", file.code);
            let Some(english) = en.get(name) else {
                return Err(format!("{}: en.toml has no text of this name", at()));
            };
            if matches!(text, Text::Plural(_)) != matches!(english, Text::Plural(_)) {
                return Err(format!("{}: it is a plural in only one of the files", at()));
            }
            placeholders(file.code, name, text, Some(&wanted[name]))?;
        }
        let missing: Vec<&str> = en
            .keys()
            .filter(|name| !texts.contains_key(*name))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            warnings.push(format!(
                "{}.toml lacks {} text(s), shown in English: {}",
                file.code,
                missing.len(),
                missing.join(", ")
            ));
        }
        languages.push((variant(file.code), texts));
    }
    let mut code = String::from("impl Lang {\n");
    for (name, text) in &en {
        method(&mut code, name, text, &wanted[name], &languages);
    }
    code.push_str("}\n");
    Ok(Generated { code, warnings })
}

/// The texts of a file, and the names of those that are empty or lack a form: they count as
/// not there (LANG-12).
fn read(file: &File<'_>) -> Result<(BTreeMap<String, Text>, Vec<String>), String> {
    let table: toml::Table = file
        .source
        .parse()
        .map_err(|e| format!("{}.toml cannot be read: {e}", file.code))?;
    let mut texts = BTreeMap::new();
    let mut lacking = Vec::new();
    for (name, value) in table {
        let at = format!("{}.toml: `{name}`", file.code);
        if !is_name(&name) {
            return Err(format!(
                "{at}: a name is lower case, digits and underscores"
            ));
        }
        match value {
            toml::Value::String(s) if s.is_empty() => lacking.push(name),
            toml::Value::String(s) => {
                texts.insert(name, Text::Single(s));
            }
            toml::Value::Table(forms) => {
                if let Some(extra) = forms.keys().find(|k| !FORMS.contains(&k.as_str())) {
                    return Err(format!("{at}: en.toml has no form `{extra}`"));
                }
                let mut found = [String::new(), String::new()];
                for (slot, form) in found.iter_mut().zip(FORMS) {
                    match forms.get(form) {
                        None => {}
                        Some(toml::Value::String(s)) => slot.clone_from(s),
                        Some(_) => return Err(format!("{at}: the form `{form}` is no text")),
                    }
                }
                if found.iter().any(String::is_empty) {
                    lacking.push(name);
                } else {
                    texts.insert(name, Text::Plural(found));
                }
            }
            _ => return Err(format!("{at}: neither a text nor a plural's forms")),
        }
    }
    Ok((texts, lacking))
}

/// The placeholders of a text, `{count}` of a plural left out, as every form may do that.
/// With `wanted`, every form has to have exactly those; without, the forms have to agree.
fn placeholders(
    code: &str,
    name: &str,
    text: &Text,
    wanted: Option<&Vec<String>>,
) -> Result<Vec<String>, String> {
    let at = || format!("{code}.toml: `{name}`");
    let forms: &[String] = match text {
        Text::Single(s) => std::slice::from_ref(s),
        Text::Plural(forms) => forms,
    };
    let mut first: Option<Vec<String>> = None;
    for form in forms {
        let mut found = scan(form).map_err(|e| format!("{}: {e}", at()))?;
        if matches!(text, Text::Plural(_)) {
            found.retain(|p| p != COUNT);
        }
        // The order of first use gives the method's parameters their order.
        let set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
        if let Some(expected) = wanted.or(first.as_ref())
            && set(expected) != set(&found)
        {
            return Err(format!(
                "{}: its placeholders are {found:?}, not {expected:?}",
                at()
            ));
        }
        first.get_or_insert(found);
    }
    Ok(first.unwrap_or_default())
}

/// The names between braces, each once, in the order they come. `{{` and `}}` are braces.
fn scan(text: &str) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' if chars.peek() == Some(&c) => {
                chars.next();
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err("a `{` is not closed".into()),
                    }
                }
                if !is_name(&name) {
                    return Err(format!("`{{{name}}}` is no placeholder"));
                }
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            '}' => return Err("a `}` stands alone; write `}}` for a brace".into()),
            _ => {}
        }
    }
    Ok(names)
}

fn is_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `de` is `Lang::De`.
fn variant(code: &str) -> String {
    let mut chars = code.chars();
    let first = chars.next().map(|c| c.to_ascii_uppercase());
    first.into_iter().chain(chars).collect()
}

/// One method: a match with an arm per language that has the text, English for the rest.
fn method(
    code: &mut String,
    name: &str,
    english: &Text,
    parameters: &[String],
    languages: &[(String, BTreeMap<String, Text>)],
) {
    let plural = matches!(english, Text::Plural(_));
    let constant = !plural && parameters.is_empty();
    let _ = write!(code, "    pub fn {name}(self");
    if plural {
        let _ = write!(code, ", {COUNT}: u64");
    }
    for p in parameters {
        let _ = write!(code, ", {p}: &str");
    }
    let result = if constant { "&'static str" } else { "String" };
    let _ = writeln!(code, ") -> {result} {{\n        match self {{");
    // A text without placeholders is its literal; a doubled brace is one brace there too.
    let value = |s: &str| {
        let literal = format!("{:?}", s.replace("{{", "{").replace("}}", "}"));
        if constant {
            literal
        } else if scan(s).is_ok_and(|names| names.is_empty()) {
            // A form of a plural, or of a text whose other forms have placeholders.
            format!("{literal}.to_owned()")
        } else if let Ok(names) = scan(s)
            && let [name] = &names[..]
            && s == format!("{{{name}}}")
        {
            // The placeholder alone, such as a library's message in English (LANG-4):
            // clippy refuses a `format!` of it.
            format!("{name}.to_owned()")
        } else {
            format!("format!({s:?})")
        }
    };
    let body = |text: &Text| match text {
        Text::Single(s) => value(s),
        Text::Plural([one, other]) => format!(
            "if self.is_one({COUNT}) {{ {} }} else {{ {} }}",
            value(one),
            value(other)
        ),
    };
    let mut fallback = vec!["Lang::En".to_owned()];
    for (variant, texts) in languages {
        match texts.get(name) {
            Some(text) => {
                let _ = writeln!(code, "            Lang::{variant} => {},", body(text));
            }
            None => fallback.push(format!("Lang::{variant}")),
        }
    }
    let _ = writeln!(
        code,
        "            {} => {},\n        }}\n    }}",
        fallback.join(" | "),
        body(english)
    );
}
