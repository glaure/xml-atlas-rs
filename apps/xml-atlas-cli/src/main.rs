//! `xml-atlas-cli`: the engine's commands without a window.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Parser, Subcommand};
use xml_atlas_engine::find::{Direction, FindOptions, Finder};
use xml_atlas_engine::{Document, FormatOptions, truncate};

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
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    match Cli::parse().command {
        Command::Stats { file, repeat } => stats(&file, repeat.max(1)),
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
            write_output(output, &doc.format(&options)?)
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
            write_output(output, &doc.compact(&options)?)
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
    }
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
fn write_output(output: Option<PathBuf>, bytes: &[u8]) -> anyhow::Result<()> {
    match output {
        Some(path) => std::fs::write(&path, bytes)?,
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
    println!(
        "open (best)   {:>9} total = read {} + index {} + lines {}",
        ms(best.total()),
        ms(best.read),
        ms(best.index),
        ms(best.lines)
    );
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
