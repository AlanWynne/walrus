mod common;

use common::{code_of, run_walrus};
use tempfile::TempDir;

const ODF_FIRST: &[u8] = include_bytes!("../../../tests/fixtures/office/tiny.odt");
#[cfg(feature = "office")]
const ODF_UPDATED: &[u8] = include_bytes!("../../../tests/fixtures/office/tiny-updated.odt");
#[cfg(feature = "office")]
const OOXML_DOCX: &[u8] = include_bytes!("../../../tests/fixtures/office/tiny.docx");

#[cfg(feature = "office")]
use walrus_fmt::{Archive, Version};

#[cfg(feature = "office")]
#[test]
fn add_detects_office_and_extract_cat_select_group_versions() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("report.odt");
    let archive = dir.path().join("office.wal");
    let first = ODF_FIRST;
    let second = ODF_UPDATED;

    std::fs::write(&source, first).expect("write first ODF");
    let source_arg = source.to_string_lossy().into_owned();
    let archive_arg = archive.to_string_lossy().into_owned();
    let added = run_walrus(dir.path(), &["add", &archive_arg, &source_arg]);
    assert_eq!(code_of(&added), 0, "office input is detected and ingested");
    std::fs::write(&source, second).expect("write second ODF");
    let updated = run_walrus(dir.path(), &["add", &archive_arg, &source_arg]);
    assert_eq!(
        code_of(&updated),
        0,
        "office input is saved as a new version"
    );

    let opened = Archive::open(&archive).expect("open office archive");
    assert_eq!(
        opened
            .group("report.odt")
            .expect("office document group")
            .version_map
            .len(),
        2
    );
    assert_eq!(
        opened
            .extract_office("report.odt", Version::Index(1))
            .expect("extract first office version"),
        first
    );
    let output_dir = TempDir::new().expect("output dir");
    let output_arg = output_dir.path().to_string_lossy().into_owned();
    let extracted = run_walrus(
        dir.path(),
        &[
            "extract",
            &archive_arg,
            "report.odt",
            "--index",
            "1",
            "-o",
            &output_arg,
        ],
    );
    assert_eq!(code_of(&extracted), 0, "group extract succeeds");
    assert_eq!(
        std::fs::read(output_dir.path().join("report.odt")).expect("read extracted ODF"),
        first
    );
    let catted = run_walrus(
        dir.path(),
        &["cat", &archive_arg, "report.odt", "--index", "0"],
    );
    assert_eq!(code_of(&catted), 0, "group cat succeeds");
    assert_eq!(catted.stdout, second);
}

#[cfg(feature = "office")]
#[test]
fn add_detects_and_stores_ooxml_fixture() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("report.docx");
    let archive = dir.path().join("office.wal");
    std::fs::write(&source, OOXML_DOCX).expect("write DOCX fixture");
    let source_arg = source.to_string_lossy().into_owned();
    let archive_arg = archive.to_string_lossy().into_owned();

    let added = run_walrus(dir.path(), &["add", &archive_arg, &source_arg]);
    assert_eq!(code_of(&added), 0, "OOXML input is detected and ingested");
    let opened = Archive::open(&archive).expect("open office archive");
    assert_eq!(
        opened
            .group("report.docx")
            .expect("OOXML document group")
            .kind,
        walrus_fmt::OfficeKind::Ooxml
    );
    assert_eq!(
        opened
            .extract_office("report.docx", Version::Index(0))
            .expect("extract OOXML"),
        OOXML_DOCX
    );
}

#[cfg(not(feature = "office"))]
#[test]
fn add_reports_when_office_support_is_not_built() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("report.odt");
    let archive = dir.path().join("office.wal");
    std::fs::write(&source, ODF_FIRST).expect("write ODF");
    let source_arg = source.to_string_lossy().into_owned();
    let archive_arg = archive.to_string_lossy().into_owned();

    let result = run_walrus(dir.path(), &["add", &archive_arg, &source_arg]);

    assert_ne!(code_of(&result), 0);
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("office support not built"),
        "diagnostic explains how office support is gated"
    );
    assert!(
        !archive.exists(),
        "failed ingest does not create an archive"
    );
}
