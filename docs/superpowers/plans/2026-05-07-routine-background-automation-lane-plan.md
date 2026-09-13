# Routine Background Automation Lane Plan

## Objective

Convert the parity demo's routine/background gap into a minimal implemented lane:

`routine definition -> explicit schedule/webhook/api/manual trigger -> agent run -> ProjectOps lease -> host/research projection`

## Non-Goals

- No always-on scheduler daemon.
- No public webhook HTTP server.
- No vendor network dependency.
- No new agent runtime.
- No bypass around permissions, review, or ProjectOps supervision.

## Implementation Order

1. Add a `routines` module with persisted definitions and trigger records. Done in implementation slice.
2. Add CLI create/list/inspect/trigger surfaces. Done in implementation slice.
3. Trigger routines through existing local agent runner. Done in implementation slice.
4. Acquire a ProjectOps lease for the triggered run. Done in implementation slice.
5. Emit routine events. Done in implementation slice.
6. Update parity demo harness and docs. Done in implementation slice.

## Acceptance Criteria

- A routine can be created with a trigger kind and command.
- A routine can be triggered explicitly as schedule/webhook/api/manual.
- Trigger creates an agent run using existing task packet/runtime/output manifest files.
- Trigger creates a ProjectOps lease tied to the routine run.
- Routine list/inspect exposes definition and trigger history.
- Parity demo routine lane reports `passed`, not `gap`, for the minimal implemented behavior.

## Test Plan

- `cargo test --test conformance operator_cli_surfaces::routines_create_trigger_and_project_background_agent -- --nocapture`
- `cargo test --test conformance parity_demo_harness_executes_expected_scenarios -- --nocapture`
- `scripts/run_parity_demo_suite.sh`
- `cargo check --quiet`
- `cargo fmt --check`
- `git diff --check`
