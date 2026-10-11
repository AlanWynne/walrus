//! `add` behaviour over fixtures (FR-W11 AC5, AC7a, AC8, CLI-DESIGN 5.3/5.4).

mod common;

use common::{code_of, first_ts, reconstruct, record_count, run_walrus};
use tempfile::TempDir;
use walrus_fmt::{Archive, Direction, Version};

/// Write a file under `dir` and return its RELATIVE name (the arg the CLI sees
/// with cwd = dir, so the stored name is exactly that relative name).
fn write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    std::fs::write(dir.join(name), bytes).expect("write input file");
    name.to_string()
}

#[test]
fn add_creates_new_tracked_file_then_cat_reproduces_bytes() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("new.wal").to_string_lossy().into_owned();
    let input = write_file(dir.path(), "hello.txt", b"hello world");

    let result = run_walrus(dir.path(), &["add", &archive, &input]);
    assert_eq!(code_of(&result), 0, "add creates the archive and the file");

    let name = stored(&input);
    assert_eq!(
        reconstruct(std::path::Path::new(&archive), &name, Version::Current),
        b"hello world"
    );
}

#[test]
fn add_second_time_with_changed_content_records_a_new_version() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("new.wal").to_string_lossy().into_owned();
    let input = write_file(dir.path(), "doc.txt", b"one");
    run_walrus(dir.path(), &["add", &archive, &input]);
    // Change the on-disk file and add again.
    std::fs::write(dir.path().join("doc.txt"), b"one two").expect("rewrite");
    let result = run_walrus(dir.path(), &["add", &archive, &input]);
    assert_eq!(code_of(&result), 0);

    let name = stored(&input);
    let path = std::path::Path::new(&archive);
    assert_eq!(record_count(path, &name), 1, "one new version recorded");
    assert_eq!(reconstruct(path, &name, Version::Current), b"one two");
    assert_eq!(reconstruct(path, &name, Version::Index(1)), b"one");
}

#[test]
fn add_no_change_save_is_rc0_no_new_version_with_stderr_note() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("new.wal").to_string_lossy().into_owned();
    let input = write_file(dir.path(), "doc.txt", b"same");
    run_walrus(dir.path(), &["add", &archive, &input]);
    // Add the identical content again: no new version, RC 0, a stderr note.
    let result = run_walrus(dir.path(), &["add", &archive, &input]);
    assert_eq!(code_of(&result), 0);
    let name = stored(&input);
    assert_eq!(record_count(std::path::Path::new(&archive), &name), 0);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("no change"), "a no-op prints a note");
}

#[test]
fn add_brand_new_archive_seed_order_flag_then_extension_then_default() {
    let dir = TempDir::new().expect("tmp");
    let input = write_file(dir.path(), "doc.txt", b"x");

    // 1) Explicit flag wins over extension: .wal path but --wbl -> reverse.
    let a1 = dir.path().join("flag.wal").to_string_lossy().into_owned();
    run_walrus(dir.path(), &["add", &a1, &input, "--wbl"]);
    assert_eq!(file_direction(&a1, &stored(&input)), Direction::Reverse);

    // 2) No flag: extension decides. .wbl -> reverse.
    let a2 = dir.path().join("ext.wbl").to_string_lossy().into_owned();
    run_walrus(dir.path(), &["add", &a2, &input]);
    assert_eq!(file_direction(&a2, &stored(&input)), Direction::Reverse);

    // 3) No flag, no known extension: default forward.
    let a3 = dir.path().join("plain").to_string_lossy().into_owned();
    run_walrus(dir.path(), &["add", &a3, &input]);
    assert_eq!(file_direction(&a3, &stored(&input)), Direction::Forward);
}

#[test]
fn add_direction_flag_ignored_for_already_tracked_file() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("f.wal").to_string_lossy().into_owned();
    let input = write_file(dir.path(), "doc.txt", b"one");
    // Create forward.
    run_walrus(dir.path(), &["add", &archive, &input, "--wal"]);
    // Change content and add with --wbl: direction must stay forward.
    std::fs::write(dir.path().join("doc.txt"), b"one two").expect("rewrite");
    run_walrus(dir.path(), &["add", &archive, &input, "--wbl"]);
    assert_eq!(
        file_direction(&archive, &stored(&input)),
        Direction::Forward,
        "the flag is ignored for an existing file"
    );
}

#[test]
fn add_directory_without_recurse_is_rc2() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("f.wal").to_string_lossy().into_owned();
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    let sub = dir.path().join("sub").to_string_lossy().into_owned();
    let result = run_walrus(dir.path(), &["add", &archive, &sub]);
    assert_eq!(code_of(&result), 2, "a directory without -R is RC 2");
}

#[test]
fn add_update_stamp_records_with_fixed_shape_utc_timestamp() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("f.wal").to_string_lossy().into_owned();
    let input = write_file(dir.path(), "doc.txt", b"one");
    run_walrus(dir.path(), &["add", &archive, &input]);
    std::fs::write(dir.path().join("doc.txt"), b"one two").expect("rewrite");
    run_walrus(dir.path(), &["add", &archive, &input]);

    let name = stored(&input);
    let ts = first_ts(std::path::Path::new(&archive), &name).expect("a recorded ts");
    assert_eq!(ts.len(), 20, "fixed-width UTC shape");
    assert!(ts.ends_with('Z'), "trailing Z");
    assert_eq!(&ts[4..5], "-");
    assert_eq!(&ts[10..11], "T");
}

/// The stored name equals the relative file argument (normalize-on-write turns
/// an OS `\` into `/`, but these fixtures use flat names, so it is identity).
fn stored(arg: &str) -> String {
    arg.to_string()
}

/// The replay direction of a tracked file in an on-disk archive.
fn file_direction(archive: &str, name: &str) -> Direction {
    let a = Archive::open(archive).expect("open");
    a.file(name).expect("tracked").direction
}
