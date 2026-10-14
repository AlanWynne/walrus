//! On-disk tests for the FR-K6 per-file + batch mutate orchestration on
//! `WalrusSource` (AC1 + the transactional guarantee).
//!
//! These build a REAL single- or two-file `.wal` container in a `TempDir`
//! (ustar tar + zstd, the same fixture shape as `tests/import.rs`), run a
//! `WalrusSource` mutate method, then RE-OPEN the archive from disk and query
//! it to prove the mutation persisted transactionally. Each test is
//! deterministic and uses `tempfile::TempDir`; sentence-style names.

use std::path::{Path, PathBuf};

use walker::source::{HistorySource, VersionId, WalrusSource};
use walrus_fmt::{ChangeRecord, Direction, Manifest, TrackedFile, Variant, WchgHeader};

/// Append one tar member.
fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) {
    let mut header = tar::Header::new_ustar();
    header.set_path(path).expect("path");
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder.append(&header, data).expect("append");
}

/// Serialize a `.wchg` member (header line + one line per record).
fn wchg_bytes(name: &str, count: usize, records: &[ChangeRecord]) -> Vec<u8> {
    let header = WchgHeader::new(Direction::Forward, 0, name, "utf-8", count);
    let mut wchg = serde_json::to_string(&header).expect("header");
    wchg.push('\n');
    for record in records {
        wchg.push_str(&serde_json::to_string(record).expect("record"));
        wchg.push('\n');
    }
    wchg.into_bytes()
}

/// Build and write a single forward `.wal` with `base` + `records`, returning
/// the path. `id`/`file` name the one tracked file.
fn single_file_archive(
    dir: &Path,
    archive_name: &str,
    id: &str,
    file: &str,
    base: &[u8],
    records: &[ChangeRecord],
) -> PathBuf {
    let count = records.iter().filter(|r| r.is_splice()).count();
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(TrackedFile::new_with_direction(
        id,
        file,
        Direction::Forward,
        count,
    ));

    let mut builder = tar::Builder::new(Vec::new());
    append(
        &mut builder,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).expect("m"),
    );
    append(&mut builder, &format!("files/{id}/base"), base);
    append(
        &mut builder,
        &format!("files/{id}/{file}.wchg"),
        &wchg_bytes(file, count, records),
    );
    let tar_bytes = builder.into_inner().expect("tar");
    let bytes = zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd");

    let path = dir.join(archive_name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

/// Build and write a two-file forward `.wal`, each file base-only (no splices),
/// returning the path.
fn two_file_archive(dir: &Path, archive_name: &str) -> PathBuf {
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(TrackedFile::new_with_direction(
        "a0",
        "a.txt",
        Direction::Forward,
        0,
    ));
    manifest.files.push(TrackedFile::new_with_direction(
        "b1",
        "b.txt",
        Direction::Forward,
        0,
    ));

    let mut builder = tar::Builder::new(Vec::new());
    append(
        &mut builder,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).expect("m"),
    );
    append(&mut builder, "files/a0/base", b"aaa");
    append(
        &mut builder,
        "files/a0/a.txt.wchg",
        &wchg_bytes("a.txt", 0, &[]),
    );
    append(&mut builder, "files/b1/base", b"bbb");
    append(
        &mut builder,
        "files/b1/b.txt.wchg",
        &wchg_bytes("b.txt", 0, &[]),
    );
    let tar_bytes = builder.into_inner().expect("tar");
    let bytes = zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd");

    let path = dir.join(archive_name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

#[test]
fn set_generations_updates_the_cap_in_the_reopened_archive() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = single_file_archive(dir.path(), "doc.wal", "f0", "a.txt", b"hi", &[]);
    let mut source = WalrusSource::open(&path).expect("open");
    let file = source.files().into_iter().next().expect("a.txt");

    source
        .set_generations(&file, Some(2), &path)
        .expect("set generations");

    let reopened = WalrusSource::open(&path).expect("re-open");
    let file = reopened.files().into_iter().next().expect("a.txt");
    assert_eq!(
        reopened.file_info(&file).generations,
        Some(2),
        "the generation cap persists transactionally"
    );
}

#[test]
fn condense_file_to_keep_zero_shrinks_the_index_size_to_one() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // base "hi" + two splices -> index_size 3 (indices [0, 2]).
    let records = [
        ChangeRecord::splice(2, 0, "!"),
        ChangeRecord::splice(3, 0, "?"),
    ];
    let path = single_file_archive(dir.path(), "doc.wal", "f0", "a.txt", b"hi", &records);
    let before = WalrusSource::open(&path).expect("open");
    let file = before.files().into_iter().next().expect("a.txt");
    assert_eq!(before.file_info(&file).index_size, 3, "precondition");

    let mut source = WalrusSource::open(&path).expect("open");
    let file = source.files().into_iter().next().expect("a.txt");
    source.condense_file(&file, 0, &path).expect("condense");

    let reopened = WalrusSource::open(&path).expect("re-open");
    let file = reopened.files().into_iter().next().expect("a.txt");
    assert_eq!(
        reopened.file_info(&file).index_size,
        1,
        "keep 0 collapses to current-only (index_size 1)"
    );
}

#[test]
fn remove_file_drops_the_file_from_the_reopened_listing() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = two_file_archive(dir.path(), "two.wal");
    let mut source = WalrusSource::open(&path).expect("open");
    let a = source
        .files()
        .into_iter()
        .find(|f| f.name() == "a.txt")
        .expect("a.txt");

    source.remove_file(&a, &path).expect("remove a.txt");

    let reopened = WalrusSource::open(&path).expect("re-open");
    let names: Vec<String> = reopened
        .files()
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    assert!(
        !names.contains(&"a.txt".to_string()),
        "a.txt is gone on disk"
    );
    assert!(names.contains(&"b.txt".to_string()), "b.txt survives");
}

#[test]
fn remove_files_of_two_files_removes_both_on_disk() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = two_file_archive(dir.path(), "two.wal");
    let mut source = WalrusSource::open(&path).expect("open");
    // Both ids: 0 = a.txt, 1 = b.txt.
    source.remove_files(&[0, 1], &path).expect("batch remove");

    let reopened = WalrusSource::open(&path).expect("re-open");
    assert!(
        reopened.files().is_empty(),
        "a batch remove of both files empties the listing"
    );
}

#[test]
fn condense_files_batch_shrinks_every_selected_files_index_size() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // Both files get one splice so each has index_size 2 before condense.
    let records = [ChangeRecord::splice(3, 0, "!")];
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(TrackedFile::new_with_direction(
        "a0",
        "a.txt",
        Direction::Forward,
        1,
    ));
    manifest.files.push(TrackedFile::new_with_direction(
        "b1",
        "b.txt",
        Direction::Forward,
        1,
    ));
    let mut builder = tar::Builder::new(Vec::new());
    append(
        &mut builder,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).expect("m"),
    );
    append(&mut builder, "files/a0/base", b"aaa");
    append(
        &mut builder,
        "files/a0/a.txt.wchg",
        &wchg_bytes("a.txt", 1, &records),
    );
    append(&mut builder, "files/b1/base", b"bbb");
    append(
        &mut builder,
        "files/b1/b.txt.wchg",
        &wchg_bytes("b.txt", 1, &records),
    );
    let tar_bytes = builder.into_inner().expect("tar");
    let bytes = zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd");
    let path = dir.path().join("two.wal");
    std::fs::write(&path, bytes).expect("write fixture");

    let mut source = WalrusSource::open(&path).expect("open");
    source
        .condense_files(&[0, 1], 0, &path)
        .expect("batch condense");

    let reopened = WalrusSource::open(&path).expect("re-open");
    for file in reopened.files() {
        assert_eq!(
            reopened.file_info(&file).index_size,
            1,
            "{} collapsed to current-only",
            file.name()
        );
    }
}

#[test]
fn export_via_version_at_zero_matches_a_bare_index_zero_for_walrus() {
    // The FR-K6 export now reconstructs the current version through
    // `source.version_at(file, 0)` rather than a bare `VersionId::Index(0)`.
    // For a walrus source `version_at` returns `Index(0)` (the trait default),
    // so the two MUST produce byte-identical current-version output -- this
    // guards the export's behaviour-identity after the player-indirection
    // change (FEAT-002, design item 9).
    let dir = tempfile::TempDir::new().expect("tempdir");
    let records = [ChangeRecord::splice(2, 0, "!")];
    let path = single_file_archive(dir.path(), "doc.wal", "f0", "a.txt", b"hi", &records);
    let source = WalrusSource::open(&path).expect("open");
    let file = source.files().into_iter().next().expect("a.txt");

    let via_version_at = source
        .reconstruct(&file, &source.version_at(&file, 0))
        .expect("reconstruct via version_at(0)");
    let via_index_zero = source
        .reconstruct(&file, &VersionId::Index(0))
        .expect("reconstruct via Index(0)");
    assert_eq!(
        via_version_at, via_index_zero,
        "walrus version_at(0) and Index(0) reconstruct identically"
    );
    assert_eq!(
        via_version_at, b"hi!",
        "the current version is base + splice"
    );
}

#[test]
fn a_mutate_that_errors_leaves_the_archive_byte_for_byte_unchanged() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = single_file_archive(dir.path(), "doc.wal", "f0", "a.txt", b"hi", &[]);
    let snapshot = std::fs::read(&path).expect("snapshot");

    let mut source = WalrusSource::open(&path).expect("open");
    // A foreign ref (id out of range) is rejected before any write, so the
    // on-disk archive must be untouched (the transactional guarantee).
    let foreign = walker::source::FileRef::new(99, "ghost.txt");
    let result = source.set_generations(&foreign, Some(3), &path);
    assert!(result.is_err(), "a foreign ref errors");
    assert_eq!(
        std::fs::read(&path).expect("re-read"),
        snapshot,
        "a failed mutate leaves the archive byte-for-byte unchanged"
    );

    // The reconstructed current version is also unchanged.
    let reopened = WalrusSource::open(&path).expect("re-open");
    let file = reopened.files().into_iter().next().expect("a.txt");
    assert_eq!(
        reopened
            .reconstruct(&file, &VersionId::Index(0))
            .expect("current"),
        b"hi"
    );
}
