# Goal Multi-Agent Task Pool Backbone Plan

## Objective

Upgrade the goal-driven research loop from one active worker claim to a small multi-agent task pool backbone:

`goal task pool -> multiple distinct claims -> multiple agent runs -> projected running work -> normal acceptance / repair gates`

## Design Constraints

- Reuse existing MissionFrame, orchestration run, agents, reviews, ProjectOps, and research board projection.
- Do not introduce a scheduler, separate kanban store, or second runtime.
- Preserve the current chat-first main-agent loop.
- Preserve existing approval and review gates.
- Keep old `dispatch` JSON compatibility while adding batch visibility.

## Implementation Order

1. Add automation-mode-aware claim budget.
2. Add selector logic that skips already claimed or closed entries.
3. Change dispatch from one optional result to a bounded vector of results, while preserving the first result as `dispatch`.
4. Emit canonical dispatch events for all results.
5. Add unit tests and operator CLI coverage for full-auto multi-dispatch and high-autonomy single-claim behavior.

## Acceptance Criteria

- Full-auto can dispatch more than one distinct allowed ready task in a single advance when budget permits.
- High-autonomy remains limited to one active worker claim.
- Duplicate claims for the same task pool entry are still blocked.
- The research board shows multiple running claim/agent entries from the same goal run.
- Existing one-cycle goal tests continue to pass.

## Deferred Work

- Real long-running asynchronous workers with later completion polling.
- External scheduler/webhook/API trigger fan-in for multi-agent task pools.
- User-facing mobile controls for task prioritization and manual reassignment.
- Smarter decomposition of high-level goals into multiple independent worker entries.
