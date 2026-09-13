# Code Agent Kernel Hardening Blueprint

This document is the follow-up to `09-code-agent-cli-foundations.md`,
`16-kernel-parity-contract.md`, and `17-operator-golden-fixtures.md`.

Those documents established the direction.

This document hardens the kernel to the point where the final product can
credibly exceed `Hermes`, `claw-code`, and `Crush` as a **code agent CLI**
before the research-specific layers are counted.

## 1. Why This Document Exists

The architecture is already strong on:

- project memory
- multi-agent orchestration
- research workflows
- remote workbench direction

But the user requirement is stricter than that:

- the system must first be an excellent code agent CLI
- the custom research/memory features only count if the base CLI is already
  stronger than `Hermes` and `claw-code`

That means we need a deeper kernel contract for:

- operator-grade startup and recovery
- provider/auth correctness
- session identity and lineage
- permission and workspace rigor
- plugin/MCP degraded-state reporting
- deterministic parity and regression harnesses

## 2. What The References Prove At Code Level

## 2.1 `Hermes`

What matters from the actual docs and runtime shape:

- one shared runtime resolver is used across CLI, gateway, cron, ACP, and
  auxiliary calls
- session persistence is treated as a product surface, not just an internal log
- session search uses SQLite + FTS with source tagging and lineage
- rollback/checkpoint behavior is integrated into operator UX
- the command surface is broad and operational, not just conversational

The problem is not lack of capability.

The problem is that too much of the product lives in a monolithic runtime
surface.

So the Hermes lesson is:

- keep the broad operator surface
- keep the shared provider/session/runtime core
- keep session lineage, search, recap, and rollback
- do not let the kernel collapse into one giant entrypoint

## 2.2 `claw-code`

What matters from the actual workspace and parity docs:

- `doctor` is treated as the first run, not an afterthought
- config precedence and workspace-local scope are explicitly tested
- provider/model routing and credential-shape mistakes are surfaced with fix
  hints
- permission modes are first-class and tool enforcement is structural
- workspace-scoped session persistence is enforced
- parity is verified through a deterministic mock service and scripted harness
- degraded plugin/MCP state is surfaced structurally instead of being buried in
  logs
- roadmap language is event-first, machine-readable, and recovery-oriented

The core Claw lesson is:

- product truth must be testable through executable fixtures
- machine consumers should not scrape prose to understand runtime state

## 2.3 `OpenCode -> Crush`

What matters from the actual docs and tests:

- OpenCode established the session-based, TUI-first, auto-compact line
- Crush turns that into stronger app/service/config/runtime separation
- provider/model parsing is validated under ambiguity and slash-heavy model IDs
- config scope guards are explicit between global and workspace writes
- project registration is a first-class operator surface
- LSP/MCP/provider/config/session are treated as runtime services, not bolt-ons

The Crush lesson is:

- host UX can be rich, but runtime truth still needs clear service boundaries
- workspace and global scope must be guarded explicitly

## 3. The New Kernel Target

The target kernel should be stronger than the references on four axes at once:

1. stronger runtime contracts than `Hermes`
2. stronger deterministic operator verification than `claw-code`
3. stronger service decomposition than `OpenCode`
4. stronger productized workspace/project semantics than `Crush`

This means the kernel must now be frozen as:

- event-first
- workspace-scoped
- lineage-aware
- permission-explicit
- preflighted before execution
- parity-tested through machine fixtures
- degraded-state explainable

## 4. Frozen Hardening Areas

## 4.1 Boot And Preflight Contract

The CLI must not treat "process started" as "runtime ready".

Before a session or long-running worker begins, the kernel should emit a
`RuntimePreflightReport`.

## `RuntimePreflightReport`

```text
RuntimePreflightReport {
  project_id
  workspace_root
  session_target?
  provider_status[]
  auth_status[]
  sandbox_status
  plugin_status[]
  hook_status[]
  mcp_status[]
  config_staleness
  recovery_hint[]
  ready
}
```

Required checks:

- workspace root exists and is writable if required
- session target is valid for this workspace
- provider/auth combination is usable
- sandbox policy is known
- plugin discovery is healthy or structurally degraded
- hook registration is healthy or structurally degraded
- MCP servers are available, pending, or structurally degraded
- config sources are current or explicitly stale

The system should not start expensive work when preflight already knows the
lane is doomed.

## 4.2 Provider/Auth Resolution Must Be Explainable

This area is where `Hermes`, `claw-code`, and `Crush` together set the bar.

The kernel must freeze:

- alias resolution
- provider-prefix routing
- credential-shape validation
- provider-scoped base URLs
- explicit-provider override behavior
- ambiguity handling

## `ProviderResolutionTrace`

```text
ProviderResolutionTrace {
  requested_model
  requested_provider?
  model_alias_applied?
  routed_by_prefix?
  resolved_provider
  resolved_model
  auth_source
  auth_shape
  base_url
  base_url_source
  degraded
  warnings[]
}
```

Required rules:

- alias resolution happens before provider inference
- provider/model prefixes beat ambient credentials
- explicit provider beats ambient credentials
- wrong-shaped credentials return targeted remediation hints
- incompatible provider/model combinations fail explicitly
- empty proxy/base-url values are treated as unset

This is the minimum bar to beat the existing projects on operator trust.

## 4.3 Session Identity, Scope, And Lineage Contract

The kernel must combine the best of Hermes session richness and Claw workspace
discipline.

Required rules:

- sessions are project-scoped by default
- session search/list/export are first-class commands
- sessions carry lineage across compaction and branch evolution
- child/tool/internal sessions cannot be resumed as normal top-level sessions
- session summaries and recaps are operator-visible
- session workspace mismatch is rejected explicitly

## `SessionIdentity`

```text
SessionIdentity {
  session_id
  project_id
  workspace_root
  title?
  source
  session_kind
  parent_session_id?
  compacted_from?
  created_at
  active
}
```

## `session_kind`

Allowed values should include:

- `interactive`
- `one_shot`
- `compact_child`
- `agent_internal`
- `tool_sidechain`
- `review`
- `branch`
- `system`

The CLI should only resume kinds that are safe to resume as operator sessions.

## 4.4 Compaction And Continuity Must Be Safer Than Resume Alone

OpenCode and Hermes prove compaction is necessary.

Claw proves session-local persistence and resume shortcuts matter.

Our kernel should therefore freeze:

- compaction is lineage-preserving, not destructive overwrite
- post-compaction resumes preserve canonical project identity
- compacted summaries are inspectable
- the system can explain why a session resumed from a child or latest alias
- durable operator logs stay correlated across request, stream, response, and
  tool-result artifacts
- session search/index layers remain rebuildable read models instead of
  alternate truths

## `SessionOperatorLogManifest`

```text
SessionOperatorLogManifest {
  session_id
  project_id
  request_seq
  request_log_path?
  response_stream_path?
  response_log_path?
  tool_results_log_path?
  transcript_source
  consistency_state
  recorded_at
}
```

This follows the strongest OpenCode lesson:

- request/response/tool-result logs should be explicit session artifacts
- sequence continuity must be inspectable
- they support debugging and replay, but they do not replace canonical event
  persistence

The newer `Meta-Harness` and `Ruah` passes make this even more important:

- `Meta-Harness` needs comparable run artifacts and ledgers for outer-loop
  improvement
- `Ruah` needs durable artifacts for task takeover, retry, and workflow resume

So operator logs are not only a debug convenience.

They are part of the self-improvement and multi-agent substrate.

## `DerivedSessionReadModel`

```text
DerivedSessionReadModel {
  model_id
  project_id
  model_kind
  storage_path
  authoritative
  rebuildable
  refreshed_at
}
```

Allowed `model_kind` values should include:

- `session_search_index`
- `gateway_session_index`
- `channel_directory_cache`

Required rule:

- every session browse/search cache must declare itself
  `authoritative=false` and `rebuildable=true`
- mutating commands may consult these read models for discovery, but must
  re-resolve against canonical session/project/kernel state before acting

## `SessionLineageRecord`

```text
SessionLineageRecord {
  parent_session_id
  child_session_id
  trigger
  summary_ref
  created_at
}
```

## 4.5 Permission And Workspace Enforcement Must Stay Structural

This remains one of Claw's strongest contributions.

The kernel must preserve explicit permission modes:

- `read-only`
- `workspace-write`
- `danger-full-access`

But it should go further by integrating them with project/research semantics.

## `PermissionDecisionTrace`

```text
PermissionDecisionTrace {
  tool_name
  action
  requested_path?
  required_mode
  current_mode
  allowlist_match?
  workspace_boundary_ok
  branch_boundary_ok
  destructive
  approved
  reason
}
```

Required behavior:

- file writes enforce workspace boundaries
- branch/worktree-local writes enforce branch boundaries
- destructive bash/file actions are classified explicitly
- approval history is queryable
- remote approval must use the same decision path, not a shadow one

## 4.6 Plugin And MCP Degraded State Must Be Structured

This is a place where existing systems are still uneven.

The kernel should freeze the idea of runtime feature status rather than simple
"enabled/disabled".

## `RuntimeFeatureStatus`

```text
RuntimeFeatureStatus {
  feature_kind
  feature_id
  status
  phase
  error_code?
  error_message?
  actionable_hint?
}
```

`status` should allow at least:

- `ready`
- `pending`
- `degraded`
- `blocked`
- `disabled`

This should apply to:

- plugins
- MCP servers
- LSP integrations
- remote host features
- background project ops

This is how the CLI becomes operable without digging into logs.

## 4.7 Rollback, Checkpoint, And Worktree Safety Need To Be Native

Hermes proves rollback/checkpoint UX matters.

The kernel should adopt:

- automatic pre-mutation snapshots
- rollback diff before restore
- pre-rollback safety snapshot
- worktree-aware isolation recommendations

But unlike Hermes, this should integrate with:

- artifact family governance
- project memory invalidation
- branch lineage

Rollback should not just restore files.

It should also emit:

- artifact invalidation events
- stale-memory invalidation candidates
- branch divergence notices if relevant

## 4.8 Event-First State Must Replace Log-First Inference

This is the deepest kernel shift required to surpass the references.

The kernel should publish one canonical event contract that covers:

- preflight
- provider resolution
- session lifecycle
- permission lifecycle
- tool lifecycle
- plugin/MCP/LSP degraded state
- compaction
- rollback
- remote control ownership
- branch/review/research extensions

## `KernelEventEnvelope`

```text
KernelEventEnvelope {
  seq
  event_name
  phase
  terminal_outcome?
  object_kind
  object_id
  project_id
  session_id?
  timestamp
  payload
}
```

This section must stay aligned with the later canonical event contract frozen
in `24-kernel-state-machine-contract.md`.

Important rule:

- `24-kernel-state-machine-contract.md` is the sole canonical owner of the
  persisted event envelope and `event_name` registry
- this document may explain why the event line matters, but it may not add
  optional fields, aliases, or a competing envelope shape
- event identity is carried by monotonically increasing `seq`; first release
  does not add a second persisted `event_id`

This should be the source for:

- TUI rendering
- JSON output
- remote projection
- conformance assertions
- silent summarization

## 4.8.1 Durable Logs Are Artifacts, Not Shadow State

Hermes and OpenCode both show that broad CLI products need durable logs for
real operator debugging.

The design should now freeze:

- session-scoped durable log artifacts
- request-sequence continuity
- session-context injection at record creation time
- explicit distinction between canonical events and auxiliary logs

Required rules:

- every process-wide log record should be able to carry session correlation at
  record creation time, not only through handler-local filters
- durable session log artifacts are append-only debug evidence tied to
  `SessionOperatorLogManifest`
- if a durable log artifact disagrees with canonical event state, the event
  stream wins and the log is treated as inconsistent evidence rather than truth
- inspect/export surfaces may expose these artifact paths, but resume and
  mutation logic may not infer authority from them

## 5. What “Exceeds The References” Now Means

The base CLI should now beat the references in a visible way.

## Better than `Hermes`

Because we keep the broad product surface, but we add:

- smaller runtime boundaries
- explicit degraded-state contracts
- workspace-scoped authoritative persistence
- fixture-grade operator verification
- durable session log manifests without turning logs into runtime truth
- read-model discipline for browse/search routing instead of monolithic storage

## Better than `claw-code`

Because we keep:

- doctor-first discipline
- deterministic parity harnessing
- permission rigor
- config precedence and provider-routing correctness

and add:

- richer project/session lineage
- rollback + memory/artifact coherence
- stronger native project management
- typed durable operator logging and session-context correlation

## Better than `Crush`

Because we keep:

- service decomposition
- session continuity
- provider/model ambiguity handling
- workspace/global config guards

and add:

- stronger permission semantics
- stronger parity/conformance harnesses
- stronger operator-visible degradation and recovery
- transactional read-model and file-lineage discipline instead of ad hoc
  versioning

## 6. New Required Golden Fixture Families

`17-operator-golden-fixtures.md` should no longer be enough by itself.

We now need golden coverage for:

- boot preflight and blocked-start cases
- provider credential-shape errors
- provider prefix vs ambient credential conflicts
- workspace-mismatch session resume failure
- child/tool/internal session resume rejection
- plugin degraded mode
- MCP degraded mode
- config staleness detection
- rollback metadata invalidation side effects
- event ordering and canonical terminal outcome

## 7. Implementation Consequence

The kernel should now be implemented as these explicit units:

- `runtime/preflight`
- `runtime/provider_resolution`
- `session/identity`
- `session/index`
- `plugins/registry`
- `plugins/hooks`
- `session/lineage`
- `permissions/policy`
- `runtime/features`
- `runtime/events`
- `workspace/checkpoint`

This is the minimum decomposition needed to keep the system stronger than the
references once research/memory layers are added.

## 8. Final Rule

From this point onward, no advanced feature should be allowed to bypass the
kernel contracts in this document.

That means:

- multi-agent debate must still emit canonical kernel events
- project memory writes must still respect session/project lineage
- remote/mobile/web actions must still pass through preflight, permission, and
  feature-status gates
- research skills must still live on top of session/workspace/provider truth

That is how the final product remains a great code agent CLI instead of a
research system built on a shaky shell.

For implementation-grade transition tables and cross-machine invariants, see
`docs/deep_study/24-kernel-state-machine-contract.md`.
