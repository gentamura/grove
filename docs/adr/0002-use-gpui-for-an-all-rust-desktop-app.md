# ADR-0002: Use GPUI for an all-Rust desktop app

## Status

Accepted

## Date

2026-07-30

## Context

The Tauri vertical slice proved that Claude Code's local JSONL session history
is sufficient for a read-only viewer. The product direction now explicitly
prioritizes Rust for both native services and the view layer, and only macOS is
required initially.

Grove's primary interface is an information-dense, continuously updated tree.
It does not need browser content, remote pages, or a shared web deployment. A
webview therefore adds a second language, two state systems, and an IPC boundary
without providing a product requirement.

Quality priorities:

1. One Rust process and one typed state graph.
2. Fast native rendering for dense session trees.
3. Read-only local session observation.
4. A compact macOS application bundle.
5. Clear containment of framework instability.

## Decision

We will replace Tauri, React, TypeScript, and webview local storage with GPUI
0.2.2 and a single Rust crate.

GPUI owns the application, windows, entities, input, drag-and-drop, rendering,
clipboard integration, and refresh tasks. The existing Rust session scanner and
models remain the data adapter. User-created branches are serialized to
`~/Library/Application Support/Grove/preferences.json`.

We pin the crates.io release rather than Zed's `main` branch. GPUI-facing code is
kept in the view and input modules so future breaking upgrades do not spread
into the scanner or persistence layers.

## Options Considered

### Option 1: GPUI 0.2.2

**Description:** Zed's GPU-accelerated Rust UI framework, published as a
reproducible crates.io release.

**Pros:**

- Entire application and state graph stay in Rust.
- Metal-backed rendering on macOS.
- Typed entities, actions, async tasks, drag-and-drop, and test support.
- Closely matches the desired Zed-like dense desktop interaction model.
- No Chromium, JavaScript runtime, webview, or IPC serialization layer.

**Cons:**

- Pre-1.0 and subject to breaking changes.
- Documentation is limited; examples and Zed source are often authoritative.
- Packaging, updates, and common controls are less turnkey than Tauri.
- Windows support is not a near-term assumption.

**Effort:** Medium

### Option 2: Continue Tauri 2 + React

**Description:** Keep the completed webview UI and Rust command adapter.

**Pros:**

- Existing slice is already polished and verified.
- Stable cross-platform packaging and capability system.
- Mature accessibility and browser tooling.

**Cons:**

- Retains TypeScript, DOM state, IPC, and two build systems.
- Does not satisfy the selected all-Rust direction.
- Webview rendering is unnecessary for this local-only product.

**Effort:** Low

### Option 3: SwiftUI + Rust library

**Description:** Use native SwiftUI for macOS and expose the scanner through an
FFI boundary.

**Pros:**

- Mature macOS controls and accessibility.
- First-class signing and packaging path.

**Cons:**

- Still uses two languages and an FFI boundary.
- Cross-platform UI would require another implementation.
- Does not reuse the team's Rust UI preference.

**Effort:** High

## Consequences

### Positive

- Session parsing, preferences, status inference, and rendering share Rust
  types directly.
- The production app has no Node or web runtime.
- The architecture is aligned with the interaction model used by Zed.
- The scanner remains independently testable.

### Negative

- We accept GPUI's pre-1.0 upgrade cost.
- Text input and several common controls require more application code.
- Release packaging is maintained by a small Grove-owned script.

### Neutral

- The first release remains macOS arm64 focused.
- The Tauri ADR and implementation history remain documented but are no longer
  active architecture.

## Implementation Notes

- Pin `gpui = 0.2.2`; do not depend on a floating Git branch.
- Keep `scanner`, `models`, and `preferences` free of GPUI imports.
- Isolate GPUI-specific text input behavior in `text_input`.
- Run five-second scans on GPUI's background executor.
- Store preferences atomically outside Claude's data directory.
- Package `target/release/grove` into an ad-hoc-signed `.app`.
- Treat a future GPUI version upgrade as a deliberate ADR review.

## References

- [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)
- [Zed on GPUI and GPU rendering](https://zed.dev/blog/videogame)
- [ADR-0001](0001-use-tauri-for-local-session-viewer.md)
