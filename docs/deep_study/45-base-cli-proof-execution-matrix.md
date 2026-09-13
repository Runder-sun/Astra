# Base CLI Proof Execution Matrix

This document turns `44-base-cli-superiority-proof-gate.md` into an executable
delivery matrix.

Its purpose is to answer one practical question:

- if we want the base CLI to eventually exceed `Hermes`, `claw-code`, and
  `OpenCode/Crush`, what must be implemented, in what order, and what counts as
  passing evidence?

This document does not replace:

- `31-kernel-foundation-implementation-plan.md`
- `32-first-build-task-pack-backlog.md`
- `38-m0-m2-pr-batch-and-review-plan.md`

Instead, it overlays them with a proof-first execution lens.

## 1. Execution Rule

No workflow below may be marked complete unless all of these are true:

1. command and payload contract exists
2. schema owner exists
3. fixture or conformance owner exists
4. implementation exists
5. a pass artifact can be shown

Status ladder:

- `design_frozen`
- `schema_ready`
- `fixture_ready`
- `implemented_and_passing`
- `implemented_and_passing_for_project_memory`
- `implemented_and_passing_for_project_memory_and_supervision`
- `feature_gated`

`feature_gated` means the command is intentionally visible as a typed,
machine-readable future surface, but it is not part of the currently graduated
terminal claim for its bounded implementation scope.

`implemented_and_passing_for_project_memory` means the M7 bounded working
memory slice and the M8 project-local durable promote/query/explain/invalidate
slice are implemented and passing, including the local deterministic hybrid
retrieval backend, embedded hashed-vector retrieval, and local temporal-graph
successor routing. This does not require an external vector database or a
Neo4j/FalkorDB deployment.

`implemented_and_passing_for_project_memory_and_supervision` additionally means
ProjectOps lease/wake supervision has duplicate-owner, stale, reclaim,
no-owner escalation, and wake-transition coverage.

## 2. Claw-Class Matrix

| Workflow | Command / Payload line | Schema / Type owner | Fixture / Conformance owner | Planned landing lane | Pass evidence | Current status |
|---|---|---|---|---|---|---|
| Doctor-first bring-up | `doctor`, `RuntimePreflightReport`, `DoctorReport`, `CommandFailure` | `23`, `37`, `28` | `17`, `29`, `31` Task 6, Batch 6 | Pack D, Batch 6 | ready/degraded/blocked JSON plus fixtures | `implemented_and_passing` |
| Prompt plus resume continuity | `prompt`, `chat`, `resume`, `continue`, `TurnResult`, `ResumeResult`, `CompactResult` | `23`, `37`, `28` | `17`, `29`, `31` Task 4-5, Batch 3-6 | Pack C, Batch 3-6 | interrupted-turn recovery, compact continuity, resume fixture | `implemented_and_passing` |
| Permission-mode rigor | `permissions mode`, `permissions pending`, `permissions history`, `PolicyRefusalResult`, `PermissionDecisionTrace` | `23`, `37`, `28` | `17`, `29`, `31` Task 7, Batch 7 | Pack E, Batch 7 | persisted pending/history plus refusal fixtures | `implemented_and_passing` |
| Machine-readable automation | `CommandSuccess`, `CommandFailure`, `HelpSurfaceReport`, `PaletteSurfaceReport` | `23`, `37`, `28` | `17`, `29`, `31` Task 4.5-5 | Pack C, Batch 4-5 | typed non-zero JSON and canonical help/palette fixture | `implemented_and_passing` |
| Deterministic parity or replay harness | mock-provider parity harness, replay coverage | `31`, `36`, `44` | `29`, `31`, `tests/mock_provider/` | Pack G + H, Batch 8-10 | deterministic parity run artifact | `implemented_and_passing` |

## 3. Hermes-Class Matrix

| Workflow | Command / Payload line | Schema / Type owner | Fixture / Conformance owner | Planned landing lane | Pass evidence | Current status |
|---|---|---|---|---|---|---|
| Provider and config product surface | `providers *`, `config *`, `ProviderResolutionTrace`, `EffectiveConfigReport`, `ConfigSourceReport` | `23`, `37`, `28` | `17`, `29`, `31` Task 8, Batch 8 | Pack F, Batch 8 | explainable provider/config decisions in one hop | `implemented_and_passing` |
| Session browse/search/title/export/prune | `sessions *`, `SessionBrowseResult`, `SessionSearchResult`, `SessionExportResult`, `ResumeAmbiguityResult` | `23`, `37`, `28` | `17`, `29`, `31` Task 4.5, Batch 4 | Pack C, Batch 4 | browse/search/export fixtures and ambiguity-safe resume | `implemented_and_passing` |
| Durable logs plus non-authoritative read models | `sessions logs`, `SessionOperatorLogManifest`, `DerivedSessionReadModel` | `23`, `37`, `28` | `17`, `29`, Batch 4-6 follow-up | Pack C extension, Batch 4-6 extension | request-seq continuity and non-authoritative index fixtures | `implemented_and_passing` |
| Usage and cost visibility | `usage`, `cost`, `stats`, `UsageSummary`, `StatsSummary` | `23`, `37`, `28` | `17`, `29`, `31` Task 8 | Pack F, Batch 8 | machine-readable accounting with degraded labels | `implemented_and_passing` |

## 4. Crush-Class Matrix

| Workflow | Command / Payload line | Schema / Type owner | Fixture / Conformance owner | Planned landing lane | Pass evidence | Current status |
|---|---|---|---|---|---|---|
| Project registry and scope resolution | `projects *`, `ProjectResolutionTrace`, `ProjectRegistryList`, `ProjectStatus` | `23`, `37`, `28` | `17`, `29`, `31` Task 3-4.5, Batch 2-4 | Pack B + C, Batch 2-4 | deterministic project resolution and ambiguity fixtures | `implemented_and_passing` |
| Workspace and session continuity | workspace resolver, current-project pointer, workspace mismatch refusal | `14`, `23`, `37`, `39` | `17`, `29`, `31` Task 3-4 | Pack B + C, Batch 2-4 | session scope cannot drift across workspaces/worktrees | `implemented_and_passing` |
| Native registries | `mcp *`, `skills *`, `plugins *`, `hooks *` | `23`, `37`, `28` | `17`, `29`, `31` Task 9, Batch 9-9.5 | Pack D/F extension, Batch 9-9.5 | inspect/test/refresh registry fixtures | `implemented_and_passing` |
| Config precedence and effective view | `config effective`, `config sources`, precedence contract | `16`, `23`, `37`, `28` | `17`, `29`, `31` Task 8, Batch 8 | Pack F, Batch 8 | precedence fixture plus effective-source reporting | `implemented_and_passing` |

## 5. Research-CLI Exceedance Overlay

These do not count toward "base CLI exceeds the references" until the
foundation matrix above is already green.

| Workflow | Command / Payload line | Schema / Type owner | Fixture / Conformance owner | Planned landing lane | Pass evidence | Current status |
|---|---|---|---|---|---|---|
| Project-native memory and ProjectOps | `memory append/status/promote/query/explain/invalidate`, `projectops status/lease/wake`, memory and supervision records | `18`, `23`, `24`, `26`, `37`, `28` | `17`, `29`, advanced plans | M7-M8 complete for project-local governed memory and supervision | bounded working memory, digest staging, durable promotion, hybrid/vector/temporal query, non-injecting invalidated/superseded states, cleanup-restore invalidation, duplicate/stale/reclaim/wake-transition ProjectOps fixtures | `implemented_and_passing_for_project_memory_and_supervision` |
| Repo governance | `artifacts *`, `repo cleanup-*`, artifact/repo cleanup records | `18`, `23`, `37`, `28` | `17`, `29`, advanced plans | M3+ | cleanup traceability and artifact-family fixtures | `implemented_and_passing` |
| Multi-agent, branch review, and verifier tournaments | `agents *`, `branches search/evaluate/debate/verify/promote`, `reviews *`, `VerifierTournament` | `15`, `23`, `26`, `37`, `28` | `17`, `29`, advanced plans, `tests/operator/cli_surfaces.rs`, `tests/conformance/schemas/batch10_enforcement.rs` | M4+M6+M9+M12 | traceable agent/branch/review fixtures plus criterion-level repeated pairwise verifier scores and verifier-gated promotion | `implemented_and_passing_for_branch_verifier_slice` |
| Research runtime | stage execution, repair/pivot, wake escalation | `15`, `24`, `26`, `37`, `28` | `17`, `29`, advanced plans | M10 | stage legality and supervision conformance | `design_frozen` |

## 6. Proof-First Batch Overlay

The existing M0-M2 sequence should be interpreted through this proof-first
overlay:

1. Batch 0-1: bootstrap compile, bundle truth, event truth
2. Batch 2-4: project/session identity and browse/search truth
3. Batch 5-7: shared runtime lane, inspectability, permissions
4. Batch 8-9.5: provider/config/accounting and native registries
5. Batch 10: conformance and golden CI enforcement

Special priority rule:

- the first claim-moving workflows are not the broadest features
- they are the workflows that unblock Claw-class and Hermes-class proof
- therefore `doctor`, `resume continuity`, `permission rigor`, `project scope`,
  `provider/config explainability`, and `sessions browse/search/logs` outrank
  any advanced memory or remote productization during foundation execution

## 7. Step-2 Immediate Start Line

If we begin implementation immediately after this document, the first practical
coding line should be:

1. Batch 0 bootstrap compile
2. Batch 1 bundle/event floor
3. Batch 2 project scope floor

Reason:

- these are the earliest batches that produce proof-bearing artifacts
- they do not depend on remote, memory, or advanced runtime surfaces
- they are the narrowest way to begin converting `conditional_go` into
  implementation-backed evidence

## 8. Current Terminal CLI Graduation Audit

Current command-registry evidence:

- registry owner: `src/commands/help.rs`
- machine contract: `schemas/help_surface_report.schema.json`
- operator coverage: `tests/operator/cli_surfaces.rs`
- schema/conformance coverage: `tests/conformance/schemas/batch10_enforcement.rs`
- live command: `research-cli help --json`

Each public command now exposes:

- `canonical_json_invocation`: the one preferred JSON entry path for automation
- `graduation_status`: `graduated` or `feature_gated`

| Command | Canonical JSON invocation | Graduation status | Current scope |
|---|---|---|---|
| `help` | `help --json` | `graduated` | canonical command registry |
| `palette` | `palette --json` | `graduated` | command-palette projection |
| `slash` | `slash help --json` | `graduated` | slash help projection |
| `chat` | `chat --json` | `graduated` | interactive launch inspection |
| `prompt` | `prompt status --json` | `graduated` | bounded non-interactive turn |
| `model` | `model current --json` | `graduated` | model selection |
| `resume` | `resume latest --json` | `graduated` | session resume |
| `continue` | `continue --json` | `graduated` | latest-session continuation |
| `inspect` | `inspect --json` | `graduated` | session/project inspection |
| `compact` | `compact latest --json` | `graduated` | deterministic compact baseline |
| `sessions` | `sessions list --json` | `graduated` | session browse/search/log/export/prune family |
| `projects` | `projects status --json` | `graduated` | project registry and canonicality family |
| `permissions` | `permissions pending --json` | `graduated` | permission state machine |
| `goals` | `goals status --json` | `graduated` | MissionFrame status |
| `tools` | `tools run read_file --path README.md --json` | `graduated` | permission-gated local tools |
| `providers` | `providers list --json` | `graduated` | provider registry and auth status |
| `config` | `config effective --json` | `graduated` | config precedence |
| `usage` | `usage --json` | `graduated` | token/cost accounting |
| `cost` | `cost --json` | `graduated` | monetary estimate |
| `stats` | `stats --json` | `graduated` | runtime stats |
| `doctor` | `doctor --json` | `graduated` | preflight diagnostics |
| `setup` | `setup status --json` | `graduated` | setup/install/repair family |
| `smoke` | `smoke --json` | `graduated` | mutation-safe smoke check |
| `conformance` | `conformance --json` | `graduated` | local conformance harness |
| `mcp` | `mcp list --json` | `graduated` | MCP registry |
| `skills` | `skills list --json` | `graduated` | skill registry |
| `plugins` | `plugins list --json` | `graduated` | plugin registry |
| `hooks` | `hooks list --json` | `graduated` | hook registry |
| `memory` | `memory status --json` | `graduated_for_project_memory` | M7 working-memory status plus M8 durable promote/query/explain/invalidate and hybrid/vector/temporal query |
| `artifacts` | `artifacts list --json` | `graduated` | artifact families |
| `repo` | `repo cleanup-plan --json` | `graduated` | reversible cleanup governance |
| `projectops` | `projectops status --json` | `graduated` | supervision lease/wake state machine |
| `remote` | `remote status --json` | `graduated` | M5 Tailscale/PWA remote operator plane |
| `reviews` | `reviews list --json` | `graduated` | review packet and trace persistence |
| `docs` | `docs index --json` | `graduated` | DocFrame index/inspect/refresh dry-run |

Graduation interpretation:

- `graduated` commands must keep stable text behavior, stable JSON behavior,
  typed failure envelopes, and regression coverage.
- `feature_gated` commands may remain visible only if the JSON failure is typed
  and the help registry explicitly marks them as not part of the current
  graduated product surface.
