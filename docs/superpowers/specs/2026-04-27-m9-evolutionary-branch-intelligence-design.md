# M9 Evolutionary Branch Intelligence Design

## Goal

Upgrade M9 from a simple branch search scheduler into a Git-native evolutionary multi-agent optimization system. M9 should search over executable project hypotheses, not just text plans: each candidate lives in an isolated branch/worktree lineage, receives machine-readable evaluation, enters structured debate, and can only affect the public project through a director promotion gate.

## Reference Lessons

- NVIDIA AVO / Agentic Variation Operators: treat LLM agents as variation operators that mutate, repair, combine, and simplify candidates under measurable feedback.
- AlphaEvolve and A-Evolve: keep a population of code candidates, evaluate them with objective metrics, and preserve operator credit for future runs.
- karpathy/autoresearch: fixed budgets, one comparable metric, and keep-or-discard loops produce practical progress without overbuilt orchestration.
- Tree/Graph of Thoughts and Reflexion: expose intermediate search states, critique, and reflection records instead of hiding them in a transcript.
- AlphaCode/AlphaCode 2, SWE-agent, and Agentless: quality comes from candidate diversity, filtering/reranking, issue localization, tests, and explicit repair loops.
- ruah and Ralph: multi-agent work needs worktree isolation, claims, artifacts, compatibility checks, role separation, and blocking quality gates.
- AutoResearchClaw: branch exploration, PIVOT/REFINE decisions, debate, human gates, and cross-run lessons are useful, but M9 must keep a stricter canonical project surface.

## Design Principle

M9 is not "many agents chatting" and not just beam search. It is an executable hypothesis optimizer over Git state.

A candidate branch is valid only if it records:

- what hypothesis it tests
- which operator produced it
- which files it owns
- what changed
- how it was evaluated
- how it was challenged
- why it was promoted, repaired, refreshed, or archived

## Architecture

### SearchBatch

A `SearchBatch` is the root optimization run. It freezes the base commit, task objective, budget, strategy, evaluation profile, promotion policy, and canonicality policy.

Strategies are advisory scheduler modes:

- `beam`: keep top K evaluated candidates
- `mcts`: expand promising candidates with exploration pressure
- `evolutionary`: maintain population, mutate/crossover/repair, keep diversity
- `manual`: operator-curated candidates with the same proof gates

### BranchRun

A `BranchRun` is an executable hypothesis node. It stores branch id, parent branch id, base commit, worktree path, hypothesis, variation operator, agent ids, claimed paths, status, evaluation id, debate id, and promotion decision id.

Statuses:

- `draft`
- `running`
- `evaluated`
- `needs_refresh`
- `debated`
- `promotable`
- `promoted`
- `archived`
- `cancelled`

### Variation Operators

M9 records the operator used to create each branch:

- `mutate_patch`
- `repair_failure`
- `crossover_branches`
- `simplify_diff`
- `expand_tests`
- `rebase_refresh`
- `doc_canonicalize`
- `manual_candidate`

The operator is part of the proof ledger and later feeds memory/skill learning.

### EvaluationPacket

An evaluation packet is mandatory before promotion. It should include objective and governance signals:

- test commands and outcomes
- conformance/schema checks
- canonicality audit result
- stale-base state
- changed paths
- claimed paths
- risk score
- metric deltas
- reviewer gate status
- blocking issues

### DebateTrace

Debate is a structured adversarial record. It compares candidates and stores proposition, supporting evidence, opposing evidence, unresolved assumptions, counterexamples, reviewer roles, and recommendation.

### PromotionDecision

Promotion is director-owned. A branch may be promoted only if:

- evaluation exists
- review/debate gate exists
- stale-base state is clean or refreshed
- canonicality blocking violations are absent
- there is no other active promoted winner for the same batch

Losing branches are archived and do not become public project truth.

## Canonicality Rule

M9 must preserve the project's single latest public truth. Search branches, debate traces, evaluations, and loser artifacts live under `.pmcli/branches/` and hidden worktree state. The externally visible docs/code surface changes only after a promotion decision. Promotion must either merge/apply the winner or report a blocked decision; it must never expose multiple generation docs or competing code paths as public truth.

## Minimal Graduated Surface

The first implementation should graduate a deterministic local proof surface:

- `branches search --objective <text> --json`
- `branches list --json`
- `branches inspect <branch-id> --json`
- `branches mutate <branch-id> --llm-command <cmd> --json`
- `branches evaluate <branch-id> --json`
- `branches debate <branch-id> --against <branch-id> --json`
- `branches promote <branch-id> --json`
- `branches archive <branch-id> --reason <text> --json`

This is enough to prove the M9 governance loop while allowing live LLM or agent
drivers to plug in through a command adapter.

Graduated implementation boundary: M9 creates real detached Git worktrees and
candidate artifacts, mutates candidates through `--llm-command`, evaluates them
with real local commands and canonicality gates, persists debate evidence, and
automatically applies the winning public diff on promotion. Hidden candidate
artifacts are excluded from the public merge. If the source public worktree is
dirty, the diff cannot apply, or post-merge canonicality fails, promotion is
blocked and any attempted merge is rolled back. Direct provider API invocation is
not hardwired into M9; provider/agent code should use the mutation adapter.

## Completion Proof

M9 is complete when tests prove:

- search creates a budgeted, auditable batch and executable branch candidates
- evaluation is required before promotion
- debate/review evidence is required before promotion
- stale base blocks promotion until refreshed or re-evaluated
- only one winner per batch can be promoted
- losing branches archive cleanly
- help/schema/conformance surfaces expose the M9 contracts
- no search artifact becomes a public canonical file without director promotion
