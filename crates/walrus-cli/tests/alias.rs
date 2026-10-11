//! `walrus alias` behaviour over fixtures (FR-W11, CLI-DESIGN section 4).
//!
//! Every test is TempDir-only and injects a FAKE PATH through the
//! `WALRUS_ALIAS_PATH` environment variable set ONLY on the spawned child; the
//! real `$PATH` is never read or mutated, and no shim is ever created outside a
//! `tempfile::TempDir`.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Spawn `walrus` with a fake PATH injected via `WALRUS_ALIAS_PATH`. The real
/// process environment is untouched: the override is set only on this child.
fn run_alias(cwd: &Path, fake_path: &[&Path], args: &[&str]) -> std::process::Output {
    let joined = join_paths(fake_path);
    Command::new(env!("CARGO_BIN_EXE_walrus"))
        .args(args)
        .env("WALRUS_ALIAS_PATH", joined)
        .current_dir(cwd)
        .output()
        .expect("spawn walrus")
}

/// Join paths with the platform PATH separator.
fn join_paths(entries: &[&Path]) -> std::ffi::OsString {
    std::env::join_paths(entries.iter().map(|p| p.as_os_str())).expect("join fake PATH")
}

/// The integer exit code of a finished process.
fn code_of(output: &std::process::Output) -> i32 {
    output.status.code().expect("process returned an exit code")
}

/// A platform-appropriate shim file name for `name` inside `dir`.
fn shim_path(dir: &Path, name: &str) -> std::path::PathBuf {
    if cfg!(windows) {
        dir.join(format!("{name}.cmd"))
    } else {
        dir.join(name)
    }
}

#[test]
fn alias_install_creates_shim_in_target_dir_rc0() {
    let bin = TempDir::new().expect("bin dir");
    let out = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "install", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(code_of(&out), 0, "install into a clean dir succeeds");
    assert!(
        shim_path(bin.path(), "wal").exists(),
        "a `wal` shim was created in the target dir"
    );
}

#[test]
fn alias_install_refuses_foreign_wal_on_path_rc2_creating_nothing() {
    let foreign_dir = TempDir::new().expect("foreign dir");
    let target = TempDir::new().expect("target dir");
    // A foreign `wal` already on the (fake) PATH, not created by us.
    let foreign = shim_path(foreign_dir.path(), "wal");
    fs::write(&foreign, b"#!/bin/sh\necho pywal\n").expect("write foreign wal");

    let out = run_alias(
        target.path(),
        &[foreign_dir.path(), target.path()],
        &[
            "alias",
            "install",
            "--dir",
            &target.path().to_string_lossy(),
        ],
    );
    assert_eq!(code_of(&out), 2, "a foreign wal on PATH refuses install");
    assert!(
        !shim_path(target.path(), "wal").exists(),
        "nothing was created in the target dir"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&foreign.to_string_lossy().to_string()),
        "the refusal names the owning path, got: {stderr}"
    );
}

#[test]
fn alias_install_refuses_foreign_even_with_force() {
    let foreign_dir = TempDir::new().expect("foreign dir");
    let target = TempDir::new().expect("target dir");
    let foreign = shim_path(foreign_dir.path(), "wal");
    fs::write(&foreign, b"#!/bin/sh\necho pywal\n").expect("write foreign wal");

    let out = run_alias(
        target.path(),
        &[foreign_dir.path(), target.path()],
        &[
            "alias",
            "install",
            "--dir",
            &target.path().to_string_lossy(),
            "--force",
        ],
    );
    assert_eq!(code_of(&out), 2, "--force still refuses a foreign wal");
    assert!(
        !shim_path(target.path(), "wal").exists(),
        "--force creates nothing when a foreign wal exists"
    );
}

#[test]
fn alias_install_repoints_our_own_prior_shim_idempotently() {
    let bin = TempDir::new().expect("bin dir");
    let first = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "install", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(code_of(&first), 0, "first install succeeds");

    // Second install must re-point our own shim, not refuse, and not duplicate.
    let second = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "install", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(
        code_of(&second),
        0,
        "re-installing our own shim is idempotent"
    );
    assert!(
        shim_path(bin.path(), "wal").exists(),
        "the shim is still present after re-point"
    );
    let count = fs::read_dir(bin.path())
        .expect("read bin dir")
        .filter(|e| {
            e.as_ref()
                .map(|e| e.file_name().to_string_lossy().starts_with("wal"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 1, "no duplicate shim was created");
}

#[test]
fn alias_remove_deletes_only_our_shim() {
    let bin = TempDir::new().expect("bin dir");
    let install = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "install", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(code_of(&install), 0, "install first");

    let remove = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "remove", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(code_of(&remove), 0, "removing our own shim succeeds");
    assert!(
        !shim_path(bin.path(), "wal").exists(),
        "our shim is gone after remove"
    );

    // A second remove with nothing of ours is a RC 0 no-op.
    let again = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "remove", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(
        code_of(&again),
        0,
        "remove with nothing of ours is a RC 0 no-op"
    );
}

#[test]
fn alias_remove_never_touches_a_foreign_wal() {
    let bin = TempDir::new().expect("bin dir");
    let foreign = shim_path(bin.path(), "wal");
    fs::write(&foreign, b"#!/bin/sh\necho pywal\n").expect("write foreign wal");

    let remove = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "remove", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(
        code_of(&remove),
        0,
        "remove is a no-op when only a foreign wal exists"
    );
    assert!(foreign.exists(), "the foreign wal is left untouched");
}

#[test]
fn alias_status_reports_ours_vs_foreign_vs_absent() {
    // Absent.
    let empty = TempDir::new().expect("empty dir");
    let absent = run_alias(empty.path(), &[empty.path()], &["alias", "status"]);
    assert_eq!(code_of(&absent), 0, "status is RC 0 when absent");
    assert!(
        String::from_utf8_lossy(&absent.stdout)
            .to_lowercase()
            .contains("not"),
        "absent status says no wal is present"
    );

    // Foreign.
    let foreign_dir = TempDir::new().expect("foreign dir");
    let foreign = shim_path(foreign_dir.path(), "wal");
    fs::write(&foreign, b"#!/bin/sh\necho pywal\n").expect("write foreign wal");
    let foreign_status = run_alias(
        foreign_dir.path(),
        &[foreign_dir.path()],
        &["alias", "status"],
    );
    assert_eq!(
        code_of(&foreign_status),
        0,
        "status is RC 0 for a foreign wal"
    );
    assert!(
        String::from_utf8_lossy(&foreign_status.stdout)
            .to_lowercase()
            .contains("not ours")
            || String::from_utf8_lossy(&foreign_status.stdout)
                .to_lowercase()
                .contains("foreign"),
        "foreign status flags a wal we did not create"
    );

    // Ours.
    let bin = TempDir::new().expect("bin dir");
    let install = run_alias(
        bin.path(),
        &[bin.path()],
        &["alias", "install", "--dir", &bin.path().to_string_lossy()],
    );
    assert_eq!(code_of(&install), 0, "install first");
    let ours = run_alias(bin.path(), &[bin.path()], &["alias", "status"]);
    assert_eq!(code_of(&ours), 0, "status is RC 0 for our shim");
    assert!(
        String::from_utf8_lossy(&ours.stdout)
            .to_lowercase()
            .contains("ours"),
        "ours status reports the shim as ours"
    );
}
