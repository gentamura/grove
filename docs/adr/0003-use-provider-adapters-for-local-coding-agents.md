# ADR-0003: Use provider adapters for local coding agents

## Status

Accepted

## Date

2026-08-02

## Context

Grove began with a tolerant reader for Claude Code's observed JSONL files. The
product now also needs to discover local Codex sessions and let the user switch
between Claude Code, Codex, or a combined view.

The two agents use different storage layouts and record schemas:

- Claude Code stores project-scoped transcripts under `~/.claude/projects` and
  records nested subagents beside the parent transcript.
- Codex stores rollout JSONL files under `~/.codex/sessions` and
  `~/.codex/archived_sessions`, with session metadata, response items, and task
  events represented as separate record types.

Neither observed file format is treated as a stable public API. Provider
details must not spread into GPUI layout, grouping, filtering, or persistence.

## Decision

Grove will use a provider-adapter boundary over one UI-independent session
model.

Each session carries a `CodingAgent` value. Provider adapters own discovery,
JSONL parsing, lazy conversation loading, and resume-command formatting. The
application receives a merged `SessionScan` and applies provider, status, and
text filters without inspecting provider-specific records.

Grove preserves existing Claude Code preference keys. Codex preference and map
keys use a `codex:` prefix so sessions from different providers cannot collide.

Codex rollout files whose `session_meta.source` identifies an internal
subagent are not shown as top-level sessions. The observed Codex metadata does
not expose a stable parent-session ID for those rollouts, so Grove will not
invent a graphical relationship. Codex subagent visualization requires a later
adapter revision backed by a reliable relationship signal.

## Options Considered

### Option 1: Provider adapters over a shared session model

**Description:** Isolate each observed storage format and merge normalized
sessions before rendering.

**Pros:**

- Keeps GPUI and preferences independent of JSONL schemas.
- Makes provider filtering a model operation rather than a second UI.
- Contains schema changes and provider-specific resume behavior.
- Allows synthetic fixtures for every provider without private transcripts.

**Cons:**

- The shared model exposes only concepts that can be represented consistently.
- Provider-specific capabilities need explicit optional fields or detail views.
- Cross-provider stable keys require a migration-aware convention.

**Effort:** Medium

### Option 2: Add Codex conditionals to the Claude scanner and UI

**Description:** Extend existing functions with path and record-type checks.

**Pros:**

- Small initial diff.
- Reuses existing parsing helpers directly.

**Cons:**

- Couples UI behavior to provider schemas.
- Becomes difficult to test and extend as more agents are added.
- Encourages incorrect assumptions that similarly named events mean the same
  thing.

**Effort:** Low initially, high over time

### Option 3: Maintain separate application views for each provider

**Description:** Give Claude Code and Codex independent state models and tabs.

**Pros:**

- Every provider can expose all native concepts.
- Minimal normalization.

**Cons:**

- Duplicates tree, map, grouping, status, and interaction code.
- Prevents a useful combined view.
- Makes preferences and visual behavior inconsistent.

**Effort:** High

## Consequences

### Positive

- Users can switch between All, Claude Code, and Codex without changing views.
- New providers have an explicit parsing and command boundary.
- Claude Code groups and map offsets remain compatible with existing installs.
- Full conversation history remains lazy and local for both providers.

### Negative

- Status is still inferred and may not exactly match either running process.
- Codex internal subagents are excluded until a stable parent link is
  available.
- A five-second refresh currently rescans both provider trees.

### Neutral

- Grove remains a read-only observer and does not launch or control either
  coding agent.
- No new runtime dependency or network access is introduced.

## Implementation Notes

- Keep `CodingAgent`, `CodingSession`, and `SessionScan` free of GPUI imports.
- Read `~/.claude/projects`, `~/.codex/sessions`, and
  `~/.codex/archived_sessions` only.
- Route message-history loading through the selected session's provider.
- Use `claude --resume <id>` and `codex resume <id>` as display-only commands.
- Ignore unknown record types and preserve partial-scan warnings.
- Commit only synthetic JSONL fixtures in tests.
