# Advanced Systems Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the advanced systems on top of the M0-M2 kernel floor: repo governance, remote operator plane, multi-agent runtime, project memory, ProjectOps, branch search/debate, and research skill orchestration.

**Architecture:** All advanced capabilities remain downstream of the kernel/session/event truth. Every subsystem must project from the same bundle, use the same event vocabulary, and persist through canonical schemas. No advanced feature may backfill missing kernel rigor with ad hoc runtime state.

**Tech Stack:** Follows the same repository layout and protocol assumptions as
`31-kernel-foundation-implementation-plan.md`; the active implementation is a
Rust kernel with schema-backed persistence, plus a `Happy`/`Happier`-derived
TypeScript remote plane and Python research sidecars.

Language transition note:

- logical ownership in this document remains canonical
- exact Rust path mapping is frozen in
  `46-rust-kernel-pivot-and-bootstrap.md`

---

## 1. Scope

This plan covers:

- Milestone 3: Repo governance and review protocols
- Milestone 5: Remote operator plane
- Milestone 6: Multi-agent core
- Milestone 7-8: Working + durable memory and ProjectOps
- Milestone 9: Evolutionary branch intelligence
- Milestone 10: Research runtime and skill pack

It assumes the kernel foundation is already in place.

## 2. Entry Conditions

Do not start this plan unless all of the following are true:

- M0-M2 kernel/session/provider/permission surfaces are live
- `KernelStateBundle` persistence is stable
- base command families already obey the canonical event vocabulary
- schema harness and golden fixture runner already exist in CI

## 3. Execution Order

### Task 0: Git-native canonical surface gate

**Files:**
- Create: `src/canonicality/mod.rs`
- Create: `schemas/git_state_snapshot.schema.json`
- Create: `schemas/canonical_surface_manifest.schema.json`
- Create: `schemas/code_owner_manifest.schema.json`
- Create: `schemas/canonicality_audit_report.schema.json`
- Modify: `src/runtime/mod.rs`
- Test: `tests/operator/cli_surfaces.rs`
- Test: `tests/conformance/schemas/batch10_enforcement.rs`

- [x] Implement read-only `projects audit --canonical --json` as the first M3/M4 canonicality gate.
- [x] Require explicit `CanonicalSurfaceManifest` before release/promotion gates.
- [x] Feed blocking violations into `repo cleanup-plan --canonical`.
- [x] Block promotion while canonicality audit has blocking violations.

**Done when:**
- the project can machine-report whether exactly one active public doc/code surface is exposed
- stale or duplicate public surfaces become typed blocking violations
- git state is evidence for promotion/merge decisions, not a second runtime authority

### Task 1: Artifact families and review persistence

**Files:**
- Create: `internal/artifacts/families.go`
- Create: `internal/artifacts/promotion.go`
- Create: `internal/reviews/store.go`
- Create: `internal/reviews/trace.go`
- Create: `schemas/artifact_family.schema.json`
- Create: `schemas/artifact_list_result.schema.json`
- Create: `schemas/artifact_inspection_result.schema.json`
- Create: `schemas/review_packet.schema.json`
- Create: `schemas/review_trace.schema.json`
- Test: `tests/conformance/research/review_packet_test.go`
- Test: `tests/golden/reviews/review_open_test.go`

- [x] Implement artifact family registry and latest/archive pointers.
- [x] Implement `artifacts list/inspect` payload assembly against canonical artifact families.
- [x] Implement review packet and review trace persistence.
- [x] Implement cleanup-plan baseline dependencies for governed outputs.
- [x] Add review packet/trace conformance and open/retry golden fixtures.

**Done when:**
- every governed output can resolve into an artifact family
- reviews are traceable and schema-valid

### Task 2: Remote lease, binding, and projection substrate

**Rust implementation status (M5 substrate slice):** The current Rust kernel
implements the local M5 remote-control substrate in `src/remote/mod.rs`, the
local HTTP daemon in `src/remote/daemon.rs`, the installable phone-browser PWA
under `src/assets/mobile/`, and the `remote` CLI dispatch in
`src/runtime/mod.rs`. The old Go file paths below remain ownership aliases from
the original plan; the active implementation surface is Rust plus schema
contracts under `schemas/`. This slice graduates local adapter-grade attach,
handoff, takeover, notification, ownership epoch, capability advertisement,
projection regeneration, and LAN/browser mobile control through
`research-cli remote daemon`. It intentionally does not claim encrypted Happy
relay or native iOS/Android packaging. Revocation, reconnect, and lease-expiry
behavior are now covered in the Rust/Tailscale substrate and harnesses.

**Files:**
- Create: `internal/remotehost/transport/lease.go`
- Create: `internal/remotehost/transport/binding.go`
- Create: `internal/remotehost/transport/cursor.go`
- Create: `internal/remotehost/projection/projection.go`
- Create: `internal/remotehost/actions/actions.go`
- Create: `internal/remotehost/daemon/state.go`
- Create: `internal/remotehost/status/status.go`
- Create: `schemas/remote_lease.schema.json`
- Create: `schemas/remote_binding.schema.json`
- Create: `schemas/machine_identity.schema.json`
- Create: `schemas/remote_client_identity.schema.json`
- Create: `schemas/pair_ticket.schema.json`
- Create: `schemas/remote_machine_metadata.schema.json`
- Create: `schemas/remote_daemon_state.schema.json`
- Create: `schemas/remote_capability_matrix.schema.json`
- Create: `schemas/remote_capability_set.schema.json`
- Create: `schemas/offline_action_policy.schema.json`
- Create: `schemas/local_remote_switch_policy.schema.json`
- Create: `schemas/remote_feature_advertisement.schema.json`
- Create: `schemas/remote_cursor.schema.json`
- Create: `schemas/control_lease.schema.json`
- Create: `schemas/remote_control_owner.schema.json`
- Create: `schemas/session_runtime_descriptor.schema.json`
- Create: `schemas/session_envelope_projection_v1.schema.json`
- Create: `schemas/remote_projection_snapshot.schema.json`
- Create: `schemas/remote_terminal_lease.schema.json`
- Create: `schemas/remote_status_report.schema.json`
- Test: `tests/remote_harness/lease_reconnect_test.go`

- [x] Implement machine-global remote pairing/auth persistence under `$XDG_STATE_HOME/research-cli/remote/` (fallback `~/.local/state/research-cli/remote/`) via `RESEARCH_CLI_STATE_HOME/remote/` in tests and runtime state-home resolution.
- [x] Implement machine-global machine-metadata and daemon-state status assembly, with machine metadata and daemon-state schema contracts frozen for the M5 substrate.
- [x] Keep project-local `.pmcli/remote/` limited to bindings, cursors, projections, and control-owner state.
- [x] Absorb `Happy`/`Happier` host-control lessons for pairing, scoped
  connectivity, daemon ownership, and mobile control, while deliberately
  replacing Happy-style public relay assumptions with a Tailscale-only private
  overlay transport.
- [x] Selectively port `Happier`-grade daemon ownership, attach/handoff eligibility, and terminal-control contracts where the `Happy` substrate is insufficient.
- [x] Persist machine identity, remote client identity, and pair-ticket lineage as canonical remote auth/control records rather than app-only pairing metadata.
- [x] Implement initial capability publication, cursor initialization/replay policy, control-lease persistence, and remote-status assembly against the frozen remote contracts.
- [x] Implement offline-action policy, local-remote switch policy, and remote feature advertisement records as schema-owned remote substrate objects.
- [x] Implement binding and control-lease storage.
- [x] Implement runtime descriptor and projection snapshot generation.
- [x] Add a projection-mismatch regeneration path so remote workbench state is mechanically treated as derived and non-authoritative.
- [x] Add `research-cli remote daemon` as a local HTTP daemon that serves the phone-browser PWA and exposes JSON endpoints for status, pair, attach, handoff, takeover, notify, and session listing without creating a second runtime state store.
- [x] Document Tailscale private-overlay mode as the preferred direct-control path when the phone cannot route to the workstation's physical `10.x` address.
- [x] Add remote harness coverage for pair, replay, reconnect, and lease
  expiry. The daemon reconnect harness now rebuilds `DaemonContext` over the
  same state home/workspace, replays cursor events, records
  `remote_reconnect`, and rejects reconnect after an expired control lease.
- [x] Add typed action rejection coverage for unpaired `remote attach`;
  revocation visibility and reconnect/lease-expiry harness coverage are active.

**Current M5 substrate evidence:**
- `tests/operator/cli_surfaces.rs::remote_status_and_pair_persist_machine_and_project_substrate`
  verifies `remote status --json`, `remote pair --client <id> --ticket <id>
  --json`, machine-global records, and project-local `.pmcli/remote/` records.
- `tests/operator/cli_surfaces.rs::remote_attach_without_pairing_returns_typed_rejection`
  verifies unpaired attach exits with typed `remote_action_rejected`
  payload and code `12`.
- `tests/operator/cli_surfaces.rs::remote_pair_graduates_lease_capabilities_and_feature_advertisements`
  verifies pairing graduates active lease, ready feature advertisements, and
  attach/handoff/takeover/notify capability publication.
- `tests/operator/cli_surfaces.rs::remote_control_executes_attach_handoff_takeover_notify_and_regenerates_projection`
  verifies local adapter-grade attach inspect/execute, handoff, takeover,
  notify receipts, ownership epoch increments, projection regeneration from
  stale derived state, and canonical event publication.
- `tests/conformance/schemas/batch10_enforcement.rs::remote_status_and_pair_payloads_validate_against_remote_schemas`
  verifies remote CLI payloads and persisted records against offline,
  self-contained schema contracts.
- `tests/conformance/remote_daemon.rs::remote_daemon_serves_installable_mobile_app_shell`
  verifies that the daemon serves the mobile app shell, PWA manifest, and app
  script entrypoint.
- `tests/conformance/remote_daemon.rs::remote_daemon_api_pairs_controls_and_writes_canonical_events`
  verifies that daemon API calls pair a phone client, execute attach, notify,
  refresh status, and write canonical `.pmcli/events/events.jsonl` events
  instead of maintaining mobile-only state.

**Tailscale direct-control runbook:**
1. Start the daemon with `research-cli remote daemon --host 0.0.0.0 --port 8787 --json`.
2. Join the workstation and phone to the same tailnet.
3. If system Tailscale is installed, use `tailscale ip -4` and open
   `http://<tailscale-ip>:8787/` on the phone.
4. If sudo/systemd is unavailable, run a local `tailscaled
   --tun=userspace-networking` and publish the daemon with `tailscale serve`.
5. Treat Tailscale as transport only; all state remains in machine-global
   remote state, project-local `.pmcli/remote/`, and canonical events.

**2026-04-27 mobile/Tailscale reference update:** New source audits of HAPI,
Farfield, codexui, CodexMonitor, itwillsync, TerminalSync, Parallel Code,
Claude Code Viewer, and related projects refine the M5 target without changing the current
graduated claim. The long-term remote plane should expose two governed lanes:

- semantic app-server control for sessions, turns, approvals, model/effort
  controls, structured user input, project status, memory, branches, reviews,
  and research-stage projections
- PTY/xterm terminal parity for exact CLI rendering, resize, input, signals,
  mobile extra keys, and scrollback replay

Tailscale/private overlay is now the preferred near-term transport. Public
relay and native iOS/Android packaging are optional future work. The PTY lane
must not bypass the kernel: every remote terminal attach needs a terminal lease,
scrollback/cursor replay, auditable terminal events, and existing permission and
canonicality gates for any operation that mutates project truth. Raw terminal
parity is required for future mobile full-CLI claims, but it must be wrapped as
a governed projection rather than implemented as an unrestricted shell bridge.

2026-04-27 TUI scope correction: the current Rust implementation exposes an
interactive launch skeleton and rich command/JSON operator surfaces, but it does
not yet implement a complete local TUI. There is no dedicated `src/tui` module
or terminal UI dependency stack in the active implementation. Full local TUI
should therefore be planned with remote/mobile control as one `HostSurface`
layer: local TUI, browser workbench, and phone workbench must read the same
command registry, runtime projections, permission queues, terminal leases, and
canonical project state. The local TUI is not a separate authority and must not
invent UI-only state that remote clients cannot replay.

2026-04-27 TUI design update from claw-code/OpenCode/Crush: claw-code's
`TUI-ENHANCEMENT-PLAN.md` shows the right incremental path from a working
REPL into a polished TUI: first extract rendering/input/session logic, then add
status HUD, live markdown, collapsible tool output, diff rendering, pager,
session picker, themes, and finally optional full-screen split panes. OpenCode
and Crush add the richer product bar: chat timeline, editor/input component,
sidebar, status component, command/model/session/file dialogs, permission
dialog, log table, theme manager, diff view, notification layer, and reusable
layout/overlay primitives. `research-cli` should absorb this as a single host
surface contract, not as local-only UI code.

TUI architecture target:

- `HostSurfaceProjection`: schema-owned snapshot consumed by local TUI,
  browser, and mobile clients. It includes session timeline, current turn,
  tool stream, permission queue, project status, memory recalls/explain,
  branches, reviews, research stages, usage/cost, remote/control owner, and
  degraded feature state.
- `TuiRuntime`: terminal adapter that subscribes to projection updates and
  emits typed actions back through the command registry. It never mutates
  `.pmcli` directly.
- `InlineReplMode`: default low-risk mode, preserving command-first behavior
  and JSON automation compatibility while adding status HUD, syntax-aware
  rendering, pager, collapsible tools, and permission overlays.
- `FullScreenTuiMode`: optional alternate-screen mode with split panes after
  the inline mode is stable. It may use a heavier TUI dependency, but must be
  feature-gated and keep a clean fallback path.
- `HostSurfaceAction`: normalized UI action envelope for local TUI, browser,
  and mobile: submit prompt, steer turn, interrupt, approve/deny permission,
  open artifact, inspect memory, switch session, attach terminal, resize PTY,
  and acknowledge HITL.

Required local TUI panes and overlays:

- conversation/timeline pane with streamed assistant content, reasoning state,
  tool-call markers, and transcript refs
- input/editor pane with slash-command palette, multiline input, completion for
  commands, models, sessions, files, skills, MCP servers, and branches
- status bar/HUD with active project, session, model/provider, permission mode,
  git branch, token/cost, turn timer, remote owner, and degraded feature flags
- tool activity panel with per-tool lifecycle, duration, stdout/stderr
  summaries, saved-output refs, and collapse/expand controls
- permission/HITL overlay showing requested action, path/command summary,
  diff/evidence refs, scope, consequences, and approve/deny options
- diff/artifact pane with colored unified diffs, file history, artifact-family
  latest/superseded state, and canonicality gate status
- memory pane with injected memories, recall route, confidence, support refs,
  invalidated/superseded/contested state, and explain links
- branch/review pane with M9 batches, candidate status, verifier/debate gates,
  review queue, and winner/publication state
- research DAG pane with active thread, stage, pending operation, open
  questions, agreed decisions, and next recommended action
- logs/diagnostics pane for doctor/setup/provider/MCP/plugin degradation
  without flooding the main conversation

TUI proof rules:

- every rendered pane must have a `--json` projection or fixture equivalent so
  behavior is testable without a real terminal
- snapshot/reducer tests must prove that resize, scroll, collapse/expand,
  permission approval, session switch, and reconnect replay do not change
  project truth except through typed actions
- no TUI feature may depend on parsing styled terminal text when a structured
  record exists
- the same projection must feed remote browser/mobile panels; local TUI can add
  keyboard ergonomics but not a different authority model
- long outputs must be paged or collapsed by default, with durable artifact refs
  for full logs
- permission and publication actions must reuse existing permission,
  `ChangeEnvelope`, canonicality, and publication gates

Future remote helper surfaces:

- `host surface status --json`: report local TUI, web, mobile, semantic lane,
  and PTY lane readiness from the same capability registry.
- `tui launch --json`: launch the local terminal UI as a projection over
  kernel-owned runtime state, not as a separate REPL runtime.
- `tui snapshot --json`: emit the current `HostSurfaceProjection` for tests,
  remote clients, and no-TTY inspection.
- `tui actions --json`: list available typed UI actions for the active
  projection, including disabled reasons.
- `remote tailscale status --json`: detect Tailscale through CLI and network
  interface fallback, validating `100.64.0.0/10` addresses.
- `remote tailscale serve-plan --json`: emit a machine-readable direct URL or
  `tailscale serve` plan for the active daemon.
- `remote terminal attach --json`: create a terminal lease and PTY projection
  instead of handing out ungoverned shell access.
- `remote terminal replay --json`: replay scrollback from a remote cursor.

2026-04-27 EvoScientist channel-adapter update: EvoScientist's channel bus and
consumer show that remote/mobile control needs more than transport reachability.
Future M5 work should add channel-style hardening around any chat, phone, or
notification adapter: inbound message deduplication, sender/channel allowlists,
pairing expiry, group mention gating, per-chat/session serialization, bounded
queues with backpressure, idle timeouts, late-response handling, metrics, and
typed ask_user/HITL resume. These adapters must route through the same command
registry and kernel-owned runtime state as local CLI/TUI commands.

**Done when:**
- remote control state survives restart
- daemon ownership and last-known machine state survive restart or graceful
  shutdown
- projection and ownership use canonical remote persistence only
- projection mismatch is repaired by regeneration rather than by inventing a
  second authority
- the remote code path clearly reads as `Happy`-based adaptation plus bounded
  `Happier` feature import, not a bespoke remote rewrite

### Task 3: Remote commands and workbench surfaces

**Rust implementation status (M5 command-surface slice):** `remote pair`,
`remote status`, `remote attach`, `remote handoff`, `remote takeover`,
`remote notify`, and `remote daemon` are wired into the Rust CLI. The
graduated M5 surface is local adapter-grade: it enforces pairing, validates
sessions, emits typed rejections for invalid control state, persists
control-owner epoch, regenerates derived projection snapshots, publishes
canonical remote events, and exposes those controls to a phone browser through
the daemon-served PWA. Real encrypted relay transport and native app packaging
remain outside this completed slice.

**Files:**
- Modify: `cmd/research-cli/main.go`
- Create: `internal/remotehost/capabilities/capabilities.go`
- Create: `internal/remotehost/terminal/terminal.go`
- Create: `internal/remotehost/workbench/workbench.go`
- Create: `schemas/remote_action_rejection.schema.json`
- Test: `tests/golden/remote/`

- [x] Implement `remote pair`, `remote status`, `remote attach` as local substrate commands with inspect and execute modes.
- [x] Implement `remote handoff`, `remote takeover`, `remote notify` as graduated local adapter-grade command surfaces rather than unknown or non-graduated commands.
- [x] Implement capability advertisement and terminal policy reporting in `remote status`; executable terminal policy enforcement is active for the local terminal-host strategy.
- [x] Implement typed remote rejection payload assembly so lease, capability,
  ownership, and feature-health failures return `RemoteActionRejection`.
- [x] Implement remote workbench projections for project status, branches, reviews, and memory state.
- [x] Implement local TUI projections for session summaries, permissions,
  project status, memory, branches, reviews, and research-stage state using
  the same projection contracts as the remote workbench. Tool streams,
  artifact diffs, diagnostics, and empty-state research panes are exposed with
  explicit `contract_ready` readiness and data refs rather than false
  `available` claims.
- [x] Implement TUI snapshot/action fixtures for status HUD, command palette,
  permission overlay, tool timeline, diff/artifact pane, memory explain pane,
  branch/review pane, and research DAG pane. Pager/collapse, resize handling,
  and session switch are represented as typed action/projection contracts but
  not yet as a raw-mode renderer.
- [x] Add an inline REPL enhancement path before optional full-screen mode:
  status HUD, live markdown rendering, collapsible tool output, colored diffs,
  pager for long outputs, interactive session picker, theme capability, and
  syntax-aware tool-result rendering.
- [x] Add optional full-screen split-pane TUI only after inline mode and
  projection fixtures are stable; it must be feature-gated and degrade cleanly
  on unsupported terminals, SSH, tmux, or CI.
- [x] Add Tailscale detection and serve-plan helper commands.
- [x] Add governed PTY/xterm terminal-parity lane with terminal lease,
  scrollback cursor replay contract, typed resize/input/signal policy, mobile
  extra-key support, canonical attach/replay events, and clear
  `host_pty_byte_stream` status for the WebSocket path. The HTTP fallback
  remains a governed intent recorder for degraded browsers and tests.
- [x] Add watch-only versus control-mode separation so log/session browsing
  does not imply write/approve authority.

2026-04-28 implementation update: M14 now has a shared `HostSurfaceProjection`
contract in Rust. `host surface status --json`, `tui launch --json`,
`tui snapshot --json`, and `tui actions --json` expose the same kernel-owned
projection for local TUI, browser, and mobile clients. `tui launch` is currently
an inline/raw-mode projection renderer with an optional full-screen split-pane
mode, not a separate runtime. The action envelope now covers submit, steer,
interrupt, approve/deny permission, HITL ack, artifact open, memory inspect,
session switch, remote attach, terminal attach/replay, and terminal resize
readiness. `remote tailscale status --json` and `remote
tailscale serve-plan --json` add a Tailscale-first direct connection helper.
`remote terminal attach --json` and `remote terminal replay --json` add the
governed terminal lane with lease, scrollback cursor, canonical replay event,
resize/signal policy, and mobile extra-key policy. This is deliberately not
claimed as an unrestricted shell: the current slice proves the authority,
projection, TUI renderer, mobile product, reconnect, governed terminal
contract, and a lease-gated host PTY byte pump. The PTY stream stays behind the
same Tailscale, lease, canonical-event, and checkpoint gates.

2026-04-28 product-surface update: the daemon-served mobile PWA now consumes
the shared host-surface contract directly. `research-cli remote daemon` exposes
`GET /api/host-surface`, `GET /api/tui/actions`,
`POST /api/terminal/attach`, and `GET /api/terminal/replay`, all backed by the
same Rust `HostSurfaceProjection`, `HostSurfaceAction`, and governed terminal
projection used by the CLI. The phone UI has moved from a status-only control
dashboard to a projection-driven workbench with session thread, command palette,
permission/workbench panels, and a terminal-lane projection panel. Mutations
still go through the existing remote lease, permission, event, and checkpoint
paths. Transport scope is intentionally Tailscale/private-overlay only: there
is no self-hosted public relay in this product line, and no UI may imply one.
Native iOS/Android packaging remains a later thin-client wrapper over the same
Tailscale daemon API, not a separate runtime. The WebSocket terminal path now
streams real host PTY bytes; current HTTP terminal endpoints remain governed
fallback actions rather than a second shell.

Product principle update: remote web and app surfaces must be structured
conversation products first, not terminal mirrors. The default mobile/web
experience should look and behave like a code-agent conversation: streamed
assistant turns, user messages, tool-call cards, permission cards, diff/code
panes, test/result panels, artifact previews, memory/review/agent status, image
attachments, and voice/text input all backed by the shared
`HostSurfaceProjection` and canonical event log. The terminal lane exists as a
complete CLI-compatibility and debugging fallback so no local CLI affordance is
lost; it must not become the primary product model or a separate source of
truth.

2026-04-28 mobile-action hardening update: the PWA command palette now executes
mapped `HostSurfaceAction` entries instead of logging them as inert UI events.
Pairing refreshes the shared host-surface view immediately, `switch_session`
uses a typed `/api/session/resume` mutation that records canonical state, and
`GET /api/terminal/replay` is explicitly read-only so projection replay cannot
create events by observation. The interrupt action is labeled as "Request
Interrupt" because the current remote endpoint records an interrupt request; it
does not yet prove runtime preemption of a running model/tool loop.

**Next product implementation sequence:**
- First complete the inline local TUI path on top of the existing
  `HostSurfaceProjection`: status HUD, markdown renderer, collapsible tool
  output, diff/pager, permission overlay, session picker, theme selection, and
  deterministic snapshot tests. This upgrades the default terminal experience
  without introducing a second runtime.
- Then upgrade the PWA command palette from button-only dispatch to
  schema-driven action forms: required input validation, disabled-reason
  surfacing, action result toasts, field prefill from projection context, and
  no placeholder command paths.
- Then promote the remote PWA/App to a structured code-agent conversation:
  streamed conversation timeline, semantic tool-call cards, permission cards,
  diff/code panes, test/result panels, artifact previews, memory/review/agent
  status panels, image attachment affordances, and voice/text input. These
  views consume structured projections/events first and may reveal terminal
  bytes only as an expandable compatibility lane.
- Completed true terminal parity over the existing Tailscale bridge for the
  WebSocket path: attach the governed bridge to a real host PTY byte pump
  running `research-cli tui launch --fullscreen`, add a bounded ANSI/CSI
  viewport renderer in the PWA terminal lane, record sequence-numbered terminal
  output byte chunks, and keep reconnect/revocation fixtures. This remains
  Tailscale-only over loopback plus Tailscale IPv4/IPv6 overlay peers and cannot
  bypass permission, lease, canonical-event, or checkpoint gates.
- Then add a streaming host-surface channel for web/mobile, replacing polling
  where possible with authenticated long-poll or WebSocket event cursors while
  preserving replay-from-cursor semantics for reconnect.
- Then add multi-server and multi-session product UX: saved Tailscale
  endpoints, server health, session tabs, active control owner, watch/control
  mode switch, and per-server token status. These are view/adaptor concerns
  only; project truth remains in `.pmcli/` and machine-global remote state.
- Finally add rich code-agent mobile affordances: diff/code panes, artifact
  viewer, image attachment, voice dictation, and mobile extra-key presets. The
  native app remains deferred until the PWA proves the shared host-surface API.

2026-04-28 inline-TUI implementation update: `tui launch --json` now returns
an `inline_projection_renderer` contract with `tui_inline_view.v1`. The view
model includes status HUD badges, markdown message rendering, collapsible tool
output, colored diff panes, internal pager support, permission overlay,
session picker keybindings, and auto dark/light theme selection. This is still
projection-first and schema-backed: it upgrades the local product surface
without introducing a second runtime or hidden UI-owned state. The remaining
work before full local TUI parity is the actual raw-mode event loop and
renderer that consumes this view model.

2026-04-28 raw-mode/PWA product update: `tui launch` in a real terminal now
enters a raw-mode alternate-screen loop backed by the same `TuiInlineView` and
exits on `q`, `esc`, `ctrl-c`, or `ctrl-d`; non-TTY and CI environments degrade
to a deterministic inline snapshot for tests and logs. The daemon PWA command
palette has also moved from button-only dispatch to schema-derived action
forms with required-field validation, disabled-reason display, and action
result toasts. This keeps local TUI, web, and mobile products on one
host-surface contract while adding real operator ergonomics.

2026-04-28 Tailscale terminal-bridge update: the remote daemon now exposes a
Tailscale/private-overlay-only terminal bridge surface. The mobile PWA has a
WebSocket connection path at `/api/terminal/ws` plus HTTP fallbacks for
`POST /api/terminal/input`, `POST /api/terminal/resize`, and
`POST /api/terminal/signal`; all of them require the remote lease, reuse the
governed terminal projection, write canonical `remote_terminal_*` events, and
publish checkpoints. Browser WebSocket auth uses the same daemon control token
through a query parameter because browser WebSocket clients cannot set custom
Authorization headers. The WebSocket path now starts a real host PTY with
`forkpty`, runs the governed `research-cli tui launch --fullscreen` surface
instead of an unrestricted shell, records lease/checkpoint-accepted
input/resize/signal operations before they touch the PTY master, streams base64
PTY output chunks as `remote_terminal_pty_output.v1`, and records canonical
`remote_terminal_output` events before sending output frames to the browser.
The PTY render child disables command-audit writes so the daemon remains the
single project-log writer for remote terminal activity. HTTP terminal endpoints
remain `contract_ready_not_host_pty` fallbacks for browsers or tests that cannot
keep the WebSocket byte stream open.

2026-04-28 structured-conversation product update: the daemon PWA now has a
primary `conversation_primary_surface` with a structured transcript timeline,
semantic card rail, conversation composer, image-attachment affordance, and
voice-input affordance. The daemon exposes
`GET /api/session/transcript` as a read-only, lease-gated typed transcript
projection (`remote_session_transcript.v1`) so the web/app product can render
messages, tool calls, tool results, permission state, memory/review status,
artifact/diff summaries, and terminal compatibility state without parsing
styled terminal text. This keeps the terminal lane as a
`terminal_compatibility_lane` rather than the primary remote UX.

2026-04-28 multi-server product update: the daemon PWA now includes a
Tailscale server directory. Operators can save multiple daemon origins, mark
one origin as the active API target, probe `/api/health`, and then continue to
use the same pairing, host-surface, transcript, command-palette, and terminal
compatibility APIs against that active origin. This is intentionally a
view/adaptor feature: it does not introduce a second runtime, a server-side
cloud registry, or a public relay. The canonical project truth remains in the
selected daemon's `.pmcli/` and machine-global remote state, while the browser
only stores local Tailscale endpoint shortcuts.

2026-04-28 streaming-cursor product update: `/api/events` now accepts
`wait_ms` in addition to `after_seq`, making the browser/mobile host-surface
refresh path a replayable long-poll cursor instead of a fixed blind poll. The
response remains SSE-shaped text sourced only from canonical
`.pmcli/events/events.jsonl`; empty windows return a heartbeat containing the
cursor and wait duration. The PWA sends `wait_ms=25000`, then refreshes the
same `HostSurfaceProjection` and transcript views when canonical remote events
arrive.

2026-04-28 artifact/code-pane product update: the daemon PWA now has a real
`artifact_code_diff_pane` instead of only an artifact affordance. The daemon
exposes read-only, lease-gated `GET /api/artifacts` and
`GET /api/artifact/inspect?target=...` endpoints backed by the existing
artifact registry and workspace-scope inspection path. The PWA renders
governed artifact families, opens selected artifacts, and previews text,
Markdown, JSON, directory listings, and unified diffs with a bounded
64 KiB preview. This keeps artifact/code viewing inside the existing
governance layer; the web/mobile client does not scan the filesystem directly
and does not create a second artifact truth.

2026-04-28 test/result panel product update: the daemon PWA now exposes a
`test_result_panel` next to the structured conversation and artifact panes.
`GET /api/results` is a read-only, lease-gated projection over governed report
locations (`reports`, `.pmcli/reports`, branch evaluation state, and research
runtime results). The panel previews available result directories/files with
the same bounded artifact preview path and marks missing result targets
explicitly. This is not a test executor and not a second result store; it is a
mobile/web projection for existing canonical artifacts and workspace-scoped
result outputs. The artifact pane also gained an open-by-family/path input so
operators can inspect exact code, diff, report, or artifact refs without
leaving the structured remote UI.

2026-04-28 conversation attachment product update: image input is now a real
governed remote action instead of a placeholder affordance. The PWA converts
selected images into bounded base64 payloads and calls lease-gated
`POST /api/session/attachments`; the daemon validates image MIME types,
persists files under `.pmcli/remote/attachments/<session>/`, writes a
canonical `remote_attachment` event, appends a transcript control marker, and
publishes a checkpoint. This gives mobile/web an IM-like image handoff path
without letting the browser own session truth or bypass the project artifact
store. Native camera/gallery UX can later wrap the same endpoint.

2026-04-28 session-tabs/watch-control product update: the PWA now exposes
explicit Watch and Control modes plus first-class session tabs in the
structured conversation surface. Watch mode can browse sessions, transcripts,
artifact/code previews, result panels, and server health without implying write
authority. Control mode is required before mutating UI actions such as message
steering, image uploads, permission responses, session resume, attach/takeover,
and terminal input/resize/signal. This is a product safety layer only; the
daemon lease, permission, and checkpoint gates remain the actual authority.

2026-04-28 voice-dictation product update: the PWA Voice button is no longer a
placeholder. When the browser provides `SpeechRecognition` or
`webkitSpeechRecognition`, mobile/web operators can dictate into the structured
conversation composer with visible listening state and a clear unsupported
browser fallback. Dictation is local input composition only: it does not send,
mutate, approve, or bypass governance. The final message still travels through
the existing Control-mode, lease-gated, canonical remote message flow.

2026-04-28 attachment-preview product update: the structured conversation
composer now previews selected image attachments before upload, including
thumbnail, filename, size, daemon-limit warning, and a clear-images control.
This is client-side composition state only. Successful upload still clears the
browser selection after the lease-gated attachment endpoint writes canonical
remote events and stores governed files under `.pmcli/remote/attachments/`.

2026-04-28 watch-mode projection retention update: session-tab browsing in
Watch mode now keeps the latest `HostSurfaceProjection` while swapping only the
read-only transcript being inspected. This prevents semantic cards for memory,
reviews, branches, artifacts, terminal readiness, and pending permissions from
disappearing when an operator browses a non-active session. The active tab
state is updated locally for product clarity; authoritative session control
still requires Control mode and the daemon resume endpoint.

2026-04-28 mobile-terminal-extra-key update: the terminal compatibility lane
now includes mobile extra-key presets for Ctrl-C, Esc, Tab, Enter, and Ctrl-D.
These buttons are convenience controls over the existing terminal input path:
they require Control mode, prefer the Tailscale WebSocket bridge when
connected, fall back to the lease-gated HTTP terminal input endpoint, and write
the same canonical remote terminal events. They do not change the product
principle that structured conversation remains the default remote surface.

2026-04-28 full-screen TUI product update: `tui launch --fullscreen` and
`tui launch --split-pane` now select an optional full-screen split-pane
projection renderer over the same `TuiInlineView` and `HostSurfaceProjection`.
The mode reports `fullscreen_split_pane_projection_renderer`,
`alternate_screen_split_pane`, and `fullscreen_split_pane_with_sidebar` in JSON
and degrades to a deterministic non-TTY snapshot in CI/SSH/log contexts. This
keeps full-screen mode as a renderer selection over the existing kernel-owned
projection, not a second runtime or hidden UI state machine.

2026-04-28 reconnect-harness product update: the remote daemon now exposes
`POST /api/reconnect` for Tailscale/mobile clients. Reconnect validates the
existing pairing and control lease, reuses the persisted project cursor and
event log, returns cursor-based replay events, records a canonical
`remote_reconnect` event, and refreshes the same `HostSurfaceProjection`. The
PWA server panel exposes this as a Reconnect action for mobile network drops.
Expired leases and mismatched client identities reject reconnect with the same
typed `RemoteActionRejection` used by attach/message/terminal mutations.

**Current M5 command-surface evidence:**
- `tests/operator/cli_surfaces.rs::remote_graduated_control_commands_advertise_ready_capabilities`
  verifies attach, handoff, takeover, and notify are advertised as ready
  capabilities after pairing.
- `tests/conformance/schemas/batch10_enforcement.rs::remote_action_rejections_validate_against_schema`
  verifies remote rejection payloads against
  `schemas/remote_action_rejection.schema.json`.

**Done when:**
- remote follow/control is operator-grade and fixture-backed

### Task 4: Agent runtime and task packet lane

**Reference-superiority preflight:** Before implementing this task, apply the
M6 entry gates in
`docs/deep_study/51-m6-m10-reference-superiority-design-audit.md`.
In particular, do not reduce `TaskPacket` to a prompt wrapper. It must carry
the same class of execution-run authority that Happier models through
intent/retention/run-class/IO/resume/replay fields, while remaining
project-governed and kernel-authoritative. EvoScientist adds a product-UX
lesson on top of that: research roles should be explicit role templates
(`planner`, `research`, `code`, `debug`, `data-analysis`, `writing`) with
success signals and output obligations, but the authority still lives in the
typed task packet rather than the prompt text.

**Files:**
- Create: `internal/agents/store.go`
- Create: `internal/agents/runtime.go`
- Create: `internal/agents/task_packet.go`
- Create: `schemas/task_packet.schema.json`
- Create: `schemas/agent_runtime_record.schema.json`
- Create: `schemas/agent_trace.schema.json`
- Create: `schemas/agent_output_manifest.schema.json`
- Test: `tests/conformance/runtime/task_packet_test.go`
- Test: `tests/golden/agents/agent_stop_test.go`

- [x] Implement agent runtime records and task-packet persistence.
- [x] Extend `TaskPacket` with intent, role/profile, retention policy, run
  class, IO mode, resume policy, replay seed reference, budget, scope, write
  authority, success criteria, output-manifest obligation, and review-gate
  requirement.
- [x] Extend `AgentRuntimeRecord` with durable lifecycle status, timestamps,
  heartbeat, task-packet ref, output-manifest ref, trace refs, stop reason,
  typed failure code, runner kind, and optional workspace/directive/status refs.
- [x] Add a deterministic mock-agent harness before any live-provider agent
  behavior is marked graduated.
- [x] Implement `.pmcli/agents/<agent_id>/` persistence.
- [x] Implement `agents list/inspect/stop/traces`.
- [x] Implement bounded local runner execution in an isolated git worktree with
  packet-bound `DIRECTIVE.md`, `STATUS.md`, stdout/stderr capture, workspace
  binding, output manifest, and typed lifecycle trace.
- [x] Add tests for task-packet-required startup, invalid packet rejection,
  stale scope rejection, safe idempotent stop, timeout, crash, replay/resume
  policy, and output-manifest validation.
- [x] Reject local-agent startup from a dirty non-`.pmcli` source worktree with
  a typed failure before creating the isolated agent worktree.

**Done when:**
- every agent run is packet-bound and inspectable
- M6 lifecycle fixtures pass without live providers

**Current implementation note (2026-04-26):** Task 4 is implementation-backed.
It has deterministic mock lifecycle coverage, external task-packet validation,
stale-scope and missing-write-authority rejection, dirty source-worktree
rejection, safe idempotent stop, bounded local execution in isolated git
worktrees, prompt timeout with process-group termination, signal/crash
classification, output-manifest validation, and `agents replay <agent-id>`
replay semantics. The local runner remains deliberately packet-bound and
inspectable before any live-provider delegation is exposed.

### Task 5: Review isolation and reviewer blinding

**Files:**
- Modify: `internal/reviews/store.go`
- Modify: `internal/agents/runtime.go`
- Create: `internal/reviews/blinding.go`
- Create: `schemas/review_runtime_record.schema.json`
- Test: `tests/golden/reviews/review_blinding_test.go`

- [x] Enforce blinded review packet creation.
- [x] Implement a `BlindedReviewPacket` / review-input redaction gate that
  proves executor interpretation is absent from default reviewer input.
- [x] Prevent executor interpretation from leaking into reviewer input by default.
- [x] Add retry and compare-linkage support.

**Done when:**
- reviewer independence is mechanically enforced

**Current implementation note (2026-04-26):** Task 5 is implementation-backed.
`reviews open` builds a blinded packet with
`blinded=true`, `blinding_policy=default_reviewer_blinding`, and a
`ReviewInputRedaction` record; `--executor-summary` is accepted only to prove it
is redacted and absent from the persisted packet and prompt snapshot.
`reviews retry <review-id>` is allowed only for timeout, transport failure, or
explicit `--force` rerun policy, emits a fresh-thread review run, records
`retry_of` / `retry_attempt`, and preserves explicit `compare_against` linkage.

### Task 6: Branch scheduler and evaluation packets

**Files:**
- Create: `internal/branches/scheduler.go`
- Create: `internal/branches/evaluation.go`
- Create: `schemas/branch_batch.schema.json`
- Create: `schemas/branch_run.schema.json`
- Create: `schemas/branch_batch_record.schema.json`
- Create: `schemas/branch_run_record.schema.json`
- Create: `schemas/evaluation_packet.schema.json`
- Test: `tests/conformance/research/evaluation_packet_test.go`
- Test: `tests/golden/branches/branch_promote_gate_test.go`

- [x] Implement `SearchBatch`, `BranchRun`, branch quotas, and lineage records.
- [x] Implement deterministic variation operators for candidate mutation, repair,
  crossover, simplification, test expansion, refresh, and canonicalization.
- [x] Implement stale-base detection and refresh-required transitions.
- [x] Implement evaluation packet compilation before promotion, including real
  `--eval-command` execution and canonicality gate capture.
- [x] Implement debate trace persistence and red-team comparison.
- [x] Implement `branches search/list/inspect/evaluate/debate/promote/archive`.

**Done when:**
- no branch can promote without evaluation and debate/review gates
- no second winner can promote in the same batch
- losing branches remain hidden archived search artifacts, not public truth

### Task 7: Working memory and digest candidates

**Files:**
- Create: `internal/memory/mission_frame.go`
- Create: `internal/docs/doc_frame.go`
- Create: `internal/docs/index.go`
- Create: `internal/memory/working.go`
- Create: `internal/memory/eviction.go`
- Create: `internal/projectops/digest.go`
- Create: `schemas/mission_frame.schema.json`
- Create: `schemas/goal_alignment_trace.schema.json`
- Create: `schemas/doc_frame.schema.json`
- Create: `schemas/doc_index.schema.json`
- Create: `schemas/working_memory_record.schema.json`
- Create: `schemas/progress_digest_candidate.schema.json`
- Create: `schemas/projectops_tick.schema.json`
- Test: `tests/conformance/runtime/working_memory_test.go`
- Test: `tests/conformance/docs/doc_frame_test.go`

Current Rust implementation maps these planned files to
`src/memory/mod.rs`, `src/projectops/mod.rs`, and
`tests/conformance/runtime/working_memory.rs`.

- [x] Implement project-scoped `MissionFrame` persistence for max goal,
  milestone goal, current implementation goal, non-goals, success criteria,
  and evidence refs.
- [x] Implement bounded mission-frame projection for prompt/resume wake-up
  context. Subagent task-packet propagation remains not graduated.
- [x] Implement `GoalAlignmentTrace` emission for non-trivial implementation,
  compact, and review lanes. `prompt` now emits the implementation-lane trace,
  `compact` emits a strict-schema compaction trace, and `reviews open` emits the
  review-lane trace using the same deterministic MissionFrame support check.
- [x] Implement `DocFrame` parsing for Markdown front matter and fenced JSON
  blocks, normalized through `schemas/doc_frame.schema.json`.
- [x] Implement rebuildable `.pmcli/docs/index.json` generation through
  `schemas/doc_index.schema.json`.
- [x] Implement stale-frame detection through `schemas/doc_index.schema.json`.
- [x] Implement `docs index`, `docs inspect`, and `docs frame refresh --dry-run`
  as the first operator surfaces for document context blocks.
- [x] Allow prompt/resume/compact to consume active non-stale `DocFrame`
  projections as optional context through
  `schemas/doc_context_projection.schema.json` while keeping `MissionFrame`
  authoritative. Milestone planning consumes the same indexed document surface
  through the project document index.
- [x] Implement working-memory append store and pin/eviction rules. `memory
  append/status` now persist a bounded active JSONL working set, retain an
  append log, evict oldest unpinned records deterministically, and report
  schema-valid `MemoryStatusReport`.
- [x] Implement digest-candidate generation with support refs. `compact` now
  stages schema-valid `ProgressDigestCandidate` files under
  `.pmcli/memory/promotion_queue/` with summary and transcript support refs.
- [x] Implement `ProjectOpsTick` publication for silent summary and maintenance triggers.
  Session compaction writes schema-valid `ProjectOpsTick` records under
  `.pmcli/projectops/ticks/` linking back to the digest candidate.
- [x] Implement session-end silent summary staging. Current graduated path is
  compaction-triggered silent summary staging: it is reviewable,
  trace-linked, and explicitly does not promote durable memory. Durable
  query/explain/invalidate remain M8.

**Done when:**
- project goal hierarchy survives resume and compact without summary drift
- document and milestone context can be rebuilt from schema-valid `DocFrame`
  blocks without trusting free-form summaries
- long sessions remain bounded
- summaries remain reviewable/promotable rather than silently trusted

### Task 8: Durable memory, query/explain, and invalidation

**Files:**
- Implemented in Rust: `src/memory/mod.rs`
- Wired through: `src/runtime/mod.rs`, `src/remote/mod.rs`
- Create: `schemas/memory_record.schema.json`
- Create: `schemas/memory_query_result.schema.json`
- Create: `schemas/memory_explain_record.schema.json`
- Test: `tests/conformance/runtime/working_memory.rs`

- [x] Implement `MemoryRecord` persistence.
- [x] Implement bounded hybrid retrieval with explainable routes, budgets, and
  provenance. The implemented backend is deterministic and local: exact
  identifier/source-artifact, provenance, support-ref, lexical,
  semantic-lite token-overlap, hashed vector-embedding, temporal-graph, and
  temporal-trust lanes are fused by weighted RRF. The vector lane is an embedded
  local backend, not a network vector database. The temporal-graph lane follows
  supersession/support-ref/session relationships without requiring Neo4j/FalkorDB.
- [x] Keep `MissionFrame` as privileged L0 project context outside ordinary
  durable-memory ranking.
- [x] Implement invalidation, supersession, contested, and expired non-injecting
  states. First-pass decay is represented through explicit `expired` status;
  no embedding/vector decay backend is required for M8 graduation.
- [x] Implement `MemoryExplainRecord` surfaces for contested, superseded, and invalidated memory.
- [x] Implement durable `memory promote/query/explain/invalidate`; `memory
  status` now reports both working-memory and durable-memory counts.
- [x] Invalidate trusted durable memory that references restored cleanup
  artifacts before the next auto-inject cycle.

**Done when:**
- project memory is explain-backed and not prompt stuffing
- identifier-heavy queries can beat noisy lexical matches through provenance
  and exact source-artifact lanes
- semantic-alias queries can recover vector-backed memories without exact term
  overlap, and superseded memories can route to the current successor through
  temporal-graph edges
- invalidated, superseded, contested, and expired records are explain-visible
  only and do not auto-inject
- cleanup restore invalidates dependent trusted memory before later query
  injection

### Task 9: ProjectOps governance and supervision

**Files:**
- Create: `internal/projectops/projectops.go`
- Create: `internal/projectops/cleanup.go`
- Create: `internal/projectops/supervisor.go`
- Create: `schemas/cleanup_plan.schema.json`
- Create: `schemas/repo_cleanup_proposal.schema.json`
- Create: `schemas/experiment_supervisor_lease.schema.json`
- Create: `schemas/wake_event.schema.json`
- Test: `tests/golden/repo/cleanup_plan_test.go`

- [x] Implement cleanup proposal generation.
- [x] Implement supervisor lease and wake-event state machine with duplicate
  live-lease rejection, stale lease refresh, reclaimed lease lineage, unowned
  wake escalation with explicit no-owner reason, and ack/resolve/escalate wake
  transitions.
- [x] Implement persisted `RepoCleanupProposal`, `ExperimentSupervisorLease`, and `WakeEvent` objects rather than ad hoc supervisor state.
- [x] Implement project inspection surfaces wired to artifacts/branches/reviews and explicit M8 memory gate state.
- [x] Add typed fixture coverage for `memory status`, `artifacts list/inspect`,
  and `repo cleanup-plan/cleanup-apply`.
- [x] Add operator coverage for ProjectOps duplicate ownership rejection,
  missing-lease rejection, no-owner escalation, wake ack/resolve, stale detection,
  and lease reclaim.

2026-04-27 ModelScope ULTRON correction: the intended ULTRON reference is
`modelscope/ultron`, not the earlier mistakenly pulled Telegram bridge. Its
source-level contribution is a collective-intelligence substrate, not a
planner/coder/reviewer scheduler: Memory Hub captures tiered shared memories;
Trajectory Hub segments `.jsonl` sessions, fingerprints spans, scores segment
quality, and gates memory extraction; Skill Hub crystallizes and
re-crystallizes skills from memory clusters with provenance and structure-score
gates; Harness Hub shares allowlisted agent profiles while excluding secrets,
logs, sessions, and hidden files. Future ProjectOps should absorb ULTRON's
health/adoption signals as typed records, but direct shell autonomy remains
forbidden; remediation must pass through the existing permission, lease, and
canonicality gates.

**Current ProjectOps supervision evidence:**
- `tests/operator/cli_surfaces.rs::projectops_supervisor_lease_and_wake_events_are_persisted_and_listed`
  verifies the persistence/read surface.
- `tests/operator/cli_surfaces.rs::projectops_enforces_supervision_state_machine_transitions`
  verifies live-owner exclusivity, missing-lease rejection, unowned escalation,
  wake acknowledgement/resolution, heartbeat-based stale detection, and reclaim
  lineage.

**Done when:**
- proactive project management is runtime-native and reversible

### Task 10: Debate and research stage runtime

**Files:**
- Create: `internal/branches/debate.go`
- Create: `internal/research/runtime.go`
- Create: `internal/research/stages.go`
- Create: `schemas/change_envelope.schema.json`
- Create: `schemas/debate_packet.schema.json`
- Create: `schemas/debate_trace.schema.json`
- Create: `schemas/research_skill_contract.schema.json`
- Create: `schemas/skill_manifest.schema.json`
- Create: `schemas/skill_availability_record.schema.json`
- Create: `schemas/research_stage_execution.schema.json`
- Create: `schemas/stage_execution_map.schema.json`
- Create: `schemas/research_thread.schema.json`
- Create: `schemas/deliberation_span.schema.json`
- Create: `schemas/research_turn_classification.schema.json`
- Test: `tests/conformance/research/stage_execution_test.go`

- [x] Implement debate packet/trace persistence.
- [x] Implement `StageExecutionMap` routing.
- [x] Require `ChangeEnvelope` on mutating research stages.
- [x] Implement nonlinear `ResearchStageExecution` graph nodes with `advance`,
  `retry`, `repair`, `pivot`, `fork`, `supersede`, `abandon`, and
  `human_override` operation classes.
- [x] Implement `ResearchThread` and `DeliberationSpan` persistence so any
  research stage can host long multi-turn discussion without losing runtime
  position.
- [x] Add research-turn classification that updates active thread, active stage,
  deliberation mode, open questions, candidate options, agreed decisions,
  pending operations, and next recommended action after each research-relevant
  turn.
- [x] Require `ResearchTurnClassification` to carry confidence, evidence refs,
  alternative interpretations, natural-language summary, and
  `human_gate_required`; high-impact operations must stay pending until
  `research decide` or equivalent interactive confirmation.
- [x] Implement `research classify --text <turn> --dry-run --json` as a
  non-mutating debugging surface before automatic prompt/resume integration.
- [x] Inject bounded `ResearchThread` and `DeliberationSpan` projections into
  prompt/resume/continue/compact after `MissionFrame` and before ordinary
  memory recall.
- [x] Implement `research status`, `research threads list`,
  `research thread inspect`, `research stage inspect`, `research decide`, and
  `research record` operator surfaces with JSON contracts.
- [x] Implement research stage runtime and repair/pivot/retry edges.
- [x] Bind `fork` operations to M9 `SearchBatch` records whenever a stage
  explores competing executable hypotheses, repairs, experiment plans, or
  research narratives.
- [x] Require `supersede` operations to update artifact-family latest pointers,
  refresh DocFrames, and preserve a single public canonical surface.
- [x] Require `abandon` operations to preserve evidence and failure lessons
  without leaving active public artifacts behind.
- [x] Implement `skills list`, `skills inspect`, `skills paths`, and
  `skills validate` baseline.
- [x] Freeze `SkillRegistryEntry`, `SkillListResult`, `SkillInspectResult`,
  `SkillPathsResult`, and `SkillValidationResult` fixtures before stage runtime
  depends on them.
- [x] Implement skill discovery/compatibility/manifest validation baseline.
- [x] Implement `SkillAvailabilityRecord` so discovered-but-disabled skills stay machine-readable and stage compatibility is runtime-owned.

Reference-code lesson applied in the implemented M10 hot path: ClawMem's
`context-surfacing` and memory-store-plugin's session/context scripts show that
operator runtimes should inject compact, bounded context at prompt/resume and
pre-compact boundaries instead of rebuilding full memory or spawning a slow
classification worker every turn. Research CLI follows that pattern but makes
the injected object schema-owned as `ResearchContextProjection`, with
MissionFrame priority first and durable memory later.

2026-04-27 EvoScientist research-runtime update: EvoScientist's planner
reflection JSON and ask_user middleware show two future refinements for M10.
Planner reflection should become a typed `StageReflectionPacket` that can
propose unmet success signals, stage modifications, new stages, skill
suggestions, and todo updates without directly mutating project truth.
ask_user/HITL should map to existing `ResearchTurnClassification`
`human_gate_required` and pending operation records, so clarification and
approval can resume a specific stage edge instead of becoming unstructured chat
state.
- [x] Require M10 governed `supersede` outputs to emit or refresh a `DocFrame`
  compatible with the project document index; the current implemented path is
  research-runtime supersede emitting
  `docs/research/runtime/latest.md` with a parseable DocFrame. General
  skill-execution hooks remain outside the M10 baseline.

Current implementation note: M10 now persists `ChangeEnvelope` records for
mutating stage edges, emits fork debate packets, exposes schema-backed
`research stage map --json`, records `supersede` canonical-surface refresh
markers, records `abandon` failure lessons with no active public artifact refs,
and makes skill availability machine-readable through inspect/validate outputs.
Stage transitions now validate against `StageExecutionMap`; `supersede` refreshes
`.pmcli/canonical_surface.json`, updates the
`research_stage_outputs` artifact-family latest pointer, and writes the active
research-runtime DocFrame.

**Done when:**
- research workflow runs as a governed native subsystem

### Post-M10 / M11: General Skill Output Publication Runtime

The most general design is not to let each skill write public documents or
metadata directly. Every skill output should first enter the runtime as a
schema-owned `SkillOutputEnvelope`, then pass through a publication gate that
decides whether the output stays private, becomes a review candidate, or
supersedes the one public canonical surface.

Core principle:

- one public latest artifact per governed family
- many private or review candidates may exist under `.pmcli`
- prompt/resume/compact consume only canonical outputs unless the operator
  explicitly selects a candidate
- every public transition is reversible, traceable, and tied to a
  `ChangeEnvelope` or equivalent decision record
- skill execution and skill publication are separate layers; a skill may keep
  its native directory, cache, and script conventions, but only governed
  `SkillOutputEnvelope` records can become project truth

Universal four-layer model:

1. `SkillContract`: declares what the skill may produce, where it may write,
   whether DocFrames are required, publication policy, human-gate policy,
   mutation scope, and rollback expectations.
2. `SkillRunnerAdapter`: runs or wraps the skill without rewriting its internal
   conventions, then records candidate side effects and final artifact refs.
3. `SkillOutputEnvelope`: normalizes one or more skill artifacts into typed
   refs, including docs, patches, datasets, figures, external URIs, logs, and
   experiment outputs.
4. `PublicationTransaction`: applies policy, review/human gates, path scope,
   DocFrame validation, artifact-family latest update, canonical-surface update,
   and supersession in one auditable transaction.

Planned schemas:

- [x] `skill_output_envelope.schema.json`
- [x] `skill_output_artifact.schema.json`
- [x] `skill_output_list_result.schema.json`
- [x] `skill_output_inspection_result.schema.json`
- [x] `skill_output_submit_result.schema.json`
- [x] `skill_publication_gate.schema.json`
- [x] `skill_output_publish_result.schema.json`
- [x] `skill_output_context_projection.schema.json`
- [x] `skill_run_result.schema.json`
- [x] `skill_docframe_publication.schema.json`

Planned `SkillOutputEnvelope` fields:

```text
SkillOutputEnvelope {
  schema_version
  envelope_id
  skill_id
  skill_version
  thread_id?
  stage_execution_id?
  operation?
  output_kind
  visibility
  artifact_refs[]
  doc_frame_candidate?
  canonicality_policy
  human_gate_required
  review_packet_ref?
  change_envelope_ref?
  supersedes[]
  created_at
}
```

Required `output_kind` values should cover at least `plan`, `review`,
`report`, `handoff`, `experiment_result`, `patch`, `dataset`, `figure`, and
`paper_section`.

Publication flow:

1. discover the skill through `SkillManifest` and `ResearchSkillContract`
2. submit external/manual/runtime-adapter output through `skills output submit`
   as a review candidate; arbitrary in-process skill execution remains a later
   adapter layer, not a prerequisite for publication governance
3. run the skill through a runtime adapter that writes only private candidate
   artifacts and one `SkillOutputEnvelope`
4. validate schema, stage compatibility, path scope, artifact refs, and
   `DocFrame` candidate
5. apply the publication policy:
   `private_only`, `candidate_only`, `requires_review`, or
   `winner_only_public_surface`
6. if promotion is allowed, update the artifact-family latest pointer,
   `.pmcli/canonical_surface.json`, and the document index in one governed
   transaction
7. mark superseded public docs as non-current without deleting their evidence

Required operator surfaces:

- [x] `skills outputs list --json`
- [x] `skills output submit --skill <id> --kind <kind> --artifact <path>
  [--artifact <path> ...] [--artifact-kind <kind> ...] --family <family>
  --doc-frame <path> --policy <policy> --human-gate --json`
- [x] `skills output inspect <envelope-id> --json`
- [x] `skills publish --inspect <envelope-id> --json`
- [x] `skills publish --execute <envelope-id> --approve-human-gate --json`
- [x] `skills run --skill <id> --adapter local-command --command <cmd>
  --kind <kind> --artifact <path> --family <family> --doc-frame <path>
  --policy <policy> --human-gate --json`
- [x] `docs publish-candidate <doc-ref> --json`

Required proof gates:

- [x] skill output publication goes through an envelope-backed submit/inspect/
  publish route instead of a direct public write route
- [x] malformed or missing `DocFrame` candidates are rejected before publication
- [x] two skill outputs cannot both be public-latest for the same artifact
  family
- [x] superseding an older skill output updates the canonical surface and keeps
  the old evidence addressable
- [x] `requires_review` outputs with `human_gate_required=true` cannot publish
  without an explicit `--approve-human-gate` decision
- [x] `private_only` and `candidate_only` policies cannot be promoted to
  public-latest by `skills publish --execute`
- [x] one envelope can carry multiple artifact refs so skill output is not
  forced into a single-file shape
- [x] prompt/resume/compact ignore private candidates by default
- [x] human gates are required for public docs, code patches, experiment
  result claims, and paper-section publication unless the contract explicitly
  proves the operation is safe
- [x] local command runner adapters keep the skill's native directory and
  command conventions, but audit side effects and reject outputs outside the
  contract's allowed write scopes before submitting a governed envelope

Post-M11 EvoScientist tool/MCP refinement:

- [ ] Add role-scoped MCP/tool exposure with exact and wildcard allowlists,
  matching EvoScientist's `expose_to` ergonomics while preserving research-cli
  task-packet and skill-contract authority.
- [ ] Add `ToolSelectionTrace` for adaptive tool selection: requested role,
  available tools, selected tools, threshold, fallback mode, and evidence refs.
  Selection may reduce tool exposure, but cannot grant capabilities outside the
  task packet, MCP route, or skill contract.
- [ ] Keep skill installation and browsing ergonomics on the backlog, but do
  not let installed skills publish public docs/code except through
  `SkillOutputEnvelope` and publication gates.

Post-M11 ModelScope ULTRON collective-intelligence refinement:

- [ ] Add `TrajectoryIngestRecord` and `TaskSegmentRecord` for session `.jsonl`
  import, LLM task segmentation, content fingerprint idempotency, segment
  quality metrics, and segment-tagged memory extraction/invalidation.
- [ ] Add durable memory adoption/tier signals inspired by ULTRON HOT/WARM/COLD:
  retrieval, inspect/details, merge, injection, citation, supersession, and
  invalidation. Tiering may change ranking, but cannot bypass trust,
  canonicality, or artifact-family latest gates.
- [ ] Add `KnowledgeClusterRecord` so repeated related memories can accumulate
  into candidate reusable workflows without silently becoming trusted skills.
- [ ] Add `SkillEvolutionRecord` for memory-cluster crystallization and
  re-crystallization. Evolved skills must enter as M11 governed candidates, not
  direct public-latest outputs.
- [ ] Add provenance-grounded skill verification: every generated step/claim is
  labeled grounded, hallucinated, or contradicted against source memories.
  Contradicted skills and skills below the grounded-evidence threshold cannot
  publish.
- [ ] Add a structure-score upgrade gate for re-crystallized skills so new
  versions replace old versions only when workflow clarity, specificity, and
  preservation of existing value improve.
- [ ] Add governed harness/profile export and import for agent blueprints:
  allowlisted files only, secret/session/log/hidden-file exclusion,
  backup-before-apply, dry-run diff, product-specific merge rules, and
  canonical-surface awareness.
- [ ] Keep supervised-training export out of the product roadmap; quality-approved
  trajectory segments feed governed memory, skill evolution, and harness/profile
  learning only.

New milestone sequencing after the latest reference pass:

- M12 `Verifier Tournament And Decision Quality`: implement
  `VerifierCriterion`, `VerifierPairScore`, and `VerifierTournament` for
  ambiguous M9 winners, high-impact M10 pivots/supersedes, and research-claim
  validation. Verifier preference remains advisory until deterministic eval,
  canonicality, and publication/merge gates pass.
- M13 `Research UX Middleware Runtime`: implement one unified middleware loop:
  `ResearchRoleTemplate -> ToolSelectionTrace -> StageReflectionPacket ->
  HitlPendingGate / ToolErrorRecovery`. Role templates define the research role,
  intent boundary, authority ceiling, and default always-include capabilities.
  Tool selection narrows candidate tools and MCP/skill exposure while recording
  selected, filtered, and fallback decisions. Stage reflection turns planner
  self-review into typed completed work, unmet success signals, stage changes,
  skill suggestions, new-stage candidates, todo updates, and next action. HITL
  gates distinguish clarification, approval, and recovery decisions and become
  pending runtime records instead of opaque chat pauses. Tool errors become
  structured recovery packets with retryability, alternatives, and escalation.
  The middleware is advisory: it can feed M10 pending operations and future M14
  projections, but it cannot publish canonical docs, merge code, promote
  branches, or widen tool authority without existing governance gates.
- M14 `Local TUI And Governed Remote Full-CLI Host Surfaces`: implement the
  missing full local TUI together with mobile/web remote control. This milestone
  owns local TUI panes, semantic app-server control, governed PTY/xterm parity,
  Tailscale helpers, watch-only/control-mode separation, permission queues,
  terminal leases, reconnect/replay/revocation fixtures, and shared host-surface
  projections.
- M15 `Trajectory Ingest And Collective Memory`: implement the design in
  `docs/deep_study/54-m15-trajectory-collective-memory-design.md`: session
  `.jsonl` ingest, replayable task segmentation, segment fingerprints, quality
  metrics, segment-tagged memory candidate extraction/invalidation, adoption
  events, and explainable HOT/WARM/COLD memory tiers. M15 may queue memory
  candidates, but only the existing M8 promotion path may create durable trusted
  injectable memory.
- M16 `Skill Evolution And Skill Catalog`: implement memory-cluster skill
  candidates, crystallization, provenance verification, structure-score upgrade
  gates, and publication only through `SkillOutputEnvelope`. The current release
  gate is the deterministic local baseline in
  `docs/deep_study/55-m16-m18-skill-evolution-feedback-design.md`; optional
  external catalog/install ergonomics remain product polish after the governed
  candidate path is stable. Verified evolved candidates must also improve the
  existing CLI context plane through `skill_outputs.evolved_recommendations`
  in prompt/resume/compact; they must not become a separate side catalog.
- M17 `Harness Profile And Agent Blueprint Sharing`: deferred by operator
  decision. It remains useful, but it imports/exports wider environment
  conventions and needs its own allowlist, secret/session/log/hidden-file
  exclusion, backup-before-apply, dry-run diff, product-specific merge, and
  canonical-surface review.
- M18 `Agent Evolution Feedback Loop`: implement privacy-gated,
  quality-approved trajectory feedback for retrieval, routing, skill evolution,
  and verifier calibration as advisory projections only. M18 must not create a
  supervised-training export path and must not bypass memory, branch, skill, or
  tool-authority gates. The first integration target is the existing
  `memory query` route: latest advisory calibration may add an explainable
  `feedback_calibration` lane and `feedback_adjustments`, but no feedback
  signal can create durable memory or project truth.

### M15 Detailed Contract: Trajectory Ingest And Collective Memory

Reference basis:

- ModelScope ULTRON `TrajectoryHub` proves the segment-level shape: capture
  session `.jsonl`, split long sessions into task segments, compute content
  fingerprints, label segment quality, extract memories only from eligible
  segments, and tag memories by segment for later invalidation.
- ModelScope ULTRON `MemoryHub` proves the adoption/tier shape: near-duplicate
  memory merge, explicit HOT/WARM/COLD tiering, retrieval boost/decay, and
  memory clustering as later skill-evolution material.
- Research-cli must exceed ULTRON by binding the same loop to `.pmcli`, Git
  lineage, canonicality, artifact-family latest gates, M7/M8 memory explain,
  and explicit invalidation. Raw trajectories are evidence, not project truth.

M15 records to implement:

- [x] `TrajectoryIngestRecord` under `.pmcli/trajectory/ingests/` and
  `.pmcli/trajectory/ingests.jsonl`, with source kind, source session, source
  digest, status, degraded reasons, and segment count.
- [x] `TaskSegmentRecord` under `.pmcli/trajectory/segments/`, with start/end
  lines, segment index, normalized message count, fingerprint, topic,
  segmentation method/model/prompt digest, quality status, extraction status,
  segment tag, and supersession fields.
- [x] `SegmentQualityRecord` under `.pmcli/trajectory/quality/`, with named
  criteria, overall score, memory eligibility, failure reason, and evidence refs.
- [x] `SegmentMemoryCandidate` through the existing
  `.pmcli/memory/promotion_queue/` path, requiring `segment_id`, `segment_tag`,
  `quality_id`, support refs, and source artifacts before M8 promotion can
  accept it.
- [x] `MemoryAdoptionEvent` append log under
  `.pmcli/memory/adoption_events.jsonl` for retrieved, inspected, injected,
  cited-in-output, merged, superseded, invalidated, and rejected signals.
- [x] `MemoryTierExplanation` projection under `.pmcli/memory/tiers/`,
  explaining HOT/WARM/COLD from adoption score and governance limits.

M15 command surface:

- [x] `memory trajectory ingest <path> --source-kind <kind> --json`
- [x] `memory trajectory segment <ingest-id> --json`
- [x] `memory trajectory label <segment-id> --json`
- [x] `memory trajectory extract <segment-id> --json`
- [x] `memory trajectory status --json`
- [x] `memory trajectory explain <segment-id> --json`
- [x] `memory adoption record <memory-id> --event <kind> --json`
- [x] `memory tier rebalance --json`
- [x] `memory tier explain <memory-id> --json`

M15 first experiment:

- [x] Build a deterministic `.jsonl` fixture with two independent task spans,
  one changed-span re-ingest, one low-quality span, and one privacy-risk span.
- [x] Prove idempotent ingest: unchanged fingerprint skips, changed fingerprint
  supersedes the old segment and invalidates old candidates/promoted memories.
- [x] Prove quality gates: unsupported, private, and low-scoring segments do not
  create memory candidates.
- [x] Prove governance: trajectory extraction can only create candidates; M8
  promotion remains the only durable-memory trust path.
- [x] Prove tier safety: adoption events can move a memory toward HOT in
  explanation/ranking, but invalidated/superseded/contested/expired/non-latest
  memory remains non-injectable.

M15 done when:

- the schemas above validate through conformance tests
- operator commands persist and inspect all records
- fixture tests cover ingest, segmentation, quality gating, extraction,
  promotion handoff, tier explanation, re-ingest supersession, and invalidation
- `memory query` / `memory explain` show trajectory-derived provenance for
  promoted records
- the implementation can honestly claim: "we match ULTRON's trajectory-to-memory
  loop and exceed it in project-local governance, canonicality, and invalidation"

2026-04-29 implementation completion: the branch now has the deterministic M15
foundation implemented and fixture-proven for the full M15 release gate:

- [x] Six M15 schemas exist and compile:
  `trajectory_ingest_record`, `task_segment_record`,
  `segment_quality_record`, `segment_memory_candidate`,
  `memory_adoption_event`, and `memory_tier_explanation`.
- [x] `src/trajectory/mod.rs` implements deterministic `.jsonl` ingest,
  marker/short/single-task segmentation, ULTRON-style segment fingerprints,
  deterministic quality labeling, candidate extraction into the existing M8
  promotion queue, explicit adoption events, and HOT/WARM/COLD tier
  explanations.
- [x] `memory trajectory ingest/segment/label/extract/status/explain`,
  `memory adoption record`, and `memory tier rebalance/explain` are wired
  through the CLI.
- [x] Conformance proves a local end-to-end experiment:
  two-task `.jsonl` -> two task segments -> quality label -> M15 memory
  candidate -> existing M8 promotion -> adoption events -> HOT tier explanation.
- [x] Conformance proves changed-span re-ingest supersession: unchanged segment
  fingerprints are reused, changed segment fingerprints create a new segment
  that supersedes the old segment, and old promoted memory is invalidated
  through the existing M8 path.
- [x] Conformance proves negative quality gates: unsupported segments and
  privacy-risk segments are marked ineligible and cannot create memory
  candidates.

Graduation boundary:

- [x] M15 is graduated for deterministic, replayable, project-local trajectory
  ingest and collective memory.
- [ ] Optional LLM segmentation for broad unmarked transcripts remains a future
  enhancement. The M15 schema already preserves segmentation method/model/prompt
  digest fields, but the release gate intentionally uses deterministic
  marker/short/single-task segmentation for reproducible tests.
- [x] M16/M18 implementation target is now narrowed and more elegant:
  memory-to-skill crystallization and advisory feedback calibration are in
  scope; M17 harness sharing is deferred; supervised-training export is not a
  roadmap requirement.
- [ ] Full superiority over all of ModelScope ULTRON still requires future M17
  harness/profile sharing. M15/M16/M18 can claim superiority over ULTRON's
  trajectory-to-memory, memory-to-skill, and feedback-calibration layers in
  governance, invalidation, M11 publication discipline, and project-local
  canonicality.

### M16/M18 Detailed Contract: Skill Evolution And Feedback

The detailed design lives in
`docs/deep_study/55-m16-m18-skill-evolution-feedback-design.md`.

Release-gate records:

- [x] `KnowledgeClusterRecord`
- [x] `SkillEvolutionCandidate`
- [x] `SkillProvenanceVerification`
- [x] `SkillStructureScore`
- [x] `SkillInstallRecord`
- [x] `AgentFeedbackSignal`
- [x] `FeedbackCalibrationRecord`

Release-gate commands:

- [x] `skills evolve cluster --min-members <n> --json`
- [x] `skills evolve crystallize <cluster-id> --json`
- [x] `skills evolve verify <candidate-id> --json`
- [x] `skills evolve submit <candidate-id> --json`
- [x] `skills evolve install <candidate-id> --approve-human-gate --json`
- [x] `feedback collect --json`
- [x] `feedback calibrate --json`
- [x] `feedback status --json`

M16/M18 governance:

- [x] evolved skills enter as M11 review-candidate envelopes, never direct
  public-latest outputs
- [x] clustering separates distinct workflow topics before crystallization, so
  unrelated memories do not collapse into a single low-signal evolved skill.
  The implementation uses deterministic weighted token-overlap connected
  components so word-order changes stay clustered while unrelated workflows
  split before crystallization.
- [x] verified evolved skills are projected into existing prompt/resume/compact
  `skill_outputs.evolved_recommendations` as advisory recommendations only;
  unverified candidates stay hidden from the default context plane
- [x] verified evolved skills can be installed into the existing local
  `.codex/skills` registry only with explicit human approval, after which
  `skills list` and `skills inspect` see them as normal skills. Install is
  fail-closed: candidate artifacts must be workspace-relative files that
  canonicalize inside the workspace, and an existing `SKILL.md` is never
  overwritten implicitly.
- [x] feedback records are `advisory_only` and cannot publish project truth,
  merge branches, widen tools, or bypass skill gates
- [x] feedback calibration enters existing `memory query` through an explicit,
  explainable `feedback_calibration` lane with per-record
  `feedback_adjustments`; it does not bypass M8 promotion or injectability
  gates
- [x] M17 is explicitly deferred to a future harness/profile sharing milestone

Current M12 implementation note: the branch-promotion slice is implemented as
`branches verify <branch-id> --against <branch-id>`. It persists
`VerifierTournament` records under `.pmcli/branches/verifier_tournaments/`,
decomposes scoring into deterministic eval, canonicality, reviewer-debate, and
implementation-risk criteria, emits repeated `VerifierPairScore` records with a
shared evidence digest, projects the tournament through `branches inspect`, and
adds `branches promote --require-verifier` so a promoted winner must match the
latest tournament recommendation. This is intentionally a local deterministic
baseline: it absorbs the LLM-as-a-Verifier control shape while keeping verifier
preference advisory until M9 eval/canonicality/merge gates pass. Remaining M12
work is wiring the same schema family to M10 high-impact research decisions and
adding optional LLM/logprob verifier providers.

Current implementation note: the implemented M11 slices are a publication
runtime, not a general arbitrary skill runner. They add `SkillOutputEnvelope`
persistence under `.pmcli/skills/outputs/`, validate workspace-relative
artifacts and DocFrame candidates, publish through `skills publish --execute`,
update `.pmcli/artifacts/families/<family>.json`, and write the `skill_outputs`
section of `.pmcli/canonical_surface.json` while preserving unrelated canonical
surface fields. The later slices add multi-artifact envelopes, publication-gate
enforcement, contract-aware submit validation, native
`docs publish-candidate`, and prompt/resume/compact
`SkillOutputContextProjection` injection that exposes only public-latest skill
outputs by default while private/review candidates remain addressable through
explicit `skills output inspect`. The current runner-adapter slice adds
`skills run --adapter local-command`: it executes an operator-provided command
inside an isolated workspace copy, exposes the skill directory through
`RESEARCH_CLI_SKILL_DIR`, initializes an independent git baseline for the
isolated copy, audits staged/unstaged/untracked side effects, enforces contract
`allowed_write_scopes`, rejects traversal and symlink escape paths, validates
active research-stage compatibility before execution/submission, copies back
only declared governed artifacts, and then submits them through the same
governed envelope path. Public promotion now also refreshes
`.pmcli/docs/index.json` alongside artifact-family latest and canonical-surface
updates. Remaining work is richer non-local adapter families; the generic local
adapter already supports arbitrary local skill commands that can run without
forcing the skill to adopt the runtime's internal directory conventions.

## 4. Exit Criteria

The M3/M4 completion slice is complete when:

- artifact/review governance is live
- canonicality gates protect cleanup and promotion
- reversible cleanup is pre-snapshotted and fixture-backed
- project inspection reports M3/M4 surfaces as available/guarded instead of
  partial/not-graduated
- base CLI parity commands remain fixture-backed

The larger advanced-systems plan remains open for M5+ when:

- remote plane is projection-driven and harness-backed
- multi-agent runtime is packet-bound and review-safe
- memory/ProjectOps is useful and bounded beyond the M3/M4 cleanup baseline,
  including governed memory retrieval and a real supervision lease/wake state
  machine
- branch/debate runtime is executable and fixture-backed; research runtime remains M10
- post-M11 collective-intelligence loops from ModelScope ULTRON are either
  implemented with proof gates or explicitly labeled future work before any
  claim that we exceed ULTRON's shared memory/skill/harness/trajectory system

## 5. Hard Stop Conditions

Stop and fix the lower layer first if any of these happens:

- remote code starts storing shadow runtime state outside `.pmcli/remote/`
- agents run without validated task packets
- branch promotion bypasses evaluation/review
- memory auto-promotion bypasses candidate/promotion separation
- research stage routing bypasses `StageExecutionMap`

Those are architectural regressions, not implementation details.
