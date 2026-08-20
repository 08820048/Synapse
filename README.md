# Synapse

<p align="center">
  <img src="assets/branding/synapse-app-icon.png" alt="Synapse icon" width="128" height="128">
</p>

<p align="center">
  A fast, native, local-first Markdown editor.<br>
  Built with Rust and <a href="https://www.gpui.rs/">GPUI</a>.
</p>

<p align="center">
  <a href="#synapse">English</a> · <a href="#synapse-中文">中文</a>
  · <a href="CHANGELOG.md">Changelog</a>
  · <a href="CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <img alt="CI" src="https://github.com/08820048/Synapse/actions/workflows/ci.yml/badge.svg">
  <img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue">
</p>

Synapse opens an ordinary folder of Markdown files. There is no account, no required cloud, and no database. The filesystem is the source of truth.

This is an early public preview (`0.1.0`). Daily Markdown editing works on macOS. Windows installers are built in CI but the Windows app is not first-class yet.

## Features

- Recursive vault of `.md` files, including empty folders
- Create, rename, drag-move, and trash notes from a native file tree
- Multi-tab editing with independent Rope buffers and dirty-state protection
- Live Markdown rendering: headings, lists, tasks, tables, callouts, footnotes, images, Mermaid, and math
- Source mode is explicit; the default editor stays structured
- Chinese IME, slash commands, format shortcuts, and clipboard image paste
- Native todo and bookmark workspaces, persisted in the user config directory
- System / Light / Dark themes and Simplified Chinese / English UI
- External filesystem changes refresh the sidebar without touching unsaved buffers
- Filename/full-text search, in-note find/replace, and native word/line selection
- Restored and drag-reorderable tabs, one-second autosave, conflict detection, and crash recovery

## Requirements

- Rust 1.93 or later
- macOS is the primary development platform
- Xcode command-line tools for a local macOS build
- Windows 10+ for the setup EXE; Inno Setup 6 is only needed to package locally

## Install

GitHub Actions builds preview installers on each `v*` tag:

- macOS: `Synapse-<version>-macos-universal.dmg` — open the disk image and drag Synapse into Applications
- Windows: `Synapse-<version>-windows-x64.exe` — Inno Setup installer, per-user by default

Download them from [Releases](https://github.com/08820048/Synapse/releases). The current packages are unsigned, so macOS Gatekeeper and Windows SmartScreen will warn.

To cut a release after pushing `main`:

```bash
git tag v0.1.1
git push origin v0.1.1
```

You can also run the **Release** workflow manually from the Actions tab to produce artifacts without publishing.

Installed builds check GitHub Releases once at startup. Settings → Updates and the command palette can run the same check manually. Synapse opens the matching DMG or EXE download; it does not replace the running app in place. Sparkle / WinSparkle can come later once the installers are signed.

## Run from source

```bash
cargo run -p synapse
```

Or open a folder directly:

```bash
cargo run -p synapse -- /path/to/markdown-folder
```

With no argument, Synapse restores the last vault. On first launch it creates `~/Documents/Synapse Vault`.

### Local packages

```bash
# Universal macOS .app + DMG (ad-hoc signed)
./scripts/package-macos.sh --dmg --universal

# Current-architecture .app, then install into /Applications
./scripts/package-macos.sh --install
```

The `.app` is written to `target/release/bundle/osx/Synapse.app`. The DMG is `Synapse-<version>-macos-universal.dmg` in the same folder. Distribution-quality macOS builds still need Developer ID signing and notarization.

On Windows (PowerShell), after installing [Inno Setup 6](https://jrsoftware.org/isinfo.php) and optionally `rcedit`:

```powershell
./scripts/package-windows.ps1
```

The installer is written to `target/release/bundle/windows/Synapse-<version>-windows-x64.exe`.

## Editor shortcuts

| Action | macOS | Windows / Linux |
|---|---|---|
| Save | `Cmd+S` | `Ctrl+S` |
| Undo | `Cmd+Z` | `Ctrl+Z` |
| Redo | `Cmd+Shift+Z` | `Ctrl+Shift+Z` or `Ctrl+Y` |
| Command palette | `Cmd+K` | `Ctrl+K` |
| Find / replace | `Cmd+F` | `Ctrl+F` |
| Move / select by word | `Option+←/→` / add `Shift` | `Ctrl+←/→` / add `Shift` |
| Bold / italic / underline | `Cmd+B` / `I` / `U` | `Ctrl+B` / `I` / `U` |
| Strikethrough | `Cmd+Shift+S` | `Ctrl+Shift+S` |
| Inline code | `Cmd+E` | `Ctrl+E` |
| Fenced code block | `Cmd+Option+C` | `Ctrl+Alt+C` |
| Soft line break | `Shift+Enter` | `Shift+Enter` |

Format shortcuts wrap the current selection. With an empty selection they insert paired marks and leave the caret in the middle.

## Architecture

```
synapse-core   Vault, path safety, directory snapshots, Rope documents, atomic saves
synapse-ui     GPUI window, tabs, editor, todos, bookmarks, command palette
```

Pinned building blocks:

- UI: GPUI 0.2.2 and `gpui-component` 0.5.1
- Buffer: `ropey`
- Markdown kernel: `writ` 0.18.1 (headless, tree-sitter Markdown)
- Diagrams: `rusty-mermaid` (Rust SVG, no WebView)
- Math: `RaTeX` (self-contained SVG)
- Trash: OS recycle bin via `trash`
- Watcher: `notify` with a 180ms trailing debounce

Web UI stacks are out of scope. See [AGENT.md](AGENT.md) for the product constraints.

## Current limitations

- Table caret visibility
- Windows is packaged, but the editor experience is still macOS-first
- Linux packages are not built yet
- Release-grade startup, memory, and huge-file numbers are not certified

## Verify

```bash
cargo fmt --package synapse-core -- --check
cargo fmt --package synapse -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not run `cargo fmt --all`. The vendored `gpui-component` tree stays in upstream form.

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option. Third-party notices are in [NOTICE](NOTICE) and [THIRD_PARTY.md](THIRD_PARTY.md).

## Contributing

Issues and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).

---

# Synapse (中文)

Synapse 是一款使用 Rust 与 GPUI 构建的高性能、本地优先 Markdown 编辑器。它直接打开普通文件夹，不绑定账号、数据库或强制云服务。文件系统是笔记的真实来源。

当前是公开预览（`0.1.0`）。macOS 上已经可以日常写 Markdown。Windows 安装包会随版本标签构建，但 Windows 体验还不是一等公民。

## 它能做什么

- 递归发现 `.md` 文件和空文件夹，并在原生文件树中创建、重命名、拖拽和移到废纸篓
- 多文档页签，独立 Rope 缓冲区，未保存内容受保护
- 实时呈现标题、列表、任务、表格、提示块、脚注、图片、Mermaid 和数学公式
- 默认是结构化编辑，源码模式需要显式切换
- 中文输入法、斜杠指令、格式快捷键、剪贴板图片粘贴
- 原生待办和书签工作区
- 系统 / 浅色 / 深色主题，简体中文 / English 界面
- 外部文件变化会刷新侧栏，不会覆盖未保存缓冲区
- 文件名/全文搜索、笔记内查找替换，以及按词和按行选择
- 页签会话恢复与拖拽排序、秒级自动保存、外部冲突检测和崩溃恢复

## 安装

打 `v*` 标签后，GitHub Actions 会构建预览安装包：

- macOS：`Synapse-<version>-macos-universal.dmg`，打开后把 Synapse 拖进「应用程序」
- Windows：`Synapse-<version>-windows-x64.exe`，Inno Setup 安装包，默认按当前用户安装

从 [Releases](https://github.com/08820048/Synapse/releases) 下载。当前包未公证/未签名，系统会弹出安全提示。

已安装的版本会在启动时检查 GitHub Releases。设置 → 更新，或命令面板里的「检查更新」可以手动再查。发现新版本后会打开对应安装包下载，不会直接替换正在运行的应用。

## 从源码运行

```bash
cargo run -p synapse
cargo run -p synapse -- /path/to/markdown-folder
```

无参数启动会恢复上次的 Vault；首次启动会创建 `~/Documents/Synapse Vault`。

本地打包：

```bash
./scripts/package-macos.sh --dmg --universal
./scripts/package-macos.sh --install
```

Windows（需安装 Inno Setup 6）：

```powershell
./scripts/package-windows.ps1
```

## 已知限制

- 表格内光标可见性
- Windows 已有安装包，但编辑体验仍以 macOS 为准
- 尚未构建 Linux 安装包
- 发布级启动速度、内存和大文件指标尚未认证

更细的产品说明见 [docs/Synapse产品需求文档.md](docs/Synapse产品需求文档.md)。变更记录见 [CHANGELOG.md](CHANGELOG.md)。
