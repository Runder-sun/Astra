# Routine Trigger Ingress Recovery Plan

## Objective

Turn routine/background automation from explicit CLI trigger parity into a durable ingress and recovery lane:

`delivery received -> ingress record -> due runner -> routine trigger -> agent run -> ProjectOps supervision -> retry/recovery`

## Constraints

- Reuse existing routines, agents, ProjectOps, events, and host projections.
- Do not add a daemon or public webhook listener in this slice.
- Do not create a second scheduler or board store.
- Keep external triggers as durable records that can be produced by future servers, cron, or mobile controls.

## Implementation Order

1. Add `RoutineIngressRecord` and persistence helpers.
2. Add `routines ingress` CLI to record schedule/webhook/api/manual delivery.
3. Add dedupe by `(routine_id, trigger_kind, dedupe_key)` as an idempotent ledger decision, not as a second queue.
4. Add `routines run-due` CLI to consume only ready ingress records through existing trigger code.
5. Add retry/recovery linkage for failed trigger records without deleting the failed attempt.
6. Surface ingress and trigger history together in `routines inspect`.
7. Add tests and update trellis task state.

## Acceptance Criteria

- Ingress records are durable and inspectable.
- Duplicate ingress with the same scoped dedupe key is recorded as a skipped ledger entry and never creates a duplicate worker.
- Running due ingress creates the same existing trigger/agent/lease artifacts as manual trigger.
- Failed triggers can be retried without deleting the failed attempt.
- Host/research projection can still reason from existing routine trigger, agent, and ProjectOps state.
- Identifiers for routines, ingress, and triggers are stable across separate CLI invocations and do not collide when process-local sequence counters reset.

## Deferred Work

- Always-on schedule daemon.
- Public webhook HTTP server.
- Remote/mobile UI controls for creating ingress deliveries.
- Backoff policy and max retry budget.

## Implemented Slice

- Added durable routine ingress records under the existing routines state tree.
- Added `routines ingress`, `routines run-due`, and `routines retry` as CLI ingress/recovery controls.
- Kept execution on the existing routine trigger path, local agent runtime records, and ProjectOps supervision leases.
- Added `retry_of_trigger_id`, `attempt`, and optional `ingress_id` linkage to trigger records.
- Added reducer support for routine ingress/run-due/retry events and updated TUI/help wording.
- Added operator tests for dedupe/run-due and failed-trigger retry recovery.
- Tightened `ready_at` to millisecond timestamp validation and numeric due comparison after subagent review.
- Tightened retry to failed/timeout trigger records and added process id to routine-side generated ids.

## Review Follow-Ups

- `routines.lock` now serializes ingress/run-due/retry writes for one data directory. Add a scoped dedupe index or stronger transactional store only if routine ingress becomes high-throughput, multi-host, or network-filesystem backed.
- Add recovery visibility for early `agents::start_local` failures that happen before a trigger record exists.
- Consider a shared id generator across ProjectOps and routines if routine ingress becomes high-throughput or multi-process.
