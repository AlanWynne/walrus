//! Shared fixtures for the walx behaviour tests.
//!
//! Each fixture builds a deterministic multi-file archive through the PUBLIC
//! default-feature `walrus-fmt` Archive API and writes it under a
//! `tempfile::TempDir`. No wall clock, no external files: content is fixed byte
//! literals and names are fixed, so every assertion is reproducible.
//!
//! Each integration-test binary compiles this module independently, so a helper
//! used by only some of them is dead code in the others; allow that here.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;
use walrus_fmt::{Archive, ChangeRecord, Manifest, Variant};

/// A built fixture archive on disk, keeping its [`TempDir`] alive.
pub struct Fixture {
    /// The temp dir backing the archive (and any extract target).
    pub dir: TempDir,
    /// Path to the written `.wal` archive inside `dir`.
    pub archive: PathBuf,
}

impl Fixture {
    /// Build the canonical multi-file fixture:
    /// - `notes.txt`: current "the cats" (index 0), one older index 1 "cats",
    ///   original (index 2) "cat".
    /// - `src/main.rs`: a nested name, single version "fn main() {}" at index 0.
    ///
    /// The nested name exercises localize-on-read and segment-aware masking.
    pub fn build() -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        let archive = dir.path().join("fixture.wal");

        let mut a = Archive::new(Manifest::new(Variant::Wal));
        // notes.txt: original "cat" -> "cats" -> "the cats"; index 0 is current.
        a.add("notes.txt", b"cat", None, None).expect("add notes");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(3, 0, "s")])
            .expect("notes save 1");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(0, 0, "the ")])
            .expect("notes save 2");
        // src/main.rs: a single-version nested file.
        a.add("src/main.rs", b"fn main() {}", None, None)
            .expect("add nested");
        a.write(&archive).expect("write fixture");

        Fixture { dir, archive }
    }

    /// Build an archive with ZERO tracked files (for the empty-archive RC-8 case).
    pub fn empty() -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        let archive = dir.path().join("empty.wal");
        let a = Archive::new(Manifest::new(Variant::Wal));
        a.write(&archive).expect("write empty fixture");
        Fixture { dir, archive }
    }
}

/// Spawn the built `walx` binary with `args`, returning its captured output.
/// The working directory is set to `cwd` so extract's default output dir and
/// the "never touches disk" assertions are deterministic.
pub fn run_walx(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_walx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn walx")
}

/// The integer exit code of a finished process (asserting it was not signalled).
pub fn code_of(output: &std::process::Output) -> i32 {
    output.status.code().expect("process returned an exit code")
}
