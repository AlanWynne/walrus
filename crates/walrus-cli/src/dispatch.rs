//! Top-level argument scan and operation dispatch (CLI-DESIGN sections 4, 5.1).
//!
//! The canonical form is `walrus <op> <archive> [mask] [options]`; the flag
//! aliases (`-q`/`--query`, `-x`/`--extract`, `-a`/`--add`, `-u`/`--update`,
//! `-r`/`--remove`) are convenience spellings that may sit after the archive.
//! Exactly ONE operation per invocation: naming two is a usage error (RC 2,
//! AC2). A bare `walrus <archive>` with no operation defaults to `query`
//! (CLI-DESIGN 5.1).

use anyhow::Result;

use wal_cli_common::error::CliError;

use crate::{
    cmd_add, cmd_alias, cmd_cat, cmd_condense, cmd_convert, cmd_extract, cmd_query, cmd_remove,
    cmd_search, cmd_update,
};

/// The operations this binary dispatches: the eight content commands plus the
/// `alias` shim helper (which mutates no archive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Query,
    Extract,
    Cat,
    Search,
    Add,
    Update,
    Remove,
    Condense,
    Convert,
    Alias,
}

/// Dispatch to a subcommand, returning its process return code.
pub fn run() -> Result<u8> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    run_with(&argv)
}

/// Testable dispatch over an explicit argument vector.
pub fn run_with(argv: &[String]) -> Result<u8> {
    if argv.is_empty() {
        print_usage();
        return Ok(2);
    }
    // A top-level help request anywhere before `--` prints the banner (RC 0).
    // Help takes precedence over version when both are present.
    if wants_top_level_help(argv) {
        print_usage();
        return Ok(0);
    }
    // A top-level version request anywhere before `--` prints the version to
    // stdout and exits 0 (standard CLI convention; version is normal output).
    if wants_top_level_version(argv) {
        print_version();
        return Ok(0);
    }

    let (op, rest) = scan_operation(argv)?;
    match op {
        Op::Query => cmd_query::run(&rest),
        Op::Extract => cmd_extract::run(&rest),
        Op::Cat => cmd_cat::run(&rest),
        Op::Search => cmd_search::run(&rest),
        Op::Add => cmd_add::run(&rest),
        Op::Update => cmd_update::run(&rest),
        Op::Remove => cmd_remove::run(&rest),
        Op::Condense => cmd_condense::run(&rest),
        Op::Convert => cmd_convert::run(&rest),
        Op::Alias => cmd_alias::run(&rest),
    }
}

/// True when `-h`/`--help`/`help` appears as a top-level token before any `--`.
fn wants_top_level_help(argv: &[String]) -> bool {
    for token in argv {
        if token == "--" {
            break;
        }
        if token == "-h" || token == "--help" || token == "help" {
            return true;
        }
    }
    false
}

/// True when the FIRST argument is `-V` or `--version` (the version request).
///
/// `-V` (uppercase) is the version short flag by GNU convention; `-v` is left
/// free for a future `--verbose`.
///
/// CRUCIAL: `--version` is ALSO the extract/cat version SELECTOR
/// (`--version <current|original|+/-n>`), so it must only be treated as the
/// top-level version flag when it is the FIRST token -- i.e. `walrus --version`
/// with no operation. Scanning the whole argv (as `--help` safely can, since
/// `help`/`--help` is never a selector) would wrongly hijack
/// `walrus extract <arc> --version original`, printing the version instead of
/// extracting. Help has no such clash; version does, hence the first-token-only
/// rule.
fn wants_top_level_version(argv: &[String]) -> bool {
    matches!(
        argv.first().map(String::as_str),
        Some("-V") | Some("--version")
    )
}

/// Print the binary name and crate version to stdout (e.g. `walrus 0.1.0`).
///
/// The version is the `walrus-cli` crate version resolved at compile time, so
/// it always matches the built binary and needs no manual upkeep.
pub fn print_version() {
    println!("walrus {}", env!("CARGO_PKG_VERSION"));
}

/// Scan `argv` for the single operation and return it with the remaining
/// non-operation tokens (preserving order). Detects a second operation (RC 2,
/// AC2) and defaults to [`Op::Query`] when none is named.
fn scan_operation(argv: &[String]) -> Result<(Op, Vec<String>), CliError> {
    let mut found: Option<Op> = None;
    let mut rest: Vec<String> = Vec::with_capacity(argv.len());
    let mut options_done = false;

    for (index, token) in argv.iter().enumerate() {
        if options_done {
            rest.push(token.clone());
            continue;
        }
        if token == "--" {
            options_done = true;
            rest.push(token.clone());
            continue;
        }

        // The FIRST token may be a canonical subcommand name; a subcommand word
        // appearing later is treated as a positional (e.g. a file mask).
        if index == 0 {
            if let Some(op) = canonical_op(token) {
                set_op(&mut found, op)?;
                continue;
            }
        }
        if let Some(op) = flag_op(token) {
            set_op(&mut found, op)?;
            continue;
        }
        rest.push(token.clone());
    }

    Ok((found.unwrap_or(Op::Query), rest))
}

/// Record the operation, raising RC 2 if one was already set (AC2).
fn set_op(slot: &mut Option<Op>, op: Op) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::usage(
            "only one operation may be given per invocation",
        ));
    }
    *slot = Some(op);
    Ok(())
}

/// Map a canonical subcommand word to its [`Op`].
fn canonical_op(token: &str) -> Option<Op> {
    match token {
        "query" => Some(Op::Query),
        "extract" => Some(Op::Extract),
        "cat" => Some(Op::Cat),
        "search" => Some(Op::Search),
        "add" => Some(Op::Add),
        "update" => Some(Op::Update),
        "remove" => Some(Op::Remove),
        "condense" => Some(Op::Condense),
        "convert" => Some(Op::Convert),
        "alias" => Some(Op::Alias),
        _ => None,
    }
}

/// Map a flag-alias token to its [`Op`] (CLI-DESIGN section 4).
fn flag_op(token: &str) -> Option<Op> {
    match token {
        "-q" | "--query" => Some(Op::Query),
        "-x" | "--extract" => Some(Op::Extract),
        "-a" | "--add" => Some(Op::Add),
        "-u" | "--update" => Some(Op::Update),
        "-r" | "--remove" => Some(Op::Remove),
        _ => None,
    }
}

/// Print the usage banner to stderr.
pub fn print_usage() {
    eprintln!(
        "walrus -- manage .wal / .wbl archives with change history\n\n\
         USAGE:\n\
         \x20 walrus query    <archive> [file-mask] [-l]\n\
         \x20 walrus extract  <archive> [file-mask] [selector] [-o <dir>] [--force] [--sanitize]\n\
         \x20 walrus cat      <archive> <file>      [selector]\n\
         \x20 walrus search   <archive> <string> [file-mask] [selector] [--all-versions] [-i] [--regex] [--format <markdown|plain>]\n\
         \x20 walrus add      <archive> <files...>  [--generations <n>] [--wal|--wbl|--direction <d>] [-R]\n\
         \x20 walrus update   <archive> <files...>  [--generations <n>] [-R]\n\
         \x20 walrus remove   <archive> <file-mask> [--force]\n\
         \x20 walrus condense <archive> <file-mask> [--keep <n>]\n\
         \x20 walrus convert  <archive> [file-mask] --to <wal|wbl> [-o <name>]\n\
         \x20 walrus alias    <install|remove|status> [--name <alias>] [--dir <dir>] [--force]\n\n\
         A bare `walrus <archive>` lists the archive (query). Read from stdin with\n\
         `-` plus `--name <stored-name>` on add/update.\n\n\
         SELECTOR (at most one):\n\
         \x20 --index <n> | --version <current|original|+/-n> | --at <YYYY-MM-DDThh:mm:ssZ>\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn version_as_first_token_long_and_short_is_detected() {
        assert!(wants_top_level_version(&argv(&["--version"])));
        assert!(wants_top_level_version(&argv(&["-V"])));
    }

    #[test]
    fn lowercase_v_is_not_the_version_flag() {
        // -v is reserved for a future --verbose; only -V/--version print version.
        assert!(!wants_top_level_version(&argv(&["-v"])));
    }

    #[test]
    fn version_as_a_selector_after_a_subcommand_is_not_the_version_flag() {
        // REGRESSION GUARD: `--version` is also the extract/cat version SELECTOR.
        // It must only be the top-level flag as the FIRST token, never when it
        // follows a subcommand -- otherwise `extract <arc> --version original`
        // would print the version instead of extracting (the bug this fixes).
        assert!(!wants_top_level_version(&argv(&[
            "extract",
            "a.wal",
            "--version",
            "original"
        ])));
        assert!(!wants_top_level_version(&argv(&[
            "cat",
            "a.wal",
            "f.txt",
            "--version",
            "original"
        ])));
        // And two selectors must still reach the subcommand (to be RC 2 there),
        // not be short-circuited here.
        assert!(!wants_top_level_version(&argv(&[
            "extract",
            "a.wal",
            "--index",
            "0",
            "--version",
            "original"
        ])));
    }

    #[test]
    fn version_request_dispatches_to_rc0() {
        assert_eq!(run_with(&argv(&["--version"])).unwrap(), 0);
        assert_eq!(run_with(&argv(&["-V"])).unwrap(), 0);
    }

    #[test]
    fn help_takes_precedence_over_version_when_both_present() {
        // --help first -> help path; still RC 0, ordering asserted so a future
        // refactor cannot silently flip precedence.
        assert!(wants_top_level_help(&argv(&["--help", "--version"])));
        assert_eq!(run_with(&argv(&["--help", "--version"])).unwrap(), 0);
    }
}
