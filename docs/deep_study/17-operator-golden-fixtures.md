# Operator Golden Fixtures

This document turns the kernel parity contract into exact test vectors.

The goal is simple: an external reviewer should be able to decide whether
`research-cli` really matches or exceeds Claw, Hermes, OpenCode, and Crush
without reading implementation code or guessing runtime intent.

## 1. Why This Document Exists

`16-kernel-parity-contract.md` freezes the product surface.

This document freezes the expected behavior of that surface under conflict,
ambiguity, interruption, and degraded runtime conditions.

That is the missing bridge between "good architecture" and an
operator-grade CLI.

## 2. Reference-Derived Priorities

The fixture families are chosen from the strongest operator behaviors in the
reference projects:

- `claw-code` for config precedence, provider routing, permission rigor, and parity harness discipline
- `Hermes` for profile-aware startup, doctor-first diagnosis, session browse/resume, and broad operator UX
- `Crush` for project-local configuration, multi-provider productization, and project/session continuity
- `OpenCode` for TUI help discoverability, session persistence, and app/service boundary lessons

The final system must not merely have similar commands. It must make the same
classes of conflict explainable.

## 3. Fixture Format

Every golden fixture should be runnable in a deterministic harness and produce:

- human-readable terminal output
- machine-readable JSON output
- structured event assertions
- effective-config and source assertions when relevant

Recommended layout:

```text
tests/golden/
├── config/
├── provider/
├── sessions/
├── slash/
├── permissions/
├── memory/
├── debate/
└── doctor/
```

Each fixture should define:

```text
GoldenFixture {
  fixture_id
  command
  cwd
  env
  stdin?
  expected_exit_code
  expected_stdout_contains[]
  expected_json?
  expected_events[]
}
```

This document is extended by:

- `docs/deep_study/21-code-agent-kernel-hardening.md` for additional kernel
  fixture families
- `docs/deep_study/22-remote-command-and-fixture-contract.md` for remote/mobile/web
  fixture families

## 4. Config Precedence Fixtures

The config precedence contract is:

1. built-in defaults
2. global config
3. project config
4. project-local private config
5. environment variables
6. explicit CLI flags
7. one-turn inline overrides

### Fixture `config_precedence_model_override`

Purpose:

- prove that project config overrides global config
- prove that env overrides project config
- prove that CLI flags override env

Inputs:

```text
default model = anthropic/claude-sonnet
global model = openai/gpt-5
project model = openai/gpt-5.4
env RESEARCH_CLI_MODEL = qwen/qwen-max
command = research-cli prompt --model openai/gpt-5.4-mini "status"
```

Expected JSON:

```json
{
  "resolved_model": "openai/gpt-5.4-mini",
  "sources": {
    "model": "cli_flag"
  }
}
```

### Fixture `config_sources_workspace_private`

Purpose:

- prove that project-local private config can override shared project config
- prove that `config sources` names both files explicitly

Expected behavior:

- `config effective --json` resolves the private value
- `config sources --json` reports both the shared and private file paths

### Fixture `config_inline_turn_override`

Purpose:

- prove that a one-turn override changes only the current turn
- prove that the persisted session default is unchanged after the turn completes

Expected behavior:

- turn N uses override
- turn N+1 without override falls back to the prior session default

## 4.5 Profile-Aware Startup Fixtures

### Fixture `startup_profile_doctor_priority`

Inputs:

```text
research-cli --profile daily-focus
```

Expected behavior:

- profile resolution is exposed in preflight or launch JSON
- doctor/preflight runs before any expensive session action
- startup can explain whether the profile came from CLI, workspace, or saved
  default

### Fixture `startup_profile_conflict_hint`

Inputs:

```text
research-cli --profile daily-focus --profile deep-research
```

Expected behavior:

- conflicting profile intent fails fast with a typed diagnostic
- output explains the resolved precedence and why it failed
- no session is opened until the user disambiguates
- non-zero JSON uses `CommandFailure.data = PolicyRefusalResult`

### Fixture `startup_launch_trace_complete`

Expected behavior:

- `InteractiveLaunchResult` includes `preflight`, `project_trace`, and
  `profile_trace`
- operators and wrappers do not need separate `projects current` or profile
  queries just to understand the launch route

### Fixture `startup_json_exits_before_repl`

Expected behavior:

- `research-cli --json` returns one `InteractiveLaunchResult`
- `launch_disposition=inspect_exit`
- no REPL frame or interactive text is written to stdout
- `repl` event is absent because no interactive session was entered

## 5. Provider And Auth Routing Fixtures

These fixtures freeze how model aliases, model prefixes, ambient credentials,
and proxy/base URL settings interact.

### Fixture `provider_alias_then_prefix`

Inputs:

```text
alias smart = gpt-5.4
OPENAI_API_KEY set
ANTHROPIC_API_KEY set
command = research-cli prompt --model smart "ping"
```

Expected behavior:

- alias resolves first to `gpt-5.4`
- provider then resolves to `openai`
- ambient Anthropic credentials are ignored

Expected JSON subset:

```json
{
  "requested_model": "smart",
  "resolved_model": "gpt-5.4",
  "resolved_provider": "openai",
  "resolution_reason": "alias_then_model_prefix"
}
```

### Fixture `provider_explicit_beats_ambient`

Inputs:

```text
ANTHROPIC_API_KEY set
OPENAI_API_KEY set
command = research-cli prompt --provider anthropic --model gpt-5.4 "ping"
```

Expected behavior:

- explicit provider wins
- resolved model is either rejected as incompatible or mapped through an
  explicit compatibility rule
- the CLI must not silently send the request to OpenAI

### Fixture `provider_conflicting_credentials_hint`

Inputs:

```text
ANTHROPIC_AUTH_TOKEN looks like sk-ant-...
ANTHROPIC_API_KEY unset
command = research-cli providers test anthropic
```

Expected behavior:

- diagnostic fails clearly
- error explains that an API key was placed in the bearer-token slot
- suggested remediation names the exact variable to move

This mirrors one of the most valuable Claw operator protections.

### Fixture `provider_proxy_scoped`

Inputs:

```text
OPENAI_BASE_URL = https://proxy.example/v1
ANTHROPIC_BASE_URL = https://anthropic-proxy.example
command = research-cli providers auth-status --json
```

Expected behavior:

- provider-scoped base URLs remain independent
- JSON exposes the base URL source for each provider
- empty proxy values are treated as unset, not as a literal empty endpoint

### Fixture `prompt_turn_trace_complete`

Expected behavior:

- `TurnResult` includes `project_trace`, `profile_trace`, and `provider_trace`
- one-shot automation can reconstruct the exact routing decision from one JSON
  payload

## 6. Session Resume And Continue Fixtures

### Fixture `resume_latest_project_scoped`

Inputs:

- project A has sessions `a1`, `a2`
- project B has session `b1`
- current cwd belongs to project A
- command = `research-cli continue`

Expected behavior:

- resumes `a2`
- does not resume `b1`
- emits `event_name=session_resume` with `project_scope=project_a`

### Fixture `resume_ambiguous_alias`

Inputs:

- two sessions share alias `latest-paper`
- command = `research-cli resume latest-paper`

Expected behavior:

- CLI refuses blind resume
- browse/search UI or JSON ambiguity result is returned
- non-zero JSON still uses `CommandFailure` with
  `data=ResumeAmbiguityResult`
- no session state mutates until the user disambiguates

### Fixture `resume_after_interrupt`

Inputs:

- a long-running turn is interrupted during tool streaming
- command sequence:
  - `research-cli chat`
  - interrupt
  - `research-cli continue`

Expected behavior:

- partial assistant/tool state is preserved
- resume surfaces the interrupted step and whether retry is available
- the transcript does not lose already-written evidence

This is required to exceed text-only resume behavior.

### Fixture `compact_derived_view_refresh`

Expected behavior:

- `CompactResult` includes `resume_recap_ref`
- `derived_views_updated[]` lists refreshed resume/search/browse surfaces
- if a derived surface requires deferred rebuild, the command returns `11` and
  names it in `deferred_repairs[]`
- refreshed resume/search/browse surfaces remain explicitly derived and
  non-authoritative

### Fixture `session_terminal_reconciliation_under_transport_loss`

Inputs:

- one session emits contradictory lifecycle noise such as
  `completed -> idle -> error -> transport_down`

Expected behavior:

- the kernel publishes one canonical actionable terminal truth
- transport death after event burst becomes a typed uncertainty state rather
  than silently rewriting a prior terminal outcome
- duplicate or contradictory terminal events remain audit-visible but do not
  double-trigger downstream automation

### Fixture `session_creation_identity_complete`

Expected behavior:

- session creation emits stable title, workspace/worktree path, and scope or an
  explicit placeholder reason for any temporarily unavailable field
- no first-class session creation fixture is allowed to surface naked
  `(unknown)` or `(untitled)` values without typed explanation
- remote-capable or provider-resumable sessions also expose typed
  runtime/source affinity and transcript-source references

### Fixture `session_event_provenance_and_scope_binding`

Expected behavior:

- event output distinguishes `live_lane`, `test`, `healthcheck`, `replay`, and
  `transport` provenance
- owner/assignee and workflow scope are visible to downstream automation
- out-of-scope external churn does not appear indistinguishable from actionable
  product-runtime events

### Fixture `session_operator_logs_seq_continuity`

Expected behavior:

- `sessions logs --json` returns `SessionOperatorLogManifest`
- one request sequence exposes distinct request, response stream, response, and
  tool-result refs when available
- missing log artifacts are typed as absent or inconsistent instead of being
  silently omitted
- request sequence continuity is preserved across repeated turns

### Fixture `session_search_index_non_authoritative`

Expected behavior:

- `sessions search --json` returns typed hits from a rebuildable derived index
- the surfaced index metadata marks the source as non-authoritative
- stale index state may degrade discovery quality but may not cause mutation or
  resume against the wrong canonical session

## 7. REPL, Slash, And Help Fixtures

### Fixture `slash_help_parity`

Inputs:

- `/help`
- command palette open
- `research-cli --help`

Expected behavior:

- all three surfaces refer to the same canonical command registry
- slash commands are not hidden aliases with missing docs
- the machine-readable help export matches the human-readable help listing

### Fixture `slash_parse_model_switch`

Inputs:

```text
/model set gpt-5.4
/model current
```

Expected behavior:

- parser captures the slash namespace, verb, and argument separately
- the current session default changes
- the command emits `event_name=config` with source `slash_command`

### Fixture `slash_parse_memory_explain`

Inputs:

```text
/memory explain mem_123
```

Expected behavior:

- the explain pane or JSON response includes source artifacts, pointer path,
  support spans if present, confidence, and superseded/contested flags

### Fixture `repl_completion_visibility`

Expected completion families:

- slash commands
- models and aliases
- projects
- sessions
- permission modes
- skills
- MCP servers

This is required to match or exceed OpenCode/Crush TUI discoverability.

## 8. Interrupt, Retry, And Undo Fixtures

### Fixture `interrupt_foreground_tool`

Inputs:

- running shell tool within a turn
- operator sends interrupt

Expected behavior:

- tool is cancelled or marked detached according to policy
- turn state becomes resumable
- the event log records `event_name=turn` and `event_name=tool` with terminal
  interruption outcomes

### Fixture `retry_same_turn_new_event`

Expected behavior:

- `retry` replays the prior request shape under the same session
- a new turn ID is created
- old and new result traces remain distinguishable

### Fixture `undo_nonreversible_rejected`

Inputs:

- prior turn created files and ran external commands
- operator requests `undo`

Expected behavior:

- CLI explains that hidden filesystem rollback is not supported
- if a cleanup plan is possible, it is proposed explicitly instead

This protects operator trust.

## 9. Permission And Worktree Fixtures

### Fixture `permission_workspace_boundary`

Inputs:

- main workspace is `/repo`
- attempted write path is `/tmp/outside.txt`
- permission mode is `workspace-write`

Expected behavior:

- write is denied or escalated
- denial explains the boundary rule
- event log records the rejected path and policy source

### Fixture `branch_worktree_narrower_than_main`

Inputs:

- main session permission mode = `workspace-write`
- spawned branch workspace under detached worktree

Expected behavior:

- branch defaults to a narrower permission envelope unless explicitly elevated
- promotion requires director approval even if branch writes succeeded locally

## 10. Memory Explain Fixtures

### Fixture `memory_pointer_first_hydration`

Inputs:

- query hits a summary candidate that points to a session span and an artifact

Expected behavior:

- initial result is pointer-first
- hydration step exposes exact supporting source paths
- if support is missing, the record is downgraded or contested

### Fixture `memory_superseded_visible`

Expected behavior:

- superseded memory is still queryable for audit
- auto-injection excludes it by default
- explain output marks the supersession chain explicitly

### Fixture `memory_promotion_state_legality`

Expected behavior:

- a memory record may move only through the lifecycle frozen in `24`
- `candidate -> supported -> trusted` succeeds only when support refs and
  promotion authority are present
- illegal jumps such as `candidate -> trusted` or `invalidated -> trusted`
  fail structurally and surface the violating transition

### Fixture `memory_invalidation_after_rollback`

Inputs:

- a trusted memory record points to a canonical artifact or transcript span
- rollback restores an older canonical state or changes the canonical artifact
  pointer

Expected behavior:

- dependent memory becomes `invalidated` or `contested` before the next
  auto-inject cycle
- `memory explain --json` still exposes provenance and invalidation reason
- rollback does not silently leave stale trusted memory active

### Fixture `projectops_digest_promotion_traceable`

Expected behavior:

- a `ProjectOpsTick` that creates a digest candidate records exactly one
  `tick_id`
- promoted digest output names supporting events, artifacts, or session spans
- promoted digest also resolves to a promoted `MemoryRecord` lineage entry

### Fixture `projectops_digest_rejected_typed`

Expected behavior:

- digest rejection remains inspectable rather than disappearing into logs
- rejection reason is structured and tied to the candidate and tick
- same tick does not silently retry the rejected digest

## 11. Debate Fixtures

### Fixture `debate_claim_rebuttal_chain`

Inputs:

- branch A asserts claim C1 with evidence E1
- branch B rebuts C1 with evidence E2
- branch A replies without evidence

Expected behavior:

- unsupported reply is marked weak or inadmissible
- contested state remains explicit if adjudication cannot resolve it
- debate trace records accepted, rejected, and contested claims separately

### Fixture `debate_no_direct_promotion`

Expected behavior:

- winning a debate does not directly promote artifacts
- promotion still requires review and repo-governance checks

## 11.5 Agent, Branch, And Review Fixtures

### Fixture `agent_stop_policy_surface`

Expected behavior:

- stopping a normal executor returns a typed stop outcome
- stopping a protected reviewer returns a policy-gated refusal when appropriate
- JSON includes stop mode and last trace ref

### Fixture `branch_inspect_stale_base_visibility`

Expected behavior:

- `branches inspect` exposes whether the branch or batch is stale against base
- stale-base state is explicit and blocks promotion eligibility until resolved

### Fixture `branch_promote_requires_review_gate`

Expected behavior:

- `branches promote` fails when evaluation exists but review gate is missing or
  failed
- failure includes the blocking review or policy ref

### Fixture `review_open_fresh_thread_blinded`

Expected behavior:

- newly opened review records `fresh_thread=true` by default
- banned executor interpretation is absent from the packet when reviewer policy
  requires blinding

### Fixture `review_retry_preserves_compare_linkage`

Expected behavior:

- retrying a compare-mode review preserves `compare_against`
- review retry emits a new review run while keeping the same logical linkage

## 11.7 Remote Projection And Revocation Fixtures

### Fixture `remote_projection_mismatch_regenerates`

Inputs:

- remote workbench holds a stale `RemoteProjectionSnapshot`
- canonical project-local state changes under `.pmcli/`
- operator runs `remote status` or follow replay

Expected behavior:

- stale projection is treated as derived and non-authoritative
- project-local kernel truth wins immediately
- projection is regenerated instead of treated as a conflict in canonical state

### Fixture `remote_revocation_visible_and_immediate`

Inputs:

- one bound remote client loses lease or is operator-revoked

Expected behavior:

- control is removed immediately
- `remote_lease_revoked` or `remote_binding_revoked` is emitted
- `remote status` exposes revocation reason and time without log scraping

### Fixture `remote_action_rejected_typed`

Inputs:

- remote client submits a stale or disallowed mutating action

Expected behavior:

- action is rejected with a typed refusal
- kernel emits `event_name=remote_action_rejected`
- JSON uses `CommandFailure.data = RemoteActionRejection`
- queued follow/projection state stays readable, but no hidden mutation occurs

### Fixture `remote_action_union_payload_legality`

Expected behavior:

- each `RemoteAction` carries exactly one typed payload field matching
  `action_type`
- generic payload blobs or multi-payload collisions fail conformance
- typed rejection names the mismatched action family rather than returning a
  generic parse failure

### Fixture `remote_attach_inspect_vs_execute`

Expected behavior:

- inspect mode returns `AttachEligibility` and does not mutate control owner
- execute mode returns `AttachExecutionResult` with
  `runtime_descriptor_ref`, `projection_ref`, and advanced ownership epoch
- machine-readable output distinguishes inspect from execute without relying on
  shell prose
- inspect also exposes whether the chosen path is `vendor_resume` or
  `happy_attach` when applicable
- execute may not silently change source affinity after inspection without
  surfacing the new typed reason

### Fixture `remote_handoff_inspect_vs_execute`

Expected behavior:

- inspect mode returns `HandoffEligibility`
- execute mode returns `HandoffExecutionResult`
- `previous_owner` and `new_owner` are structured owner objects, not strings
- runtime descriptor and projection continuity are preserved across handoff

### Fixture `remote_local_keypress_reclaim`

Inputs:

- remote mobile/web client currently owns control after attach or handoff
- local operator presses a key to reclaim the session

Expected behavior:

- local reclaim increments `ownership_epoch`
- remote mutating clients are degraded to follow-only until they re-request
  control
- `remote status --json` exposes the new control owner immediately
- reclaim obeys `LocalControlCapability.topology` and `attach_strategy`
- reclaim preserves session identity, runtime descriptor, and transcript-source
  continuity

### Fixture `remote_notify_payload_and_receipts`

Expected behavior:

- `remote notify --json` returns a canonical `RemoteNotificationPayload`
  together with delivery receipts
- receipt records point back to the emitted notification id
- typed notification kinds such as `permission_pending` and `wake_event`
  remain machine-readable

### Fixture `remote_cached_read_model_non_authoritative`

Expected behavior:

- stale cached remote browse/index data is surfaced as degraded browse state
- mutating commands re-resolve against canonical bindings and ownership epochs
  before acting
- cached session/channel indexes are marked derived and non-authoritative
- mismatch triggers regeneration or typed refusal instead of shadow arbitration

### Fixture `remote_feature_advertisement_surface`

Expected behavior:

- `remote status` returns typed feature advertisements rather than generic
  health labels
- advertised feature families include enablement, policy mode, and reason when
  degraded

## 12. Doctor And Setup Fixtures

### Fixture `projects_current_resolution_trace`

Purpose:

- prove that current-project resolution is machine-explainable instead of an
  implicit cwd side effect

Inputs:

- explicit `--project` omitted
- cwd matches multiple historical registry candidates or differs from the saved
  current pointer
- command = `research-cli projects current --json`

Expected behavior:

- JSON returns `CommandSuccess.data = ProjectResolutionTrace`
- result names the winning source such as `cwd`, `explicit_project`, or
  `current_pointer`
- losing candidates and warnings are visible when ambiguity pressure exists

### Fixture `projects_current_unresolved_typed`

Expected behavior:

- unresolved project exits with `6`
- JSON returns `CommandFailure.data = ProjectResolutionTrace`
- trace shows why resolution failed rather than only a string error

### Fixture `doctor_provider_matrix`

Purpose:

- mirror Hermes-style operator bring-up confidence

Expected checks:

- config file presence and readability
- provider credential availability
- provider endpoint sanity
- MCP registry visibility
- writable local data paths
- schema/conformance line compatibility

### Fixture `doctor_blocked_typed_failure`

Expected behavior:

- blocked doctor exits non-zero
- JSON still uses `CommandFailure`
- `CommandFailure.data` carries `DoctorReport`
- repair hints remain structured and component-specific

### Fixture `smoke_mutation_free`

Expected behavior:

- smoke writes only under `.pmcli/`
- `SmokeResult.source_mutation_free=true`
- `SmokeResult.mutated_paths[]` contains no user source paths
- provider/session/event outputs remain inspectable from the same payload

### Fixture `setup_status_payload_complete`

Expected behavior:

- `setup status --json` returns `SetupStatusReport`
- report includes schema version, data directories, and last migration summary
- missing install state is explicit rather than silently omitted

### Fixture `setup_install_routes_platform_surface`

Expected behavior:

- `setup install-routes --json` returns `InstallRoutesReport`
- provider, skill, MCP, plugin, hook, remote, and `.pmcli/` routes are all
  named explicitly
- platform-specific route differences are surfaced in data, not hidden in text

### Fixture `doctor_fix_hint_quality`

Expected behavior:

- failure output contains concrete remediation commands or file paths
- generic "something went wrong" text is not acceptable

### Fixture `feature_not_graduated_typed`

Expected behavior:

- a not-yet-graduated command exits `8`
- JSON uses `CommandFailure.data = FeatureGateResult`
- payload names missing capability and target milestone

### Fixture `degraded_plugin_followup_typed`

Expected behavior:

- degraded-but-loadable plugin validation exits `11`
- JSON uses `CommandFailure.data = FollowupRequiredResult`
- payload distinguishes validated parts from deferred repairs

### Fixture `skills_registry_typed`

Expected behavior:

- `skills list --json` returns `SkillListResult`
- `skills inspect --json` returns `SkillInspectResult`
- `skills paths --json` returns `SkillPathsResult`
- clean `skills validate --json` returns `SkillValidationResult`
- degraded skill validation still uses `CommandFailure.data = FollowupRequiredResult`

### Fixture `plugin_hook_registry_typed`

Expected behavior:

- `plugins list --json` returns `PluginListResult`
- `plugins inspect --json` returns `PluginInspectionResult`
- clean `plugins validate --json` returns `PluginValidationResult`
- `hooks list --json` returns `HookListResult`
- `hooks inspect --json` returns `HookInspectionResult`
- `hooks test --json` returns `HookTestResult`
- unsafe hook-test mode returns typed refusal data instead of shell prose

### Fixture `mcp_registry_typed`

Expected behavior:

- `mcp list --json` returns `MCPListResult`
- `mcp inspect --json` returns `MCPInspectionResult`
- clean `mcp test --json` returns `MCPTestResult`
- degraded `mcp test` uses `CommandFailure.data = FollowupRequiredResult`
- `mcp refresh --json` returns `MCPRefreshResult`
- changed and degraded servers remain visible without log scraping

### Fixture `projects_prune_dry_run_typed`

Expected behavior:

- `projects prune --json` returns `PruneResult`
- dry-run lists candidate refs without mutating registry state
- apply conflicts remain explicit in `blocking_conflicts[]`

### Fixture `permissions_pending_history_typed`

Expected behavior:

- `permissions pending --json` returns `PermissionPendingList`
- `permissions history --json` returns `PermissionHistoryResult`
- empty lists stay valid and typed instead of collapsing to prose

### Fixture `conformance_result_typed`

Expected behavior:

- `research-cli conformance --json` returns `ConformanceResult`
- passed and failed fixture families are distinguishable
- failure refs are exposed without requiring log scraping

### Fixture `memory_status_typed`

Expected behavior:

- `memory status --json` returns `MemoryStatusReport`
- working-memory, durable-memory, and promotion-queue health are machine-readable
- degraded memory state is explicit in `degraded_reasons[]`

### Fixture `artifacts_list_inspect_typed`

Expected behavior:

- `artifacts list --json` returns `ArtifactListResult`
- `artifacts inspect --json` returns `ArtifactInspectionResult`
- canonical path, lineage, promotion history, and review links stay structured

### Fixture `repo_cleanup_plan_apply_typed`

Expected behavior:

- `repo cleanup-plan --json` and `repo cleanup-apply --json` return
  `RepoCleanupProposal`
- apply requires an explicit plan id and never silently recomputes
- stale-plan conflicts remain machine-readable

### Fixture `wake_event_escalation_typed`

Expected behavior:

- a stale or unacknowledged wake transitions into an explicit escalated state
- escalation records whether main-system or operator intervention is required
- wake output stays linked to the originating supervisor lease

### Fixture `research_stage_gate_repair_pivot_chain`

Expected behavior:

- a running stage that fails gate evaluation may route into `repairing` or
  `pivoting`, but only with explicit gate linkage
- repair and pivot outputs remain connected to the originating stage and
  change envelope
- successor stages do not inherit success or promotion state from the failed
  path

### Fixture `remote_pair_status_notify_takeover_typed`

Expected behavior:

- `remote pair --json` returns `RemotePairResult`
- `remote status --json` returns `RemoteStatusReport`
- `remote takeover --json` returns `RemoteTakeoverResult`
- `remote notify --json` returns `RemoteNotifyResult`
- remote feature exposure uses `feature_advertisements[]`, not drifting field names

## 13. Acceptance Standard

The kernel may claim parity or superiority only when:

1. all golden fixtures pass in deterministic CI
2. JSON and human-readable outputs agree on the same resolved state
3. degraded cases stay explainable instead of falling back to silent heuristics

If a behavior matters to daily operation, it needs a golden fixture.
