# Goal Repair Review Resolution Loop Plan

## Objective

Close the repair-review gate in the goal loop:

`failed worker -> repair review opened -> review resolved -> goal loop consumes review -> claim closed or remains blocked`

## Non-Goals

- No new scheduler.
- No new review database.
- No new repair queue.
- No automatic code repair execution.
- No automatic pivot or publish.

## Implementation Order

1. Add a review resolve API that appends a new trace and updates `trace.latest.json`.
2. Expose `reviews resolve` in the runtime CLI.
3. Teach `goals advance` to inspect repair review artifacts when `goal_acceptance` is blocked.
4. On passing review, close the matching blocked claim and mark the review consumed.
5. On failing review, preserve the blocked state with explicit failure artifacts.
6. Add unit and operator CLI tests.

## Acceptance Criteria

- An open review can be resolved without creating a new review packet.
- Review history retains the original pending trace and the resolved trace.
- `goals advance` no longer stays in `await_repair_review` after a passing repair review is resolved.
- Passing repair review closes the blocked claim and returns the run to a resumable state.
- Failing repair review remains blocked and exposes the failed review decision.
- Existing failed-worker behavior still opens only one repair review until it is resolved.

## Test Plan

- `cargo test reviews::tests:: --lib -- --nocapture`
- `cargo test goals::tests:: --lib -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::reviews_resolve_updates_review_trace -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::goals_advance_consumes_resolved_repair_review -- --nocapture`
- `cargo check --quiet`
- `cargo fmt --check`
- `git diff --check`

## Completion Notes

- Implemented review resolution as an append-only trace update on the existing review packet.
- Reused `goal_acceptance` artifacts to connect repair review ids back to agent ids.
- Passing review verdicts close the blocked goal task claim and resume the existing goal run.
- Blocking verdicts keep the claim and run blocked with explicit failure artifacts.
- No new scheduler, queue, board store, or agent runtime was introduced.
