//! `xml-atlas-cli`: the engine's commands without a window.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

mod mcp;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use xml_atlas_engine::command::{self, CommandError};
use xml_atlas_engine::find::{self as find_text, Direction, FindOptions, Finder};
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
    /// Find text and print the lines that contain it.
    Find {
        file: PathBuf,
        query: String,
        #[arg(long)]
        match_case: bool,
        #[arg(long)]
        whole_word: bool,
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
        Command::Find {
            file,
            query,
            match_case,
            whole_word,
            limit,
        } => find(
            &file,
            &query,
            FindOptions {
                match_case,
                whole_word,
            },
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
    let range = match within {
        Some(path) => {
            if let Some(e) = &doc.index.error {
                anyhow::bail!("{} is not well-formed: {e}", file.display());
            }
            doc.index.node(command::resolve(&doc, path)?).span()
        }
        None => 0..doc.src.len(),
    };
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

fn find(file: &PathBuf, query: &str, options: FindOptions, limit: usize) -> anyhow::Result<()> {
    let doc = Document::open(file).with_context(|| format!("opening {}", file.display()))?;
    let finder = Finder::new(query, options)?;
    let t = Instant::now();
    let mut count = 0usize;
    let mut from = 0;
    while let Some(found) = finder.next(&doc.src, from, Direction::Forward) {
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
