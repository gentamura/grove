# Grove

Grove is a native macOS viewer for Claude Code sessions started from any
terminal. The interface and application runtime are written entirely in Rust
with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the
GPU-accelerated UI framework created for Zed.

## Current vertical slice

- Discovers main Claude Code sessions under `~/.claude/projects`
- Shows project, branch, title, recent activity, and session ID
- Offers both a text-oriented tree and a pannable, zoomable graphical mind map centered on Grove
- Detects Claude Code subagents and connects nested agent relationships
- Compacts large subagent fans into cluster nodes whose toggle reveals a
  grouped, multi-column set of clickable mini leaves directly below the summary
- Opens session/subagent details from map nodes, supports Command-scroll zoom,
  Space-drag canvas panning, and persistent node positioning
- Filters both tree and map sessions by All, Working, Waiting, or Idle status
- Infers `Working`, `Waiting`, or `Idle` from recent log events
- Refreshes automatically every five seconds
- Creates groups and moves sessions by drag-and-drop or the inspector
- Persists grouping locally without changing Claude Code data
- Renders Claude Code slash-command metadata as readable text such as
  `/review #17` and excludes internal reminder/caveat messages from titles
- Provides the exact `claude --resume <session-id>` command
- Supports macOS text input and IME composition in search/group fields

Grove does not launch or control Claude Code yet. Status is deliberately
heuristic: Grove observes local session history written by Claude Code rather
than relying on an account API or process it owns.

## Develop on macOS

Prerequisites:

- Rust 1.95 or newer
- Xcode and Xcode Command Line Tools
- The Xcode Metal Toolchain

Install the Metal Toolchain once if it is missing:

```bash
xcodebuild -downloadComponent MetalToolchain
```

Run the application:

```bash
cargo run
```

## Verify

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
```

The scanner has an ignored smoke test that reads your installed Claude Code
session directory:

```bash
cargo test scans_installed_claude_sessions -- --ignored --nocapture
```

## Build the macOS app

```bash
./scripts/build-macos.sh
```

The ad-hoc-signed development bundle is written to
`target/release/bundle/macos/Grove.app`.

## Privacy and data

Grove reads local session logs under `~/.claude/projects` and never modifies
them. It does not read account credentials or call the Anthropic API. Group
preferences are stored separately at
`~/Library/Application Support/Grove/preferences.json`.

See the [MVP requirements](docs/requirements.md) and
[GPUI architecture decision](docs/adr/0002-use-gpui-for-an-all-rust-desktop-app.md).
