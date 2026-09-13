# Goal Research Stage Decision Loop Plan

## Objective

Connect goal acceptance to the research-stage decision lane:

`worker accepted -> accepted evidence recorded -> stage decision needed -> research decide gate visible`

## Non-Goals

- Do not auto-run `research decide`.
- Do not change stage transition rules.
- Do not create a new stage queue.
- Do not mark final claims or papers complete.
- Do not weaken review, approval, or external publish boundaries.

## Implementation Order

1. Add a stable artifact for research-stage decision needed after accepted goal work.
2. Record that artifact from goal acceptance when an active research stage exists.
3. Project the artifact into `GoalTaskPoolSnapshot`.
4. Project the artifact into `ResearchBoardProjection`.
5. Add unit and operator tests.

## Acceptance Criteria

- Accepted full-auto worker output still closes the original goal task claim.
- If an active research stage exists, goal acceptance records a stage-decision-needed artifact.
- Goal status/task pool shows an item sourced from that artifact.
- Research board shows the same item without storing board-owned state.
- The item recommends the existing `research decide` command.
- Full-auto does not silently advance the research stage.

## Test Plan

- `cargo test goals::tests:: --lib -- --nocapture`
- `cargo test research::tests:: --lib -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::goals_advance_creates_goal_run_and_advances_one_cycle -- --nocapture`
- `cargo check --quiet`
- `cargo fmt --check`
- `git diff --check`
