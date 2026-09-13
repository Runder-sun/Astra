pub mod agent_loop;
pub mod cancel;
pub mod checkpoint;
pub mod dispatch;
pub mod launch;
pub mod reducer;
pub use dispatch::Runtime;
include!("autonomous_research/job.rs");
include!("autonomous_research/route_change.rs");
include!("autonomous_research/routing.rs");
include!("autonomous_research/main_agent_round.rs");
include!("autonomous_research/stage_closure.rs");

pub(crate) use self::agent_loop::{
    finalize_autonomous_research_budgeted_round, run_agent_loop, AgentLoopBudget, AgentLoopError,
    AgentLoopResult,
};
use self::cancel::{
    active_runtime_turn_registry, runtime_interrupt_pair, ActiveRuntimeTurnGuard,
    ActiveRuntimeTurnKey, RuntimeCancelToken,
};
use self::launch::{
    resolve_profile_trace, InteractiveLaunchResult, TurnResult, TURN_OUTCOME_CANCELLED,
    TURN_OUTCOME_COMPLETED,
};
use crate::agents;
use crate::artifacts;
use crate::branches;
use crate::canonical_artifacts::{self, CanonicalArtifactSeed};
use crate::canonicality;
use crate::commands::{help_surface_report, palette_surface_report, render_help_text};
use crate::config;
use crate::config::{config_get, config_set, effective_config, ConfigError, ConfigScope};
use crate::docs;
use crate::doctor::{DoctorService, SmokeService};
use crate::events::writer::{append_event_unlocked, lock_event_log};
use crate::events::{read_events_from, KernelEventEnvelope};
use crate::evolution;
use crate::feedback;
use crate::goals::{
    GoalAutomationMode, GoalStageTaskQualityProfile, GoalStageTaskSemanticReviewResult,
    GoalTaskAcceptanceResult, GoalWatchInstallApplyTarget, GoalWatchInstallOptions,
    MissionFrameError, MissionFrameUpdate,
};
use crate::host_surface;
use crate::mcp;
use crate::memory::{self, WorkingMemoryAppendRequest};
use crate::orchestration::RunControlAction;
use crate::permissions::{ToolClassification, ToolSpec};
use crate::plugins;
use crate::projects::current::{
    resolve_current_project, resolve_current_project_with_trace, ResolveCurrentProjectError,
    ResolveCurrentProjectWithTraceError, ResolvedProject,
};
use crate::projects::registry::{
    stable_project_id, ProjectRegistry, ProjectRegistryEntry, RegistryPruneResult,
};
use crate::providers::{
    classify_provider_execution_error, complete_prompt_streaming_live_blocking_with_cancel,
    complete_prompt_streaming_with_tools_and_timeout_policy, inspect_provider, list_models,
    list_providers, provider_auth_status, provider_failure_backoff_ms, refresh_catalog,
    refresh_catalog_from_file, resolve_provider_trace, resolve_provider_trace_with_profiles,
    test_provider, ChatMessage, ModelCurrentResult, ProviderCatalogRefreshResult,
    ProviderExecutionError, ProviderFailureDisposition, ProviderPromptCompletion,
    ProviderResolveError, ProviderTestError, ProviderTestResult, ProviderTimeoutPolicy,
};
use crate::remote;
use crate::research;
use crate::reviews;
use crate::routines;
use crate::runtime::checkpoint::KernelStateBundle;
use crate::runtime::reducer::{publish_checkpoint, CheckpointPublishPlan};
use crate::session::context::build_context_messages;
use crate::session::index::ResumeSelector;
use crate::session::store::{ProjectInspection, SessionStore, SessionStoreError};
use crate::session::transcript::TranscriptLine;
use crate::setup::RepairHintComponent;
use crate::skills;
use crate::telemetry;
use crate::tools::{
    builtin_registry, is_read_only_shell_command, LocalToolExecutor, LocalToolExecutorContext,
    ToolCall,
};
use crate::trajectory;
use crate::tui;
use crate::workspace::resolve::{resolve_or_create_workspace_from, resolve_workspace_from};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Agent Loop: multi-step tool-calling loop
// ---------------------------------------------------------------------------

const DEFAULT_MAX_AGENT_ITERATIONS: usize = 50;
const DEFAULT_GOAL_WATCH_INTERVAL_MS: u64 = 2_500;
const GOAL_WATCH_SLEEP_SLICE_MS: u64 = 250;
const DEFAULT_AUTONOMOUS_RESEARCH_JOB_MAX_TICKS: usize = 10_000;
const DEFAULT_AUTONOMOUS_RESEARCH_JOB_INTERVAL_MS: u64 = 30_000;
const DEFAULT_AUTONOMOUS_RESEARCH_JOB_RUNTIME_MS: u64 = 6 * 60 * 60 * 1_000;
const DEFAULT_AUTONOMOUS_RESEARCH_REVIEW_ROUNDS: usize = 10_000;
const AUTONOMOUS_RESEARCH_STALLED_AGENT_TRIGGER_TICKS: usize = 1;
const AUTONOMOUS_RESEARCH_REVIEW_ARTIFACT_BODY_MAX_CHARS: usize = 40_000;
const AUTONOMOUS_RESEARCH_REVIEW_DOCFRAME_MAX_CHARS: usize = 6_000;
const AUTONOMOUS_RESEARCH_REVIEW_ACCEPTED_EVIDENCE_MAX_CHARS: usize = 16_000;
const AUTONOMOUS_RESEARCH_REVIEW_ARTIFACT_REFS_MAX_CHARS: usize = 8_000;
const AUTONOMOUS_RESEARCH_REVIEW_TICK_TRACE_MAX_CHARS: usize = 6_000;
const AUTONOMOUS_RESEARCH_REVIEW_PROMPT_MAX_CHARS: usize = 120_000;
const AUTONOMOUS_RESEARCH_PROVIDER_BACKOFF_MS: u64 = 5 * 60 * 1_000;
const AUTONOMOUS_RESEARCH_AGENT_ROUND_MAX_ITERATIONS: usize = 24;
const AUTONOMOUS_RESEARCH_AGENT_ROUND_MAX_TOOL_CALLS: usize = usize::MAX;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GoalTickResult {
    schema_version: String,
    status: String,
    project_id: String,
    routine_run_due: routines::RoutineRunDueResult,
    goal_advance: crate::goals::GoalAdvanceResult,
    triggered_count: usize,
    dispatch_count: usize,
    accepted: bool,
    next_recommended_action: String,
    recovery_governance: routines::RoutineRecoveryGovernanceSummary,
    loop_closure: crate::goals::GoalLoopClosureProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GoalWatchResult {
    schema_version: String,
    status: String,
    project_id: String,
    interval_ms: u64,
    ticks_completed: usize,
    stop_reason: String,
    watch_lock_path: String,
    watch_state_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_tick: Option<GoalTickResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct GoalWatchRequest {
    tick_request: routines::RoutineRunDueRequest,
    interval_ms: u64,
    once: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AutonomousResearchRunRequest {
    prompt: String,
    workflow_profile: AutonomousWorkflowProfile,
    automation_mode: GoalAutomationMode,
    automation_mode_source: String,
    max_ticks: usize,
    max_ticks_explicit: bool,
    interval_ms: u64,
    interval_ms_explicit: bool,
    report_path: Option<String>,
    background: bool,
    max_runtime_ms: Option<u64>,
    review_model: Option<String>,
    stage_task_semantic_review_mode: String,
    max_review_rounds: usize,
    permission_mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutonomousWorkflowProfile {
    ResearchPipeline,
    SystemValidation,
}

impl AutonomousWorkflowProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::ResearchPipeline => "research-pipeline",
            Self::SystemValidation => "system-validation",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AutonomousResearchResumeRequest {
    job_id: String,
    background: bool,
    max_ticks: Option<usize>,
    additional_ticks: Option<usize>,
    max_runtime_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchTickSummary {
    tick_index: usize,
    status: String,
    dispatch_count: usize,
    accepted: bool,
    loop_status: String,
    next_recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchJobState {
    schema_version: String,
    job_id: String,
    project_id: String,
    prompt: String,
    #[serde(default = "default_autonomous_workflow_profile")]
    workflow_profile: String,
    status: String,
    phase: String,
    automation_mode: GoalAutomationMode,
    permission_mode: String,
    review_model: Option<String>,
    #[serde(default = "default_stage_task_semantic_review_mode")]
    stage_task_semantic_review_mode: String,
    created_at: String,
    updated_at: String,
    max_ticks: usize,
    ticks_completed: usize,
    interval_ms: u64,
    max_runtime_ms: u64,
    max_review_rounds: usize,
    review_rounds_completed: usize,
    report_path: String,
    job_state_path: String,
    job_events_path: String,
    runner_log_path: String,
    thread_id: Option<String>,
    stage_execution_id: Option<String>,
    session_id: Option<String>,
    background_pid: Option<u32>,
    stop_reason: Option<String>,
    last_error: Option<String>,
    last_review: Option<AutonomousResearchReviewState>,
    last_loop_closure: Option<crate::goals::GoalLoopClosureProjection>,
    tick_summaries: Vec<AutonomousResearchTickSummary>,
    artifact_refs: Vec<String>,
    warnings: Vec<String>,
    cleanup_plan_ids: Vec<String>,
    #[serde(default)]
    failure_patterns: Vec<AutonomousResearchFailurePattern>,
    #[serde(default)]
    obligations: Vec<AutonomousResearchObligation>,
    #[serde(default)]
    continuity_packets: Vec<AutonomousResearchContinuityPacketSummary>,
    #[serde(default)]
    provider_faults: Vec<AutonomousResearchProviderFaultState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_provider_fault: Option<AutonomousResearchProviderFaultState>,
    provider_rounds: Vec<AutonomousResearchAgentRoundSummary>,
}

fn default_autonomous_workflow_profile() -> String {
    AutonomousWorkflowProfile::ResearchPipeline
        .as_str()
        .to_string()
}

fn default_stage_task_semantic_review_mode() -> String {
    "provider".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchProviderFaultState {
    schema_version: String,
    fault_id: String,
    source: String,
    provider_id: Option<String>,
    model: Option<String>,
    category: String,
    disposition: String,
    retryable: bool,
    operator_gate_required: bool,
    message: String,
    consecutive_failures: usize,
    backoff_ms: u64,
    next_retry_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) enum AutonomousResearchMainAgentRoundError {
    ProviderFault(AutonomousResearchProviderFaultState),
    NonProvider(String),
}

impl From<String> for AutonomousResearchMainAgentRoundError {
    fn from(value: String) -> Self {
        Self::NonProvider(value)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum AutonomousResearchReviewGateOutcome {
    Review(AutonomousResearchReviewState),
    ProviderFault(AutonomousResearchProviderFaultState),
}

#[derive(Debug, Clone)]
pub(crate) enum AutonomousResearchLiveReviewOutcome {
    Completed(String),
    Unavailable,
    ProviderFault(AutonomousResearchProviderFaultState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchObligation {
    schema_version: String,
    obligation_id: String,
    kind: String,
    stage_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage_execution_id: Option<String>,
    source: String,
    status: String,
    blocking: bool,
    required_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    satisfied_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    missing_task_type: Option<String>,
    detail: String,
    #[serde(default)]
    handled_by_refs: Vec<String>,
    #[serde(default)]
    evidence_refs: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchContinuityPacketSummary {
    packet_id: String,
    reason: String,
    artifact_path: String,
    active_stage_id: Option<String>,
    open_blocking_obligation_count: usize,
    previous_provider_id: Option<String>,
    previous_model: Option<String>,
    current_provider_id: Option<String>,
    current_model: Option<String>,
    model_handoff: bool,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchContinuityPacket {
    schema_version: String,
    packet_id: String,
    reason: String,
    job_id: String,
    project_id: String,
    created_at: String,
    snapshot: AutonomousResearchProjectContinuitySnapshot,
    previous_provider_id: Option<String>,
    previous_model: Option<String>,
    current_provider_id: Option<String>,
    current_model: Option<String>,
    model_handoff: bool,
    previous_main_agent_decision: Option<String>,
    next_required_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchProjectContinuitySnapshot {
    schema_version: String,
    project_id: String,
    workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_autonomous_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_job_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_job_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_stage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_stage_execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    automation_mode: Option<GoalAutomationMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mission_frame_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mission_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_implementation_goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task_pool: Option<AutonomousResearchTaskPoolContinuity>,
    open_blocking_obligation_count: usize,
    #[serde(default)]
    open_obligations: Vec<AutonomousResearchObligation>,
    #[serde(default)]
    unresolved_worker_review_failures: Vec<AutonomousResearchWorkerReviewFailureContext>,
    accepted_worker_evidence_count: usize,
    #[serde(default)]
    accepted_worker_evidence_task_types: Vec<String>,
    #[serde(default)]
    active_worker_evidence_count: usize,
    #[serde(default)]
    active_worker_evidence_task_ids: Vec<String>,
    #[serde(default)]
    active_worker_evidence_task_types: Vec<String>,
    #[serde(default)]
    active_evidence_set_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage_closure_ledger: Option<AutonomousResearchStageClosureLedger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_review_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_review_score: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_continuity_packet_ref: Option<String>,
    #[serde(default)]
    cleanup_plan_ids: Vec<String>,
    #[serde(default)]
    artifact_refs: Vec<String>,
    #[serde(default)]
    project_context_files: Vec<AutonomousResearchProjectContextFileSummary>,
    #[serde(default)]
    project_context_warnings: Vec<String>,
    next_required_action: String,
    resume_recommendation: String,
    generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchWorkerReviewFailureContext {
    #[serde(default)]
    main_agent_decision_id: String,
    #[serde(default)]
    main_agent_required_action: String,
    #[serde(default)]
    main_agent_allowed_tools: Vec<String>,
    #[serde(default)]
    repair_instruction_excerpt: String,
    #[serde(default)]
    runtime_boundary: String,
    agent_id: String,
    review_id: String,
    verdict: String,
    task_id: Option<String>,
    task_type: Option<String>,
    worker_role: Option<String>,
    failure_class: Option<String>,
    suggested_operation: Option<String>,
    cleanup_requirement: Option<String>,
    review_packet_ref: String,
    review_trace_ref: String,
    response_excerpt: String,
    target_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchStageClosureLedger {
    schema_version: String,
    stage_id: String,
    stage_execution_id: String,
    artifact_type: String,
    artifact_path: String,
    rubric_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rubric_path: Option<String>,
    standard_source: String,
    evidence_plan_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evidence_plan_ref: Option<String>,
    #[serde(default)]
    advisory_task_types: Vec<String>,
    required_task_types: Vec<String>,
    accepted_task_types: Vec<String>,
    missing_task_types: Vec<String>,
    #[serde(default)]
    active_evidence_count: usize,
    #[serde(default)]
    active_evidence_task_ids: Vec<String>,
    #[serde(default)]
    active_evidence_set_ids: Vec<String>,
    #[serde(default)]
    non_current_evidence_count: usize,
    #[serde(default)]
    adoption_ready_candidate_count: usize,
    #[serde(default)]
    adoption_ready_candidates: Vec<AutonomousResearchAdoptionReadyCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_failed_review_repair: Option<AutonomousResearchFailedReviewRepairLedger>,
    #[serde(default)]
    canonical_artifact_count: usize,
    #[serde(default)]
    canonical_adoption_requested_count: usize,
    #[serde(default)]
    canonical_materialized_count: usize,
    #[serde(default)]
    canonical_baseline_visible_count: usize,
    #[serde(default)]
    canonical_integration_verified_count: usize,
    #[serde(default)]
    canonical_active_stage_evidence_count: usize,
    #[serde(default)]
    canonical_artifact_blockers: Vec<String>,
    #[serde(default)]
    canonical_artifacts: Vec<AutonomousResearchCanonicalArtifactClosureSummary>,
    task_type_statuses: Vec<AutonomousResearchStageTaskTypeStatus>,
    published_tasks: Vec<AutonomousResearchStageClosureTaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_review_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_review_score: Option<u64>,
    adoption_blockers: Vec<String>,
    review_rerun_blockers: Vec<String>,
    next_stage_action_constraints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchCanonicalArtifactClosureSummary {
    artifact_id: String,
    target_artifact_path: String,
    status: String,
    artifact_kind: String,
    task_type: String,
    source_agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_artifact_path: Option<String>,
    decision_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline_ref: Option<String>,
    #[serde(default)]
    integration_check_count: usize,
    #[serde(default)]
    integration_evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    materialization_blocker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline_blocker: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchAdoptionReadyCandidate {
    agent_id: String,
    task_id: String,
    task_type: String,
    worker_role: String,
    required_output_artifact_type: String,
    quality_level: String,
    quality_score: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    semantic_review_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    semantic_review_score: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    semantic_review_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    main_agent_decision_ref: Option<String>,
    post_latest_failed_review: bool,
    candidate_refs: Vec<String>,
    recommended_source_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recommended_source_artifact_path: Option<String>,
    target_artifact_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AutonomousResearchPendingWorkerArtifactDecision {
    agent_id: String,
    task_id: String,
    task_type: String,
    worker_role: String,
    required_output_artifact_type: String,
    quality_level: String,
    quality_score: u64,
    semantic_review_verdict: Option<String>,
    semantic_review_score: Option<u64>,
    semantic_review_ref: Option<String>,
    candidate_refs: Vec<String>,
    required_main_agent_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchFailedReviewRepairLedger {
    review_id: String,
    score: Option<u64>,
    failure_class: Option<String>,
    suggested_operation: Option<String>,
    cleanup_requirement: Option<String>,
    review_summary_path: Option<String>,
    required_repairs_excerpt: String,
    post_review_state_transition_refs: Vec<String>,
    review_rerun_blocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchStageTaskTypeStatus {
    task_type: String,
    status: String,
    accepted: bool,
    missing: bool,
    published_count: usize,
    running_count: usize,
    ready_count: usize,
    blocked_count: usize,
    merged_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchStageClosureTaskStatus {
    task_id: String,
    task_type: String,
    worker_role: String,
    bucket_id: String,
    status: String,
    action_policy: String,
    source_kind: String,
    source_id: String,
    stage_execution_id: String,
    merged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    merged_into: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claimed_by_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contract_violation: Option<String>,
    title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchProjectContextFileSummary {
    schema_version: String,
    path: String,
    advisory_only: bool,
    canonical_authority: bool,
    content_hash: String,
    preview: String,
    conflict_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchTaskPoolContinuity {
    summary: crate::goals::GoalTaskPoolSummary,
    next_recommended_action: String,
    entries: Vec<AutonomousResearchTaskPoolEntryContinuity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchTaskPoolEntryContinuity {
    entry_id: String,
    #[serde(default)]
    bucket_id: String,
    title: String,
    #[serde(default)]
    summary: String,
    status: String,
    source_kind: String,
    source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    action_refs: Vec<crate::goals::GoalTaskPoolEntryActionRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    recommended_command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projected_stage_task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projected_stage_task: Option<crate::goals::GoalStageTaskMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchFailurePattern {
    stage_id: String,
    failure_class: String,
    count: usize,
    first_review_id: String,
    latest_review_id: String,
    latest_score: Option<u64>,
    strategy_escalated: bool,
    last_operation: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchReviewState {
    review_id: String,
    verdict: String,
    score: Option<u64>,
    response_text: String,
    target_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rubric_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_summary_path: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchAgentRoundSummary {
    round_index: usize,
    artifact_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_identity_ref: Option<String>,
    provider_id: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_route_source: Option<String>,
    execution_mode: String,
    iterations: usize,
    tool_calls_made: usize,
    finish_reason: String,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchAcceptedWorkerEvidenceIndex {
    schema_version: String,
    job_id: String,
    project_id: String,
    stage_execution_id: String,
    stage_id: String,
    generated_at: String,
    entries: Vec<AutonomousResearchAcceptedWorkerEvidenceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchAcceptedWorkerEvidenceEntry {
    agent_id: String,
    task_id: String,
    task_type: String,
    worker_role: String,
    required_output_artifact_type: String,
    output_manifest_ref: String,
    task_packet_ref: String,
    evidence_refs: Vec<String>,
    matched_required_fields: Vec<String>,
    matched_acceptance_checks: Vec<String>,
    #[serde(default)]
    matched_quality_signals: Vec<String>,
    #[serde(default)]
    quality_profile: GoalStageTaskQualityProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    semantic_review: Option<GoalStageTaskSemanticReviewResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    main_agent_acceptance: Option<GoalStageTaskSemanticReviewResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acceptance_authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    main_agent_decision_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_evidence_set_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    superseded_by_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    replacement_of_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decision_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MainAgentStageArtifactAdoptionRecord {
    adoption_ref: String,
    stage_id: String,
    stage_execution_id: String,
    source_agent_id: String,
    source_task_id: Option<String>,
    source_ref: String,
    source_artifact_path: Option<String>,
    target_artifact_path: String,
    rationale: String,
    evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evidence_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    accepted_evidence_revision_refs: Vec<String>,
    #[serde(default)]
    replacement_of_artifact_ids: Vec<String>,
    cleanup_required: bool,
    request_review_rerun: bool,
    created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainAgentStageArtifactAdoptionTargetKind {
    StageArtifact,
    ProjectFile,
}

#[derive(Debug, Clone)]
pub(crate) struct MainAgentStageArtifactAdoptionSourceCandidate {
    source_ref: String,
    relative_path: Option<String>,
    bytes: Vec<u8>,
    content: String,
    sha256: String,
    kind: MainAgentStageArtifactAdoptionSourceCandidateKind,
    directory_manifest: Option<WorkerDirectoryCandidateManifestRuntime>,
    source_worktree_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MainAgentStageArtifactAdoptionPreflight {
    pub(crate) source_agent_id: String,
    pub(crate) source_task_id: Option<String>,
    pub(crate) source_ref: String,
    pub(crate) target_artifact_path: String,
    pub(crate) target_kind: String,
    pub(crate) resolved_source_refs: Vec<String>,
    pub(crate) candidate_relative_paths: Vec<String>,
    pub(crate) evidence_snapshot_hash: String,
    pub(crate) accepted_evidence_revision_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MainAgentStageArtifactAdoptionSourceCandidateKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct WorkerDirectoryCandidateManifestRuntime {
    root_relative_path: String,
    entries: Vec<WorkerDirectoryCandidateEntryRuntime>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct WorkerDirectoryCandidateEntryRuntime {
    relative_path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MainAgentWorkerArtifactDecisionRecord {
    decision_ref: String,
    agent_id: String,
    task_id: String,
    #[serde(default)]
    candidate_refs: Vec<String>,
    decision: String,
    rationale: String,
    #[serde(default)]
    adoption_scope: Option<String>,
    #[serde(default)]
    canonical_target_refs: Vec<String>,
    review_required: bool,
    cleanup_required: bool,
    #[serde(default)]
    readiness_refs: Vec<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    stage_id: Option<String>,
    #[serde(default)]
    stage_execution_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MainAgentRouteChangeRequestRecord {
    record_ref: String,
    operation: String,
    target_stage_id: String,
    rationale: String,
    cleanup_required: bool,
    readiness_refs: Vec<String>,
    job_id: Option<String>,
    stage_id: Option<String>,
    stage_execution_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AutonomousResearchAgentRound {
    summary: AutonomousResearchAgentRoundSummary,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchStageEvidenceRequirement {
    task_type: String,
    worker_role: String,
    objective: String,
    required_output_artifact_type: String,
    #[serde(default)]
    required_output_fields: Vec<String>,
    #[serde(default)]
    acceptance_checks: Vec<String>,
    #[serde(default)]
    failure_signals: Vec<String>,
    evidence_standard: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MainAgentStageEvidencePlanRecord {
    schema_version: String,
    plan_id: String,
    plan_ref: String,
    stage_id: String,
    stage_execution_id: String,
    rationale: String,
    #[serde(default)]
    plan_source_refs: Vec<String>,
    #[serde(default)]
    evidence_requirements: Vec<AutonomousResearchStageEvidenceRequirement>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MainAgentStageClosureDecisionRecord {
    decision_ref: String,
    decision: String,
    target_stage_id: String,
    closure_rationale: String,
    why_no_more_stage_work_is_needed: String,
    accepted_evidence_refs: Vec<String>,
    review_ref: String,
    stage_artifact_ref: String,
    remaining_risks: Vec<String>,
    cleanup_required: bool,
    cleanup_rationale: String,
    readiness_refs: Vec<String>,
    job_id: Option<String>,
    stage_id: String,
    stage_execution_id: String,
    created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AutonomousResearchStageContract {
    stage_id: String,
    stage_class: String,
    artifact_type: String,
    artifact_path: String,
    review_strength: String,
    objective: String,
    required_fields: Vec<String>,
    pass_criteria: Vec<String>,
    failure_signals: Vec<String>,
    worker_task_types: Vec<String>,
    worker_task_requirements: Vec<AutonomousResearchStageEvidenceRequirement>,
    advisory_worker_task_types: Vec<String>,
    evidence_plan_status: String,
    evidence_plan_ref: Option<String>,
    paper_allowed: bool,
    final_completion_stage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchRepairPlan {
    schema_version: String,
    repair_task_id: String,
    review_id: String,
    affected_stage_id: String,
    affected_stage_execution_id: Option<String>,
    failure_class: String,
    suggested_operation: String,
    rollback_target_stage_id: Option<String>,
    cleanup_required: bool,
    cleanup_reason: Option<String>,
    worker_role: String,
    required_output_artifact_type: String,
    required_evidence: Vec<String>,
    acceptance_checks: Vec<String>,
    failure_signals: Vec<String>,
    source_review_excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    strategy_escalation: Option<AutonomousResearchStrategyEscalation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repair_plan_doc_path: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchStrategyEscalation {
    schema_version: String,
    pattern_key: String,
    failure_count: usize,
    threshold: usize,
    escalation_reason: String,
    required_strategy_action: String,
    recommended_stage_id: String,
    recommended_operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchJobRunResult {
    schema_version: String,
    status: String,
    project_id: String,
    job_id: String,
    background: bool,
    pid: Option<u32>,
    job_state_path: String,
    job_events_path: String,
    runner_log_path: String,
    status_command: Vec<String>,
    tail_command: Vec<String>,
    resume_command: Vec<String>,
    tick_command: Vec<String>,
    job: AutonomousResearchJobState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchJobTickResult {
    schema_version: String,
    status: String,
    project_id: String,
    job_id: String,
    tick_index: usize,
    job_status: String,
    phase: String,
    should_continue: bool,
    review_passed: bool,
    tick_summary: Option<AutonomousResearchTickSummary>,
    review: Option<AutonomousResearchReviewState>,
    job: AutonomousResearchJobState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchJobListResult {
    schema_version: String,
    project_id: String,
    jobs: Vec<AutonomousResearchJobState>,
    total_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutonomousResearchJobTailResult {
    schema_version: String,
    project_id: String,
    job_id: String,
    events_path: String,
    events: Vec<Value>,
}

#[derive(Debug)]
pub(crate) struct GoalWatchLockGuard {
    file: File,
}

#[derive(Debug)]
pub(crate) enum GoalWatchLockError {
    Busy(PathBuf),
    Io(std::io::Error),
}

impl Drop for GoalWatchLockGuard {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub fn run_remote_prompt_turn(
    registry: &ProjectRegistry,
    project: &ResolvedProject,
    session_id: &str,
    prompt_text: &str,
) -> Result<TurnResult, String> {
    run_remote_prompt_turn_streaming(registry, project, session_id, prompt_text, |_| {})
}

pub fn run_remote_prompt_turn_streaming<F>(
    registry: &ProjectRegistry,
    project: &ResolvedProject,
    session_id: &str,
    prompt_text: &str,
    on_delta: F,
) -> Result<TurnResult, String>
where
    F: FnMut(&str),
{
    let (cancel_token, interrupt_handle) = runtime_interrupt_pair();
    run_remote_prompt_turn_streaming_with_policy(
        registry,
        project,
        session_id,
        prompt_text,
        on_delta,
        cancel_token,
        Some(interrupt_handle),
    )
}

pub fn run_remote_prompt_turn_streaming_with_cancel<F>(
    registry: &ProjectRegistry,
    project: &ResolvedProject,
    session_id: &str,
    prompt_text: &str,
    on_delta: F,
    cancel_token: RuntimeCancelToken,
) -> Result<TurnResult, String>
where
    F: FnMut(&str),
{
    run_remote_prompt_turn_streaming_with_policy(
        registry,
        project,
        session_id,
        prompt_text,
        on_delta,
        cancel_token,
        None,
    )
}

pub fn run_tui_prompt_turn_streaming<F>(
    registry: &ProjectRegistry,
    project: &ResolvedProject,
    session_id: &str,
    prompt_text: &str,
    on_delta: F,
) -> Result<TurnResult, String>
where
    F: FnMut(&str),
{
    run_remote_prompt_turn_streaming_with_policy(
        registry,
        project,
        session_id,
        prompt_text,
        on_delta,
        RuntimeCancelToken::default(),
        None,
    )
}

pub fn run_tui_prompt_turn_streaming_with_cancel<F>(
    registry: &ProjectRegistry,
    project: &ResolvedProject,
    session_id: &str,
    prompt_text: &str,
    on_delta: F,
    cancel_token: RuntimeCancelToken,
) -> Result<TurnResult, String>
where
    F: FnMut(&str),
{
    run_remote_prompt_turn_streaming_with_policy(
        registry,
        project,
        session_id,
        prompt_text,
        on_delta,
        cancel_token,
        None,
    )
}

fn run_remote_prompt_turn_streaming_with_policy<F>(
    registry: &ProjectRegistry,
    project: &ResolvedProject,
    session_id: &str,
    prompt_text: &str,
    on_delta: F,
    cancel_token: RuntimeCancelToken,
    interrupt_handle: Option<self::cancel::RuntimeInterruptHandle>,
) -> Result<TurnResult, String>
where
    F: FnMut(&str),
{
    let doctor = DoctorService::run(
        registry,
        &project.workspace_root,
        None,
        Some(&project.workspace_root),
    );
    if doctor.overall_status == "blocked" {
        return Err(format!(
            "runtime preflight blocked remote prompt execution: {}",
            doctor
                .repair_hints
                .first()
                .cloned()
                .unwrap_or_else(|| "inspect `research-cli doctor --json`".to_string())
        ));
    }

    let profile_trace = resolve_profile_trace(None);
    let mission_frame = crate::goals::projection(
        &project.data_dir,
        &project.workspace_root,
        &project.project_id,
    )
    .map_err(|err| err.to_string())?;
    let doc_context = crate::docs::context_projection(
        &project.data_dir,
        &project.workspace_root,
        &project.project_id,
    )
    .map_err(|err| err.to_string())?;
    let invocation = PromptInvocation {
        session_id: Some(session_id.to_string()),
        new_session: false,
        prompt_text: prompt_text.to_string(),
        provider: None,
        model: None,
        permission_mode: None,
    };
    let prompt_defaults =
        resolve_prompt_request_defaults(project, &invocation).map_err(|err| err.to_string())?;
    let provider_trace = resolve_prompt_provider_trace(
        prompt_defaults.provider.as_deref(),
        prompt_defaults.model.as_deref(),
        prompt_defaults.provider_source.as_deref(),
        prompt_defaults.model_source.as_deref(),
    )
    .map_err(|err| err.envelope.error.message)?;

    let store = SessionStore::new(project.data_dir.clone());
    let session = store
        .resolve_resume_target(ResumeSelector::Exact(session_id.to_string()))
        .map_err(|err| err.to_string())?;
    registry
        .touch_project(&project.project_id, Some(session.session_id.clone()))
        .map_err(|err| err.to_string())?;

    let turn_id = format!("turn_{}", timestamp_string());
    let _active_turn_guard: Option<ActiveRuntimeTurnGuard> = interrupt_handle.map(|handle| {
        active_runtime_turn_registry().register(
            ActiveRuntimeTurnKey::new(
                project.project_id.clone(),
                session.session_id.clone(),
                turn_id.clone(),
            ),
            handle,
        )
    });
    let permission_request_id = format!("perm_{}", timestamp_string());
    let mutation_requested = prompt_requests_mutation(prompt_text);
    let requires_permission = mutation_requested && prompt_defaults.permission_mode == "read-only";

    // Build context messages with transcript history
    let context_messages = build_context_messages(
        &store,
        &project.project_id,
        &session.session_id,
        prompt_text,
        &provider_trace.resolved_model,
    );

    emit_runtime_preflight_event(
        project,
        "remote prompt",
        &session.session_id,
        &doctor.preflight,
    )
    .map_err(|err| err.to_string())?;
    emit_provider_resolution_event(
        project,
        &session.session_id,
        &turn_id,
        &provider_trace,
        &prompt_defaults,
    )
    .map_err(|err| err.to_string())?;
    emit_turn_event(
        project,
        &session.session_id,
        &turn_id,
        "start",
        None,
        json!({
            "turn_id": turn_id,
            "status": if requires_permission { "awaiting_permission" } else { "running" },
            "input": prompt_text,
            "source": "remote_message"
        }),
    )
    .map_err(|err| err.to_string())?;

    if cancel_token.is_cancelled() {
        return cancelled_remote_prompt_turn_result(
            project,
            &session.session_id,
            &turn_id,
            &profile_trace,
            mission_frame,
            doc_context,
            &provider_trace,
            String::new(),
        );
    }

    if requires_permission {
        let requested_at = timestamp_string();
        let expires_at_ms = permission_request_expires_at_ms();
        emit_permission_event(
            project,
            &session.session_id,
            &permission_request_id,
            "start",
            None,
            json!({
                "request_id": permission_request_id,
                "session_id": session.session_id,
                "turn_id": turn_id,
                "tool_name": "remote_prompt_execution",
                "permission_mode": prompt_defaults.permission_mode,
                "reason": "remote prompt requested a mutating action under read-only mode",
                "requested_at": requested_at,
                "expires_at_ms": expires_at_ms,
                "status": "pending"
            }),
        )
        .map_err(|err| err.to_string())?;
        publish_project_checkpoint(project, &["session", "turn", "permission"])
            .map_err(|err| err.to_string())?;
        return Err(format!(
            "remote prompt requires permission approval before execution: {permission_request_id}"
        ));
    }

    let (provider_completion, agent_meta) = {
        let tool_defs =
            crate::tools::builtin_tool_definitions_for_mode(&prompt_defaults.permission_mode);
        let tool_defs_json = crate::tools::tool_definitions_to_openai_json(&tool_defs);
        let tool_executor = LocalToolExecutor::new(&project.workspace_root);
        let perm_mode = crate::permissions::PermissionMode::parse(&prompt_defaults.permission_mode)
            .unwrap_or(crate::permissions::PermissionMode::ReadOnly);
        let permission_policy =
            crate::permissions::PermissionPolicy::new(perm_mode, &project.workspace_root);
        let mut delta_forwarder = on_delta;
        let mut tool_start_cb = |_name: &str, _id: &str| {};
        let mut tool_result_cb = |_event: crate::runtime::agent_loop::AgentLoopToolResultEvent| {};

        match run_agent_loop(
            &provider_trace,
            &context_messages,
            tool_defs_json,
            &tool_executor,
            &permission_policy,
            &store,
            &session.session_id,
            &cancel_token,
            &mut delta_forwarder,
            &mut tool_start_cb,
            &mut tool_result_cb,
            prompt_defaults.reasoning_effort.as_deref(),
        ) {
            Ok(agent_result) => (
                ProviderPromptCompletion {
                    content: agent_result.final_content,
                    execution_mode: "agent_loop".to_string(),
                    endpoint: String::new(),
                    tool_calls: None,
                    finish_reason: Some(agent_result.finish_reason),
                },
                Some((agent_result.iterations, agent_result.tool_calls_made)),
            ),
            Err(AgentLoopError::ProviderError(ProviderExecutionError::Cancelled {
                partial_content,
            })) => {
                return cancelled_remote_prompt_turn_result(
                    project,
                    &session.session_id,
                    &turn_id,
                    &profile_trace,
                    mission_frame,
                    doc_context,
                    &provider_trace,
                    partial_content,
                );
            }
            Err(AgentLoopError::Cancelled {
                partial_content, ..
            }) => {
                return cancelled_remote_prompt_turn_result(
                    project,
                    &session.session_id,
                    &turn_id,
                    &profile_trace,
                    mission_frame,
                    doc_context,
                    &provider_trace,
                    partial_content,
                );
            }
            Err(err) => return Err(err.to_string()),
        }
    };
    store
        .append_line(
            &session.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: prompt_text.to_string(),
            },
        )
        .map_err(|err| err.to_string())?;
    store
        .append_line(
            &session.session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: provider_completion.content.clone(),
            },
        )
        .map_err(|err| err.to_string())?;

    // Auto-compaction: check if context is approaching the token budget
    {
        let context_tokens: usize = context_messages
            .iter()
            .map(|m| crate::providers::estimate_tokens(&m.content))
            .sum();
        let config = crate::providers::model_window_config(&provider_trace.resolved_model);
        let threshold = (config.history_soft_limit * config.context_window as f64) as usize;
        if context_tokens > threshold {
            let store_clone = store.clone();
            let sid = session.session_id.clone();
            let project_id = project.project_id.clone();
            let trace_clone = provider_trace.clone();
            std::thread::spawn(move || {
                let _ =
                    store_clone.compact_session_llm_for_project(&sid, &project_id, &trace_clone);
            });
        }
    }

    emit_turn_event(
        project,
        &session.session_id,
        &turn_id,
        "terminal",
        Some("succeeded"),
        json!({
            "turn_id": turn_id,
            "status": "succeeded",
            "outcome": TURN_OUTCOME_COMPLETED,
            "source": "remote_message",
            "provider_execution": {
                "execution_mode": provider_completion.execution_mode,
                "endpoint": provider_completion.endpoint
            }
        }),
    )
    .map_err(|err| err.to_string())?;
    let mission_frame_value =
        serde_json::to_value(&mission_frame).expect("mission frame should serialize");
    emit_goal_alignment_trace_event(
        project,
        &session.session_id,
        goal_alignment_trace_payload_from_text(
            project,
            &session.session_id,
            "remote prompt",
            mission_frame
                .as_ref()
                .map(|frame| frame.mission_frame_ref.as_str())
                .unwrap_or_default(),
            Some(&mission_frame_value),
            prompt_text,
            "remote_message",
        ),
    )
    .map_err(|err| err.to_string())?;
    publish_project_checkpoint(project, &["session", "turn"]).map_err(|err| err.to_string())?;
    let research_classification = crate::research::observe_turn(
        &project.data_dir,
        &project.project_id,
        &turn_id,
        prompt_text,
    )
    .map_err(|err| err.to_string())?;
    let research_context = crate::research::projection(&project.data_dir, &project.project_id)
        .map_err(|err| err.to_string())?;
    let skill_outputs =
        skills::context_projection(&project.data_dir).map_err(|err| err.to_string())?;
    auto_advance_goal_run_after_turn(
        project,
        &session.session_id,
        &turn_id,
        mission_frame.as_ref(),
    )?;

    Ok(TurnResult {
        session_id: session.session_id,
        turn_id,
        project_id: project.project_id.clone(),
        outcome: TURN_OUTCOME_COMPLETED.to_string(),
        project_trace: None,
        profile_trace: Some(profile_trace),
        mission_frame,
        doc_context: Some(doc_context),
        research_context,
        skill_outputs: Some(skill_outputs),
        research_classification,
        provider_trace,
        assistant_content: provider_completion.content,
        agent_iterations: agent_meta.map(|(i, _)| i),
        agent_tool_calls: agent_meta.map(|(_, t)| t),
    })
}

fn cancelled_remote_prompt_turn_result(
    project: &ResolvedProject,
    session_id: &str,
    turn_id: &str,
    profile_trace: &launch::ProfileResolutionTrace,
    mission_frame: Option<crate::goals::MissionFrameProjection>,
    doc_context: crate::docs::DocContextProjection,
    provider_trace: &crate::providers::ProviderResolutionTrace,
    partial_content: String,
) -> Result<TurnResult, String> {
    emit_turn_event(
        project,
        session_id,
        turn_id,
        "terminal",
        Some(TURN_OUTCOME_CANCELLED),
        json!({
            "turn_id": turn_id,
            "status": TURN_OUTCOME_CANCELLED,
            "outcome": TURN_OUTCOME_CANCELLED,
            "source": "remote_message",
            "partial_content_len": partial_content.len()
        }),
    )
    .map_err(|err| err.to_string())?;
    publish_project_checkpoint(project, &["session", "turn"]).map_err(|err| err.to_string())?;

    Ok(TurnResult {
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        project_id: project.project_id.clone(),
        outcome: TURN_OUTCOME_CANCELLED.to_string(),
        project_trace: None,
        profile_trace: Some(profile_trace.clone()),
        mission_frame,
        doc_context: Some(doc_context),
        research_context: None,
        skill_outputs: None,
        research_classification: None,
        provider_trace: provider_trace.clone(),
        assistant_content: partial_content,
        agent_iterations: None,
        agent_tool_calls: None,
    })
}

#[derive(Debug)]
pub(crate) struct ParsedCliArgs {
    project: Option<String>,
    profile: Option<String>,
    cwd: Option<PathBuf>,
    output_json: bool,
    #[allow(dead_code)]
    quiet: bool,
    #[allow(dead_code)]
    no_color: bool,
    #[allow(dead_code)]
    trace: bool,
    continue_requested: bool,
    help_requested: bool,
    version_requested: bool,
    command_args: Vec<String>,
}

fn parse_cli_args(args: &[String]) -> Result<ParsedCliArgs, CommandFailureOutcome> {
    let mut project = None;
    let mut profile = None;
    let mut profile_source_value: Option<String> = None;
    let mut profile_conflict: Option<(String, String)> = None;
    let mut cwd = None;
    let mut output_json = false;
    let mut quiet = false;
    let mut no_color = false;
    let mut trace = false;
    let mut continue_requested = false;
    let mut help_requested = false;
    let mut version_requested = false;
    let mut command_args = Vec::new();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => {
                help_requested = true;
                index += 1;
            }
            "--version" | "-V" => {
                version_requested = true;
                index += 1;
            }
            "--json" => {
                output_json = true;
                index += 1;
            }
            "--quiet" => {
                quiet = true;
                index += 1;
            }
            "--no-color" => {
                no_color = true;
                index += 1;
            }
            "--trace" => {
                trace = true;
                index += 1;
            }
            "--continue" => {
                continue_requested = true;
                index += 1;
            }
            "--project" => match args.get(index + 1) {
                Some(value) if !value.starts_with("--") => {
                    project = Some(value.clone());
                    index += 2;
                }
                _ => {
                    command_args.push(args[index].clone());
                    index += 1;
                }
            },
            "--profile" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "--profile".to_string(),
                        "usage_invalid",
                        "missing value for --profile".to_string(),
                        Some("Pass a profile id after --profile.".to_string()),
                    )
                    .with_output_json(output_json)
                })?;
                if let Some(existing) = profile_source_value.as_deref() {
                    if existing != value {
                        profile_conflict = Some((existing.to_string(), value.clone()));
                    }
                }
                profile = Some(value.clone());
                profile_source_value = Some(value.clone());
                index += 2;
            }
            "--cwd" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "--cwd".to_string(),
                        "usage_invalid",
                        "missing value for --cwd".to_string(),
                        Some(
                            "Pass an absolute or relative workspace path after --cwd.".to_string(),
                        ),
                    )
                    .with_output_json(output_json)
                })?;
                cwd = Some(PathBuf::from(value));
                index += 2;
            }
            _ => {
                command_args.push(args[index].clone());
                index += 1;
            }
        }
    }

    if let Some((existing, requested)) = profile_conflict {
        return Err(policy_refusal_failure(
            "launch",
            7,
            "profile_conflict",
            "startup_profile",
            "cli_flag",
            "conflicting_profile_intent",
            Some("--profile".to_string()),
            Some(format!(
                "Conflicting startup profiles requested: `{existing}` and `{requested}`."
            )),
            vec![
                format!(
                    "Retry with exactly one `--profile` value. Current conflicting values: `{existing}` and `{requested}`."
                ),
                "If you intended a saved default, omit `--profile` entirely.".to_string(),
            ],
        )
        .with_output_json(output_json));
    }

    Ok(ParsedCliArgs {
        project,
        profile,
        cwd,
        output_json,
        quiet,
        no_color,
        trace,
        continue_requested,
        help_requested,
        version_requested,
        command_args,
    })
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandSuccess {
    ok: bool,
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing)]
    text_override: Option<String>,
}

impl CommandSuccess {
    fn new(command: &str) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            project_id: None,
            session_id: None,
            data: None,
            text_override: None,
        }
    }

    fn new_with_data(command: &str, data: Value) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            project_id: None,
            session_id: None,
            data: Some(data),
            text_override: None,
        }
    }

    fn with_project(command: &str, project_id: String, data: Value) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            project_id: Some(project_id),
            session_id: None,
            data: Some(data),
            text_override: None,
        }
    }

    fn with_project_and_session(
        command: &str,
        project_id: String,
        session_id: String,
        data: Value,
    ) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            project_id: Some(project_id),
            session_id: Some(session_id),
            data: Some(data),
            text_override: None,
        }
    }

    fn with_text_override(mut self, text: String) -> Self {
        self.text_override = Some(text);
        self
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandFailure {
    ok: bool,
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    error: CommandError,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    retryable: bool,
}

#[derive(Debug)]
pub(crate) struct CommandFailureOutcome {
    exit_code: i32,
    output_json: bool,
    envelope: CommandFailure,
}

#[derive(Debug, Serialize)]
pub(crate) struct PermissionPendingList {
    requests: Vec<Value>,
    total_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct PermissionHistoryResult {
    decisions: Vec<Value>,
    total_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConformanceResult {
    scope: String,
    execution_mode: String,
    passed_families: Vec<String>,
    failed_families: Vec<String>,
    failure_refs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectRegistryListPayload {
    projects: Vec<ProjectRegistryEntry>,
    current_project_id: Option<String>,
    total_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectStatusPayload {
    project_id: String,
    workspace_root: String,
    session_count: usize,
    resolution_source: String,
    registry_entry: ProjectRegistryEntry,
    init_state: String,
    active_session_id: Option<String>,
    open_branch_count: u64,
    repo_health: String,
    degraded_features: Vec<String>,
    project_continuity: AutonomousResearchProjectContinuitySnapshot,
}

impl CommandFailureOutcome {
    fn usage(command: String, code: &str, message: String, hint: Option<String>) -> Self {
        Self::new(
            2, command, None, None, code, message, hint, false, None, false,
        )
    }

    fn new(
        exit_code: i32,
        command: String,
        project_id: Option<String>,
        session_id: Option<String>,
        code: &str,
        message: String,
        hint: Option<String>,
        retryable: bool,
        data: Option<Value>,
        output_json: bool,
    ) -> Self {
        Self {
            exit_code,
            output_json,
            envelope: CommandFailure {
                ok: false,
                command,
                project_id,
                session_id,
                error: CommandError {
                    code: code.to_string(),
                    message,
                    hint,
                    retryable,
                },
                data,
            },
        }
    }

    fn render_for_stderr(&self) -> String {
        let mut line = format!("{}: {}", self.envelope.command, self.envelope.error.message);
        if let Some(hint) = &self.envelope.error.hint {
            line.push_str(&format!("\nnext: {hint}"));
        }
        line
    }

    fn with_output_json(mut self, output_json: bool) -> Self {
        self.output_json = output_json;
        self
    }
}

fn project_status_payload(
    resolved: &ResolvedProject,
    registry_entry: ProjectRegistryEntry,
    session_count: usize,
) -> ProjectStatusPayload {
    let repo_health = registry_entry
        .repo_health
        .clone()
        .unwrap_or_else(|| "healthy".to_string());
    let degraded_features = if repo_health == "healthy" {
        Vec::new()
    } else {
        vec!["repo_health".to_string()]
    };

    ProjectStatusPayload {
        project_id: resolved.project_id.clone(),
        workspace_root: resolved.workspace_root.display().to_string(),
        session_count,
        resolution_source: resolved.resolution_source.clone(),
        init_state: registry_entry.init_state.clone(),
        active_session_id: registry_entry.active_session_id.clone(),
        open_branch_count: registry_entry.open_branch_count.unwrap_or(0),
        repo_health,
        degraded_features,
        project_continuity: autonomous_research_project_continuity_snapshot(
            resolved,
            registry_entry.active_session_id.clone(),
        ),
        registry_entry,
    }
}

fn emit_output<T>(output_json: bool, envelope: &T) -> Result<(), serde_json::Error>
where
    T: Serialize + TextRenderable,
{
    if output_json {
        println!("{}", serde_json::to_string(envelope)?);
    } else {
        println!("{}", envelope.render_text());
    }
    Ok(())
}

trait TextRenderable {
    fn render_text(&self) -> String;
}

impl TextRenderable for CommandSuccess {
    fn render_text(&self) -> String {
        if let Some(text) = &self.text_override {
            return text.clone();
        }
        if self.command == "help" {
            return render_help_text("cli");
        }
        if self.command == "version" {
            let version = self
                .data
                .as_ref()
                .and_then(|data| data.get("version"))
                .and_then(Value::as_str)
                .unwrap_or(env!("CARGO_PKG_VERSION"));
            return format!("astra {version}");
        }
        let mut lines = vec![format!("{}: {}", self.command, success_status_label(self))];
        if let Some(project_id) = &self.project_id {
            lines.push(format!("project_id: {project_id}"));
        }
        if let Some(session_id) = &self.session_id {
            lines.push(format!("session_id: {session_id}"));
        }
        if self
            .data
            .as_ref()
            .and_then(|data| data.get("launch_disposition"))
            .and_then(Value::as_str)
            == Some("interactive_skeleton_exit")
        {
            lines.push("interactive skeleton: ready".to_string());
            lines.push(
                "next: Use `research-cli --json` for machine-readable launch inspection."
                    .to_string(),
            );
        }
        if let Some(data) = &self.data {
            append_text_lines("data", data, &mut lines);
        }
        lines.join("\n")
    }
}

fn success_status_label(envelope: &CommandSuccess) -> &'static str {
    if envelope
        .data
        .as_ref()
        .and_then(|data| data.get("launch_disposition"))
        .and_then(Value::as_str)
        == Some("interactive_skeleton_exit")
    {
        "ready"
    } else {
        "ok"
    }
}

impl TextRenderable for CommandFailure {
    fn render_text(&self) -> String {
        let mut lines = vec![format!("{}: failed", self.command)];
        if let Some(project_id) = &self.project_id {
            lines.push(format!("project_id: {project_id}"));
        }
        if let Some(session_id) = &self.session_id {
            lines.push(format!("session_id: {session_id}"));
        }
        lines.push(format!("error_code: {}", self.error.code));
        lines.push(format!("message: {}", self.error.message));
        if let Some(hint) = &self.error.hint {
            lines.push(format!("next: {hint}"));
        }
        if let Some(data) = &self.data {
            append_text_lines("data", data, &mut lines);
        }
        lines.join("\n")
    }
}

fn append_text_lines(prefix: &str, value: &Value, lines: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                append_text_lines(&format!("{prefix}.{key}"), nested, lines);
            }
        }
        Value::Array(items) => {
            lines.push(format!(
                "{prefix}: {}",
                serde_json::to_string(items).unwrap_or_default()
            ));
        }
        Value::Null => {}
        _ => lines.push(format!("{prefix}: {}", render_scalar(value))),
    }
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        _ => value.to_string(),
    }
}

fn parse_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn parse_flag_values(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        if args[index] == flag {
            if let Some(value) = args.get(index + 1) {
                values.push(value.clone());
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    values
}

fn parse_positional(args: &[String], label: &str) -> Result<String, String> {
    args.iter()
        .find(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("missing {label}"))
}

fn parse_trajectory_ingest_request(
    args: &[String],
) -> Result<trajectory::TrajectoryIngestRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--source-kind" | "--source-agent-id" | "--source-session-id" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            value if value.starts_with("--") => {
                return Err(format!(
                    "memory trajectory ingest received unknown argument: {value}"
                ));
            }
            _ => index += 1,
        }
    }
    let source_path = parse_positional(args, "trajectory source path")?;
    let source_kind =
        parse_flag_value(args, "--source-kind").unwrap_or_else(|| "cli_session".to_string());
    Ok(trajectory::TrajectoryIngestRequest {
        source_path,
        source_kind,
        source_agent_id: parse_flag_value(args, "--source-agent-id")
            .unwrap_or_else(|| "local".to_string()),
        source_session_id: parse_flag_value(args, "--source-session-id").unwrap_or_default(),
    })
}

fn parse_skill_list_filter(args: &[String]) -> Result<skills::SkillListFilter, String> {
    let mut filter = skills::SkillListFilter::default();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--include-disabled" => {
                filter.include_disabled = true;
                index += 1;
            }
            "--stage" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--stage requires a value".to_string())?;
                filter.stage = Some(value.clone());
                index += 2;
            }
            "--source" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--source requires a value".to_string())?;
                filter.source = Some(value.clone());
                index += 2;
            }
            "--json" => index += 1,
            other => return Err(format!("skills list received unknown argument: {other}")),
        }
    }
    Ok(filter)
}

fn parse_skill_output_submit_request(
    args: &[String],
) -> Result<skills::SkillOutputSubmitRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--human-gate" => index += 1,
            "--skill" | "--kind" | "--artifact" | "--artifact-kind" | "--family"
            | "--doc-frame" | "--policy" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            "--json" => index += 1,
            other => {
                return Err(format!(
                    "skills output submit received unknown argument: {other}"
                ))
            }
        }
    }

    let artifact_paths = parse_flag_values(args, "--artifact");
    let artifact_kinds = parse_flag_values(args, "--artifact-kind");
    let output_kind = parse_flag_value(args, "--kind")
        .ok_or_else(|| "skills output submit requires --kind".to_string())?;
    if !artifact_kinds.is_empty() && artifact_kinds.len() != artifact_paths.len() {
        return Err("--artifact-kind must be repeated once per --artifact".to_string());
    }
    let artifacts = artifact_paths
        .iter()
        .enumerate()
        .map(|(index, artifact_path)| skills::SkillOutputArtifactInput {
            artifact_path: artifact_path.clone(),
            artifact_kind: artifact_kinds
                .get(index)
                .cloned()
                .unwrap_or_else(|| output_kind.clone()),
        })
        .collect();

    Ok(skills::SkillOutputSubmitRequest {
        skill_id: parse_flag_value(args, "--skill")
            .ok_or_else(|| "skills output submit requires --skill".to_string())?,
        output_kind,
        artifacts,
        artifact_family: parse_flag_value(args, "--family")
            .ok_or_else(|| "skills output submit requires --family".to_string())?,
        doc_frame_path: parse_flag_value(args, "--doc-frame"),
        canonicality_policy: parse_flag_value(args, "--policy")
            .unwrap_or_else(|| "requires_review".to_string()),
        human_gate_required: args.iter().any(|arg| arg == "--human-gate"),
    })
}

fn parse_skill_run_request(args: &[String]) -> Result<skills::SkillRunRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--human-gate" => index += 1,
            "--skill" | "--adapter" | "--command" | "--kind" | "--artifact" | "--artifact-kind"
            | "--family" | "--doc-frame" | "--policy" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            "--json" => index += 1,
            other => return Err(format!("skills run received unknown argument: {other}")),
        }
    }
    let skill_id = parse_flag_value(args, "--skill")
        .ok_or_else(|| "skills run requires --skill".to_string())?;
    let runner_adapter =
        parse_flag_value(args, "--adapter").unwrap_or_else(|| "local-command".to_string());
    let command = parse_flag_value(args, "--command")
        .ok_or_else(|| "skills run requires --command".to_string())?;
    let mut submit_args = Vec::new();
    let mut submit_index = 0usize;
    while submit_index < args.len() {
        match args[submit_index].as_str() {
            "--adapter" | "--command" => submit_index += 2,
            other => {
                submit_args.push(other.to_string());
                submit_index += if matches!(other, "--human-gate" | "--json") {
                    1
                } else {
                    2
                };
                if !matches!(other, "--human-gate" | "--json") {
                    if let Some(value) = args.get(submit_index - 1) {
                        submit_args.push(value.clone());
                    }
                }
            }
        }
    }
    let mut submit_request = parse_skill_output_submit_request(&submit_args)?;
    submit_request.skill_id.clone_from(&skill_id);
    Ok(skills::SkillRunRequest {
        skill_id,
        runner_adapter,
        command,
        submit_request,
    })
}

fn parse_docs_publish_candidate_request(
    target: &str,
    args: &[String],
) -> Result<skills::SkillOutputSubmitRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--human-gate" => index += 1,
            "--kind" | "--family" | "--policy" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            "--json" => index += 1,
            other => {
                return Err(format!(
                    "docs publish-candidate received unknown argument: {other}"
                ))
            }
        }
    }
    let output_kind = parse_flag_value(args, "--kind").unwrap_or_else(|| "report".to_string());
    Ok(skills::SkillOutputSubmitRequest {
        skill_id: "docs.publish_candidate".to_string(),
        output_kind: output_kind.clone(),
        artifacts: vec![skills::SkillOutputArtifactInput {
            artifact_path: target.to_string(),
            artifact_kind: output_kind,
        }],
        artifact_family: parse_flag_value(args, "--family")
            .ok_or_else(|| "docs publish-candidate requires --family".to_string())?,
        doc_frame_path: Some(target.to_string()),
        canonicality_policy: parse_flag_value(args, "--policy")
            .unwrap_or_else(|| "requires_review".to_string()),
        human_gate_required: args.iter().any(|arg| arg == "--human-gate"),
    })
}

fn parse_optional_usize_flag(args: &[String], flag: &str) -> Result<Option<usize>, String> {
    match parse_flag_value(args, flag) {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| format!("{flag} must be a positive integer"))?;
            if parsed == 0 {
                return Err(format!("{flag} must be a positive integer"));
            }
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

fn reject_unknown_flags(
    command: &str,
    args: &[String],
    allowed_flags: &[&str],
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let mut index = 0usize;
    while index < args.len() {
        let value = args[index].as_str();
        if allowed_flags.contains(&value) {
            if value != "--json" {
                if args.get(index + 1).is_none() {
                    return Err(CommandFailureOutcome::usage(
                        command.to_string(),
                        "usage_invalid",
                        format!("{value} requires a value"),
                        None,
                    )
                    .with_output_json(output_json));
                }
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        return Err(CommandFailureOutcome::usage(
            command.to_string(),
            "usage_invalid",
            format!("{command} received unknown argument: {value}"),
            None,
        )
        .with_output_json(output_json));
    }
    Ok(())
}

fn reject_unknown_positionals_after_first(
    command: &str,
    args: &[String],
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let mut positional_count = 0usize;
    for value in args {
        if value == "--json" {
            continue;
        }
        if value.starts_with("--") {
            return Err(CommandFailureOutcome::usage(
                command.to_string(),
                "usage_invalid",
                format!("{command} received unknown argument: {value}"),
                None,
            )
            .with_output_json(output_json));
        }
        positional_count += 1;
        if positional_count > 1 {
            return Err(CommandFailureOutcome::usage(
                command.to_string(),
                "usage_invalid",
                format!("{command} received extra positional argument: {value}"),
                None,
            )
            .with_output_json(output_json));
        }
    }
    Ok(())
}

fn reject_unknown_positionals_and_flags_after_first(
    command: &str,
    args: &[String],
    allowed_flags: &[&str],
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let mut positional_count = 0usize;
    for value in args {
        if allowed_flags.contains(&value.as_str()) {
            continue;
        }
        if value.starts_with("--") {
            return Err(CommandFailureOutcome::usage(
                command.to_string(),
                "usage_invalid",
                format!("{command} received unknown argument: {value}"),
                None,
            )
            .with_output_json(output_json));
        }
        positional_count += 1;
        if positional_count > 1 {
            return Err(CommandFailureOutcome::usage(
                command.to_string(),
                "usage_invalid",
                format!("{command} received extra positional argument: {value}"),
                None,
            )
            .with_output_json(output_json));
        }
    }
    Ok(())
}

fn parse_working_memory_append_request(
    args: &[String],
) -> Result<WorkingMemoryAppendRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--pin" => index += 1,
            "--content" | "--kind" | "--support-ref" | "--limit" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            other => return Err(format!("memory append received unknown argument: {other}")),
        }
    }
    let content = parse_flag_value(args, "--content")
        .ok_or_else(|| "memory append requires --content".to_string())?;
    let kind = parse_flag_value(args, "--kind").unwrap_or_else(|| "note".to_string());
    let limit = parse_optional_usize_flag(args, "--limit")?;
    Ok(WorkingMemoryAppendRequest {
        kind,
        content,
        support_refs: parse_flag_values(args, "--support-ref"),
        pinned: has_flag(args, "--pin"),
        limit,
    })
}

fn parse_memory_promote_request(args: &[String]) -> Result<memory::MemoryPromoteRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--candidate-id" | "--trust" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            other => return Err(format!("memory promote received unknown argument: {other}")),
        }
    }
    let candidate_id = parse_flag_value(args, "--candidate-id")
        .ok_or_else(|| "memory promote requires --candidate-id".to_string())?;
    let trust = parse_flag_value(args, "--trust").unwrap_or_else(|| "supported".to_string());
    Ok(memory::MemoryPromoteRequest {
        candidate_id,
        trust,
    })
}

fn parse_memory_query_request(args: &[String]) -> Result<memory::MemoryQueryRequest, String> {
    let mut query_parts = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--include-inactive" => index += 1,
            "--limit" => {
                if args.get(index + 1).is_none() {
                    return Err("--limit requires a value".to_string());
                }
                index += 2;
            }
            value if value.starts_with("--") => {
                return Err(format!("memory query received unknown argument: {value}"));
            }
            value => {
                query_parts.push(value.to_string());
                index += 1;
            }
        }
    }
    let query = query_parts.join(" ");
    if query.trim().is_empty() {
        return Err("memory query requires query text".to_string());
    }
    Ok(memory::MemoryQueryRequest {
        query,
        limit: parse_optional_usize_flag(args, "--limit")?,
        include_inactive: has_flag(args, "--include-inactive"),
    })
}

fn parse_research_record_request(
    args: &[String],
) -> Result<research::ResearchRecordRequest, String> {
    let kind = parse_flag_value(args, "--kind")
        .ok_or_else(|| "research record requires --kind".to_string())?;
    let title = parse_flag_value(args, "--title")
        .ok_or_else(|| "research record requires --title".to_string())?;
    let stage_id = parse_flag_value(args, "--stage").unwrap_or_else(|| "refine".to_string());
    let mode = parse_flag_value(args, "--mode").unwrap_or_else(|| "exploring".to_string());
    Ok(research::ResearchRecordRequest {
        kind,
        title,
        stage_id,
        mode,
        decisions: parse_flag_values(args, "--decision"),
        open_questions: parse_flag_values(args, "--open-question"),
        evidence_refs: parse_flag_values(args, "--evidence-ref"),
    })
}

fn parse_autonomous_research_run_request(
    args: &[String],
) -> Result<AutonomousResearchRunRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--prompt"
            | "--workflow"
            | "--automation-mode"
            | "--max-ticks"
            | "--interval-ms"
            | "--report"
            | "--max-runtime-ms"
            | "--duration-hours"
            | "--duration-minutes"
            | "--review-model"
            | "--stage-task-semantic-review"
            | "--max-review-rounds"
            | "--permission-mode" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("research run {} requires a value", args[index]));
                }
                index += 2;
            }
            "--full-auto"
            | "--high-autonomy"
            | "--human-in-the-loop"
            | "--background"
            | "--json" => {
                index += 1;
            }
            other => {
                return Err(format!("research run received unknown argument: {other}"));
            }
        }
    }

    let prompt = parse_flag_value(args, "--prompt")
        .ok_or_else(|| "research run requires --prompt".to_string())?;
    if prompt.trim().is_empty() {
        return Err("research run --prompt must not be empty".to_string());
    }
    let workflow_profile = match parse_flag_value(args, "--workflow").as_deref() {
        None | Some("research-pipeline") => AutonomousWorkflowProfile::ResearchPipeline,
        Some("system-validation") => AutonomousWorkflowProfile::SystemValidation,
        Some(_) => {
            return Err(
                "research run --workflow must be research-pipeline or system-validation"
                    .to_string(),
            )
        }
    };
    let (automation_mode, automation_mode_source) =
        parse_autonomous_research_automation_mode(args)?;
    let max_ticks_explicit = parse_flag_value(args, "--max-ticks").is_some();
    let max_ticks = parse_optional_usize_flag(args, "--max-ticks")?
        .unwrap_or(DEFAULT_AUTONOMOUS_RESEARCH_JOB_MAX_TICKS);
    let interval_ms_explicit = parse_flag_value(args, "--interval-ms").is_some();
    let interval_ms = match parse_flag_value(args, "--interval-ms") {
        Some(value) => {
            let parsed = value
                .parse::<u64>()
                .map_err(|_| "research run --interval-ms must be a positive integer".to_string())?;
            if parsed == 0 {
                return Err("research run --interval-ms must be a positive integer".to_string());
            }
            parsed
        }
        None => DEFAULT_AUTONOMOUS_RESEARCH_JOB_INTERVAL_MS,
    };

    let max_runtime_ms = parse_autonomous_research_runtime_ms(args)?;
    let max_review_rounds = parse_optional_usize_flag(args, "--max-review-rounds")?
        .unwrap_or(DEFAULT_AUTONOMOUS_RESEARCH_REVIEW_ROUNDS);
    let stage_task_semantic_review_mode = parse_flag_value(args, "--stage-task-semantic-review")
        .map(|value| normalize_autonomous_research_stage_task_semantic_review_mode(&value))
        .transpose()?
        .unwrap_or_else(default_stage_task_semantic_review_mode);
    let permission_mode = parse_flag_value(args, "--permission-mode")
        .unwrap_or_else(|| "workspace-write".to_string());
    if !is_valid_permission_mode(&permission_mode) {
        return Err("research run --permission-mode must be read-only, workspace-write, or danger-full-access".to_string());
    }

    Ok(AutonomousResearchRunRequest {
        prompt,
        workflow_profile,
        automation_mode,
        automation_mode_source,
        max_ticks,
        max_ticks_explicit,
        interval_ms,
        interval_ms_explicit,
        report_path: parse_flag_value(args, "--report"),
        background: has_flag(args, "--background"),
        max_runtime_ms,
        review_model: parse_flag_value(args, "--review-model"),
        stage_task_semantic_review_mode,
        max_review_rounds,
        permission_mode,
    })
}

fn parse_autonomous_research_resume_request(
    args: &[String],
) -> Result<AutonomousResearchResumeRequest, String> {
    let job_id = args
        .get(2)
        .cloned()
        .ok_or_else(|| "missing job id".to_string())?;
    let mut index = 3usize;
    while index < args.len() {
        match args[index].as_str() {
            "--max-ticks" | "--additional-ticks" | "--max-runtime-ms" | "--duration-hours"
            | "--duration-minutes" => {
                if args.get(index + 1).is_none() {
                    return Err(format!(
                        "research jobs resume {} requires a value",
                        args[index]
                    ));
                }
                index += 2;
            }
            "--background" | "--json" => {
                index += 1;
            }
            other => {
                return Err(format!(
                    "research jobs resume received unknown argument: {other}"
                ));
            }
        }
    }

    Ok(AutonomousResearchResumeRequest {
        job_id,
        background: has_flag(args, "--background"),
        max_ticks: parse_optional_usize_flag(args, "--max-ticks")?,
        additional_ticks: parse_optional_usize_flag(args, "--additional-ticks")?,
        max_runtime_ms: parse_autonomous_research_resume_runtime_ms(args)?,
    })
}

fn normalize_autonomous_research_stage_task_semantic_review_mode(
    value: &str,
) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local" | "local-preflight" | "local_preflight" => Ok("local".to_string()),
        "provider" | "provider-backed" | "provider_backed" | "live" => Ok("provider".to_string()),
        other => Err(format!(
            "invalid research run --stage-task-semantic-review `{other}`; expected provider or hidden local diagnostic mode"
        )),
    }
}

fn parse_autonomous_research_automation_mode(
    args: &[String],
) -> Result<(GoalAutomationMode, String), String> {
    let mut selected: Option<(GoalAutomationMode, String)> = None;
    let mut set_mode = |mode: GoalAutomationMode, source: &str| -> Result<(), String> {
        if let Some((existing, existing_source)) = selected.as_ref() {
            if *existing != mode {
                return Err(format!(
                    "research run automation mode conflict: {existing_source} selects {}, but {source} selects {}",
                    existing.as_str(),
                    mode.as_str()
                ));
            }
            return Ok(());
        }
        selected = Some((mode, source.to_string()));
        Ok(())
    };

    if let Some(value) = parse_flag_value(args, "--automation-mode") {
        let mode = GoalAutomationMode::from_str(&value).ok_or_else(|| {
            "invalid research run --automation-mode; expected human_in_the_loop, high_autonomy, or full_auto"
                .to_string()
        })?;
        set_mode(mode, "--automation-mode")?;
    }
    if has_flag(args, "--full-auto") {
        set_mode(GoalAutomationMode::FullAuto, "--full-auto")?;
    }
    if has_flag(args, "--high-autonomy") {
        set_mode(GoalAutomationMode::HighAutonomy, "--high-autonomy")?;
    }
    if has_flag(args, "--human-in-the-loop") {
        set_mode(GoalAutomationMode::HumanInTheLoop, "--human-in-the-loop")?;
    }

    Ok(selected.unwrap_or((
        GoalAutomationMode::HighAutonomy,
        "default_high_autonomy".to_string(),
    )))
}

fn parse_autonomous_research_runtime_ms(args: &[String]) -> Result<Option<u64>, String> {
    if let Some(value) = parse_flag_value(args, "--max-runtime-ms") {
        let parsed = value
            .parse::<u64>()
            .map_err(|_| "research run --max-runtime-ms must be a positive integer".to_string())?;
        if parsed == 0 {
            return Err("research run --max-runtime-ms must be a positive integer".to_string());
        }
        return Ok(Some(parsed));
    }
    if let Some(value) = parse_flag_value(args, "--duration-hours") {
        let parsed = value
            .parse::<u64>()
            .map_err(|_| "research run --duration-hours must be a positive integer".to_string())?;
        if parsed == 0 {
            return Err("research run --duration-hours must be a positive integer".to_string());
        }
        return Ok(Some(parsed.saturating_mul(60 * 60 * 1_000)));
    }
    if let Some(value) = parse_flag_value(args, "--duration-minutes") {
        let parsed = value.parse::<u64>().map_err(|_| {
            "research run --duration-minutes must be a positive integer".to_string()
        })?;
        if parsed == 0 {
            return Err("research run --duration-minutes must be a positive integer".to_string());
        }
        return Ok(Some(parsed.saturating_mul(60 * 1_000)));
    }
    Ok(None)
}

fn parse_autonomous_research_resume_runtime_ms(args: &[String]) -> Result<Option<u64>, String> {
    if let Some(value) = parse_flag_value(args, "--max-runtime-ms") {
        let parsed = value.parse::<u64>().map_err(|_| {
            "research jobs resume --max-runtime-ms must be a positive integer".to_string()
        })?;
        if parsed == 0 {
            return Err(
                "research jobs resume --max-runtime-ms must be a positive integer".to_string(),
            );
        }
        return Ok(Some(parsed));
    }
    if let Some(value) = parse_flag_value(args, "--duration-hours") {
        let parsed = value.parse::<u64>().map_err(|_| {
            "research jobs resume --duration-hours must be a positive integer".to_string()
        })?;
        if parsed == 0 {
            return Err(
                "research jobs resume --duration-hours must be a positive integer".to_string(),
            );
        }
        return Ok(Some(parsed.saturating_mul(60 * 60 * 1_000)));
    }
    if let Some(value) = parse_flag_value(args, "--duration-minutes") {
        let parsed = value.parse::<u64>().map_err(|_| {
            "research jobs resume --duration-minutes must be a positive integer".to_string()
        })?;
        if parsed == 0 {
            return Err(
                "research jobs resume --duration-minutes must be a positive integer".to_string(),
            );
        }
        return Ok(Some(parsed.saturating_mul(60 * 1_000)));
    }
    Ok(None)
}

fn parse_research_decision_request(
    args: &[String],
) -> Result<research::ResearchDecisionRequest, String> {
    let thread_id = parse_flag_value(args, "--thread")
        .ok_or_else(|| "research decide requires --thread".to_string())?;
    let operation = parse_flag_value(args, "--operation")
        .ok_or_else(|| "research decide requires --operation".to_string())?;
    let decision = parse_flag_value(args, "--decision")
        .ok_or_else(|| "research decide requires --decision".to_string())?;
    Ok(research::ResearchDecisionRequest {
        thread_id,
        operation,
        decision,
        reason: parse_flag_value(args, "--reason").unwrap_or_default(),
        evidence_refs: parse_flag_values(args, "--evidence-ref"),
    })
}

fn parse_research_classify_request(
    args: &[String],
) -> Result<research::ResearchClassifyRequest, String> {
    let text = parse_flag_value(args, "--text")
        .ok_or_else(|| "research classify requires --text".to_string())?;
    Ok(research::ResearchClassifyRequest {
        text,
        thread_id: parse_flag_value(args, "--thread"),
        dry_run: has_flag(args, "--dry-run"),
    })
}

fn parse_research_middleware_plan_request(
    args: &[String],
) -> Result<research::ResearchMiddlewarePlanRequest, String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--role" | "--intent" | "--thread" | "--stage-execution" | "--candidate-tool"
            | "--tool-error" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{} requires a value", args[index]));
                }
                index += 2;
            }
            other => {
                return Err(format!(
                    "research middleware plan received unknown argument: {other}"
                ))
            }
        }
    }
    let role = parse_flag_value(args, "--role")
        .ok_or_else(|| "research middleware plan requires --role".to_string())?;
    let intent = parse_flag_value(args, "--intent")
        .ok_or_else(|| "research middleware plan requires --intent".to_string())?;
    Ok(research::ResearchMiddlewarePlanRequest {
        role,
        intent,
        thread_id: parse_flag_value(args, "--thread"),
        stage_execution_id: parse_flag_value(args, "--stage-execution"),
        candidate_tools: parse_flag_values(args, "--candidate-tool"),
        tool_error: parse_flag_value(args, "--tool-error"),
    })
}

fn parse_routine_create_request(args: &[String]) -> Result<routines::RoutineCreateRequest, String> {
    let name = parse_flag_value(args, "--name")
        .ok_or_else(|| "routines create requires --name".to_string())?;
    let trigger_kind =
        parse_flag_value(args, "--trigger-kind").unwrap_or_else(|| "manual".to_string());
    let intent = parse_flag_value(args, "--intent")
        .ok_or_else(|| "routines create requires --intent".to_string())?;
    let role_profile = parse_flag_value(args, "--role-profile")
        .ok_or_else(|| "routines create requires --role-profile".to_string())?;
    let message = parse_flag_value(args, "--message")
        .ok_or_else(|| "routines create requires --message".to_string())?;
    let command = parse_flag_value(args, "--command")
        .ok_or_else(|| "routines create requires --command".to_string())?;
    let delivery_target =
        parse_flag_value(args, "--delivery-target").unwrap_or_else(|| "projectops".to_string());
    Ok(routines::RoutineCreateRequest {
        name,
        trigger_kind,
        intent,
        role_profile,
        message,
        command,
        delivery_target,
    })
}

fn parse_routine_trigger_request(
    args: &[String],
) -> Result<routines::RoutineTriggerRequest, String> {
    let routine_id = args
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .or_else(|| parse_flag_value(args, "--routine-id"))
        .ok_or_else(|| "routines trigger requires a routine id".to_string())?;
    let trigger_kind =
        parse_flag_value(args, "--trigger-kind").unwrap_or_else(|| "manual".to_string());
    let source = parse_flag_value(args, "--source").unwrap_or_else(|| "operator".to_string());
    let stale_after_sec = parse_flag_value(args, "--stale-after-sec")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(900);
    Ok(routines::RoutineTriggerRequest {
        routine_id,
        trigger_kind,
        source,
        stale_after_sec,
        max_retry_attempts: routines::DEFAULT_MAX_RETRY_ATTEMPTS,
        retry_backoff_ms: routines::DEFAULT_RETRY_BACKOFF_MS,
    })
}

fn parse_routine_ingress_request(
    args: &[String],
) -> Result<routines::RoutineIngressRequest, String> {
    let routine_id = args
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .or_else(|| parse_flag_value(args, "--routine-id"))
        .ok_or_else(|| "routines ingress requires a routine id".to_string())?;
    let trigger_kind =
        parse_flag_value(args, "--trigger-kind").unwrap_or_else(|| "manual".to_string());
    let source = parse_flag_value(args, "--source")
        .ok_or_else(|| "routines ingress requires --source".to_string())?;
    let dedupe_key = parse_flag_value(args, "--dedupe-key").unwrap_or_default();
    let ready_at = parse_flag_value(args, "--ready-at");
    Ok(routines::RoutineIngressRequest {
        routine_id,
        trigger_kind,
        source,
        dedupe_key,
        ready_at,
    })
}

fn parse_routine_run_due_request(
    args: &[String],
) -> Result<routines::RoutineRunDueRequest, String> {
    let limit = match parse_flag_value(args, "--limit") {
        Some(value) => Some(
            value
                .parse::<usize>()
                .map_err(|_| "routines run-due --limit must be a positive integer".to_string())?,
        ),
        None => None,
    };
    let stale_after_sec = parse_flag_value(args, "--stale-after-sec")
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                "routines run-due --stale-after-sec must be a positive integer".to_string()
            })
        })
        .transpose()?
        .unwrap_or(900);
    Ok(routines::RoutineRunDueRequest {
        trigger_kind: parse_flag_value(args, "--trigger-kind"),
        limit,
        stale_after_sec,
        max_retry_attempts: routines::DEFAULT_MAX_RETRY_ATTEMPTS,
        retry_backoff_ms: routines::DEFAULT_RETRY_BACKOFF_MS,
    })
}

fn parse_routine_retry_request(args: &[String]) -> Result<routines::RoutineRetryRequest, String> {
    let trigger_id = args
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .or_else(|| parse_flag_value(args, "--trigger-id"))
        .ok_or_else(|| "routines retry requires a trigger id".to_string())?;
    let source = parse_flag_value(args, "--source").unwrap_or_else(|| "operator-retry".to_string());
    let stale_after_sec = parse_flag_value(args, "--stale-after-sec")
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                "routines retry --stale-after-sec must be a positive integer".to_string()
            })
        })
        .transpose()?
        .unwrap_or(900);
    Ok(routines::RoutineRetryRequest {
        trigger_id,
        source,
        stale_after_sec,
        max_retry_attempts: routines::DEFAULT_MAX_RETRY_ATTEMPTS,
        retry_backoff_ms: routines::DEFAULT_RETRY_BACKOFF_MS,
    })
}

fn tool_parameters_from_args(args: &[String]) -> Vec<(String, String)> {
    [
        "--pattern",
        "--old",
        "--new",
        "--command",
        "--url",
        "--allow-private-network",
    ]
    .iter()
    .filter_map(|flag| {
        parse_flag_value(args, flag)
            .map(|value| (flag.trim_start_matches("--").replace('-', "_"), value))
    })
    .collect()
}

fn tool_input_payload(tool_name: &str, args: &[String]) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("tool_name".to_string(), json!(tool_name));
    if let Some(path) = parse_flag_value(args, "--path") {
        object.insert("path".to_string(), json!(path));
    }
    if let Some(content) =
        parse_flag_value(args, "--content").or_else(|| parse_flag_value(args, "--command"))
    {
        object.insert("content".to_string(), json!(content));
    }
    for (key, value) in tool_parameters_from_args(args) {
        object.insert(key, json!(value));
    }
    Value::Object(object)
}

fn permission_input_digest(
    input_payload: &Value,
    permission_mode: &str,
    evaluation: &crate::permissions::PermissionCheckResult,
) -> String {
    let payload = json!({
        "tool_input": input_payload,
        "permission_mode": permission_mode,
        "reason_code": evaluation.reason_code,
        "requires_approval": evaluation.requires_approval,
        "workspace_boundary_ok": evaluation.workspace_boundary_ok
    });
    let bytes = serde_json::to_vec(&payload).expect("permission digest payload should serialize");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonicality_cleanup_gate(
    unresolved_violation_count: usize,
    repair_action_count: usize,
) -> artifacts::RepoCleanupGate {
    if unresolved_violation_count == 0 {
        artifacts::RepoCleanupGate {
            gate_id: "canonicality_audit".to_string(),
            status: "satisfied".to_string(),
            detail: if repair_action_count == 0 {
                "canonicality audit has no blocking violations".to_string()
            } else {
                format!(
                    "canonicality audit produced {repair_action_count} safe cleanup repair actions"
                )
            },
        }
    } else {
        artifacts::RepoCleanupGate {
            gate_id: "canonicality_audit".to_string(),
            status: "blocked".to_string(),
            detail: format!(
                "canonicality audit has {unresolved_violation_count} unresolved blocking violations"
            ),
        }
    }
}

fn ensure_canonicality_promotion_gate(
    resolved: &ResolvedProject,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let report = canonicality::audit_canonical_with_mode(
        &resolved.workspace_root,
        &resolved.data_dir,
        &resolved.project_id,
        true,
    )
    .map_err(|err| internal_failure("artifacts promote", err.to_string()))?;
    if !report.ok {
        return Err(canonicality_gate_failure(
            resolved,
            "release canonicality audit has blocking violations before artifact promotion",
            serde_json::to_value(report).expect("canonicality audit report should serialize"),
            output_json,
        ));
    }
    Ok(())
}

fn canonicality_gate_failure(
    resolved: &ResolvedProject,
    message: &str,
    data: Value,
    output_json: bool,
) -> CommandFailureOutcome {
    CommandFailureOutcome::new(
        9,
        "artifacts promote".to_string(),
        Some(resolved.project_id.clone()),
        None,
        "canonicality_gate_blocked",
        message.to_string(),
        Some("Run `research-cli projects audit --canonical --release --json` and write explicit canonical manifests before promotion.".to_string()),
        false,
        Some(data),
        output_json,
    )
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn has_unrecognized_positionals(args: &[String], recognized_flags: &[&str]) -> bool {
    let mut index = 0usize;
    while index < args.len() {
        let current = args[index].as_str();
        if recognized_flags.contains(&current) {
            if args.get(index + 1).is_none() {
                return true;
            }
            index += 2;
            continue;
        }
        return true;
    }
    false
}

fn has_unrecognized_remote_args(
    args: &[String],
    recognized_value_flags: &[&str],
    recognized_bool_flags: &[&str],
) -> bool {
    let mut index = 0usize;
    while index < args.len() {
        let current = args[index].as_str();
        if recognized_value_flags.contains(&current) {
            if args.get(index + 1).is_none() {
                return true;
            }
            index += 2;
            continue;
        }
        if recognized_bool_flags.contains(&current) {
            index += 1;
            continue;
        }
        return true;
    }
    false
}

#[derive(Debug, Clone)]
pub(crate) struct PromptInvocation {
    session_id: Option<String>,
    new_session: bool,
    prompt_text: String,
    provider: Option<String>,
    model: Option<String>,
    permission_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PromptRequestDefaults {
    provider: Option<String>,
    provider_source: Option<String>,
    model: Option<String>,
    model_source: Option<String>,
    reasoning_effort: Option<String>,
    reasoning_effort_source: Option<String>,
    permission_mode: String,
    permission_mode_source: Option<String>,
}

pub(crate) struct ProviderRouteSelection {
    provider: Option<String>,
    provider_source: Option<String>,
    model: Option<String>,
    model_source: Option<String>,
    failover_used: bool,
}

fn provider_model_for_configured_route_result(
    effective: &crate::config::EffectiveConfig,
    provider: Option<&str>,
    requested_model: Option<&str>,
    provider_source: Option<&str>,
    model_source: Option<&str>,
) -> Result<ProviderRouteSelection, ProviderResolveError> {
    let trace = resolve_provider_trace_with_profiles(
        provider,
        requested_model,
        provider_source,
        model_source,
        &effective.provider_profiles,
    )?;
    if trace.auth_status != "configured" {
        return Err(ProviderResolveError::UnknownProvider(
            trace.resolved_provider,
        ));
    }
    if provider_source != Some("cli_flag")
        && model_source.is_some()
        && !provider_profile_supports_model(
            effective,
            &trace.resolved_provider,
            &trace.resolved_model,
        )
    {
        return Err(ProviderResolveError::IncompatibleProviderModel {
            provider_id: trace.resolved_provider,
            model: trace.resolved_model,
        });
    }
    Ok(ProviderRouteSelection {
        provider: Some(trace.resolved_provider),
        provider_source: provider_source.map(ToString::to_string),
        model: Some(trace.resolved_model),
        model_source: model_source.map(ToString::to_string),
        failover_used: false,
    })
}

fn provider_profile_supports_model(
    effective: &crate::config::EffectiveConfig,
    provider: &str,
    model: &str,
) -> bool {
    let Some(profile) =
        crate::config::provider_profile_for_provider(&effective.provider_profiles, provider)
    else {
        return true;
    };
    let supported = if profile.supported_models.is_empty() {
        profile
            .default_model
            .as_ref()
            .map(|default_model| vec![default_model.clone()])
            .unwrap_or_default()
    } else {
        profile.supported_models.clone()
    };
    supported.iter().any(|candidate| candidate == model)
}

fn provider_model_for_requested_model(
    effective: &crate::config::EffectiveConfig,
    requested_model: Option<&str>,
    model_source: Option<&str>,
    excluded_providers: &[String],
) -> Option<ProviderRouteSelection> {
    let requested_model = requested_model?;
    let trace = resolve_provider_trace_with_profiles(
        None,
        Some(requested_model),
        None,
        model_source,
        &effective.provider_profiles,
    )
    .ok()
    .filter(|trace| trace.auth_status == "configured")?;
    if excluded_providers
        .iter()
        .any(|excluded| excluded == &trace.resolved_provider)
    {
        return None;
    }
    Some(ProviderRouteSelection {
        provider: Some(trace.resolved_provider),
        provider_source: Some("model_catalog_match".to_string()),
        model: Some(trace.resolved_model),
        model_source: model_source.map(ToString::to_string),
        failover_used: false,
    })
}

fn select_configured_provider_route(
    resolved: &ResolvedProject,
    preferred_provider: Option<String>,
    preferred_provider_source: Option<String>,
    preferred_model: Option<String>,
    preferred_model_source: Option<String>,
    excluded_providers: &[String],
) -> Option<ProviderRouteSelection> {
    let effective = crate::config::effective_config(resolved).ok()?.effective;
    let is_excluded = |provider: Option<&str>| {
        provider
            .map(|candidate| {
                excluded_providers
                    .iter()
                    .any(|excluded| excluded == candidate)
            })
            .unwrap_or(false)
    };

    if !is_excluded(preferred_provider.as_deref()) {
        match provider_model_for_configured_route_result(
            &effective,
            preferred_provider.as_deref(),
            preferred_model.as_deref(),
            preferred_provider_source.as_deref(),
            preferred_model_source.as_deref(),
        ) {
            Ok(selection) => {
                if !is_excluded(selection.provider.as_deref()) {
                    return Some(selection);
                }
            }
            Err(ProviderResolveError::IncompatibleProviderModel { .. })
                if preferred_provider_source.as_deref() != Some("cli_flag") =>
            {
                if let Some(selection) = provider_model_for_requested_model(
                    &effective,
                    preferred_model.as_deref(),
                    preferred_model_source.as_deref(),
                    excluded_providers,
                ) {
                    return Some(selection);
                }
            }
            Err(_) => {}
        }
    }

    if preferred_provider.is_none() {
        if let Some(selection) = provider_model_for_requested_model(
            &effective,
            preferred_model.as_deref(),
            preferred_model_source.as_deref(),
            excluded_providers,
        ) {
            if !is_excluded(selection.provider.as_deref()) {
                return Some(selection);
            }
        }
    }

    for provider in effective.provider_failover {
        if provider.trim().is_empty() || is_excluded(Some(provider.as_str())) {
            continue;
        }
        let trace = resolve_provider_trace_with_profiles(
            Some(provider.as_str()),
            None,
            Some("provider_failover"),
            None,
            &effective.provider_profiles,
        )
        .ok()
        .filter(|trace| trace.auth_status == "configured");
        if let Some(trace) = trace {
            return Some(ProviderRouteSelection {
                provider: Some(trace.resolved_provider),
                provider_source: Some("provider_failover".to_string()),
                model: Some(trace.resolved_model),
                model_source: Some("provider_profile_default".to_string()),
                failover_used: true,
            });
        }
    }

    None
}

fn autonomous_research_select_provider_route_for_main_agent(
    resolved: &ResolvedProject,
    defaults: &PromptRequestDefaults,
    job: Option<&AutonomousResearchJobState>,
) -> Option<ProviderRouteSelection> {
    let excluded_providers = job
        .map(|job| autonomous_research_backoff_excluded_providers(resolved, job))
        .unwrap_or_default();
    autonomous_research_select_provider_route(
        resolved,
        defaults.provider.clone(),
        defaults.provider_source.clone(),
        defaults.model.clone(),
        defaults.model_source.clone(),
        &excluded_providers,
    )
}

fn autonomous_research_backoff_excluded_providers(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<String> {
    let mut excluded = Vec::new();
    let config = effective_config(resolved).ok();
    let profiles = config
        .as_ref()
        .map(|config| &config.effective.provider_profiles);
    for fault in &job.provider_faults {
        if !autonomous_research_historical_provider_fault_excludes_provider_for_recovery(
            fault, profiles,
        ) {
            continue;
        }
        if let Some(provider_id) = fault.provider_id.as_ref() {
            merge_unique_strings(&mut excluded, vec![provider_id.clone()]);
        }
    }
    if let Some(fault) = job.active_provider_fault.as_ref() {
        if autonomous_research_provider_fault_excludes_provider_for_recovery(fault) {
            if let Some(provider_id) = fault.provider_id.as_ref() {
                merge_unique_strings(&mut excluded, vec![provider_id.clone()]);
            }
        }
    }
    excluded
}

fn autonomous_research_select_provider_route(
    resolved: &ResolvedProject,
    preferred_provider: Option<String>,
    preferred_provider_source: Option<String>,
    preferred_model: Option<String>,
    preferred_model_source: Option<String>,
    excluded_providers: &[String],
) -> Option<ProviderRouteSelection> {
    select_configured_provider_route(
        resolved,
        preferred_provider,
        preferred_provider_source,
        preferred_model,
        preferred_model_source,
        excluded_providers,
    )
}

fn autonomous_research_available_provider_recovery_route(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Option<ProviderRouteSelection> {
    let defaults = resolve_prompt_request_defaults(
        resolved,
        &PromptInvocation {
            session_id: None,
            new_session: false,
            prompt_text: String::new(),
            provider: None,
            model: None,
            permission_mode: Some("read-only".to_string()),
        },
    )
    .ok()?;
    let mut excluded_providers = autonomous_research_backoff_excluded_providers(resolved, job);
    if let Some(provider_id) = job
        .active_provider_fault
        .as_ref()
        .and_then(|fault| fault.provider_id.clone())
    {
        merge_unique_strings(&mut excluded_providers, vec![provider_id]);
    }
    autonomous_research_select_provider_route(
        resolved,
        defaults.provider,
        defaults.provider_source,
        defaults.model,
        defaults.model_source,
        &excluded_providers,
    )
}

fn autonomous_research_operator_gate_recovery_route(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    fault: &AutonomousResearchProviderFaultState,
) -> Option<ProviderRouteSelection> {
    if !fault.operator_gate_required {
        return None;
    }
    autonomous_research_available_provider_recovery_route(resolved, job)
}

fn resolve_autonomous_research_provider_route(
    resolved: &ResolvedProject,
    review_model: &Option<String>,
) -> Option<ProviderRouteSelection> {
    autonomous_research_select_provider_route_for_review_model(resolved, review_model, None)
}

fn resolve_autonomous_research_agent_team_worker_route(
    resolved: &ResolvedProject,
    job: Option<&AutonomousResearchJobState>,
) -> Option<ProviderRouteSelection> {
    let defaults = resolve_prompt_request_defaults(
        resolved,
        &PromptInvocation {
            session_id: None,
            new_session: false,
            prompt_text: String::new(),
            provider: None,
            model: None,
            permission_mode: Some("workspace-write".to_string()),
        },
    )
    .ok()?;
    let excluded_providers = job
        .map(|job| autonomous_research_backoff_excluded_providers(resolved, job))
        .unwrap_or_default();
    autonomous_research_select_provider_route(
        resolved,
        defaults.provider,
        defaults.provider_source,
        defaults.model,
        defaults.model_source,
        &excluded_providers,
    )
}

fn autonomous_research_select_provider_route_for_review_model(
    resolved: &ResolvedProject,
    review_model: &Option<String>,
    job: Option<&AutonomousResearchJobState>,
) -> Option<ProviderRouteSelection> {
    let defaults = resolve_prompt_request_defaults(
        resolved,
        &PromptInvocation {
            session_id: None,
            new_session: false,
            prompt_text: String::new(),
            provider: None,
            model: review_model.clone(),
            permission_mode: Some("read-only".to_string()),
        },
    )
    .ok()?;
    let excluded_providers = job
        .map(|job| autonomous_research_backoff_excluded_providers(resolved, job))
        .unwrap_or_default();
    autonomous_research_select_provider_route(
        resolved,
        defaults.provider,
        defaults.provider_source,
        defaults.model,
        defaults.model_source,
        &excluded_providers,
    )
}

fn parse_prompt_invocation(args: &[String]) -> Result<PromptInvocation, CommandFailureOutcome> {
    let mut session_id = None;
    let mut new_session = false;
    let mut provider = None;
    let mut model = None;
    let mut permission_mode = None;
    let mut prompt_parts = Vec::new();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--session" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "prompt".to_string(),
                        "usage_invalid",
                        "missing value for --session".to_string(),
                        Some("Pass an existing session id after --session.".to_string()),
                    )
                })?;
                session_id = Some(value.clone());
                index += 2;
            }
            "--new-session" => {
                new_session = true;
                index += 1;
            }
            "--provider" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "prompt".to_string(),
                        "usage_invalid",
                        "missing value for --provider".to_string(),
                        Some("Pass a provider id such as `openai` or `anthropic`.".to_string()),
                    )
                })?;
                provider = Some(value.clone());
                index += 2;
            }
            "--model" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "prompt".to_string(),
                        "usage_invalid",
                        "missing value for --model".to_string(),
                        Some(
                            "Pass a model id such as `gpt-5.4` or `claude-sonnet-4-6`.".to_string(),
                        ),
                    )
                })?;
                model = Some(value.clone());
                index += 2;
            }
            "--permission-mode" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "prompt".to_string(),
                        "usage_invalid",
                        "missing value for --permission-mode".to_string(),
                        Some("Pass read-only, workspace-write, or danger-full-access.".to_string()),
                    )
                })?;
                permission_mode = Some(value.clone());
                index += 2;
            }
            value if value.starts_with("--") => {
                return Err(CommandFailureOutcome::usage(
                    "prompt".to_string(),
                    "usage_invalid",
                    format!("unknown prompt flag: {value}"),
                    Some(
                        "Use `--session`, `--new-session`, `--provider`, `--model`, or `--permission-mode` before the prompt payload.".to_string(),
                    ),
                ));
            }
            value => {
                prompt_parts.push(value.to_string());
                index += 1;
            }
        }
    }

    if prompt_parts.is_empty() {
        return Err(CommandFailureOutcome::usage(
            "prompt".to_string(),
            "usage_invalid",
            "prompt requires one bounded prompt payload".to_string(),
            Some("Use `research-cli prompt \"status\" --json`.".to_string()),
        ));
    }

    if session_id.is_some() && new_session {
        return Err(CommandFailureOutcome::usage(
            "prompt".to_string(),
            "usage_invalid",
            "--session and --new-session cannot be combined".to_string(),
            Some("Choose one prompt session routing mode.".to_string()),
        ));
    }

    Ok(PromptInvocation {
        session_id,
        new_session,
        provider,
        model,
        permission_mode,
        prompt_text: prompt_parts.join(" "),
    })
}

fn prompt_session_title(prompt_text: &str) -> String {
    let trimmed = prompt_text.trim();
    let mut shortened = trimmed.chars().take(40).collect::<String>();
    if trimmed.chars().count() > 40 {
        shortened.push_str("...");
    }
    format!("Prompt: {shortened}")
}

fn resolve_prompt_request_defaults(
    resolved: &ResolvedProject,
    invocation: &PromptInvocation,
) -> Result<PromptRequestDefaults, ConfigError> {
    let effective = effective_config(resolved)?.effective;
    let provider = invocation.provider.clone().or(effective.default_provider);
    let provider_source = if invocation.provider.is_some() {
        Some("cli_flag".to_string())
    } else {
        config::effective_value_source(resolved, "default_provider")?
    };
    let provider_profile_default_model = provider
        .as_deref()
        .and_then(|provider| {
            crate::config::provider_profile_for_provider(&effective.provider_profiles, provider)
        })
        .and_then(|profile| profile.default_model.clone());
    let global_default_model = effective.default_model.clone();
    let should_use_provider_profile_model = provider_source.as_deref() == Some("cli_flag")
        || global_default_model
            .as_ref()
            .zip(provider_profile_default_model.as_ref())
            .map(|(global, profile_default)| global != profile_default)
            .unwrap_or(false);
    let model = if invocation.model.is_some() {
        invocation.model.clone()
    } else if should_use_provider_profile_model {
        provider_profile_default_model.or(global_default_model)
    } else {
        global_default_model
    };
    let model_source = if invocation.model.is_some() {
        Some("cli_flag".to_string())
    } else if should_use_provider_profile_model {
        Some("provider_profile_default".to_string())
    } else {
        config::effective_value_source(resolved, "default_model")?
    };
    let reasoning_effort = effective
        .values
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let reasoning_effort_source = config::effective_value_source(resolved, "reasoning_effort")?;
    let permission_mode = invocation
        .permission_mode
        .clone()
        .or(effective.permission_mode)
        .unwrap_or_else(|| "read-only".to_string());
    let permission_mode_source = if invocation.permission_mode.is_some() {
        Some("cli_flag".to_string())
    } else {
        config::effective_value_source(resolved, "permission_mode")?
            .or_else(|| Some("built_in_default".to_string()))
    };

    Ok(PromptRequestDefaults {
        provider,
        provider_source,
        model,
        model_source,
        reasoning_effort,
        reasoning_effort_source,
        permission_mode,
        permission_mode_source,
    })
}

fn resolve_model_current_result(
    resolved: &ResolvedProject,
) -> Result<ModelCurrentResult, ConfigError> {
    let prompt_defaults = resolve_prompt_request_defaults(
        resolved,
        &PromptInvocation {
            session_id: None,
            new_session: false,
            prompt_text: String::new(),
            provider: None,
            model: None,
            permission_mode: None,
        },
    )?;
    let provider_trace = resolve_provider_trace(
        prompt_defaults.provider.as_deref(),
        prompt_defaults.model.as_deref(),
        prompt_defaults.provider_source.as_deref(),
        prompt_defaults.model_source.as_deref(),
    )
    .map_err(provider_resolve_to_config_error)?;
    let config_source = prompt_defaults
        .model_source
        .or(prompt_defaults.provider_source)
        .unwrap_or_else(|| "default".to_string());

    Ok(ModelCurrentResult {
        provider_id: provider_trace.resolved_provider,
        model: provider_trace.resolved_model,
        resolved_scope: "project".to_string(),
        config_source,
    })
}

fn provider_resolve_to_config_error(err: ProviderResolveError) -> ConfigError {
    ConfigError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        err.to_string(),
    ))
}

fn telemetry_failure(
    command: &str,
    err: telemetry::TelemetryError,
    resolved: &ResolvedProject,
) -> CommandFailureOutcome {
    match err {
        telemetry::TelemetryError::SessionStore(inner) => CommandFailureOutcome::new(
            10,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "telemetry_unavailable",
            inner.to_string(),
            Some("Inspect the session artifacts and retry.".to_string()),
            false,
            Some(json!({
                "project_id": resolved.project_id
            })),
            false,
        ),
        telemetry::TelemetryError::Registry(inner) => CommandFailureOutcome::new(
            10,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "telemetry_unavailable",
            inner.to_string(),
            Some("Inspect the project registry and retry.".to_string()),
            false,
            Some(json!({
                "project_id": resolved.project_id
            })),
            false,
        ),
    }
}

fn resolve_prompt_provider_trace(
    requested_provider: Option<&str>,
    requested_model: Option<&str>,
    requested_provider_source: Option<&str>,
    requested_model_source: Option<&str>,
) -> Result<crate::providers::ProviderResolutionTrace, CommandFailureOutcome> {
    match resolve_provider_trace(
        requested_provider,
        requested_model,
        requested_provider_source,
        requested_model_source,
    ) {
        Ok(trace) => Ok(trace),
        Err(ProviderResolveError::UnknownProvider(provider)) => Err(CommandFailureOutcome::new(
            3,
            "prompt".to_string(),
            None,
            None,
            "provider_not_found",
            format!("unknown provider: {provider}"),
            Some("Inspect the embedded provider catalog before forcing a provider.".to_string()),
            false,
            Some(json!({
                "requested_provider": provider
            })),
            false,
        )),
        Err(ProviderResolveError::UnsupportedModel(model)) => Err(CommandFailureOutcome::new(
            3,
            "prompt".to_string(),
            None,
            None,
            "model_not_supported",
            format!("unsupported model: {model}"),
            Some("Choose a model that exists in the embedded provider catalog.".to_string()),
            false,
            Some(json!({
                "requested_model": model
            })),
            false,
        )),
        Err(ProviderResolveError::IncompatibleProviderModel { provider_id, model }) => {
            Err(CommandFailureOutcome::new(
                3,
                "prompt".to_string(),
                None,
                None,
                "provider_model_incompatible",
                format!("provider {provider_id} does not support model {model}"),
                Some(
                    "Choose a model supported by the requested provider, or remove the explicit provider override.".to_string(),
                ),
                false,
                Some(json!({
                    "requested_provider": provider_id,
                    "resolved_provider": provider_id,
                    "requested_model": model,
                    "resolution_reason": "explicit_provider_incompatible_model"
                })),
                false,
            ))
        }
    }
}

fn parse_resume_selector(selector_raw: &str) -> ResumeSelector {
    match selector_raw {
        "latest" => ResumeSelector::Latest,
        value if value.starts_with("sess_") => ResumeSelector::Exact(value.to_string()),
        value => ResumeSelector::Prefix(value.to_string()),
    }
}

fn parse_mission_frame_update(args: &[String]) -> Result<MissionFrameUpdate, String> {
    let mut project_max_goal = None;
    let mut milestone_goal = None;
    let mut current_implementation_goal = None;
    let mut non_goals = Vec::new();
    let mut success_criteria = Vec::new();
    let mut evidence_refs = Vec::new();
    let mut risk_notes = Vec::new();
    let mut automation_mode = None;
    let mut index = 0;

    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag {
            "--project-max-goal" => project_max_goal = Some(value.clone()),
            "--milestone-goal" => milestone_goal = Some(value.clone()),
            "--current-implementation-goal" => current_implementation_goal = Some(value.clone()),
            "--non-goal" => non_goals.push(value.clone()),
            "--success-criterion" => success_criteria.push(value.clone()),
            "--evidence-ref" => evidence_refs.push(value.clone()),
            "--risk-note" => risk_notes.push(value.clone()),
            "--automation-mode" => {
                automation_mode = Some(
                    GoalAutomationMode::from_str(value)
                        .ok_or_else(|| {
                            "invalid --automation-mode; expected human_in_the_loop, high_autonomy, or full_auto"
                                .to_string()
                        })?,
                )
            }
            other => return Err(format!("unknown goals set flag: {other}")),
        }
        index += 2;
    }

    Ok(MissionFrameUpdate {
        project_max_goal: project_max_goal
            .ok_or_else(|| "missing --project-max-goal".to_string())?,
        milestone_goal: milestone_goal.ok_or_else(|| "missing --milestone-goal".to_string())?,
        current_implementation_goal: current_implementation_goal
            .ok_or_else(|| "missing --current-implementation-goal".to_string())?,
        non_goals,
        success_criteria,
        evidence_refs,
        risk_notes,
        automation_mode,
    })
}

fn parse_goal_tick_request(args: &[String]) -> Result<routines::RoutineRunDueRequest, String> {
    parse_routine_run_due_request(args).map_err(|err| err.replace("routines run-due", "goals tick"))
}

fn parse_goal_watch_no_args(command: &str, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Ok(());
    }
    Err(format!("{command} accepts no extra arguments"))
}

fn parse_goal_watch_install_request(
    command: &str,
    args: &[String],
) -> Result<GoalWatchInstallOptions, String> {
    let mut write_files = false;
    let mut saw_dry_run = false;
    let mut saw_write_files = false;
    let mut apply_target = None;

    for arg in args {
        match arg.as_str() {
            "--dry-run" => {
                saw_dry_run = true;
                write_files = false;
            }
            "--write-files" => {
                saw_write_files = true;
                write_files = true;
            }
            "--apply-cron" => {
                if apply_target.is_some() {
                    return Err(format!("{command} accepts only one apply target"));
                }
                apply_target = Some(GoalWatchInstallApplyTarget::Cron);
            }
            "--apply-systemd-user" => {
                if apply_target.is_some() {
                    return Err(format!("{command} accepts only one apply target"));
                }
                apply_target = Some(GoalWatchInstallApplyTarget::SystemdUser);
            }
            other => return Err(format!("unknown {command} flag: {other}")),
        }
    }

    if saw_dry_run && (saw_write_files || apply_target.is_some()) {
        return Err(format!(
            "{command} accepts --dry-run only without write/apply flags"
        ));
    }

    Ok(GoalWatchInstallOptions {
        write_files,
        apply_target,
    })
}

fn parse_goal_watch_request(args: &[String]) -> Result<GoalWatchRequest, String> {
    let mut watch_args = Vec::new();
    let mut once = false;
    let mut interval_ms = DEFAULT_GOAL_WATCH_INTERVAL_MS;

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--once" => {
                once = true;
                index += 1;
            }
            "--interval-ms" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "goals watch --interval-ms requires a value".to_string())?;
                interval_ms = value.parse::<u64>().map_err(|_| {
                    "goals watch --interval-ms must be a positive integer".to_string()
                })?;
                if interval_ms == 0 {
                    return Err("goals watch --interval-ms must be a positive integer".to_string());
                }
                index += 2;
            }
            value => {
                watch_args.push(value.to_string());
                if let Some(next) = args.get(index + 1) {
                    if !next.starts_with("--") {
                        watch_args.push(next.clone());
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
        }
    }

    Ok(GoalWatchRequest {
        tick_request: parse_goal_tick_request(&watch_args)?,
        interval_ms,
        once,
    })
}

fn current_cli_executable_hint() -> PathBuf {
    crate::process_bootstrap::current_cli_executable()
}

fn goal_watch_lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join("goals").join("goals_watch.lock")
}

fn acquire_goal_watch_lock(data_dir: &Path) -> Result<GoalWatchLockGuard, GoalWatchLockError> {
    let path = goal_watch_lock_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(GoalWatchLockError::Io)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(GoalWatchLockError::Io)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock {
            return Err(GoalWatchLockError::Busy(path));
        }
        return Err(GoalWatchLockError::Io(err));
    }
    Ok(GoalWatchLockGuard { file })
}

fn sleep_goal_watch_interval(interval_ms: u64) {
    let mut remaining = interval_ms;
    while remaining > 0 && !self::cancel::sigint_requested() {
        let slice = remaining.min(GOAL_WATCH_SLEEP_SLICE_MS);
        std::thread::sleep(Duration::from_millis(slice));
        remaining = remaining.saturating_sub(slice);
    }
}

fn save_goal_watch_health(
    resolved: &ResolvedProject,
    status: &str,
    interval_ms: u64,
    ticks_completed: usize,
    stop_reason: &str,
    watch_lock_path: &Path,
    last_tick: Option<serde_json::Value>,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let health = crate::goals::GoalWatchHealthProjection::new(
        status,
        &resolved.project_id,
        interval_ms,
        ticks_completed,
        stop_reason,
        watch_lock_path.display().to_string(),
        last_tick,
    );
    crate::goals::save_goal_watch_health(&resolved.data_dir, &health)
        .map_err(|err| goals_failure("goals watch", err, resolved, output_json))
}

fn goal_watch_stop_reason_from_tick(tick: &GoalTickResult) -> Option<String> {
    if tick.loop_closure.should_continue {
        return None;
    }

    Some(tick.loop_closure.pause_reason.clone().unwrap_or_else(|| {
        match tick.loop_closure.status.as_str() {
            "completed" => "loop_completed".to_string(),
            "blocked" => "loop_blocked".to_string(),
            "needs_operator" => "loop_needs_operator".to_string(),
            "recovering" => "loop_recovering".to_string(),
            "waiting" => "loop_waiting".to_string(),
            other => format!("loop_{other}"),
        }
    }))
}

fn compact_single_line(value: &str, max_chars: usize) -> String {
    let compacted = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if max_chars == 0 || compacted.chars().count() <= max_chars {
        return compacted;
    }
    let mut truncated = compacted.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn relative_workspace_ref(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn short_sha256_hex(value: &str) -> String {
    sha256_hex(value.as_bytes())
        .chars()
        .take(16)
        .collect::<String>()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GoalTickRunOptions {
    publish_events: bool,
    publish_checkpoint: bool,
}

impl Default for GoalTickRunOptions {
    fn default() -> Self {
        Self {
            publish_events: true,
            publish_checkpoint: true,
        }
    }
}

fn run_goal_tick_once(
    resolved: &ResolvedProject,
    request: &routines::RoutineRunDueRequest,
    output_json: bool,
) -> Result<GoalTickResult, CommandFailureOutcome> {
    run_goal_tick_once_with_options(
        resolved,
        request,
        output_json,
        GoalTickRunOptions::default(),
    )
}

fn run_goal_tick_once_with_options(
    resolved: &ResolvedProject,
    request: &routines::RoutineRunDueRequest,
    output_json: bool,
    options: GoalTickRunOptions,
) -> Result<GoalTickResult, CommandFailureOutcome> {
    let routine_request = apply_goal_automation_recovery_policy(resolved, request.clone());
    let routine_run_due = routines::run_due(
        &resolved.data_dir,
        &resolved.workspace_root,
        routine_request,
    )
    .map_err(|err| match err {
        routines::RoutineError::Agent(agent_err) => {
            agent_start_failure("goals tick", agent_err, resolved, output_json)
        }
        other => routine_failure("goals tick", other, resolved, output_json),
    })?;
    let routine_triggered_count = routine_run_due.triggered_count;
    let routine_failed_count = routine_run_due.failed_count;
    if options.publish_events {
        append_canonical_event(
            resolved,
            "routine_run_due",
            "terminal",
            Some("succeeded"),
            "routine_run_due",
            "goal_tick",
            None,
            serde_json::to_value(&routine_run_due)
                .expect("routine run due result should serialize"),
        )
        .map_err(|err| internal_failure("goals tick", err))?;
    }
    let goal_advance = crate::goals::advance(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    )
    .map_err(|err| goals_failure("goals tick", err, resolved, output_json))?;
    let dispatch_count = goal_advance.dispatches.len();
    let accepted = goal_advance
        .acceptance
        .as_ref()
        .map(|acceptance| acceptance.status == "accepted")
        .unwrap_or(false);
    let next_recommended_action = goal_advance.next_recommended_action.clone();
    let recovery_governance = routine_run_due.recovery_governance.clone();
    let loop_closure = crate::goals::loop_closure_projection(
        &goal_advance.task_pool,
        Some(&recovery_governance),
        dispatch_count,
        accepted,
    );
    publish_goal_advance_result_with_options(resolved, None, &goal_advance, options)
        .map_err(|err| internal_failure("goals tick", err))?;
    if options.publish_checkpoint {
        publish_project_checkpoint(
            resolved,
            &[
                "routines",
                "mission_frame",
                "orchestration",
                "agents",
                "projectops",
            ],
        )
        .map_err(|err| internal_failure("goals tick", err))?;
    }
    Ok(GoalTickResult {
        schema_version: "goal_tick_result.v1".to_string(),
        status: if routine_failed_count == 0 {
            "advanced".to_string()
        } else {
            "advanced_with_routine_failures".to_string()
        },
        project_id: resolved.project_id.clone(),
        routine_run_due,
        goal_advance,
        triggered_count: routine_triggered_count,
        dispatch_count,
        accepted,
        next_recommended_action,
        recovery_governance,
        loop_closure,
    })
}

fn apply_goal_automation_recovery_policy(
    resolved: &ResolvedProject,
    mut request: routines::RoutineRunDueRequest,
) -> routines::RoutineRunDueRequest {
    let mode = crate::goals::status(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    )
    .ok()
    .and_then(|status| {
        status
            .projection
            .map(|projection| projection.automation_mode)
    })
    .unwrap_or_default();
    let (max_retry_attempts, retry_backoff_ms) = match mode {
        GoalAutomationMode::HumanInTheLoop => (1, 120_000),
        GoalAutomationMode::HighAutonomy => (3, 60_000),
        GoalAutomationMode::FullAuto => (5, 30_000),
    };
    request.max_retry_attempts = max_retry_attempts;
    request.retry_backoff_ms = retry_backoff_ms;
    request
}

fn goal_watch_failure(
    command: &str,
    err: GoalWatchLockError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        GoalWatchLockError::Busy(path) => CommandFailureOutcome::new(
            11,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "goal_watch_busy",
            format!("goal watch lane is busy: {}", path.display()),
            Some("Wait for the existing watch loop to stop, or run with --once.".to_string()),
            true,
            Some(json!({ "lock_path": path.display().to_string() })),
            output_json,
        ),
        GoalWatchLockError::Io(err) => {
            internal_failure(command, err.to_string()).with_output_json(output_json)
        }
    }
}

fn goal_watch_text_summary(envelope: &CommandSuccess) -> String {
    let mut lines = vec![format!("{}: ok", envelope.command)];
    if let Some(project_id) = &envelope.project_id {
        lines.push(format!("project_id: {project_id}"));
    }
    if let Some(data) = &envelope.data {
        if let Some(status) = data.get("status").and_then(Value::as_str) {
            lines.push(format!("status: {status}"));
        }
        if let Some(ticks_completed) = data.get("ticks_completed").and_then(Value::as_u64) {
            lines.push(format!("ticks_completed: {ticks_completed}"));
        }
        if let Some(interval_ms) = data.get("interval_ms").and_then(Value::as_u64) {
            lines.push(format!("interval_ms: {interval_ms}"));
        }
        if let Some(stop_reason) = data.get("stop_reason").and_then(Value::as_str) {
            lines.push(format!("stop_reason: {stop_reason}"));
        }
    }
    lines.join("\n")
}

fn command_cwd<'a>(cwd: &'a Path, parsed: &'a ParsedCliArgs) -> &'a Path {
    parsed.cwd.as_deref().unwrap_or(cwd)
}

fn trajectory_failure(
    command: &str,
    err: trajectory::TrajectoryError,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        trajectory::TrajectoryError::InvalidInput(message) => CommandFailureOutcome::usage(
            command.to_string(),
            "usage_invalid",
            message,
            Some("Use workspace-relative trajectory sources and valid M15 arguments.".to_string()),
        )
        .with_output_json(output_json),
        other => internal_failure(command, other.to_string()).with_output_json(output_json),
    }
}

fn is_non_loopback_bind_host(host: &str) -> bool {
    let host = host.trim();
    !(host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|addr| addr.is_loopback()))
}

fn is_valid_permission_mode(mode: &str) -> bool {
    matches!(mode, "read-only" | "workspace-write" | "danger-full-access")
}

fn pending_permission_count(resolved: &ResolvedProject) -> Result<usize, String> {
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let mut pending_request_ids = std::collections::BTreeSet::new();
    let mut resolved_request_ids = std::collections::BTreeSet::new();
    for event in read_events_from(&event_log_path).map_err(|err| err.to_string())? {
        if event.event_name != "permission" {
            continue;
        }
        if event.phase == "start" {
            pending_request_ids.insert(event.object_id);
        } else if event.phase == "terminal" {
            resolved_request_ids.insert(event.object_id);
        }
    }
    Ok(pending_request_ids
        .difference(&resolved_request_ids)
        .count())
}

fn resolve_permission_decision(
    resolved: &ResolvedProject,
    decision: &str,
    request_id: &str,
    output_json: bool,
) -> Result<Value, CommandFailureOutcome> {
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let events = read_events_from(&event_log_path)
        .map_err(|err| internal_failure("permissions history", err.to_string()))?;
    let already_resolved = events.iter().any(|event| {
        event.event_name == "permission"
            && event.phase == "terminal"
            && event.object_id == request_id
    });
    if already_resolved {
        return Err(CommandFailureOutcome::new(
            6,
            format!("permissions {decision}"),
            Some(resolved.project_id.clone()),
            None,
            "permission_request_resolved",
            format!("permission request is already resolved: {request_id}"),
            Some("Inspect `research-cli permissions history --json`.".to_string()),
            false,
            Some(json!({ "request_id": request_id })),
            output_json,
        ));
    }

    let request = events
        .iter()
        .find(|event| {
            event.event_name == "permission"
                && event.phase == "start"
                && event.object_id == request_id
        })
        .map(|event| event.payload.clone())
        .ok_or_else(|| {
            CommandFailureOutcome::new(
                6,
                format!("permissions {decision}"),
                Some(resolved.project_id.clone()),
                None,
                "permission_request_not_found",
                format!("unknown pending permission request: {request_id}"),
                Some("Run `research-cli permissions pending --json` first.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            )
        })?;

    if permission_request_is_expired(&request) {
        let payload = permission_decision_payload(request_id, "expired", &request, false);
        append_canonical_event(
            resolved,
            "permission",
            "terminal",
            Some("succeeded"),
            "permission",
            request_id,
            request.get("session_id").and_then(Value::as_str),
            payload.clone(),
        )
        .map_err(|err| internal_failure(&format!("permissions {decision}"), err))?;
        publish_project_checkpoint(resolved, &["permission"])
            .map_err(|err| internal_failure(&format!("permissions {decision}"), err))?;

        return Err(CommandFailureOutcome::new(
            6,
            format!("permissions {decision}"),
            Some(resolved.project_id.clone()),
            request
                .get("session_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            "permission_request_expired",
            format!("permission request expired: {request_id}"),
            Some("Re-run the action to create a fresh permission request.".to_string()),
            false,
            Some(payload),
            output_json,
        ));
    }

    let resolved_decision = if decision == "approve" {
        "approved"
    } else {
        "denied"
    };
    let payload = permission_decision_payload(request_id, resolved_decision, &request, true);
    let session_id = request.get("session_id").and_then(Value::as_str);

    append_canonical_event(
        resolved,
        "permission",
        "terminal",
        Some("succeeded"),
        "permission",
        request_id,
        session_id,
        payload.clone(),
    )
    .map_err(|err| internal_failure(&format!("permissions {decision}"), err))?;
    publish_project_checkpoint(resolved, &["permission"])
        .map_err(|err| internal_failure(&format!("permissions {decision}"), err))?;

    Ok(payload)
}

fn permission_request_expires_at_ms() -> Option<u128> {
    let ttl_ms = env::var("RESEARCH_CLI_PERMISSION_REQUEST_TTL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u128>().ok())
        .unwrap_or(10 * 60 * 1000);
    Some(timestamp_millis().saturating_add(ttl_ms))
}

fn permission_request_is_expired(request: &Value) -> bool {
    request
        .get("expires_at_ms")
        .and_then(Value::as_u64)
        .map(|expires_at| timestamp_millis() >= u128::from(expires_at))
        .unwrap_or(false)
}

fn permission_decision_payload(
    request_id: &str,
    decision: &str,
    request: &Value,
    approved: bool,
) -> Value {
    let tool_name = request.get("tool_name").and_then(Value::as_str);
    let permission_mode = request.get("permission_mode").and_then(Value::as_str);
    json!({
        "request_id": request_id,
        "decision": decision,
        "status": "resolved",
        "session_id": request.get("session_id").and_then(Value::as_str),
        "turn_id": request.get("turn_id").and_then(Value::as_str),
        "tool_name": tool_name,
        "permission_mode": permission_mode,
        "resolved_at": timestamp_string(),
        "trace": {
            "request_id": request_id,
            "tool_name": tool_name.unwrap_or("unknown"),
            "action": "permission_decision",
            "requested_path": request.get("target_path").and_then(Value::as_str),
            "required_mode": "workspace-write",
            "current_mode": permission_mode.unwrap_or("read-only"),
            "allowlist_match": false,
            "workspace_boundary_ok": true,
            "branch_boundary_ok": true,
            "destructive": false,
            "approved": approved,
            "reason": match decision {
                "approved" => "operator approved pending permission",
                "denied" => "operator denied pending permission",
                "expired" => "permission request expired before resolution",
                _ => "permission request resolved"
            }
        }
    })
}

fn permission_trace_payload(
    request_id: &str,
    tool_name: &str,
    action: &str,
    requested_path: Option<&str>,
    current_mode: &str,
    evaluation: &crate::permissions::PermissionCheckResult,
    approved: bool,
) -> Value {
    json!({
        "request_id": request_id,
        "tool_name": tool_name,
        "action": action,
        "requested_path": requested_path,
        "required_mode": if evaluation.requires_approval {
            "workspace-write"
        } else {
            evaluation.mode.as_str()
        },
        "current_mode": current_mode,
        "allowlist_match": false,
        "workspace_boundary_ok": evaluation.workspace_boundary_ok,
        "branch_boundary_ok": true,
        "destructive": tool_name == "delete_file" || tool_name == "shell",
        "approved": approved,
        "reason": evaluation.reason,
        "reason_code": evaluation.reason_code
    })
}

fn prompt_requests_mutation(prompt_text: &str) -> bool {
    let lowered = prompt_text.to_ascii_lowercase();
    [
        "write",
        "edit",
        "modify",
        "change",
        "delete",
        "remove",
        "rename",
        "create file",
        "patch",
        "refactor",
        "implement",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn state_home() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("RESEARCH_CLI_STATE_HOME") {
        return Ok(PathBuf::from(path));
    }

    if let Ok(path) = env::var("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("research-cli"));
    }

    let home = env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("research-cli"))
}

fn internal_failure(command: &str, message: String) -> CommandFailureOutcome {
    CommandFailureOutcome::new(
        10,
        command.to_string(),
        None,
        None,
        "invariant_violation",
        message,
        Some("Inspect the local runtime state and fix the invariant violation.".to_string()),
        false,
        None,
        false,
    )
}

fn skill_output_failure(
    command: &str,
    err: skills::SkillOutputError,
    project_id: Option<String>,
    output_json: bool,
) -> CommandFailureOutcome {
    let (exit_code, code, hint) = match &err {
        skills::SkillOutputError::UnknownSkill(_) => (
            6,
            "skill_unknown",
            "Run `research-cli skills list --json` to inspect known skills.",
        ),
        skills::SkillOutputError::UnknownEnvelope(_) => (
            6,
            "skill_output_unknown",
            "Run `research-cli skills outputs list --json` to inspect submitted outputs.",
        ),
        skills::SkillOutputError::ScopeViolation(_) => (
            7,
            "scope_violation",
            "Use workspace-relative artifact and DocFrame paths inside the current project.",
        ),
        skills::SkillOutputError::InvalidInput(_)
        | skills::SkillOutputError::MissingDocFrame(_) => (
            2,
            "usage_invalid",
            "Submit an existing workspace artifact with a valid DocFrame before publishing.",
        ),
        skills::SkillOutputError::SkillContractViolation(_) => (
            7,
            "skill_contract_violation",
            "Inspect the skill research contract and resubmit an output that matches it.",
        ),
        skills::SkillOutputError::RunnerFailed(_) => (
            13,
            "skill_runner_failed",
            "Inspect the runner stdout/stderr and rerun after repairing the skill command.",
        ),
        skills::SkillOutputError::PublicationGateRequired(_) => (
            12,
            "publication_gate_required",
            "Retry with `skills publish --execute <envelope-id> --approve-human-gate --json` after reviewing the candidate.",
        ),
        skills::SkillOutputError::PublicationPolicyBlocked(_) => (
            12,
            "publication_policy_blocked",
            "Use a review/candidate flow or resubmit with a policy that permits public publication.",
        ),
        skills::SkillOutputError::Io(_) | skills::SkillOutputError::Serde(_) => (
            10,
            "invariant_violation",
            "Inspect .pmcli skill output state and repair invalid JSON or filesystem issues.",
        ),
    };
    CommandFailureOutcome::new(
        exit_code,
        command.to_string(),
        project_id,
        None,
        code,
        err.to_string(),
        Some(hint.to_string()),
        false,
        None,
        output_json,
    )
}

fn evolution_failure(
    command: &str,
    err: evolution::EvolutionError,
    project_id: Option<String>,
    output_json: bool,
) -> CommandFailureOutcome {
    let (exit_code, code, hint) = match &err {
        evolution::EvolutionError::PublicationGateRequired(_) => (
            12,
            "publication_gate_required",
            "Retry with `skills evolve install <candidate-id> --approve-human-gate --json` after verification and review.",
        ),
        evolution::EvolutionError::InvalidInput(_) => (
            2,
            "usage_invalid",
            "Inspect the evolved skill candidate and rerun with a verified candidate id.",
        ),
        evolution::EvolutionError::Io(_) | evolution::EvolutionError::Json(_) => (
            10,
            "invariant_violation",
            "Inspect .pmcli skill evolution state and repair invalid JSON or filesystem issues.",
        ),
    };
    CommandFailureOutcome::new(
        exit_code,
        command.to_string(),
        project_id,
        None,
        code,
        err.to_string(),
        Some(hint.to_string()),
        false,
        None,
        output_json,
    )
}

fn feature_not_graduated_failure(
    command: &str,
    feature_id: &str,
    milestone: &str,
    next_steps: Vec<String>,
) -> CommandFailureOutcome {
    CommandFailureOutcome::new(
        8,
        command.to_string(),
        None,
        None,
        "not_graduated",
        format!("{command} is not graduated yet"),
        Some("Use the graduated baseline lanes called out in next_steps.".to_string()),
        false,
        Some(json!({
            "feature_id": feature_id,
            "requested_command": command,
            "gate_state": "not_graduated",
            "milestone": milestone,
            "next_steps": next_steps
        })),
        false,
    )
}

fn degraded_but_loadable_failure(
    command: &str,
    feature_id: &str,
    milestone: &str,
    next_steps: Vec<String>,
) -> CommandFailureOutcome {
    CommandFailureOutcome::new(
        11,
        command.to_string(),
        None,
        None,
        "degraded_but_loadable",
        format!("{command} is degraded but the runtime remains loadable"),
        Some("Use a ready baseline lane or inspect the degraded feature data.".to_string()),
        true,
        Some(json!({
            "feature_id": feature_id,
            "requested_command": command,
            "gate_state": "degraded",
            "milestone": milestone,
            "exit_lane": 11,
            "next_steps": next_steps
        })),
        false,
    )
}

fn policy_refusal_failure(
    command: &str,
    exit_code: i32,
    code: &str,
    policy_kind: &str,
    policy_source: &str,
    refusal_reason: &str,
    blocking_object_id: Option<String>,
    message: Option<String>,
    next_steps: Vec<String>,
) -> CommandFailureOutcome {
    CommandFailureOutcome::new(
        exit_code,
        command.to_string(),
        None,
        None,
        code,
        message.unwrap_or_else(|| format!("{command} refused by {policy_kind} policy")),
        next_steps.first().cloned(),
        false,
        Some(json!({
            "policy_kind": policy_kind,
            "policy_source": policy_source,
            "refusal_reason": refusal_reason,
            "blocking_object_id": blocking_object_id,
            "retryable": false,
            "next_steps": next_steps
        })),
        false,
    )
}

fn project_resolution_failure(
    command: &str,
    err: ResolveCurrentProjectError,
) -> CommandFailureOutcome {
    let (code, message, hint, warnings) = match &err {
        ResolveCurrentProjectError::UnknownProject(project_id) => (
            "project_unresolved",
            format!("unknown project in registry: {project_id}"),
            Some(
                "Run `research-cli projects list --json` to inspect registered projects."
                    .to_string(),
            ),
            vec!["unknown_project".to_string()],
        ),
        ResolveCurrentProjectError::UnknownWorkspace(path) => (
            "project_unresolved",
            format!("workspace is not registered: {}", path.display()),
            Some("Run `research-cli projects init --json` in that workspace first.".to_string()),
            vec!["unknown_workspace".to_string()],
        ),
        ResolveCurrentProjectError::StaleCurrentProjectPointer(path) => (
            "project_unresolved",
            format!(
                "stale current project pointer: {} no longer exists",
                path.display()
            ),
            Some(
                "Re-enter a registered workspace or reset the current project pointer.".to_string(),
            ),
            vec!["stale_current_project_pointer".to_string()],
        ),
        ResolveCurrentProjectError::NoProject => (
            "project_unresolved",
            "no project could be resolved from scope or pointer".to_string(),
            Some(
                "Pass --project, --cwd, or run the command inside a registered workspace."
                    .to_string(),
            ),
            vec!["no_project".to_string()],
        ),
        ResolveCurrentProjectError::Workspace(workspace_err) => (
            "project_unresolved",
            workspace_err.to_string(),
            Some("Run the command from a workspace root or pass --cwd explicitly.".to_string()),
            vec!["workspace_resolution_failed".to_string()],
        ),
        ResolveCurrentProjectError::Registry(registry_err) => (
            "project_unresolved",
            registry_err.to_string(),
            Some("Inspect the registry files under your state home.".to_string()),
            vec!["registry_resolution_failed".to_string()],
        ),
    };

    CommandFailureOutcome::new(
        6,
        command.to_string(),
        None,
        None,
        code,
        message,
        hint,
        false,
        Some(json!({
            "resolution_status": "unresolved",
            "warnings": warnings
        })),
        false,
    )
}

fn project_resolution_failure_with_trace(
    command: &str,
    err: ResolveCurrentProjectWithTraceError,
) -> CommandFailureOutcome {
    let (code, message, hint) = match &err.error {
        ResolveCurrentProjectError::UnknownProject(project_id) => (
            "project_unresolved",
            format!("unknown project in registry: {project_id}"),
            Some(
                "Run `research-cli projects list --json` to inspect registered projects."
                    .to_string(),
            ),
        ),
        ResolveCurrentProjectError::UnknownWorkspace(path) => (
            "project_unresolved",
            format!("workspace is not registered: {}", path.display()),
            Some("Run `research-cli projects init --json` in that workspace first.".to_string()),
        ),
        ResolveCurrentProjectError::StaleCurrentProjectPointer(path) => (
            "project_unresolved",
            format!(
                "stale current project pointer: {} no longer exists",
                path.display()
            ),
            Some(
                "Re-enter a registered workspace or reset the current project pointer.".to_string(),
            ),
        ),
        ResolveCurrentProjectError::NoProject => (
            "project_unresolved",
            "no project could be resolved from scope or pointer".to_string(),
            Some(
                "Pass --project, --cwd, or run the command inside a registered workspace."
                    .to_string(),
            ),
        ),
        ResolveCurrentProjectError::Workspace(workspace_err) => (
            "project_unresolved",
            workspace_err.to_string(),
            Some("Run the command from a workspace root or pass --cwd explicitly.".to_string()),
        ),
        ResolveCurrentProjectError::Registry(registry_err) => (
            "project_unresolved",
            registry_err.to_string(),
            Some("Inspect the registry files under your state home.".to_string()),
        ),
    };

    CommandFailureOutcome::new(
        6,
        command.to_string(),
        None,
        None,
        code,
        message,
        hint,
        false,
        Some(serde_json::to_value(err.trace).expect("project resolution trace should serialize")),
        false,
    )
}

fn session_store_failure(
    command: &str,
    err: SessionStoreError,
    resolved: &ResolvedProject,
    selector: Option<String>,
) -> CommandFailureOutcome {
    match err {
        SessionStoreError::NoSessions => CommandFailureOutcome::new(
            6,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "session_not_found",
            "no sessions exist in this project".to_string(),
            Some("Create a new session or choose a different project.".to_string()),
            false,
            selector.map(|value| json!({ "selector": value })),
            false,
        ),
        SessionStoreError::UnknownSession(session_id) => CommandFailureOutcome::new(
            6,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "session_not_found",
            format!("unknown session: {session_id}"),
            Some(
                "Run `research-cli sessions list --json` to inspect available sessions."
                    .to_string(),
            ),
            false,
            Some(json!({ "selector": session_id })),
            false,
        ),
        SessionStoreError::AmbiguousSessionSelector(prefix) => CommandFailureOutcome::new(
            6,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "session_ambiguous",
            format!("ambiguous session selector: {prefix}"),
            Some("Use a full session id instead of a shared prefix.".to_string()),
            false,
            Some(json!({ "selector": prefix })),
            false,
        ),
        SessionStoreError::Io(io_err) => CommandFailureOutcome::new(
            10,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "session_store_io_failed",
            io_err.to_string(),
            Some("Inspect the session store on disk and retry.".to_string()),
            true,
            None,
            false,
        ),
        SessionStoreError::Serde(serde_err) => CommandFailureOutcome::new(
            10,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "session_store_corrupt",
            serde_err.to_string(),
            Some("Inspect the stored session JSON and repair the corrupt artifact.".to_string()),
            false,
            None,
            false,
        ),
    }
}

fn config_error_failure(
    command: &str,
    err: ConfigError,
    key: Option<String>,
) -> CommandFailureOutcome {
    match err {
        ConfigError::KeyNotFound(missing_key) => CommandFailureOutcome::new(
            6,
            command.to_string(),
            None,
            None,
            "config_key_not_found",
            format!("config key not found: {missing_key}"),
            Some(
                "Run `research-cli config effective --json` to inspect available keys.".to_string(),
            ),
            false,
            Some(json!({
                "key": key.unwrap_or(missing_key)
            })),
            false,
        ),
        other => CommandFailureOutcome::new(
            10,
            command.to_string(),
            None,
            None,
            "config_operation_failed",
            other.to_string(),
            Some("Inspect the config files and retry the operation.".to_string()),
            false,
            key.map(|value| json!({ "key": value })),
            false,
        ),
    }
}

fn docs_failure(
    command: &str,
    err: docs::DocFrameError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    let (exit_code, code, hint) = match err {
        docs::DocFrameError::ScopeViolation(_) | docs::DocFrameError::NotFound(_) => (
            6,
            "doc_path_invalid",
            "Pass a workspace-relative markdown path inside the project.".to_string(),
        ),
        docs::DocFrameError::InvalidFrame(_) => (
            6,
            "doc_frame_invalid",
            "Inspect the DocFrame block and repair required fields.".to_string(),
        ),
        docs::DocFrameError::WriteNotGraduated => (
            8,
            "not_graduated",
            "Use `docs frame refresh <path> --dry-run --json` until write mode graduates."
                .to_string(),
        ),
        docs::DocFrameError::Io(_) | docs::DocFrameError::Serde(_) => (
            10,
            "doc_frame_operation_failed",
            "Inspect the document and .pmcli/docs/index.json state, then retry.".to_string(),
        ),
    };

    CommandFailureOutcome::new(
        exit_code,
        command.to_string(),
        Some(resolved.project_id.clone()),
        None,
        code,
        err.to_string(),
        Some(hint),
        false,
        None,
        output_json,
    )
}

fn goals_failure(
    command: &str,
    err: MissionFrameError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    let (exit_code, code, hint, retryable) = match err {
        MissionFrameError::MissingRequired(_) | MissionFrameError::Invalid(_) => (
            6,
            "mission_frame_invalid",
            "Inspect `research-cli goals status --json` and set all required goal fields.".to_string(),
            false,
        ),
        MissionFrameError::AgentStart(agents::AgentError::ProviderExecution(_))
        | MissionFrameError::ProviderExecution(_) => (
            8,
            "provider_execution_failed",
            "Inspect provider configuration and retry; autonomous research jobs convert retryable provider errors into provider_backoff."
                .to_string(),
            true,
        ),
        MissionFrameError::AgentStart(_) => (
            9,
            "agent_start_failed",
            "Inspect agent task packet, workspace state, and `.pmcli/agents` records before retrying."
                .to_string(),
            false,
        ),
        MissionFrameError::Io(_) | MissionFrameError::Serde(_) => (
            10,
            "mission_frame_operation_failed",
            "Inspect `.pmcli/project_goals/mission_frame.json`, repair it, then retry.".to_string(),
            false,
        ),
    };
    CommandFailureOutcome::new(
        exit_code,
        command.to_string(),
        Some(resolved.project_id.clone()),
        None,
        code,
        err.to_string(),
        Some(hint),
        retryable,
        None,
        output_json,
    )
}

fn routine_failure(
    command: &str,
    err: routines::RoutineError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        routines::RoutineError::UnknownRoutine(routine_id) => CommandFailureOutcome::new(
            6,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "routine_unknown",
            format!("unknown routine: {routine_id}"),
            Some("Run `research-cli routines list --json` to inspect routine ids.".to_string()),
            false,
            Some(json!({ "routine_id": routine_id })),
            output_json,
        ),
        routines::RoutineError::UnknownTrigger(trigger_id) => CommandFailureOutcome::new(
            6,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "routine_trigger_unknown",
            format!("unknown routine trigger: {trigger_id}"),
            Some(
                "Run `research-cli routines inspect <routine-id> --json` to inspect trigger ids."
                    .to_string(),
            ),
            false,
            Some(json!({ "trigger_id": trigger_id })),
            output_json,
        ),
        routines::RoutineError::InvalidInput(message) => CommandFailureOutcome::new(
            2,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "usage_invalid",
            message,
            Some("Use manual, schedule, webhook, or api as the routine trigger kind.".to_string()),
            false,
            None,
            output_json,
        ),
        routines::RoutineError::ProjectOps(err) => CommandFailureOutcome::new(
            10,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "routine_projectops_failed",
            err.to_string(),
            Some("Inspect `research-cli projectops status --json` and retry.".to_string()),
            false,
            None,
            output_json,
        ),
        routines::RoutineError::Agent(agent_err) => {
            agent_start_failure(command, agent_err, resolved, output_json)
        }
        routines::RoutineError::Io(err) => {
            internal_failure(command, err.to_string()).with_output_json(output_json)
        }
        routines::RoutineError::Json(err) => {
            internal_failure(command, err.to_string()).with_output_json(output_json)
        }
    }
}

fn branch_failure(
    command_name: &str,
    err: branches::BranchError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        branches::BranchError::InvalidInput(message) => CommandFailureOutcome::new(
            2,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "usage_invalid",
            message,
            Some("Inspect `research-cli branches search --json` command usage.".to_string()),
            false,
            None,
            output_json,
        ),
        branches::BranchError::UnknownBranch(branch_id) => CommandFailureOutcome::new(
            6,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "branch_unknown",
            format!("unknown branch run: {branch_id}"),
            Some("Run `research-cli branches list --json` to inspect branch ids.".to_string()),
            false,
            Some(json!({ "branch_id": branch_id })),
            output_json,
        ),
        branches::BranchError::PromotionBlocked {
            branch_id,
            missing_gates,
        } => CommandFailureOutcome::new(
            9,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "branch_promotion_blocked",
            format!("branch promotion blocked for {branch_id}"),
            Some(
                "Run `branches evaluate`, `branches debate`, and optionally `branches verify` before promotion."
                    .to_string(),
            ),
            false,
            Some(json!({ "branch_id": branch_id, "missing_gates": missing_gates })),
            output_json,
        ),
        branches::BranchError::WinnerExists {
            batch_id,
            winner_branch_id,
        } => CommandFailureOutcome::new(
            9,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "branch_winner_exists",
            format!("search batch {batch_id} already has promoted winner {winner_branch_id}"),
            Some("Archive losing branches or open a new search batch.".to_string()),
            false,
            Some(json!({ "batch_id": batch_id, "winner_branch_id": winner_branch_id })),
            output_json,
        ),
        branches::BranchError::GitCommand { action, stderr } => CommandFailureOutcome::new(
            10,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "branch_git_command_failed",
            format!("git command failed during {action}"),
            Some(
                "Inspect git worktree state and retry `research-cli branches search --json`."
                    .to_string(),
            ),
            false,
            Some(json!({ "action": action, "stderr": stderr })),
            output_json,
        ),
        branches::BranchError::Io(err) => {
            internal_failure(command_name, err.to_string()).with_output_json(output_json)
        }
        branches::BranchError::Serde(err) => {
            internal_failure(command_name, err.to_string()).with_output_json(output_json)
        }
    }
}

fn research_failure(
    command_name: &str,
    err: research::ResearchError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        research::ResearchError::InvalidInput(message) => CommandFailureOutcome::new(
            2,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "usage_invalid",
            message,
            Some("Inspect `research-cli research status --json` and retry.".to_string()),
            false,
            None,
            output_json,
        ),
        research::ResearchError::UnknownThread(thread_id) => CommandFailureOutcome::new(
            6,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "research_thread_unknown",
            format!("unknown research thread: {thread_id}"),
            Some("Run `research-cli research threads list --json` to inspect threads.".to_string()),
            false,
            Some(json!({ "thread_id": thread_id })),
            output_json,
        ),
        research::ResearchError::UnknownStage(execution_id) => CommandFailureOutcome::new(
            6,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "research_stage_unknown",
            format!("unknown research stage execution: {execution_id}"),
            Some("Run `research-cli research status --json` to inspect active stage.".to_string()),
            false,
            Some(json!({ "execution_id": execution_id })),
            output_json,
        ),
        research::ResearchError::UnknownSpan(span_id) => CommandFailureOutcome::new(
            6,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "research_span_unknown",
            format!("unknown research deliberation span: {span_id}"),
            Some(
                "Run `research-cli research status --json` to inspect the active span.".to_string(),
            ),
            false,
            Some(json!({ "span_id": span_id })),
            output_json,
        ),
        research::ResearchError::Branch(message) => CommandFailureOutcome::new(
            8,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "research_branch_binding_failed",
            message,
            Some("Inspect `research-cli branches list --json` and git worktree state.".to_string()),
            false,
            None,
            output_json,
        ),
        research::ResearchError::Io(err) => {
            internal_failure(command_name, err.to_string()).with_output_json(output_json)
        }
        research::ResearchError::Serde(err) => {
            internal_failure(command_name, err.to_string()).with_output_json(output_json)
        }
    }
}

fn agent_start_failure(
    command_name: &str,
    err: agents::AgentError,
    resolved: &ResolvedProject,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        agents::AgentError::InvalidTaskPacket { reason } => CommandFailureOutcome::new(
            9,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "agent_invalid_task_packet",
            format!("invalid task packet: {reason}"),
            Some(
                "Fix the task packet schema and required M6 fields before starting the agent."
                    .to_string(),
            ),
            false,
            Some(json!({
                "failure_code": "invalid_task_packet",
                "reason": reason
            })),
            output_json,
        ),
        agents::AgentError::StaleScope { expected, actual } => CommandFailureOutcome::new(
            9,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "agent_stale_scope",
            "task packet scope does not match the current project workspace".to_string(),
            Some(
                "Regenerate the task packet for the current project before starting the agent."
                    .to_string(),
            ),
            false,
            Some(json!({
                "failure_code": "stale_scope",
                "expected_workspace_root": expected,
                "actual_workspace_root": actual
            })),
            output_json,
        ),
        agents::AgentError::MissingWriteAuthority { required, actual } => {
            CommandFailureOutcome::new(
                9,
                command_name.to_string(),
                Some(resolved.project_id.clone()),
                None,
                "agent_missing_write_authority",
                "task packet does not grant the write authority required by the local runner"
                    .to_string(),
                Some("Use write_authority=workspace_write for local mutating agent runs."
                    .to_string()),
                false,
                Some(json!({
                    "failure_code": "missing_write_authority",
                    "required_write_authority": required,
                    "actual_write_authority": actual
                })),
                output_json,
            )
        }
        agents::AgentError::DirtySourceWorktree { dirty_paths } => CommandFailureOutcome::new(
            9,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "agent_dirty_worktree_forbidden",
            "local agent start refused because the source worktree has uncommitted changes"
                .to_string(),
            Some("Commit, stash, or remove non-.pmcli worktree changes before starting an isolated local agent.".to_string()),
            false,
            Some(json!({
                "failure_code": "dirty_source_worktree_forbidden",
                "runner_kind": "local",
                "dirty_paths": dirty_paths
            })),
            output_json,
        ),
        agents::AgentError::ProviderExecution(message) => CommandFailureOutcome::new(
            8,
            command_name.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "provider_execution_failed",
            format!("provider runner failed: {message}"),
            Some(
                "Inspect provider worker configuration and retry; autonomous research jobs can apply provider recovery policy."
                    .to_string(),
            ),
            true,
            Some(json!({
                "failure_code": "provider_execution_failed",
                "runner_kind": "provider"
            })),
            output_json,
        ),
        other => internal_failure(command_name, other.to_string()).with_output_json(output_json),
    }
}

fn emit_config_event(
    resolved: &ResolvedProject,
    key: &str,
    scope: &str,
    result: &config::ConfigValueResult,
) -> Result<(), String> {
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let _guard = lock_event_log(&event_log_path).map_err(|err| err.to_string())?;
    let next_seq = read_events_from(&event_log_path)
        .map_err(|err| err.to_string())?
        .last()
        .map(|event| event.seq + 1)
        .unwrap_or(1);

    append_event_unlocked(
        &event_log_path,
        &KernelEventEnvelope {
            event_id: format!("evt_config_{next_seq}"),
            seq: next_seq,
            event_name: "config".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "config".to_string(),
            object_id: key.to_string(),
            session_id: None,
            project_id: Some(resolved.project_id.clone()),
            timestamp: timestamp_string(),
            payload: json!({
                "key": key,
                "scope": scope,
                "value": result.value,
                "config_path": result.config_path,
                "project_id": result.project_id,
                "workspace_root": result.workspace_root,
                "resolution_source": result.resolution_source
            }),
        },
    )
    .map_err(|err| err.to_string())
}

fn emit_provider_auth_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    result: &crate::providers::ProviderAuthStatusResult,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            "provider_auth",
            "terminal",
            Some("succeeded"),
            "provider",
            "providers auth-status",
            None,
            json!({
                "total_count": result.total_count,
                "providers": result.providers
            }),
        )?;
    }
    Ok(())
}

fn emit_provider_test_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    result: &ProviderTestResult,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            "provider_test",
            "terminal",
            Some("succeeded"),
            "provider",
            &result.provider_id,
            None,
            serde_json::to_value(result).expect("provider test result should serialize"),
        )?;
    }
    Ok(())
}

fn emit_provider_catalog_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    result: &ProviderCatalogRefreshResult,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            "provider_catalog",
            "terminal",
            Some("succeeded"),
            "provider_catalog",
            &result.applied_source,
            None,
            serde_json::to_value(result).expect("provider catalog result should serialize"),
        )?;
    }
    Ok(())
}

fn emit_setup_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            "setup",
            "terminal",
            Some("succeeded"),
            "setup",
            object_id,
            None,
            payload,
        )?;
    }
    Ok(())
}

fn emit_mcp_registry_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    payload: Value,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            "mcp_registry",
            "terminal",
            Some("succeeded"),
            "mcp",
            "mcp refresh",
            None,
            payload,
        )?;
    }
    Ok(())
}

fn emit_cleanup_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    event_name: &str,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            event_name,
            "terminal",
            Some("succeeded"),
            "cleanup",
            object_id,
            None,
            payload,
        )?;
    }
    Ok(())
}

fn emit_review_event_if_possible(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    payload: Value,
    review_id: &str,
) -> Result<(), String> {
    if let Some(resolved) = resolve_optional_event_project(registry, cwd, parsed) {
        append_canonical_event(
            &resolved,
            "review",
            "terminal",
            Some("succeeded"),
            "review",
            review_id,
            None,
            payload,
        )?;
    }
    Ok(())
}

fn emit_inspection_event(
    resolved: &ResolvedProject,
    scope: &str,
    object_id: &str,
    session_id: Option<String>,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "inspection",
        "terminal",
        Some("succeeded"),
        scope,
        object_id,
        session_id.as_deref(),
        payload,
    )
}

fn emit_session_compaction_event(
    resolved: &ResolvedProject,
    session_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "session_compaction",
        "terminal",
        Some("succeeded"),
        "session",
        session_id,
        Some(session_id),
        payload,
    )
}

fn emit_goal_alignment_trace_event(
    resolved: &ResolvedProject,
    session_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "goal_alignment_trace",
        "terminal",
        Some("succeeded"),
        "session",
        session_id,
        Some(session_id),
        payload,
    )
}

fn emit_goal_advance_event(
    resolved: &ResolvedProject,
    session_id: &str,
    terminal_outcome: Option<&str>,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "goal_advance",
        "terminal",
        terminal_outcome,
        "goal_run",
        object_id,
        Some(session_id),
        payload,
    )
}

fn emit_goal_task_dispatch_event(
    resolved: &ResolvedProject,
    session_id: Option<&str>,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "goal_task_dispatch",
        "terminal",
        Some("succeeded"),
        "agent_task_packet",
        object_id,
        session_id,
        payload,
    )
}

fn goal_task_dispatch_event_payload(
    result: &crate::goals::GoalAdvanceResult,
    dispatch: &crate::goals::GoalTaskDispatchResult,
) -> Value {
    json!({
        "schema_version": "goal_task_dispatch_event.v1",
        "project_id": result.project_id,
        "run_id": result.run.run_id,
        "dispatch": dispatch
    })
}

fn emit_goal_task_acceptance_event(
    resolved: &ResolvedProject,
    session_id: Option<&str>,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "goal_task_acceptance",
        "terminal",
        Some("succeeded"),
        "agent_output_manifest",
        object_id,
        session_id,
        payload,
    )
}

fn emit_goal_research_stage_decision_event(
    resolved: &ResolvedProject,
    session_id: Option<&str>,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "goal_research_stage_decision",
        "terminal",
        Some("succeeded"),
        "research_decision",
        object_id,
        session_id,
        payload,
    )
}

fn goal_task_acceptance_event_payload(
    result: &crate::goals::GoalAdvanceResult,
    acceptance: &crate::goals::GoalTaskAcceptanceResult,
) -> Value {
    json!({
        "schema_version": "goal_task_acceptance_event.v1",
        "project_id": result.project_id,
        "run_id": result.run.run_id,
        "acceptance": acceptance
    })
}

fn goal_research_stage_decision_event_payload(
    result: &crate::goals::GoalAdvanceResult,
    stage_decision: &crate::goals::GoalResearchStageDecisionResult,
) -> Value {
    json!({
        "schema_version": "goal_research_stage_decision_event.v1",
        "project_id": result.project_id,
        "run_id": result.run.run_id,
        "stage_decision": stage_decision
    })
}

fn publish_goal_advance_result(
    resolved: &ResolvedProject,
    session_id: Option<&str>,
    result: &crate::goals::GoalAdvanceResult,
) -> Result<(), String> {
    publish_goal_advance_result_with_options(
        resolved,
        session_id,
        result,
        GoalTickRunOptions::default(),
    )
}

fn publish_goal_advance_result_with_options(
    resolved: &ResolvedProject,
    session_id: Option<&str>,
    result: &crate::goals::GoalAdvanceResult,
    options: GoalTickRunOptions,
) -> Result<(), String> {
    if options.publish_events {
        append_canonical_event(
            resolved,
            "goal_advance",
            "terminal",
            Some("succeeded"),
            "goal_run",
            &result.run.run_id,
            session_id,
            serde_json::to_value(result).expect("goal advance result should serialize"),
        )?;
        for dispatch in &result.dispatches {
            emit_goal_task_dispatch_event(
                resolved,
                session_id,
                &dispatch.agent_id,
                goal_task_dispatch_event_payload(result, dispatch),
            )?;
        }
        if let Some(acceptance) = result.acceptance.as_ref() {
            emit_goal_task_acceptance_event(
                resolved,
                session_id,
                &acceptance.agent_id,
                goal_task_acceptance_event_payload(result, acceptance),
            )?;
        }
        if let Some(stage_decision) = result.stage_decision.as_ref() {
            emit_goal_research_stage_decision_event(
                resolved,
                session_id,
                &stage_decision.decision_id,
                goal_research_stage_decision_event_payload(result, stage_decision),
            )?;
        }
    }
    if options.publish_checkpoint {
        publish_project_checkpoint(
            resolved,
            &["mission_frame", "orchestration", "agents", "research"],
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn auto_advance_goal_run_after_turn(
    resolved: &ResolvedProject,
    session_id: &str,
    turn_id: &str,
    mission_frame: Option<&crate::goals::MissionFrameProjection>,
) -> Result<(), String> {
    let Some(frame) = mission_frame else {
        return Ok(());
    };
    if matches!(
        frame.automation_mode,
        crate::goals::GoalAutomationMode::HumanInTheLoop
    ) {
        return Ok(());
    }

    match crate::goals::advance(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    ) {
        Ok(result) => {
            publish_goal_advance_result(resolved, Some(session_id), &result)?;
        }
        Err(err) => {
            emit_goal_advance_event(
                resolved,
                session_id,
                Some("failed"),
                turn_id,
                json!({
                    "turn_id": turn_id,
                    "automation_mode": frame.automation_mode.as_str(),
                    "error": err.to_string()
                }),
            )?;
        }
    }

    Ok(())
}

fn goal_alignment_trace_payload(
    resolved: &ResolvedProject,
    session_id: &str,
    command: &str,
    compact_result: &Value,
) -> Value {
    let mission_frame_ref = compact_result
        .get("mission_frame_ref")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mission_frame = compact_result.get("mission_frame");
    let summary_text = compact_result
        .get("summary_ref")
        .and_then(Value::as_str)
        .and_then(|summary_ref| {
            read_compact_summary_text(&resolved.data_dir, session_id, summary_ref).ok()
        })
        .unwrap_or_default();
    goal_alignment_trace_payload_from_text(
        resolved,
        session_id,
        command,
        mission_frame_ref,
        mission_frame,
        &recent_highlights_text(&summary_text),
        "summary_recent_highlights",
    )
}

fn goal_alignment_trace_payload_from_text(
    resolved: &ResolvedProject,
    session_id: &str,
    command: &str,
    mission_frame_ref: &str,
    mission_frame: Option<&Value>,
    alignment_text: &str,
    evidence_label: &str,
) -> Value {
    let project_goal = mission_frame
        .and_then(|frame| frame.get("project_max_goal"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let milestone_goal = mission_frame
        .and_then(|frame| frame.get("milestone_goal"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let implementation_goal = mission_frame
        .and_then(|frame| frame.get("current_implementation_goal"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let project_score = goal_support_score(project_goal, alignment_text);
    let milestone_score = goal_support_score(milestone_goal, alignment_text);
    let implementation_score = goal_support_score(implementation_goal, alignment_text);
    let project_goal_supported = project_score >= 0.25;
    let milestone_goal_supported = milestone_score >= 0.25;
    let implementation_goal_supported = implementation_score >= 0.25;
    let mission_frame_present = !mission_frame_ref.is_empty()
        && mission_frame.map(|frame| !frame.is_null()).unwrap_or(false);
    let evidence_description = match evidence_label {
        "summary_recent_highlights" => "compact summary recent highlights",
        "prompt_text" => "prompt text",
        "review_packet" => "review packet",
        _ => "command context",
    };
    let drift_context = match evidence_label {
        "summary_recent_highlights" => "after compaction",
        "prompt_text" => "for this prompt turn",
        "review_packet" => "for this review packet",
        _ => "for this command",
    };
    let mut drift_reasons = Vec::new();
    let mut drift_risks = Vec::new();

    if !mission_frame_present {
        drift_reasons.push(format!(
            "mission frame projection is missing from {evidence_description}"
        ));
        drift_risks.push(format!(
            "command cannot enforce project > milestone > implementation priority {drift_context} without a MissionFrame"
        ));
    } else {
        if !project_goal_supported {
            drift_reasons.push(format!(
                "{evidence_description} does not support project_max_goal"
            ));
            drift_risks.push(format!(
                "project-level objective may be lost {drift_context}"
            ));
        }
        if !milestone_goal_supported {
            drift_reasons.push(format!(
                "{evidence_description} does not support milestone_goal"
            ));
            drift_risks.push(format!(
                "milestone-level objective may be underspecified {drift_context}"
            ));
        }
        if !implementation_goal_supported {
            drift_reasons.push(format!(
                "{evidence_description} does not support current_implementation_goal"
            ));
            drift_risks.push(format!(
                "next step may continue from unrelated context instead of the active implementation goal {drift_context}"
            ));
        }
    }

    let alignment_status = if !mission_frame_present {
        "mission_frame_missing"
    } else if project_goal_supported && milestone_goal_supported && implementation_goal_supported {
        "aligned"
    } else {
        "drift_risk"
    };
    let recommended_next_action = if alignment_status == "aligned" {
        "continue_with_mission_frame_priority"
    } else if command == "compact" {
        "review_compaction_against_mission_frame"
    } else {
        "review_or_continue_with_mission_frame_priority"
    };

    json!({
        "trace_id": format!("goal_alignment:{command}:{session_id}"),
        "project_id": resolved.project_id,
        "session_id": session_id,
        "command": command,
        "mission_frame_ref": mission_frame_ref,
        "alignment_status": alignment_status,
        "alignment_method": "deterministic_command_context_token_overlap_v1",
        "evidence_label": evidence_label,
        "mission_frame_present": mission_frame_present,
        "project_goal_supported": project_goal_supported,
        "milestone_goal_supported": milestone_goal_supported,
        "implementation_goal_supported": implementation_goal_supported,
        "support_scores": {
            "project_max_goal": project_score,
            "milestone_goal": milestone_score,
            "current_implementation_goal": implementation_score
        },
        "checked_goals": [
            "project_max_goal",
            "milestone_goal",
            "current_implementation_goal"
        ],
        "drift_reasons": drift_reasons,
        "drift_risks": drift_risks,
        "evidence_refs": [
            command,
            evidence_label
        ],
        "recommended_next_action": recommended_next_action
    })
}

fn read_compact_summary_text(
    data_dir: &Path,
    session_id: &str,
    summary_ref: &str,
) -> Result<String, std::io::Error> {
    std::fs::read_to_string(
        data_dir
            .join("sessions")
            .join(session_id)
            .join("summaries")
            .join(format!("{summary_ref}.md")),
    )
}

fn recent_highlights_text(summary_text: &str) -> String {
    let Some((_, highlights)) = summary_text.split_once("## Recent Highlights") else {
        return summary_text.to_string();
    };
    highlights.to_string()
}

fn goal_support_score(goal: &str, text: &str) -> f64 {
    let goal_tokens = significant_tokens(goal);
    if goal_tokens.is_empty() {
        return 0.0;
    }
    let text_tokens = significant_tokens(text);
    let supported = goal_tokens
        .iter()
        .filter(|token| text_tokens.contains(*token))
        .count();
    supported as f64 / goal_tokens.len() as f64
}

fn significant_tokens(text: &str) -> std::collections::BTreeSet<String> {
    const STOPWORDS: &[&str] = &[
        "a", "an", "and", "are", "as", "be", "by", "for", "from", "in", "into", "is", "it", "of",
        "on", "or", "that", "the", "to", "with",
    ];
    let mut tokens = std::collections::BTreeSet::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            current.push(character.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 4 && !STOPWORDS.contains(&current.as_str()) {
                tokens.insert(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 4 && !STOPWORDS.contains(&current.as_str()) {
        tokens.insert(current);
    }
    tokens
}

fn emit_session_resume_event(
    resolved: &ResolvedProject,
    command: &str,
    session_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "session_resume",
        "terminal",
        Some("succeeded"),
        "command",
        command,
        Some(session_id),
        payload,
    )
}

fn emit_runtime_preflight_event(
    resolved: &ResolvedProject,
    command: &str,
    session_id: &str,
    preflight: &crate::doctor::RuntimePreflightReport,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "runtime_preflight",
        "terminal",
        Some("succeeded"),
        "runtime",
        command,
        Some(session_id),
        serde_json::to_value(preflight).expect("preflight should serialize"),
    )
}

fn emit_session_open_event(
    resolved: &ResolvedProject,
    session_id: &str,
    transcript_path: &str,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "session_open",
        "terminal",
        Some("succeeded"),
        "session",
        session_id,
        Some(session_id),
        json!({
            "status": "active",
            "transcript_path": transcript_path
        }),
    )
}

fn emit_turn_event(
    resolved: &ResolvedProject,
    session_id: &str,
    turn_id: &str,
    phase: &str,
    terminal_outcome: Option<&str>,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "turn",
        phase,
        terminal_outcome,
        "turn",
        turn_id,
        Some(session_id),
        payload,
    )
}

fn emit_permission_event(
    resolved: &ResolvedProject,
    session_id: &str,
    request_id: &str,
    phase: &str,
    terminal_outcome: Option<&str>,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "permission",
        phase,
        terminal_outcome,
        "permission",
        request_id,
        Some(session_id),
        payload,
    )
}

fn emit_provider_resolution_event(
    resolved: &ResolvedProject,
    session_id: &str,
    turn_id: &str,
    provider_trace: &crate::providers::ProviderResolutionTrace,
    defaults: &PromptRequestDefaults,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "provider_resolution",
        "terminal",
        Some("succeeded"),
        "turn",
        turn_id,
        Some(session_id),
        json!({
            "turn_id": turn_id,
            "provider_trace": provider_trace,
            "reasoning_effort": defaults.reasoning_effort,
            "reasoning_effort_source": defaults.reasoning_effort_source,
            "permission_mode": defaults.permission_mode,
            "permission_mode_source": defaults.permission_mode_source
        }),
    )
}

fn emit_project_event(
    resolved: &ResolvedProject,
    object_id: &str,
    payload: Value,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "project",
        "terminal",
        Some("succeeded"),
        "project",
        object_id,
        None,
        payload,
    )
}

fn emit_permission_mode_event(
    resolved: &ResolvedProject,
    permission_mode: &str,
    scope: &str,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "permission_mode",
        "terminal",
        Some("succeeded"),
        "permission_mode",
        permission_mode,
        None,
        json!({
            "permission_mode": permission_mode,
            "scope": scope
        }),
    )
}

fn emit_mission_frame_event(
    resolved: &ResolvedProject,
    result: &crate::goals::MissionFrameStatus,
) -> Result<(), String> {
    append_canonical_event(
        resolved,
        "mission_frame",
        "terminal",
        Some("succeeded"),
        "mission_frame",
        "mission_frame",
        None,
        serde_json::to_value(result).expect("mission frame status should serialize"),
    )
}

fn command_emits_project_events(command: &str) -> bool {
    !matches!(command, "help" | "palette" | "slash help")
}

fn resolve_optional_event_project(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
) -> Option<ResolvedProject> {
    resolve_current_project(
        registry,
        parsed.project.as_deref(),
        parsed.cwd.as_deref(),
        cwd,
    )
    .ok()
}

#[derive(Debug, Clone)]
pub(crate) struct CommandAuditScope {
    resolved: ResolvedProject,
}

fn append_canonical_event(
    resolved: &ResolvedProject,
    event_name: &str,
    phase: &str,
    terminal_outcome: Option<&str>,
    object_kind: &str,
    object_id: &str,
    session_id: Option<&str>,
    payload: Value,
) -> Result<(), String> {
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let _guard = lock_event_log(&event_log_path).map_err(|err| err.to_string())?;
    let next_seq = read_events_from(&event_log_path)
        .map_err(|err| err.to_string())?
        .last()
        .map(|event| event.seq + 1)
        .unwrap_or(1);

    append_event_unlocked(
        &event_log_path,
        &KernelEventEnvelope {
            event_id: format!("evt_{event_name}_{next_seq}"),
            seq: next_seq,
            event_name: event_name.to_string(),
            phase: phase.to_string(),
            terminal_outcome: terminal_outcome.map(ToString::to_string),
            object_kind: object_kind.to_string(),
            object_id: object_id.to_string(),
            session_id: session_id.map(ToString::to_string),
            project_id: Some(resolved.project_id.clone()),
            timestamp: timestamp_string(),
            payload,
        },
    )
    .map_err(|err| err.to_string())
}

fn active_session_id_for(
    registry: &ProjectRegistry,
    resolved: &ResolvedProject,
    command: &str,
) -> Result<Option<String>, CommandFailureOutcome> {
    registry
        .get_by_project_id(&resolved.project_id)
        .map_err(|err| internal_failure(command, err.to_string()))
        .map(|entry| entry.and_then(|entry| entry.active_session_id))
}

fn apply_tui_config_action(
    resolved: &ResolvedProject,
    action: tui::TuiConfigAction,
) -> Result<tui::TuiConfigActionResult, String> {
    match action.kind {
        tui::TuiConfigActionKind::Model => {
            let model = action.value.trim();
            if model.is_empty() {
                return Err("model id is empty".to_string());
            }
            let provider_trace =
                resolve_provider_trace(None, Some(model), None, Some("tui_command"))
                    .map_err(|err| err.to_string())?;
            let result = config_set(
                resolved,
                "default_model",
                &provider_trace.resolved_model,
                ConfigScope::Project,
            )
            .map_err(|err| err.to_string())?;
            emit_config_event(
                resolved,
                "default_model",
                ConfigScope::Project.as_str(),
                &result,
            )?;
            Ok(tui::TuiConfigActionResult {
                applied: true,
                provider_id: Some(provider_trace.resolved_provider),
                model: Some(provider_trace.resolved_model),
                reasoning_effort: None,
                session_id: None,
                scope: ConfigScope::Project.as_str().to_string(),
                message: format!(
                    "model set {} --scope project --json",
                    result
                        .value
                        .as_ref()
                        .and_then(Value::as_str)
                        .unwrap_or(model)
                ),
            })
        }
        tui::TuiConfigActionKind::Reasoning => {
            let effort = action.value.trim();
            if !matches!(effort, "auto" | "low" | "medium" | "high") {
                return Err("reasoning effort must be auto, low, medium, or high".to_string());
            }
            let result = config_set(resolved, "reasoning_effort", effort, ConfigScope::Project)
                .map_err(|err| err.to_string())?;
            emit_config_event(
                resolved,
                "reasoning_effort",
                ConfigScope::Project.as_str(),
                &result,
            )?;
            Ok(tui::TuiConfigActionResult {
                applied: true,
                provider_id: None,
                model: None,
                reasoning_effort: Some(effort.to_string()),
                session_id: None,
                scope: ConfigScope::Project.as_str().to_string(),
                message: format!("config set reasoning_effort {effort} --scope project --json"),
            })
        }
        tui::TuiConfigActionKind::Session(_) => {
            Err("TUI session actions are not bound in the runtime executor yet".to_string())
        }
    }
}

fn apply_bound_tui_action(
    registry: &ProjectRegistry,
    resolved: &ResolvedProject,
    active_session_id: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
    action: tui::TuiConfigAction,
) -> Result<tui::TuiConfigActionResult, String> {
    match action.kind {
        tui::TuiConfigActionKind::Session(tui::TuiSessionActionKind::Resume) => {
            resume_tui_session(registry, resolved, active_session_id, &action.value)
        }
        _ => apply_tui_config_action(resolved, action),
    }
}

fn apply_tui_permission_action(
    resolved: &ResolvedProject,
    action: tui::TuiPermissionAction,
) -> Result<tui::TuiPermissionActionResult, String> {
    let decision = action.decision.as_command();
    let payload = resolve_permission_decision(resolved, decision, &action.request_id, false)
        .map_err(|failure| failure.render_for_stderr())?;
    Ok(tui::TuiPermissionActionResult {
        request_id: payload
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or(&action.request_id)
            .to_string(),
        decision: payload
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or(decision)
            .to_string(),
        pending_count: pending_permission_count(resolved).ok(),
        message: format!("permissions {decision} {} --json", action.request_id),
    })
}

fn resume_tui_session(
    registry: &ProjectRegistry,
    resolved: &ResolvedProject,
    active_session_id: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
    selector_raw: &str,
) -> Result<tui::TuiConfigActionResult, String> {
    let selector_label = if selector_raw.trim().is_empty() {
        "latest"
    } else {
        selector_raw.trim()
    };
    let store = SessionStore::new(resolved.data_dir.clone());
    let session = store
        .resolve_resume_target(parse_resume_selector(selector_label))
        .map_err(|err| err.to_string())?;
    registry
        .touch_project(&resolved.project_id, Some(session.session_id.clone()))
        .map_err(|err| err.to_string())?;
    emit_session_resume_event(
        resolved,
        "tui resume",
        &session.session_id,
        json!({
            "project_id": resolved.project_id,
            "session_id": session.session_id,
            "selector": selector_label
        }),
    )?;
    publish_project_checkpoint(resolved, &["session"])?;
    {
        let mut session_guard = active_session_id
            .lock()
            .map_err(|_| "TUI session state lock poisoned".to_string())?;
        *session_guard = Some(session.session_id.clone());
    }

    Ok(tui::TuiConfigActionResult::session_resumed(
        session.session_id,
        "project".to_string(),
        format!("sessions resume {selector_label} --json"),
    ))
}

fn ensure_tui_prompt_session(
    registry: &ProjectRegistry,
    resolved: &ResolvedProject,
    active_session_id: &mut Option<String>,
) -> Result<String, String> {
    if let Some(session_id) = active_session_id.clone() {
        return Ok(session_id);
    }

    let store = SessionStore::new(resolved.data_dir.clone());
    let session = store
        .create_session(Some("Astra TUI Session".to_string()))
        .map_err(|err| err.to_string())?;
    let stored = store
        .load_session(&session.session_id)
        .map_err(|err| err.to_string())?;
    registry
        .touch_project(&resolved.project_id, Some(session.session_id.clone()))
        .map_err(|err| err.to_string())?;
    emit_session_open_event(
        resolved,
        &session.session_id,
        &stored.transcript_path.display().to_string(),
    )?;
    publish_project_checkpoint(resolved, &["session"])?;
    *active_session_id = Some(session.session_id.clone());
    Ok(session.session_id)
}

fn tui_execution_from_turn_result(turn: &TurnResult) -> tui::TuiCommandExecution {
    let mut lines = Vec::new();
    if turn.outcome == TURN_OUTCOME_CANCELLED {
        if turn.assistant_content.trim().is_empty() {
            lines.push("Prompt cancelled.".to_string());
        } else {
            lines.push(format!(
                "Prompt cancelled after partial response:\n{}",
                turn.assistant_content
            ));
        }
    } else if turn.assistant_content.trim().is_empty() {
        lines.push("Prompt completed.".to_string());
    } else {
        lines.push(turn.assistant_content.clone());
    }
    lines.push(String::new());
    lines.push(format!(
        "tool: provider_completion | provider {} | model {} | outcome {} | mode text_completion",
        turn.provider_trace.resolved_provider, turn.provider_trace.resolved_model, turn.outcome
    ));
    lines.push(format!(
        "session {} | turn {} | provider {} | model {}",
        turn.session_id,
        turn.turn_id,
        turn.provider_trace.resolved_provider,
        turn.provider_trace.resolved_model
    ));
    if turn.outcome == TURN_OUTCOME_CANCELLED {
        lines.push(format!(
            "permission cancelled | session {} | turn {}",
            turn.session_id, turn.turn_id
        ));
    }
    if let Some(classification) = &turn.research_classification {
        lines.push(tui_research_classification_bridge_line(classification));
    } else if let Some(context) = &turn.research_context {
        lines.push(format!(
            "research context: thread {} | stage {} | next {} | confidence {}",
            context.active_thread_id,
            context.active_stage_id,
            context.next_recommended_action,
            context.confidence
        ));
    }
    let mut exec = tui::TuiCommandExecution::new(lines.join("\n"));
    exec.iterations = turn.agent_iterations;
    exec.tool_calls_made = turn.agent_tool_calls;
    exec
}

fn tui_research_classification_bridge_line(
    classification: &crate::research::ResearchTurnClassification,
) -> String {
    let thread = classification.thread_id.as_deref().unwrap_or("none");
    let stage = classification
        .stage_execution_id
        .as_deref()
        .or(classification.inferred_stage_id.as_deref())
        .unwrap_or("none");
    let operation = classification
        .candidate_operation
        .as_deref()
        .unwrap_or("none");
    format!(
        "research evidence: {} | thread {} | stage {} | operation {} | confidence {} | next {}",
        classification.natural_language_summary,
        thread,
        stage,
        operation,
        classification.confidence,
        classification.next_recommended_action
    )
}

fn remote_failure(
    command: &str,
    resolved: &ResolvedProject,
    err: remote::RemoteError,
    output_json: bool,
) -> CommandFailureOutcome {
    match err {
        remote::RemoteError::ActionRejected(rejection) => CommandFailureOutcome::new(
            12,
            command.to_string(),
            Some(resolved.project_id.clone()),
            None,
            "remote_action_rejected",
            rejection.reason.clone(),
            rejection.next_steps.first().cloned(),
            rejection.retryable,
            Some(serde_json::to_value(rejection).expect("remote rejection should serialize")),
            output_json,
        ),
        other => internal_failure(command, other.to_string()),
    }
}

fn host_surface_failure(
    command: &str,
    resolved: &ResolvedProject,
    err: host_surface::HostSurfaceError,
    output_json: bool,
) -> CommandFailureOutcome {
    CommandFailureOutcome::new(
        10,
        command.to_string(),
        Some(resolved.project_id.clone()),
        None,
        "host_surface_operation_failed",
        err.to_string(),
        Some(
            "Inspect `research-cli host surface status --json` and remote pairing state."
                .to_string(),
        ),
        false,
        None,
        output_json,
    )
}

fn run_failure(
    command: &str,
    resolved: &ResolvedProject,
    err: crate::orchestration::OrchestrationError,
    output_json: bool,
) -> CommandFailureOutcome {
    let code = if matches!(
        &err,
        crate::orchestration::OrchestrationError::NoActiveRun(_)
    ) {
        "run_not_found"
    } else {
        "run_operation_failed"
    };
    CommandFailureOutcome::new(
        10,
        command.to_string(),
        Some(resolved.project_id.clone()),
        None,
        code,
        err.to_string(),
        Some(
            "Use `research-cli run status --json` to inspect active orchestration state."
                .to_string(),
        ),
        false,
        None,
        output_json,
    )
}

fn reject_remote_not_ready(
    command: &str,
    action: &str,
    resolved: &ResolvedProject,
    registry: &ProjectRegistry,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let state_home = state_home().map_err(|err| internal_failure(command, err))?;
    let report = remote::status(
        &state_home,
        resolved,
        active_session_id_for(registry, resolved, command)?,
    )
    .map_err(|err| internal_failure(command, err.to_string()))?;
    if report.remote_ready {
        return Ok(());
    }
    let rejection = match report.revocation_reason.as_deref() {
        Some("lease_expired") => remote::lease_expired_rejection(action),
        Some("revoked") => remote::binding_revoked_rejection(action),
        _ => remote::attach_rejection(action),
    };
    Err(CommandFailureOutcome::new(
        12,
        command.to_string(),
        Some(resolved.project_id.clone()),
        None,
        "remote_action_rejected",
        rejection.reason.clone(),
        rejection.next_steps.first().cloned(),
        rejection.retryable,
        Some(serde_json::to_value(rejection).expect("remote rejection should serialize")),
        output_json,
    ))
}

fn command_label_from_parsed(parsed: &ParsedCliArgs) -> String {
    if parsed.command_args.is_empty() {
        if parsed.continue_requested {
            "continue".to_string()
        } else {
            "launch".to_string()
        }
    } else if parsed.command_args[0] == "slash" && parsed.command_args.len() > 1 {
        format!("slash {}", parsed.command_args[1])
    } else if matches!(
        parsed.command_args[0].as_str(),
        "model"
            | "config"
            | "providers"
            | "setup"
            | "mcp"
            | "skills"
            | "plugins"
            | "hooks"
            | "projects"
            | "goals"
            | "permissions"
            | "memory"
            | "artifacts"
            | "docs"
            | "repo"
            | "sessions"
            | "run"
    ) && parsed.command_args.len() > 1
        && !parsed.command_args[1].starts_with("--")
    {
        format!("{} {}", parsed.command_args[0], parsed.command_args[1])
    } else {
        parsed.command_args[0].clone()
    }
}

fn prepare_command_audit(
    registry: &ProjectRegistry,
    cwd: &Path,
    parsed: &ParsedCliArgs,
    command: &str,
) -> Option<CommandAuditScope> {
    if env::var("RESEARCH_CLI_TERMINAL_RENDER_CHILD")
        .ok()
        .as_deref()
        == Some("1")
    {
        return None;
    }
    if !command_emits_project_events(command) {
        return None;
    }
    if command == "projects init" {
        let binding = resolve_workspace_from(command_cwd(cwd, parsed)).ok()?;
        let project_id = stable_project_id(&binding.workspace_root).ok()?;
        let data_dir = registry.project_data_dir(&project_id);
        return Some(CommandAuditScope {
            resolved: ResolvedProject {
                project_id,
                workspace_root: binding.workspace_root.clone(),
                workspace_hash: crate::workspace::hash::hash_workspace_root(
                    &binding.workspace_root,
                ),
                data_dir,
                resolution_source: "projects_init".to_string(),
            },
        });
    }
    resolve_current_project(
        registry,
        parsed.project.as_deref(),
        parsed.cwd.as_deref(),
        cwd,
    )
    .ok()
    .map(|resolved| CommandAuditScope { resolved })
}

fn emit_command_start(scope: &CommandAuditScope, command: &str) -> Result<(), String> {
    append_canonical_event(
        &scope.resolved,
        "command",
        "start",
        None,
        "command",
        command,
        None,
        json!({
            "command": command,
            "scope": {
                "project_id": scope.resolved.project_id,
                "workspace_root": scope.resolved.workspace_root.display().to_string(),
                "resolution_source": scope.resolved.resolution_source
            }
        }),
    )
}

fn emit_command_terminal_success(
    scope: &CommandAuditScope,
    envelope: &CommandSuccess,
) -> Result<(), String> {
    append_canonical_event(
        &scope.resolved,
        "command",
        "terminal",
        Some("succeeded"),
        "command",
        &envelope.command,
        envelope.session_id.as_deref(),
        json!({
            "command": envelope.command,
            "ok": true
        }),
    )
}

fn emit_command_terminal_failure(
    scope: &CommandAuditScope,
    envelope: &CommandFailure,
) -> Result<(), String> {
    let outcome = match envelope.error.code.as_str() {
        "permission_unresolved"
        | "runtime_preflight_blocked"
        | "doctor_blocked"
        | "smoke_blocked"
        | "provider_test_blocked" => "blocked",
        "project_unresolved" if envelope.error.message.contains("stale") => "stale_conflict",
        _ => "failed",
    };

    append_canonical_event(
        &scope.resolved,
        "command",
        "terminal",
        Some(outcome),
        "command",
        &envelope.command,
        envelope.session_id.as_deref(),
        json!({
            "command": envelope.command,
            "ok": false,
            "error": envelope.error
        }),
    )
}

fn finalize_command_outcome(
    command_audit: Option<&CommandAuditScope>,
    output_json: bool,
    result: Result<CommandSuccess, CommandFailureOutcome>,
) -> Result<i32, String> {
    match result {
        Ok(envelope) => {
            if let Some(audit) = command_audit {
                let _ = emit_command_terminal_success(audit, &envelope);
            }
            emit_output(output_json, &envelope).map_err(|err| err.to_string())?;
            Ok(0)
        }
        Err(failure) => {
            if let Some(audit) = command_audit {
                let _ = emit_command_terminal_failure(audit, &failure.envelope);
            }
            emit_output(output_json, &failure.envelope).map_err(|err| err.to_string())?;
            Ok(failure.exit_code)
        }
    }
}

fn publish_project_checkpoint(
    resolved: &ResolvedProject,
    touched_families: &[&str],
) -> Result<(), String> {
    const CHECKPOINT_PUBLISH_RETRY_COUNT: usize = 20;
    const CHECKPOINT_PUBLISH_RETRY_SLEEP_MS: u64 = 25;
    let checkpoint_path = resolved.data_dir.join("project_state.json");
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let touched_families = touched_families
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    let mut last_error = None;
    for attempt in 0..=CHECKPOINT_PUBLISH_RETRY_COUNT {
        let committed = if checkpoint_path.exists() {
            KernelStateBundle::load_from(&checkpoint_path).map_err(|err| err.to_string())?
        } else {
            KernelStateBundle::default()
        };
        match publish_checkpoint(
            &checkpoint_path,
            &event_log_path,
            CheckpointPublishPlan {
                base_seq_cursor: committed.seq_cursor,
                base_checkpoint_epoch: committed.checkpoint_epoch,
                touched_families: touched_families.clone(),
            },
        ) {
            Ok(_) => return Ok(()),
            Err(err) => {
                let message = err.to_string();
                if !message.contains("checkpoint write lane is busy")
                    || attempt == CHECKPOINT_PUBLISH_RETRY_COUNT
                {
                    return Err(message);
                }
                last_error = Some(message);
                std::thread::sleep(std::time::Duration::from_millis(
                    CHECKPOINT_PUBLISH_RETRY_SLEEP_MS,
                ));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "checkpoint publish failed".to_string()))
}

fn provider_test_failure(err: ProviderTestError) -> CommandFailureOutcome {
    match err {
        ProviderTestError::UnknownProvider(provider_id) => CommandFailureOutcome::new(
            6,
            "providers test".to_string(),
            None,
            None,
            "provider_not_found",
            format!("unknown provider: {provider_id}"),
            Some(
                "Inspect the embedded provider catalog instead of inventing a provider id."
                    .to_string(),
            ),
            false,
            Some(json!({ "provider_id": provider_id })),
            false,
        ),
        ProviderTestError::AuthMissing {
            provider_id,
            env_var,
        } => CommandFailureOutcome::new(
            4,
            "providers test".to_string(),
            None,
            None,
            "provider_test_blocked",
            format!("provider {provider_id} is missing required credential: {env_var}"),
            Some(format!("Set {env_var} and retry the provider healthcheck.")),
            false,
            Some(json!({
                "provider_id": provider_id,
                "overall_status": "blocked",
                "live_ready": false
            })),
            false,
        ),
        ProviderTestError::TransportBlocked {
            provider_id,
            transport,
            message,
        } => CommandFailureOutcome::new(
            4,
            "providers test".to_string(),
            None,
            None,
            "provider_test_blocked",
            format!("provider {provider_id} transport check failed: {message}"),
            Some("Check the provider base URL and local network reachability.".to_string()),
            true,
            Some(json!({
                "provider_id": provider_id,
                "overall_status": "blocked",
                "live_ready": false,
                "transport": transport
            })),
            false,
        ),
    }
}

fn timestamp_string() -> String {
    timestamp_millis().to_string()
}

fn timestamp_string_is_after(candidate: &str, baseline: &str) -> bool {
    match (candidate.parse::<u128>(), baseline.parse::<u128>()) {
        (Ok(candidate), Ok(baseline)) => candidate > baseline,
        _ => candidate > baseline,
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
}

fn compact_subprocess_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let combined = if stderr.trim().is_empty() {
        stdout.into_owned()
    } else if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        format!("stdout={}; stderr={}", stdout.trim(), stderr.trim())
    };
    let compact = combined.replace('\n', " ").trim().to_string();
    if compact.chars().count() > 240 {
        compact.chars().take(240).collect::<String>() + "..."
    } else {
        compact
    }
}

#[cfg(test)]
mod tests;

impl fmt::Display for CommandFailureOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render_for_stderr())
    }
}
