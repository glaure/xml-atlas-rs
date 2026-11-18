//! `xml-atlas-cli`: the engine's commands without a window.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod mcp;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use xml_atlas_engine::command::{self, CommandError};
use xml_atlas_engine::find::{self as find_text, Direction, FindOptions, Finder};
use xml_atlas_engine::references::{self, Resolution, Rules};
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
        /// The rules file; by default `xml-atlas-references.json` next to FILE or above it.
        #[arg(long, value_name = "PATH")]
        rules: Option<PathBuf>,
        /// Print at most this many references that find nothing; all are counted.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Print an element with its references resolved (REF-5), as XML Atlas previews it.
    Preview {
        file: PathBuf,
        /// The element, by path: `/root/child[2]`.
        path: String,
        /// The rules file; by default `xml-atlas-references.json` next to FILE or above it.
        #[arg(long, value_name = "PATH")]
        rules: Option<PathBuf>,
        /// Take a `nearest` context from this section, by path (REF-9); may be given once per
        /// scope.
        #[arg(long, value_name = "PATH")]
        section: Vec<String>,
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
        Command::References { file, rules, limit } => check_references(&file, rules, limit),
        Command::Preview {
            file,
            path,
            rules,
            section,
        } => preview(&file, &path, rules, &section),
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

/// The rules for `file`: from `rules`, or the rules file next to it or above it.
fn load_rules(file: &Path, rules: Option<PathBuf>) -> anyhow::Result<(PathBuf, Rules)> {
    let path = match rules {
        Some(p) => p,
        None => Rules::find(&std::path::absolute(file)?).with_context(|| {
            format!(
                "no {} next to {} or in a folder above it; give one with --rules",
                references::RULES_FILE,
                file.display()
            )
        })?,
    };
    let rules = Rules::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok((path, rules))
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
    let p = references::preview_with(&doc, &rules, id, references::PREVIEW_LIMIT, &choices);
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
