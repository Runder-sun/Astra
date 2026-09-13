# Astra Pi integration changes

## 2026-08-21

- Separated permanent provider configuration failures from temporary capacity
  failures; 429, overload, and transient 5xx events now persist exponential
  backoff and retry the same scientific task without consuming its attempt.
- Added guided resume, which records user guidance and supersedes frozen active
  work before the main agent replans the current research capability.
- Added task-owned durable resource roots for environments, datasets, and
  checkpoints, with downstream canonical reuse, retry inheritance, shell path
  confinement, and archival alongside discarded candidate workspaces.
- Made dependency, simulator, dataset, and checkpoint provisioning explicit
  implementation work and removed default multi-candidate search from the
  implementation capability to avoid duplicate large installations.

## 2026-08-19

- Separated governed process completion from `scientificOutcome` and
  `missionCoverage`; inconclusive or refuted work can close honestly without
  being presented as a supported primary hypothesis.
- Added typed claim assessments so unsupported and unresolved claims no longer
  enter the accepted-claim index.
- Added two-review quorum requirements for result-to-claim and whole-research
  review artifacts.
- Added `--require-paper`, which makes manuscript and compiled-paper artifacts
  hard completion deliverables.
- Projected user guidance into persistent main-agent route context and the
  shared Research Board.
- Upgraded run auditing to v4 with a separate scientific-result section and
  claim/outcome consistency checks.

## 2026-08-12

- Added `@earendil-works/pi-astra` as the Astra product layer on Pi `0.84.1`.
- Added durable `.astra` JSON snapshot and append-only JSONL event store.
- Added MissionFrame, stage DAG, TaskPacket, lease, evidence/review/adoption,
  blocking-obligation, and canonical replacement state transitions.
- Added `createAstraExtension()` and `runAstra()` composition root. The extension
  consumes Pi lifecycle hooks and registers research tools/commands without
  copying Pi's provider loop, session runtime, or UI.
- Added fixture supervisor integration tests. Child Pi worker/reviewer sessions
  and live provider execution remain the next migration phase.

## 2026-08-13

- Added real Pi JSON child-session adapters for worker, reviewer, and main-agent
  rounds, including deterministic session ids, timeout/failure records, fresh
  decision sessions, and manifest identity checks.
- Added review target snapshots, immutable review packets/traces, canonical
  materialization receipts, replacement lifecycle states, durable job memory,
  package stage/role skills, and read-only `.pmcli` migration reports.
- Added Pi TUI status/widget projections and `/research`, `/research-tasks`,
  `/research-review`, and `/research-continue` commands.
- The Rust runtime remains a legacy compatibility surface; `.astra` is the
  canonical state for the Pi-native product.
- Added recursive SHA-256 inventory to the read-only `.pmcli` migration report;
  legacy files are never copied or promoted automatically.

## 2026-08-14

- Routed shell and slash-command research control through Pi `main()` and the
  shared `runResearchControl()` service.
- Preserved extension-only parent Pi sessions and exposed `session_start`
  control events in JSON mode before any model prompt.
- Added parent-session linkage to the run audit and froze the reviewed Rust
  source inventory.
- Made the offline fixture provider dispatch only for explicit child-session
  roles and complete the post-tool assistant turn without a terminal error.
- Extended run audits to count Pi assistant `error` and `aborted` terminations
  as failed child sessions.
- Changed `research resume` to continue an interrupted autonomous job through
  the same bounded outer loop used by `research run`; `research tick` remains
  the single-transaction control.
- Added parent-session audit evidence for resumed completion, Pi compaction,
  and matching Astra checkpoints.
- Enforced `collaborative`, `autonomous`, and `full` automation policies with
  durable dispatch/closure gates and resumable approvals.
- Enforced job-wide task, child-session turn, and Pi-reported cost budgets;
  budget gates require an explicit limit increase before resume.
- Made accepted evidence and already-dispatched worker tasks resumable across
  supervisor transaction boundaries instead of relying on one uninterrupted tick.
- Converted packaged stage/role instructions into valid Pi skills and bound only
  the active stage and child role through Pi's explicit `--skill` loader.
- Archived the complete pre-Pi Cargo crate under `legacy/rust/`; root product
  entry points and default Pi build/release flows no longer include Rust.

## 2026-08-17

- Replaced the fixed stage DAG with a persistent main-agent, canonical research
  graph, capability catalog, and explicit dynamic route decisions.
- Added durable multi-candidate search, frozen criterion-level evaluations,
  winner-only promotion, and physical cleanup of loser workspaces.
- Made reviewer sessions receive immutable snapshots plus checksum-verified raw
  evidence files; passing reviews must cover every frozen criterion.
- Added lineage-based backtracking that prunes obsolete canonical artifacts,
  evidence, reviews, task workspaces, and session files while retaining minimal
  retirement receipts.
- Added completion gates for accepted claims, a positive whole-research review,
  reviewed canonical artifacts, and zero blocking objections or obligations.
- Added `/research-board`, `/research-guide`, and `/research-route` for shared
  human/main-agent research control. Collaborative mode now asks only at
  material scientific uncertainty instead of pausing every worker dispatch.
- Upgraded run auditing to v3 with separate runtime-integrity, research-quality,
  and unresolved-uncertainty sections.
