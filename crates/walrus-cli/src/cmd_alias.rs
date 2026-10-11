//! `alias`: the opt-in, collision-aware `wal` shim helper (CLI-DESIGN section 4).
//!
//! `alias install|remove|status` manages a single shim file in a bin directory.
//! It NEVER edits a shell rc file or the registry. `install` refuses to shadow a
//! FOREIGN `wal` (RC 2, creating nothing) even with `--force`, and re-points its
//! own prior shim idempotently. `remove` deletes only our shim; `status` reports
//! whether a `wal` is on PATH and whether it is ours.
//!
//! The PATH to search and the target directory are resolved up front so the
//! tests can inject a FAKE PATH (via the `WALRUS_ALIAS_PATH` environment
//! variable) and a `TempDir` `--dir`, never touching the real `$PATH`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use wal_cli_common::error::CliError;

use crate::alias_shim::{self, ShimKind};
use crate::common::{split_option, take_value};

/// The subcommand of `alias`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Install,
    Remove,
    Status,
}

/// Parsed `alias` inputs.
struct AliasArgs {
    action: Action,
    name: String,
    dir: Option<PathBuf>,
    // Accepted for CLI compatibility (CLI-DESIGN section 4) but deliberately
    // has NO effect: --force never overrides a foreign-`wal` refusal. Kept as a
    // field so an unknown-use never silently changes the refuse-by-default
    // behaviour; read here only to document that intent.
    #[allow(dead_code)]
    force: bool,
}

/// Run `alias`, returning the process return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let args = parse(tokens)?;
    let path_entries = injected_or_real_path();
    match args.action {
        Action::Install => install(&args, &path_entries),
        Action::Remove => remove(&args, &path_entries),
        Action::Status => status(&args, &path_entries),
    }
}

// === install =========================================================

/// Install (or re-point) our shim, refusing to shadow a foreign `wal`.
fn install(args: &AliasArgs, path_entries: &[PathBuf]) -> Result<u8> {
    // Collision check FIRST: any `wal` already on PATH that is not ours makes
    // us refuse and create nothing -- even with --force (CLI-DESIGN section 4).
    if let Some(found) = find_on_path(&args.name, path_entries) {
        if alias_shim::classify(&found) == ShimKind::Foreign {
            return Err(CliError::usage(format!(
                "'{name}' is already on PATH at {path}; refusing to shadow a \
                 binary we did not create (pick --name or --dir)",
                name = args.name,
                path = found.display()
            ))
            .into());
        }
    }

    let target_dir = resolve_target_dir(args, path_entries)?;
    let shim = alias_shim::shim_path(&target_dir, &args.name);

    // Re-pointing our own prior shim in the target dir is idempotent; a foreign
    // file sitting at the exact target path is still never overwritten.
    if alias_shim::exists(&shim) && alias_shim::classify(&shim) == ShimKind::Foreign {
        return Err(CliError::usage(format!(
            "a file we did not create already occupies {}; refusing to overwrite it",
            shim.display()
        ))
        .into());
    }

    let walrus_bin = current_binary()?;
    alias_shim::create(&shim, &walrus_bin)
        .with_context(|| format!("creating shim {}", shim.display()))?;
    println!("installed '{}' -> {}", args.name, walrus_bin.display());
    Ok(0)
}

// === remove ==========================================================

/// Remove our shim if present; a RC 0 no-op (with a note) otherwise.
fn remove(args: &AliasArgs, path_entries: &[PathBuf]) -> Result<u8> {
    // Prefer the explicit --dir; otherwise scan PATH for a shim of ours.
    let candidates = match &args.dir {
        Some(dir) => vec![alias_shim::shim_path(dir, &args.name)],
        None => path_entries
            .iter()
            .map(|d| alias_shim::shim_path(d, &args.name))
            .collect(),
    };

    for shim in &candidates {
        let deleted = alias_shim::remove_if_ours(shim)
            .with_context(|| format!("removing shim {}", shim.display()))?;
        if deleted {
            println!("removed '{}' at {}", args.name, shim.display());
            return Ok(0);
        }
    }
    eprintln!("walrus: no '{}' shim of ours to remove", args.name);
    Ok(0)
}

// === status ==========================================================

/// Report whether a `wal` is on PATH and whether it is ours or foreign.
fn status(args: &AliasArgs, path_entries: &[PathBuf]) -> Result<u8> {
    let search: Vec<PathBuf> = match &args.dir {
        Some(dir) => vec![dir.clone()],
        None => path_entries.to_vec(),
    };
    match find_on_path(&args.name, &search) {
        Some(found) => match alias_shim::classify(&found) {
            ShimKind::Ours => println!("'{}' is present at {} (ours)", args.name, found.display()),
            ShimKind::Foreign => println!(
                "'{}' is present at {} (not ours / foreign)",
                args.name,
                found.display()
            ),
        },
        None => println!("'{}' is not present on the searched PATH", args.name),
    }
    Ok(0)
}

// === resolution ======================================================

/// Find the first `name` shim across the given directories, if any.
fn find_on_path(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = alias_shim::shim_path(dir, name);
        if alias_shim::exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// The target directory for a new shim: `--dir` if given, else the first
/// user-writable PATH entry (CLI-DESIGN section 4).
fn resolve_target_dir(args: &AliasArgs, path_entries: &[PathBuf]) -> Result<PathBuf, CliError> {
    if let Some(dir) = &args.dir {
        return Ok(dir.clone());
    }
    for dir in path_entries {
        if is_writable_dir(dir) {
            return Ok(dir.clone());
        }
    }
    Err(CliError::usage(
        "no writable directory on PATH; pass --dir <dir> to choose one",
    ))
}

/// True when `dir` is an existing directory we can create a file in.
fn is_writable_dir(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let probe = dir.join(".walrus-alias-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe); // best-effort cleanup
            true
        }
        Err(_) => false,
    }
}

/// The PATH entries to search: the injected `WALRUS_ALIAS_PATH` override if set
/// (used by tests to avoid the real `$PATH`), else the real `PATH`.
fn injected_or_real_path() -> Vec<PathBuf> {
    let var = std::env::var_os("WALRUS_ALIAS_PATH")
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    std::env::split_paths(&var).collect()
}

/// The path of the running `walrus` binary (the shim's forward target).
fn current_binary() -> Result<PathBuf> {
    std::env::current_exe().context("locating the running walrus binary")
}

// === parsing =========================================================

/// Parse `alias <install|remove|status> [--name <alias>] [--dir <dir>] [--force]`.
fn parse(tokens: &[String]) -> Result<AliasArgs, CliError> {
    let mut iter = tokens.iter().peekable();
    let action = match iter.next().map(String::as_str) {
        Some("install") => Action::Install,
        Some("remove") => Action::Remove,
        Some("status") => Action::Status,
        Some(other) => {
            return Err(CliError::usage(format!(
                "unknown alias action '{other}' (expected install|remove|status)"
            )))
        }
        None => return Err(CliError::usage(
            "usage: walrus alias <install|remove|status> [--name <alias>] [--dir <dir>] [--force]",
        )),
    };

    let mut name = String::from("wal");
    let mut dir: Option<PathBuf> = None;
    let mut force = false;

    while let Some(token) = iter.next() {
        if !token.starts_with('-') {
            return Err(CliError::usage(format!("unexpected argument '{token}'")));
        }
        let (opt, inline) = split_option(token);
        match opt {
            "--name" => name = take_value("--name", inline, &mut iter)?,
            "--dir" => dir = Some(PathBuf::from(take_value("--dir", inline, &mut iter)?)),
            "--force" => force = true,
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    Ok(AliasArgs {
        action,
        name,
        dir,
        force,
    })
}
