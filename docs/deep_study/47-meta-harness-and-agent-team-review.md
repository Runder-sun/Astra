# Meta-Harness And Agent-Team Review

This document records the focused deep study of the new reference set added
after the initial architecture freeze:

- `reference_repos/meta_harness/metaharness`
- `reference_repos/agent_teams/squad`
- `reference_repos/agent_teams/autogen`
- `reference_repos/agent_teams/ruah-orch`
- `reference_repos/agent_teams/ruah-cli`

Its purpose is not to reopen the whole architecture.

Its purpose is to extract the parts that should concretely change
`research-cli` design and implementation.

## 1. What Each Reference Teaches

## 1.1 `metaharness`

The strongest lesson from `metaharness` is that the harness itself must become
an optimization target.

The notable mechanisms are:

- filesystem-first run store
- candidate ledger with explicit keep/discard/crash/timeout outcomes
- compact environment bootstrap snapshot before each proposal
- explicit mutable-scope allowlist such as `allowed_write_paths`
- compare/summarize/ledger surfaces that make outer-loop optimization
  inspectable

What matters for `research-cli`:

- we need durable, typed operator artifacts that can be compared across runs
- meta-optimization must optimize instruction files, setup flows, validation
  scripts, and routing logic, not only prompts
- environment facts should be captured early and re-used instead of forcing
  every run to rediscover the workspace

## 1.2 `squad`

The strongest lesson from `squad` is that agent teams become usable only when
their state is persistent, inspectable, and human-directed.

The notable mechanisms are:

- persistent repo-local team state as files
- human-led team model rather than opaque autonomous takeover
- watch/triage/health surfaces for long-running operation
- externalized state support
- explicit context hygiene and compression (`nap`)

What matters for `research-cli`:

- multi-agent state cannot live only in transient process memory
- the operator must be able to inspect team state without opening raw logs
- long-running background maintenance should expose health and wake surfaces
- compaction and silent summarization must be first-class project operations

## 1.3 `autogen`

The strongest lesson from `autogen` is not its specific chat patterns.

The stronger lesson is the layered split between:

- runtime and message passing
- high-level agent orchestration API
- extensions and integrations
- developer tools such as bench and studio

What matters for `research-cli`:

- orchestration, runtime, and host tooling must remain separate planes
- benchmarking and review tooling should not become runtime authority
- multi-agent patterns should compile down to one kernel protocol, not invent a
  second orchestration truth

## 1.4 `ruah`

`ruah` is the most directly useful agent-team reference for our code-agent CLI
line.

The notable mechanisms are:

- worktree-backed workspace isolation
- claim-aware file ownership and conflict rejection before execution
- durable task artifacts with changed files, patch, and validation results
- takeover and workflow-resume paths
- governance gates before merge
- JSON-visible orchestration state instead of hidden scheduler state

What matters for `research-cli`:

- our multi-agent lane should be worktree-native and claim-aware
- takeover, retry, refresh, and resume must be operator-visible commands
- agent work products need durable artifacts, not just chat summaries
- governance and merge validation belong in the orchestration plane, not as an
  afterthought

## 2. Adoption Decisions

The design should adopt these ideas:

- `metaharness` outer-loop thinking: optimize executable harness artifacts, not
  only prompts
- `metaharness` evidence model: candidate ledgers, environment snapshots, and
  bounded write scope
- `squad` persistent team state and human-led control posture
- `squad` health/watch/context-hygiene productization mindset
- `autogen` layered runtime/orchestration/extension split
- `ruah` worktree isolation, file claims, durable task artifacts, takeover, and
  governance

The design should explicitly reject these failure modes:

- hidden app/server runtime authority
- multi-agent magic that leaves no typed artifact trail
- logs that become the only way to understand state
- background agents mutating the repo without claim or permission visibility
- benchmark and meta-optimization tooling becoming runtime truth

## 3. Architecture Changes Required

These references sharpen several previously broad design goals into hard
requirements.

## 3.1 Harness Meta-Loop Must Be Native

`research-cli` needs a native meta-optimization lane for its own harness:

- instruction files
- setup/bootstrap scripts
- validation scripts
- routing policies
- repo-specific acceptance checks

This lane must produce:

- run manifests
- candidate manifests
- keep/discard outcomes
- diff and validation artifacts
- compare/summarize surfaces

The meta-loop is a consumer of kernel artifacts, not a second runtime.

## 3.2 Agent Teams Must Be Claim-Aware

The agent-team design must not stop at "multiple agents can be spawned."

It needs:

- isolated workspaces per task or branch
- explicit owned/shared/read-only path claims
- takeover and retry semantics
- compatibility checks before promotion
- governance gates before merge or canonical promotion

This is the strongest reusable line from `ruah`.

## 3.3 Durable Logs Must Be First-Class

The current architecture already froze `SessionOperatorLogManifest` and
`DerivedSessionReadModel`.

These references make that requirement stronger, not weaker:

- `metaharness` needs comparable run artifacts
- `squad` needs inspectable long-running team state
- `ruah` needs durable task artifacts for takeover/recovery

Therefore `sessions logs` is not a nice-to-have export surface.

It is a base-CLI foundation requirement for both self-improvement and
multi-agent governance.

## 3.4 Team State Must Stay Human-Led

The operator remains accountable for:

- strategy choice
- approval boundaries
- promotion to canonical memory/repo state
- remote takeover and permission decisions

The agent team is native and durable, but never the final authority.

This is the correct synthesis of `squad`, `ruah`, and our existing kernel
authority line.

## 4. Immediate Implementation Priorities

These references suggest the next implementation priorities in order:

1. durable session/operator log surfaces
2. explicit derived read models for inspect/search/export
3. meta-harness-ready run/candidate artifact lane
4. claim-aware worktree execution for future multi-agent tasks
5. watch/health/takeover surfaces for long-running project operations

## 5. First Concrete Landing

The first concrete landing from this review should be:

- implement `sessions logs`
- emit typed `SessionOperatorLogManifest`
- expose rebuildable `DerivedSessionReadModel`
- include operator log refs in `sessions export`

This gives the current Rust kernel a real substrate for:

- future meta-harness optimization loops
- future agent-team replay and takeover
- project-memory auditability
- remote/mobile/web observability
