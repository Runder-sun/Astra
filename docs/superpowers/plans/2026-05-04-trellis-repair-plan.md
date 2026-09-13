# Trellis Repair Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repair the runtime, TUI, mobile, and bridge seams exposed by the current review so the stack behaves like one coherent agentic coding product instead of adjacent surfaces with partial overlap.

**Architecture:** Treat `src/runtime/mod.rs` as the execution core, `src/host_surface.rs` as the projection contract, `src/tui.rs` as the local conversational surface, and `src/assets/mobile/app.js` as the remote client. Fix the seams in that order: transcript/replay correctness first, then host control and projection fidelity, then TUI command semantics, then mobile parity. Keep every change anchored to an existing contract or test so we do not invent a second product model.

**Tech Stack:** Rust runtime/daemon/TUI, static mobile JS shell, conformance tests under `tests/conformance`, operator tests under `tests/operator`, git worktrees, `cargo test`.

---

## Current Findings

The review exposed four classes of problems:

- Runtime is capable but not yet fully coherent as an agent loop. The code explicitly says the current TUI prompt path is a compatibility bridge, not the mature loop (`src/runtime/mod.rs:11750-11753`). Prompt turns still append transcript lines in a simple user/assistant order after completion (`src/runtime/mod.rs:524-560`, `src/runtime/mod.rs:8073-8090`), while `transcript_to_messages` reconstructs tool turns by transcript order and tool-call pairing (`src/session/context.rs:16-121`). That makes replay/resume fragile once tool calls are present.
- Host/terminal projection is still projection-first. `tool_activity` is present only as a projected pane, but not materialized (`src/host_surface.rs:1015-1021`). `interrupt_turn` is wired to a remote notification rather than a direct cancellation path (`src/remote/daemon.rs:1863-1885`). Terminal attach/replay are persisted as projection state and a one-time ticket is required for websocket access (`src/host_surface.rs:566-603`, `src/remote/daemon.rs:2051-2060`). This is acceptable as a bridge, but not yet a complete control path.
- TUI shows more commands than it actually executes. It still carries placeholder/copy-driven affordances around resume/continue and command guidance (`src/tui.rs:1459`, `src/tui.rs:1542`, `src/tui.rs:3528-3529`, `src/tui.rs:4017`, `src/tui.rs:4064-4068`, `src/tui.rs:4280-4341`). The visible model and the real runtime commands are close, but not consistently named or routed.
- Mobile is close in layout but not fully aligned with backend contracts. It reads `projection.runtime_descriptor.permission_mode` and falls back to `projection.permission_mode` (`src/assets/mobile/app.js:1612-1614`), replays terminal data from REST and then opens a websocket bridge (`src/assets/mobile/app.js:1117-1175`), and renders transcript/permissions from the projection plus transcript endpoint (`src/assets/mobile/app.js:1616-1662`). That is workable, but the projection shape and terminal control flow need a tighter contract to avoid drift.

## Repair Order

1. Fix runtime transcript/replay correctness and add an end-to-end regression test.
2. Tighten host/daemon control semantics for terminal, permission, and interrupt flows.
3. Clean up TUI command semantics so visible commands map to real runtime behavior.
4. Align mobile projection consumption and terminal/session handling with the backend contract.

This order matters because every later surface depends on the runtime transcript and projection shape being trustworthy first.

## Progress Update 2026-05-05

- Runtime transcript/replay first slice is implemented and covered by focused conformance tests.
- Mobile static asset and daemon bridge contract first slice is implemented. The mobile client now stays on existing daemon/runtime APIs rather than inventing a second event source.
- TUI command semantics first slice is implemented: `/continue` now routes through the same session resume executor as `/resume latest` instead of presenting a staged placeholder.
- TUI permissions inspector slice is implemented: `/permissions` now renders live pending count and permission mode without staged placeholder copy; `/approve` and `/deny` remain executor-backed.
- TUI terminal contract slice is implemented: `/terminal` and `/terminal replay` now describe the governed remote projection, control lease, websocket ticket, and read-only cursor replay contract instead of generic staged copy.
- TUI research brief slice is implemented: `/research` now renders the live research summary line without staged placeholder copy, while backend research context conformance remains green.
- Host/daemon interrupt contract slice is implemented: `/api/tui/action interrupt_turn` now emits an explicit `remote_interrupt_requested` control-phase event and annotates the response with the `notification_backed_interrupt_request` contract while still using the remote notification mechanism under the hood.
- `cargo test remote_daemon --test conformance -- --nocapture` passed with 28 tests.
- Full `cargo test` is now green again after synchronizing the `SessionRuntimeDescriptor` schema with the current runtime projection fields (`model`, `reasoning_effort`, `permission_mode`, `branch`, `workspace_root`).
- Remaining deeper work is a direct host interrupt/cancellation implementation and browser-level mobile UX review; the documented bridge contract is green.

## Task 1: Fix Runtime Transcript Ordering and Replay Fidelity

**Files:**
- Modify: `src/runtime/mod.rs`
- Modify: `src/session/context.rs`
- Modify: `tests/conformance/remote_daemon.rs`
- Modify: `tests/conformance/runtime/session_store.rs` or a nearby runtime conformance test file if a better home exists

- [x] **Step 1: Write a failing replay/resume regression test**

Add a test that creates a session transcript containing:

1. user message
2. assistant message with tool call(s)
3. tool result(s)
4. assistant follow-up

Then verify that `transcript_to_messages` rebuilds the expected message order for replay/resume, including `tool_call_id` preservation.

- [x] **Step 2: Run the test and confirm the current behavior fails or is incomplete**

Run:

```bash
cargo test transcript_to_messages -- --nocapture
cargo test remote_daemon --test conformance -- --nocapture
```

Expected: the new replay test should fail before the fix or demonstrate a mismatch in message ordering / tool metadata.

- [x] **Step 3: Adjust the transcript write path to preserve turn structure**

Keep transcript append order and replay reconstruction consistent. If a turn includes tool use, ensure the stored transcript can round-trip through `transcript_to_messages` without reordering tool events around the wrong assistant/user boundary.

- [x] **Step 4: Run the focused tests again**

Run:

```bash
cargo test transcript_to_messages -- --nocapture
cargo test remote_daemon --test conformance -- --nocapture
```

Expected: PASS.

## Task 2: Tighten Host and Daemon Control Semantics

**Files:**
- Modify: `src/host_surface.rs`
- Modify: `src/remote/daemon.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `tests/conformance/remote_daemon.rs`

- [x] **Step 1: Write tests for the real control path**

Cover:

- terminal attach and replay still write a usable projection
- interrupt requests are surfaced as a deliberate control event, not just a generic remote notification
- permission requests and approvals continue through the existing session/turn state machine

- [x] **Step 2: Run the tests and confirm the current contract gap**

Run:

```bash
cargo test remote_daemon --test conformance -- --nocapture
```

- [x] **Step 3: Normalize the daemon control path**

Keep `terminal_attach` / `terminal_replay` as the projection-backed terminal contract, but make the exposed control semantics explicit and coherent. If interrupt remains notification-based for now, record that in the contract and test it directly instead of letting it look like a direct cancellation hook.

- [x] **Step 4: Re-run the conformance tests**

Run:

```bash
cargo test remote_daemon --test conformance -- --nocapture
```

Expected: PASS. On 2026-05-05 the focused interrupt/event-log tests passed and the broader `remote_daemon` slice remained green.

## Task 3: Align TUI Commands With Real Runtime Actions

**Files:**
- Modify: `src/tui.rs`
- Modify: `src/surface_commands.rs` if needed
- Modify: `src/commands/help.rs` if the command registry needs normalization
- Modify: `tests/operator/cli_surfaces.rs`

- [x] **Step 1: Add tests for visible command routing**
- [x] **Step 1a: Add `/continue` routing regression**

Added a focused TUI test that verifies `/continue` invokes `TuiSessionActionKind::Resume` with `latest`, updates active session state, and no longer renders staged placeholder copy.

- [x] **Step 1b: Implement `/continue` as `/resume latest` alias**

`continue_session` now reuses the existing resume renderer/executor path. This matches the CLI `continue --json` behavior covered by operator conformance tests.

- [x] **Step 1c: Make `/permissions` a live inspector**

Added a focused test that verifies `/permissions` reports the TUI's pending approval count and permission mode without invoking executors and without staged placeholder copy. `/approve` and `/deny` were re-run to verify their executor-backed behavior remains unchanged.

- [x] **Step 1d: Align `/terminal` and `/terminal replay` with the daemon contract**

Added a focused TUI test requiring terminal commands to state the current governed remote projection contract: active control lease, one-time websocket ticket for the bridge, and cursor-based read-only replay. The existing operator conformance test for the terminal lane remains green.

- [x] **Step 1e: Make `/research` a live brief**

Added a focused TUI test requiring `/research` to show the current `research_line` as a live brief without staged placeholder copy. Research runtime and middleware conformance tests were re-run to keep the TUI view aligned with backend context behavior.

- [x] **Step 1f: Audit remaining staged projected commands**

Remaining staged responses are currently limited to CLI-only projected entries without a bound TUI executor in this slice: `/diff`, `/commit`, `/cost`, `/usage`, `/doctor`, `/providers`, `/config`, `/tools`, `/mcp`, `/artifacts`, and `/memory`. Keeping explicit staged copy plus canonical CLI command names is the accurate behavior for this pass; converting them to live inspectors should be handled one-by-one only when their data contracts are bound into `TuiInteractionState` or a TUI executor.

Cover the current user-visible commands that are still inconsistent:

- any placeholder response that should instead project a real runtime action

- [x] **Step 2: Run the command-surface tests and confirm gaps**

Run:

```bash
cargo test --lib "tui::tests::" -- --nocapture
cargo test --test conformance continue_resumes_latest_session_in_scope -- --nocapture
cargo test --test conformance tailscale_helpers_and_terminal_lane_are_governed_projection_surfaces -- --nocapture
```

- [x] **Step 3: Replace placeholder copy with projected actions or explicit help**

Make sure every visible command either:

- executes a real runtime action,
- opens a supported inspector or projection, or
- clearly says it is only a staged command and points to the real command name.

Do not leave a command looking executable when it is not.

- [x] **Step 4: Re-run the TUI surface tests**

Run:

```bash
cargo test --lib "tui::tests::" -- --nocapture
```

Expected: PASS. On 2026-05-05 this passed with 91 TUI tests.

## Task 4: Align Mobile Projection and Terminal Behavior

**Files:**
- Modify: `src/assets/mobile/app.js`
- Modify: `src/assets/mobile/index.html` if the control surface needs contract changes
- Modify: `src/remote/daemon.rs` if a server-side contract needs to be tightened
- Modify: `tests/conformance/remote_daemon.rs`

- [x] **Step 1: Add tests for projection shape and terminal behavior**

Cover:

- permission mode and session metadata read from the canonical projection shape
- terminal attach/replay are consistent with the websocket ticket flow
- slash and skill controls execute projected actions instead of inserting plain text

- [x] **Step 2: Run the tests and confirm the current mismatch**

Run:

```bash
cargo test remote_daemon --test conformance -- --nocapture
```

- [x] **Step 3: Make the mobile client consume one canonical contract**

Stop falling back across multiple projection shapes unless the fallback is intentionally part of the contract. Keep websocket ticket auth, terminal replay, and session switching consistent with the backend surface.

- [x] **Step 4: Re-run the mobile-facing conformance coverage**

Run:

```bash
cargo test remote_daemon --test conformance -- --nocapture
```

Expected: PASS.

## Task 5: Baseline Verification

**Files:**
- None; this is verification only

- [x] **Step 1: Run the focused test matrix**

Run:

```bash
cargo test
```

Focused matrix run in this repair slice:

- `cargo test --test conformance remote_daemon -- --nocapture` passed with 28 tests.
- `cargo test --lib "tui::tests::" -- --nocapture` passed with 91 tests.
- `cargo test --test conformance control_phase_event_is_valid_without_terminal_outcome -- --nocapture` passed.
- `cargo test --test conformance remote_status_and_pair_payloads_validate_against_remote_schemas -- --nocapture` passed.
- `cargo test` passed.
- `git diff --check` passed.

- [x] **Step 2: Fix any remaining failures before merge**

The full compiler/test matrix and targeted conformance/operator tests are green.

## Notes

- This plan intentionally starts with the runtime transcript and replay path because that is the deepest shared seam.
- Keep each fix small. If a step reveals a deeper architectural mismatch, stop and write down the mismatch before widening the change.
- Avoid parallel feature changes while these seams are being repaired.
- Verification for the `/continue` TUI slice:
  - `cargo test continue_invokes_session_executor_as_resume_latest_alias --lib -- --nocapture`
  - `cargo test resume_latest_invokes_session_executor_and_updates_active_state --lib -- --nocapture`
  - `cargo test tui::tests::tui_projected_actions_are_explicitly_staged_not_fake_executed --lib -- --nocapture`
  - `cargo test --test conformance continue_resumes_latest_session_in_scope -- --nocapture`
  - `cargo test --test conformance bare_continue_flag_resumes_latest_session_in_scope -- --nocapture`
  - `cargo test --test conformance goals_mission_frame_persists_and_survives_prompt_resume_continue_and_compact -- --nocapture`
  - `cargo test --test conformance sessions_create_list_and_resume_latest_are_project_scoped -- --nocapture`
- Verification for the `/permissions` TUI slice:
  - `cargo test permissions_inspect_reports_live_state_without_staged_copy --lib -- --nocapture`
  - `cargo test approve_permission_command_executes_permission_action_and_updates_state --lib -- --nocapture`
  - `cargo test deny_permission_command_executes_permission_action_and_updates_state --lib -- --nocapture`
  - `cargo test tui::tests::tui_projected_actions_are_explicitly_staged_not_fake_executed --lib -- --nocapture`
- Verification for the `/terminal` TUI slice:
  - `cargo test terminal_commands_explain_governed_remote_projection_contract --lib -- --nocapture`
  - `cargo test tui_input_accepts_text_and_submits_without_control_key_only_paths --lib -- --nocapture`
  - `cargo test tui::tests::tui_projected_actions_are_explicitly_staged_not_fake_executed --lib -- --nocapture`
  - `cargo test --test conformance tailscale_helpers_and_terminal_lane_are_governed_projection_surfaces -- --nocapture`
- Verification for the `/research` TUI slice:
  - `cargo test research_command_renders_live_brief_without_staged_copy --lib -- --nocapture`
  - `cargo test tui::tests::tui_projected_actions_are_explicitly_staged_not_fake_executed --lib -- --nocapture`
  - `cargo test --test conformance research_runtime_tracks_thread_stage_deliberation_decision_and_record -- --nocapture`
  - `cargo test --test conformance m13_research_middleware_records_role_tool_reflection_and_hitl_gate -- --nocapture`
