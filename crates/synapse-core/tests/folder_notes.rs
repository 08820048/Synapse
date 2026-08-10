use std::{fs, path::Path};

use synapse_core::{Vault, VaultEntryKind, VaultError};

#[test]
fn v3_ac1_discovers_empty_folders_and_nested_markdown_notes() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("empty/nested")).unwrap();
    fs::write(directory.path().join("empty/note.md"), "# Note").unwrap();
    fs::write(directory.path().join("ignored.txt"), "ignored").unwrap();

    let entries = Vault::open(directory.path())
        .unwrap()
        .discover_entries()
        .unwrap();
    let actual: Vec<_> = entries
        .iter()
        .map(|entry| (entry.relative_path.as_path(), entry.kind))
        .collect();

    assert_eq!(
        actual,
        vec![
            (Path::new("empty"), VaultEntryKind::Directory),
            (Path::new("empty/nested"), VaultEntryKind::Directory),
            (Path::new("empty/note.md"), VaultEntryKind::Note),
        ]
    );
}

#[test]
fn v3_ac2_creates_root_and_nested_folders_and_notes_without_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    let folder = vault.create_directory(Path::new(""), "Projects").unwrap();
    let note = vault.create_note(&folder, "Roadmap").unwrap();

    assert_eq!(folder, Path::new("Projects"));
    assert_eq!(note, Path::new("Projects/Roadmap.md"));
    assert!(directory.path().join(&folder).is_dir());
    assert_eq!(
        fs::read_to_string(directory.path().join(&note)).unwrap(),
        "# Roadmap\n"
    );
    assert!(matches!(
        vault.create_note(&folder, "Roadmap"),
        Err(VaultError::AlreadyExists(path)) if path == Path::new("Projects/Roadmap.md")
    ));
}

#[test]
fn v3_ac4_renames_notes_and_folders_without_losing_descendants() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("Drafts")).unwrap();
    fs::write(directory.path().join("Drafts/old.md"), "content").unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    let note = vault
        .rename_entry(Path::new("Drafts/old.md"), "new")
        .unwrap();
    let folder = vault
        .rename_entry(Path::new("Drafts"), "Published")
        .unwrap();

    assert_eq!(note, Path::new("Drafts/new.md"));
    assert_eq!(folder, Path::new("Published"));
    assert_eq!(
        fs::read_to_string(directory.path().join("Published/new.md")).unwrap(),
        "content"
    );
}

#[test]
fn v3_ac5_moves_notes_and_folders_and_rejects_recursive_or_colliding_moves() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("source/child")).unwrap();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(directory.path().join("note.md"), "note").unwrap();
    fs::write(directory.path().join("target/note.md"), "collision").unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.move_entry(Path::new("source"), Path::new("source/child")),
        Err(VaultError::MoveIntoSelf { .. })
    ));
    assert!(matches!(
        vault.move_entry(Path::new("note.md"), Path::new("target")),
        Err(VaultError::AlreadyExists(path)) if path == Path::new("target/note.md")
    ));

    fs::remove_file(directory.path().join("target/note.md")).unwrap();
    let moved_note = vault
        .move_entry(Path::new("note.md"), Path::new("target"))
        .unwrap();
    let moved_folder = vault
        .move_entry(Path::new("source"), Path::new("target"))
        .unwrap();

    assert_eq!(moved_note, Path::new("target/note.md"));
    assert_eq!(moved_folder, Path::new("target/source"));
    assert!(directory.path().join("target/source/child").is_dir());
}

#[test]
fn v3_sr_rejects_traversal_invalid_names_and_non_markdown_extensions() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.create_directory(Path::new(".."), "escape"),
        Err(VaultError::InvalidEntryPath(_))
    ));
    assert!(matches!(
        vault.create_directory(Path::new(""), "nested/name"),
        Err(VaultError::InvalidEntryName(_))
    ));
    assert!(matches!(
        vault.create_note(Path::new(""), "note.txt"),
        Err(VaultError::NotMarkdown(_))
    ));
}

#[cfg(unix)]
#[test]
fn v3_sr_rejects_symlink_components_for_mutations() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), directory.path().join("linked")).unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.create_note(Path::new("linked"), "escape"),
        Err(VaultError::UnsafeEntryPath(path)) if path == Path::new("linked")
    ));
}
