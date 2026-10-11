//! Headless tests for the FR-K5 import flow (add / update, resolved
//! automatically).
//!
//! These drive the REAL external navigator pane through the format-agnostic
//! seam with a `FakePicker` injected in place of the OS-native dialog, so the
//! whole import flow after the pick is headless (FR-K5 AC2). The only MANUAL
//! carve-out is the real `rfd` pick inside `RfdPicker`, which an `accesskit`
//! harness cannot drive.
//!
//! Each harness test runs a FIXED, bounded number of frames and asserts on the
//! accessibility tree (labels) and the `ImportState` model, never on pixels or
//! frame counts (mirrors `tests/navigator.rs`).
//!
//! The real-archive, on-disk mutation tests (AC1 "a tracked file is added" /
//! "a version is appended" and the transactional guarantee) need the
//! `walrus_fmt` mutate API (FR-W7), which is a cross-stream dependency not yet
//! merged on this branch. They are gated behind the `import-mutate` cargo
//! feature and `#[cfg_attr(..., ignore)]`-ed off until it lands -- NOT deleted,
//! so they un-ignore the moment FR-W7 merges and the feature is flipped.

use std::path::PathBuf;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walker::import::{FilePicker, ImportState};
use walker::navigator::{self, NavigatorState};
use walker::source::testsupport::InMemorySource;

/// A test [`FilePicker`] returning a canned path, so the import flow runs with
/// no OS dialog. `pick()` never touches `rfd`.
struct FakePicker {
    path: Option<PathBuf>,
}

impl FakePicker {
    fn returning(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

impl FilePicker for FakePicker {
    fn pick(&self) -> Option<PathBuf> {
        self.path.clone()
    }
}

/// Shared harness model: the navigator selection + the import state, driven over
/// a sample `InMemorySource` whose names stand in for an archive's file list.
#[derive(Default)]
struct Model {
    navigator: NavigatorState,
    import: ImportState,
}

/// Build a harness that renders the real navigator panes with `picker` injected
/// and a sample multi-file source, writing the picked file into `dir` first so
/// the pane can read its bytes.
fn harness_with(picker: FakePicker) -> Harness<'static, Model> {
    Harness::new_ui_state(
        move |ui, model: &mut Model| {
            let source = InMemorySource::sample();
            navigator::panels(
                ui,
                &source,
                &picker,
                &mut model.import,
                &mut model.navigator,
            );
        },
        Model::default(),
    )
}

/// Write `bytes` to `dir/name` and return the path (so `FakePicker` can point at
/// a real file the pane reads).
fn write_pick(dir: &std::path::Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write picked file");
    path
}

// === AC2: resolution + prompt surfacing, headless via FakePicker ========

#[test]
fn clicking_import_with_a_new_name_sets_an_add_outcome_without_a_prompt() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // "brand-new.txt" is NOT one of the sample source's names.
    let picked = write_pick(dir.path(), "brand-new.txt", b"hello");
    let mut harness = harness_with(FakePicker::returning(picked));
    harness.run();

    harness.get_by_label("Import...").click();
    harness.run();

    // No prompt widget is present for a brand-new name (AC1: add, no prompt).
    assert!(
        harness
            .query_by_label_contains("import as a new version")
            .is_none(),
        "a new name surfaces no update prompt"
    );
    // The pane drained the resolved Add outcome, so no prompt lingers in model.
    assert!(
        !harness.state().import.has_prompt(),
        "a new name leaves no pending prompt in the model"
    );
}

#[test]
fn clicking_import_with_an_existing_name_shows_the_update_prompt() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // "README.md" IS a sample source name, so the import collides.
    let picked = write_pick(dir.path(), "README.md", b"new contents");
    let mut harness = harness_with(FakePicker::returning(picked));
    harness.run();

    harness.get_by_label("Import...").click();
    harness.run();

    assert!(
        harness
            .query_by_label_contains("README.md exists -- import as a new version?")
            .is_some(),
        "an existing name surfaces the update prompt with the file name"
    );
    assert!(
        harness.state().import.has_prompt(),
        "the model holds a pending update prompt"
    );
}

#[test]
fn confirming_the_prompt_resolves_to_an_append_outcome_and_clears_the_prompt() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let picked = write_pick(dir.path(), "data.csv", b"a,b,c\n9,9,9");
    let mut harness = harness_with(FakePicker::returning(picked));
    harness.run();

    harness.get_by_label("Import...").click();
    harness.run();
    assert!(
        harness.state().import.has_prompt(),
        "precondition: the prompt is up for an existing name"
    );

    harness.get_by_label("Import as new version").click();
    harness.run();

    // Confirming resolves the prompt (the append intent) and the pane drains the
    // outcome, leaving no pending prompt (the list-refresh precondition).
    assert!(
        !harness.state().import.has_prompt(),
        "confirming clears the surfaced prompt"
    );
    assert!(
        harness.state().import.pending().is_none(),
        "confirming drains the pending import"
    );
}

#[test]
fn cancelling_the_prompt_abandons_the_import() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let picked = write_pick(dir.path(), "data.csv", b"x");
    let mut harness = harness_with(FakePicker::returning(picked));
    harness.run();

    harness.get_by_label("Import...").click();
    harness.run();
    assert!(harness.state().import.has_prompt());

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(
        harness.state().import.pending().is_none(),
        "cancelling abandons the pending import entirely"
    );
}

// === AC1 + transactional guarantee: real archive on disk ================
//
// Gated on the walrus_fmt mutate API (FR-W7). Un-ignores when the
// `import-mutate` feature is enabled (which is only valid once FR-W7 has merged
// so `Archive::add` exists). See crates/walker/Cargo.toml and
// .agents/tasks/walker-viewer/frk5-plan.md section 0.

#[cfg(feature = "import-mutate")]
mod on_disk {
    use super::*;
    use walker::import::ImportResolution;
    use walker::source::{HistorySource, VersionId, WalrusSource};
    use walrus_fmt::{Direction, Manifest, TrackedFile, Variant, WchgHeader};

    /// Build a single-file forward `.wal` container (base + no splices) and
    /// write it to `dir/name`, returning the path.
    fn single_file_archive(dir: &std::path::Path, name: &str, file: &str, base: &[u8]) -> PathBuf {
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(TrackedFile::new_with_direction(
            "f0",
            file,
            Direction::Forward,
            0,
        ));
        let header = WchgHeader::new(Direction::Forward, 0, file, "utf-8", 0);
        let mut wchg = serde_json::to_string(&header).expect("header");
        wchg.push('\n');

        let mut builder = tar::Builder::new(Vec::new());
        append(
            &mut builder,
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest).expect("m"),
        );
        append(&mut builder, "files/f0/base", base);
        append(
            &mut builder,
            &format!("files/f0/{file}.wchg"),
            wchg.as_bytes(),
        );
        let tar_bytes = builder.into_inner().expect("tar");
        let bytes = zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd");

        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write fixture");
        path
    }

    fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) {
        let mut header = tar::Header::new_ustar();
        header.set_path(path).expect("path");
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder.append(&header, data).expect("append");
    }

    #[test]
    fn importing_a_new_name_adds_a_tracked_file_to_the_archive_on_disk() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = single_file_archive(dir.path(), "doc.wal", "a.txt", b"first");
        let mut source = WalrusSource::open(&path).expect("open");

        let resolution = ImportResolution::Add {
            name: "extra.txt".to_string(),
        };
        source
            .import(&resolution, b"hello", &path)
            .expect("import add");

        // Re-open from disk: the change must have hit the file transactionally.
        let reopened = WalrusSource::open(&path).expect("re-open");
        let names: Vec<String> = reopened
            .files()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        assert!(
            names.contains(&"extra.txt".to_string()),
            "the new file is tracked on disk"
        );
        let extra = reopened
            .files()
            .into_iter()
            .find(|f| f.name() == "extra.txt")
            .expect("extra.txt ref");
        assert_eq!(
            reopened
                .reconstruct(&extra, &VersionId::Index(0))
                .expect("current"),
            b"hello"
        );
    }

    #[test]
    fn importing_an_existing_name_appends_a_version_growing_the_index_size() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = single_file_archive(dir.path(), "doc.wal", "a.txt", b"first");
        let before = WalrusSource::open(&path).expect("open");
        let file = before.files().into_iter().next().expect("a.txt");
        let n = before.file_info(&file).index_size;

        let mut source = WalrusSource::open(&path).expect("open");
        let resolution = ImportResolution::UpdatePrompt {
            name: "a.txt".to_string(),
        };
        source
            .import(&resolution, b"second", &path)
            .expect("import update");

        let reopened = WalrusSource::open(&path).expect("re-open");
        let file = reopened.files().into_iter().next().expect("a.txt");
        assert_eq!(
            reopened.file_info(&file).index_size,
            n + 1,
            "an update appends exactly one version"
        );
        assert_eq!(
            reopened
                .reconstruct(&file, &VersionId::Index(0))
                .expect("current"),
            b"second",
            "index 0 is the newly imported current version"
        );
    }

    #[test]
    fn a_failed_import_leaves_the_existing_archive_byte_for_byte_unchanged() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = single_file_archive(dir.path(), "doc.wal", "a.txt", b"first");
        let snapshot = std::fs::read(&path).expect("snapshot");

        let mut source = WalrusSource::open(&path).expect("open");
        // Updating an UNTRACKED name is rejected by the mutate path, so the
        // write is never reached and `path` must be untouched.
        let resolution = ImportResolution::UpdatePrompt {
            name: "ghost.txt".to_string(),
        };
        let result = source.import(&resolution, b"nope", &path);
        assert!(result.is_err(), "updating an untracked name errors");
        assert_eq!(
            std::fs::read(&path).expect("re-read"),
            snapshot,
            "a failed import leaves the archive byte-for-byte unchanged"
        );
    }
}
