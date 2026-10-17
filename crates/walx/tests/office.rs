mod common;

#[cfg(feature = "office")]
use common::{code_of, run_walx};
#[cfg(feature = "office")]
use walrus_fmt::{Archive, Manifest, Variant};

#[cfg(feature = "office")]
const ODF_FIRST: &[u8] = include_bytes!("../../../tests/fixtures/office/tiny.odt");
#[cfg(feature = "office")]
const ODF_UPDATED: &[u8] = include_bytes!("../../../tests/fixtures/office/tiny-updated.odt");
#[cfg(feature = "office")]
const OOXML_DOCX: &[u8] = include_bytes!("../../../tests/fixtures/office/tiny.docx");

#[cfg(feature = "office")]
#[test]
fn extract_and_cat_restore_office_group_versions() {
    let dir = common::Fixture::empty();
    let archive_path = dir.dir.path().join("office.wal");
    let mut archive = Archive::new(Manifest::new(Variant::Wal));
    archive
        .add_office("report.odt", ODF_FIRST)
        .expect("add first ODF version");
    archive
        .add_office("report.odt", ODF_UPDATED)
        .expect("add current ODF version");
    archive.write(&archive_path).expect("write office archive");

    let archive_arg = archive_path.to_string_lossy().into_owned();
    let output_dir = tempfile::TempDir::new().expect("output dir");
    let output_arg = output_dir.path().to_string_lossy().into_owned();
    let extracted = run_walx(
        dir.dir.path(),
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
    assert_eq!(
        code_of(&extracted),
        0,
        "group extraction succeeds: {}",
        String::from_utf8_lossy(&extracted.stderr)
    );
    assert_eq!(
        std::fs::read(output_dir.path().join("report.odt")).expect("read extracted ODF"),
        ODF_FIRST
    );
    assert!(
        !output_dir
            .path()
            .join("report.odt")
            .join("content.xml")
            .exists(),
        "extract writes the office document, not its internal members"
    );

    let catted = run_walx(
        dir.dir.path(),
        &["cat", &archive_arg, "report.odt", "--index", "0"],
    );
    assert_eq!(code_of(&catted), 0, "group cat succeeds");
    assert_eq!(catted.stdout, ODF_UPDATED);
}

#[cfg(feature = "office")]
#[test]
fn extract_and_cat_restore_docx_fixture() {
    let dir = common::Fixture::empty();
    let archive_path = dir.dir.path().join("office.wal");
    let mut archive = Archive::new(Manifest::new(Variant::Wal));
    archive
        .add_office("report.docx", OOXML_DOCX)
        .expect("add DOCX fixture");
    archive.write(&archive_path).expect("write office archive");
    let archive_arg = archive_path.to_string_lossy().into_owned();

    let catted = run_walx(dir.dir.path(), &["cat", &archive_arg, "report.docx"]);
    assert_eq!(
        code_of(&catted),
        0,
        "DOCX group cat succeeds: {}",
        String::from_utf8_lossy(&catted.stderr)
    );
    assert_eq!(catted.stdout, OOXML_DOCX);
}
