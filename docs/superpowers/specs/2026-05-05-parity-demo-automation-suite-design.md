# Parity Demo Automation Suite Design

Date: 2026-05-05

## Overview

This suite turns the Claude/Hermes parity question into runnable evidence. It
does not try to prove parity by prose or by individual unit tests. It runs a
small set of product demos, records the evidence in a stable JSON manifest, and
marks known gaps explicitly.

The initial scope contains five demos:

1. runtime turn control
2. local TUI inline REPL
3. remote/mobile handoff
4. host contract consistency
5. routines/background automation gap

The first four are expected to pass once the current runtime/TUI/mobile work is
properly connected into the harness. The fifth is expected to report `gap` until
the system implements a Hermes/Claude-routines class automation lane.

## Product References

The suite uses the local source-grounded Claw/Claude UX study for terminal
behavior and the Hermes release/reference repo for background automation
expectations.

Reference behaviors:

- Claude/Claw-style code CLI: chat-first transcript, serious composer, slash
  command registry, permission gates, status/session/resume, streaming output,
  and interrupt semantics.
- Hermes-style remote automation: cron-like schedules, webhook/API triggers,
  model/tool context injection, background process monitoring, and delivery
  targets.
- research-cli architecture rule: TUI, web, and mobile are projections and
  control clients over kernel-owned runtime truth. They must not create a
  second event model.

## Harness Contract

Add a dedicated script:

```bash
scripts/run_parity_demo_suite.sh
```

Default manifest path:

```text
tests/parity_demo/last_run_manifest.json
```

The manifest must be deterministic and parseable:

```json
{
  "harness": "parity_demo_suite",
  "schema_version": "1",
  "status": "partial",
  "generated_at": "timestamp",
  "summary": {
    "passed": 4,
    "failed": 0,
    "gap": 1
  },
  "demos": [
    {
      "demo_id": "runtime_turn_demo",
      "category": "runtime",
      "reference": "claude_code_agent_loop",
      "status": "passed",
      "required_for_parity": true,
      "test_names": ["remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly"],
      "evidence": ["direct_runtime_cancellation"],
      "gap": null
    }
  ]
}
```

Status rules:

- `passed`: implemented demo behavior was validated.
- `failed`: implemented demo behavior failed.
- `gap`: capability is not implemented or not proven by deterministic tests.
- Overall `passed`: every required demo is `passed`.
- Overall `partial`: implemented demos pass and at least one required demo is
  `gap`.
- Overall `failed`: any implemented demo is `failed`.

## Demo 1: Runtime Turn Control

Purpose: prove that a remote/mobile-visible active turn is a real runtime object
that can be cancelled and replayed safely.

Required assertions:

- A remote prompt turn registers a cancellable active turn.
- `/api/tui/action interrupt_turn` uses `direct_runtime_cancellation` when the
  turn is active.
- The event log records the control event and cancelled outcome metadata.
- The notification-backed fallback remains explicit when no active handle
  exists.
- Transcript/replay preserves turn order and can be resumed without corrupting
  tool-like history.

Expected tests:

- `remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly`
- `remote_daemon_executes_projected_tui_action_ids`
- runtime transcript/replay regression from the Trellis repair slice

## Demo 2: TUI Inline REPL

Purpose: prove the local surface behaves like a mature code CLI rather than a
projection debugger.

Required assertions:

- Default launch path selects the Claw-style inline REPL.
- Startup/status copy exposes model, permission mode, branch/workspace/session
  context without flooding the screen.
- Slash palette and command help are registry-backed.
- `/continue`, `/resume`, `/permissions`, `/research`, and interrupt actions
  route to real behavior or honest staged copy.
- Structured output preserves code, diff, markdown, and tool blocks.
- Narrow terminal rendering does not wrap or overflow critical status text.

Expected tests:

- `cargo test --lib tui::tests:: -- --nocapture`
- selected TUI tests for inline REPL, palette, permissions, interrupt, and
  narrow terminal rendering

## Demo 3: Remote/Mobile Handoff

Purpose: prove web/mobile are useful companion clients over the same runtime
truth.

Required assertions:

- Mobile static app installs and exposes theme/language/session controls.
- Browser startup probes readiness before prompting for a token.
- Host-surface projection and session transcript come from the daemon, not from
  duplicated client state.
- Mobile/web action execution uses projected action IDs.
- Reconnect/resume replays from cursor and respects lease expiry.
- Permission response and direct interrupt travel through the same host action
  path as local TUI controls.
- Governed terminal input/resize/signal uses ticketed websocket flow and
  cursor-based replay.

Expected tests:

- `remote_mobile_web_app_assets_expose_theme_language_and_session_controls`
- `remote_mobile_runtime_bootstrap_probes_before_requiring_control_token`
- `remote_daemon_reconnect_replays_cursor_and_rejects_expired_lease`
- `remote_daemon_terminal_bridge_records_governed_input_resize_and_signal`
- `remote_daemon_exposes_workbench_message_and_permission_response`

## Demo 4: Host Contract Consistency

Purpose: prove all surfaces share one schema-owned control and projection
contract.

Required assertions:

- `HostSurfaceProjection` validates against schema.
- `HostSurfaceAction` includes `control_contract` and
  `fallback_control_contract` for interrupt.
- `/api/host-surface` and `/api/tui/action` agree on action IDs and contract
  semantics.
- Terminal attach/replay, artifact/result panels, permissions, skills, and
  session metadata remain projection-backed.
- Event logs are cursor-addressable and can be replayed after reconnect.

Expected tests:

- `host_surface_and_terminal_payloads_validate_against_m14_schemas`
- `remote_daemon_exposes_host_surface_and_governed_terminal_projection`
- `remote_daemon_events_streams_canonical_events_after_cursor`
- `remote_daemon_exposes_artifact_code_viewer_projection`
- `remote_daemon_exposes_result_panel_projection_without_new_truth`

## Demo 5: Routine Background Gap

Purpose: prevent overclaiming against Hermes/Claude routines.

This demo intentionally reports `gap` until implemented behavior exists.

Reference capability:

- schedule-based agent runs
- webhook/GitHub/API-triggered runs
- background process monitoring
- context/script injection before an agent run
- multi-destination delivery

Required current behavior:

- The harness records the missing command/API families as a structured gap.
- The overall manifest status becomes `partial`, not `passed`.
- The gap is actionable: it names the missing runtime surface and suggested
  future tests.

Initial gap evidence:

- No default parity demo should assume `research-cli cron create`, `webhook
  subscribe`, or `routine run` exists unless a future task implements them.
- Existing project-ops/research background hints are not enough to claim a
  Hermes-style routines lane.

## Non-Goals

- No live Anthropic/OpenAI/Hermes network dependency in the default run.
- No broad new routine runtime implementation in this task.
- No mobile visual redesign. Browser viewport polish can be referenced by the
  demo but belongs to a separate visual regression task if it fails.
- No claim of full parity while any required demo reports `gap`.

## Success Criteria

- One command runs the whole suite and writes a manifest.
- The manifest clearly separates passed implemented demos from known gaps.
- Existing focused test families remain usable on their own.
- Trellis can cite the manifest when answering whether the system has caught up
  with Claude/Hermes.
