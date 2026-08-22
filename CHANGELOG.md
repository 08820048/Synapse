# Changelog

All notable changes to Synapse are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses a `0.y.z` preview version until the first stable editor release.

## [Unreleased]

## [0.3.0] - 2026-08-22

### Added

- A dedicated Git workspace and sidebar change panel with wrapped, scrollable, inline-highlighted diffs, commit history, explicit commit/pull/push/sync actions, conflict visibility, and system-Git credential reuse
- A cached statistics dashboard for notes, activity, content, tags, tasks, and note-reference relationships

## [0.2.0] - 2026-08-21

### Added

- Editor undo and redo, with coalesced typing/backspace, per-tab history, and `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z`
- Native update checks against GitHub Releases, with a startup prompt, a sidebar “有更新” affordance, a top-level Settings page, and a command-palette entry that open the platform installer
- Filename and full-text note search, plus in-note find, previous/next match, replace, and replace-all
- Word-wise movement and selection, double-click word selection, and triple-click line selection
- Drag-reorderable tabs with restored order, active tab, cursor, and pin state
- One-second autosave, dirty-tab discard confirmation, external edit conflict protection, and crash recovery copies
- Code completion, language-server integration, richer fenced-code editing, and list-to-todo conversion

### Changed

- Large notes use viewport-bounded progressive rendering, cached previews, and deferred syntax highlighting
- Settings use consistent component buttons in a titlebar-free layout
- The application icon now uses a restrained graphite folded-page design
- Application, editor, platform, and workspace code is split into focused modules

### Fixed

- Unified editor cursor weight and table caret placement
- Reduced excess blank-line spacing around Markdown blocks
- Preserved collapsed file-tree state while excluding hidden directories
- Hardened rename and overlay interactions and refreshed tab borders after theme changes

## [0.1.2] - 2026-08-18

### Fixed

- Windows clippy and Ubuntu rustfmt accept the macOS app-bundle icon helper

## [0.1.1] - 2026-08-18

### Added

- Tag-triggered GitHub Release workflow that publishes a universal macOS DMG and a Windows x64 setup EXE
- `scripts/package-macos.sh --dmg --universal` for a drag-to-Applications disk image
- `scripts/package-windows.ps1` and an Inno Setup script for `Synapse-<version>-windows-x64.exe`
- Windows application icon (`assets/branding/synapse-app-icon.ico`)
- Windows job in CI (`clippy` + tests)

### Fixed

- Packaged macOS apps keep the bundle `.icns` so Dock and the app switcher use the system rounded mask
- Windows release packaging no longer tries to copy `synapse.exe` onto itself
- Windows CI compiles after unused icon bytes and italic-fallback arguments were gated by platform

## [0.1.0] - 2026-08-18

First public source release. This is an early macOS preview, not a finished editor.

### Added

- Native Rust + GPUI desktop shell with a unified sidebar and center editor
- Vault discovery for `.md` files and empty folders, with live refresh via `notify`
- File tree create, inline rename, drag-move, Finder reveal, and system trash
- Multi-tab Markdown editing on Rope buffers, with dirty-state protection
- Structured live rendering through `writ`, including headings, lists, tasks, tables, callouts, footnotes, images, Mermaid, and math
- Chinese IME, slash commands, note links, format shortcuts, and clipboard image paste
- Native todo and bookmark workspaces persisted in the user config directory
- Settings window for theme, language, and current vault
- Notification queue and confirmation dialogs for destructive actions
- macOS application icon, ad-hoc `.app` packaging, and bilingual UI strings

### Known gaps

- Undo / redo, word-wise selection, find/replace
- Real filename and full-text search
- Session restore and tab reordering
- Table caret visibility
- Windows / Linux support
