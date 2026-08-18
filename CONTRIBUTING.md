# Contributing to Synapse

Thank you for wanting to help. Synapse is a native Rust + GPUI Markdown editor. Please read [AGENT.md](AGENT.md) for product constraints before writing code.

[中文说明](#中文)

## Before you start

- macOS is the development platform that is actually exercised.
- Rust 1.93+ is required (`rust-toolchain.toml` pins the channel).
- Do not introduce HTML, CSS, JavaScript, Electron, egui, or iced.
- Do not add dependencies unless the change truly needs them.
- User-visible strings need both Simplified Chinese and English.

Good first areas: editor gaps (undo, table caret, word selection), docs, tests, and Linux build notes. Large new product surfaces should be discussed in an issue first.

## Setup

```bash
git clone https://github.com/08820048/Synapse.git
cd Synapse
cargo run -p synapse
```

## Checks

Run these before opening a pull request:

```bash
cargo fmt --package synapse-core -- --check
cargo fmt --package synapse -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do **not** run `cargo fmt --all`. `vendor/gpui-component` must stay in upstream form.

CI runs `fmt` on Ubuntu and `clippy` / tests on macOS and Windows. Installers are not built on every pull request. Push a `v*` tag (or run the **Release** workflow) to produce the macOS DMG and Windows setup EXE.

## Architecture

| Crate | Responsibility |
|---|---|
| `synapse-core` | Vault, path safety, snapshots, `NoteDocument`, atomic save, trash |
| `synapse` (`crates/synapse-ui`) | Window, navigation, editor rendering, todos, bookmarks |

Keep filesystem-backed note logic in `synapse-core`. Keep GPUI views and session chrome in `synapse-ui`. The Markdown file on disk is the source of truth; the open Rope buffer is the source of truth for unsaved content.

Never silently discard dirty buffers. File operations that would touch an unsaved tab must fail clearly.

## Pull requests

1. Fork and branch from `main`.
2. Keep the change scoped. One problem per PR.
3. Add or update tests when behavior changes.
4. Update `CHANGELOG.md` under `Unreleased` if the change is user-visible.
5. Fill in the pull request template, including how you verified the change.

Please do not commit `target/`, editor swap files, or reformatted vendor sources.

## Security

Do not file public issues for vulnerabilities. See [SECURITY.md](SECURITY.md).

---

## 中文

先读 [AGENT.md](AGENT.md)。Synapse 是本地优先的原生 Markdown 编辑器，性能和架构清晰度优先于功能堆叠。

开发环境以 macOS 为准，Rust 1.93+。不要引入 Web 技术栈，不要随手加依赖。所有用户可见文案都要同时有中文和英文。

提交前只格式化两个 Synapse 包，不要 `cargo fmt --all`。未保存缓冲区不能被静默丢弃。用户可见改动请写进 `CHANGELOG.md`。
