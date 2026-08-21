# ACP Agent V1

**Status:** Approved
**Owner:** Synapse
**Last updated:** 2026-08-20
**Related ADR:** `docs/adr/0002-pi-acp-agent-runtime.md`

## Context

Synapse is a native, local-first Markdown editor whose open documents may contain
unsaved Rope buffers that differ from the files on disk. The application needs an
Agent experience that can understand the active note and selection, stream a
conversation, use local tools, and preserve conversations per Vault.

V1 uses `pi-acp` as the single Agent runtime. `pi-acp` speaks ACP JSON-RPC over
stdio and starts Pi in RPC mode. Pi reads, writes, executes Shell commands, and
accesses the network with the permissions of the current operating-system user.
Synapse is an ACP client and observability surface; it is not an operating-system
sandbox and does not limit Pi to the Vault.

Because Pi mutates files directly, Synapse must protect unsaved editor state and
refresh externally changed documents without silently overwriting local edits.

## Functional Requirements

- FR-1: Synapse MUST start `npx -y pi-acp@0.0.33` as a child process with the
  active Vault root as the ACP session working directory.
- FR-2: Synapse MUST set `PI_ACP_ENABLE_EMBEDDED_CONTEXT=true` and initialize
  the child with ACP protocol version V1.
- FR-3: Synapse MUST detect missing or unsupported Node.js, `npx`, `pi`, or
  provider configuration and MUST show an actionable setup error instead of
  failing silently.
- FR-4: The Agent panel MUST open on the right side of the main window and MUST
  support creating, switching, renaming, and deleting multiple sessions for the
  active Vault.
- FR-5: A prompt MUST include the user's text and SHOULD include the active note
  as an ACP embedded text resource. When the prompt originates from a selection,
  the selected Markdown and its relative note path MUST be included.
- FR-6: The user MUST be able to remove automatically attached context and MUST
  be able to attach additional open or Vault notes before sending.
- FR-7: Synapse MUST stream assistant message chunks and tool call/update events
  into the active conversation in protocol order.
- FR-8: Tool rows MUST show the tool title, status, referenced path and line when
  supplied, and structured edit diff when supplied by `pi-acp`.
- FR-9: The user MUST be able to stop the active turn. Synapse MUST send ACP
  cancellation and SHOULD terminate the child only if it does not stop cleanly.
- FR-10: Before each prompt, Synapse MUST save every dirty open note. If any note
  cannot be saved because of an external conflict or persistence failure, the
  prompt MUST NOT be sent and the existing editor buffer MUST be preserved.
- FR-11: After a turn finishes, fails, or is cancelled, Synapse MUST refresh the
  Vault tree and all externally changed clean open notes.
- FR-12: Synapse MUST NOT replace a dirty open buffer with disk content during
  Agent refresh. It MUST report the conflict and leave both the buffer and disk
  file recoverable.
- FR-13: A structured file edit emitted by `pi-acp` MUST be visible in the Agent
  transcript. When Pi changes an open clean note, Synapse MUST reconcile that
  external replacement as one native history entry so Undo/Redo can restore or
  reapply the Agent change without corrupting the saved snapshot.
- FR-14: Session IDs and display metadata MUST be persisted per canonical Vault
  path. On reopening a Vault, Synapse MUST list saved sessions and MUST use ACP
  `session/load` when the user resumes one.
- FR-15: The selection context menu's existing “Ask AI” action MUST open the
  right panel, populate the selected text as context, and focus the prompt input.
- FR-16: The command palette MUST expose actions to toggle the Agent panel, create
  a new Agent session, and focus the Agent prompt.
- FR-17: The Agent prompt MUST support IME composition, multiline text, Enter to
  send, Shift+Enter for a newline, Escape to close transient menus, and the
  platform-standard shortcut to toggle the panel.
- FR-18: The Agent panel MUST use the current Synapse light/dark theme tokens and
  MUST prevent pointer and keyboard events from reaching the editor behind it.
- FR-19: Synapse MUST surface ACP permission requests when the Agent emits them.
  Synapse MUST NOT invent an additional Vault-only permission layer.
- FR-20: Pi's native local filesystem, Shell, network, skills, prompts, and
  configured extensions MUST remain available to the Agent session.
- FR-21: The Agent panel MUST clearly disclose that Pi commands run with the
  current user's permissions before the first prompt in a Vault.
- FR-22: Closing a Vault or the application MUST stop its active ACP child process
  without leaving an orphan process.
- FR-23: Synapse MUST expose live Todo and Bookmark read/create/update/delete
  operations to Pi through a bundled Pi extension and a localhost bridge owned by
  the running application; successful mutations MUST update and persist the native
  workspace immediately.
- FR-24: The workspace bridge MUST bind only to localhost, MUST require a
  per-process random bearer token passed to the Pi child, and MUST reject malformed
  tool input without mutating workspace state.

## Non-Functional Requirements

- NFR-P1: Opening the Agent panel MUST NOT block the GPUI render thread while
  discovering prerequisites, starting `pi-acp`, or waiting for ACP messages.
- NFR-P2: Assistant and tool updates SHOULD appear within 100 ms of receipt.
- NFR-R1: A malformed ACP message, broken pipe, or child exit MUST affect only the
  Agent panel; note editing and saving MUST remain usable.
- NFR-R2: Session metadata MUST be written atomically and a corrupt metadata file
  MUST be ignored with a visible recovery message.
- NFR-D1: Synapse MUST never log prompt content, note content, provider secrets,
  or the child process environment by default.
- NFR-S1: Executable lookup MUST use explicit argument arrays and MUST NOT build a
  command through a system Shell.
- NFR-S2: The disclosure in FR-21 MUST state that Pi can read/write local files,
  run commands, and access the network with current-user permissions.
- NFR-A1: All panel controls MUST have visible focus states, keyboard activation,
  accessible labels/tooltips, and at least the repository's minimum hit area.
- NFR-C1: V1 MUST use one concrete `PiAcpRuntime`; it MUST NOT add a runtime
  registry, factory, plugin framework, or one-implementation trait.
- NFR-C2: macOS and Windows packages MUST compile with the Agent integration.

## Acceptance Criteria

### AC-1: Start and initialize Pi (FR-1, FR-2, FR-3)

**Given** Node.js 22+, `npx`, Pi 0.80.4+, and provider configuration are available
**When** the user creates an Agent session in an open Vault
**Then** Synapse starts pinned `pi-acp`, initializes ACP V1, creates a session
using that Vault root, and shows a ready prompt without blocking the editor.

### AC-2: Actionable prerequisite failure (FR-3)

**Given** one required executable is unavailable
**When** the user opens the Agent panel
**Then** the panel names the missing prerequisite and shows the corresponding
installation or configuration command; the main application remains responsive.

### AC-3: Current note and selection context (FR-5, FR-6, FR-15)

**Given** a note is open with a non-empty selection
**When** the user chooses “Ask AI” and sends a prompt
**Then** the prompt contains an embedded resource with the selected Markdown,
the relative note path, and no unrelated Vault contents.

### AC-4: Stream and stop (FR-7, FR-8, FR-9)

**Given** an Agent turn is producing message and tool updates
**When** updates arrive and the user presses Stop
**Then** each received update appears once and in order, cancellation is sent,
and the composer returns to an editable state.

### AC-5: Preserve unsaved work (FR-10, FR-12)

**Given** one open note is dirty and its disk copy changed externally
**When** the user attempts to send a prompt
**Then** Synapse does not send the prompt, does not overwrite either copy, and
shows the conflicting relative path.

### AC-6: Refresh direct Pi edits (FR-11, FR-13)

**Given** Pi edits a clean open note and emits a structured diff
**When** the turn completes
**Then** the transcript shows the diff, the editor displays the new disk content,
Undo restores the prior note, Redo reapplies the Agent edit, and the Vault tree
reflects files created, moved, or deleted by the turn.

### AC-7: Restore per-Vault sessions (FR-4, FR-14)

**Given** Vault A and Vault B have different saved Agent sessions
**When** the application restarts and Vault A is opened
**Then** only Vault A's sessions are listed and selecting one loads its Pi history.

### AC-8: Full Pi capability (FR-19, FR-20, FR-21)

**Given** the user has acknowledged the first-use disclosure
**When** Pi uses a local file outside the Vault, runs a Shell command, or accesses
the network
**Then** Synapse does not reject the operation solely because of its path or tool
type and displays the tool progress supplied through ACP.

### AC-9: Theme, focus, and keyboard containment (FR-16, FR-17, FR-18)

**Given** either light or dark mode is active
**When** the Agent panel and composer have focus
**Then** controls remain visible, typing affects only the composer, keyboard
navigation works, and clicks do not activate editor content behind the panel.

### AC-10: Lifecycle and build health (FR-22, NFR-R1, NFR-C2)

**Given** an ACP child is running
**When** the Vault or application closes, or the child exits unexpectedly
**Then** no orphan remains, editing stays usable, and formatting, Clippy,
workspace tests, macOS packaging checks, and Windows compilation checks pass.

### AC-11: Native workspace tools (FR-23, FR-24)

**Given** Todo and Bookmark workspaces contain existing records
**When** Pi invokes a bundled Synapse workspace tool with a valid bridge token
**Then** the operation reads or mutates the live workspace, persists successful
changes, updates the visible UI, and rejects invalid or unauthenticated requests.

## Edge Cases and Error Scenarios

- EC-1: No Vault is open: the panel MAY open, but session creation and sending
  MUST remain disabled with “Open a Vault first.”
- EC-2: `npx` downloads `pi-acp` slowly or offline: startup MUST remain
  cancellable and MUST expose a child stderr summary without freezing GPUI.
- EC-3: Provider authentication is missing: advertised ACP terminal
  authentication MUST be surfaced; Synapse MUST NOT collect provider secrets.
- EC-4: The child emits invalid JSON on stdout: the turn MUST fail visibly and
  the process MUST be restartable; raw note or prompt content MUST NOT be logged.
- EC-5: The child writes excessive stderr: stderr MUST be drained continuously
  with a bounded in-memory diagnostic tail.
- EC-6: The active Vault is moved or deleted: the current turn MUST be cancelled
  and session creation MUST remain disabled until a valid Vault is opened.
- EC-7: Pi changes a file that is dirty in Synapse after the preflight save: the
  external refresh MUST preserve the editor buffer and report a conflict.
- EC-8: A loaded ACP session no longer exists in Pi's mapping: Synapse MUST mark
  the local metadata stale and offer a new session without deleting other data.
- EC-9: Session metadata contains an unknown field: Synapse MUST ignore it for
  forward compatibility.
- EC-10: A tool update omits title, path, line, or diff: the row MUST render the
  available fields and MUST NOT panic.
- EC-11: A permission request has no selectable option: Synapse MUST allow cancel
  and MUST NOT auto-select an invalid option.
- EC-12: Multiple prompts are submitted quickly: only one turn per ACP session
  MUST run at a time; later text MUST stay in the composer.
- EC-13: The native workspace bridge or Pi extension is unavailable: the affected
  tool MUST fail visibly without terminating the conversation or changing native
  Todo/Bookmark data.

## API Contracts

No HTTP endpoint such as `POST /agent/prompt` exists in V1; communication is
local ACP over child-process stdio.

### Process contract

```text
executable: npx
arguments:  -y pi-acp@0.0.33
environment: PI_ACP_ENABLE_EMBEDDED_CONTEXT=true
             PI_ACP_PI_COMMAND=<Synapse-generated Pi launcher>
             SYNAPSE_AGENT_BRIDGE_URL=http://127.0.0.1:<random-port>
             SYNAPSE_AGENT_BRIDGE_TOKEN=<per-process random token>
cwd:         <canonical active Vault root>
transport:   newline-delimited ACP JSON-RPC 2.0 on stdin/stdout
diagnostics: stderr, drained separately and never parsed as ACP
```

Synapse MUST launch the executable directly with an argument array. Exit before
ACP initialization, EOF, invalid JSON, and non-zero exit are process errors.

### ACP client contract

V1 uses these stable ACP methods:

| Direction | Method | Required behavior |
|---|---|---|
| Client → Agent | `initialize` | Send protocol V1 and Synapse client info |
| Client → Agent | `session/new` | Send canonical Vault root as `cwd` |
| Client → Agent | `session/load` | Resume a saved Pi ACP session ID |
| Client → Agent | `session/prompt` | Send text plus selected embedded resources |
| Client → Agent | `session/cancel` | Stop the active turn |
| Agent → Client | `session/update` | Append message/tool/config updates in order |
| Agent → Client | `session/request_permission` | Return selected or cancelled |

Successful prompts end with an ACP stop reason. Protocol errors become a failed
turn containing a user-readable summary and a retry action.

### Native workspace tool contract

The bundled Pi extension calls a random localhost port advertised through the Pi
child environment. Requests carry a per-process bearer token and a JSON body. V1
exposes `todo.list`, `todo.create`, `todo.update`, `todo.delete`,
`bookmark.list`, `bookmark.create`, `bookmark.update`, and
`bookmark.delete`. A success response contains the current native record;
invalid input returns a typed error and no partial mutation.

### Local persistence contract

Session metadata is stored in the existing Synapse application-data directory
under `agent-sessions.json`. Writes MUST use the repository's atomic persistence
pattern. Pi message history remains owned by Pi/`pi-acp`; Synapse stores only the
ACP session ID and presentation metadata needed to load it.

## Data Models

### `AgentSessionMetadata`

| Field | Type | Constraint |
|---|---|---|
| `id` | string | Local stable identifier; non-empty |
| `acp_session_id` | string | Opaque value returned by `pi-acp` |
| `vault_path` | path string | Canonical absolute Vault root |
| `title` | string | Non-empty user or generated label |
| `created_at_ms` | integer | Unix epoch milliseconds |
| `updated_at_ms` | integer | Monotonic per record |

### `AgentPanelState`

| Field | Type | Constraint |
|---|---|---|
| `open` | boolean | Right panel visibility |
| `sessions` | list of metadata | Filtered to active Vault in UI |
| `active_session` | optional local ID | Must reference the filtered list |
| `turn_state` | idle/starting/running/stopping/failed | One turn maximum |
| `messages` | ordered transcript items | Current loaded session only |
| `attachments` | list of prompt contexts | Explicit, removable before send |
| `diagnostic` | optional bounded text | No note/prompt/environment dumps |

### `AgentTranscriptItem`

An item is one of `user_message`, `assistant_message`, `tool_call`,
`permission_request`, or `error`. Tool items retain the ACP tool call ID so
later updates replace the same row rather than creating duplicates.

## Out of Scope

- OS-1: Shipping or auto-updating a bundled Node.js/Pi runtime; V1 detects and guides
  installation of the documented prerequisites.
- OS-2: Supporting another ACP Agent, a registry UI, runtime plugins, or a generic
  runtime abstraction.
- OS-3: Restricting Pi to the Vault, maintaining an allowlist of external directories,
  disabling Shell/network tools, or claiming sandbox isolation.
- OS-4: Vector search/RAG indexing, background or scheduled Agents, multi-Agent
  orchestration, browser automation UI, and cloud session sync.
- OS-5: Per-keystroke replay of Agent edits; each external Agent replacement is a
  single native Undo/Redo history entry.
- OS-6: Windows code signing, macOS notarization, provider billing, and provider account
  management.
