# ADR 0001: Adopt a modular-monolith UI architecture

- Status: Accepted
- Date: 2026-08-19

## Context

Synapse ships as one native desktop application. Its core boundary is sound:
`synapse-core` owns filesystem-backed notes and `synapse-ui` owns GPUI state and
rendering. However, `synapse-ui/src/main.rs` had grown into a single module
covering application startup, state coordination, editor behavior, workspace
views, and platform integration. This obscures ownership and makes unrelated
changes conflict.

The product has a single release unit, a small local-first domain, and no need
for independently deployed services. Adding Cargo crates for every UI feature
would create public APIs and compile-time overhead without improving the user
experience.

## Decision

Synapse remains a modular monolith.

- `synapse-core` stays GPUI-free and owns vault paths, discovery, mutations,
  `NoteDocument`, and atomic persistence.
- `synapse-ui` remains one GPUI application package. Its binary entry point is
  limited to module assembly and `app::run()`.
- Application coordination lives under `src/app/`. Features are progressively
  grouped under `shell/`, `editor/`, `workspaces/`, `platform/`, and `ui/`.
- Dependencies flow from application coordination to feature modules and then
  to `synapse-core`. Feature modules must not mutate vault files directly.
- New extractions should pass explicit render state, commands, or callbacks
  instead of importing the root application type. The compatibility imports in
  `main.rs` are temporary until each feature has an explicit interface.

The current package layout is:

```text
src/
├── main.rs                 # binary entry point only
└── app/
    ├── mod.rs              # state, startup composition, shared helpers
    ├── commands.rs         # SynapseApp actions and state transitions
    ├── editor/             # Markdown editing, selection, rendering primitives
    ├── platform/           # HTTP client and release updater
    ├── shell/              # GPUI workspace and editor-row rendering
    ├── ui/                 # icons and settings presentation primitives
    └── workspaces/         # Todo and Bookmark workspaces
```

## Consequences

The initial slices move the application coordinator from `main.rs` to
`app/mod.rs`, then extract commands and GPUI shell rendering into dedicated
modules without changing behavior. This creates a stable entry-point and
dependency boundary while reducing `app/mod.rs` incrementally rather than by a
risky rewrite.

Follow-up slices, in order:

1. Extract shared UI configuration (theme, language, settings, notifications).
2. Extract shell navigation (vault tree, tabs, menus).
3. Split editor parsing, input, and GPUI rendering.
4. Split Todo and Bookmark into model, persistence, and view modules.
5. Split `Vault` internals only when a change needs an internal boundary.

## Alternatives considered

- **Keep one large `main.rs`:** rejected because ownership and merge safety
  continue to worsen.
- **Separate Cargo crate per feature:** rejected for now; all features share a
  process, GPUI lifecycle, and release cadence.
- **Rewrite into a new architecture:** rejected because it would risk data-loss
  behavior and editor regressions.
