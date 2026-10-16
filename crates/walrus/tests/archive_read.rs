//! Integration tests for the per-file container reader and the read-only
//! [`Archive`] type (FR-W2, FORMAT.md section 2.1).
//!
//! Fixtures are hand-built IN MEMORY as a ustar tar (optionally zstd-compressed)
//! with `manifest.json` + per-file `files/<id>/base` + `files/<id>/<name>.wchg`
//! members. They do NOT depend on the multi-file writer (FR-W4), which does not
//! exist yet.

use walrus_fmt::{Direction, Manifest, TrackedFile, Variant, WalrusError, WchgHeader};

// === in-memory fixture builder =========================================

/// One tracked file's fixture inputs: id, name, direction, base bytes, and the
/// change-record lines (already-serialised JSON, one per record) for its
/// `.wchg`. The header line is synthesised from direction/name/count.
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
        let mut out = serde_json::to_string(&header).expect("serialise header");
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

/// Build an uncompressed ustar tar from the given members (mirrors the writer's
/// `append_member`, but standalone so it does not depend on `write.rs`).
fn build_tar(members: &[Member]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for member in members {
        let mut header = tar::Header::new_ustar();
        header.set_path(&member.path).expect("set path");
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
fn build_archive(manifest: &Manifest, files: &[FileFixture]) -> Vec<u8> {
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

// === AC extra: zero-file archive =======================================

#[test]
fn empty_archive_reads_into_an_archive_with_no_files() {
    let manifest = Manifest::new(Variant::Wal);
    let bytes = build_archive(&manifest, &[]);
    let archive = walrus_fmt::Archive::read(&bytes).expect("empty archive reads");
    assert_eq!(archive.files().count(), 0);
    assert_eq!(archive.manifest().variant, Variant::Wal);
}

// === AC1: multi-file, mixed direction ==================================

fn mixed_fixtures() -> Vec<FileFixture> {
    vec![
        FileFixture {
            id: "fwd",
            name: "forward.txt",
            direction: Direction::Forward,
            base: b"cat".to_vec(),
            record_lines: vec![splice_line(3, 0, "s"), splice_line(0, 0, "the ")],
        },
        FileFixture {
            id: "rev",
            name: "reverse.txt",
            direction: Direction::Reverse,
            base: b"current text".to_vec(),
            record_lines: vec![splice_line(0, 4, "old ")],
        },
    ]
}

fn mixed_manifest(fixtures: &[FileFixture]) -> Manifest {
    // A .wal archive (variant seeds forward) that still holds one reverse file.
    let mut manifest = Manifest::new(Variant::Wal);
    for fixture in fixtures {
        manifest.files.push(fixture.tracked());
    }
    manifest
}

#[test]
fn multi_file_mixed_direction_archive_reads_each_files_base_header_records() {
    let fixtures = mixed_fixtures();
    let manifest = mixed_manifest(&fixtures);
    let bytes = build_archive(&manifest, &fixtures);
    let archive = walrus_fmt::Archive::read(&bytes).expect("mixed archive reads");

    assert_eq!(archive.files().count(), 2);

    // Forward file.
    assert_eq!(archive.base("fwd"), Some(&b"cat"[..]));
    assert_eq!(
        archive.header("fwd").expect("fwd header").direction,
        Direction::Forward
    );
    assert_eq!(archive.records("fwd").expect("fwd records").len(), 2);

    // Reverse file whose direction DIFFERS from the archive variant.
    assert_eq!(archive.base("rev"), Some(&b"current text"[..]));
    assert_eq!(
        archive.header("rev").expect("rev header").direction,
        Direction::Reverse
    );
    assert_eq!(archive.records("rev").expect("rev records").len(), 1);
}

#[test]
fn reading_does_not_reject_a_file_whose_direction_differs_from_the_variant() {
    // INV-3: a mixed forward/reverse archive is legal; reading must not reject.
    let fixtures = mixed_fixtures();
    let manifest = mixed_manifest(&fixtures);
    let bytes = build_archive(&manifest, &fixtures);
    walrus_fmt::Archive::read(&bytes).expect("mixed-direction archive is legal");
}

#[test]
fn file_lookup_resolves_by_id_and_by_name() {
    let fixtures = mixed_fixtures();
    let manifest = mixed_manifest(&fixtures);
    let bytes = build_archive(&manifest, &fixtures);
    let archive = walrus_fmt::Archive::read(&bytes).expect("archive reads");
    assert_eq!(archive.file("fwd").expect("by id").name, "forward.txt");
    assert_eq!(archive.file("forward.txt").expect("by name").id, "fwd");
    assert!(archive.file("nope").is_none());
}

// === AC2: framing + per-file error reporting ===========================

#[test]
fn bad_record_line_reports_the_owning_file_and_header_adjusted_line() {
    let fixtures = vec![FileFixture {
        id: "f0",
        name: "doc.txt",
        direction: Direction::Forward,
        base: b"base".to_vec(),
        record_lines: vec!["{ not json }".to_string()],
    }];
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(fixtures[0].tracked());
    let bytes = build_archive(&manifest, &fixtures);
    let err = walrus_fmt::Archive::read(&bytes).expect_err("bad record rejected");
    match err {
        WalrusError::Wchg { id, line, .. } => {
            assert_eq!(id, "f0");
            assert_eq!(line, 2, "first record line is header-adjusted line 2");
        }
        other => panic!("expected Wchg error, got {other:?}"),
    }
}

#[test]
fn crlf_terminated_record_line_is_rejected_at_the_archive_boundary() {
    // Hand-build a .wchg with a CRLF-terminated record line.
    let header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", 1);
    let mut wchg = serde_json::to_string(&header).expect("header");
    wchg.push('\n');
    wchg.push_str(&splice_line(0, 0, "a"));
    wchg.push_str("\r\n");

    let manifest_file = TrackedFile::new_with_direction("f0", "doc.txt", Direction::Forward, 1);
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(manifest_file);

    let members = vec![
        Member {
            path: "manifest.json".to_string(),
            data: serde_json::to_vec_pretty(&manifest).expect("manifest"),
        },
        Member {
            path: "files/f0/base".to_string(),
            data: b"base".to_vec(),
        },
        Member {
            path: "files/f0/doc.txt.wchg".to_string(),
            data: wchg.into_bytes(),
        },
    ];
    let bytes = zstd(&build_tar(&members));
    let err = walrus_fmt::Archive::read(&bytes).expect_err("CRLF line rejected");
    assert!(matches!(err, WalrusError::Wchg { .. }));
}

#[test]
fn missing_base_member_reports_the_file_id() {
    let manifest_file = TrackedFile::new_with_direction("f0", "doc.txt", Direction::Forward, 0);
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(manifest_file);

    let header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", 0);
    let mut wchg = serde_json::to_string(&header).expect("header");
    wchg.push('\n');

    // base member deliberately omitted.
    let members = vec![
        Member {
            path: "manifest.json".to_string(),
            data: serde_json::to_vec_pretty(&manifest).expect("manifest"),
        },
        Member {
            path: "files/f0/doc.txt.wchg".to_string(),
            data: wchg.into_bytes(),
        },
    ];
    let bytes = zstd(&build_tar(&members));
    let err = walrus_fmt::Archive::read(&bytes).expect_err("missing base rejected");
    match err {
        WalrusError::BaseMissing { id } => assert_eq!(id, "f0"),
        other => panic!("expected BaseMissing, got {other:?}"),
    }
}

#[test]
fn missing_wchg_member_reports_the_file_id() {
    let manifest_file = TrackedFile::new_with_direction("f0", "doc.txt", Direction::Forward, 0);
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(manifest_file);

    // .wchg member deliberately omitted.
    let members = vec![
        Member {
            path: "manifest.json".to_string(),
            data: serde_json::to_vec_pretty(&manifest).expect("manifest"),
        },
        Member {
            path: "files/f0/base".to_string(),
            data: b"base".to_vec(),
        },
    ];
    let bytes = zstd(&build_tar(&members));
    let err = walrus_fmt::Archive::read(&bytes).expect_err("missing .wchg rejected");
    match err {
        WalrusError::WchgMissing { id } => assert_eq!(id, "f0"),
        other => panic!("expected WchgMissing, got {other:?}"),
    }
}

#[test]
fn non_wchg_header_is_a_typed_wchg_header_error() {
    let manifest_file = TrackedFile::new_with_direction("f0", "doc.txt", Direction::Forward, 0);
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.files.push(manifest_file);

    let bad_header = r#"{ "format": "notwchg", "spec_version": "0.1", "direction": "forward", "generations": 0, "name": "doc.txt", "encoding": "utf-8", "count": 0 }"#;
    let mut wchg = bad_header.to_string();
    wchg.push('\n');

    let members = vec![
        Member {
            path: "manifest.json".to_string(),
            data: serde_json::to_vec_pretty(&manifest).expect("manifest"),
        },
        Member {
            path: "files/f0/base".to_string(),
            data: b"base".to_vec(),
        },
        Member {
            path: "files/f0/doc.txt.wchg".to_string(),
            data: wchg.into_bytes(),
        },
    ];
    let bytes = zstd(&build_tar(&members));
    let err = walrus_fmt::Archive::read(&bytes).expect_err("non-wchg header rejected");
    assert!(matches!(err, WalrusError::WchgHeader(_)));
}

// === AC3: codec sniffing at the Archive boundary =======================

#[test]
fn zstd_archive_reads_successfully() {
    let manifest = Manifest::new(Variant::Wbl);
    let bytes = build_archive(&manifest, &[]);
    // Confirm the fixture really is zstd-magic'd.
    assert_eq!(&bytes[0..4], &[0x28, 0xB5, 0x2F, 0xFD]);
    walrus_fmt::Archive::read(&bytes).expect("zstd container reads");
}

#[test]
fn unknown_outer_magic_is_rejected() {
    let err = walrus_fmt::Archive::read(b"not a walrus container").expect_err("unknown magic");
    assert!(matches!(err, WalrusError::UnknownCodec));
}

#[test]
fn gzip_outer_magic_errors_clearly_without_panicking() {
    // gzip magic 0x1F 0x8B is a documented deferral: a clear error, no panic.
    let gzip_like = [0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
    let err = walrus_fmt::Archive::read(&gzip_like).expect_err("gzip deferral errors");
    assert!(matches!(
        err,
        WalrusError::UnsupportedCodec { codec: "gzip" }
    ));
}
