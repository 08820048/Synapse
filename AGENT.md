# Synapse project principles

This file is the product and engineering constitution for humans and AI agents working in this repository.

Repository: https://github.com/08820048/Synapse

## Product

Synapse is a high-performance, local-first knowledge workbench written in Rust + GPUI.

Identity:

- Extremely fast startup, very low memory
- Native desktop experience (never Electron)
- Connection as a core idea (wikilinks, later)
- Clean, modern, keyboard-first UI
- Center editor + lightweight surrounding chrome
- Every user-facing string must ship in both Chinese and English

Think: Obsidian's writing power + Zed's performance + Linear's restraint.

Priority order, always:

1. Performance and fluidity
2. Clear architecture
3. Beautiful, minimal UI
4. Local-first privacy

In early stages, never add a convenience feature that spends startup time, idle memory, or input latency.

## Hard rules

- No web UI stack: no HTML/CSS/JS, no Electron, no egui, no iced, unless the maintainer explicitly asks.
- No heavy frameworks and no casual new dependencies.
- Prefer composition over inheritance.
- Keep module boundaries:
  - `synapse-core` → domain (notes, vault, filesystem)
  - `synapse-ui` → GPUI views and session state
  - later: editor, search, link
- The editor buffer is the source of truth for open content.
- The filesystem is the source of truth for notes. No required database in the current phase.
- UI state should stay easy to serialize so layouts can be saved later.

Dangerous actions and notifications have dedicated specs:

- `docs/危险操作确认效果规范.md`
- `docs/通知系统规范.md`

## UI

- Native and fast, not decorative
- Use GPUI's style system and `gpui-component`
- Prefer declarative, composable elements
- Use as few dividers as possible

## Performance (non-negotiable)

- Cold start target: < 0.8s
- Idle memory target: < 80MB
- Smooth scrolling on very large notes
- Fast search even with thousands of notes
- Do not block the main thread
- Be careful with allocations on render, input, and search paths

When unsure, pick the faster and leaner option.

## Code style

- Idiomatic modern Rust
- Use `Result` and handle errors
- Clarity over cleverness
- Names matter: Synapse, Vault, Note, Link, Panel
- Comment only when the why is not obvious
- Keep functions small
- Write code that a future human or agent can change safely

## Decision tests

1. Does this keep or improve performance?
2. Does this keep the architecture clear?
3. Does this serve a center-editor workspace?
4. Is this the simplest good MVP solution?

If a request fights performance or simplicity, say so and propose a better alternative.

Do not push to `origin` unless a maintainer explicitly asks. External contributions should go through a pull request. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Communication

- Direct and practical
- Prefer complete, runnable examples
- Call out performance or architecture risk early
- If a request fights the product tone, say so politely and suggest another path
