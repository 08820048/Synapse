use std::{fs, path::Path};

use synapse_core::{Vault, VaultError};

#[test]
fn ac1_open_valid_vault_uses_canonical_root() {
    let directory = tempfile::tempdir().unwrap();

    let vault = Vault::open(directory.path()).unwrap();

    assert_eq!(vault.root(), directory.path().canonicalize().unwrap());
}

#[test]
fn ac2_missing_root_returns_not_found() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");

    let error = Vault::open(&missing).unwrap_err();

    assert!(matches!(error, VaultError::NotFound(path) if path == missing));
}

#[test]
fn ac2_regular_file_root_returns_not_directory() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("note.md");
    fs::write(&file, "# Note").unwrap();

    let error = Vault::open(&file).unwrap_err();

    assert!(matches!(error, VaultError::NotDirectory(path) if path == file));
}

#[test]
fn ac3_discovers_markdown_recursively_in_relative_path_order() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("nested")).unwrap();
    fs::write(directory.path().join("zeta.md"), "# Zeta").unwrap();
    fs::write(directory.path().join("nested/Alpha.MD"), "# Alpha").unwrap();
    fs::write(directory.path().join("beta.md"), "# Beta").unwrap();

    let notes = Vault::open(directory.path())
        .unwrap()
        .discover_notes()
        .unwrap();

    let actual: Vec<_> = notes
        .iter()
        .map(|note| (note.relative_path.as_path(), note.title.as_str()))
        .collect();
    assert_eq!(
        actual,
        vec![
            (Path::new("beta.md"), "beta"),
            (Path::new("nested/Alpha.MD"), "Alpha"),
            (Path::new("zeta.md"), "zeta"),
        ]
    );
}

#[test]
fn ac4_ignores_non_markdown_files() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("note.md"), "# Note").unwrap();
    fs::write(directory.path().join("draft.txt"), "draft").unwrap();

    let notes = Vault::open(directory.path())
        .unwrap()
        .discover_notes()
        .unwrap();

    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].relative_path, Path::new("note.md"));
}

#[test]
fn ec3_discovery_returns_typed_io_error_when_root_disappears() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Vault::open(directory.path()).unwrap();
    fs::remove_dir(directory.path()).unwrap();

    let error = vault.discover_notes().unwrap_err();

    assert!(matches!(error, VaultError::Io { path, .. } if path == vault.root()));
}

#[cfg(unix)]
#[test]
fn ac4_ignores_file_and_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("notes")).unwrap();
    fs::write(directory.path().join("notes/real.md"), "# Real").unwrap();
    symlink(
        directory.path().join("notes/real.md"),
        directory.path().join("linked.md"),
    )
    .unwrap();
    symlink(directory.path(), directory.path().join("cycle")).unwrap();

    let notes = Vault::open(directory.path())
        .unwrap()
        .discover_notes()
        .unwrap();

    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].relative_path, Path::new("notes/real.md"));
}

#[test]
fn ec4_skips_markdown_path_without_a_file_stem() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join(".md"), "# No stem").unwrap();

    let notes = Vault::open(directory.path())
        .unwrap()
        .discover_notes()
        .unwrap();

    assert!(notes.is_empty());
}
