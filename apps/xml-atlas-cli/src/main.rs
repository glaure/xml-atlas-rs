//! `xml-atlas-cli`: the engine's commands without a window.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use xml_atlas_engine::{Document, format};

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
    /// Re-indent a file.
    Format {
        file: PathBuf,
        #[arg(long, default_value_t = 2)]
        indent: usize,
        /// Output file; defaults to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    match Cli::parse().command {
        Command::Stats { file, repeat } => stats(&file, repeat.max(1)),
        Command::Format {
            file,
            indent,
            output,
        } => {
            let src =
                std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
            let out = format(&src, indent)?;
            match output {
                Some(path) => std::fs::write(&path, out)?,
                None => std::io::Write::write_all(&mut std::io::stdout().lock(), &out)?,
            }
            Ok(())
        }
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
