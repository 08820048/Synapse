# Spec: V1 Workspace Tabs and Unified Layout

**Author:** Codex  
**Date:** 2026-08-10  
**Status:** Approved  
**Reviewers:** Product owner (approved the V1 tab and layout slice)  
**Related specs:** `specs/note-editing.md`, `specs/phase-0-foundation.md`

> V2 note: `specs/v2-editor-navigation.md` supersedes the Backlinks/right-sidebar portions of FR-11 through FR-14 and AC-9. V1 tab behavior remains current.

## Context

The current workspace can open, edit, and save one Markdown note, but selecting another note replaces the active document. It has no visible document history and cannot preserve independent cursor and dirty state for several notes. The current three floating cards also fragment the workspace and place sidebar controls inside their individual panels instead of a single editor toolbar.

V1 introduces a Zed-style unified workspace: multiple open document tabs in the center, thin separators between the three regions, and one aligned top toolbar controlling both sidebars. The file system remains the source of truth, while each open tab owns an independent in-memory `NoteDocument` and cursor.

## Functional Requirements

- FR-1: Selecting a file-tree note that is not open MUST create a document tab and make it active.
- FR-2: Selecting an already-open note MUST activate its existing tab without reloading or duplicating it.
- FR-3: Every open tab MUST preserve its own document buffer, dirty state, and Unicode character cursor while another tab is active.
- FR-4: Clicking a tab MUST make its document the active editor document.
- FR-5: Every tab MUST display its note name, dirty state, and a close control on its right side.
- FR-6: Right-clicking a tab MUST show actions for Close, Close Left, Close Right, and Close All.
- FR-7: Close Left, Close Right, and Close All MUST apply to the tab that opened the context menu, not implicitly to another tab.
- FR-8: Closing the active tab MUST activate the nearest remaining tab, preferring the tab formerly to its right and then the tab to its left.
- FR-9: A close operation that includes any dirty tab MUST be refused atomically and MUST preserve every affected tab and its buffer.
- FR-10: Switching Vaults while any tab is dirty MUST be refused and MUST preserve the current Vault and all tabs.
- FR-11: The workspace MUST render the left sidebar, editor, and right backlinks sidebar as one contiguous surface separated only by one-pixel dividers.
- FR-12: The top toolbar MUST contain controls that independently collapse and expand the left and right sidebars.
- FR-13: Top-toolbar controls MUST share one vertical center line, including the macOS traffic-light region.
- FR-14: Collapsing either sidebar MUST give the released width to the editor without closing tabs or changing the active document.

## Non-Functional Requirements

- NFR-R1: Tab activation and sidebar visibility changes MUST NOT read or write note files.
- NFR-R2: Failed tab-close and Vault-switch operations MUST NOT discard unsaved buffer content.
- NFR-P1: Activating an existing tab MUST complete by index/path lookup without rescanning the Vault.
- NFR-U1: At a window width of 900 pixels, the active editor and every expanded fixed-width sidebar MUST remain inside the window bounds.
- NFR-U2: Tab labels and note text MUST be constrained so long content cannot expand the workspace beyond its window bounds.

## Acceptance Criteria

### AC-1: Open multiple notes (FR-1, FR-3)
Given a Vault containing two Markdown notes
When the user selects the first note and then the second note
Then two tabs exist in selection order
And the second tab is active
And the first tab retains its document buffer and cursor

### AC-2: Reuse an existing tab (FR-2, NFR-P1)
Given two notes are already open and the first note contains unsaved edits
When the user selects the first note again from the file tree
Then the existing first tab becomes active
And no duplicate tab is created
And its unsaved buffer is unchanged

### AC-3: Switch tabs (FR-4, FR-3, NFR-R1)
Given two tabs have different cursor positions
When the user activates each tab in turn
Then each tab exposes its own prior document and cursor
And neither note is reloaded from disk

### AC-4: Close a clean tab (FR-5, FR-8)
Given three clean tabs and the middle tab is active
When the middle tab close control is activated
Then the middle tab is removed
And the tab formerly to its right becomes active

### AC-5: Context close left (FR-6, FR-7)
Given four clean tabs
When the third tab is right-clicked and Close Left is selected
Then only the first and second tabs are closed
And the third and fourth tabs remain

### AC-6: Context close right (FR-6, FR-7)
Given four clean tabs
When the second tab is right-clicked and Close Right is selected
Then only the third and fourth tabs are closed
And the first and second tabs remain

### AC-7: Context close all (FR-6, FR-7)
Given several clean tabs
When any tab is right-clicked and Close All is selected
Then every tab is closed
And the editor shows its empty state

### AC-8: Protect dirty tabs during close (FR-9, NFR-R2)
Given a close operation targets at least one dirty tab
When the close action is invoked
Then no targeted tab is closed
And all tab buffers and the active selection remain unchanged
And the UI reports that modified tabs must be saved first

### AC-9: Unified workspace and sidebar toggles (FR-11, FR-12, FR-14, NFR-U1)
Given the workspace is open with both sidebars visible
When the left and right toolbar controls are activated independently
Then the corresponding sidebar collapses or expands
And the editor consumes the released or restored width
And open tabs and the active document remain unchanged

### AC-10: Aligned top toolbar (FR-13)
Given the application window is rendered on macOS
When the top toolbar is displayed
Then both sidebar controls and other toolbar actions share one vertical center line
And the toolbar background extends behind the transparent system titlebar

### AC-11: Constrain long content (NFR-U2)
Given a tab has a long filename and its document has a long line
When the workspace renders at its minimum width
Then the tab label is truncated or scroll-constrained
And document text wraps inside the editor
And no expanded sidebar is pushed outside the window

### AC-12: Protect dirty tabs during Vault switch (FR-10, NFR-R2)
Given at least one open tab contains unsaved edits
When the user selects a different Vault
Then the Vault switch is refused
And the current Vault, all tabs, their buffers, and the active selection remain unchanged

## Edge Cases

- EC-1: Activating a tab index that does not exist → return `SessionError::InvalidTabIndex` without changing the active tab.
- EC-2: Closing a tab index that does not exist → return `SessionError::InvalidTabIndex` without changing any tab.
- EC-3: Closing the only clean tab → leave no active tab and show the editor empty state.
- EC-4: Closing an inactive tab before the active tab → keep the same document active and adjust its index.
- EC-5: Close Left on the first tab or Close Right on the last tab → succeed as a no-op.
- EC-6: A bulk-close range contains one dirty tab among clean tabs → refuse the entire operation; do not partially close clean tabs.
- EC-7: Opening a note fails after other tabs are open → preserve all existing tabs and the active tab.
- EC-8: User clicks outside a tab context menu or invokes an action → dismiss the menu.
- EC-9: Either sidebar is already collapsed → its toggle expands it; visibility state is independent of the other sidebar.

## API Contracts

N/A — V1 is a local desktop feature with Rust state APIs rather than HTTP endpoints. The state boundary is:

```rust
pub struct TabInfo {
    pub relative_path: PathBuf,
    pub title: String,
    pub is_dirty: bool,
}

impl ShellState {
    pub fn tabs(&self) -> Vec<TabInfo>;
    pub fn active_tab_index(&self) -> Option<usize>;
    pub fn activate_tab(&mut self, index: usize) -> Result<(), SessionError>;
    pub fn close_tab(&mut self, index: usize) -> Result<bool, SessionError>;
    pub fn close_tabs_left(&mut self, index: usize) -> Result<usize, SessionError>;
    pub fn close_tabs_right(&mut self, index: usize) -> Result<usize, SessionError>;
    pub fn close_all_tabs(&mut self) -> Result<usize, SessionError>;
}
```

## Data Models

### OpenTab

| Field | Type | Constraints |
|---|---|---|
| document | `NoteDocument` | One unique Vault-relative path per open tab |
| cursor | `usize` | Unicode character index; never greater than document length |

### TabInfo

| Field | Type | Constraints |
|---|---|---|
| relative_path | `PathBuf` | Immutable identity of the open note |
| title | `String` | Filename displayed in the tab |
| is_dirty | `bool` | Mirrors the current `NoteDocument` dirty state |

### WorkspaceViewState

| Field | Type | Constraints |
|---|---|---|
| left_sidebar_open | `bool` | Independent toggle; defaults to `true` |
| right_sidebar_open | `bool` | Independent toggle; defaults to `true` |
| tab_context_menu | `Option<usize>` | Index of the tab that owns the visible context menu |

## Out of Scope

- OS-1: Drag-to-reorder tabs — requires pointer drag state and insertion indicators; deferred beyond V1 basics.
- OS-2: Persisting open tabs across application restarts — requires workspace-session persistence.
- OS-3: Split editor groups — separate workspace-layout milestone.
- OS-4: Unsaved-close confirmation dialogs — V1 refuses destructive closes instead of introducing modal decision flows.
- OS-5: Functional backlinks — the V1 requirement covers only the right sidebar layout and visibility.
- OS-6: Mobile or touch-specific tab interactions — the current product target is native desktop.
