//! walx -- a small CLI over the walrus-fmt crate.
//!
//! Subcommands (scaffold):
//!   walx info     <file>              print manifest + version count
//!   walx extract  <file> <index>      write a reconstructed version to stdout
//!   walx convert  <in> <out>          convert .wal <-> .wbl (TODO)
//!
//! A fuller argument parser (clap) can be added later; this hand-rolled parser
//! keeps the scaffold dependency-light and the intent obvious.

use std::process::ExitCode;

use anyhow::{bail, Context};
use walrus_fmt::{Version, WalrusFile};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("walx: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        print_usage();
        bail!("no subcommand given");
    };

    match cmd.as_str() {
        "info" => cmd_info(args.get(1)),
        "extract" => cmd_extract(args.get(1), args.get(2)),
        "convert" => cmd_convert(args.get(1), args.get(2)),
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        other => {
            print_usage();
            bail!("unknown subcommand '{other}'");
        }
    }
}

fn cmd_info(path: Option<&String>) -> anyhow::Result<()> {
    let path = path.context("usage: walx info <file>")?;
    let doc = WalrusFile::open(path).with_context(|| format!("opening {path}"))?;
    let m = doc.manifest();
    println!("file:      {path}");
    println!("variant:   {:?}", m.variant);
    println!("spec:      {}", m.spec_version);
    // Document metadata is per-file now; the single-document model stores it on
    // the first (and only) tracked file.
    if let Some(file) = m.files.first() {
        println!(
            "document:  {} ({}, {})",
            file.name, file.mime, file.encoding
        );
    }
    println!(
        "versions:  0..={} ({} change records)",
        doc.len(),
        doc.len()
    );
    let checkpoints = m.checkpoints();
    if !checkpoints.is_empty() {
        println!("checkpoints:");
        for cp in checkpoints {
            println!("  [{:>5}] {}", cp.index, cp.label);
        }
    }
    Ok(())
}

fn cmd_extract(path: Option<&String>, index: Option<&String>) -> anyhow::Result<()> {
    let path = path.context("usage: walx extract <file> <index>")?;
    let index: usize = index
        .context("usage: walx extract <file> <index>")?
        .parse()
        .context("<index> must be a non-negative integer")?;
    let doc = WalrusFile::open(path).with_context(|| format!("opening {path}"))?;
    let bytes = doc
        .reconstruct(Version::Index(index))
        .with_context(|| format!("reconstructing version {index}"))?;
    use std::io::Write;
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}

fn cmd_convert(_input: Option<&String>, _output: Option<&String>) -> anyhow::Result<()> {
    // TODO: implement .wal <-> .wbl conversion:
    //   1. open the input, reconstruct both endpoints,
    //   2. invert each splice to flip log direction,
    //   3. re-base onto the opposite endpoint,
    //   4. write the opposite-variant container.
    bail!("`walx convert` is not implemented yet (scaffold)");
}

fn print_usage() {
    eprintln!(
        "walx -- inspect and extract .wal / .wbl files\n\n\
         USAGE:\n\
         \x20 walx info    <file>           show manifest and version count\n\
         \x20 walx extract <file> <index>   write version <index> to stdout\n\
         \x20 walx convert <in> <out>       convert .wal <-> .wbl (todo)\n"
    );
}
