# Feature: Claude Code Session Grove

## Summary

A macOS desktop viewer that discovers Claude Code sessions launched in other
terminals, presents them as leaves in a tree, and lets a developer group and
monitor them without consuming an API key or changing the sessions.

## Scope and constraints

- Initial platform: macOS.
- Initial coding agent: Claude Code authenticated through the user's existing
  subscription/CLI installation.
- The MVP observes external sessions; launching or controlling Claude Code is
  out of scope.
- Claude Code data is read-only. Grove has its own local grouping state.
- Session presence is inferred from log activity because no stable local
  presence API is assumed.
- The view, state, scanner, and persistence layers use Rust and GPUI.
- The design must leave room for additional agent adapters later.

## User Stories

### US-1: Discover external Claude Code sessions

As a developer, I want Grove to find sessions launched in other terminals so
that I can see my current agent work in one place.

**Acceptance Criteria:**

- [x] Given a standard `~/.claude/projects` tree, when Grove opens, then every
  parseable main session appears once.
- [x] Given Claude Code writes a new event, when the next five-second scan runs,
  then the corresponding leaf updates without restarting Grove.
- [x] Given agent/subagent JSONL files or malformed lines, when scanning, then
  Grove attaches subagents to their parent session rather than listing them as
  top-level sessions, and continues past malformed data.
- [x] Given no Claude session directory, when Grove opens, then it shows an empty
  state rather than crashing.

**Tasks:**

1. Parse main Claude Code JSONL session logs (M) — native.
2. Attach subagent metadata and nested parent relationships (M) — native.
3. Poll and reconcile the GPUI session view model (S) — desktop.
4. Cover parsing, Unicode, and status classification (S) — tests.

### US-2: Understand session progress

As a developer, I want a quick progress signal and recent activity so that I
know which terminal needs attention.

**Acceptance Criteria:**

- [x] Given a session with a non-finished event in the last 90 seconds, then it
  is shown as `Working`.
- [x] Given a session whose Claude turn ended in the last 15 minutes, then it is
  shown as `Waiting`.
- [x] Given an older session, then it is shown as `Idle`.
- [x] Given a selected session, then its title, project, branch, timestamps,
  last tool, recent prompt/response/tool activity, and session ID are visible.
- [x] Long session titles wrap to at most two lines instead of being clipped.
- [x] Inspector notices wrap within the fixed-width panel instead of overflowing.
- [x] Status language identifies the signal as inferred rather than guaranteed.

**Edge cases:**

- Future timestamps are treated as recent rather than causing a duration error.
- Large prompts/responses are whitespace-normalized and truncated.
- Claude Code slash-command markup such as `<command-name>` is rendered as
  readable command text instead of exposing the internal tags.
- Claude-injected caveats, local-command output, IDE context, and system
  reminders are not treated as user prompts or session titles.
- Tool commands are summarized; raw shell commands are not surfaced.

### US-3: Organize sessions as a tree

As a developer, I want to place session leaves on named branches so that
parallel work remains legible.

**Acceptance Criteria:**

- [x] A Grove root, named group branches, and session leaves are visually
  distinct.
- [x] A group can be created and deleted.
- [x] A session can be moved with drag-and-drop or an inspector select.
- [x] A session can be returned to `Ungrouped`.
- [x] Group membership persists between app launches.
- [x] Search and status filters narrow visible leaves without deleting group
  membership.

### US-4: Return to the original terminal workflow

As a developer, I want the session ID and resume command so that the viewer does
not trap me in a separate workflow.

**Acceptance Criteria:**

- [x] The selected session ID can be copied.
- [x] The exact `claude --resume <session-id>` command is shown.
- [x] Grove does not spawn Claude Code in the MVP.
- [x] Clicking the Messages count lazily loads every readable user/assistant
  message from the selected JSONL transcript into a scrollable timeline.
- [ ] A later interactive mode may resume Claude Code through a PTY-backed
  composer while preserving permission prompts and preventing concurrent writers.

### US-5: Explore sessions as a graphical mind map

As a developer, I want to see Grove, Claude Code sessions, and spawned
subagents as connected nodes so that parallel and delegated work is legible at
a glance.

**Acceptance Criteria:**

- [x] The existing detail tree and the graphical map can be switched without
  restarting Grove.
- [x] The map places Grove at its center and distributes Claude Code sessions
  on both sides.
- [x] Each session node shows status, project, recency, and subagent count.
- [x] Given a session with up to 12 subagents, each subagent appears as a
  connected child node with its type, description, status, and last tool.
- [x] Given a session with more than 12 subagents, its fan is replaced by one
  compact cluster node showing type/status totals.
- [x] A compact cluster's toggle expands a dynamically sized group directly
  below its summary, without adding or replacing a separate map node.
- [x] The expanded group has no internal scrolling: every mini leaf is visible
  together in explicit rows using at least two columns, increasing to three or
  four columns according to the agent count.
- [x] An explicit Collapse action removes only the inline group; the cluster
  node remains visible with total count, type totals, and status totals.
- [x] Clicking a mini leaf opens its subagent detail with status, depth,
  description, messages, last tool, updated time, and parent.
- [x] Given a subagent that starts another subagent, tool-use metadata is used
  to connect the nested child to the correct parent agent.
- [x] Large maps scroll in both directions and do not remove the text-oriented
  detail view.
- [x] Canvas padding beyond every outer node is at least the distance from
  Grove to that side's farthest node, allowing edge nodes to move to the
  viewport center instead of becoming pinned to the canvas boundary.
- [x] The map can be zoomed from 50% to 160% in 10% steps, displays the current
  zoom level, and can be reset to 100%.
- [x] Zooming keeps the same logical map position near the viewport center, and
  the Center action centers the map at the selected zoom level.
- [x] Holding Command while scrolling a trackpad zooms around the pointer,
  while unmodified scrolling keeps its normal pan behavior.
- [x] Holding Space while dragging anywhere, including over a node, pans the
  canvas in both directions and remains within the content boundary.
- [x] Dragging a node without Space moves that node, keeps its connecting edges
  attached, does not open the detail panel as an accidental click, and persists
  its position locally across refreshes and relaunches.
- [x] The map explains that each edge represents a parent spawning a child:
  Grove to session, session to direct subagent, or subagent to nested subagent.
- [x] Clicking a session or subagent opens its visible detail panel; toggling a
  compact cluster opens its inline group, whose individual agents open detail.
- [x] Scrolling the map detail panel is consumed by that panel and does not
  move the canvas underneath, including when the panel reaches either end.
- [x] Clicking empty map canvas closes both session/subagent detail and the
  Messages drawer; clicking a different node closes Messages and immediately
  replaces the detail with that node's session or subagent.
- [x] Escape closes the topmost Map overlay: Messages first, then the underlying
  session/subagent detail on a second press.
- [x] Finishing a node drag or Space-drag pan does not trigger the empty-canvas
  dismissal as an accidental click.
- [x] The map header switches between All, Working, Waiting, and Idle; the
  layout, session count, and subagent count reflect only matching sessions.
- [x] If a status filter hides the inspected session, its detail panel closes
  and the first matching session becomes the current selection.

## Non-functional requirements

- Read-only access to Claude data; no credentials or Anthropic API calls.
- Filesystem scanning occurs off the UI thread.
- The 18 MB / 14-session reference data set should scan comfortably within the
  five-second refresh interval.
- macOS minimum version is 13.0.

## Technical breakdown

| Task | Size | Layer | Depends on | Definition of done |
|---|---:|---|---|---|
| GPUI application shell | M | Desktop | — | macOS GPUI window compiles and launches |
| Claude JSONL adapter | M | Native | — | Unit tests and real-data scan succeed |
| Tree/session UI | L | GPUI view | View model | Responsive at 880×620 and 1280×820 |
| Mind-map UI | L | GPUI view | Subagent hierarchy | Root, sessions, agents, and curved connections pan, scroll, and zoom as one canvas |
| Group persistence | S | Rust state | Tree UI | Create/move/delete survive relaunch |
| Refresh and error states | S | GPUI task/view | Adapter | Empty, loading, live, and error states render |
| macOS bundle and icon | M | Distribution | All above | ad-hoc-signed `.app` builds and validates locally |

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Claude JSONL schema is internal and can change | High | Tolerant value-based parser, malformed-line skipping, adapter boundary, fixtures from observed shapes |
| Log freshness is not process presence | Medium | Call status “inferred,” use conservative time windows, plan a process/PTY adapter later |
| Full rescans become expensive with years of history | Medium | Current volume is small; add metadata cache and incremental tailing before broader rollout |
| Prompt previews contain local project information | Medium | Keep all processing local, no network calls, truncate previews, add privacy controls later |
| Text becomes difficult to read at extreme map scales | Low | Bound zoom to 50–160%, show the current percentage, and provide a one-click 100% reset |
| Large sessions create unreadable edge fans | Medium | Collapse more than 12 children into one aggregate node; expand a multi-column inline group beneath that same node on demand |
| macOS signing/notarization is required for distribution | Medium | Keep local unsigned build for development; add signing in the release milestone |

## Next milestones

1. Incremental file watching and cached offsets.
2. Pinned/archived branches and notification preferences.
3. Reliable “needs permission/input” detection.
4. Optional terminal deep links and Claude Code launch.
5. Adapter interface for Codex and other subscription-backed local agents.
6. Signed/notarized macOS release.

## Open Questions

- [ ] Should groups be global, project-scoped, or both?
- [ ] Should Grove display full transcripts, or remain an activity-only surface?
- [ ] Which terminal applications should receive deep-link support first?
- [ ] How long should finished sessions remain visible by default?
