//! Shared fixtures for the walrus-cli behaviour tests.
//!
//! Each fixture builds a deterministic multi-file archive through the PUBLIC
//! default-feature `walrus-fmt` Archive API and writes it under a
//! `tempfile::TempDir`. No wall clock, no external files: content is fixed byte
//! literals, names are fixed, and any timestamps are fixed strings, so every
//! assertion is reproducible. The live `walrus add`/`update` path is the ONLY
//! clock reader; the fixtures never stamp.
//!
//! Each integration-test binary compiles this module independently, so a helper
//! used by only some of them is dead code in the others; allow that here.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use walrus_fmt::{Archive, ChangeRecord, FileId, Manifest, Variant, Version};

/// A built fixture archive on disk, keeping its [`TempDir`] alive.
pub struct Fixture {
    /// The temp dir backing the archive (and any extract target).
    pub dir: TempDir,
    /// Path to the written archive inside `dir`.
    pub archive: PathBuf,
}

impl Fixture {
    /// Build the canonical multi-file fixture:
    /// - `notes.txt`: current "the cats" (index 0), index 1 "cats", original
    ///   (index 2) "cat".
    /// - `src/main.rs`: a nested single-version name "fn main() {}" at index 0.
    pub fn build() -> Fixture {
        Self::build_named("fixture.wal", Variant::Wal)
    }

    /// Build the canonical fixture under a chosen file name and variant.
    pub fn build_named(file: &str, variant: Variant) -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        let archive = dir.path().join(file);

        let mut a = Archive::new(Manifest::new(variant));
        a.add("notes.txt", b"cat", None, None).expect("add notes");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(3, 0, "s")])
            .expect("notes save 1");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(0, 0, "the ")])
            .expect("notes save 2");
        a.add("src/main.rs", b"fn main() {}", None, None)
            .expect("add nested");
        a.write(&archive).expect("write fixture");

        Fixture { dir, archive }
    }

    /// Build an archive with ZERO tracked files (for empty / create cases).
    pub fn empty() -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        let archive = dir.path().join("empty.wal");
        let a = Archive::new(Manifest::new(Variant::Wal));
        a.write(&archive).expect("write empty fixture");
        Fixture { dir, archive }
    }

    /// The archive path as a string argument.
    pub fn archive_arg(&self) -> String {
        self.archive.to_string_lossy().into_owned()
    }
}

/// Spawn the built `walrus` binary with `args`, returning its captured output.
/// The working directory is set to `cwd` so default output dirs and the "never
/// touches disk" assertions are deterministic.
pub fn run_walrus(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_walrus"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn walrus")
}

/// Spawn `walrus` with `stdin_bytes` piped to its standard input.
pub fn run_walrus_stdin(cwd: &Path, args: &[&str], stdin_bytes: &[u8]) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_walrus"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn walrus");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin_bytes)
        .expect("write stdin");
    child.wait_with_output().expect("wait walrus")
}

/// The integer exit code of a finished process (asserting it was not signalled).
pub fn code_of(output: &std::process::Output) -> i32 {
    output.status.code().expect("process returned an exit code")
}

/// Open an archive on disk and reconstruct one tracked file at `version`.
/// Used to verify a mutation round-trips (the walx-extract round-trip of AC7b
/// is equivalent to reconstructing through the shared library API).
pub fn reconstruct(archive: &Path, name: &str, version: Version) -> Vec<u8> {
    let a = Archive::open(archive).expect("open archive");
    let tracked = a.file(name).expect("tracked file present");
    let id = FileId::from(tracked.id.as_str());
    a.reconstruct(&id, version).expect("reconstruct")
}

/// The number of change records of a tracked file in an on-disk archive.
pub fn record_count(archive: &Path, name: &str) -> usize {
    let a = Archive::open(archive).expect("open archive");
    a.records(name).map(|r| r.len()).unwrap_or(0)
}

/// The `ts` of the newest (index-0) change record of a tracked file, if any.
pub fn first_ts(archive: &Path, name: &str) -> Option<String> {
    let a = Archive::open(archive).expect("open archive");
    a.records(name)
        .and_then(|r| r.first())
        .and_then(|rec| rec.ts.clone())
}
