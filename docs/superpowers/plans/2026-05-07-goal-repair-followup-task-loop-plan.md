# Goal Repair Follow-Up Task Loop Plan

## Objective

Close the post-review execution loop:

`failed worker -> repair review -> review resolved pass -> repair follow-up task projected -> agent team dispatch -> normal acceptance`

## Non-Goals

- No new scheduler.
- No new repair queue.
- No new board store.
- No automatic natural-language task decomposition beyond preserving the review response as task context.
- No changes to the review packet schema.

## Implementation Order

1. Record a stable repair follow-up artifact when a passing repair review is consumed.
2. Project that artifact into the existing goal task pool.
3. Use `repair_failed_task` automation policy to decide ready versus approval-gated status.
4. Reuse the existing dispatch claim path for the follow-up task.
5. Reuse the existing acceptance path for follow-up worker output.
6. Add focused unit and operator CLI tests.

## Acceptance Criteria

- Passing repair review still closes the original blocked claim.
- The same consume step creates a visible task-pool entry sourced from the repair review.
- The entry includes the review response as bounded task context.
- The entry is not re-projected after it has been dispatched or claimed.
- The next `goals advance` can dispatch the follow-up task in full-auto mode.
- Follow-up worker output can be accepted through the existing acceptance gate.

## Test Plan

- `cargo test goals::tests:: --lib -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::goals_advance_consumes_resolved_repair_review -- --nocapture`
- `cargo check --quiet`
- `cargo fmt --check`
- `git diff --check`
