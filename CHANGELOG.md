# Changelog

All notable changes to Synapse are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses a `0.y.z` preview version until the first stable editor release.

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
