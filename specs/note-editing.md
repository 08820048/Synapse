# Spec: Open, Edit, and Save a Markdown Note

**Author:** Codex
**Date:** 2026-08-10
**Status:** Approved
**Reviewers:** Product owner (approved by the request to continue substantive feature development)
**Related specs:** `docs/Synapse产品需求文档.md` v1.1, `specs/phase-0-foundation.md`

> V1 note: `specs/v1-workspace-tabs.md` supersedes FR-11 and AC-10. Switching notes now preserves dirty documents in independent tabs; dirty protection applies when closing tabs or switching Vaults.

## Context

The Phase 0 application can discover Markdown files and render a native shell, but users cannot interact with note contents. The product is therefore not yet a functional knowledge workspace. The next slice must complete the smallest meaningful writing loop: select a discovered note, view its exact UTF-8 content, edit it, and save it back to the local file system.

The file system remains the source of truth, while a `ropey::Rope` editor buffer is the in-memory source of truth for the active note. Path validation and save failure behavior are part of this slice because an editor must never write outside its opened vault or silently discard user changes.

## Functional Requirements

- FR-1: The core MUST open an existing Markdown note from a vault-relative path and preserve its exact UTF-8 text.
- FR-2: The active note MUST store editable content in a `ropey::Rope` buffer.
- FR-3: Text insertion and deletion MUST operate on Unicode scalar-value character indices and update a monotonically increasing revision.
- FR-4: A newly opened note MUST be clean; a successful content change MUST mark it dirty; a successful save MUST mark the current revision clean.
- FR-5: Saving MUST persist the buffer's exact UTF-8 text to the originally opened note.
- FR-6: Open and save operations MUST reject absolute paths, parent-directory traversal, non-Markdown paths, and symbolic-link targets.
- FR-7: Selecting a file-tree note in the UI MUST load it into the centered editor area.
- FR-8: The UI MUST accept basic keyboard character insertion, Enter for newline, Backspace, Delete, Left, Right, Home, and End for the active note.
- FR-9: `Cmd+S` on macOS and `Ctrl+S` on Windows/Linux MUST save the active note and update the visible status from modified to saved.
- FR-10: When open or save fails, the UI MUST show the error and MUST preserve the current in-memory buffer.
- FR-11: Selecting a different note while the active note is dirty MUST be refused until the active note is saved.
- FR-12: When no initial Vault path is provided, the UI MUST expose an in-app control that opens the operating system folder picker and loads the selected folder as the Vault.
- FR-13: Long note lines MUST soft-wrap inside the editor without expanding the editor column or pushing navigation outside the window.

## Non-Functional Requirements

- NFR-P1: Core insertion and deletion MUST use Rope character operations and MUST NOT rebuild the full document for each edit.
- NFR-R1: A save MUST write a temporary file in the note's directory and atomically replace the original on supported local file systems.
- NFR-R2: A failed save MUST leave the buffer dirty and MUST NOT report a successful saved state.
- NFR-S1: Validated note paths MUST remain inside the canonical vault root after symbolic-link resolution.
- NFR-A1: Every file-tree note MUST be reachable by keyboard focus in a future accessibility slice; this slice MUST at minimum provide mouse activation and a visible selected state.

## Acceptance Criteria

### AC-1: Open exact Markdown content (FR-1, FR-2, FR-4)
Given a vault containing a UTF-8 Markdown note with multiple lines and Unicode text
When the note is opened by its relative path
Then the returned document contains the exact original text in a Rope buffer
And its revision is zero and it is not dirty

### AC-2: Edit using character indices (FR-3, FR-4, NFR-P1)
Given an opened note containing multi-byte Unicode characters
When text is inserted and removed at valid character indices
Then the resulting text matches the requested edits
And the revision increases once per successful edit and the document is dirty

### AC-3: Reject invalid edit ranges (FR-3, FR-4)
Given an opened note
When insertion or deletion uses an out-of-bounds or reversed character range
Then a typed buffer error is returned
And the content, revision, and dirty state remain unchanged

### AC-4: Save exact content atomically (FR-5, NFR-R1, NFR-R2)
Given a dirty opened note
When save succeeds
Then the original file contains the buffer's exact UTF-8 text
And the document is no longer dirty
And no temporary save file remains in the note directory

### AC-5: Reject unsafe note paths (FR-6, NFR-S1)
Given a valid vault
When a caller opens an absolute path, a parent traversal path, a non-Markdown path, or a symlink
Then a typed path error is returned
And no file outside the vault is read or modified

### AC-6: Select and display a note (FR-7, NFR-A1)
Given the GPUI shell displays discovered notes
When the user clicks a note in the file tree
Then that note becomes visibly selected
And its title, full text, cursor, and saved state appear in the editor

### AC-7: Edit a selected note (FR-8)
Given a selected note and focused editor
When the user types characters, inserts a newline, navigates, or deletes text
Then the active Rope buffer and rendered editor content update consistently
And the status changes to modified after the first content edit

### AC-8: Save from the keyboard (FR-9, NFR-R2)
Given a dirty active note
When the user presses the platform save shortcut
Then the active note is saved to disk
And the visible status changes to saved only after the write succeeds

### AC-9: Preserve content after an error (FR-10, NFR-R2)
Given a note is active in the editor
When opening another note or saving the active note fails
Then the prior in-memory content remains available
And a visible error message describes the failed operation

### AC-10: Prevent unsaved note loss (FR-11, NFR-R2)
Given the active note has unsaved edits
When the user selects a different note
Then the active note and its in-memory content remain unchanged
And a visible message instructs the user to save before switching

### AC-11: Open a Vault from the application (FR-12)
Given Synapse starts without a command-line Vault path
When the user activates `Open Vault` and selects a local folder
Then the folder is validated and its Markdown notes populate the file tree
And canceling the picker leaves the current state unchanged
And an invalid selection reports an error without discarding the previously opened Vault

### AC-12: Keep the workspace visible while editing long lines (FR-13)
Given a note contains text wider than the editor viewport
When the note is displayed or edited
Then its text wraps within the editor column
And the editor header, status, and expanded navigation remain inside the window

## Edge Cases

- EC-1: Relative path is empty or `.` → return `VaultError::InvalidNotePath`.
- EC-2: Relative path contains `..`, a root, or a platform prefix → return `VaultError::InvalidNotePath` before file access.
- EC-3: Relative path extension is not `.md` case-insensitively → return `VaultError::NotMarkdown`.
- EC-4: Note path resolves through a symbolic link → return `VaultError::UnsafeNotePath`.
- EC-5: Note disappears between discovery and open → return `VaultError::Io` without changing the active UI document.
- EC-6: File content is invalid UTF-8 → return `VaultError::InvalidUtf8`.
- EC-7: Save cannot create or replace the file → return `VaultError::Io`, keep the document dirty, and remove any created temporary file when possible.
- EC-8: Backspace at character index zero or Delete at document end → perform no edit and do not increment revision.
- EC-9: User invokes save with no active note → leave UI state unchanged and do not access the file system.
- EC-10: User selects the already active note while it is dirty → retain the same document without error or reload.
- EC-11: User cancels the folder picker → leave the Vault, active note, and status unchanged.
- EC-12: User tries to switch Vaults with a dirty active note → refuse the switch and preserve the active buffer.

## API Contracts

N/A — this local desktop slice exposes Rust APIs rather than HTTP endpoints:

```rust
pub struct NoteDocument {
    relative_path: PathBuf,
    buffer: ropey::Rope,
    revision: u64,
    saved_revision: u64,
}

pub enum BufferError {
    CharacterIndexOutOfBounds { index: usize, len: usize },
    InvalidCharacterRange { start: usize, end: usize, len: usize },
}

impl NoteDocument {
    pub fn relative_path(&self) -> &Path;
    pub fn text(&self) -> String;
    pub fn len_chars(&self) -> usize;
    pub fn revision(&self) -> u64;
    pub fn is_dirty(&self) -> bool;
    pub fn insert(&mut self, char_index: usize, text: &str) -> Result<(), BufferError>;
    pub fn remove(&mut self, range: Range<usize>) -> Result<(), BufferError>;
}

impl Vault {
    pub fn open_note(&self, relative_path: impl AsRef<Path>) -> Result<NoteDocument, VaultError>;
    pub fn save_note(&self, document: &mut NoteDocument) -> Result<(), VaultError>;
}
```

## Data Models

### NoteDocument

| Field | Type | Constraints |
|---|---|---|
| relative_path | `PathBuf` | Validated vault-relative Markdown path, immutable after open |
| buffer | `ropey::Rope` | In-memory source of truth for active content |
| revision | `u64` | Starts at 0; increments per successful content-changing edit |
| saved_revision | `u64` | Equals `revision` only when current content is persisted |

### EditorSession

| Field | Type | Constraints |
|---|---|---|
| active_document | `Option<NoteDocument>` | At most one active note in this slice |
| cursor | `usize` | Unicode character index, never greater than document length |
| status_message | `String` | Visible saved, modified, or error state |

## Out of Scope

- OS-1: Mouse-based cursor placement, drag selection, and multi-cursor editing — requires shaped multi-line hit testing.
- OS-2: IME composition and accessibility-grade text input — a dedicated editor-input slice is required before release.
- OS-3: Undo/redo, clipboard operations, syntax highlighting, and Markdown preview — subsequent editor milestones.
- OS-4: Automatic save and external-change conflict resolution — requires file watching and revision conflict rules.
- OS-5: Creating, renaming, moving, and deleting notes — separate file-management slice.
- OS-6: Multiple tabs, split editors, and persistent cursor positions — later workspace-state work.
