//! Shared option-scan helpers for the `walrus` command modules.
//!
//! These mirror the hand-rolled scan `walx` uses (INV-2: no clap), adapted for
//! the richer `walrus` command set. The selector grammar, mask globbing, and
//! naming safety come from `wal-cli-common`; this module owns only the
//! low-level token helpers every command shares.

use wal_cli_common::error::CliError;

/// Split a `--opt=value` token into its name and the inline value, or
/// `(token, None)` when there is no `=`.
pub fn split_option(token: &str) -> (&str, Option<String>) {
    match token.split_once('=') {
        Some((name, value)) => (name, Some(value.to_string())),
        None => (token, None),
    }
}

/// Take an option's value: the inline `--opt=value` form if present, else the
/// next token. A missing value is RC 2.
pub fn take_value<'a, I>(
    name: &str,
    inline: Option<String>,
    iter: &mut std::iter::Peekable<I>,
) -> Result<String, CliError>
where
    I: Iterator<Item = &'a String>,
{
    if let Some(value) = inline {
        return Ok(value);
    }
    match iter.next() {
        Some(value) => Ok(value.clone()),
        None => Err(CliError::usage(format!("option '{name}' needs a value"))),
    }
}
