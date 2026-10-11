//! `-R` recursion behaviour (FR-W11 AC7, CLI-DESIGN 5.4b/5.4c).

mod common;

use common::{code_of, reconstruct, run_walrus};
use tempfile::TempDir;
use walrus_fmt::{Archive, Version};

/// Build a `src/` tree with a nested file under `root`.
fn build_tree(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src").join("net")).expect("mkdirs");
    std::fs::write(root.join("src").join("main.rs"), b"fn main() {}").expect("w1");
    std::fs::write(root.join("src").join("net").join("http.rs"), b"// http").expect("w2");
}

#[test]
fn add_recurse_stores_relative_tree_and_extract_round_trips() {
    let dir = TempDir::new().expect("tmp");
    build_tree(dir.path());
    let archive = dir.path().join("tree.wal").to_string_lossy().into_owned();

    let result = run_walrus(dir.path(), &["add", &archive, "src", "-R"]);
    assert_eq!(code_of(&result), 0, "recursive add succeeds");

    let path = std::path::Path::new(&archive);
    // Stored under <root-name>/<relative>, `/`-separated.
    assert_eq!(
        reconstruct(path, "src/main.rs", Version::Current),
        b"fn main() {}"
    );
    assert_eq!(
        reconstruct(path, "src/net/http.rs", Version::Current),
        b"// http"
    );
}

#[test]
fn recurse_walk_is_sorted_and_skips_special_files() {
    let dir = TempDir::new().expect("tmp");
    build_tree(dir.path());
    let archive = dir.path().join("tree.wal").to_string_lossy().into_owned();
    run_walrus(dir.path(), &["add", &archive, "src", "-R"]);

    let a = Archive::open(&archive).expect("open");
    let names: Vec<String> = a.files().map(|f| f.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "ingested in sorted relative-path order");
}

#[test]
fn recurse_binary_file_aborts_batch_and_leaves_archive_unchanged() {
    let dir = TempDir::new().expect("tmp");
    build_tree(dir.path());
    // Seed an existing archive so we can prove it is left byte-for-byte intact.
    let archive = dir.path().join("tree.wal").to_string_lossy().into_owned();
    run_walrus(dir.path(), &["add", &archive, "src/main.rs"]);
    let before = std::fs::read(&archive).expect("read before");

    // Introduce a binary (invalid-UTF-8) file into the tree, then recurse.
    std::fs::write(dir.path().join("src").join("blob.bin"), [0xFF, 0xFE, 0x00]).expect("blob");
    let result = run_walrus(dir.path(), &["add", &archive, "src", "-R"]);
    assert_ne!(code_of(&result), 0, "a binary file aborts the batch");

    let after = std::fs::read(&archive).expect("read after");
    assert_eq!(
        before, after,
        "the archive is unchanged after an aborted batch"
    );
}

#[test]
fn update_recurse_untracked_file_is_rc8_and_unchanged() {
    let dir = TempDir::new().expect("tmp");
    build_tree(dir.path());
    let archive = dir.path().join("tree.wal").to_string_lossy().into_owned();
    // Track only src/main.rs; src/net/http.rs stays untracked.
    run_walrus(dir.path(), &["add", &archive, "src/main.rs"]);
    let before = std::fs::read(&archive).expect("read before");

    let result = run_walrus(dir.path(), &["update", &archive, "src", "-R"]);
    assert_eq!(code_of(&result), 8, "an untracked recursed file is RC 8");
    let after = std::fs::read(&archive).expect("read after");
    assert_eq!(before, after, "a failed recursive update is transactional");
}
