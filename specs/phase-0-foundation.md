# Spec: Phase 0 Foundation and Vault Slice

**Author:** Codex
**Date:** 2026-08-10
**Status:** Approved
**Reviewers:** Product owner (approval inferred from the request to begin development from PRD v1.1)
**Related specs:** `docs/Synapse产品需求文档.md` v1.1

> Historical foundation note: V2 changes the product to a Markdown editor and supersedes the Backlinks region in FR-7 and AC-6. The current layout is left navigation plus the central editor.

## Context

Synapse currently contains an approved product requirements document but no buildable code. The first implementation slice must prove the native Rust + GPUI toolchain, establish module boundaries that can grow into the MVP, and exercise the product's file-system-as-source-of-truth model.

The slice intentionally starts with read-only vault discovery and a native three-column shell. This exposes the two highest-risk foundations early—GPUI application startup and deterministic local note discovery—without prematurely implementing the professional editor, indexing, or link graph.

## Functional Requirements

- FR-1: The repository MUST be a Cargo workspace with separate `synapse-core` and `synapse-ui` crates.
- FR-2: `synapse-core` MUST open an existing local directory as a `Vault` and retain its canonical root path.
- FR-3: `synapse-core` MUST recursively discover regular files whose extension is `.md`, using ASCII case-insensitive extension matching.
- FR-4: Vault discovery MUST return notes in deterministic relative-path order.
- FR-5: Each discovered note MUST expose its vault-relative path and a title derived from its file stem.
- FR-6: Vault discovery MUST NOT traverse symbolic links or include non-Markdown files.
- FR-7: `synapse-ui` MUST start a native GPUI window with a file-tree panel, centered editor area, and backlinks panel.
- FR-8: When a vault path is supplied as the first command-line argument, the UI MUST display its discovered notes in the file-tree panel.
- FR-9: When no vault path is supplied, the UI MUST remain usable and show an empty-vault prompt.

## Non-Functional Requirements

- NFR-P1: The release application MUST preserve the PRD release target of cold startup below 0.8 seconds on the reference Apple Silicon development machine; this slice MUST avoid network initialization and database startup.
- NFR-P2: Vault discovery MUST use a single streaming directory traversal and MUST NOT read note contents.
- NFR-R1: File-system failures MUST be returned as typed errors rather than panics.
- NFR-C1: The implementation MUST compile on Rust 1.93.1 and pin GPUI to the tested `0.2.2` release; local development MAY use GPUI's `runtime_shaders` feature when the full Xcode Metal compiler is unavailable.

## Acceptance Criteria

### AC-1: Open a valid vault (FR-2, NFR-R1)
Given an existing temporary directory
When the core opens it as a vault
Then the returned vault root is the directory's canonical path

### AC-2: Reject an invalid vault root (FR-2, NFR-R1)
Given a missing path or a regular file path
When the core attempts to open it as a vault
Then it returns a typed `NotFound` or `NotDirectory` error and does not panic

### AC-3: Discover Markdown notes deterministically (FR-3, FR-4, FR-5, NFR-P2)
Given a vault containing nested `.md` and `.MD` files in arbitrary creation order
When notes are discovered
Then every Markdown note is returned once in relative-path order with a file-stem title

### AC-4: Exclude unsupported entries (FR-6, NFR-P2)
Given a vault containing text files and a symbolic link to a Markdown file
When notes are discovered
Then neither the text file nor symbolic link appears in the result

### AC-5: Workspace boundaries compile (FR-1, NFR-C1)
Given the initialized repository and Rust 1.93.1
When `cargo check --workspace` runs
Then both workspace crates compile successfully with GPUI 0.2.2

### AC-6: Render the native shell with a vault (FR-7, FR-8)
Given a valid vault path passed as the first argument
When the Synapse application starts
Then it opens one GPUI window whose visible regions are the file tree, centered editor, and backlinks panel
And the file tree contains each discovered note path

### AC-7: Render the native shell without a vault (FR-7, FR-9)
Given no command-line vault path
When the Synapse application starts
Then it opens one GPUI window and displays an instruction to open a vault

### AC-8: Keep startup local and lightweight (NFR-P1)
Given the application starts with no vault path
When the startup path is inspected
Then it performs no network request and initializes no database or search index

## Edge Cases

- EC-1: A vault root does not exist → return `VaultError::NotFound`.
- EC-2: A vault root is a regular file → return `VaultError::NotDirectory`.
- EC-3: A directory entry becomes inaccessible during discovery → return `VaultError::Io` with its path.
- EC-4: A file has no file stem or has a non-UTF-8 name → skip it without panicking.
- EC-5: A directory contains a symlink cycle → do not follow the symlink, so discovery terminates.
- EC-6: The UI receives an invalid vault argument → open the shell and display the typed error instead of terminating.

## API Contracts

N/A — Phase 0 is a local desktop application and exposes no HTTP API. Its public Rust contracts are:

```rust
pub struct Vault {
    root: PathBuf,
}

pub struct NoteEntry {
    pub relative_path: PathBuf,
    pub title: String,
}

pub enum VaultError {
    NotFound(PathBuf),
    NotDirectory(PathBuf),
    Io { path: PathBuf, source: std::io::Error },
}

impl Vault {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VaultError>;
    pub fn root(&self) -> &Path;
    pub fn discover_notes(&self) -> Result<Vec<NoteEntry>, VaultError>;
}
```

## Data Models

### Vault

| Field | Type | Constraints |
|---|---|---|
| root | `PathBuf` | Canonical, absolute, existing directory, immutable after open |

### NoteEntry

| Field | Type | Constraints |
|---|---|---|
| relative_path | `PathBuf` | Relative to vault root, unique within a discovery result |
| title | `String` | UTF-8 file stem, non-empty |

### AppState

| Field | Type | Constraints |
|---|---|---|
| vault_name | `Option<String>` | Derived from the opened root directory name |
| notes | `Vec<NoteEntry>` | Deterministically ordered snapshot |
| vault_error | `Option<String>` | Human-readable typed-open/discovery error |

## Out of Scope

- OS-1: Editing and saving note contents — requires a dedicated editor-buffer spec and rope integration.
- OS-2: Creating, renaming, moving, deleting, or dragging files — deferred until read-only discovery is stable.
- OS-3: Markdown rendering, syntax highlighting, math, and image insertion — separate editor/rendering milestones.
- OS-4: Backlink parsing, search indexing, and file watching — separate core services after the workspace foundation.
- OS-5: Floating/dockable panel interactions — this slice proves the three-region visual shell only.
- OS-6: Release performance certification on Windows, Linux, and macOS — retained as an MVP release gate, not claimed by the first slice.
