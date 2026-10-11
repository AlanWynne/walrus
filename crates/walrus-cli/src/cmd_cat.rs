//! `cat`: write exactly ONE tracked file's bytes to stdout (CLI-DESIGN 5.2.2).
//!
//! Parsing only: the one-match-or-RC2 and beyond-last RC 4 rules live in the
//! shared `wal_cli_common::cat` engine.

use std::path::PathBuf;

use anyhow::Result;

use wal_cli_common::cat::{self, CatArgs};
use wal_cli_common::error::CliError;
use wal_cli_common::selector::{self, Selector};

use crate::common::{split_option, take_value};

/// Run `cat`, returning the shared engine's return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let parsed = parse(tokens)?;
    cat::run(&parsed)
}

/// Parse `cat` tokens into [`CatArgs`].
fn parse(tokens: &[String]) -> Result<CatArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut sel: Option<Selector> = None;
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
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut positionals = positionals.into_iter();
    let archive = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus cat <archive> <file> [selector]"))?;
    let file = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus cat <archive> <file> [selector]"))?;
    if positionals.next().is_some() {
        return Err(CliError::usage("cat takes exactly <archive> and <file>"));
    }
    Ok(CatArgs {
        archive: PathBuf::from(archive),
        file,
        selector: selector::effective_selector(sel),
    })
}
