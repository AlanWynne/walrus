//! Integration tests for the `WalrusSource` adapter (FR-K2).
//!
//! These exercise the FORMAT-AGNOSTIC `HistorySource` surface over REAL
//! multi-file archives, for BOTH a `.wal` and a `.wbl` fixture. The fixtures are
//! assembled IN MEMORY as a ustar tar (zstd-compressed) from the library's
//! public `Manifest` / `TrackedFile` / `WchgHeader` types -- mirroring the
//! in-crate `archive_read.rs` builder -- then written to a `tempfile::TempDir`
//! so nothing binary is committed and the run is deterministic.
//!
//! The adapter under test (`WalrusSource`) is the ONLY place allowed to touch
//! `walrus_fmt::Archive`; this test deliberately drives it through the
//! `HistorySource` trait so it asserts the format-agnostic contract, never the
//! archive internals.

use std::path::Path;

use walker::source::{HistorySource, VersionId, WalrusSource};
use walrus_fmt::{Direction, Manifest, TrackedFile, Variant, WchgHeader};

// === in-memory multi-file fixture builder ===============================
//
// A standalone container builder (mirrors `crates/walrus/tests/archive_read.rs`
// and the writer member layout) so the test does not depend on a from-parts
// `Archive` constructor, which is not public. It emits exactly the members the
// reader collects: `manifest.json` + per-file `files/<id>/base` +
// `files/<id>/<name>.wchg`.

/// One tracked file's fixture inputs.
struct FileFixture {
    id: &'static str,
    name: &'static str,
    direction: Direction,
    base: Vec<u8>,
    record_lines: Vec<String>,
}

impl FileFixture {
    fn tracked(&self) -> TrackedFile {
        TrackedFile::new_with_direction(self.id, self.name, self.direction, self.record_lines.len())
    }

    /// Build the `.wchg` member bytes: header line, then one record per line,
    /// every line LF-terminated (FORMAT.md 4.1).
    fn wchg_bytes(&self) -> Vec<u8> {
        let header = WchgHeader::new(
            self.direction,
            0,
            self.name,
            "utf-8",
            self.record_lines.len(),
        );
        let mut out = serde_json::to_string(&header).expect("serialise wchg header");
        out.push('\n');
        for line in &self.record_lines {
            out.push_str(line);
            out.push('\n');
        }
        out.into_bytes()
    }
}

/// A member to place in the tar: path + bytes.
struct Member {
    path: String,
    data: Vec<u8>,
}

/// Build an uncompressed ustar tar from the given members (fixed mode/mtime so
/// the output is deterministic, mirroring the writer's `append_member`).
fn build_tar(members: &[Member]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for member in members {
        let mut header = tar::Header::new_ustar();
        header.set_path(&member.path).expect("set member path");
        header.set_size(member.data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append(&header, &member.data[..])
            .expect("append member");
    }
    builder.into_inner().expect("finish tar")
}

/// zstd-compress the tar bytes, producing a full container (zstd magic).
fn zstd(tar_bytes: &[u8]) -> Vec<u8> {
    zstd::stream::encode_all(tar_bytes, 19).expect("zstd encode")
}

/// Assemble a complete zstd container from a manifest and per-file fixtures.
fn build_container(manifest: &Manifest, files: &[FileFixture]) -> Vec<u8> {
    let mut members = vec![Member {
        path: "manifest.json".to_string(),
        data: serde_json::to_vec_pretty(manifest).expect("serialise manifest"),
    }];
    for file in files {
        members.push(Member {
            path: format!("files/{}/base", file.id),
            data: file.base.clone(),
        });
        members.push(Member {
            path: format!("files/{}/{}.wchg", file.id, file.name),
            data: file.wchg_bytes(),
        });
    }
    zstd(&build_tar(&members))
}

fn splice_line(at: usize, delete_len: usize, insert: &str) -> String {
    format!(r#"{{"op":"splice","at":{at},"delete_len":{delete_len},"insert":"{insert}"}}"#)
}

/// Write container `bytes` to `dir/name` and return the full path.
fn write_fixture(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write fixture to tempdir");
    path
}

// === fixtures ===========================================================
//
// Both variants carry the SAME two tracked files so AC1 (file listing) is
// identical across variants. Each file's OWN direction drives reconstruction;
// index 0 is the current version for both directions (FORMAT.md section 5).

/// A `.wal` archive (variant seeds forward) holding two forward files:
///   - "notes.txt": base "cat" + 2 splices -> current "the cats"
///   - "data.csv":  base "a,b" + 1 splice  -> current "a,b,c"
fn wal_fixtures() -> Vec<FileFixture> {
    vec![
        FileFixture {
            id: "f0",
            name: "notes.txt",
            direction: Direction::Forward,
            base: b"cat".to_vec(),
            record_lines: vec![splice_line(3, 0, "s"), splice_line(0, 0, "the ")],
        },
        FileFixture {
            id: "f1",
            name: "data.csv",
            direction: Direction::Forward,
            base: b"a,b".to_vec(),
            record_lines: vec![splice_line(3, 0, ",c")],
        },
    ]
}

/// A `.wbl` archive (variant seeds reverse) holding two reverse files whose
/// CURRENT (index 0) bytes equal the `.wal` fixtures' current bytes. The reverse
/// log plays current -> original, so the base IS the current version.
///   - "notes.txt": base (current) "the cats"; reverse splices -> original "cat"
///   - "data.csv":  base (current) "a,b,c";    reverse splice  -> original "a,b"
fn wbl_fixtures() -> Vec<FileFixture> {
    vec![
        FileFixture {
            id: "f0",
            name: "notes.txt",
            direction: Direction::Reverse,
            base: b"the cats".to_vec(),
            // Invert the forward log, newest step undone first:
            //   undo "insert 'the ' at 0"  -> delete 4 at 0
            //   undo "insert 's' at 3"     -> delete 1 at 3
            record_lines: vec![splice_line(0, 4, ""), splice_line(3, 1, "")],
        },
        FileFixture {
            id: "f1",
            name: "data.csv",
            direction: Direction::Reverse,
            base: b"a,b,c".to_vec(),
            // undo "insert ',c' at 3" -> delete 2 at 3
            record_lines: vec![splice_line(3, 2, "")],
        },
    ]
}

fn wal_manifest(fixtures: &[FileFixture]) -> Manifest {
    let mut manifest = Manifest::new(Variant::Wal);
    for fixture in fixtures {
        manifest.files.push(fixture.tracked());
    }
    manifest
}

fn wbl_manifest(fixtures: &[FileFixture]) -> Manifest {
    let mut manifest = Manifest::new(Variant::Wbl);
    for fixture in fixtures {
        manifest.files.push(fixture.tracked());
    }
    manifest
}

// === AC1: WalrusSource lists every tracked file in manifest order =======

#[test]
fn walrus_source_lists_every_tracked_file_in_manifest_order_for_wal() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wal_fixtures();
    let bytes = build_container(&wal_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wal", &bytes);

    let source = WalrusSource::open(&path).expect("open .wal fixture");
    let files = source.files();
    let names: Vec<&str> = files.iter().map(|f| f.name()).collect();
    assert_eq!(names, vec!["notes.txt", "data.csv"]);
    // The FileRef id is the 0-based manifest position.
    assert_eq!(files[0].id(), 0);
    assert_eq!(files[1].id(), 1);
}

#[test]
fn walrus_source_lists_every_tracked_file_in_manifest_order_for_wbl() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wbl_fixtures();
    let bytes = build_container(&wbl_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wbl", &bytes);

    let source = WalrusSource::open(&path).expect("open .wbl fixture");
    let files = source.files();
    let names: Vec<&str> = files.iter().map(|f| f.name()).collect();
    assert_eq!(names, vec!["notes.txt", "data.csv"]);
    assert_eq!(files[0].id(), 0);
    assert_eq!(files[1].id(), 1);
}

// === AC2: reconstruct(file, Index(0)) is the CURRENT version ============

#[test]
fn reconstruct_at_index_zero_returns_current_for_wal() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wal_fixtures();
    let bytes = build_container(&wal_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wal", &bytes);

    let source = WalrusSource::open(&path).expect("open .wal fixture");
    let files = source.files();

    let notes = source
        .reconstruct(&files[0], &VersionId::Index(0))
        .expect("reconstruct notes current");
    assert_eq!(notes, b"the cats");

    let data = source
        .reconstruct(&files[1], &VersionId::Index(0))
        .expect("reconstruct data current");
    assert_eq!(data, b"a,b,c");
}

#[test]
fn reconstruct_at_index_zero_returns_current_for_wbl() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wbl_fixtures();
    let bytes = build_container(&wbl_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wbl", &bytes);

    let source = WalrusSource::open(&path).expect("open .wbl fixture");
    let files = source.files();

    let notes = source
        .reconstruct(&files[0], &VersionId::Index(0))
        .expect("reconstruct notes current");
    assert_eq!(notes, b"the cats");

    let data = source
        .reconstruct(&files[1], &VersionId::Index(0))
        .expect("reconstruct data current");
    assert_eq!(data, b"a,b,c");
}

// === cross-variant: index 0 is byte-identical across .wal and .wbl ======

#[test]
fn reconstruct_current_is_byte_identical_across_wal_and_wbl() {
    let dir = tempfile::TempDir::new().expect("tempdir");

    let wal = wal_fixtures();
    let wal_bytes = build_container(&wal_manifest(&wal), &wal);
    let wal_path = write_fixture(dir.path(), "x.wal", &wal_bytes);

    let wbl = wbl_fixtures();
    let wbl_bytes = build_container(&wbl_manifest(&wbl), &wbl);
    let wbl_path = write_fixture(dir.path(), "x.wbl", &wbl_bytes);

    let wal_source = WalrusSource::open(&wal_path).expect("open .wal");
    let wbl_source = WalrusSource::open(&wbl_path).expect("open .wbl");

    for index in 0..wal_source.files().len() {
        let from_wal = wal_source
            .reconstruct(&wal_source.files()[index], &VersionId::Index(0))
            .expect("wal current");
        let from_wbl = wbl_source
            .reconstruct(&wbl_source.files()[index], &VersionId::Index(0))
            .expect("wbl current");
        assert_eq!(
            from_wal, from_wbl,
            "current version of file {index} differs across .wal and .wbl"
        );
    }
}

// === versions + the original endpoint ===================================

#[test]
fn versions_span_zero_through_splice_count_inclusive() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wal_fixtures();
    let bytes = build_container(&wal_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wal", &bytes);

    let source = WalrusSource::open(&path).expect("open .wal fixture");
    let files = source.files();

    // "notes.txt" has 2 splices -> indices 0..=2.
    assert_eq!(
        source.versions(&files[0]),
        vec![
            VersionId::Index(0),
            VersionId::Index(1),
            VersionId::Index(2)
        ]
    );
    // "data.csv" has 1 splice -> indices 0..=1.
    assert_eq!(
        source.versions(&files[1]),
        vec![VersionId::Index(0), VersionId::Index(1)]
    );
}

#[test]
fn reconstruct_at_the_far_index_returns_the_original_for_both_variants() {
    let dir = tempfile::TempDir::new().expect("tempdir");

    let wal = wal_fixtures();
    let wal_bytes = build_container(&wal_manifest(&wal), &wal);
    let wal_path = write_fixture(dir.path(), "x.wal", &wal_bytes);
    let wal_source = WalrusSource::open(&wal_path).expect("open .wal");

    let wbl = wbl_fixtures();
    let wbl_bytes = build_container(&wbl_manifest(&wbl), &wbl);
    let wbl_path = write_fixture(dir.path(), "x.wbl", &wbl_bytes);
    let wbl_source = WalrusSource::open(&wbl_path).expect("open .wbl");

    // "notes.txt": 2 splices, so index 2 is the original "cat" for both.
    let notes_wal = &wal_source.files()[0];
    let notes_wbl = &wbl_source.files()[0];
    assert_eq!(
        wal_source
            .reconstruct(notes_wal, &VersionId::Index(2))
            .expect("wal original"),
        b"cat"
    );
    assert_eq!(
        wbl_source
            .reconstruct(notes_wbl, &VersionId::Index(2))
            .expect("wbl original"),
        b"cat"
    );
}

// === content_hint ========================================================

#[test]
fn content_hint_returns_the_tracked_mime_and_name() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wal_fixtures();
    let bytes = build_container(&wal_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wal", &bytes);

    let source = WalrusSource::open(&path).expect("open .wal fixture");
    let files = source.files();

    // TrackedFile::new_with_direction defaults mime to "text/plain"; the name is
    // the tracked file's own name.
    let (mime, name) = source.content_hint(&files[1]);
    assert_eq!(name, "data.csv");
    assert_eq!(mime, "text/plain");
}

// === history (FR-K9) =====================================================

#[test]
fn history_lists_entries_in_index_order_for_wal() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let fixtures = wal_fixtures();
    let bytes = build_container(&wal_manifest(&fixtures), &fixtures);
    let path = write_fixture(dir.path(), "fixture.wal", &bytes);

    let source = WalrusSource::open(&path).expect("open .wal fixture");
    let notes = &source.files()[0]; // "notes.txt": base "cat" + 2 splices.

    let history = source.history(notes);
    let indices: Vec<usize> = history.iter().map(|e| e.index).collect();
    assert_eq!(
        indices,
        vec![0, 1],
        "two splices -> entries at index 0 and 1"
    );

    // The forward log is [splice(3,0,"s"), splice(0,0,"the ")], oldest step
    // first. Version index 0 (current) is produced by the LAST splice ordinal
    // (count - 0 = 2) = splice(0,0,"the "); index 1 by ordinal 1 =
    // splice(3,0,"s").
    assert_eq!(
        history[0].splice,
        walker::source::SpliceSummary {
            at: 0,
            deleted_len: 0,
            inserted_len: 4, // "the "
        }
    );
    assert_eq!(
        history[1].splice,
        walker::source::SpliceSummary {
            at: 3,
            deleted_len: 0,
            inserted_len: 1, // "s"
        }
    );
}

#[test]
fn history_is_consistent_with_versions_for_both_variants() {
    let dir = tempfile::TempDir::new().expect("tempdir");

    let wal = wal_fixtures();
    let wal_bytes = build_container(&wal_manifest(&wal), &wal);
    let wal_path = write_fixture(dir.path(), "x.wal", &wal_bytes);
    let wal_source = WalrusSource::open(&wal_path).expect("open .wal");

    let wbl = wbl_fixtures();
    let wbl_bytes = build_container(&wbl_manifest(&wbl), &wbl);
    let wbl_path = write_fixture(dir.path(), "x.wbl", &wbl_bytes);
    let wbl_source = WalrusSource::open(&wbl_path).expect("open .wbl");

    // Every version except the original base is produced by one splice, so the
    // entry COUNT equals versions - 1 for BOTH variants. The splice NUMBERS
    // differ across variants (the .wbl log stores the inverse records), so this
    // asserts only the count symmetry, not byte-identical summaries.
    for source in [&wal_source, &wbl_source] {
        for file in source.files() {
            assert_eq!(
                source.history(&file).len(),
                source.versions(&file).len() - 1,
                "one history entry per non-original version"
            );
        }
    }
}
