//! `extract`: restore selected tracked files to disk (CLI-DESIGN 5.2.1).
//!
//! Parsing only: the execution engine (per-file reconstruct, zip-slip / reserved
//! -name safety, beyond-last RC 4, overwrite RC 2, empty-match RC 8) lives in
//! `wal_cli_common::extract` and is shared verbatim with `walx`.

use std::path::PathBuf;

use anyhow::Result;

use wal_cli_common::error::CliError;
use wal_cli_common::extract::{self, ExtractArgs};
use wal_cli_common::selector::{self, Selector};

use crate::common::{split_option, take_value};

/// Run `extract`, returning the shared engine's return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let parsed = parse(tokens)?;
    extract::run(&parsed)
}

/// Parse `extract` tokens into [`ExtractArgs`].
fn parse(tokens: &[String]) -> Result<ExtractArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut sel: Option<Selector> = None;
    let mut output: Option<String> = None;
    let mut force = false;
    let mut sanitize = false;
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
            "-o" | "--output" => output = Some(take_value(name, inline, &mut iter)?),
            "--force" => force = true,
            "--sanitize" => sanitize = true,
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut positionals = positionals.into_iter();
    let archive = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus extract <archive> [file-mask] [selector]"))?;
    let mask = positionals.next();
    if positionals.next().is_some() {
        return Err(CliError::usage(
            "extract takes at most <archive> and one file-mask",
        ));
    }
    Ok(ExtractArgs {
        archive: PathBuf::from(archive),
        mask,
        selector: selector::effective_selector(sel),
        output: extract::output_dir(output.as_deref()),
        force,
        sanitize,
    })
}
