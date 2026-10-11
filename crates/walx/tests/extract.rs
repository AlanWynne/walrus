//! Behaviour tests for `walx extract` over fixture archives (AC1, AC2, 5.4d).

mod common;

use common::{code_of, run_walx, Fixture};
use tempfile::TempDir;
use walrus_fmt::{Archive, Manifest, Variant};

/// Read an extracted file's bytes from a path beneath `out`.
fn read_out(out: &std::path::Path, rel: &[&str]) -> Vec<u8> {
    let mut path = out.to_path_buf();
    for seg in rel {
        path.push(seg);
    }
    std::fs::read(&path).unwrap_or_else(|_| panic!("read {}", path.display()))
}

#[test]
fn extract_all_files_restores_under_stored_names_into_output_dir_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(fx.dir.path(), &["extract", &archive, "-o", &out_arg]);
    assert_eq!(code_of(&result), 0, "extract-all succeeds");
    assert_eq!(read_out(out.path(), &["notes.txt"]), b"the cats");
    assert_eq!(read_out(out.path(), &["src", "main.rs"]), b"fn main() {}");
}

#[test]
fn extract_mask_selects_a_subset() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &["extract", &archive, "notes.txt", "-o", &out_arg],
    );
    assert_eq!(code_of(&result), 0);
    assert_eq!(read_out(out.path(), &["notes.txt"]), b"the cats");
    assert!(
        !out.path().join("src").join("main.rs").exists(),
        "the mask excluded src/main.rs"
    );
}

#[test]
fn extract_recreates_nested_relative_paths_beneath_output_dir() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &["extract", &archive, "src/**", "-o", &out_arg],
    );
    assert_eq!(code_of(&result), 0);
    assert!(
        out.path().join("src").join("main.rs").is_file(),
        "nested relative path recreated"
    );
}

#[test]
fn extract_existing_target_is_refused_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();
    // Pre-create the target so extract must refuse it.
    std::fs::write(out.path().join("notes.txt"), b"existing").expect("pre-write");

    let result = run_walx(
        fx.dir.path(),
        &["extract", &archive, "notes.txt", "-o", &out_arg],
    );
    assert_eq!(
        code_of(&result),
        2,
        "existing target refused without --force"
    );
    assert_eq!(
        read_out(out.path(), &["notes.txt"]),
        b"existing",
        "the existing file is untouched"
    );
}

#[test]
fn extract_force_overwrites_existing_target_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();
    std::fs::write(out.path().join("notes.txt"), b"existing").expect("pre-write");

    let result = run_walx(
        fx.dir.path(),
        &["extract", &archive, "notes.txt", "-o", &out_arg, "--force"],
    );
    assert_eq!(code_of(&result), 0, "--force overwrites");
    assert_eq!(read_out(out.path(), &["notes.txt"]), b"the cats");
}

#[test]
fn extract_index_beyond_last_writes_original_and_warns_rc4() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    // notes.txt count is 2; index 9 is beyond last -> original + warn + RC 4.
    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--index",
            "9",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 4, "beyond-last numeric index exits RC 4");
    assert_eq!(
        read_out(out.path(), &["notes.txt"]),
        b"cat",
        "the original version is written"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("notes.txt"), "warning names the file");
}

#[test]
fn sanitize_extracts_remapped_names_and_reports_on_stderr() {
    // Build an archive whose stored name is unrepresentable on Windows (a `:`),
    // then prove --sanitize rewrites it, writes the bytes, and reports the remap.
    let dir = TempDir::new().expect("temp dir");
    let archive_path = dir.path().join("odd.wal");
    let mut a = Archive::new(Manifest::new(Variant::Wal));
    a.add("weird:name.txt", b"payload", None, None)
        .expect("add odd name");
    a.write(&archive_path).expect("write odd fixture");
    let archive = archive_path.to_string_lossy().into_owned();

    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();
    let result = run_walx(
        dir.path(),
        &["extract", &archive, "-o", &out_arg, "--sanitize"],
    );
    assert_eq!(code_of(&result), 0, "sanitize writes the file (RC 0)");
    assert_eq!(read_out(out.path(), &["weird_name.txt"]), b"payload");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("weird_name.txt"),
        "the remap is reported on stderr"
    );
}

#[test]
fn default_refuses_unrepresentable_name_rc2() {
    // Without --sanitize the unrepresentable name is refused (RC 2) and nothing
    // is written for it. Pair of the sanitize test; proves the default policy.
    let dir = TempDir::new().expect("temp dir");
    let archive_path = dir.path().join("odd.wal");
    let mut a = Archive::new(Manifest::new(Variant::Wal));
    a.add("weird:name.txt", b"payload", None, None)
        .expect("add odd name");
    a.write(&archive_path).expect("write odd fixture");
    let archive = archive_path.to_string_lossy().into_owned();

    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();
    let result = run_walx(dir.path(), &["extract", &archive, "-o", &out_arg]);
    assert_eq!(
        code_of(&result),
        2,
        "unrepresentable name refused by default"
    );
    assert!(
        !out.path().join("weird_name.txt").exists(),
        "no file is written on refusal"
    );
}

/// A stored name with a `..` component must be refused even under --sanitize
/// (zip-slip is never relaxed). The fixture is built with a `..`-bearing name
/// through the public add API, which stores the name verbatim.
#[test]
fn zip_slip_name_is_refused_even_with_sanitize_rc2() {
    let dir = TempDir::new().expect("temp dir");
    let archive_path = dir.path().join("evil.wal");
    let mut a = Archive::new(Manifest::new(Variant::Wal));
    a.add("../escape.txt", b"gotcha", None, None)
        .expect("add escaping name");
    a.write(&archive_path).expect("write evil fixture");
    let archive = archive_path.to_string_lossy().into_owned();

    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();
    let result = run_walx(
        dir.path(),
        &["extract", &archive, "-o", &out_arg, "--sanitize"],
    );
    assert_eq!(code_of(&result), 2, "zip-slip refused even with --sanitize");
    // The escape target one level above `out` must not exist.
    let parent = out.path().parent().expect("out has a parent");
    assert!(
        !parent.join("escape.txt").exists(),
        "nothing was written outside the output directory"
    );
}
