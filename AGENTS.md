# Agent Guide

This file defines the repository-wide working agreement for coding agents.
Human contributors should find the same commands and constraints useful.

## Mission

Grove is a native macOS, read-only viewer for Claude Code sessions stored under
`~/.claude/projects`. It presents main sessions and nested subagents in a tree
and a graphical map without launching Claude Code or modifying its data.

Preserve these product boundaries unless a task explicitly changes them:

- Claude Code data is read-only.
- Grove remains local-first and does not require an Anthropic API key.
- Session state is inferred from log events and must be labeled as inferred.
- The application and view layer are Rust and GPUI; do not introduce a webview
  frontend or revive the superseded Tauri architecture.
- macOS 13 is the current minimum supported platform.

## Start here

Before making changes, read:

1. `README.md`
2. `docs/requirements.md`
3. `docs/adr/README.md`
4. `docs/licensing.md` when changing dependencies, assets, packaging, or release
   behavior
5. The relevant accepted ADR, especially
   `docs/adr/0002-use-gpui-for-an-all-rust-desktop-app.md`

ADR-0001 is historical and superseded. Do not implement its Tauri/React design.

## Repository map

- `src/main.rs`: application startup, macOS window lifecycle, and menus.
- `src/app.rs`: Grove state and GPUI rendering/interactions.
- `src/scanner.rs`: Claude Code JSONL discovery, parsing, status inference, and
  transcript loading.
- `src/models.rs`: UI-independent session, subagent, activity, and message
  types.
- `src/preferences.rs`: Grove-owned groups and map offsets, saved atomically.
- `src/text_input.rs`: GPUI text input, selection, clipboard, and IME behavior.
- `docs/requirements.md`: current behavior and acceptance criteria.
- `docs/licensing.md`: source publication and binary distribution obligations.
- `docs/adr/`: architectural decisions and their status.
- `TRADEMARKS.md`: permitted use of the Grove name and icon.
- `assets/README.md`: copyright scope for Grove brand assets.
- `scripts/build-macos.sh`: release build and local ad-hoc app signing.
- `packaging/macos/Info.plist`: bundle metadata and minimum macOS version.

## Required verification

Run these checks for every Rust change:

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
```

For packaging, lifecycle, or release-build changes, also run:

```bash
./scripts/build-macos.sh
/usr/bin/codesign --verify --deep --strict \
  target/release/bundle/macos/Grove.app
```

The real-data scanner smoke test is optional because it reads private local
Claude Code history:

```bash
cargo test scans_installed_claude_sessions -- --ignored --nocapture
```

Do not make ordinary tests depend on `~/.claude`, the network, or the current
user's preferences.

## Engineering constraints

### Claude Code data

- Never write, rename, delete, or normalize files under `~/.claude`.
- Treat observed JSONL shapes as unstable input, not a guaranteed API.
- Ignore unknown fields and continue past malformed records where safe.
- Preserve partial-scan warnings so users can distinguish full and partial
  results.
- Keep transcript loading lazy; do not load every complete conversation during
  the five-second session scan.
- Avoid exposing raw shell commands or Claude-injected internal metadata as
  session titles.

### State and persistence

- Grove preferences belong under
  `~/Library/Application Support/Grove/preferences.json`.
- Preference writes must remain atomic.
- Failed writes must not leave in-memory state pretending persistence
  succeeded.
- Stable element IDs must use stable model IDs, not display labels that may be
  duplicated.

### GPUI

- Keep expensive filesystem work off the UI thread.
- Stop event propagation at overlay and nested scroll boundaries when the
  underlying canvas must not react.
- Distinguish clicks from completed drags so dragging does not accidentally
  select or dismiss nodes.
- Preserve focus and IME behavior. Enter and Escape must not submit or cancel
  while marked IME text is still being composed.
- Ensure macOS window lifecycle behavior still works after the final window is
  closed and the Dock icon is reopened.

### Architecture

- Keep `scanner`, `models`, and `preferences` free of GPUI imports.
- Prefer small pure helper functions for layout, parsing, and classification so
  behavior can be unit tested.
- Use tolerant parsing at the Claude boundary and strongly typed models inside
  the application.
- Do not add network access for functionality that can be derived locally.

### Licensing and distribution

- Keep `Cargo.toml`, `LICENSE`, and the README license statement consistent.
- Do not describe the Grove name or icon as MIT-licensed. Preserve
  `TRADEMARKS.md`, `assets/README.md`, and the SVG copyright metadata.
- Modified distributions and forks must replace Grove's product name and icon;
  unmodified official builds may retain them under the brand policy.
- Repeat the dependency license review when `Cargo.lock`, enabled features,
  supported targets, or packaging contents change.
- Do not publish an official binary until the checklist in
  `docs/licensing.md#distributing-a-binary` is complete.
- Treat repository visibility changes and binary releases as separate actions:
  public source availability does not mean the current app bundle is ready for
  redistribution.

## Change workflow

1. Inspect the relevant code and requirements before editing.
2. Preserve unrelated local changes.
3. Add or update tests for parsing, persistence, layout, or state transitions.
4. Update `docs/requirements.md` when visible behavior changes.
5. Add or update an ADR when a decision changes architecture, persistence,
   external dependencies, or major UI structure.
6. Run the required verification commands.
7. Keep generated output such as `target/` out of version control.

## Documentation style

- Write repository documentation in English.
- Describe status as inferred, never authoritative.
- Distinguish current behavior from future milestones.
- Do not claim support for schemas, platforms, or integrations that are not
  covered by the code and tests.
