use std::{fs, path::Path};

use synapse_core::{BufferError, Vault, VaultError};

#[test]
fn ac1_open_preserves_exact_unicode_markdown_and_starts_clean() {
    let directory = tempfile::tempdir().unwrap();
    let original = "# 标题\n\nHello, 世界 👋\n";
    fs::write(directory.path().join("note.md"), original).unwrap();

    let document = Vault::open(directory.path())
        .unwrap()
        .open_note("note.md")
        .unwrap();

    assert_eq!(document.relative_path(), Path::new("note.md"));
    assert_eq!(document.text(), original);
    assert_eq!(document.revision(), 0);
    assert!(!document.is_dirty());
}

#[test]
fn ac2_edits_use_unicode_character_indices_and_track_revisions() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("note.md"), "A你B").unwrap();
    let mut document = Vault::open(directory.path())
        .unwrap()
        .open_note("note.md")
        .unwrap();

    document.insert(2, "好").unwrap();
    assert_eq!(document.text(), "A你好B");
    assert_eq!(document.revision(), 1);
    assert!(document.is_dirty());

    document.remove(1..3).unwrap();
    assert_eq!(document.text(), "AB");
    assert_eq!(document.revision(), 2);
}

#[test]
fn ac3_invalid_edit_ranges_preserve_document_state() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("note.md"), "abc").unwrap();
    let mut document = Vault::open(directory.path())
        .unwrap()
        .open_note("note.md")
        .unwrap();

    assert!(matches!(
        document.insert(4, "x"),
        Err(BufferError::CharacterIndexOutOfBounds { index: 4, len: 3 })
    ));
    let reversed_start = 3;
    let reversed_end = 2;
    assert!(matches!(
        document.remove(reversed_start..reversed_end),
        Err(BufferError::InvalidCharacterRange {
            start: 3,
            end: 2,
            len: 3
        })
    ));
    assert!(matches!(
        document.remove(0..4),
        Err(BufferError::InvalidCharacterRange {
            start: 0,
            end: 4,
            len: 3
        })
    ));

    assert_eq!(document.text(), "abc");
    assert_eq!(document.revision(), 0);
    assert!(!document.is_dirty());
}

#[test]
fn ac4_save_persists_exact_text_marks_clean_and_removes_temp_file() {
    let directory = tempfile::tempdir().unwrap();
    let note_path = directory.path().join("note.md");
    fs::write(&note_path, "before").unwrap();
    let vault = Vault::open(directory.path()).unwrap();
    let mut document = vault.open_note("note.md").unwrap();
    document.remove(0..6).unwrap();
    document.insert(0, "# After\n\n你好\n").unwrap();

    vault.save_note(&mut document).unwrap();

    assert_eq!(fs::read_to_string(note_path).unwrap(), "# After\n\n你好\n");
    assert!(!document.is_dirty());
    assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".synapse-save-")
    }));
}

#[test]
fn ac5_rejects_absolute_parent_and_non_markdown_paths() {
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(directory.path().join("note.txt"), "text").unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.open_note(outside.path()),
        Err(VaultError::InvalidNotePath(_))
    ));
    assert!(matches!(
        vault.open_note("../outside.md"),
        Err(VaultError::InvalidNotePath(_))
    ));
    assert!(matches!(
        vault.open_note("note.txt"),
        Err(VaultError::NotMarkdown(_))
    ));
}

#[test]
fn ec1_rejects_empty_and_current_directory_paths() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.open_note(""),
        Err(VaultError::InvalidNotePath(_))
    ));
    assert!(matches!(
        vault.open_note("."),
        Err(VaultError::InvalidNotePath(_))
    ));
}

#[cfg(unix)]
#[test]
fn ec4_rejects_file_and_directory_symlink_paths() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("real")).unwrap();
    fs::write(directory.path().join("real/note.md"), "real").unwrap();
    symlink(
        directory.path().join("real/note.md"),
        directory.path().join("linked.md"),
    )
    .unwrap();
    symlink(
        directory.path().join("real"),
        directory.path().join("linked-directory"),
    )
    .unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.open_note("linked.md"),
        Err(VaultError::UnsafeNotePath(_))
    ));
    assert!(matches!(
        vault.open_note("linked-directory/note.md"),
        Err(VaultError::UnsafeNotePath(_))
    ));
}

#[test]
fn ec5_missing_discovered_note_returns_io_error() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.open_note("missing.md"),
        Err(VaultError::Io { path, .. }) if path == vault.root().join("missing.md")
    ));
}

#[test]
fn ec6_invalid_utf8_returns_typed_error() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("invalid.md"), [0xff, 0xfe]).unwrap();
    let vault = Vault::open(directory.path()).unwrap();

    assert!(matches!(
        vault.open_note("invalid.md"),
        Err(VaultError::InvalidUtf8(path)) if path == vault.root().join("invalid.md")
    ));
}

#[test]
fn ec7_failed_save_keeps_document_dirty() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("nested")).unwrap();
    fs::write(directory.path().join("nested/note.md"), "before").unwrap();
    let vault = Vault::open(directory.path()).unwrap();
    let mut document = vault.open_note("nested/note.md").unwrap();
    document.insert(document.len_chars(), " after").unwrap();
    fs::remove_file(directory.path().join("nested/note.md")).unwrap();
    fs::remove_dir(directory.path().join("nested")).unwrap();

    assert!(matches!(
        vault.save_note(&mut document),
        Err(VaultError::Io { .. })
    ));
    assert!(document.is_dirty());
    assert_eq!(document.text(), "before after");
}

#[test]
fn ec8_empty_edits_are_noops() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("note.md"), "abc").unwrap();
    let mut document = Vault::open(directory.path())
        .unwrap()
        .open_note("note.md")
        .unwrap();

    document.insert(0, "").unwrap();
    document.remove(0..0).unwrap();
    document.remove(3..3).unwrap();

    assert_eq!(document.text(), "abc");
    assert_eq!(document.revision(), 0);
    assert!(!document.is_dirty());
}
