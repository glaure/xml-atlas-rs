//! `xml-atlas-cli`: the engine's commands without a window.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod mcp;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use xml_atlas_engine::command::{self, CommandError};
use xml_atlas_engine::find::{self as find_text, Direction, FindOptions, Finder};
use xml_atlas_engine::problems::Severity;
use xml_atlas_engine::references::{self, Resolution, Rules};
use xml_atlas_engine::rules_folder::RulesFolder;
use xml_atlas_engine::structure::{MAX_PATHS, PathId, Structure};
use xml_atlas_engine::xpath::{self, Value};
use xml_atlas_engine::{Document, Edit, FormatOptions, truncate};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Open a file and print size, node counts, memory and timings.
    Stats {
        file: PathBuf,
        /// Open the file this many times and report the best and median timings.
        #[arg(long, default_value_t = 1)]
        repeat: u32,
    },
    /// Re-indent a file. A settings comment in the file ("Indentation Settings") overrides the
    /// options given here.
    Format {
        file: PathBuf,
        /// Spaces per level.
        #[arg(long, default_value_t = 4)]
        indent: usize,
        /// Indent with one tab per level.
        #[arg(long)]
        tabs: bool,
        /// Put each attribute on its own line when a start tag has more than this; 0 never.
        #[arg(long, default_value_t = 6)]
        max_attributes: usize,
        /// Use only the options given here, not the file's settings comment.
        #[arg(long)]
        ignore_document_settings: bool,
        #[command(flatten)]
        content: ContentArgs,
        /// Output file; defaults to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Put a file on one line: the opposite of format.
    Compact {
        file: PathBuf,
        #[command(flatten)]
        content: ContentArgs,
        /// Output file; defaults to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Set the text of an element that holds only text, e.g. `/orders/order[2]/customer`.
    SetText {
        #[command(flatten)]
        target: Target,
        text: String,
    },
    /// Set an attribute of an element, adding it if it is not there.
    SetAttribute {
        #[command(flatten)]
        target: Target,
        name: String,
        value: String,
    },
    /// Remove an attribute of an element.
    RemoveAttribute {
        #[command(flatten)]
        target: Target,
        name: String,
    },
    /// Rename an element (start and end tag).
    Rename {
        #[command(flatten)]
        target: Target,
        name: String,
    },
    /// Delete an element, with its lines when it stands on lines of its own.
    Delete {
        #[command(flatten)]
        target: Target,
    },
    /// Put a copy of an element right after it.
    Duplicate {
        #[command(flatten)]
        target: Target,
    },
    /// Swap an element with the previous or next element among its siblings.
    Move {
        #[command(flatten)]
        target: Target,
        direction: MoveDirection,
    },
    /// Check the references a rules file names (REF-4): count them and print those that find
    /// nothing.
    References {
        file: PathBuf,
        /// The rules file; by default the one that is on and for FILE in the
        /// configuration folder (Settings > References).
        #[arg(long, value_name = "PATH")]
        rules: Option<PathBuf>,
        /// Print at most this many references that find nothing; all are counted.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Check the rules file itself, against FILE (REF-14); fails when it has an error.
        #[arg(long)]
        check: bool,
    },
    /// Print an element with its references resolved (REF-5), as XML Atlas previews it.
    Preview {
        file: PathBuf,
        /// The element, by path: `/root/child[2]`.
        path: String,
        /// The rules file; by default the one that is on and for FILE in the
        /// configuration folder (Settings > References).
        #[arg(long, value_name = "PATH")]
        rules: Option<PathBuf>,
        /// Take a `nearest` context from this section, by path (REF-9); may be given once per
        /// scope.
        #[arg(long, value_name = "PATH")]
        section: Vec<String>,
        /// Leave repeated elements as written, though the rules have a `repeat` (REF-11).
        #[arg(long)]
        no_repeat: bool,
    },
    /// Find text and print the lines that contain it.
    Find {
        file: PathBuf,
        query: String,
        #[arg(long)]
        match_case: bool,
        #[arg(long)]
        whole_word: bool,
        /// Only inside this element (its tags included), by path: `/root/child[2]`.
        #[arg(long, value_name = "PATH")]
        within: Option<String>,
        /// Print at most this many matches; all are counted.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Evaluate an XPath 1.0 expression (D35): print each node it selects with its line, path
    /// and value, or the number, string or boolean it gives.
    Xpath {
        file: PathBuf,
        expr: String,
        /// Print at most this many nodes; all are counted.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// The document's structure (D36): one line per distinct path of element names, indented
    /// by depth, with how many elements have it, their attributes and whether they hold text.
    Structure { file: PathBuf },
    /// Replace every match of TEXT with WITH, as typed: in the whole file, or within one
    /// element. Refused when the result would not be well-formed.
    Replace {
        file: PathBuf,
        query: String,
        with: String,
        #[arg(long)]
        match_case: bool,
        #[arg(long)]
        whole_word: bool,
        /// Only inside this element (its tags included), by path: `/root/child[2]`.
        #[arg(long, value_name = "PATH")]
        within: Option<String>,
        /// Output file (may be the input file); defaults to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Serve the tools to an AI agent over the Model Context Protocol, on standard input and
    /// output (D27). Add it to Claude Code with `claude mcp add xml-atlas -- xml-atlas-cli mcp`.
    Mcp {
        /// Work in the open XML Atlas window, on its tabs, instead of on files (D28). The
        /// window must allow it: View > Allow AI agents (MCP).
        #[arg(long)]
        window: bool,
    },
}

/// The element an edit command works on, and where the result goes.
#[derive(Args)]
struct Target {
    file: PathBuf,
    /// Path of the element, as the GUI shows it: `/root/child[2]/grandchild`. A step without
    /// `[n]` must match exactly one element.
    path: String,
    /// Output file (may be the input file); defaults to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum MoveDirection {
    Up,
    Down,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    match Cli::parse().command {
        Command::Stats { file, repeat } => stats(&file, repeat.max(1)),
        Command::Mcp { window } => mcp::run(window),
        Command::Format {
            file,
            indent,
            tabs,
            max_attributes,
            ignore_document_settings,
            content,
            output,
        } => {
            let doc =
                Document::open(&file).with_context(|| format!("opening {}", file.display()))?;
            let base = FormatOptions {
                indent_size: indent,
                tabs,
                max_attributes,
                remove_comments: content.remove_comments,
                trim_text: content.trim_text,
            };
            let options = if ignore_document_settings {
                base
            } else {
                let chosen = doc.format_options(base);
                if !chosen.from_document.is_empty() {
                    eprintln!(
                        "using the file's settings comment for: {}",
                        chosen.from_document.join(", ")
                    );
                }
                chosen.options
            };
            eprintln!("format: {options}");
            write_output(output, &in_file_encoding(&doc, &doc.format(&options)?)?)
        }
        Command::Compact {
            file,
            content,
            output,
        } => {
            let doc =
                Document::open(&file).with_context(|| format!("opening {}", file.display()))?;
            let options = FormatOptions {
                remove_comments: content.remove_comments,
                trim_text: content.trim_text,
                ..FormatOptions::default()
            };
            write_output(output, &in_file_encoding(&doc, &doc.compact(&options)?)?)
        }
        Command::SetText { target, text } => edit(target, |d, id| command::set_text(d, id, &text)),
        Command::SetAttribute {
            target,
            name,
            value,
        } => edit(target, |d, id| command::set_attribute(d, id, &name, &value)),
        Command::RemoveAttribute { target, name } => {
            edit(target, |d, id| command::remove_attribute(d, id, &name))
        }
        Command::Rename { target, name } => edit(target, |d, id| command::rename(d, id, &name)),
        Command::Delete { target } => edit(target, command::delete),
        Command::Duplicate { target } => edit(target, command::duplicate),
        Command::Move { target, direction } => {
            let direction = match direction {
                MoveDirection::Up => command::Direction::Up,
                MoveDirection::Down => command::Direction::Down,
            };
            edit(target, |d, id| command::move_element(d, id, direction))
        }
        Command::References {
            file,
            rules,
            check: true,
            ..
        } => check_rules(&file, rules),
        Command::References {
            file, rules, limit, ..
        } => check_references(&file, rules, limit),
        Command::Preview {
            file,
            path,
            rules,
            section,
            no_repeat,
        } => preview(&file, &path, rules, &section, !no_repeat),
        Command::Find {
            file,
            query,
            match_case,
            whole_word,
            within,
            limit,
        } => find(
            &file,
            &query,
            FindOptions {
                match_case,
                whole_word,
            },
            within.as_deref(),
            limit,
        ),
        Command::Xpath { file, expr, limit } => xpath(&file, &expr, limit),
        Command::Structure { file } => structure(&file),
        Command::Replace {
            file,
            query,
            with,
            match_case,
            whole_word,
            within,
            output,
        } => replace(
            &file,
            &query,
            &with,
            FindOptions {
                match_case,
                whole_word,
            },
            within.as_deref(),
            output,
        ),
    }
}

/// Replace All (TEXT-14), the same as the GUI's.
fn replace(
    file: &PathBuf,
    query: &str,
    with: &str,
    options: FindOptions,
    within: Option<&str>,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let mut doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let finder = Finder::new(query, options)?;
    let range = within_range(&doc, file, within)?;
    let was_broken = doc.index.error.is_some();
    match find_text::replace_all(&finder, &doc.src, range, with) {
        None => eprintln!("no matches"),
        Some((edit, count)) => {
            match doc.apply(edit) {
                Ok(()) => eprintln!("replaced {count}"),
                Err(xml_atlas_engine::EditError::Empty) => {
                    eprintln!("{count} matches, all already as given: nothing to change")
                }
                Err(e) => return Err(e.into()),
            }
            if let (false, Some(e)) = (was_broken, &doc.index.error) {
                anyhow::bail!("the replacement would leave the document not well-formed: {e}");
            }
        }
    }
    write_output(output, &doc.encoded()?)
}

/// Options of format and compact that change content, so both are off unless asked for.
#[derive(clap::Args)]
struct ContentArgs {
    /// Leave out comments (not the "Indentation Settings" comment, nor comments inside text).
    #[arg(long)]
    remove_comments: bool,
    /// Trim whitespace at the start and end of elements that hold only text.
    #[arg(long)]
    trim_text: bool,
}

/// Writes `bytes` to `output`, or to stdout.
/// Opens the target's file, runs one edit command on the element at its path and writes the
/// result. The same commands as the GUI's (EDIT-5 to EDIT-10).
fn edit(
    target: Target,
    command: impl FnOnce(&Document, xml_atlas_engine::xml::NodeId) -> Result<Edit, CommandError>,
) -> anyhow::Result<()> {
    let file = &target.file;
    let mut doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    if let Some(e) = &doc.index.error {
        anyhow::bail!("{} is not well-formed: {e}", file.display());
    }
    let id = command::resolve(&doc, &target.path)?;
    let edit = command(&doc, id)?;
    let label = edit.label().to_string();
    match doc.apply(edit) {
        Ok(()) => eprintln!("{label}"),
        Err(xml_atlas_engine::EditError::Empty) => eprintln!("{label}: nothing to change"),
        Err(e) => return Err(e.into()),
    }
    if let Some(e) = &doc.index.error {
        anyhow::bail!("the edit would leave the document not well-formed: {e}");
    }
    write_output(target.output, &doc.encoded()?)
}

/// `bytes`, a new text of `doc`, in the encoding of `doc`'s file (OPEN-8).
fn in_file_encoding(doc: &Document, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(xml_atlas_engine::encoding::encode(bytes, doc.encoding())?.into_owned())
}

fn write_output(output: Option<PathBuf>, bytes: &[u8]) -> anyhow::Result<()> {
    match output {
        Some(path) => xml_atlas_engine::save_atomic(&path, bytes)
            .with_context(|| format!("writing {}", path.display()))?,
        None => std::io::Write::write_all(&mut std::io::stdout().lock(), bytes)?,
    }
    Ok(())
}

/// Characters of a matching line that are printed.
const FIND_LINE_CHARS: usize = 160;

/// Find (TEXT-12), in the whole file or within one element (TEXT-26), as the GUI's.
fn find(
    file: &PathBuf,
    query: &str,
    options: FindOptions,
    within: Option<&str>,
    limit: usize,
) -> anyhow::Result<()> {
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let finder = Finder::new(query, options)?;
    let range = within_range(&doc, file, within)?;
    let t = Instant::now();
    let mut count = 0usize;
    let mut from = range.start;
    while let Some(found) = finder.next_within(&doc.src, range.clone(), from, Direction::Forward) {
        if found.wrapped {
            break;
        }
        if count < limit {
            let n = doc.line_of(found.range.start as u64);
            let line = String::from_utf8_lossy(doc.line(n));
            println!("{:>8}: {}", n + 1, truncate(&line, FIND_LINE_CHARS));
        }
        count += 1;
        from = found.range.end.max(found.range.start + 1);
    }
    let noun = if count == 1 { "match" } else { "matches" };
    println!("{count} {noun} in {}", ms(t.elapsed()));
    Ok(())
}

/// How much of a node's value `xpath` prints.
const XPATH_VALUE_CHARS: usize = 80;

fn xpath(file: &Path, expr: &str, limit: usize) -> anyhow::Result<()> {
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let query = xpath::XPath::parse(expr)?;
    let t = Instant::now();
    let value = query.evaluate(
        &doc.src,
        &doc.index,
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    let took = t.elapsed();
    match value {
        Value::Nodes(nodes) => {
            for n in nodes.iter().take(limit) {
                let line = doc.line_of(n.span(&doc.src, &doc.index).start as u64) + 1;
                let path = n.path(&doc.src, &doc.index);
                let value = n.value(&doc.src, &doc.index, XPATH_VALUE_CHARS);
                if value.is_empty() {
                    println!("{line:>8}: {path}");
                } else {
                    println!("{line:>8}: {path}  {value}");
                }
            }
            let noun = if nodes.len() == 1 { "node" } else { "nodes" };
            println!("{} {noun} in {}", nodes.len(), ms(took));
        }
        other => {
            println!("{}", other.to_text().unwrap_or_default());
            let kind = match other {
                Value::Number(_) => "a number",
                Value::String(_) => "a string",
                _ => "a boolean",
            };
            println!("{kind}, in {}", ms(took));
        }
    }
    Ok(())
}

fn structure(file: &Path) -> anyhow::Result<()> {
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let t = Instant::now();
    let s = Structure::walk(
        &doc.src,
        &doc.index,
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    let took = t.elapsed();
    // Depth first: each path, then its children in the order they were met.
    let mut todo: Vec<PathId> = s.roots.iter().rev().copied().collect();
    while let Some(p) = todo.pop() {
        let info = &s.paths[p as usize];
        let mut line = format!(
            "{}{} \u{d7}{}",
            "  ".repeat(info.depth as usize - 1),
            String::from_utf8_lossy(&info.name),
            info.count
        );
        for a in &info.attributes {
            line.push_str(" @");
            line.push_str(&String::from_utf8_lossy(a));
        }
        if info.text {
            line.push_str(" (text)");
        }
        println!("{line}");
        todo.extend(info.children.iter().rev());
    }
    let noun = if s.paths.len() == 1 { "path" } else { "paths" };
    println!("{} {noun} in {}", s.paths.len(), ms(took));
    if s.capped {
        println!("stopped at {MAX_PATHS} paths: deeper elements are left out");
    }
    Ok(())
}

/// The rules for `file`, with their file.
fn load_rules(file: &Path, rules: Option<PathBuf>) -> anyhow::Result<(PathBuf, Rules)> {
    let path = rules_path(file, rules)?;
    let rules = Rules::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok((path, rules))
}

/// The rules file for `file`: `rules`, or the one Settings > References uses for it
/// (REF-13). View > Resolve references does not count here (REF-12).
fn rules_path(file: &Path, rules: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = rules {
        return Ok(path);
    }
    let settings =
        xml_atlas_mcp::bridge::settings_file().context("there is no configuration folder")?;
    let folder = RulesFolder::new(settings.parent().unwrap_or(Path::new(".")));
    let library = folder.load();
    if let Some(e) = library.error {
        anyhow::bail!(e);
    }
    let entry = library.for_document(Some(file)).with_context(|| {
        format!(
            "no rules file in {} is on and for {}; switch one on in File > Settings > \
             References, or give one with --rules",
            folder.path().display(),
            file.display()
        )
    })?;
    Ok(entry.path.clone())
}

/// `references --check` (REF-14), the same check as the rules editor's.
fn check_rules(file: &Path, rules: Option<PathBuf>) -> anyhow::Result<()> {
    let path = rules_path(file, rules)?;
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let report = references::check::check(&text, Some(&doc));
    for f in &report.findings {
        let kind = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let (line, column) = (f.line, f.column);
        println!("{}:{line}:{column}: {kind}: {}", path.display(), f.message);
    }
    for c in &report.counts {
        println!("{:>8}  {}", c.places, c.rule);
    }
    let errors = report.errors();
    let warnings = report.findings.len() - errors;
    println!(
        "{errors} error(s), {warnings} warning(s) in {}",
        path.display()
    );
    anyhow::ensure!(errors == 0, "the rules have errors");
    Ok(())
}

/// References (REF-4), the same check as the GUI's problems list.
fn check_references(file: &Path, rules: Option<PathBuf>, limit: usize) -> anyhow::Result<()> {
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let (rules_path, rules) = load_rules(file, rules)?;
    let root = doc
        .index
        .root_element()
        .context("the document has no root element")?;
    let t = Instant::now();
    let refs = references::references(&doc, &rules, root);
    let (mut found, mut skipped, mut runtime, mut elsewhere, mut missing) = (0, 0, 0, 0, 0);
    for r in &refs {
        match references::resolve(&doc, &rules, r, r.site).resolution {
            Resolution::Nodes(_) | Resolution::Value(_) => found += 1,
            Resolution::Skipped => skipped += 1,
            Resolution::Runtime(_) => runtime += 1,
            Resolution::Elsewhere(_) => elsewhere += 1,
            Resolution::Unresolved(why) => {
                if missing < limit {
                    let line = doc.line_of(r.span.start as u64) + 1;
                    println!("{line:>8}: {:?} {why}", r.expr);
                }
                missing += 1;
            }
        }
    }
    println!(
        "{} references in {} (rules: {}): {found} found, {missing} find nothing, {runtime} \
         known only at run time, {elsewhere} resolved where imported, {skipped} skipped",
        refs.len(),
        ms(t.elapsed()),
        rules_path.display()
    );
    Ok(())
}

/// Preview (REF-5), the same text as the GUI's preview pane.
fn preview(
    file: &Path,
    path: &str,
    rules: Option<PathBuf>,
    sections: &[String],
    repeat: bool,
) -> anyhow::Result<()> {
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let (_, rules) = load_rules(file, rules)?;
    let id = command::resolve(&doc, path)?;
    let mut choices = Vec::new();
    for s in sections {
        let section = command::resolve(&doc, s)?;
        choices.push(
            references::choice_for(&doc, &rules, section).with_context(|| {
                format!("{s} is not a section of a nearest context in the rules")
            })?,
        );
    }
    let mut p = references::preview_with(&doc, &rules, id, references::PREVIEW_LIMIT, &choices);
    // Expanded as the pane starts: where the rules have a `repeat` that is switched on.
    if let Some(rule) = rules.repeat().filter(|r| repeat && r.enabled) {
        p = xml_atlas_engine::repeat::expand(&p, rule, references::PREVIEW_LIMIT);
    }
    println!("{}", p.text);
    eprintln!(
        "{} references, {} not resolved{}",
        p.references,
        p.unresolved,
        if p.cut { ", cut" } else { "" }
    );
    Ok(())
}

/// The source bytes of the element at `within`, its tags included, or the whole file.
fn within_range(
    doc: &Document,
    file: &Path,
    within: Option<&str>,
) -> anyhow::Result<std::ops::Range<usize>> {
    match within {
        Some(path) => {
            if let Some(e) = &doc.index.error {
                anyhow::bail!("{} is not well-formed: {e}", file.display());
            }
            Ok(doc.index.node(command::resolve(doc, path)?).span())
        }
        None => Ok(0..doc.src.len()),
    }
}

fn stats(file: &PathBuf, repeat: u32) -> anyhow::Result<()> {
    let mut runs = Vec::with_capacity(repeat as usize);
    let mut last = None;
    for _ in 0..repeat {
        // Drop the previous document first so peak memory reflects one copy.
        drop(last.take());
        let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
        runs.push(doc.timings);
        last = Some(doc);
    }
    let doc = last.expect("repeat >= 1");
    let s = doc.stats();
    println!("file          {}", file.display());
    println!("size          {}", mib(s.bytes));
    if let Some(enc) = doc.encoding() {
        println!("encoding      {} (converted to UTF-8)", enc.label);
    }
    println!("lines         {}", s.lines);
    println!(
        "nodes         {} ({} elements, max depth {})",
        s.nodes, s.elements, s.max_depth
    );
    println!("index memory  {}", mib(s.index_memory));
    if let Some(err) = &s.error {
        println!("parse error   {err}");
    }
    runs.sort_by_key(|t| t.total());
    let best = runs[0];
    let median = runs[runs.len() / 2];
    match best.together {
        Some(together) => println!(
            "open (best)   {:>9} total = read {} + decode {} + {} for index {} alongside lines {}",
            ms(best.total()),
            ms(best.read),
            ms(best.decode),
            ms(together),
            ms(best.index),
            ms(best.lines)
        ),
        None => println!(
            "open (best)   {:>9} total = read {} + decode {} + index {} + lines {}",
            ms(best.total()),
            ms(best.read),
            ms(best.decode),
            ms(best.index),
            ms(best.lines)
        ),
    }
    if repeat > 1 {
        println!("open (median) {:>9} over {repeat} runs", ms(median.total()));
    }
    let secs = best.index.as_secs_f64();
    if secs > 0.0 {
        println!(
            "index speed   {:.0} MiB/s",
            s.bytes as f64 / 1048576.0 / secs
        );
    }
    Ok(())
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / 1048576.0)
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}
