//! XML Atlas's MCP tools (`docs/decisions.md`, D27): what an agent can do with XML documents,
//! as calls into the engine (D4).
//!
//! The tools work on a [`Documents`]: the documents an agent opened. `xml-atlas-cli mcp`
//! keeps them in memory ([`Files`], step 1); the window will offer its tabs (step 2). This
//! crate speaks no protocol: the server hands it a tool's name and its JSON arguments and
//! sends back the JSON it returns.
//!
//! Answers stay small, for an agent's short context: lists come in pages with a total, and
//! long texts are cut at [`MAX_TEXT`] bytes with their length given.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xml_atlas_engine::command::{self, CommandError, Direction as MoveDirection};
use xml_atlas_engine::find::{self as find_text, Direction, FindOptions, Finder};
use xml_atlas_engine::grid::{Cell, Column, Grid};
use xml_atlas_engine::problems::Severity;
use xml_atlas_engine::xml::{NodeId, NodeKind};
use xml_atlas_engine::{Document, Edit, EditError, FormatOptions};

pub mod bridge;

/// Longest text a tool returns, in bytes; longer ones are cut and their length given.
pub const MAX_TEXT: usize = 64 * 1024;

/// How many items a list holds when the call does not say.
pub const DEFAULT_LIMIT: usize = 100;

/// The most items a list holds.
pub const MAX_LIMIT: usize = 1000;

/// Characters of a matching line in `find`.
const LINE_CHARS: usize = 160;

/// A tool: its name, what it does, and its arguments as a JSON Schema object.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    /// Changes nothing: neither the documents nor any file.
    pub read_only: bool,
}

/// Why a tool call failed; the agent reads the message.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("unknown tool {0}")]
    UnknownTool(String),
    #[error("argument `{0}` is missing or has the wrong type")]
    Argument(&'static str),
    #[error("no open document {0}; `documents` lists them")]
    NoDocument(u64),
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Command(#[from] CommandError),
}

/// The documents the tools work on, each under a number.
pub trait Documents {
    /// Opens the file at `path` and returns its number.
    fn open(&mut self, path: &Path) -> std::io::Result<u64>;
    /// Closes a document. The window refuses one with unsaved changes (D28).
    fn close(&mut self, id: u64) -> Result<(), ToolError>;
    /// The open documents' numbers, in the order they were opened.
    fn ids(&self) -> Vec<u64>;
    fn get(&self, id: u64) -> Option<&Document>;
    fn get_mut(&mut self, id: u64) -> Option<&mut Document>;

    /// Runs `f`, a change, on a document; `None` when there is none of that number. The
    /// window runs it through its own edit path, so that its views follow (D28).
    fn change<R>(&mut self, id: u64, f: impl FnOnce(&mut Document) -> R) -> Option<R> {
        self.get_mut(id).map(f)
    }
}

/// Documents kept in memory for one session of `xml-atlas-cli mcp` (step 1).
#[derive(Default)]
pub struct Files {
    docs: BTreeMap<u64, Document>,
    next: u64,
}

impl Documents for Files {
    fn open(&mut self, path: &Path) -> std::io::Result<u64> {
        let doc = Document::open(path)?;
        self.next += 1;
        self.docs.insert(self.next, doc);
        Ok(self.next)
    }

    fn close(&mut self, id: u64) -> Result<(), ToolError> {
        match self.docs.remove(&id) {
            Some(_) => Ok(()),
            None => Err(ToolError::NoDocument(id)),
        }
    }

    fn ids(&self) -> Vec<u64> {
        self.docs.keys().copied().collect()
    }

    fn get(&self, id: u64) -> Option<&Document> {
        self.docs.get(&id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Document> {
        self.docs.get_mut(&id)
    }
}

/// Every tool.
pub fn tools() -> Vec<Tool> {
    let document = json!({"type": "integer", "description": "The document's number, from `open`."});
    let path = json!({"type": "string", "description": "An element's path, as `/orders/order[2]/customer`: `[n]` counts elements of that name among their siblings, from 1, and is left out when the name is there once."});
    let find_options = |mut props: Map<String, Value>| {
        props.insert(
            "match_case".into(),
            json!({"type": "boolean", "default": false}),
        );
        props.insert(
            "whole_word".into(),
            json!({"type": "boolean", "default": false}),
        );
        props
    };
    let page = |mut props: Map<String, Value>| {
        props.insert(
            "offset".into(),
            json!({"type": "integer", "default": 0, "description": "Items to skip."}),
        );
        props.insert(
            "limit".into(),
            json!({"type": "integer", "default": DEFAULT_LIMIT, "maximum": MAX_LIMIT}),
        );
        props
    };
    let format_options = || {
        let mut p = Map::new();
        p.insert("document".into(), document.clone());
        p.insert(
            "remove_comments".into(),
            json!({"type": "boolean", "default": false}),
        );
        p.insert("trim_text".into(), json!({"type": "boolean", "default": false, "description": "Trim whitespace at the start and end of elements that hold only text."}));
        p
    };
    let object = |props: Map<String, Value>, required: &[&str]| json!({"type": "object", "properties": props, "required": required});
    let props = |pairs: &[(&str, &Value)]| {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), (*v).clone()))
            .collect::<Map<String, Value>>()
    };
    let string = |d: &str| json!({"type": "string", "description": d});
    let mut format_props = format_options();
    format_props.insert(
        "indent".into(),
        json!({"type": "integer", "default": 4, "description": "Spaces per level."}),
    );
    format_props.insert("tabs".into(), json!({"type": "boolean", "default": false, "description": "One tab per level instead of spaces."}));
    format_props.insert("max_attributes".into(), json!({"type": "integer", "default": 6, "description": "Put each attribute on its own line when a start tag has more; 0 never."}));
    let edit = |extra: &[(&str, Value)], required: &[&str]| {
        let mut p = props(&[("document", &document), ("path", &path)]);
        for (k, v) in extra {
            p.insert(k.to_string(), v.clone());
        }
        let mut req = vec!["document", "path"];
        req.extend_from_slice(required);
        object(p, &req)
    };
    vec![
        Tool {
            name: "open",
            description: "Open an XML file. Returns its document number and what it holds: size, lines, elements, encoding, whether it is well-formed, and the root element. Large files open in about a second per 100 MiB.",
            schema: object(props(&[("path", &string("The file's path."))]), &["path"]),
            read_only: true,
        },
        Tool {
            name: "documents",
            description: "The open documents: number, path, and whether they have unsaved changes.",
            schema: object(Map::new(), &[]),
            read_only: true,
        },
        Tool {
            name: "close",
            description: "Close a document. Unsaved changes are lost.",
            schema: object(props(&[("document", &document)]), &["document"]),
            read_only: true,
        },
        Tool {
            name: "element",
            description: "One element: its name, line, attributes, text, and its child elements by name and count.",
            schema: object(
                props(&[("document", &document), ("path", &path)]),
                &["document", "path"],
            ),
            read_only: true,
        },
        Tool {
            name: "children",
            description: "The child elements of an element as a table, a page at a time: each row has its path and a cell per column, the columns being the children's attributes (`@name`), their child elements by name, and `text()`. A cell is a value, null when the row lacks it, or `{\"element\": name, \"count\": n}` for elements of their own.",
            schema: object(
                page(props(&[("document", &document), ("path", &path)])),
                &["document", "path"],
            ),
            read_only: true,
        },
        Tool {
            name: "text",
            description: "Lines of the document's source, from a line number (from 1).",
            schema: object(
                props(&[
                    ("document", &document),
                    ("from", &json!({"type": "integer", "default": 1})),
                    (
                        "lines",
                        &json!({"type": "integer", "default": DEFAULT_LIMIT, "maximum": MAX_LIMIT}),
                    ),
                ]),
                &["document"],
            ),
            read_only: true,
        },
        Tool {
            name: "find",
            description: "Find text in the source, tags and names included, in the whole source or within one element (its tags included). Returns the total and a page of matches with line, column, the element's path and the line.",
            schema: object(
                page(find_options(props(&[
                    ("document", &document),
                    ("query", &string("The text to find.")),
                    ("within", &string("Only inside this element, by path.")),
                ]))),
                &["document", "query"],
            ),
            read_only: true,
        },
        Tool {
            name: "problems",
            description: "The document's errors and warnings, with line and column.",
            schema: object(page(props(&[("document", &document)])), &["document"]),
            read_only: true,
        },
        Tool {
            name: "set_text",
            description: "Set the text of an element that holds only text or nothing.",
            schema: edit(
                &[(
                    "text",
                    string("The new text, as text: `<` and `&` are escaped."),
                )],
                &["text"],
            ),
            read_only: false,
        },
        Tool {
            name: "set_attribute",
            description: "Set an attribute of an element, adding it when it is not there.",
            schema: edit(
                &[
                    ("name", string("The attribute's name.")),
                    ("value", string("Its value, as text.")),
                ],
                &["name", "value"],
            ),
            read_only: false,
        },
        Tool {
            name: "remove_attribute",
            description: "Remove an attribute of an element.",
            schema: edit(&[("name", string("The attribute's name."))], &["name"]),
            read_only: false,
        },
        Tool {
            name: "rename",
            description: "Rename an element, its start and end tag.",
            schema: edit(&[("name", string("The new name."))], &["name"]),
            read_only: false,
        },
        Tool {
            name: "delete",
            description: "Delete an element, with its lines when it stands on lines of its own.",
            schema: edit(&[], &[]),
            read_only: false,
        },
        Tool {
            name: "duplicate",
            description: "Put a copy of an element right after it.",
            schema: edit(&[], &[]),
            read_only: false,
        },
        Tool {
            name: "move",
            description: "Swap an element with the previous or next element among its siblings.",
            schema: edit(
                &[(
                    "direction",
                    json!({"type": "string", "enum": ["up", "down"]}),
                )],
                &["direction"],
            ),
            read_only: false,
        },
        Tool {
            name: "replace",
            description: "Replace every match of a text, as typed, in the whole source or within one element (its tags included). Refused when the result would not be well-formed.",
            schema: object(
                find_options(props(&[
                    ("document", &document),
                    ("query", &string("The text to find.")),
                    ("with", &string("The replacement.")),
                    ("within", &string("Only inside this element, by path.")),
                ])),
                &["document", "query", "with"],
            ),
            read_only: false,
        },
        Tool {
            name: "format",
            description: "Indent the document. Changes only whitespace between tags; text, mixed content and xml:space=\"preserve\" stay, and so do the byte order mark and line endings. A settings comment in the file overrides the options.",
            schema: object(format_props, &["document"]),
            read_only: false,
        },
        Tool {
            name: "compact",
            description: "Put the document on one line: the opposite of format.",
            schema: object(format_options(), &["document"]),
            read_only: false,
        },
        Tool {
            name: "undo",
            description: "Undo the last change to a document.",
            schema: object(props(&[("document", &document)]), &["document"]),
            read_only: false,
        },
        Tool {
            name: "redo",
            description: "Redo the last undone change.",
            schema: object(props(&[("document", &document)]), &["document"]),
            read_only: false,
        },
        Tool {
            name: "save",
            description: "Save a document to its file, atomically, in the file's encoding. Changes nothing else in the file.",
            schema: object(props(&[("document", &document)]), &["document"]),
            read_only: false,
        },
        Tool {
            name: "save_as",
            description: "Save a document under another path, which it keeps.",
            schema: object(
                props(&[
                    ("document", &document),
                    ("path", &string("The new file's path.")),
                ]),
                &["document", "path"],
            ),
            read_only: false,
        },
    ]
}

/// Runs the tool `name` with `args` on `docs`.
pub fn call(
    docs: &mut impl Documents,
    name: &str,
    args: &Map<String, Value>,
) -> Result<Value, ToolError> {
    match name {
        "open" => {
            let path = PathBuf::from(string(args, "path")?);
            let id = docs
                .open(&path)
                .map_err(|e| ToolError::Refused(format!("cannot open {}: {e}", path.display())))?;
            Ok(summary(id, doc(docs, id)?))
        }
        "documents" => Ok(Value::Array(
            docs.ids()
                .into_iter()
                .filter_map(|id| {
                    let d = docs.get(id)?;
                    Some(json!({"document": id, "path": d.path, "modified": d.is_modified()}))
                })
                .collect(),
        )),
        "close" => {
            let id = number(args, "document")?;
            docs.close(id)?;
            Ok(json!({"closed": id}))
        }
        "element" => {
            let d = doc(docs, number(args, "document")?)?;
            let id = command::resolve(d, string(args, "path")?)?;
            Ok(element(d, id))
        }
        "children" => {
            let d = doc(docs, number(args, "document")?)?;
            let id = command::resolve(d, string(args, "path")?)?;
            let (offset, limit) = page(args);
            Ok(children(d, id, offset, limit))
        }
        "text" => {
            let d = doc(docs, number(args, "document")?)?;
            let from = optional_number(args, "from").unwrap_or(1).max(1) as usize;
            let lines = optional_number(args, "lines")
                .map_or(DEFAULT_LIMIT, |n| n as usize)
                .min(MAX_LIMIT);
            Ok(text(d, from, lines))
        }
        "find" => {
            let d = doc(docs, number(args, "document")?)?;
            let finder = Finder::new(string(args, "query")?, find_options(args))
                .map_err(|e| ToolError::Refused(e.to_string()))?;
            let within = within(d, args)?;
            let (offset, limit) = page(args);
            Ok(find(d, &finder, within, offset, limit))
        }
        "problems" => {
            let d = doc(docs, number(args, "document")?)?;
            let (offset, limit) = page(args);
            let all = d.problems();
            let shown: Vec<Value> = all
                .iter()
                .skip(offset)
                .take(limit)
                .map(|p| {
                    let (line, column) = position(d, p.offset as usize);
                    json!({
                        "severity": if p.severity == Severity::Error { "error" } else { "warning" },
                        "line": line,
                        "column": column,
                        "message": p.message,
                    })
                })
                .collect();
            Ok(json!({"total": all.len(), "problems": shown}))
        }
        "set_text" => {
            let text = string(args, "text")?;
            structured(docs, args, |d, id| command::set_text(d, id, text))
        }
        "set_attribute" => {
            let (name, value) = (string(args, "name")?, string(args, "value")?);
            structured(docs, args, |d, id| {
                command::set_attribute(d, id, name, value)
            })
        }
        "remove_attribute" => {
            let name = string(args, "name")?;
            structured(docs, args, |d, id| command::remove_attribute(d, id, name))
        }
        "rename" => {
            let name = string(args, "name")?;
            structured(docs, args, |d, id| command::rename(d, id, name))
        }
        "delete" => structured(docs, args, command::delete),
        "duplicate" => structured(docs, args, command::duplicate),
        "move" => {
            let direction = match string(args, "direction")? {
                "up" => MoveDirection::Up,
                "down" => MoveDirection::Down,
                _ => return Err(ToolError::Argument("direction")),
            };
            structured(docs, args, |d, id| command::move_element(d, id, direction))
        }
        "replace" => replace(docs, args),
        "format" | "compact" => format(docs, args, name == "format"),
        "undo" | "redo" => {
            let id = number(args, "document")?;
            let undo = name == "undo";
            let (done, modified) = docs
                .change(id, |d| {
                    let done = if undo { d.undo() } else { d.redo() };
                    (done, d.is_modified())
                })
                .ok_or(ToolError::NoDocument(id))?;
            Ok(json!({ if undo { "undone" } else { "redone" }: done, "modified": modified }))
        }
        "save" => {
            let id = number(args, "document")?;
            let d = doc_mut(docs, id)?;
            let path = d.path.clone().ok_or_else(|| {
                ToolError::Refused("the document has no file yet: use save_as".into())
            })?;
            save(d, &path)
        }
        "save_as" => {
            let id = number(args, "document")?;
            let path = PathBuf::from(string(args, "path")?);
            save(doc_mut(docs, id)?, &path)
        }
        _ => Err(ToolError::UnknownTool(name.into())),
    }
}

/// What `open` tells about a document.
fn summary(id: u64, d: &Document) -> Value {
    let s = d.stats();
    let root = d
        .index
        .root_element()
        .map(|r| String::from_utf8_lossy(d.index.name(&d.src, r)).into_owned());
    json!({
        "document": id,
        "path": d.path,
        "bytes": s.bytes,
        "lines": s.lines,
        "elements": s.elements,
        "encoding": d.encoding().map_or("UTF-8", |e| e.label.as_str()),
        "well_formed": s.error.is_none(),
        "error": s.error,
        "root": root,
    })
}

fn element(d: &Document, id: NodeId) -> Value {
    let idx = &d.index;
    let name = String::from_utf8_lossy(idx.name(&d.src, id)).into_owned();
    let attributes: Vec<Value> = idx
        .attributes(&d.src, id)
        .into_iter()
        .map(|(name, value)| json!({"name": name, "value": cut(&value)}))
        .collect();
    let mut counts: Vec<(String, usize)> = Vec::new();
    for c in idx.children(id) {
        if idx.node(c).kind != NodeKind::Element {
            continue;
        }
        let n = String::from_utf8_lossy(idx.name(&d.src, c)).into_owned();
        match counts.iter_mut().find(|(k, _)| *k == n) {
            Some((_, count)) => *count += 1,
            None => counts.push((n, 1)),
        }
    }
    let children: Vec<Value> = counts
        .into_iter()
        .map(|(name, count)| json!({"name": name, "count": count}))
        .collect();
    let text = idx.has_text(id).then(|| cut(&idx.text(&d.src, id)));
    json!({
        "path": idx.path(&d.src, id),
        "name": name,
        "line": d.line_of(idx.node(id).start) + 1,
        "attributes": attributes,
        "text": text,
        "children": children,
    })
}

fn children(d: &Document, parent: NodeId, offset: usize, limit: usize) -> Value {
    let grid = Grid::build(d, parent);
    let idx = &d.index;
    let base = idx.path(&d.src, parent);
    // A row's path: its name, and its position among the rows of that name when there are
    // several. The rows are the parent's child elements in document order.
    let names: Vec<&[u8]> = grid.rows.iter().map(|&r| idx.name(&d.src, r)).collect();
    let mut totals: HashMap<&[u8], usize> = HashMap::new();
    for n in &names {
        *totals.entry(n).or_default() += 1;
    }
    let mut seen: HashMap<&[u8], usize> = HashMap::new();
    let only_parent = grid.rows == [parent];
    let columns: Vec<String> = grid.columns.iter().map(column_name).collect();
    let mut rows = Vec::new();
    for (i, (&row, name)) in grid.rows.iter().zip(&names).enumerate() {
        let k = seen.entry(name).or_default();
        *k += 1;
        if i < offset || rows.len() >= limit {
            continue;
        }
        let path = if only_parent {
            base.clone()
        } else {
            let label = String::from_utf8_lossy(name);
            let sep = if base == "/" { "" } else { "/" };
            if totals[*name] > 1 {
                format!("{base}{sep}{label}[{k}]")
            } else {
                format!("{base}{sep}{label}")
            }
        };
        let cells: Map<String, Value> = grid
            .columns
            .iter()
            .zip(&columns)
            .map(|(c, label)| {
                let v = match grid.cell(d, row, c) {
                    Cell::Absent => Value::Null,
                    Cell::Value(v) => Value::String(v),
                    Cell::Element(name) => json!({"element": name, "count": 1}),
                    Cell::Repeated { name, count } => json!({"element": name, "count": count}),
                };
                (label.clone(), v)
            })
            .collect();
        rows.push(json!({"path": path, "cells": cells}));
    }
    json!({
        "path": base,
        "total": if only_parent { 0 } else { grid.rows.len() },
        "columns": columns,
        "rows": rows,
    })
}

fn text(d: &Document, from: usize, lines: usize) -> Value {
    let total = d.line_count();
    let first = from - 1;
    let mut out = String::new();
    let mut last = first;
    let mut cut_short = false;
    for n in first..(first + lines).min(total) {
        let line = String::from_utf8_lossy(d.line(n));
        if out.len() + line.len() + 1 > MAX_TEXT {
            cut_short = true;
            break;
        }
        out.push_str(&line);
        out.push('\n');
        last = n + 1;
    }
    json!({
        "from": from,
        "to": last,
        "total_lines": total,
        "text": out,
        "cut_at_64_kib": cut_short,
    })
}

fn find(d: &Document, finder: &Finder, within: Range<usize>, offset: usize, limit: usize) -> Value {
    let mut total = 0usize;
    let mut matches = Vec::new();
    let mut from = within.start;
    while let Some(found) = finder.next_within(&d.src, within.clone(), from, Direction::Forward) {
        if found.wrapped {
            break;
        }
        if total >= offset && matches.len() < limit {
            let at = found.range.start;
            let (line, column) = position(d, at);
            let path = element_at(d, at as u64).map(|e| d.index.path(&d.src, e));
            let text = String::from_utf8_lossy(d.line(line - 1));
            matches.push(json!({
                "line": line,
                "column": column,
                "path": path,
                "line_text": xml_atlas_engine::truncate(&text, LINE_CHARS),
            }));
        }
        total += 1;
        from = found.range.end.max(found.range.start + 1);
    }
    json!({"total": total, "matches": matches})
}

/// The source bytes of the element at `within`, its tags included, or the whole source.
fn within(d: &Document, args: &Map<String, Value>) -> Result<Range<usize>, ToolError> {
    match args.get("within").and_then(Value::as_str) {
        Some(path) => {
            if let Some(e) = &d.index.error {
                return Err(ToolError::Refused(format!(
                    "the document is not well-formed: {e}"
                )));
            }
            Ok(d.index.node(command::resolve(d, path)?).span())
        }
        None => Ok(0..d.src.len()),
    }
}

/// A structured edit (EDIT-5 to EDIT-10) on the element at `path`, as one undo step. Refused
/// on a document that is not well-formed, and undone when it would leave one.
fn structured(
    docs: &mut impl Documents,
    args: &Map<String, Value>,
    make: impl FnOnce(&Document, NodeId) -> Result<Edit, CommandError>,
) -> Result<Value, ToolError> {
    let id = number(args, "document")?;
    let d = doc(docs, id)?;
    if let Some(e) = &d.index.error {
        return Err(ToolError::Refused(format!(
            "the document is not well-formed: {e}"
        )));
    }
    let target = command::resolve(d, string(args, "path")?)?;
    let edit = make(d, target)?;
    let label = edit.label().to_string();
    apply(docs, id, edit, &label)
}

fn replace(docs: &mut impl Documents, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let id = number(args, "document")?;
    let d = doc(docs, id)?;
    let finder = Finder::new(string(args, "query")?, find_options(args))
        .map_err(|e| ToolError::Refused(e.to_string()))?;
    let range = within(d, args)?;
    match find_text::replace_all(&finder, &d.src, range, string(args, "with")?) {
        None => Ok(json!({"replaced": 0})),
        Some((edit, count)) => {
            let mut done = apply(docs, id, edit, "Replace All")?;
            done["replaced"] = json!(count);
            Ok(done)
        }
    }
}

fn format(
    docs: &mut impl Documents,
    args: &Map<String, Value>,
    indent: bool,
) -> Result<Value, ToolError> {
    let id = number(args, "document")?;
    let d = doc(docs, id)?;
    let flag = |k| args.get(k).and_then(Value::as_bool).unwrap_or(false);
    let base = FormatOptions {
        indent_size: optional_number(args, "indent").map_or(4, |n| n as usize),
        tabs: flag("tabs"),
        max_attributes: optional_number(args, "max_attributes").map_or(6, |n| n as usize),
        remove_comments: flag("remove_comments"),
        trim_text: flag("trim_text"),
    };
    let (label, result) = if indent {
        let chosen = d.format_options(base);
        ("Format", d.format(&chosen.options))
    } else {
        ("Compact", d.compact(&base))
    };
    let out = result.map_err(|e| ToolError::Refused(e.to_string()))?;
    let edit = Edit::whole(label, d.src.len(), out);
    apply(docs, id, edit, label)
}

/// [`apply_whole`] through [`Documents::change`].
fn apply(docs: &mut impl Documents, id: u64, edit: Edit, label: &str) -> Result<Value, ToolError> {
    docs.change(id, |d| apply_whole(d, edit, label))
        .ok_or(ToolError::NoDocument(id))?
}

/// Applies `edit` as one undo step; undoes it when it would leave a well-formed document
/// broken.
fn apply_whole(d: &mut Document, edit: Edit, label: &str) -> Result<Value, ToolError> {
    let was_whole = d.index.error.is_none();
    match d.apply(edit) {
        Ok(()) => {}
        Err(EditError::Empty) => return Ok(json!({"done": label, "changed": false})),
        Err(e) => return Err(ToolError::Refused(e.to_string())),
    }
    if was_whole && let Some(e) = d.index.error.clone() {
        d.undo();
        return Err(ToolError::Refused(format!(
            "{label} would leave the document not well-formed: {e}"
        )));
    }
    Ok(json!({"done": label, "changed": true, "modified": d.is_modified()}))
}

fn save(d: &mut Document, path: &Path) -> Result<Value, ToolError> {
    d.save(path)
        .map_err(|e| ToolError::Refused(format!("cannot save {}: {e}", path.display())))?;
    Ok(json!({"saved": path}))
}

/// Line and column of byte `at`, both from 1.
fn position(d: &Document, at: usize) -> (usize, usize) {
    (
        d.line_of(at as u64) + 1,
        xml_atlas_engine::text::column(d, at) + 1,
    )
}

/// The deepest element whose span holds `offset`.
fn element_at(d: &Document, offset: u64) -> Option<NodeId> {
    let idx = &d.index;
    let mut found = None;
    let mut cur = idx.root_element()?;
    'down: loop {
        let n = idx.node(cur);
        if !(n.start <= offset && offset < n.end) {
            return found;
        }
        found = Some(cur);
        for c in idx.children(cur) {
            let k = idx.node(c);
            if k.kind == NodeKind::Element && k.start <= offset && offset < k.end {
                cur = c;
                continue 'down;
            }
        }
        return found;
    }
}

fn column_name(c: &Column) -> String {
    match c {
        Column::Attribute(a) => format!("@{a}"),
        Column::Child(c) => c.clone(),
        Column::Text => "text()".into(),
    }
}

/// `s`, cut at [`MAX_TEXT`] bytes on a character boundary, with a note of its length.
fn cut(s: &str) -> String {
    if s.len() <= MAX_TEXT {
        return s.to_string();
    }
    let mut end = MAX_TEXT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [cut: {} bytes in all]", &s[..end], s.len())
}

fn doc(docs: &impl Documents, id: u64) -> Result<&Document, ToolError> {
    docs.get(id).ok_or(ToolError::NoDocument(id))
}

fn doc_mut(docs: &mut impl Documents, id: u64) -> Result<&mut Document, ToolError> {
    docs.get_mut(id).ok_or(ToolError::NoDocument(id))
}

fn string<'a>(args: &'a Map<String, Value>, key: &'static str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or(ToolError::Argument(key))
}

fn number(args: &Map<String, Value>, key: &'static str) -> Result<u64, ToolError> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or(ToolError::Argument(key))
}

fn optional_number(args: &Map<String, Value>, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn page(args: &Map<String, Value>) -> (usize, usize) {
    let offset = optional_number(args, "offset").unwrap_or(0) as usize;
    let limit = optional_number(args, "limit")
        .map_or(DEFAULT_LIMIT, |n| n as usize)
        .clamp(1, MAX_LIMIT);
    (offset, limit)
}

fn find_options(args: &Map<String, Value>) -> FindOptions {
    let flag = |k| args.get(k).and_then(Value::as_bool).unwrap_or(false);
    FindOptions {
        match_case: flag("match_case"),
        whole_word: flag("whole_word"),
    }
}
