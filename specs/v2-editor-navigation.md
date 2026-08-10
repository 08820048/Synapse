# Spec: V2 Markdown Editor Navigation

**Author:** Codex  
**Date:** 2026-08-10  
**Status:** Approved  
**Reviewers:** Product owner (approved through the explicit request to implement `docs/Task.md` V2)  
**Related specs:** `docs/Task.md`, `specs/v1-workspace-tabs.md`, `specs/note-editing.md`

## Context

V1 introduced multi-document tabs and a contiguous three-region workspace, but product acceptance found that the tab context menu is not reachable through an actual right-button mouse event. The product direction has also changed: Synapse is now a focused, native Markdown editor rather than a knowledge workspace centered on backlinks and graphs.

V2 first updates the editor chrome to match the supplied references. The left navigation becomes the primary file surface, with a search launcher, Todo and Bookmark shortcuts, note creation affordances, a hierarchical file list, and a bottom Settings entry. Search, Todo, Bookmark, Settings, and file-creation business logic are intentionally deferred; this slice establishes their visible and interactive UI entry points and the central command-palette shell.

## Functional Requirements

- FR-1: A native right-button mouse-down on a document tab MUST open that tab's context menu.
- FR-2: The tab context menu MUST remain available after the corresponding right-button mouse-up and MUST expose Close, Close Left, Close Right, and Close All.
- FR-3: Product-facing documentation MUST describe Synapse as a local-first Markdown editor and MUST remove backlinks, knowledge graphs, and knowledge-workspace positioning from the current roadmap.
- FR-4: The default editor layout MUST remove the right Backlinks sidebar and its toolbar toggle.
- FR-5: The left sidebar MUST display a search launcher as its first content element.
- FR-6: Activating the search launcher MUST open a centered floating command-palette shell above a dimmed editor backdrop.
- FR-7: Clicking outside the command palette MUST dismiss it without changing files or tabs.
- FR-8: The command palette MUST display a search/command prompt row and representative command rows, including New Note and Open Vault.
- FR-9: The left sidebar MUST display Todo and Bookmark shortcut rows below the search launcher and above a divider.
- FR-10: The left sidebar MUST display a Notes section header with separate new-file and new-folder icon controls on its right.
- FR-11: The file area MUST render Vault-relative directory rows and Markdown note rows with indentation derived from their path depth.
- FR-12: Activating a Markdown note row MUST preserve the existing V1 open-or-activate-tab behavior.
- FR-13: The left sidebar SHOULD display a Settings entry pinned to its bottom edge.
- FR-14: Todo, Bookmark, Settings, new-file, and new-folder controls MUST be visibly interactive placeholders in this slice and MUST NOT mutate local files.

## Non-Functional Requirements

- NFR-R1: Opening or dismissing the command palette MUST NOT mutate the Vault, tab collection, or active document.
- NFR-R2: Placeholder controls MUST NOT create, rename, move, or delete file-system entries.
- NFR-P1: Building the visible file hierarchy MUST operate on the existing discovery snapshot and MUST NOT rescan or read note contents.
- NFR-U1: The search launcher, shortcut rows, Notes header, tree rows, and Settings row MUST fit inside a 248-pixel sidebar without horizontal expansion.
- NFR-U2: The command palette MUST be no wider than 560 pixels and remain centered inside a 900-pixel minimum-width window.

## Acceptance Criteria

### AC-1: Open the tab context menu with native right-click (FR-1, FR-2)
Given at least one document tab is open
When the user presses the right mouse button on that tab
Then the context menu becomes visible for that tab
And releasing the right mouse button does not immediately dismiss it

### AC-2: Reposition the product as an editor (FR-3, FR-4)
Given the current product documentation and default workspace
When V2 is applied
Then current documentation identifies Synapse as a Markdown editor
And backlinks and graph features are absent from the current roadmap
And the default workspace has no Backlinks sidebar or right-sidebar toggle

### AC-3: Open and dismiss the command palette (FR-5, FR-6, FR-7, NFR-R1)
Given the editor window is visible
When the user clicks the search launcher
Then a centered command palette appears above a dimmed backdrop
When the user clicks the backdrop
Then the palette closes
And the Vault, tabs, and active document are unchanged

### AC-4: Render shortcut and note controls (FR-9, FR-10, FR-13, NFR-U1)
Given the left sidebar is expanded
When it is rendered
Then Todo and Bookmark rows appear below the search launcher
And a divider separates them from the Notes section
And the Notes header contains new-file and new-folder controls
And Settings is anchored at the bottom

### AC-5: Render hierarchical files (FR-11, NFR-P1)
Given discovered notes include `root.md`, `product/plan.md`, and `product/archive/old.md`
When sidebar rows are derived
Then one `product` directory row precedes `product/plan.md`
And one nested `archive` directory row precedes `product/archive/old.md`
And row depths are 0, 1, and 2 according to their path hierarchy

### AC-6: Open notes from the new tree (FR-12)
Given a note is visible in the V2 file area
When its row is clicked twice
Then the existing tab is activated
And no duplicate tab is created

### AC-7: Keep deferred controls non-destructive (FR-14, NFR-R2)
Given the Vault has a known directory snapshot
When Todo, Bookmark, Settings, new-file, or new-folder placeholder controls are activated
Then no file-system entry is created, changed, or deleted

### AC-8: Constrain the command palette (FR-8, NFR-U2)
Given the editor is at its 900-pixel minimum width
When the command palette is open
Then its panel width is at most 560 pixels
And its prompt and command rows remain inside the panel

## Edge Cases

- EC-1: Right-click occurs on a tab close icon → open the owning tab context menu without invoking close.
- EC-2: Right-click occurs with a stale tab index after a synchronous close → state validation rejects later menu actions without panic.
- EC-3: Search launcher is clicked while the palette is already open → keep one palette instance visible.
- EC-4: Palette is dismissed with no Vault open → return to the original empty editor state.
- EC-5: Multiple notes share a directory → emit the directory row once.
- EC-6: A note is at the Vault root → render it at depth zero without a synthetic root row.
- EC-7: A note is nested several directories deep → emit every previously unseen ancestor once in parent-before-child order.
- EC-8: A path or label exceeds the sidebar width → truncate the label; do not expand the sidebar.

## API Contracts

N/A — V2 is a local GPUI feature and exposes no HTTP endpoints. The pure hierarchy boundary is:

```rust
enum FileTreeRow {
    Directory { relative_path: PathBuf, name: String, depth: usize },
    Note { relative_path: PathBuf, name: String, depth: usize },
}

fn build_file_tree_rows(note_paths: &[PathBuf]) -> Vec<FileTreeRow>;
```

## Data Models

### FileTreeRow

| Field | Type | Constraints |
|---|---|---|
| kind | `Directory` or `Note` | Directory ancestors precede their first descendant note |
| relative_path | `PathBuf` | Vault-relative; unique for directories and notes independently |
| name | `String` | Final path component; note extension omitted for display |
| depth | `usize` | Root entries are 0; increases once per directory level |

### V2ViewState

| Field | Type | Constraints |
|---|---|---|
| left_sidebar_open | `bool` | Defaults to `true` |
| command_palette_open | `bool` | At most one palette instance |
| tab_context_menu | `Option<usize>` | Owning tab index for the visible menu |

## Out of Scope

- OS-1: Search query entry, filtering, indexing, and result navigation — explicitly deferred by `docs/Task.md` V2.
- OS-2: Todo persistence and task workflows — explicitly deferred by `docs/Task.md` V2.
- OS-3: Bookmark persistence and bookmark navigation — explicitly deferred by `docs/Task.md` V2.
- OS-4: Settings implementation — only the navigation entry is included.
- OS-5: Actual new-file and new-folder creation — this slice establishes the referenced UI controls without mutating the Vault.
- OS-6: Directory collapse state, rename, drag, move, and delete — separate file-management slice.
- OS-7: Backlinks, bidirectional links, and knowledge graphs — removed from the Markdown-editor product direction.
