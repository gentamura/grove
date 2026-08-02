# Claude Code Instructions

Read and follow `AGENTS.md` before changing this repository. It is the canonical
repository-wide guide; this file adds Claude Code-specific reminders.

## Project context

Grove observes the local files written by Claude Code and Codex and visualizes
sessions and supported subagents in a native Rust/GPUI macOS application. Grove
does not currently launch coding agents, send prompts, or own terminal
processes.

## Non-negotiable rules

- Never modify anything under `~/.claude`.
- Never modify anything under `~/.codex`.
- Never use real user transcripts as committed fixtures.
- Treat Claude Code JSONL as an observed, evolving format. Parse defensively and
  retain partial-scan warnings.
- Keep conversation history loading lazy.
- Do not introduce Anthropic API calls, credential access, telemetry, or a
  network dependency without an explicit architecture decision.
- Do not replace GPUI with Tauri, React, Electron, or a webview. ADR-0002 is the
  accepted view-stack decision.
- Do not commit `target/`, local preferences, logs, or generated app bundles.
- Do not apply the MIT License to the Grove name or tree icon. Preserve
  `TRADEMARKS.md`, `assets/README.md`, and the icon's copyright metadata.

## Before editing

Review the relevant files:

- Product behavior: `docs/requirements.md`
- Architecture: `docs/adr/README.md`
- Licensing and releases: `docs/licensing.md`
- JSONL parsing: `src/scanner.rs`
- GPUI behavior: `src/app.rs`
- Persistence: `src/preferences.rs`
- IME and input behavior: `src/text_input.rs`

Search for an existing helper or test before adding another abstraction.

## Before finishing

Run:

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
```

If the change affects the macOS bundle, app startup, window lifecycle, or
release-only behavior, also run:

```bash
./scripts/build-macos.sh
/usr/bin/codesign --verify --deep --strict \
  target/release/bundle/macos/Grove.app
```

Report exactly which checks ran and whether the ignored real-data smoke test was
intentionally skipped.
