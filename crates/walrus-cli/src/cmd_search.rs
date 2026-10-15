//! `search`: find a string in one version of selected tracked files and print
//! a Markdown (default) or plain report (CLI-DESIGN 5.6).
//!
//! Parsing only: the reconstruct-and-match engine, the return-code rules
//! (0 found / 1 none / 4 beyond-last clamp), and the renderer live in the
//! shared `wal_cli_common::search` engine so `walrus` and `walx` behave
//! identically. Search is READ-ONLY.

use std::path::PathBuf;

use anyhow::Result;

use wal_cli_common::error::CliError;
use wal_cli_common::search::{self, SearchArgs};
use wal_cli_common::search_render::OutputFormat;
use wal_cli_common::selector::{self, Selector};

use crate::common::{split_option, take_value};

/// Run `search`, returning the shared engine's return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let parsed = parse(tokens)?;
    search::run(&parsed)
}

/// Parse `search` tokens into [`SearchArgs`]:
/// `search <archive> <string> [file-mask] [selector] [-i] [--regex] [--format]`.
fn parse(tokens: &[String]) -> Result<SearchArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut sel: Option<Selector> = None;
    let mut case_insensitive = false;
    let mut regex = false;
    let mut format = OutputFormat::Markdown;
    let mut options_done = false;
    let mut iter = tokens.iter().peekable();

    while let Some(token) = iter.next() {
        if options_done || !token.starts_with('-') || token == "-" {
            positionals.push(token.clone());
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
                selector::set_selector(&mut sel, selector::parse_index_value(&value)?)?;
            }
            "--version" => {
                let value = take_value(name, inline, &mut iter)?;
                selector::set_selector(&mut sel, selector::parse_version_value(&value)?)?;
            }
            "--at" => {
                let value = take_value(name, inline, &mut iter)?;
                let epoch = selector::parse_at_utc(&value)?;
                selector::set_selector(&mut sel, Selector::At(epoch))?;
            }
            "--label" => return Err(CliError::usage("--label is not supported yet")),
            "-i" | "--ignore-case" => case_insensitive = true,
            "--regex" => regex = true,
            "--format" => {
                let value = take_value(name, inline, &mut iter)?;
                format = parse_format(&value)?;
            }
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut positionals = positionals.into_iter();
    let archive = positionals.next().ok_or_else(|| {
        CliError::usage("usage: walrus search <archive> <string> [file-mask] [selector]")
    })?;
    let needle = positionals.next().ok_or_else(|| {
        CliError::usage("usage: walrus search <archive> <string> [file-mask] [selector]")
    })?;
    let mask = positionals.next();
    if positionals.next().is_some() {
        return Err(CliError::usage(
            "search takes at most <archive> <string> and one file-mask",
        ));
    }
    Ok(SearchArgs {
        archive: PathBuf::from(archive),
        needle,
        mask,
        selector: selector::effective_selector(sel),
        case_insensitive,
        regex,
        format,
    })
}

/// Parse a `--format` value into an [`OutputFormat`]; anything else is RC 2.
fn parse_format(value: &str) -> Result<OutputFormat, CliError> {
    match value {
        "markdown" => Ok(OutputFormat::Markdown),
        "plain" => Ok(OutputFormat::Plain),
        other => Err(CliError::usage(format!(
            "invalid --format value '{other}' (expected markdown or plain)"
        ))),
    }
}
