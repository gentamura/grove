# ADR-0001: Use Tauri 2 for the local session viewer

## Status

Superseded by ADR-0002

## Date

2026-07-30

## Context

Grove starts as a macOS viewer for Claude Code sessions launched in separate
terminals. It must read and incrementally observe local JSONL files, present a
rich tree UI, preserve a path to other desktop platforms, and avoid expanding
webview filesystem authority.

The first release is deliberately not a Claude Code launcher. It should work
with the user's installed CLI and subscription, without an Anthropic API key or
cloud service.

Quality priorities:

1. Safe local filesystem access.
2. Small and responsive desktop distribution.
3. Fast UI iteration for a tree-heavy interface.
4. A clean boundary for future coding-agent adapters.
5. Practical macOS packaging.

## Decision

We will use Tauri 2 with React/TypeScript in the webview and a Rust-native
Claude Code adapter.

The webview calls one application command that returns a compact session view
model. It receives no general filesystem plugin permission. The adapter reads
`~/.claude/projects` with Rust standard libraries and parses Claude's observed
JSONL records tolerantly.

Group metadata is application-owned and initially stored in webview local
storage. Claude data remains read-only.

## Options Considered

### Option 1: Tauri 2 + React + Rust

**Description:** Use the system webview for UI and Rust commands for privileged
local operations.

**Pros:**

- Narrow command boundary keeps filesystem access out of UI code.
- Uses the system webview instead of bundling Chromium.
- Rust is a strong fit for file tailing, parsing, and later process observation.
- React supports fast iteration on the branch/leaf interaction model.
- Tauri has documented macOS bundling and capability controls.

**Cons:**

- Requires Rust and web frontend expertise.
- System-webview differences require testing if Windows/Linux are added.
- Tauri capability configuration adds concepts beyond a plain web app.

**Effort:** Medium

### Option 2: Electron + React + Node.js

**Description:** Bundle Chromium and use Electron main/preload processes for
local access.

**Pros:**

- One JavaScript/TypeScript ecosystem.
- Mature desktop APIs and consistent Chromium rendering.
- Large community and established packaging tools.

**Cons:**

- Bundles Chromium and Node.js for a relatively small local viewer.
- Requires careful preload, sender validation, sandbox, navigation, and CSP
  discipline for privileged file APIs.
- Native high-volume file processing is less naturally isolated than a Rust
  adapter.

**Effort:** Low to medium

### Option 3: Native SwiftUI

**Description:** Build a macOS-only application in Swift/SwiftUI.

**Pros:**

- Best access to macOS windowing, notifications, and process APIs.
- Native distribution and interaction conventions.
- Small runtime footprint.

**Cons:**

- Makes Windows/Linux support a rewrite.
- Slower iteration for the existing web-style tree concept.
- A second implementation would be needed for cross-platform UI.

**Effort:** Medium to high

### Option 4: Rust-native UI (egui/iced)

**Description:** Keep the full application in Rust using a cross-platform native
or immediate-mode GUI toolkit.

**Pros:**

- Single language and direct access to local data.
- No webview frontend boundary.
- Potentially small and fast.

**Cons:**

- Rich desktop tree interactions and typography require more custom work.
- Smaller UI ecosystem and fewer designers familiar with the stack.
- Immediate-mode patterns are not an obvious fit for this information-dense app.

**Effort:** High

## Consequences

### Positive

- The MVP can be entirely local and needs no agent API credentials.
- Claude Code schema changes are contained in one Rust adapter.
- The same UI can later consume adapters for other coding agents.
- Privileged access remains explicit and narrow.

### Negative

- Contributors need both Node and Rust toolchains.
- The initial five-second full scan is intentionally simple and must be replaced
  with cached offsets/file watching as histories grow.
- Status remains a heuristic until a process-level or official presence signal
  exists.

### Neutral

- Group state is independent of Claude sessions and therefore does not follow a
  user to another Mac in the MVP.

## Implementation Notes

- Exclude `agent-*.jsonl` so subagent transcripts are not mistaken for roots.
- Parse records as flexible JSON values rather than a single strict schema.
- Run scanning through `spawn_blocking`, never on the webview/UI thread.
- Keep status windows centralized and tested.
- Keep the current view model provider-specific internally, then introduce a
  general `CodingAgentAdapter` when the second provider is added.
- Add signing, notarization, and update strategy before public distribution.

## References

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri command model](https://v2.tauri.app/develop/calling-rust/)
- [Tauri filesystem security](https://v2.tauri.app/plugin/file-system/)
- [Electron process model](https://www.electronjs.org/docs/latest/tutorial/process-model)
- [Electron security checklist](https://www.electronjs.org/docs/latest/tutorial/security)
- [Claude Code CLI session resume](https://docs.anthropic.com/en/docs/claude-code/cli-usage)
