//! walx -- an extract-only CLI over the walrus-fmt multi-file Archive API.
//!
//! Two subcommands, mutating nothing and transforming nothing:
//!   walx extract <archive> [file-mask] [selector] [-o <dir>] [--force] [--sanitize]
//!   walx cat     <archive> <file>      [selector]
//!
//! where `[selector]` is at most one of
//!   --index <n> | --version <current|original|+/-n> | --at <YYYY-MM-DDThh:mm:ssZ>
//!
//! The parser is hand-rolled to keep the dependency surface minimal (INV-2).
//! The selector grammar, naming safety, and the extract/cat engines live in the
//! shared `wal-cli-common` crate; this binary owns only dispatch, the option
//! scan, and the usage banner.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;

use wal_cli_common::cat::{self, CatArgs};
use wal_cli_common::error::CliError;
use wal_cli_common::extract::{self, ExtractArgs};
use wal_cli_common::selector::{self, Selector};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            // A typed CliError carries its own return code; anything else is a
            // general failure (RC 1 via anyhow).
            if let Some(cli) = e.downcast_ref::<CliError>() {
                eprintln!("walx: {}", cli.message);
                ExitCode::from(cli.code)
            } else {
                eprintln!("walx: {e:#}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Dispatch to a subcommand, returning its process return code.
fn run() -> Result<u8> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first() else {
        print_usage();
        return Ok(2);
    };

    match cmd.as_str() {
        "extract" => {
            let parsed = parse_extract(&argv[1..])?;
            Ok(extract::run(&parsed)?)
        }
        "cat" => {
            let parsed = parse_cat(&argv[1..])?;
            Ok(cat::run(&parsed)?)
        }
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(0)
        }
        other => {
            print_usage();
            Err(CliError::usage(format!("unknown subcommand '{other}'")).into())
        }
    }
}

/// Collected tokens from the shared selector/positional scan.
struct Scan {
    positionals: Vec<String>,
    selector: Option<Selector>,
    output: Option<String>,
    force: bool,
    sanitize: bool,
}

/// Parse the shared option grammar for both subcommands: positionals, the
/// mutually-exclusive version selector, and the extract-only `-o`/`--force`/
/// `--sanitize` flags. `allow_disk` enables the disk-targeting flags (extract
/// only); `cat` rejects them as unknown options.
fn scan(tokens: &[String], allow_disk: bool) -> Result<Scan, CliError> {
    let mut scan = Scan {
        positionals: Vec::new(),
        selector: None,
        output: None,
        force: false,
        sanitize: false,
    };
    let mut options_done = false;
    let mut iter = tokens.iter().enumerate().peekable();

    while let Some((_, token)) = iter.next() {
        if options_done || !token.starts_with('-') {
            if token == "--" && !options_done {
                options_done = true;
                continue;
            }
            scan.positionals.push(token.clone());
            continue;
        }
        if token == "--" {
            options_done = true;
            continue;
        }

        let (name, inline) = split_option(token);
        match name {
            "--index" => {
                let value = take_value(name, inline, &mut iter)?;
                selector::set_selector(&mut scan.selector, selector::parse_index_value(&value)?)?;
            }
            "--version" => {
                let value = take_value(name, inline, &mut iter)?;
                selector::set_selector(&mut scan.selector, selector::parse_version_value(&value)?)?;
            }
            "--at" => {
                let value = take_value(name, inline, &mut iter)?;
                let epoch = selector::parse_at_utc(&value)?;
                selector::set_selector(&mut scan.selector, Selector::At(epoch))?;
            }
            "--label" => {
                // Deferred past 0.1 (CLI-DESIGN 5.2.1): reject as usage.
                return Err(CliError::usage("--label is not supported yet"));
            }
            "-o" | "--output" if allow_disk => {
                let value = take_value(name, inline, &mut iter)?;
                scan.output = Some(value);
            }
            "--force" if allow_disk => scan.force = true,
            "--sanitize" if allow_disk => scan.sanitize = true,
            other => {
                return Err(CliError::usage(format!("unknown option '{other}'")));
            }
        }
    }
    Ok(scan)
}

/// Split a `--opt=value` token into its name and the inline value, or `(token,
/// None)` when there is no `=`.
fn split_option(token: &str) -> (&str, Option<String>) {
    match token.split_once('=') {
        Some((name, value)) => (name, Some(value.to_string())),
        None => (token, None),
    }
}

/// Take an option's value: the inline `--opt=value` form if present, else the
/// next token. A missing value is RC 2.
fn take_value<'a, I>(
    name: &str,
    inline: Option<String>,
    iter: &mut std::iter::Peekable<I>,
) -> Result<String, CliError>
where
    I: Iterator<Item = (usize, &'a String)>,
{
    if let Some(value) = inline {
        return Ok(value);
    }
    match iter.next() {
        Some((_, value)) => Ok(value.clone()),
        None => Err(CliError::usage(format!("option '{name}' needs a value"))),
    }
}

/// Build [`ExtractArgs`] from the raw `extract` tokens.
fn parse_extract(tokens: &[String]) -> Result<ExtractArgs, CliError> {
    let scan = scan(tokens, true)?;
    let mut positionals = scan.positionals.into_iter();
    let archive = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walx extract <archive> [file-mask] [selector]"))?;
    let mask = positionals.next();
    if positionals.next().is_some() {
        return Err(CliError::usage(
            "extract takes at most <archive> and one file-mask",
        ));
    }
    Ok(ExtractArgs {
        archive: PathBuf::from(archive),
        mask,
        selector: selector::effective_selector(scan.selector),
        output: extract::output_dir(scan.output.as_deref()),
        force: scan.force,
        sanitize: scan.sanitize,
    })
}

/// Build [`CatArgs`] from the raw `cat` tokens.
fn parse_cat(tokens: &[String]) -> Result<CatArgs, CliError> {
    let scan = scan(tokens, false)?;
    let mut positionals = scan.positionals.into_iter();
    let archive = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walx cat <archive> <file> [selector]"))?;
    let file = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walx cat <archive> <file> [selector]"))?;
    if positionals.next().is_some() {
        return Err(CliError::usage("cat takes exactly <archive> and <file>"));
    }
    Ok(CatArgs {
        archive: PathBuf::from(archive),
        file,
        selector: selector::effective_selector(scan.selector),
    })
}

/// Print the usage banner to stderr.
fn print_usage() {
    eprintln!(
        "walx -- extract-only CLI for .wal / .wbl archives\n\n\
         USAGE:\n\
         \x20 walx extract <archive> [file-mask] [selector] [-o <dir>] [--force] [--sanitize]\n\
         \x20 walx cat     <archive> <file>      [selector]\n\n\
         SELECTOR (at most one):\n\
         \x20 --index <n> | --version <current|original|+/-n> | --at <YYYY-MM-DDThh:mm:ssZ>\n"
    );
}
