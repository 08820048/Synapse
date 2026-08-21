# ADR 0002: Use pi-acp as the Agent runtime

- Status: Accepted
- Date: 2026-08-20

## Context

Synapse needs a local Agent that can work with notes while retaining broad coding
Agent abilities: local files, Shell commands, network access, Pi skills, and Pi
provider configuration. Building a model/provider client, tool loop, session
store, and streaming protocol inside Synapse would duplicate an existing Agent
runtime and bind the UI to provider-specific behavior.

ACP provides a process boundary and typed session protocol. `pi-acp` adapts Pi
to ACP over JSON-RPC stdio and already maps streaming output, tool progress,
structured edit diffs, modes, commands, authentication, and persisted sessions.
It intentionally lets Pi execute locally rather than delegating filesystem and
terminal calls back to the ACP client.

## Decision

Synapse will implement one concrete ACP client for pinned `pi-acp@0.0.33`.

- Synapse launches `npx -y pi-acp@0.0.33` directly, enables embedded context,
  and uses the active Vault root as the session working directory.
- Pi inherits current-user filesystem, Shell, network, skills, extensions, and
  provider capabilities. Synapse does not impose Vault-only isolation.
- Synapse owns the right-side conversation UI, session metadata, active
  note/selection attachments, ACP permission prompts, cancellation, tool/diff
  presentation, and child-process lifecycle.
- Pi/`pi-acp` owns inference, tool execution, detailed message history, and ACP
  session restoration.
- Before prompting, Synapse saves dirty open notes. After a turn, it reuses the
  existing Vault watcher and reconciles clean external replacements into one
  native Undo/Redo entry. Dirty buffers are never replaced by Agent disk changes.
- Todo and Bookmark operations are exposed by one bundled Pi extension connected
  to a token-authenticated localhost bridge owned by the running Synapse process.
  Synapse points `PI_ACP_PI_COMMAND` at a generated Pi launcher which explicitly
  loads that bundled extension; this is needed because `pi-acp` does not forward
  ACP MCP server parameters.
- V1 requires Node.js 22+, `npx`, Pi 0.80.4+, and an independently configured Pi
  provider. The panel diagnoses missing prerequisites.
- No runtime trait, registry, factory, sidecar permission service, or custom tool
  bridge is introduced until a second runtime or native workspace workflow
  requires it.

## Consequences

The Agent has the broad capabilities requested by the user and Synapse avoids
duplicating Pi's tool loop. The right panel can observe tool calls and diffs but
cannot truthfully claim sandbox enforcement because Pi performs operations in its
own process with the user's permissions.

Direct disk edits require a save-before-turn invariant and conflict-safe history
reconciliation. Node/Pi remain V1 prerequisites, which keeps the first
implementation and bundle small at the cost of a setup step for users without
that toolchain. The native workspace bridge adds a small authenticated local IPC
surface that must be validated like any other trust boundary.

## Alternatives considered

- **Custom constrained ACP sidecar:** rejected because Vault-only files and no
  Shell/network would intentionally remove capabilities required for this Agent.
- **Embed a provider SDK and custom tool loop:** rejected because it duplicates
  authentication, streaming, tools, sessions, and provider compatibility.
- **Generic multi-Agent ACP registry:** deferred; one implementation does not
  justify its abstractions or UI.
- **Bundle Node.js, Pi, and pi-acp immediately:** deferred until prerequisite
  setup proves to be a release blocker; it materially increases packaging and
  update responsibilities.
