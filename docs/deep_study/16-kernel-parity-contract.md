# Kernel Parity Contract

This document freezes the first-release kernel contract for `research-cli`.

It exists to remove the remaining ambiguity called out by external review:
the system should not only have a strong architecture, it should have a
product-grade CLI/TUI kernel contract that can be compared against Claw Code,
Hermes, and Crush.

## 1. Why This Document Exists

The runtime protocol in `14` and the research/review contracts in `15` are
necessary but not sufficient.

To clearly exceed the current baseline projects, the kernel must also publish:

- the exact operator-facing command surface
- active project-management semantics
- active memory query/explain semantics
- cost/usage/accounting semantics
- project/worktree lifecycle semantics
- parity and acceptance tests for real workflows

Without this document, the design is still too easy to overclaim.

## 2. Baseline Comparison Targets

The kernel is compared against:

- `claw-code` for permission rigor, session/runtime discipline, doctor, and parity harnesses
- `Hermes` for broad operator surface, session browsing, model/config/product completeness
- `Crush` for workspace/project/config/MCP productization and session-based runtime UX

The goal is not to clone any one project.

The goal is to exceed them on:

- protocol safety
- project memory depth
- repo governance
- auditable multi-agent orchestration

while matching them on:

- daily usability
- operator ergonomics
- recovery/debug visibility

## 3. Frozen First-Release CLI Surface

The first stable product surface must include the following commands.

### Core runtime

- `research-cli`
- `research-cli prompt`
- `research-cli chat`
- `research-cli resume`
- `research-cli continue`
- `research-cli inspect`
- `research-cli inspect --project`
- `research-cli compact`
- `research-cli doctor`
- `research-cli smoke`
- `research-cli conformance`

### Project management

- `research-cli projects list`
- `research-cli projects current`
- `research-cli projects register`
- `research-cli projects init`
- `research-cli projects status`
- `research-cli projects prune`

### Session management

- `research-cli sessions list`
- `research-cli sessions browse`
- `research-cli sessions search`
- `research-cli sessions export`
- `research-cli sessions rename`
- `research-cli sessions delete`
- `research-cli sessions prune`
- `research-cli sessions stats`

### Model, provider, auth, config

- `research-cli model list`
- `research-cli model current`
- `research-cli model set`
- `research-cli providers list`
- `research-cli providers inspect`
- `research-cli providers auth-status`
- `research-cli providers test`
- `research-cli providers refresh-catalog`
- `research-cli config get`
- `research-cli config set`
- `research-cli config edit`
- `research-cli config effective`
- `research-cli config sources`

### Permissions and usage

- `research-cli permissions mode`
- `research-cli permissions pending`
- `research-cli permissions history`
- `research-cli usage`
- `research-cli cost`
- `research-cli stats`

### Memory and repo governance

- `research-cli memory query`
- `research-cli memory explain`
- `research-cli memory status`
- `research-cli memory invalidate`
- `research-cli artifacts list`
- `research-cli artifacts inspect`
- `research-cli repo cleanup-plan`
- `research-cli repo cleanup-apply`

### Agent, branch, review, skill, MCP

- `research-cli agents list`
- `research-cli agents inspect`
- `research-cli agents stop`
- `research-cli agents traces`
- `research-cli branches list`
- `research-cli branches inspect`
- `research-cli branches cancel`
- `research-cli branches promote`
- `research-cli reviews list`
- `research-cli reviews inspect`
- `research-cli reviews open`
- `research-cli reviews retry`
- `research-cli skills list`
- `research-cli skills inspect`
- `research-cli skills paths`
- `research-cli skills validate`
- `research-cli plugins list`
- `research-cli plugins inspect`
- `research-cli plugins validate`
- `research-cli hooks list`
- `research-cli hooks inspect`
- `research-cli hooks test`
- `research-cli mcp list`
- `research-cli mcp inspect`
- `research-cli mcp test`
- `research-cli mcp refresh`
- `research-cli setup status`
- `research-cli setup migrate-check`
- `research-cli setup repair-hints`
- `research-cli setup install-routes`

### Remote host

- `research-cli remote pair`
- `research-cli remote status`
- `research-cli remote attach`
- `research-cli remote handoff`
- `research-cli remote takeover`
- `research-cli remote notify`

## 4. Frozen JSON Output Contract

The following commands must support machine-readable JSON output:

- `prompt`
- `inspect`
- `compact`
- `projects *`
- `sessions *`
- `model *`
- `providers *`
- `config effective/sources/get`
- `permissions *`
- `usage`
- `cost`
- `memory query/explain/status`
- `artifacts list/inspect`
- `branches *`
- `reviews *`
- `agents *`
- `skills *`
- `plugins *`
- `hooks *`
- `mcp *`
- `setup *`
- `remote pair/status/attach/handoff/takeover/notify`
- `doctor`
- `conformance`

If a surface is important to automation, it cannot be text-only.

## 4.5 Frozen Config Precedence Contract

Runtime configuration must resolve in a deterministic order.

Later entries override earlier ones:

1. built-in defaults
2. global user config
3. project config
4. project-local private config
5. environment variables
6. explicit CLI flags
7. one-turn inline overrides

The kernel must expose both:

- the effective merged config
- the source of every overridden field

This is required to match Claw/Crush-grade operator predictability.

Further hardening requirements for boot preflight, provider/auth explainability,
session identity/lineage, degraded feature reporting, and event-first kernel
state are specified in
`docs/deep_study/21-code-agent-kernel-hardening.md`.

The exact command grammar, exit codes, JSON envelopes, and feature-graduation
rules are frozen in `docs/deep_study/23-cli-operator-contract.md`.

## 4.6 Provider And Auth Routing Contract

Provider routing must be explicit rather than heuristic-only.

### Resolution order

1. explicit CLI provider/model flag
2. inline turn override
3. explicit config provider/model
4. model-prefix routing
5. credential-driven fallback
6. default provider

### Required rules

- model aliases resolve before provider inference
- model-name prefixes may force provider routing
- explicit provider beats ambient credentials
- proxy/base-url configuration is provider-scoped
- conflicting credentials must surface an explainable resolution result
- failed auth should return a concrete fix hint, not generic failure

### `ProviderResolution`

```text
ProviderResolution {
  requested_model
  resolved_model
  requested_provider?
  resolved_provider
  resolution_reason
  auth_source
  base_url_source
  degraded
}
```

## 4.7 Proxy And Endpoint Contract

The kernel must support both direct providers and proxy/gateway endpoints.

Required rules:

- provider-scoped base URLs
- unified proxy URL or per-scheme proxy config
- explicit `NO_PROXY` handling
- empty proxy values count as unset
- invalid proxy config should fail with a diagnostic, not silently mutate routing semantics

## 4.8 Session Default And Resume Contract

The kernel must freeze what happens when no explicit session target is given.

Required rules:

- plain `research-cli` starts a fresh interactive chat unless `--continue` is set
- `resume` accepts exact session ID, alias, or `latest`
- `continue` resumes the most recent matching session in the current project scope
- browse/search surfaces must resolve ambiguities before resume
- resume by title or alias must degrade safely when ambiguous

## 4.9 REPL / Slash / Help Contract

The TUI/REPL must publish concrete interactive semantics.

Required rules:

- slash commands are first-class operator surfaces, not hidden aliases
- tab completion covers slash commands, model aliases, permission modes, recent sessions, projects, skills, and MCP servers
- `/help` and command palette must describe the same canonical surface
- help text must exist in both human-readable and machine-readable forms

Minimum slash surfaces:

- `/help`
- `/status`
- `/doctor`
- `/model`
- `/config`
- `/session`
- `/resume`
- `/projects`
- `/permissions`
- `/usage`
- `/cost`
- `/skills`
- `/plugins`
- `/hooks`
- `/mcp`
- `/agents`
- `/branches`
- `/reviews`
- `/memory`
- `/artifacts`
- `/repo`

## 4.10 Interrupt / Retry / Undo Contract

Daily operator behavior depends on clear control semantics.

Required rules:

- interrupt stops foreground execution and preserves a resumable state
- retry repeats the last turn under the same session and records a new event
- undo is only allowed for reversible local actions and must never imply hidden filesystem rollback
- provider/network interruption must produce resumable partial state instead of silent loss

## 4.11 Usage, Cost, And Stats Contract

Usage and cost are first-class kernel surfaces.

Required behaviors:

- per-turn token accounting
- per-session token and tool accounting
- per-project aggregated usage
- branch-batch cost summary
- degraded / estimated accounting must be labeled as such

### `UsageSummary`

```text
UsageSummary {
  scope
  input_tokens
  output_tokens
  tool_calls
  estimated_cost
  accounting_quality
}
```

## 5. Active Project Management Contract

Project management is a first-class capability, not an accidental side effect
of being inside one repository.

### `ProjectRegistryEntry`

```text
ProjectRegistryEntry {
  project_id
  workspace_root
  workspace_hash
  data_dir
  last_accessed_at
  init_state
  active_session_id?
  open_branch_count
  repo_health
}
```

### Required behaviors

- the kernel registers projects by workspace root
- recency is updated on every successful session start or resume
- init state is visible and queryable
- users can browse projects, not only sessions
- stale projects and dead data dirs can be pruned safely

This is the minimum needed to claim “active project management”.

## 6. Active Memory Query And Explain Contract

The design must not silently inject memory without an explain path.

### `MemoryQueryResult`

```text
MemoryQueryResult {
  query_id
  route
  matched_records[]
  total_budget_used
  degraded_reasons[]
}
```

### `MemoryExplainRecord`

```text
MemoryExplainRecord {
  record_id
  recall_reason
  matched_via
  source_artifacts[]
  source_span?
  hydrated_from_pointer
  confidence
  contested
  superseded
}
```

### Required behaviors

- every injected memory must be explainable
- pointer-first retrieval must expose the hydrated verbatim source
- summary-derived memories must expose supporting source spans or artifact links
- contested or superseded memory must be visible as such
- degraded retrieval must be visible to the user and the audit log

This is the minimum needed to claim “active memory querying” instead of
uninspected recall.

## 7. Silent Summary Promotion Contract

Silent summarization must not write directly into trusted durable memory.

Required stages:

1. produce candidate summary
2. attach supporting event/session/artifact references
3. run promotion rule
4. promote, reject, or mark contested

Candidate summaries must be stored separately from already-trusted durable
memory.

## 8. Worktree And Branch Lifecycle Contract

Branch search is not complete without filesystem lifecycle governance.

### Required branch lifecycle states

- `allocated`
- `running`
- `needs_refresh`
- `awaiting_review`
- `won`
- `lost`
- `archived`
- `gc_eligible`

### Required behaviors

- each executor branch gets an explicit workspace binding
- base revision is recorded at branch creation
- refresh/rebase policy is recorded before promotion
- merge conflict status is explicit, not implicit
- dead worktrees are garbage-collected through reviewed cleanup

This is the minimum needed to claim controlled grid-search-like exploration.

## 8.5 Golden Parity Fixtures

The kernel must publish golden fixtures for operator-grade workflows.

Minimum fixture families:

- provider/auth routing
- config precedence
- session resume/continue
- slash-command parsing
- permission prompt flows
- JSON output envelopes
- memory query/explain
- branch/worktree lifecycle
- cleanup-plan / cleanup-apply

No claim of parity or superiority is valid without these fixtures.

The exact fixture definitions live in
`docs/deep_study/17-operator-golden-fixtures.md`.

The exact submachine transitions and terminal outcomes they must assert are
frozen in `docs/deep_study/24-kernel-state-machine-contract.md`.

## 9. Repo Hygiene Contract

Artifact governance is necessary, but repo hygiene needs additional policy.

Required hygiene surfaces:

- duplicate-file detection
- stale-run directory detection
- orphan worktree detection
- superseded output cleanup plan
- destructive cleanup review gate

The cleanup engine must propose actions before applying them.

## 10. Skill Manifest Contract

Research skills must be discoverable and versioned, not just named in docs.

### `SkillManifest`

```text
SkillManifest {
  skill_id
  version
  stage
  inputs[]
  outputs[]
  artifact_family
  required_agents[]
  reviewer_policy
  citation_rules[]
  recovery_rules[]
}
```

Required behaviors:

- skills are discoverable from manifests
- manifests are schema-validated
- stage routing uses manifests, not ad hoc naming
- skill upgrades are versioned

## 11. TUI Discoverability Contract

The TUI must make the kernel visible and operable.

Required discoverability:

- command palette or slash-command list
- keyboard-accessible session browse/search
- visible pending permissions
- visible active agents and branches
- visible memory-injection/explain pane
- visible artifact family and cleanup queue
- visible usage/cost surface
- visible plugin and hook degradation state

The final host surface should extend beyond the local TUI:

- mobile/web/desktop remote control should be treated as a first-class host
  plane
- the implementation route should be a `Happy`-based remote substrate rather
  than a from-scratch relay shell
- research-native remote panels should expose ProjectOps, branches, reviews,
  and memory explain without creating a second hidden runtime

See `docs/deep_study/19-remote-host-control-plane.md`.

## 12. Acceptance Matrix

Before claiming parity or superiority, the kernel must pass these workflow
classes:

### Claw-class workflows

- doctor-first bring-up
- prompt + resume
- permission-mode transitions
- JSON output automation
- deterministic replay/parity harness

### Hermes-class workflows

- model/provider switching
- config mutation and persistence
- session browse/search/resume/title/export/prune
- usage/cost inspection
- memory status/config surfaces

### Crush-class workflows

- project registry and recency tracking
- workspace init flow
- MCP and skill registry inspection and refresh
- config precedence and effective view
- project-local session continuity

### Research-cli-specific workflows

- active memory query/explain
- candidate summary promotion
- worktree-backed branch search
- agent inspect/stop traceability
- review open/retry with blinding and trace refs
- artifact family cleanup plan
- doc/code/experiment repair-and-rerun loop
- debate trace and disagreement resolution

## 13. What “Exceeds The Baseline” Means

The design may claim it exceeds the baseline only when all three are true:

1. it matches Claw/Hermes/Crush on operator-facing kernel surfaces
2. it exceeds them on project memory, repo governance, and research contracts
3. it proves those claims through parity and acceptance tests, not prose alone

## 14. Debate As A First-Class Operator Surface

The kernel must treat debate as a visible operating mode, not only hidden
branch comparison.

Required behaviors:

- users can inspect debate state, active claims, and unresolved disagreements
- debate outputs must be traceable to evidence and review packets
- debate verdicts must record what was accepted, rejected, or left contested

This is necessary to claim native multi-agent advantage rather than “parallel
branches plus paperwork”.

The runtime policies behind memory retrieval, scheduler quotas, retries,
promotion authority, and research-stage gating are frozen in
`docs/deep_study/26-memory-branch-research-runtime-policy.md`.
