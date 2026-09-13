# Schema Registry And Conformance Plan

This document converts the protocol-heavy architecture into a concrete schema
registry and validation plan.

`14-executable-runtime-protocol.md` already freezes the universal envelope and
published lines.

This document answers the next engineering question:

- which schema files must exist
- what each schema owns
- which docs are authoritative vs informative
- how each schema is tested and promoted

## 1. Schema Ownership Rule

There are only three layers of authority:

1. `14-executable-runtime-protocol.md` defines the universal envelope,
   migration rules, atomic-write rules, and published lines
2. `schemas/*.schema.json` are the machine-readable canonical owners for
   persisted objects
3. other docs may define payload semantics or policy views, but they are not
   allowed to become competing schema authorities

If a persisted object exists without a schema file, it is not ready to ship.

Envelope dispatch rule:

- `command_success.schema.json` and `command_failure.schema.json` own the outer
  operator envelope only
- the `data` field is valid only when the command family points to a
  schema-owned payload registered in this document
- conformance must validate both the outer envelope and the referenced payload
  schema together for each golden fixture

## 2. Required Schema Registry

## Kernel and session

| Schema file | Object | Primary owner doc | First milestone |
|---|---|---|---|
| `schemas/kernel_state_bundle.schema.json` | `KernelStateBundle` checkpoint | `24-kernel-state-machine-contract.md` | M0 |
| `schemas/project_state.schema.json` | `ProjectState` payload | `11-implementation-blueprint.md` | M0 |
| `schemas/event.schema.json` | `KernelEventEnvelope` | `24-kernel-state-machine-contract.md` | M0 |
| `schemas/command_success.schema.json` | `CommandSuccess` operator envelope | `23-cli-operator-contract.md` | M0 |
| `schemas/command_failure.schema.json` | `CommandFailure` operator envelope | `23-cli-operator-contract.md` | M0 |
| `schemas/help_surface_report.schema.json` | `HelpSurfaceReport` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/palette_surface_report.schema.json` | `PaletteSurfaceReport` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/profile_resolution_trace.schema.json` | `ProfileResolutionTrace` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/interactive_launch_result.schema.json` | `InteractiveLaunchResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/turn_result.schema.json` | `TurnResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/resume_result.schema.json` | `ResumeResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/interrupt_result.schema.json` | `InterruptResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/retry_result.schema.json` | `RetryResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/undo_refusal.schema.json` | `UndoRefusal` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/feature_gate_result.schema.json` | `FeatureGateResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/policy_refusal_result.schema.json` | `PolicyRefusalResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/followup_required_result.schema.json` | `FollowupRequiredResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/session.schema.json` | session transcript/control lines | `14-executable-runtime-protocol.md` | M1 |
| `schemas/session_identity.schema.json` | `SessionIdentity` | `21-code-agent-kernel-hardening.md` | M1 |
| `schemas/session_lineage.schema.json` | `SessionLineageRecord` | `21-code-agent-kernel-hardening.md` | M1 |
| `schemas/session_title_record.schema.json` | `SessionTitleRecord` | `23-cli-operator-contract.md` | M1 |
| `schemas/resume_recap.schema.json` | `ResumeRecap` | `23-cli-operator-contract.md` | M1 |
| `schemas/session_browse_result.schema.json` | `SessionBrowseResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/resume_ambiguity_result.schema.json` | `ResumeAmbiguityResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/session_search_result.schema.json` | `SessionSearchResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/session_export_result.schema.json` | `SessionExportResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/session_stats.schema.json` | `SessionStats` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/session_summary.schema.json` | compaction and resume recap metadata | `23-cli-operator-contract.md` | M1 |
| `schemas/compact_result.schema.json` | `CompactResult` inspection/result payload | `23-cli-operator-contract.md` | M1 |
| `schemas/session_inspection.schema.json` | `SessionInspection` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/project_registry_entry.schema.json` | `ProjectRegistryEntry` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/current_project_pointer.schema.json` | `CurrentProjectPointer` persistence payload | `23-cli-operator-contract.md` | M1 |
| `schemas/project_resolution_trace.schema.json` | `ProjectResolutionTrace` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/project_inspection_core.schema.json` | `ProjectInspectionCore` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/project_inspection.schema.json` | `ProjectInspection` operator payload | `23-cli-operator-contract.md` | M1 |

## Provider, permission, config

| Schema file | Object | Primary owner doc | First milestone |
|---|---|---|---|
| `schemas/permission_request.schema.json` | `PermissionRequest` persistence | `24-kernel-state-machine-contract.md` | M2 |
| `schemas/permission_decision_trace.schema.json` | `PermissionDecisionTrace` | `21-code-agent-kernel-hardening.md` | M2 |
| `schemas/project_registry_list.schema.json` | `ProjectRegistryList` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/project_status.schema.json` | `ProjectStatus` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/provider_status.schema.json` | `ProviderStatus` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/provider_catalog_state.schema.json` | `ProviderCatalogState` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/provider_resolution.schema.json` | `ProviderResolution` result envelope payload | `16-kernel-parity-contract.md` | M2 |
| `schemas/provider_resolution_trace.schema.json` | `ProviderResolutionTrace` | `21-code-agent-kernel-hardening.md` | M2 |
| `schemas/provider_status_list.schema.json` | `ProviderStatusList` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/model_catalog_list.schema.json` | `ModelCatalogList` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/model_current_result.schema.json` | `ModelCurrentResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/effective_config_report.schema.json` | `EffectiveConfigReport` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/config_value_result.schema.json` | `ConfigValueResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/config_source_report.schema.json` | `ConfigSourceReport` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/usage_summary.schema.json` | `UsageSummary` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/stats_summary.schema.json` | `StatsSummary` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/runtime_preflight_report.schema.json` | `RuntimePreflightReport` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/doctor_report.schema.json` | `DoctorReport` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/smoke_result.schema.json` | `SmokeResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/setup_status_report.schema.json` | `SetupStatusReport` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/migrate_check_report.schema.json` | `MigrateCheckReport` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/repair_hints_report.schema.json` | `RepairHintsReport` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/install_routes_report.schema.json` | `InstallRoutesReport` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/mcp_server_record.schema.json` | `MCPServerRecord` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/mcp_list_result.schema.json` | `MCPListResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/mcp_inspection_result.schema.json` | `MCPInspectionResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/mcp_test_result.schema.json` | `MCPTestResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/mcp_refresh_result.schema.json` | `MCPRefreshResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/plugin_record.schema.json` | `PluginRecord` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/plugin_list_result.schema.json` | `PluginListResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/plugin_inspection_result.schema.json` | `PluginInspectionResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/plugin_validation_result.schema.json` | `PluginValidationResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/hook_record.schema.json` | `HookRecord` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/hook_list_result.schema.json` | `HookListResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/hook_inspection_result.schema.json` | `HookInspectionResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/hook_test_result.schema.json` | `HookTestResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/runtime_feature_status.schema.json` | `RuntimeFeatureStatus` | `21-code-agent-kernel-hardening.md` | M2 |

## Review, branch, agent

| Schema file | Object | Primary owner doc | First milestone |
|---|---|---|---|
| `schemas/task_packet.schema.json` | `TaskPacket` | `26-memory-branch-research-runtime-policy.md` | M6 |
| `schemas/agent_runtime_identity.schema.json` | `AgentRuntimeIdentity` | `11-implementation-blueprint.md` + `23-cli-operator-contract.md` | M6 |
| `schemas/agent_runtime_record.schema.json` | `AgentRuntimeRecord` | `23-cli-operator-contract.md` | M6 |
| `schemas/agent_trace.schema.json` | agent-local trace line | `24-kernel-state-machine-contract.md` | M6 |
| `schemas/agent_output_manifest.schema.json` | `AgentOutputManifest` | `11-implementation-blueprint.md` | M6 |
| `schemas/agent_worktree_artifact_candidate_manifest.schema.json` | `AgentWorktreeArtifactCandidateManifest` | `11-implementation-blueprint.md` + `23-cli-operator-contract.md` | M6 |
| `schemas/main_agent_worker_artifact_decision.schema.json` | main-agent accept/reject/defer decision for candidate-only worker artifacts | `11-implementation-blueprint.md` + `23-cli-operator-contract.md` | M6 |
| `schemas/review_packet.schema.json` | `ReviewPacket` | `15-research-review-and-contracts.md` | M3 |
| `schemas/review_trace.schema.json` | `ReviewTrace` | `15-research-review-and-contracts.md` | M3 |
| `schemas/review_runtime_record.schema.json` | `ReviewRuntimeRecord` | `23-cli-operator-contract.md` | M3 |
| `schemas/evaluation_packet.schema.json` | `EvaluationPacket` | `26-memory-branch-research-runtime-policy.md` | M9 |
| `schemas/branch_batch.schema.json` | `BranchBatchRecord` / `SearchBatch` persistence | `26-memory-branch-research-runtime-policy.md` | M9 |
| `schemas/branch_run.schema.json` | `BranchRunRecord` | `26-memory-branch-research-runtime-policy.md` | M9 |
| `schemas/branch_batch_record.schema.json` | `BranchBatchRecord` | `23-cli-operator-contract.md` | M9 |
| `schemas/branch_run_record.schema.json` | `BranchRunRecord` | `23-cli-operator-contract.md` | M9 |
| `schemas/debate_packet.schema.json` | `DebatePacket` | `15-research-review-and-contracts.md` | M9 |
| `schemas/debate_trace.schema.json` | `DebateTrace` | `15-research-review-and-contracts.md` | M9 |

## Memory, artifact, ProjectOps

| Schema file | Object | Primary owner doc | First milestone |
|---|---|---|---|
| `schemas/memory_record.schema.json` | `MemoryRecord` | `18-proactive-project-ops.md` + `26-memory-branch-research-runtime-policy.md` | M8 |
| `schemas/working_memory_record.schema.json` | bounded working-memory line | `26-memory-branch-research-runtime-policy.md` | M7 |
| `schemas/memory_query_result.schema.json` | `MemoryQueryResult` operator payload | `16-kernel-parity-contract.md` | M8 |
| `schemas/memory_explain_record.schema.json` | `MemoryExplainRecord` | `18-proactive-project-ops.md` + `26-memory-branch-research-runtime-policy.md` | M8 |
| `schemas/memory_status_report.schema.json` | M7 working-memory status now; M8 durable-memory fields later | `23-cli-operator-contract.md` | M7 |
| `schemas/mission_frame.schema.json` | `MissionFrame` project-goal anchor | `48-mission-frame-context-and-compact-policy.md` | M7 |
| `schemas/goal_alignment_trace.schema.json` | `GoalAlignmentTrace` | `48-mission-frame-context-and-compact-policy.md` | M7 |
| `schemas/doc_frame.schema.json` | `DocFrame` document context anchor | `49-document-info-block-and-docframe-policy.md` | M7 |
| `schemas/doc_index.schema.json` | `DocIndex` derived document-context index | `49-document-info-block-and-docframe-policy.md` | M7 |
| `schemas/progress_digest_candidate.schema.json` | digest candidate | `18-proactive-project-ops.md` | M7 |
| `schemas/projectops_tick.schema.json` | `ProjectOpsTick` | `18-proactive-project-ops.md` | M7 |
| `schemas/cleanup_plan.schema.json` | cleanup proposal | `18-proactive-project-ops.md` | M3 |
| `schemas/repo_cleanup_proposal.schema.json` | `RepoCleanupProposal` | `18-proactive-project-ops.md` | M3 |
| `schemas/artifact_list_result.schema.json` | `ArtifactListResult` operator payload | `23-cli-operator-contract.md` | M3 |
| `schemas/artifact_inspection_result.schema.json` | `ArtifactInspectionResult` operator payload | `23-cli-operator-contract.md` | M3 |
| `schemas/permission_pending_list.schema.json` | `PermissionPendingList` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/permission_history_result.schema.json` | `PermissionHistoryResult` operator payload | `23-cli-operator-contract.md` | M2 |
| `schemas/conformance_result.schema.json` | `ConformanceResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/prune_result.schema.json` | `PruneResult` operator payload | `23-cli-operator-contract.md` | M1 |
| `schemas/experiment_supervisor_lease.schema.json` | `ExperimentSupervisorLease` | `18-proactive-project-ops.md` | M9 |
| `schemas/wake_event.schema.json` | `WakeEvent` | `18-proactive-project-ops.md` | M9 |
| `schemas/change_envelope.schema.json` | `ChangeEnvelope` | `26-memory-branch-research-runtime-policy.md` | M10 |
| `schemas/artifact_family.schema.json` | `ArtifactFamily` | `11-implementation-blueprint.md` | M3 |
| `schemas/artifact_promotion.schema.json` | promotion decision and lineage | `11-implementation-blueprint.md` | M3 |

## Remote operator plane

| Schema file | Object | Primary owner doc | First milestone |
|---|---|---|---|
| `schemas/remote_lease.schema.json` | `RemoteLease` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_binding.schema.json` | `RemoteSessionBinding` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_machine.schema.json` | `RemoteMachine` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_pair_result.schema.json` | `RemotePairResult` | `22-remote-command-and-fixture-contract.md` | M5 |
| `schemas/machine_identity.schema.json` | `MachineIdentity` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_client_identity.schema.json` | `RemoteClientIdentity` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/pair_ticket.schema.json` | `PairTicket` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_machine_metadata.schema.json` | `RemoteMachineMetadata` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_daemon_state.schema.json` | `RemoteDaemonState` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_capability_matrix.schema.json` | `RemoteCapabilityMatrix` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_capability_set.schema.json` | `RemoteCapabilitySet` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/offline_action_policy.schema.json` | `OfflineActionPolicy` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/local_remote_switch_policy.schema.json` | `LocalRemoteSwitchPolicy` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_feature_advertisement.schema.json` | `RemoteFeatureAdvertisement` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/control_lease.schema.json` | `ControlLease` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_cursor.schema.json` | `RemoteCursor` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_control_owner.schema.json` | `RemoteControlOwner` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_action_actor.schema.json` | `RemoteActionActor` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_action.schema.json` | `RemoteAction` tagged union | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/send_user_message_action.schema.json` | `SendUserMessageAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/interrupt_turn_action.schema.json` | `InterruptTurnAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/respond_permission_request_action.schema.json` | `RespondPermissionRequestAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/request_attach_action.schema.json` | `RequestAttachAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/request_handoff_action.schema.json` | `RequestHandoffAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/request_takeover_action.schema.json` | `RequestTakeoverAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/approve_cleanup_proposal_action.schema.json` | `ApproveCleanupProposalAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/inspect_memory_explain_action.schema.json` | `InspectMemoryExplainAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/inspect_branch_batch_action.schema.json` | `InspectBranchBatchAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/wake_supervised_run_action.schema.json` | `WakeSupervisedRunAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/invalidate_memory_record_action.schema.json` | `InvalidateMemoryRecordAction` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_takeover_result.schema.json` | `RemoteTakeoverResult` | `22-remote-command-and-fixture-contract.md` | M5 |
| `schemas/session_runtime_descriptor.schema.json` | `SessionRuntimeDescriptor` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/session_envelope_projection_v1.schema.json` | `SessionEnvelopeProjectionV1` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_projection_snapshot.schema.json` | `RemoteProjectionSnapshot` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_terminal_lease.schema.json` | `RemoteTerminalLease` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/attach_eligibility.schema.json` | `AttachEligibility` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/attach_execution_result.schema.json` | `AttachExecutionResult` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/handoff_eligibility.schema.json` | `HandoffEligibility` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/handoff_execution_result.schema.json` | `HandoffExecutionResult` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_action_rejection.schema.json` | `RemoteActionRejection` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_status_report.schema.json` | `RemoteStatusReport` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_notification_payload.schema.json` | `RemoteNotificationPayload` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/notification_receipt.schema.json` | `NotificationReceipt` | `25-remote-transport-auth-contract.md` | M5 |
| `schemas/remote_notify_result.schema.json` | `RemoteNotifyResult` | `22-remote-command-and-fixture-contract.md` | M5 |

## Research runtime and skills

| Schema file | Object | Primary owner doc | First milestone |
|---|---|---|---|
| `schemas/research_skill_contract.schema.json` | `ResearchSkillContract` | `15-research-review-and-contracts.md` | M10 |
| `schemas/skill_manifest.schema.json` | `SkillManifest` | `15-research-review-and-contracts.md` | M4/M10 |
| `schemas/skill_availability_record.schema.json` | `SkillAvailabilityRecord` | `26-memory-branch-research-runtime-policy.md` | M10 |
| `schemas/skill_registry_entry.schema.json` | `SkillRegistryEntry` | `23-cli-operator-contract.md` | M4/M10 |
| `schemas/skill_list_result.schema.json` | `SkillListResult` operator payload | `23-cli-operator-contract.md` | M4/M10 |
| `schemas/skill_inspect_result.schema.json` | `SkillInspectResult` operator payload | `23-cli-operator-contract.md` | M4/M10 |
| `schemas/skill_path_record.schema.json` | `SkillPathRecord` | `23-cli-operator-contract.md` | M4/M10 |
| `schemas/skill_paths_result.schema.json` | `SkillPathsResult` operator payload | `23-cli-operator-contract.md` | M4/M10 |
| `schemas/skill_validation_result.schema.json` | `SkillValidationResult` operator payload | `23-cli-operator-contract.md` | M4/M10 |
| `schemas/research_stage_execution.schema.json` | `ResearchStageExecution` | `26-memory-branch-research-runtime-policy.md` | M10 |
| `schemas/stage_execution_map_entry.schema.json` | `StageExecutionMapEntry` | `15-research-review-and-contracts.md` | M10 |
| `schemas/stage_execution_map.schema.json` | `StageExecutionMap` | `15-research-review-and-contracts.md` | M10 |

## 3. Authoritative vs Informative Docs

The following docs are allowed to define authoritative payload semantics:

- `11-implementation-blueprint.md`
- `15-research-review-and-contracts.md`
- `18-proactive-project-ops.md`
- `24-kernel-state-machine-contract.md`
- `25-remote-transport-auth-contract.md`
- `26-memory-branch-research-runtime-policy.md`

The following docs are implementation-facing but not canonical schema owners:

- `12-milestone-roadmap.md`
- `16-kernel-parity-contract.md`
- `17-operator-golden-fixtures.md`
- `20-happy-happier-integration-blueprint.md`
- `22-remote-command-and-fixture-contract.md`
- `23-cli-operator-contract.md`
- `27-implementation-workstreams.md`

If a field must be persisted, the final authority must end up in `schemas/`.

## 4. Conformance Per Schema

Every schema must ship with four things:

1. valid fixture
2. invalid fixture
3. round-trip load/save test
4. migration-compatibility expectation

Recommended layout:

```text
tests/conformance/
├── schemas/
│   ├── valid/
│   ├── invalid/
│   └── migrations/
├── runtime/
├── remote/
└── research/
```

## 5. Minimum Validation Matrix

### Envelope-level checks

Every persisted canonical object must validate:

- `schema_version`
- `runtime_version`
- `migration_version`
- `conformance_line`
- `canonical_path`
- `retention_policy`
- `atomic_write_policy`
- when `CommandFailure.data` is present, it must validate against the
  schema-owned payload promised by that command family

### Object-level checks

Examples:

- `TaskPacket`: path scope, budget fields, permission mode, success criteria
- `ReviewPacket`: fresh-thread and output-schema presence
- `ControlLease`: ownership epoch and expiry invariants
- `SessionEnvelopeProjectionV1`: runtime descriptor ref and seq bounds
- `ResearchStageExecution`: stage mapping, packet refs, change envelope refs
- `ProjectResolutionTrace`: resolution source, status, resolved entry, and
  candidate set may not contradict each other
- `SmokeResult`: `source_mutation_free=true` forbids mutated paths outside
  `.pmcli/`
- `FeatureGateResult`: `gate_state=not_graduated` must include milestone or
  missing owners
- `FollowupRequiredResult`: exit `11` lanes must distinguish validated vs
  deferred parts
- `RemoteActionRejection`: rejection stage/code must align with the remote
  validation pipeline stage

### Cross-object checks

- every `AgentRuntimeRecord.task_packet_ref` resolves
- every promoted branch has an `EvaluationPacket`
- every non-cancelled review has a `ReviewTrace`
- every remote binding resolves to a machine identity and control lease
- every `RemoteSessionBinding.control_owner`, `RemoteStatusReport.control_owner`,
  `SessionEnvelopeProjectionV1.control_owner`, and
  `HandoffExecutionResult.previous_owner/new_owner` conform to the same
  `RemoteControlOwner` schema line
- every `RemoteStatusReport.feature_advertisements[]` object conforms to the
  same `RemoteFeatureAdvertisement` line used by
  `SessionEnvelopeProjectionV1.feature_advertisements[]`
- every `RemoteAction` has exactly one typed payload field and that field must
  match `action_type`
- every `RemoteNotifyResult.notification` conforms to
  `RemoteNotificationPayload`, and every receipt inside the same result points
  back to that notification ID
- every `MemoryRecord.status` transition must be legal under the lifecycle
  frozen in `24-kernel-state-machine-contract.md`
- every `ProjectOpsTick.digest_candidate_id`, when present, resolves to a
  `ProgressDigestCandidate` whose `tick_id` points back to the same tick
- every `DocIndex.doc_frames[]` object conforms to `DocFrame`, and every
  active `DocFrame.source_path` points to an existing document or an explicit
  archived artifact reference
- every `DocFrame.evidence_refs[]` entry resolves to a local path, schema,
  test, artifact, or external reference with provenance
- every `MissionFrame` refresh candidate derived from `DocFrame` objects keeps
  the canonical `MissionFrame` as authority until an explicit governed update
- every `WakeEvent.lease_id`, when present, resolves to an
  `ExperimentSupervisorLease`
- every `ResearchStageExecution.stage_id` resolves through
  `StageExecutionMap.entries[]`
- every `ResearchStageExecution` repair or pivot linkage resolves to a prior
  gate, stage, or repair edge declared in the runtime policy docs
- every `SkillInspectResult.entry` resolves to a `SkillRegistryEntry` that is
  legal inside `SkillListResult.skills[]`
- every `PluginInspectionResult.plugin`, `HookInspectionResult.hook`, and
  `MCPInspectionResult.server` may also appear in the corresponding list result
- every `InteractiveLaunchResult.project_trace` and `TurnResult.project_trace`
  resolves to the same project scope that the command envelope reports

## 5.5 Minimum Authority Packet For Blind Review

The smallest no-context review packet that may be handed to an external
reviewer is:

1. `24-kernel-state-machine-contract.md`
2. `14-executable-runtime-protocol.md`
3. `15-research-review-and-contracts.md`
4. `18-proactive-project-ops.md`
5. `25-remote-transport-auth-contract.md`
6. `26-memory-branch-research-runtime-policy.md`
7. `23-cli-operator-contract.md`
8. `28-schema-registry-and-conformance-plan.md`
9. `37-kernel-type-and-interface-manifest.md`
10. `40-reference-superiority-self-audit.md`

Authority packet rule:

- blind review prompts may reference other docs as optional background, but the
  packet above must be sufficient to reconstruct runtime authority, operator
  behavior, schema ownership, remote semantics, and superiority gating
- if a subsystem cannot be evaluated from this packet, the architecture is not
  yet ready for no-context review

## 5.6 Blind-Packet Verification Summary

The blind packet may not rely on package-external fixture documents for its
verification story.

So the minimum verification obligations are summarized here.

### Base CLI obligations

The packet requires fixture-backed validation for:

- session terminal reconciliation under transport loss
- session creation identity completeness
- session event provenance and scope binding
- session operator log request-sequence continuity
- session search/index non-authoritative behavior
- launch/turn/compact payload completeness

### Remote obligations

The packet requires fixture-backed validation for:

- remote attach inspect-vs-execute behavior
- remote handoff inspect-vs-execute behavior
- remote local keypress reclaim
- remote cached read-model non-authoritative behavior
- typed remote action payload legality
- typed notification payload and receipt linkage

### Advanced runtime obligations

The packet requires fixture-backed validation for:

- memory promotion legality
- memory invalidation after rollback
- ProjectOps digest promotion/rejection traceability
- wake escalation with supervisor-lease linkage
- research-stage repair/pivot legality

Conditional-proof rule:

- architecture-level blind review may grant only `conditional_go` until these
  schema and fixture obligations are implemented and passing
- the packet is sufficient to judge whether the obligations are coherent and
  complete, but not to claim completed superiority proof in the absence of
  implementation evidence

## 5.7 Base-CLI Comparative Proof Matrix

The blind packet must also expose what would count as proof against the core
CLI references.

### Claw-class proof obligations

Required workflow classes:

- doctor-first bring-up
- prompt plus resume continuity
- permission-mode transitions
- machine-readable JSON automation
- deterministic parity or replay harness

Required evidence:

- command payload schemas
- golden fixtures
- conformance coverage

Current packet status:

- command and payload line: frozen
- schema and fixture obligation: frozen
- completed implementation proof: pending

### Hermes-class proof obligations

Required workflow classes:

- model and provider switching
- config mutation and persistence
- session browse, search, resume, title, export, and prune
- usage and cost inspection
- durable session logs and non-authoritative search indexes

Required evidence:

- session/operator payloads
- provider/config traces
- browse and export fixtures
- durable-log and read-model fixtures

Current packet status:

- command and payload line: frozen
- schema and fixture obligation: frozen
- completed implementation proof: pending

### Crush-class proof obligations

Required workflow classes:

- project registry and current-project resolution
- workspace init flow
- MCP and skill registry inspection and refresh
- config precedence and effective view
- project-local session continuity

Required evidence:

- project-resolution traces
- setup and registry payloads
- config precedence fixtures
- project/session continuity fixtures

Current packet status:

- command and payload line: frozen
- schema and fixture obligation: frozen
- completed implementation proof: pending

Architecture-level interpretation:

- the blind packet is now explicit about where parity is already specified
- it is equally explicit that proof of exceedance remains pending until these
  workflow classes are implemented and passing

## 6. Build Order

Schema implementation order should mirror risk:

1. bundle + event + session
2. permission/provider/config support payloads
3. artifact + review
4. remote lease/binding/control
5. task packet + agent output
6. memory/projectops/docframes
7. branch/debate/evaluation
8. research skill/runtime

This prevents high-level automation from outpacing the protocol substrate.

## 7. Promotion Rule

A schema is allowed to move from draft to active only when:

- its owner doc is frozen
- at least one valid and invalid fixture exists
- the load/save round-trip test passes
- the previous compatible version, if any, is readable

If any one of those is missing, the feature may exist only in
`not_graduated` form.

## 8. What This Prevents

Without this registry, the implementation would drift into:

- doc-only "schemas"
- duplicated object definitions
- ad hoc JSON blobs
- untestable migrations

With this registry, implementation stays contract-first and externally
auditable.
