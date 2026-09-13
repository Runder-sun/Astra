# Parity Demo Automation Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an automated demo harness that runs the core Claude/Hermes parity demos and emits a machine-readable pass/fail/gap manifest.

**Architecture:** Add a `tests/parity_demo` harness family modeled after the existing mock-provider parity harness. Keep behavior assertions in existing Rust conformance/TUI/operator tests where possible, add only missing cross-surface checks, and drive them from `scripts/run_parity_demo_suite.sh`. The routines/background lane is represented as a structured `gap` until implemented runtime support exists.

**Tech Stack:** Rust conformance tests, shell harness, JSON manifest, existing `cargo test` matrix, Trellis task docs.

---

## File Structure

- Create: `tests/parity_demo/README.md`
  - Documents demo IDs, manifest status semantics, and how to run the suite.
- Create: `tests/parity_demo/last_run_manifest.json`
  - Last deterministic harness output.
- Create: `scripts/run_parity_demo_suite.sh`
  - Runs selected tests and writes the parity demo manifest.
- Modify: `tests/conformance.rs`
  - Include a new conformance module if the repo pattern requires it.
- Create or modify: `tests/conformance/parity_demo.rs`
  - Self-test for the harness script, expected demo list, and manifest shape.
- Modify: `tests/conformance/remote_daemon.rs`
  - Add missing cross-surface assertions only if existing tests do not expose the required evidence.
- Modify: `tests/operator/cli_surfaces.rs`
  - Add missing command-surface assertions only if the TUI demo cannot cite existing tests.
- Modify: `.trellis/tasks/05-05-parity-demo-automation-suite/README.md`
  - Update status and validation notes as tasks complete.

## Task 1: Add Harness Skeleton and Manifest Contract

**Files:**
- Create: `tests/parity_demo/README.md`
- Create: `scripts/run_parity_demo_suite.sh`
- Create: `tests/parity_demo/last_run_manifest.json`
- Create or modify: `tests/conformance/parity_demo.rs`
- Modify: `tests/conformance.rs`

- [ ] **Step 1: Write the failing harness self-test**

Add a test named:

```rust
parity_demo_harness_executes_expected_scenarios
```

The test should run `scripts/run_parity_demo_suite.sh` with a fake `CARGO_BIN`,
read a temporary manifest path from `PARITY_DEMO_MANIFEST`, and assert:

- `harness == "parity_demo_suite"`
- `schema_version == "1"`
- demo IDs are exactly:
  - `runtime_turn_demo`
  - `tui_inline_repl_demo`
  - `remote_mobile_handoff_demo`
  - `host_contract_demo`
  - `routine_background_gap_demo`
- the fifth demo has `status == "gap"`
- overall status is `partial` when the first four are `passed` and the fifth is `gap`

- [ ] **Step 2: Run the failing test**

Run:

```bash
cargo test --test conformance parity_demo_harness_executes_expected_scenarios -- --nocapture
```

Expected: FAIL because the script/module does not exist yet.

- [ ] **Step 3: Implement the minimal script and manifest**

Create `scripts/run_parity_demo_suite.sh` modeled after
`scripts/run_mock_parity_harness.sh`.

Use arrays like:

```bash
scenarios=(
  "runtime_turn_demo|runtime|claude_code_agent_loop|remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly"
  "tui_inline_repl_demo|tui|claw_code_inline_repl|tui::tests::"
  "remote_mobile_handoff_demo|mobile|claude_mobile_continuity|remote_mobile_web_app_assets_expose_theme_language_and_session_controls"
  "host_contract_demo|host|shared_host_surface_contract|host_surface_and_terminal_payloads_validate_against_m14_schemas"
)
gaps=(
  "routine_background_gap_demo|automation|hermes_routines|missing scheduled/webhook/API-triggered background automation lane"
)
```

The script should run the pass scenarios through cargo, then write JSON with
four `passed` demos, one `gap`, and overall `partial`.

- [ ] **Step 4: Re-run the harness self-test**

Run:

```bash
cargo test --test conformance parity_demo_harness_executes_expected_scenarios -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tests/parity_demo scripts/run_parity_demo_suite.sh tests/conformance.rs tests/conformance/parity_demo.rs
git commit -m "Add parity demo harness skeleton"
```

## Task 2: Wire Runtime Turn Demo Evidence

**Files:**
- Modify: `scripts/run_parity_demo_suite.sh`
- Modify: `tests/conformance/remote_daemon.rs` only if evidence is missing.

- [ ] **Step 1: Audit existing runtime evidence**

Confirm the harness can cite:

- `remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly`
- `remote_daemon_executes_projected_tui_action_ids`
- replay/session tests from the runtime repair slice

- [ ] **Step 2: Add missing assertion if needed**

If the direct-cancellation test does not assert both event-log and action-result
evidence, add the missing assertion before changing the harness.

- [ ] **Step 3: Run focused runtime tests**

Run:

```bash
cargo test --test conformance remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Update the runtime demo manifest evidence list**

Include evidence keys:

- `active_turn_registry`
- `direct_runtime_cancellation`
- `cancelled_turn_outcome`
- `notification_fallback_contract`

- [ ] **Step 5: Commit**

```bash
git add scripts/run_parity_demo_suite.sh tests/conformance/remote_daemon.rs
git commit -m "Wire runtime turn parity demo"
```

## Task 3: Wire TUI Inline REPL Demo Evidence

**Files:**
- Modify: `scripts/run_parity_demo_suite.sh`
- Modify: `tests/operator/cli_surfaces.rs` or `src/tui.rs` tests only if evidence is missing.

- [ ] **Step 1: Select focused TUI tests**

Use existing tests that cover:

- default inline REPL launch
- slash palette
- `/continue` and `/resume`
- `/permissions`
- running-turn interrupt
- structured output
- narrow terminal fit

- [ ] **Step 2: Avoid running the entire lib suite if a focused list is enough**

Prefer named tests to keep the demo fast. If naming becomes fragile, use:

```bash
cargo test --lib tui::tests:: -- --nocapture
```

- [ ] **Step 3: Run the selected TUI matrix**

Run the exact commands the harness will use.

Expected: PASS.

- [ ] **Step 4: Update manifest evidence**

Include evidence keys:

- `inline_repl_default`
- `registry_backed_palette`
- `live_permissions`
- `turn_interrupt`
- `structured_output`
- `narrow_terminal_fit`

- [ ] **Step 5: Commit**

```bash
git add scripts/run_parity_demo_suite.sh tests/operator/cli_surfaces.rs src/tui.rs
git commit -m "Wire TUI inline REPL parity demo"
```

## Task 4: Wire Remote/Mobile Handoff Demo Evidence

**Files:**
- Modify: `scripts/run_parity_demo_suite.sh`
- Modify: `tests/conformance/remote_daemon.rs` only if evidence is missing.

- [ ] **Step 1: Select mobile/remote tests**

Use existing tests that cover:

- mobile app assets and controls
- bootstrap readiness probe
- workbench message and permission response
- reconnect cursor replay
- governed terminal input/resize/signal
- direct interrupt via host action

- [ ] **Step 2: Add any missing cross-surface assertion**

If mobile action execution does not currently verify action IDs and direct
interrupt contract together, add a focused conformance assertion.

- [ ] **Step 3: Run the mobile/remote test matrix**

Run:

```bash
cargo test --test conformance remote_mobile_web_app_assets_expose_theme_language_and_session_controls -- --nocapture
cargo test --test conformance remote_mobile_runtime_bootstrap_probes_before_requiring_control_token -- --nocapture
cargo test --test conformance remote_daemon_reconnect_replays_cursor_and_rejects_expired_lease -- --nocapture
cargo test --test conformance remote_daemon_terminal_bridge_records_governed_input_resize_and_signal -- --nocapture
cargo test --test conformance remote_daemon_exposes_workbench_message_and_permission_response -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Update manifest evidence**

Include evidence keys:

- `shared_session_projection`
- `readiness_probe_before_token`
- `permission_response`
- `cursor_reconnect`
- `governed_terminal_bridge`
- `direct_interrupt`

- [ ] **Step 5: Commit**

```bash
git add scripts/run_parity_demo_suite.sh tests/conformance/remote_daemon.rs
git commit -m "Wire remote mobile handoff parity demo"
```

## Task 5: Wire Host Contract Demo Evidence

**Files:**
- Modify: `scripts/run_parity_demo_suite.sh`
- Modify: `tests/conformance/remote_daemon.rs` only if evidence is missing.

- [ ] **Step 1: Select host contract tests**

Use existing tests that cover:

- host-surface schema validation
- projected action IDs
- direct/fallback interrupt contract metadata
- terminal attach/replay projection
- event cursor replay
- artifact/result panels without new truth

- [ ] **Step 2: Run focused host tests**

Run:

```bash
cargo test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture
cargo test --test conformance remote_daemon_exposes_host_surface_and_governed_terminal_projection -- --nocapture
cargo test --test conformance remote_daemon_events_streams_canonical_events_after_cursor -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Update manifest evidence**

Include evidence keys:

- `schema_valid_host_surface`
- `shared_action_ids`
- `direct_and_fallback_contracts`
- `cursor_event_log`
- `projection_backed_artifacts`

- [ ] **Step 4: Commit**

```bash
git add scripts/run_parity_demo_suite.sh tests/conformance/remote_daemon.rs
git commit -m "Wire host contract parity demo"
```

## Task 6: Make Routine Background Gap Actionable

**Files:**
- Modify: `scripts/run_parity_demo_suite.sh`
- Modify: `tests/parity_demo/README.md`
- Modify: `.trellis/tasks/05-05-parity-demo-automation-suite/README.md`

- [ ] **Step 1: Encode the gap in the manifest**

The `routine_background_gap_demo` entry should include:

```json
{
  "demo_id": "routine_background_gap_demo",
  "category": "automation",
  "reference": "hermes_routines",
  "status": "gap",
  "required_for_parity": true,
  "gap": {
    "missing_capability": "scheduled/webhook/API-triggered background automation",
    "missing_surfaces": ["routine", "cron", "webhook"],
    "future_tests": [
      "routine_create_schedule_persists_trigger",
      "webhook_trigger_starts_background_agent_run",
      "background_run_delivers_result_to_configured_target"
    ]
  }
}
```

- [ ] **Step 2: Assert the gap is not counted as pass**

Extend the harness self-test so the overall status remains `partial`, not
`passed`.

- [ ] **Step 3: Run the harness self-test**

Run:

```bash
cargo test --test conformance parity_demo_harness_executes_expected_scenarios -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add scripts/run_parity_demo_suite.sh tests/parity_demo/README.md .trellis/tasks/05-05-parity-demo-automation-suite/README.md
git commit -m "Record routine background parity gap"
```

## Task 7: Baseline Verification and Trellis Closure

**Files:**
- Modify: `.trellis/tasks/05-05-parity-demo-automation-suite/README.md`
- Modify: `.trellis/tasks/05-05-parity-demo-automation-suite/research/00-findings.md`

- [ ] **Step 1: Run the full parity demo suite**

Run:

```bash
scripts/run_parity_demo_suite.sh
```

Expected: script exits 0 and writes `tests/parity_demo/last_run_manifest.json`
with overall `partial`.

- [ ] **Step 2: Run focused regression matrix**

Run:

```bash
cargo test --test conformance parity_demo_harness_executes_expected_scenarios -- --nocapture
cargo test --test conformance remote_daemon -- --nocapture
cargo test --lib tui::tests:: -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run hygiene check**

Run:

```bash
git diff --check
```

Expected: no output.

- [ ] **Step 4: Update Trellis status**

Mark the Trellis task as validated. Record that parity status is `partial`
because the routines/background demo is an intentional required gap.

- [ ] **Step 5: Commit**

```bash
git add .trellis/tasks/05-05-parity-demo-automation-suite tests/parity_demo scripts/run_parity_demo_suite.sh tests/conformance.rs tests/conformance/parity_demo.rs tests/conformance/remote_daemon.rs tests/operator/cli_surfaces.rs docs/superpowers/specs/2026-05-05-parity-demo-automation-suite-design.md docs/superpowers/plans/2026-05-05-parity-demo-automation-suite-plan.md
git commit -m "Add parity demo automation suite"
```

## Non-Goals

- Do not implement the routines/background runtime in this task.
- Do not add live vendor or network requirements to the default demo suite.
- Do not weaken the manifest by counting known gaps as passes.
- Do not duplicate existing conformance assertions unless a cross-surface demo
  requires an explicit evidence key.
