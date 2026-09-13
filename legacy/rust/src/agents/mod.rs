use crate::board_task_refs;
use crate::canonical_artifacts;
use crate::review_refs;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROVIDER_WORKER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_PROVIDER_WORKER_MAX_RUNTIME_MS: u64 = 300_000;
const EXTENDED_PROVIDER_WORKER_MAX_RUNTIME_MS: u64 = 900_000;
const PROVIDER_WORKER_EXTENDED_INPUT_REF_THRESHOLD: usize = 20;
const PROVIDER_WORKER_EXTENDED_TASK_COMPLEXITY_THRESHOLD: usize = 30;
const PROVIDER_WORKER_EXTENDED_OUTPUT_FIELD_THRESHOLD: usize = 14;
const PROVIDER_WORKER_EXTENDED_REVIEW_SURFACE_THRESHOLD: usize = 10;
const PROVIDER_TOOL_RECEIPT_PREVIEW_CHARS: usize = 4096;
const PROVIDER_TOOL_RECEIPT_SNAPSHOT_MAX_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPacket {
    pub schema_version: String,
    pub task_packet_id: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_kind: Option<String>,
    pub intent: String,
    pub role_profile: String,
    pub retention_policy: String,
    pub run_class: String,
    pub io_mode: String,
    pub resume_policy: String,
    pub replay_seed_ref: String,
    pub budget: AgentBudget,
    pub scope: AgentScope,
    pub write_authority: String,
    pub success_criteria: Vec<String>,
    pub output_manifest_required: bool,
    pub review_gate_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_task_contract: Option<AgentStageTaskContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collaboration_protocol: Option<AgentCollaborationProtocol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_refs: Vec<AgentSkillRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_policy: Option<AgentToolPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBudget {
    pub max_turns: u64,
    pub max_runtime_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentScope {
    pub workspace_root: String,
    pub allowed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStageTaskContract {
    pub schema_version: String,
    pub task_id: String,
    pub stage_execution_id: String,
    pub stage_id: String,
    pub task_type: String,
    pub worker_role: String,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_canonical_artifacts: Vec<canonical_artifacts::RequiredCanonicalArtifact>,
    pub required_output_artifact_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_output_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_signals: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on_task_ids: Vec<String>,
    pub priority: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_findings_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocker_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_target_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_target_evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replacement_of_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_evidence_set_id: Option<String>,
    #[serde(default)]
    pub consumed_input_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCollaborationProtocol {
    pub schema_version: String,
    pub protocol_id: String,
    pub main_agent_authority: String,
    pub runtime_authority: String,
    pub worker_authority: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_worker_capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_worker_output_sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_worker_actions: Vec<String>,
    pub artifact_flow: String,
    pub adoption_rule: String,
    pub review_rule: String,
    pub cleanup_rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkillRef {
    pub skill_id: String,
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolPolicy {
    pub schema_version: String,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimeRecord {
    pub schema_version: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_identity_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    pub task_packet_ref: String,
    pub lifecycle_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub heartbeat_at: String,
    pub output_manifest_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_binding_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directive_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_ref: Option<String>,
    pub trace_refs: Vec<String>,
    pub stop_reason: String,
    pub failure_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimeIdentity {
    pub schema_version: String,
    pub runtime_id: String,
    pub agent_id: String,
    pub runtime_kind: String,
    pub role_kind: String,
    pub role_profile: String,
    pub runner_kind: String,
    pub authority_scope: String,
    pub lifecycle_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_packet_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_manifest_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRuntimeIdentityWriteResult {
    pub runtime_identity_ref: String,
    pub identity: AgentRuntimeIdentity,
}

#[derive(Debug, Clone)]
pub struct MainAgentRuntimeIdentityRequest {
    pub job_id: String,
    pub session_id: String,
    pub round_index: usize,
    pub provider_id: String,
    pub model: String,
    pub artifact_ref: String,
    pub lifecycle_status: String,
    pub tool_scope: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    pub schema_version: String,
    pub agent_id: String,
    pub trace_id: String,
    pub task_packet_ref: String,
    pub runtime_events: Vec<AgentTraceEvent>,
    pub tool_actions: Vec<AgentTraceEvent>,
    pub permission_decisions: Vec<AgentTraceEvent>,
    pub output_records: Vec<AgentTraceEvent>,
    pub final_status: String,
    pub created_at: String,
    pub trace_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTraceEvent {
    pub event: String,
    pub detail: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutputManifest {
    pub schema_version: String,
    pub agent_id: String,
    pub manifest_id: String,
    pub status: String,
    pub output_refs: Vec<AgentOutputRef>,
    pub validation_status: String,
    pub validation_errors: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutputRef {
    pub kind: String,
    pub r#ref: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolReceiptManifest {
    pub schema_version: String,
    pub agent_id: String,
    pub authority_scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    pub receipt_count: usize,
    pub receipts: Vec<AgentToolReceipt>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolReceipt {
    pub call_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub tool_name: String,
    pub status: String,
    pub arguments: serde_json::Value,
    pub output_bytes: usize,
    pub output_sha256: String,
    pub output_preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_ref: Option<String>,
    pub snapshot_bytes: usize,
    pub snapshot_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_summary: Option<serde_json::Value>,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkspaceBinding {
    pub schema_version: String,
    pub agent_id: String,
    pub mode: String,
    pub source_workspace_root: String,
    pub worktree_path: String,
    pub git_head: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_artifact_ledger_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_artifact_ledger_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_overlay_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_patch_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untracked_manifest_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentDirtyWorkspaceOverlayManifest {
    schema_version: String,
    agent_id: String,
    source_workspace_root: String,
    worktree_path: String,
    git_head: String,
    status_entries: Vec<String>,
    tracked_patch_ref: Option<String>,
    tracked_patch_sha256: Option<String>,
    staged_patch_ref: Option<String>,
    staged_patch_sha256: Option<String>,
    untracked_manifest_ref: Option<String>,
    untracked_paths: Vec<String>,
    applied: bool,
    generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentUntrackedWorkspaceFileManifest {
    schema_version: String,
    agent_id: String,
    files: Vec<AgentUntrackedWorkspaceFileEntry>,
    generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentUntrackedWorkspaceFileEntry {
    relative_path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorktreeArtifactCandidateManifest {
    pub schema_version: String,
    pub agent_id: String,
    pub authority_scope: String,
    pub adoption_status: String,
    pub workspace_binding_ref: String,
    pub source_workspace_root: String,
    pub worktree_path: String,
    pub git_head: String,
    pub status_entries: Vec<String>,
    pub changed_paths: Vec<String>,
    pub untracked_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_entries: Vec<AgentWorktreeArtifactCandidateEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_ref: Option<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentWorkerWorkspaceBaselineManifest {
    schema_version: String,
    agent_id: String,
    authority_scope: String,
    workspace_binding_ref: String,
    worktree_path: String,
    entries: Vec<AgentWorkerWorkspaceBaselineEntry>,
    generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentWorkerWorkspaceBaselineEntry {
    relative_path: String,
    path_kind: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorktreeArtifactCandidateEntry {
    pub relative_path: String,
    pub path_kind: String,
    pub artifact_kind: String,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_archive_ref: Option<String>,
    pub safe_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsafe_reason: Option<String>,
    pub from_input_bundle: bool,
    pub is_directory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_manifest_ref: Option<String>,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorktreeDirectoryCandidateManifest {
    pub schema_version: String,
    pub root_relative_path: String,
    pub entries: Vec<AgentWorktreeDirectoryCandidateEntry>,
    pub total_size_bytes: u64,
    pub file_count: usize,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorktreeDirectoryCandidateEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInputArtifactMountManifest {
    pub schema_version: String,
    pub agent_id: String,
    pub authority_scope: String,
    pub source_workspace_root: String,
    pub worktree_path: String,
    pub manifest_path_in_worktree: String,
    #[serde(default)]
    pub workspace_visibility: AgentWorkspaceVisibility,
    pub mounted_inputs: Vec<AgentInputArtifactMount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmounted_input_refs: Vec<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentWorkspaceVisibility {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub visibility_scope: String,
    #[serde(default)]
    pub source_workspace_root: String,
    #[serde(default)]
    pub worktree_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_mode: Option<String>,
    #[serde(default)]
    pub git_head: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_root_entries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visible_root_entries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_marker_paths_visible: Vec<String>,
    #[serde(default)]
    pub project_marker_status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInputArtifactMount {
    pub input_ref: String,
    pub source_path: String,
    pub mounted_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_manifest_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundled_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedInputArtifact {
    input_ref: String,
    source_paths: Vec<PathBuf>,
    bundle_kind: String,
    fail_if_missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStartResult {
    pub status: String,
    pub agent_id: String,
    pub task_packet_path: String,
    pub runtime_record_path: String,
    pub output_manifest_path: String,
    pub trace_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_binding: Option<AgentWorkspaceBinding>,
    pub task_packet: TaskPacket,
    pub runtime_record: AgentRuntimeRecord,
    pub output_manifest: AgentOutputManifest,
    pub trace: AgentTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListEntry {
    pub agent_id: String,
    pub lifecycle_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_kind: Option<String>,
    pub intent: String,
    pub role_profile: String,
    pub created_at: String,
    pub updated_at: String,
    pub task_packet_ref: String,
    pub output_manifest_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_binding_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListResult {
    pub agents: Vec<AgentListEntry>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInspectionResult {
    pub agent_id: String,
    pub agent_root: String,
    pub task_packet: TaskPacket,
    pub runtime_record: AgentRuntimeRecord,
    pub output_manifest: AgentOutputManifest,
    pub trace: AgentTrace,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_binding: Option<AgentWorkspaceBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStopResult {
    pub agent_id: String,
    pub idempotent: bool,
    pub runtime_record: AgentRuntimeRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStaleRunningReclaimResult {
    pub agent_id: String,
    pub reclaimed: bool,
    pub reason: String,
    pub runtime_record: AgentRuntimeRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAgentBackgroundRunResult {
    pub schema_version: String,
    pub status: String,
    pub agent_id: String,
    pub pid: u32,
    pub task_packet_path: String,
    pub runtime_record_path: String,
    pub output_manifest_path: String,
    pub trace_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTracesResult {
    pub agent_id: String,
    pub trace_path: String,
    pub trace: AgentTrace,
}

#[derive(Debug, Clone)]
pub struct AgentStartRequest {
    pub intent: String,
    pub role_profile: String,
    pub message: String,
    pub stage_task_contract: Option<AgentStageTaskContract>,
}

#[derive(Debug, Clone)]
pub struct LocalAgentStartRequest {
    pub intent: String,
    pub role_profile: String,
    pub message: String,
    pub command: String,
    pub stage_task_contract: Option<AgentStageTaskContract>,
}

#[derive(Debug, Clone)]
pub struct ProviderAgentStartRequest {
    pub intent: String,
    pub role_profile: String,
    pub message: String,
    pub provider_id: String,
    pub model: String,
    pub automation_mode: Option<String>,
    pub stage_task_contract: Option<AgentStageTaskContract>,
}

#[derive(Debug, Clone)]
struct ProviderWorkerRun {
    output: String,
    session_id: String,
    iterations: usize,
    tool_calls_made: usize,
    finish_reason: String,
    tool_scope: Vec<String>,
    tool_actions: Vec<AgentTraceEvent>,
    tool_receipts: AgentToolReceiptManifest,
}

#[derive(Debug)]
struct ProviderWorkerFailure {
    error: AgentError,
    iterations: usize,
    tool_calls_made: usize,
    tool_actions: Vec<AgentTraceEvent>,
    tool_receipts: Option<AgentToolReceiptManifest>,
}

impl ProviderWorkerFailure {
    fn new(error: AgentError) -> Self {
        let (iterations, tool_calls_made) = worker_loop_counts_from_agent_error(&error);
        Self {
            error,
            iterations,
            tool_calls_made,
            tool_actions: Vec::new(),
            tool_receipts: None,
        }
    }
}

#[derive(Debug)]
pub enum AgentError {
    UnknownAgent(String),
    InvalidTaskPacket {
        reason: String,
    },
    StaleScope {
        expected: String,
        actual: String,
    },
    MissingWriteAuthority {
        required: String,
        actual: String,
    },
    DirtySourceWorktree {
        dirty_paths: Vec<String>,
    },
    GitCommand {
        action: String,
        stderr: String,
    },
    LocalCommand(String),
    ProviderExecution(String),
    AgentLoopExhausted(String),
    Io(std::io::Error),
    IoContext {
        action: String,
        path: String,
        source: std::io::Error,
    },
    Serde(serde_json::Error),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAgent(agent_id) => write!(f, "unknown agent: {agent_id}"),
            Self::InvalidTaskPacket { reason } => write!(f, "invalid task packet: {reason}"),
            Self::StaleScope { expected, actual } => {
                write!(
                    f,
                    "stale task packet scope: expected {expected}, got {actual}"
                )
            }
            Self::MissingWriteAuthority { required, actual } => write!(
                f,
                "missing task packet write authority: required {required}, got {actual}"
            ),
            Self::DirtySourceWorktree { dirty_paths } => write!(
                f,
                "source worktree has uncommitted changes: {}",
                dirty_paths.join(", ")
            ),
            Self::GitCommand { action, stderr } => {
                write!(f, "git command failed during {action}: {stderr}")
            }
            Self::LocalCommand(message) => write!(f, "local runner failed: {message}"),
            Self::ProviderExecution(message) => write!(f, "provider runner failed: {message}"),
            Self::AgentLoopExhausted(message) => {
                write!(f, "agent loop exhausted before producing output: {message}")
            }
            Self::Io(err) => write!(f, "agent IO failed: {err}"),
            Self::IoContext {
                action,
                path,
                source,
            } => {
                write!(f, "agent IO failed while {action} `{path}`: {source}")
            }
            Self::Serde(err) => write!(f, "agent serialization failed: {err}"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<std::io::Error> for AgentError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn start_mock(
    data_dir: &Path,
    workspace_root: &Path,
    request: AgentStartRequest,
) -> Result<AgentStartResult, AgentError> {
    let seed = timestamp_string();
    let created_at = seed.clone();
    let agent_id = format!("agent_{seed}");
    let task_packet_id = format!("task_{seed}");
    let trace_id = format!("trace_{seed}");
    let manifest_id = format!("manifest_{seed}");

    let root = agent_dir(data_dir, &agent_id);
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;
    let role_profile = request.role_profile;
    let stage_task_contract = request.stage_task_contract;
    let role_package = resolve_worker_role_package(workspace_root, &role_profile)?;

    let task_packet = TaskPacket {
        schema_version: "v1alpha1".to_string(),
        task_packet_id,
        agent_id: agent_id.clone(),
        runner_kind: Some("mock".to_string()),
        intent: request.intent,
        role_profile,
        retention_policy: "ephemeral".to_string(),
        run_class: "bounded".to_string(),
        io_mode: "request_response".to_string(),
        resume_policy: "fresh_thread".to_string(),
        replay_seed_ref: String::new(),
        budget: AgentBudget {
            max_turns: 1,
            max_runtime_ms: 30_000,
        },
        scope: AgentScope {
            workspace_root: workspace_root.display().to_string(),
            allowed_paths: vec![".".to_string()],
        },
        write_authority: "none".to_string(),
        success_criteria: vec![
            "task packet persisted".to_string(),
            "runtime record persisted".to_string(),
            "trace persisted".to_string(),
            "output manifest persisted".to_string(),
        ],
        output_manifest_required: true,
        review_gate_required: true,
        stage_task_contract,
        collaboration_protocol: Some(default_agent_collaboration_protocol()),
        skill_refs: role_package_skill_refs(role_package.skill_refs),
        tool_policy: Some(role_package_tool_policy(role_package.tool_policy)),
        command: None,
        message: request.message,
        created_at: created_at.clone(),
    };
    let output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id,
        status: "complete".to_string(),
        output_refs: vec![AgentOutputRef {
            kind: "mock_summary".to_string(),
            r#ref: trace_path.display().to_string(),
            summary: "mock lifecycle completed without spawning an external worker".to_string(),
        }],
        validation_status: "valid".to_string(),
        validation_errors: Vec::new(),
        created_at: created_at.clone(),
    };
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id,
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events: vec![
            trace_event("packet_created", "task packet accepted", &created_at),
            trace_event(
                "mock_agent_completed",
                "bounded mock run finished",
                &created_at,
            ),
        ],
        tool_actions: Vec::new(),
        permission_decisions: Vec::new(),
        output_records: vec![trace_event(
            "manifest_written",
            "output manifest recorded",
            &created_at,
        )],
        final_status: "succeeded".to_string(),
        created_at: created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: Some("mock".to_string()),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref: None,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: "succeeded".to_string(),
        created_at: created_at.clone(),
        updated_at: created_at.clone(),
        heartbeat_at: created_at.clone(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: None,
        directive_ref: None,
        status_ref: None,
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: String::new(),
        failure_code: String::new(),
    };
    let runtime_identity = runtime_identity_for_task_packet(
        &task_packet,
        "succeeded",
        "worker_evidence_only",
        &runtime_record,
        Some(&output_manifest_path),
        &created_at,
    );

    atomic_write(
        &task_packet_path,
        &serde_json::to_string_pretty(&task_packet)?,
    )?;
    atomic_write(
        &runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        &output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(&trace_path, &serde_json::to_string_pretty(&trace)?)?;

    Ok(AgentStartResult {
        status: "succeeded".to_string(),
        agent_id,
        task_packet_path: task_packet_path.display().to_string(),
        runtime_record_path: runtime_record_path.display().to_string(),
        output_manifest_path: output_manifest_path.display().to_string(),
        trace_path: trace_path.display().to_string(),
        workspace_binding: None,
        task_packet,
        runtime_record,
        output_manifest,
        trace,
    })
}

pub fn start_local(
    data_dir: &Path,
    workspace_root: &Path,
    request: LocalAgentStartRequest,
) -> Result<AgentStartResult, AgentError> {
    let seed = timestamp_string();
    let created_at = seed.clone();
    let agent_id = format!("agent_{seed}");
    let task_packet_id = format!("task_{seed}");
    let role_profile = request.role_profile;
    let stage_task_contract = request.stage_task_contract;
    let role_package = resolve_worker_role_package(workspace_root, &role_profile)?;
    let task_packet = TaskPacket {
        schema_version: "v1alpha1".to_string(),
        task_packet_id,
        agent_id,
        runner_kind: Some("local".to_string()),
        intent: request.intent,
        role_profile,
        retention_policy: "ephemeral".to_string(),
        run_class: "bounded".to_string(),
        io_mode: "request_response".to_string(),
        resume_policy: "fresh_thread".to_string(),
        replay_seed_ref: String::new(),
        budget: AgentBudget {
            max_turns: 1,
            max_runtime_ms: 30_000,
        },
        scope: AgentScope {
            workspace_root: workspace_root.display().to_string(),
            allowed_paths: vec![".".to_string()],
        },
        write_authority: "workspace_write".to_string(),
        success_criteria: vec![
            "task packet persisted before execution".to_string(),
            "git worktree bound before execution".to_string(),
            "directive and status files persisted".to_string(),
            "runtime trace and output manifest persisted".to_string(),
        ],
        output_manifest_required: true,
        review_gate_required: true,
        stage_task_contract,
        collaboration_protocol: Some(default_agent_collaboration_protocol()),
        skill_refs: role_package_skill_refs(role_package.skill_refs),
        tool_policy: Some(role_package_tool_policy(role_package.tool_policy)),
        command: Some(request.command),
        message: request.message,
        created_at,
    };
    start_local_from_packet(data_dir, workspace_root, task_packet)
}

pub fn start_provider(
    data_dir: &Path,
    workspace_root: &Path,
    request: ProviderAgentStartRequest,
) -> Result<AgentStartResult, AgentError> {
    let seed = timestamp_string();
    let created_at = seed.clone();
    let agent_id = format!("agent_{seed}");
    let task_packet_id = format!("task_{seed}");
    let role_profile = request.role_profile;
    let stage_task_contract = request.stage_task_contract;
    let role_package = resolve_worker_role_package(workspace_root, &role_profile)?;
    let task_packet = TaskPacket {
        schema_version: "v1alpha1".to_string(),
        task_packet_id,
        agent_id,
        runner_kind: Some("provider".to_string()),
        intent: request.intent,
        role_profile,
        retention_policy: "ephemeral".to_string(),
        run_class: provider_run_class(request.automation_mode.as_deref()).to_string(),
        io_mode: "request_response".to_string(),
        resume_policy: "fresh_thread".to_string(),
        replay_seed_ref: String::new(),
        budget: provider_worker_budget_for_stage_task(stage_task_contract.as_ref()),
        scope: AgentScope {
            workspace_root: workspace_root.display().to_string(),
            allowed_paths: vec![".".to_string()],
        },
        write_authority: "workspace_write".to_string(),
        success_criteria: vec![
            "task packet persisted before execution".to_string(),
            "provider-backed worker output persisted".to_string(),
            "runtime trace and output manifest persisted".to_string(),
        ],
        output_manifest_required: true,
        review_gate_required: true,
        stage_task_contract,
        collaboration_protocol: Some(default_agent_collaboration_protocol()),
        skill_refs: role_package_skill_refs(role_package.skill_refs),
        tool_policy: Some(role_package_tool_policy(role_package.tool_policy)),
        command: None,
        message: request.message,
        created_at,
    };
    start_provider_from_packet(
        data_dir,
        workspace_root,
        task_packet,
        &request.provider_id,
        &request.model,
    )
}

pub fn start_provider_background(
    data_dir: &Path,
    workspace_root: &Path,
    request: ProviderAgentStartRequest,
) -> Result<AgentStartResult, AgentError> {
    let seed = timestamp_string();
    let created_at = seed.clone();
    let agent_id = format!("agent_{seed}");
    let task_packet_id = format!("task_{seed}");
    let role_profile = request.role_profile;
    let stage_task_contract = request.stage_task_contract;
    let role_package = resolve_worker_role_package(workspace_root, &role_profile)?;
    let task_packet = TaskPacket {
        schema_version: "v1alpha1".to_string(),
        task_packet_id,
        agent_id,
        runner_kind: Some("provider".to_string()),
        intent: request.intent,
        role_profile,
        retention_policy: "ephemeral".to_string(),
        run_class: provider_run_class(request.automation_mode.as_deref()).to_string(),
        io_mode: "request_response".to_string(),
        resume_policy: "fresh_thread".to_string(),
        replay_seed_ref: String::new(),
        budget: provider_worker_budget_for_stage_task(stage_task_contract.as_ref()),
        scope: AgentScope {
            workspace_root: workspace_root.display().to_string(),
            allowed_paths: vec![".".to_string()],
        },
        write_authority: "workspace_write".to_string(),
        success_criteria: vec![
            "task packet persisted before execution".to_string(),
            "provider-backed worker output persisted".to_string(),
            "runtime trace and output manifest persisted".to_string(),
        ],
        output_manifest_required: true,
        review_gate_required: true,
        stage_task_contract,
        collaboration_protocol: Some(default_agent_collaboration_protocol()),
        skill_refs: role_package_skill_refs(role_package.skill_refs),
        tool_policy: Some(role_package_tool_policy(role_package.tool_policy)),
        command: None,
        message: request.message,
        created_at,
    };
    start_provider_background_from_packet(
        data_dir,
        workspace_root,
        task_packet,
        &request.provider_id,
        &request.model,
    )
}

pub fn start_provider_from_task_packet_file(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet_path: &Path,
    provider_id: &str,
    model: &str,
) -> Result<AgentStartResult, AgentError> {
    let task_packet = read_task_packet_file(task_packet_path)?;
    run_provider_from_persisted_packet(data_dir, workspace_root, task_packet, provider_id, model)
}

fn start_provider_background_from_packet(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: TaskPacket,
    provider_id: &str,
    model: &str,
) -> Result<AgentStartResult, AgentError> {
    if let Err(err) = validate_provider_task_packet(data_dir, workspace_root, &task_packet) {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }
    if let Err(err) = ensure_source_git_repository(workspace_root) {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }
    if let Err(err) = ensure_source_worktree_has_head(workspace_root) {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }

    let created_at = task_packet.created_at.clone();
    let agent_id = task_packet.agent_id.clone();
    let seed = timestamp_string();
    let trace_id = format!("trace_{seed}");
    let manifest_id = format!("manifest_{seed}");

    let root = agent_dir(data_dir, &agent_id);
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    let workspace_binding_path = workspace_binding_path(data_dir, &agent_id);
    let input_mounts_path = input_mounts_path(data_dir, &agent_id);
    let status_path = status_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;

    let workspace_binding = create_git_worktree(data_dir, workspace_root, &agent_id, &created_at)?;
    let _input_mounts = mount_task_input_artifacts(
        &agent_id,
        data_dir,
        workspace_root,
        &workspace_binding,
        &task_packet,
        &input_mounts_path,
        &created_at,
    )?;
    record_worker_workspace_baseline(data_dir, &workspace_binding, &task_packet, &created_at)?;

    atomic_write(
        &task_packet_path,
        &serde_json::to_string_pretty(&task_packet)?,
    )?;
    atomic_write(
        &workspace_binding_path,
        &serde_json::to_string_pretty(&workspace_binding)?,
    )?;

    let mut child = match spawn_provider_worker_process(
        workspace_root,
        &task_packet_path,
        provider_id,
        model,
        &root.join("provider-worker.log"),
    ) {
        Ok(child) => child,
        Err(err) => {
            let _ =
                persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
            return Err(err);
        }
    };
    let started_at = timestamp_string();
    persist_provider_running_record(
        &task_packet,
        provider_id,
        model,
        &workspace_binding,
        &task_packet_path,
        &runtime_record_path,
        &output_manifest_path,
        &trace_path,
        &runtime_identity_path,
        &workspace_binding_path,
        &input_mounts_path,
        &status_path,
        &started_at,
        &trace_id,
        &manifest_id,
        Some(child.id()),
        child_starttime(child.id()),
    )?;

    std::thread::spawn(move || {
        let _ = child.wait();
    });

    let runtime_record = load_runtime(data_dir, &agent_id)?;
    let output_manifest = load_output_manifest(data_dir, &agent_id)?;
    let trace = load_trace(data_dir, &agent_id)?;
    Ok(AgentStartResult {
        status: runtime_record.lifecycle_status.clone(),
        agent_id,
        task_packet_path: task_packet_path.display().to_string(),
        runtime_record_path: runtime_record_path.display().to_string(),
        output_manifest_path: output_manifest_path.display().to_string(),
        trace_path: trace_path.display().to_string(),
        workspace_binding: Some(workspace_binding),
        task_packet,
        runtime_record,
        output_manifest,
        trace,
    })
}

fn run_provider_from_persisted_packet(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: TaskPacket,
    provider_id: &str,
    model: &str,
) -> Result<AgentStartResult, AgentError> {
    if let Err(err) = validate_provider_task_packet_for_persisted_run(workspace_root, &task_packet)
    {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }
    let agent_id = task_packet.agent_id.clone();
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let workspace_binding_path = workspace_binding_path(data_dir, &agent_id);
    let workspace_binding: AgentWorkspaceBinding =
        serde_json::from_str(&fs::read_to_string(&workspace_binding_path)?)?;
    run_provider_worker_to_completion(
        data_dir,
        &task_packet,
        provider_id,
        model,
        workspace_binding,
        task_packet_path,
        workspace_binding_path,
    )
}

pub fn replay(
    data_dir: &Path,
    workspace_root: &Path,
    source_agent_id: &str,
) -> Result<AgentStartResult, AgentError> {
    let mut task_packet = load_task_packet(data_dir, source_agent_id)?;
    if task_packet.runner_kind.as_deref() != Some("local") {
        return Err(AgentError::InvalidTaskPacket {
            reason: "only local runner task packets can be replayed in M6".to_string(),
        });
    }
    let seed = timestamp_string();
    task_packet.task_packet_id = format!("task_replay_{seed}");
    task_packet.agent_id = format!("agent_replay_{seed}");
    task_packet.resume_policy = "replay".to_string();
    task_packet.replay_seed_ref = source_agent_id.to_string();
    task_packet.created_at = seed;
    task_packet.scope.workspace_root = workspace_root.display().to_string();
    start_local_from_packet(data_dir, workspace_root, task_packet)
}

pub fn read_task_packet_file(path: &Path) -> Result<TaskPacket, AgentError> {
    let contents = fs::read_to_string(path).map_err(|err| AgentError::InvalidTaskPacket {
        reason: err.to_string(),
    })?;
    serde_json::from_str(&contents).map_err(|err| AgentError::InvalidTaskPacket {
        reason: err.to_string(),
    })
}

pub fn write_main_agent_runtime_identity(
    data_dir: &Path,
    request: MainAgentRuntimeIdentityRequest,
) -> Result<AgentRuntimeIdentityWriteResult, AgentError> {
    let agent_id = format!("main_agent_{}", request.job_id);
    let runtime_id = format!(
        "{}_round_{}",
        sanitize_runtime_id_component(&agent_id),
        request.round_index
    );
    let root = agent_dir(data_dir, &agent_id).join("rounds");
    fs::create_dir_all(&root)?;
    let path = root.join(format!(
        "runtime_identity_round_{}.json",
        request.round_index
    ));
    let identity = AgentRuntimeIdentity {
        schema_version: "agent_runtime_identity.v1".to_string(),
        runtime_id,
        agent_id,
        runtime_kind: "main_agent".to_string(),
        role_kind: "main_agent".to_string(),
        role_profile: "autonomous_research_main_agent".to_string(),
        runner_kind: "provider_agent_loop".to_string(),
        authority_scope: "research_semantic_authority".to_string(),
        lifecycle_status: request.lifecycle_status,
        session_ref: Some(request.session_id),
        task_packet_ref: None,
        output_manifest_ref: None,
        provider_id: Some(request.provider_id),
        model: Some(request.model),
        trace_refs: Vec::new(),
        tool_scope: request.tool_scope,
        evidence_refs: vec![request.artifact_ref],
        created_at: request.created_at,
        updated_at: request.updated_at,
    };
    atomic_write(&path, &serde_json::to_string_pretty(&identity)?)?;
    Ok(AgentRuntimeIdentityWriteResult {
        runtime_identity_ref: path.display().to_string(),
        identity,
    })
}

pub fn start_local_from_packet(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: TaskPacket,
) -> Result<AgentStartResult, AgentError> {
    if let Err(err) = validate_local_task_packet(data_dir, workspace_root, &task_packet) {
        let _ = persist_local_start_failure(data_dir, &task_packet, &err);
        return Err(err);
    }
    if let Err(err) = ensure_source_git_repository(workspace_root) {
        let _ = persist_local_start_failure(data_dir, &task_packet, &err);
        return Err(err);
    }
    if let Err(err) = ensure_source_worktree_has_head(workspace_root) {
        let _ = persist_local_start_failure(data_dir, &task_packet, &err);
        return Err(err);
    }

    let created_at = task_packet.created_at.clone();
    let agent_id = task_packet.agent_id.clone();
    let seed = timestamp_string();
    let trace_id = format!("trace_{seed}");
    let manifest_id = format!("manifest_{seed}");

    let root = agent_dir(data_dir, &agent_id);
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let directive_path = directive_path(data_dir, &agent_id);
    let system_prompt_path = system_prompt_path(data_dir, &agent_id);
    let status_path = status_path(data_dir, &agent_id);
    let stdout_path = stdout_path(data_dir, &agent_id);
    let stderr_path = stderr_path(data_dir, &agent_id);
    let workspace_binding_path = workspace_binding_path(data_dir, &agent_id);
    let input_mounts_path = input_mounts_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;

    let workspace_binding = create_git_worktree(data_dir, workspace_root, &agent_id, &created_at)?;
    let _input_mounts = mount_task_input_artifacts(
        &agent_id,
        data_dir,
        workspace_root,
        &workspace_binding,
        &task_packet,
        &input_mounts_path,
        &created_at,
    )?;
    record_worker_workspace_baseline(data_dir, &workspace_binding, &task_packet, &created_at)?;
    let command = task_packet
        .command
        .clone()
        .expect("validated local task packet should include command");

    atomic_write(
        &task_packet_path,
        &serde_json::to_string_pretty(&task_packet)?,
    )?;
    atomic_write(
        &workspace_binding_path,
        &serde_json::to_string_pretty(&workspace_binding)?,
    )?;
    atomic_write(
        &directive_path,
        &render_directive(&task_packet, &created_at, &system_prompt_path),
    )?;
    atomic_write(
        &system_prompt_path,
        &render_worker_role_soul_system_context(workspace_root, &task_packet),
    )?;

    let started_at = timestamp_string();
    let command_output = run_local_command(
        &command,
        Path::new(&workspace_binding.worktree_path),
        task_packet.budget.max_runtime_ms,
    )?;
    let completed_at = timestamp_string();
    atomic_write(
        &stdout_path,
        &String::from_utf8_lossy(&command_output.stdout),
    )?;
    atomic_write(
        &stderr_path,
        &String::from_utf8_lossy(&command_output.stderr),
    )?;

    let succeeded = command_output
        .status
        .as_ref()
        .map(|status| status.success())
        .unwrap_or(false)
        && !command_output.timed_out;
    let failure_code = local_failure_code(&command_output);
    let lifecycle_status = if command_output.timed_out {
        "timeout"
    } else if succeeded {
        "succeeded"
    } else {
        "failed"
    };
    let manifest_status = if succeeded { "complete" } else { "failed" };
    let exit_reason = if succeeded {
        "task_complete"
    } else if command_output.timed_out {
        "timeout"
    } else if failure_code.starts_with("signal_") {
        "crash"
    } else {
        "error"
    };

    atomic_write(
        &status_path,
        &render_status(
            &task_packet,
            &started_at,
            &completed_at,
            lifecycle_status,
            exit_reason,
            &failure_code,
            &[
                stdout_path.display().to_string(),
                stderr_path.display().to_string(),
            ],
        ),
    )?;

    let mut output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id,
        status: manifest_status.to_string(),
        output_refs: vec![
            AgentOutputRef {
                kind: "stdout".to_string(),
                r#ref: stdout_path.display().to_string(),
                summary: "local runner stdout".to_string(),
            },
            AgentOutputRef {
                kind: "stderr".to_string(),
                r#ref: stderr_path.display().to_string(),
                summary: "local runner stderr".to_string(),
            },
            AgentOutputRef {
                kind: "status".to_string(),
                r#ref: status_path.display().to_string(),
                summary: "OpenAGS-style worker status".to_string(),
            },
        ],
        validation_status: "pending".to_string(),
        validation_errors: Vec::new(),
        created_at: completed_at.clone(),
    };
    output_manifest.validation_errors = validate_output_manifest_refs(&output_manifest);
    output_manifest.validation_status = if output_manifest.validation_errors.is_empty() {
        "valid".to_string()
    } else {
        "invalid".to_string()
    };
    let mut runtime_events = vec![
        trace_event(
            "agent_spawn_requested",
            "local runner requested",
            &created_at,
        ),
        trace_event("agent_ready", "git worktree bound", &created_at),
        trace_event(
            "agent_task_bound",
            "task packet and directive persisted",
            &created_at,
        ),
    ];
    if !task_packet.replay_seed_ref.trim().is_empty() {
        runtime_events.push(trace_event(
            "agent_replay_bound",
            &task_packet.replay_seed_ref,
            &created_at,
        ));
    }
    let terminal_event = if command_output.timed_out {
        "agent_timeout"
    } else if succeeded {
        "agent_completed"
    } else if failure_code.starts_with("signal_") {
        "agent_crashed"
    } else {
        "agent_failed"
    };
    runtime_events.push(trace_event(
        terminal_event,
        "local runner exited",
        &completed_at,
    ));
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id,
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events,
        tool_actions: vec![trace_event("shell_command", &command, &started_at)],
        permission_decisions: vec![trace_event(
            "write_authority",
            "workspace_write confined to agent worktree",
            &created_at,
        )],
        output_records: vec![
            trace_event(
                "input_artifacts_mounted",
                &input_mounts_path.display().to_string(),
                &created_at,
            ),
            trace_event(
                "stdout_written",
                &stdout_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "stderr_written",
                &stderr_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "status_written",
                &status_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "output_manifest_validated",
                &output_manifest.validation_status,
                &completed_at,
            ),
        ],
        final_status: lifecycle_status.to_string(),
        created_at: created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: Some("local".to_string()),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref: None,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: lifecycle_status.to_string(),
        created_at,
        updated_at: completed_at.clone(),
        heartbeat_at: completed_at.clone(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: Some(workspace_binding_path.display().to_string()),
        directive_ref: Some(directive_path.display().to_string()),
        status_ref: Some(status_path.display().to_string()),
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: String::new(),
        failure_code,
    };
    let mut runtime_identity = runtime_identity_for_task_packet(
        &task_packet,
        lifecycle_status,
        "worker_evidence_only",
        &runtime_record,
        Some(&output_manifest_path),
        &completed_at,
    );
    runtime_identity
        .evidence_refs
        .push(input_mounts_path.display().to_string());

    atomic_write(
        &runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        &output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(&trace_path, &serde_json::to_string_pretty(&trace)?)?;

    Ok(AgentStartResult {
        status: lifecycle_status.to_string(),
        agent_id,
        task_packet_path: task_packet_path.display().to_string(),
        runtime_record_path: runtime_record_path.display().to_string(),
        output_manifest_path: output_manifest_path.display().to_string(),
        trace_path: trace_path.display().to_string(),
        workspace_binding: Some(workspace_binding),
        task_packet,
        runtime_record,
        output_manifest,
        trace,
    })
}

fn start_provider_from_packet(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: TaskPacket,
    provider_id: &str,
    model: &str,
) -> Result<AgentStartResult, AgentError> {
    if let Err(err) = validate_provider_task_packet(data_dir, workspace_root, &task_packet) {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }
    if let Err(err) = ensure_source_git_repository(workspace_root) {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }
    if let Err(err) = ensure_source_worktree_has_head(workspace_root) {
        let _ = persist_provider_start_failure(data_dir, &task_packet, &err, provider_id, model);
        return Err(err);
    }

    let created_at = task_packet.created_at.clone();
    let agent_id = task_packet.agent_id.clone();
    let seed = timestamp_string();
    let trace_id = format!("trace_{seed}");
    let manifest_id = format!("manifest_{seed}");

    let root = agent_dir(data_dir, &agent_id);
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let tool_receipts_path = tool_receipts_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    let workspace_binding_path = workspace_binding_path(data_dir, &agent_id);
    let input_mounts_path = input_mounts_path(data_dir, &agent_id);
    let status_path = status_path(data_dir, &agent_id);
    let stdout_path = stdout_path(data_dir, &agent_id);
    let stderr_path = stderr_path(data_dir, &agent_id);
    let evidence_path = root.join("provider_worker_evidence.md");
    let worktree_candidates_path = worktree_candidates_path(data_dir, &agent_id);
    let worktree_patch_path = worktree_patch_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;

    let workspace_binding = create_git_worktree(data_dir, workspace_root, &agent_id, &created_at)?;
    let _input_mounts = mount_task_input_artifacts(
        &agent_id,
        data_dir,
        workspace_root,
        &workspace_binding,
        &task_packet,
        &input_mounts_path,
        &created_at,
    )?;
    record_worker_workspace_baseline(data_dir, &workspace_binding, &task_packet, &created_at)?;

    atomic_write(
        &task_packet_path,
        &serde_json::to_string_pretty(&task_packet)?,
    )?;
    atomic_write(
        &workspace_binding_path,
        &serde_json::to_string_pretty(&workspace_binding)?,
    )?;

    let started_at = timestamp_string();
    persist_provider_running_record(
        &task_packet,
        provider_id,
        model,
        &workspace_binding,
        &task_packet_path,
        &runtime_record_path,
        &output_manifest_path,
        &trace_path,
        &runtime_identity_path,
        &workspace_binding_path,
        &input_mounts_path,
        &status_path,
        &started_at,
        &trace_id,
        &manifest_id,
        None,
        None,
    )?;
    let heartbeat_guard = ProviderWorkerHeartbeatGuard::start(
        data_dir.to_path_buf(),
        agent_id.clone(),
        task_packet.clone(),
        provider_id.to_string(),
        model.to_string(),
        started_at.clone(),
        None,
        None,
    );
    let provider_result = run_provider_worker(
        data_dir,
        Path::new(&workspace_binding.worktree_path),
        &task_packet,
        provider_id,
        model,
    );
    heartbeat_guard.stop();
    let completed_at = timestamp_string();
    let (
        worker_output,
        mut stderr_text,
        provider_succeeded,
        mut failure_code,
        session_ref,
        tool_scope,
        iterations,
        tool_calls_made,
        finish_reason,
        provider_tool_actions,
        tool_receipts,
    ) = match provider_result {
        Ok(run) => (
            run.output,
            String::new(),
            true,
            String::new(),
            Some(run.session_id),
            run.tool_scope,
            run.iterations,
            run.tool_calls_made,
            run.finish_reason,
            run.tool_actions,
            run.tool_receipts,
        ),
        Err(failure) => {
            let failure_code = agent_error_failure_code(&failure.error);
            (
                String::new(),
                failure.error.to_string(),
                false,
                failure_code,
                None,
                provider_worker_tool_scope(&task_packet),
                failure.iterations,
                failure.tool_calls_made,
                "failed".to_string(),
                failure.tool_actions,
                failure
                    .tool_receipts
                    .unwrap_or_else(|| provider_worker_empty_tool_receipts(&agent_id, None)),
            )
        }
    };
    let worktree_candidates = capture_worktree_artifact_candidates(
        &agent_id,
        &task_packet,
        &workspace_binding,
        &workspace_binding_path,
        &worktree_candidates_path,
        &worktree_candidate_archive_dir(data_dir, &agent_id),
        &worktree_patch_path,
        &completed_at,
    )?;
    let mut validation_errors = if provider_succeeded {
        validate_provider_worker_final_output(&task_packet, &worker_output, &worktree_candidates)
    } else {
        vec![stderr_text.clone()]
    };
    if provider_succeeded {
        validation_errors.extend(validate_provider_worker_tool_action_delivery(
            &task_packet,
            &provider_tool_actions,
            &worktree_candidates,
        ));
    }
    validation_errors.retain(|error| !error.trim().is_empty());
    let recovered_candidate_output =
        !provider_succeeded && worktree_candidate_manifest_has_safe_file(&worktree_candidates);
    let output_valid =
        (provider_succeeded && validation_errors.is_empty()) || recovered_candidate_output;
    if provider_succeeded && !output_valid {
        stderr_text = validation_errors.join("\n");
        failure_code = "invalid_worker_output".to_string();
    } else if recovered_candidate_output {
        failure_code = "provider_finalize_failed_with_candidate_artifacts".to_string();
    }
    let evidence_text = if provider_succeeded {
        render_provider_worker_evidence(
            &task_packet,
            provider_id,
            model,
            &worker_output,
            &validation_errors,
            &worktree_candidates,
            tool_calls_made,
            Some(&tool_receipts),
        )
    } else if recovered_candidate_output {
        render_provider_worker_partial_candidate_evidence(
            &task_packet,
            provider_id,
            model,
            &stderr_text,
            &worktree_candidates,
        )
    } else {
        format!(
            "# Provider Worker Failure\n\n- provider: `{provider_id}`\n- model: `{model}`\n- status: `failed`\n- failure: {}\n",
            stderr_text
        )
    };
    atomic_write(&stdout_path, &worker_output)?;
    atomic_write(&stderr_path, &stderr_text)?;
    atomic_write(&evidence_path, &evidence_text)?;
    atomic_write(
        &tool_receipts_path,
        &serde_json::to_string_pretty(&tool_receipts)?,
    )?;

    let lifecycle_status = if output_valid { "succeeded" } else { "failed" };
    let manifest_status = if output_valid { "complete" } else { "failed" };
    let validation_status = if output_valid { "valid" } else { "invalid" };
    if recovered_candidate_output {
        validation_errors.clear();
    }
    let mut output_refs = vec![
        AgentOutputRef {
            kind: "provider_worker_evidence".to_string(),
            r#ref: evidence_path.display().to_string(),
            summary: "provider-backed delegated worker evidence".to_string(),
        },
        AgentOutputRef {
            kind: "worker_worktree_artifact_candidates".to_string(),
            r#ref: worktree_candidates_path.display().to_string(),
            summary: "candidate-only files and paths produced in the worker worktree".to_string(),
        },
        AgentOutputRef {
            kind: "provider_worker_tool_receipts".to_string(),
            r#ref: tool_receipts_path.display().to_string(),
            summary: "runtime-generated worker tool call receipts and output snapshots".to_string(),
        },
    ];
    if let Some(patch_ref) = worktree_candidates.patch_ref.as_ref() {
        output_refs.push(AgentOutputRef {
            kind: "worker_worktree_patch".to_string(),
            r#ref: patch_ref.clone(),
            summary: "candidate-only git diff from the worker worktree".to_string(),
        });
    }
    output_refs.extend([
        AgentOutputRef {
            kind: "provider_worker_trace".to_string(),
            r#ref: trace_path.display().to_string(),
            summary: "provider worker runtime trace with per-tool audit events".to_string(),
        },
        AgentOutputRef {
            kind: "stdout".to_string(),
            r#ref: stdout_path.display().to_string(),
            summary: "provider worker stdout".to_string(),
        },
        AgentOutputRef {
            kind: "stderr".to_string(),
            r#ref: stderr_path.display().to_string(),
            summary: "provider worker stderr".to_string(),
        },
    ]);
    let output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id,
        status: manifest_status.to_string(),
        output_refs,
        validation_status: validation_status.to_string(),
        validation_errors,
        created_at: completed_at.clone(),
    };
    let mut tool_actions = provider_tool_actions;
    if tool_actions.is_empty() {
        tool_actions.push(trace_event(
            "provider_agent_loop",
            &format!("worker tool calls: {tool_calls_made}"),
            &completed_at,
        ));
    } else {
        tool_actions.push(trace_event(
            "provider_agent_loop_summary",
            &format!("worker tool calls: {tool_calls_made}"),
            &completed_at,
        ));
    }
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id,
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events: vec![
            trace_event(
                "agent_spawn_requested",
                "provider runner requested",
                &created_at,
            ),
            trace_event(
                "agent_provider_bound",
                &format!("{provider_id}/{model}"),
                &started_at,
            ),
            trace_event(
                "agent_ready",
                "provider worker git worktree bound",
                &created_at,
            ),
            trace_event(
                "input_artifacts_mounted",
                &input_mounts_path.display().to_string(),
                &created_at,
            ),
            trace_event(
                if output_valid {
                    "agent_completed"
                } else {
                    "agent_failed"
                },
                "provider runner exited",
                &completed_at,
            ),
            trace_event(
                "provider_worker_agent_loop_finished",
                &format!(
                    "iterations={iterations}; tool_calls_made={tool_calls_made}; finish_reason={finish_reason}"
                ),
                &completed_at,
            ),
        ],
        tool_actions,
        permission_decisions: vec![trace_event(
            "write_authority",
            "workspace_write confined to provider agent worktree",
            &created_at,
        )],
        output_records: vec![
            trace_event(
                "provider_worker_evidence_recorded",
                &evidence_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "worker_worktree_artifact_candidates_recorded",
                &worktree_candidates_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "provider_worker_tool_receipts_recorded",
                &tool_receipts_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "output_manifest_validated",
                validation_status,
                &completed_at,
            ),
        ],
        final_status: lifecycle_status.to_string(),
        created_at: created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: Some("provider".to_string()),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: lifecycle_status.to_string(),
        created_at: created_at.clone(),
        updated_at: completed_at.clone(),
        heartbeat_at: completed_at.clone(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: Some(workspace_binding_path.display().to_string()),
        directive_ref: None,
        status_ref: Some(status_path.display().to_string()),
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: String::new(),
        failure_code,
    };
    let mut runtime_identity = runtime_identity_for_task_packet(
        &task_packet,
        lifecycle_status,
        "worker_evidence_only",
        &runtime_record,
        Some(&output_manifest_path),
        &completed_at,
    );
    runtime_identity.runner_kind = "provider_agent_worker".to_string();
    runtime_identity.provider_id = Some(provider_id.to_string());
    runtime_identity.model = Some(model.to_string());
    runtime_identity.tool_scope = tool_scope;
    runtime_identity
        .evidence_refs
        .push(evidence_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(input_mounts_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(worktree_candidates_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(tool_receipts_path.display().to_string());
    if let Some(patch_ref) = worktree_candidates.patch_ref.as_ref() {
        runtime_identity.evidence_refs.push(patch_ref.clone());
    }

    atomic_write(
        &runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        &output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(&trace_path, &serde_json::to_string_pretty(&trace)?)?;
    atomic_write(
        &status_path,
        &render_status(
            &task_packet,
            &created_at,
            &completed_at,
            lifecycle_status,
            "provider runner exited",
            if output_valid {
                ""
            } else {
                &runtime_record.failure_code
            },
            &[
                runtime_record_path.display().to_string(),
                output_manifest_path.display().to_string(),
                tool_receipts_path.display().to_string(),
                trace_path.display().to_string(),
                stdout_path.display().to_string(),
                stderr_path.display().to_string(),
            ],
        ),
    )?;

    Ok(AgentStartResult {
        status: lifecycle_status.to_string(),
        agent_id,
        task_packet_path: task_packet_path.display().to_string(),
        runtime_record_path: runtime_record_path.display().to_string(),
        output_manifest_path: output_manifest_path.display().to_string(),
        trace_path: trace_path.display().to_string(),
        workspace_binding: Some(workspace_binding),
        task_packet,
        runtime_record,
        output_manifest,
        trace,
    })
}

fn run_provider_worker_to_completion(
    data_dir: &Path,
    task_packet: &TaskPacket,
    provider_id: &str,
    model: &str,
    workspace_binding: AgentWorkspaceBinding,
    task_packet_path: PathBuf,
    workspace_binding_path: PathBuf,
) -> Result<AgentStartResult, AgentError> {
    let created_at = task_packet.created_at.clone();
    let agent_id = task_packet.agent_id.clone();
    let seed = timestamp_string();
    let trace_id = format!("trace_{seed}");
    let manifest_id = format!("manifest_{seed}");

    let root = agent_dir(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let tool_receipts_path = tool_receipts_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    let status_path = status_path(data_dir, &agent_id);
    let input_mounts_path = input_mounts_path(data_dir, &agent_id);
    let stdout_path = stdout_path(data_dir, &agent_id);
    let stderr_path = stderr_path(data_dir, &agent_id);
    let evidence_path = root.join("provider_worker_evidence.md");
    let worktree_candidates_path = worktree_candidates_path(data_dir, &agent_id);
    let worktree_patch_path = worktree_patch_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;

    let heartbeat_guard = ProviderWorkerHeartbeatGuard::start(
        data_dir.to_path_buf(),
        agent_id.clone(),
        task_packet.clone(),
        provider_id.to_string(),
        model.to_string(),
        created_at.clone(),
        Some(std::process::id()),
        child_starttime(std::process::id()),
    );
    let provider_result = run_provider_worker(
        data_dir,
        Path::new(&workspace_binding.worktree_path),
        task_packet,
        provider_id,
        model,
    );
    heartbeat_guard.stop();
    let completed_at = timestamp_string();
    let (
        worker_output,
        mut stderr_text,
        provider_succeeded,
        mut failure_code,
        session_ref,
        tool_scope,
        iterations,
        tool_calls_made,
        finish_reason,
        provider_tool_actions,
        tool_receipts,
    ) = match provider_result {
        Ok(run) => (
            run.output,
            String::new(),
            true,
            String::new(),
            Some(run.session_id),
            run.tool_scope,
            run.iterations,
            run.tool_calls_made,
            run.finish_reason,
            run.tool_actions,
            run.tool_receipts,
        ),
        Err(failure) => {
            let failure_code = agent_error_failure_code(&failure.error);
            (
                String::new(),
                failure.error.to_string(),
                false,
                failure_code,
                None,
                provider_worker_tool_scope(task_packet),
                failure.iterations,
                failure.tool_calls_made,
                "failed".to_string(),
                failure.tool_actions,
                failure
                    .tool_receipts
                    .unwrap_or_else(|| provider_worker_empty_tool_receipts(&agent_id, None)),
            )
        }
    };
    let worktree_candidates = capture_worktree_artifact_candidates(
        &agent_id,
        task_packet,
        &workspace_binding,
        &workspace_binding_path,
        &worktree_candidates_path,
        &worktree_candidate_archive_dir(data_dir, &agent_id),
        &worktree_patch_path,
        &completed_at,
    )?;
    let mut validation_errors = if provider_succeeded {
        validate_provider_worker_final_output(task_packet, &worker_output, &worktree_candidates)
    } else {
        vec![stderr_text.clone()]
    };
    if provider_succeeded {
        validation_errors.extend(validate_provider_worker_tool_action_delivery(
            task_packet,
            &provider_tool_actions,
            &worktree_candidates,
        ));
    }
    validation_errors.retain(|error| !error.trim().is_empty());
    let recovered_candidate_output =
        !provider_succeeded && worktree_candidate_manifest_has_safe_file(&worktree_candidates);
    let output_valid =
        (provider_succeeded && validation_errors.is_empty()) || recovered_candidate_output;
    if provider_succeeded && !output_valid {
        stderr_text = validation_errors.join("\n");
        failure_code = "invalid_worker_output".to_string();
    } else if recovered_candidate_output {
        failure_code = "provider_finalize_failed_with_candidate_artifacts".to_string();
    }
    let evidence_text = if provider_succeeded {
        render_provider_worker_evidence(
            task_packet,
            provider_id,
            model,
            &worker_output,
            &validation_errors,
            &worktree_candidates,
            tool_calls_made,
            Some(&tool_receipts),
        )
    } else if recovered_candidate_output {
        render_provider_worker_partial_candidate_evidence(
            task_packet,
            provider_id,
            model,
            &stderr_text,
            &worktree_candidates,
        )
    } else {
        format!(
            "# Provider Worker Failure\n\n- provider: `{provider_id}`\n- model: `{model}`\n- status: `failed`\n- failure: {}\n",
            stderr_text
        )
    };
    atomic_write(&stdout_path, &worker_output)?;
    atomic_write(&stderr_path, &stderr_text)?;
    atomic_write(&evidence_path, &evidence_text)?;
    atomic_write(
        &tool_receipts_path,
        &serde_json::to_string_pretty(&tool_receipts)?,
    )?;

    let lifecycle_status = if output_valid { "succeeded" } else { "failed" };
    let manifest_status = if output_valid { "complete" } else { "failed" };
    let validation_status = if output_valid { "valid" } else { "invalid" };
    if recovered_candidate_output {
        validation_errors.clear();
    }
    let mut output_refs = vec![
        AgentOutputRef {
            kind: "provider_worker_evidence".to_string(),
            r#ref: evidence_path.display().to_string(),
            summary: "provider-backed delegated worker evidence".to_string(),
        },
        AgentOutputRef {
            kind: "worker_worktree_artifact_candidates".to_string(),
            r#ref: worktree_candidates_path.display().to_string(),
            summary: "candidate-only files and paths produced in the worker worktree".to_string(),
        },
        AgentOutputRef {
            kind: "provider_worker_tool_receipts".to_string(),
            r#ref: tool_receipts_path.display().to_string(),
            summary: "runtime-generated worker tool call receipts and output snapshots".to_string(),
        },
    ];
    if let Some(patch_ref) = worktree_candidates.patch_ref.as_ref() {
        output_refs.push(AgentOutputRef {
            kind: "worker_worktree_patch".to_string(),
            r#ref: patch_ref.clone(),
            summary: "candidate-only git diff from the worker worktree".to_string(),
        });
    }
    output_refs.extend([
        AgentOutputRef {
            kind: "provider_worker_trace".to_string(),
            r#ref: trace_path.display().to_string(),
            summary: "provider worker runtime trace with per-tool audit events".to_string(),
        },
        AgentOutputRef {
            kind: "stdout".to_string(),
            r#ref: stdout_path.display().to_string(),
            summary: "provider worker stdout".to_string(),
        },
        AgentOutputRef {
            kind: "stderr".to_string(),
            r#ref: stderr_path.display().to_string(),
            summary: "provider worker stderr".to_string(),
        },
    ]);
    let output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id,
        status: manifest_status.to_string(),
        output_refs,
        validation_status: validation_status.to_string(),
        validation_errors,
        created_at: completed_at.clone(),
    };
    let mut tool_actions = provider_tool_actions;
    if tool_actions.is_empty() {
        tool_actions.push(trace_event(
            "provider_agent_loop",
            &format!("worker tool calls: {tool_calls_made}"),
            &completed_at,
        ));
    } else {
        tool_actions.push(trace_event(
            "provider_agent_loop_summary",
            &format!("worker tool calls: {tool_calls_made}"),
            &completed_at,
        ));
    }
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id,
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events: vec![
            trace_event(
                "agent_spawn_requested",
                "provider runner requested",
                &created_at,
            ),
            trace_event(
                "agent_provider_bound",
                &format!("{provider_id}/{model}"),
                &created_at,
            ),
            trace_event(
                "agent_ready",
                "provider worker git worktree bound",
                &created_at,
            ),
            trace_event(
                "input_artifacts_mounted",
                &input_mounts_path.display().to_string(),
                &created_at,
            ),
            trace_event(
                if output_valid {
                    "agent_completed"
                } else {
                    "agent_failed"
                },
                "provider runner exited",
                &completed_at,
            ),
            trace_event(
                "provider_worker_agent_loop_finished",
                &format!(
                    "iterations={iterations}; tool_calls_made={tool_calls_made}; finish_reason={finish_reason}"
                ),
                &completed_at,
            ),
        ],
        tool_actions,
        permission_decisions: vec![trace_event(
            "write_authority",
            "workspace_write confined to provider agent worktree",
            &created_at,
        )],
        output_records: vec![
            trace_event(
                "provider_worker_evidence_recorded",
                &evidence_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "worker_worktree_artifact_candidates_recorded",
                &worktree_candidates_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "provider_worker_tool_receipts_recorded",
                &tool_receipts_path.display().to_string(),
                &completed_at,
            ),
            trace_event(
                "output_manifest_validated",
                validation_status,
                &completed_at,
            ),
        ],
        final_status: lifecycle_status.to_string(),
        created_at: created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: Some("provider".to_string()),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: lifecycle_status.to_string(),
        created_at: created_at.clone(),
        updated_at: completed_at.clone(),
        heartbeat_at: completed_at.clone(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: Some(workspace_binding_path.display().to_string()),
        directive_ref: None,
        status_ref: Some(status_path.display().to_string()),
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: String::new(),
        failure_code,
    };
    let mut runtime_identity = runtime_identity_for_task_packet(
        task_packet,
        lifecycle_status,
        "worker_evidence_only",
        &runtime_record,
        Some(&output_manifest_path),
        &completed_at,
    );
    runtime_identity.runner_kind = "provider_agent_worker".to_string();
    runtime_identity.provider_id = Some(provider_id.to_string());
    runtime_identity.model = Some(model.to_string());
    runtime_identity.tool_scope = tool_scope;
    runtime_identity
        .evidence_refs
        .push(evidence_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(input_mounts_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(worktree_candidates_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(tool_receipts_path.display().to_string());
    if let Some(patch_ref) = worktree_candidates.patch_ref.as_ref() {
        runtime_identity.evidence_refs.push(patch_ref.clone());
    }

    atomic_write(
        &runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        &output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(&trace_path, &serde_json::to_string_pretty(&trace)?)?;
    atomic_write(
        &status_path,
        &render_status(
            task_packet,
            &created_at,
            &completed_at,
            lifecycle_status,
            "provider runner exited",
            if output_valid {
                ""
            } else {
                &runtime_record.failure_code
            },
            &[
                runtime_record_path.display().to_string(),
                output_manifest_path.display().to_string(),
                tool_receipts_path.display().to_string(),
                trace_path.display().to_string(),
                stdout_path.display().to_string(),
                stderr_path.display().to_string(),
            ],
        ),
    )?;

    Ok(AgentStartResult {
        status: lifecycle_status.to_string(),
        agent_id,
        task_packet_path: task_packet_path.display().to_string(),
        runtime_record_path: runtime_record_path.display().to_string(),
        output_manifest_path: output_manifest_path.display().to_string(),
        trace_path: trace_path.display().to_string(),
        workspace_binding: Some(workspace_binding),
        task_packet: task_packet.clone(),
        runtime_record,
        output_manifest,
        trace,
    })
}

fn persist_provider_running_record(
    task_packet: &TaskPacket,
    provider_id: &str,
    model: &str,
    workspace_binding: &AgentWorkspaceBinding,
    task_packet_path: &Path,
    runtime_record_path: &Path,
    output_manifest_path: &Path,
    trace_path: &Path,
    runtime_identity_path: &Path,
    workspace_binding_path: &Path,
    input_mounts_path: &Path,
    status_path: &Path,
    started_at: &str,
    trace_id: &str,
    manifest_id: &str,
    supervisor_pid: Option<u32>,
    supervisor_starttime: Option<u128>,
) -> Result<(), AgentError> {
    let agent_id = task_packet.agent_id.clone();
    let output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id: manifest_id.to_string(),
        status: "running".to_string(),
        output_refs: vec![
            AgentOutputRef {
                kind: "provider_worker_pending".to_string(),
                r#ref: input_mounts_path.display().to_string(),
                summary:
                    "provider-backed delegated worker is running; final evidence not recorded yet"
                        .to_string(),
            },
            AgentOutputRef {
                kind: "status".to_string(),
                r#ref: status_path.display().to_string(),
                summary: "provider worker running status and supervisor process identity"
                    .to_string(),
            },
        ],
        validation_status: "pending".to_string(),
        validation_errors: Vec::new(),
        created_at: started_at.to_string(),
    };
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id: trace_id.to_string(),
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events: vec![
            trace_event(
                "agent_spawn_requested",
                "provider runner requested",
                &task_packet.created_at,
            ),
            trace_event(
                "agent_provider_bound",
                &format!("{provider_id}/{model}"),
                started_at,
            ),
            trace_event(
                "agent_ready",
                "provider worker git worktree bound",
                started_at,
            ),
            trace_event(
                "input_artifacts_mounted",
                &input_mounts_path.display().to_string(),
                started_at,
            ),
            trace_event(
                "provider_worker_supervisor_bound",
                &format!(
                    "pid={} starttime={}",
                    supervisor_pid.unwrap_or(0),
                    supervisor_starttime
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ),
                started_at,
            ),
            trace_event("provider_worker_running", "agent loop started", started_at),
        ],
        tool_actions: Vec::new(),
        permission_decisions: vec![trace_event(
            "write_authority",
            "workspace_write confined to provider agent worktree",
            started_at,
        )],
        output_records: vec![trace_event(
            "output_manifest_recorded",
            &output_manifest_path.display().to_string(),
            started_at,
        )],
        final_status: "running".to_string(),
        created_at: task_packet.created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: Some("provider".to_string()),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref: None,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: "running".to_string(),
        created_at: task_packet.created_at.clone(),
        updated_at: started_at.to_string(),
        heartbeat_at: started_at.to_string(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: Some(workspace_binding_path.display().to_string()),
        directive_ref: None,
        status_ref: Some(status_path.display().to_string()),
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: String::new(),
        failure_code: String::new(),
    };
    let mut runtime_identity = runtime_identity_for_task_packet(
        task_packet,
        "running",
        "worker_evidence_only",
        &runtime_record,
        Some(output_manifest_path),
        started_at,
    );
    runtime_identity.runner_kind = "provider_agent_worker".to_string();
    runtime_identity.provider_id = Some(provider_id.to_string());
    runtime_identity.model = Some(model.to_string());
    runtime_identity.tool_scope = provider_worker_tool_scope(task_packet);
    runtime_identity
        .evidence_refs
        .push(input_mounts_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(status_path.display().to_string());
    runtime_identity
        .evidence_refs
        .push(workspace_binding.worktree_path.clone());

    atomic_write(
        status_path,
        &render_provider_running_status(
            task_packet,
            provider_id,
            model,
            started_at,
            started_at,
            supervisor_pid.unwrap_or(0),
            supervisor_starttime,
        ),
    )?;
    atomic_write(
        runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(trace_path, &serde_json::to_string_pretty(&trace)?)?;
    Ok(())
}

fn render_provider_running_status(
    packet: &TaskPacket,
    provider_id: &str,
    model: &str,
    started_at: &str,
    heartbeat_at: &str,
    supervisor_pid: u32,
    supervisor_starttime: Option<u128>,
) -> String {
    let supervisor_starttime = supervisor_starttime
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "---\nagent: \"{}\"\nstatus: \"running\"\nrunner_kind: \"provider_agent_worker\"\nprovider_id: \"{}\"\nmodel: \"{}\"\nsupervisor_pid: {}\nsupervisor_starttime: \"{}\"\nstarted_at: \"{}\"\nheartbeat_at: \"{}\"\n---\n\nProvider-backed delegated worker is running under the recorded supervisor process.\n",
        packet.agent_id,
        provider_id,
        model,
        supervisor_pid,
        supervisor_starttime,
        started_at,
        heartbeat_at
    )
}

struct ProviderWorkerHeartbeatGuard {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProviderWorkerHeartbeatGuard {
    fn start(
        data_dir: PathBuf,
        agent_id: String,
        task_packet: TaskPacket,
        provider_id: String,
        model: String,
        started_at: String,
        supervisor_pid: Option<u32>,
        supervisor_starttime: Option<u128>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || loop {
            for _ in 0..PROVIDER_WORKER_HEARTBEAT_INTERVAL.as_secs().max(1) {
                if worker_stop.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_secs(1));
            }
            if worker_stop.load(Ordering::Relaxed) {
                return;
            }
            if refresh_provider_worker_heartbeat(
                &data_dir,
                &agent_id,
                &task_packet,
                &provider_id,
                &model,
                &started_at,
                supervisor_pid,
                supervisor_starttime,
            )
            .ok()
                != Some(true)
            {
                return;
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn refresh_provider_worker_heartbeat(
    data_dir: &Path,
    agent_id: &str,
    task_packet: &TaskPacket,
    provider_id: &str,
    model: &str,
    started_at: &str,
    supervisor_pid: Option<u32>,
    supervisor_starttime: Option<u128>,
) -> Result<bool, AgentError> {
    let mut runtime_record = load_runtime(data_dir, agent_id)?;
    if runtime_record.lifecycle_status != "running" {
        return Ok(false);
    }
    let heartbeat_at = timestamp_string();
    runtime_record.heartbeat_at = heartbeat_at.clone();
    atomic_write(
        &runtime_path(data_dir, agent_id),
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;

    if let Ok(mut identity) = load_runtime_identity(data_dir, agent_id) {
        if identity.lifecycle_status == "running" {
            identity.updated_at = heartbeat_at.clone();
            atomic_write(
                &runtime_identity_path(data_dir, agent_id),
                &serde_json::to_string_pretty(&identity)?,
            )?;
        }
    }

    if let Some(status_ref) = runtime_record.status_ref.as_deref() {
        atomic_write(
            Path::new(status_ref),
            &render_provider_running_status(
                task_packet,
                provider_id,
                model,
                started_at,
                &heartbeat_at,
                supervisor_pid.unwrap_or(0),
                supervisor_starttime,
            ),
        )?;
    }
    Ok(true)
}

fn spawn_provider_worker_process(
    workspace_root: &Path,
    task_packet_path: &Path,
    provider_id: &str,
    model: &str,
    log_path: &Path,
) -> Result<std::process::Child, AgentError> {
    let exe = crate::process_bootstrap::current_cli_executable();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let stderr = stdout.try_clone()?;
    let mut command = Command::new(&exe);
    let task_packet_arg = task_packet_path
        .strip_prefix(workspace_root)
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|_| task_packet_path.to_path_buf());
    command
        .current_dir(workspace_root)
        .args(["agents", "start", "--runner", "provider", "--task-packet"])
        .arg(task_packet_arg)
        .args(["--provider", provider_id, "--model", model, "--json"])
        .stdout(stdout)
        .stderr(stderr);
    crate::process_bootstrap::propagate_current_cli_executable(&mut command, &exe);
    configure_detached_provider_process(&mut command);
    command
        .spawn()
        .map_err(|err| AgentError::ProviderExecution(err.to_string()))
}

#[cfg(unix)]
fn configure_detached_provider_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_detached_provider_process(_command: &mut Command) {}

fn persist_local_start_failure(
    data_dir: &Path,
    task_packet: &TaskPacket,
    err: &AgentError,
) -> Result<(), AgentError> {
    if task_packet.agent_id.trim().is_empty() || agent_dir(data_dir, &task_packet.agent_id).exists()
    {
        return Ok(());
    }
    let created_at = if task_packet.created_at.trim().is_empty() {
        timestamp_string()
    } else {
        task_packet.created_at.clone()
    };
    let updated_at = timestamp_string();
    let agent_id = task_packet.agent_id.clone();
    let trace_id = format!("trace_{}", updated_at);
    let manifest_id = format!("manifest_{}", updated_at);
    let failure_code = agent_error_failure_code(err);
    let failure_detail = err.to_string();

    let root = agent_dir(data_dir, &agent_id);
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;

    let output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id,
        status: "failed".to_string(),
        output_refs: vec![AgentOutputRef {
            kind: "early_start_failure".to_string(),
            r#ref: trace_path.display().to_string(),
            summary: failure_detail.clone(),
        }],
        validation_status: "invalid".to_string(),
        validation_errors: vec![failure_detail.clone()],
        created_at: updated_at.clone(),
    };
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id,
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events: vec![
            trace_event(
                "agent_spawn_requested",
                "local runner requested",
                &created_at,
            ),
            trace_event("agent_launch_failed", &failure_detail, &updated_at),
        ],
        tool_actions: Vec::new(),
        permission_decisions: Vec::new(),
        output_records: vec![trace_event(
            "output_manifest_recorded",
            &output_manifest_path.display().to_string(),
            &updated_at,
        )],
        final_status: "launch_failed".to_string(),
        created_at: created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: task_packet.runner_kind.clone(),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref: None,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: "launch_failed".to_string(),
        created_at,
        updated_at: updated_at.clone(),
        heartbeat_at: updated_at.clone(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: None,
        directive_ref: None,
        status_ref: None,
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: "early_start_failure".to_string(),
        failure_code,
    };
    let runtime_identity = runtime_identity_for_task_packet(
        task_packet,
        "launch_failed",
        "worker_evidence_only",
        &runtime_record,
        Some(&output_manifest_path),
        &updated_at,
    );

    atomic_write(
        &task_packet_path,
        &serde_json::to_string_pretty(task_packet)?,
    )?;
    atomic_write(
        &runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        &output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(&trace_path, &serde_json::to_string_pretty(&trace)?)?;
    Ok(())
}

fn persist_provider_start_failure(
    data_dir: &Path,
    task_packet: &TaskPacket,
    err: &AgentError,
    provider_id: &str,
    model: &str,
) -> Result<(), AgentError> {
    if task_packet.agent_id.trim().is_empty() {
        return Ok(());
    }
    if runtime_path(data_dir, &task_packet.agent_id).exists() {
        return Ok(());
    }
    let created_at = if task_packet.created_at.trim().is_empty() {
        timestamp_string()
    } else {
        task_packet.created_at.clone()
    };
    let updated_at = timestamp_string();
    let agent_id = task_packet.agent_id.clone();
    let trace_id = format!("trace_{}", updated_at);
    let manifest_id = format!("manifest_{}", updated_at);
    let failure_code = agent_error_failure_code(err);
    let failure_detail = err.to_string();

    let root = agent_dir(data_dir, &agent_id);
    let task_packet_path = task_packet_path(data_dir, &agent_id);
    let runtime_record_path = runtime_path(data_dir, &agent_id);
    let output_manifest_path = output_manifest_path(data_dir, &agent_id);
    let trace_path = trace_path(data_dir, &agent_id);
    let runtime_identity_path = runtime_identity_path(data_dir, &agent_id);
    let stderr_path = stderr_path(data_dir, &agent_id);
    fs::create_dir_all(root.join("traces"))?;
    atomic_write(&stderr_path, &failure_detail)?;

    let output_manifest = AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        manifest_id,
        status: "failed".to_string(),
        output_refs: vec![AgentOutputRef {
            kind: "provider_start_failure".to_string(),
            r#ref: stderr_path.display().to_string(),
            summary: failure_detail.clone(),
        }],
        validation_status: "invalid".to_string(),
        validation_errors: vec![failure_detail.clone()],
        created_at: updated_at.clone(),
    };
    let trace = AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        trace_id,
        task_packet_ref: task_packet_path.display().to_string(),
        runtime_events: vec![
            trace_event(
                "agent_spawn_requested",
                "provider runner requested",
                &created_at,
            ),
            trace_event("agent_launch_failed", &failure_detail, &updated_at),
        ],
        tool_actions: Vec::new(),
        permission_decisions: Vec::new(),
        output_records: vec![trace_event(
            "output_manifest_recorded",
            &output_manifest_path.display().to_string(),
            &updated_at,
        )],
        final_status: "launch_failed".to_string(),
        created_at: created_at.clone(),
        trace_path: trace_path.display().to_string(),
    };
    let runtime_record = AgentRuntimeRecord {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.clone(),
        runner_kind: Some("provider".to_string()),
        runtime_identity_ref: Some(runtime_identity_path.display().to_string()),
        role_kind: Some(role_kind_from_profile(&task_packet.role_profile).to_string()),
        authority_scope: Some("worker_evidence_only".to_string()),
        session_ref: None,
        task_packet_ref: task_packet_path.display().to_string(),
        lifecycle_status: "launch_failed".to_string(),
        created_at,
        updated_at: updated_at.clone(),
        heartbeat_at: updated_at.clone(),
        output_manifest_ref: output_manifest_path.display().to_string(),
        workspace_binding_ref: None,
        directive_ref: None,
        status_ref: None,
        trace_refs: vec![trace_path.display().to_string()],
        stop_reason: "early_start_failure".to_string(),
        failure_code,
    };
    let mut runtime_identity = runtime_identity_for_task_packet(
        task_packet,
        "launch_failed",
        "worker_evidence_only",
        &runtime_record,
        Some(&output_manifest_path),
        &updated_at,
    );
    runtime_identity.runner_kind = "provider_agent_worker".to_string();
    runtime_identity.provider_id = Some(provider_id.to_string());
    runtime_identity.model = Some(model.to_string());
    runtime_identity.tool_scope = provider_worker_tool_scope(task_packet);

    atomic_write(
        &task_packet_path,
        &serde_json::to_string_pretty(task_packet)?,
    )?;
    atomic_write(
        &runtime_record_path,
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &runtime_identity_path,
        &serde_json::to_string_pretty(&runtime_identity)?,
    )?;
    atomic_write(
        &output_manifest_path,
        &serde_json::to_string_pretty(&output_manifest)?,
    )?;
    atomic_write(&trace_path, &serde_json::to_string_pretty(&trace)?)?;
    Ok(())
}

fn agent_error_failure_code(err: &AgentError) -> String {
    match err {
        AgentError::InvalidTaskPacket { .. } => "invalid_task_packet".to_string(),
        AgentError::StaleScope { .. } => "stale_scope".to_string(),
        AgentError::MissingWriteAuthority { .. } => "missing_write_authority".to_string(),
        AgentError::DirtySourceWorktree { .. } => "dirty_source_worktree_forbidden".to_string(),
        AgentError::GitCommand { .. } => "git_command_failed".to_string(),
        AgentError::LocalCommand(_) => "local_command_failed".to_string(),
        AgentError::ProviderExecution(_) => "provider_execution_failed".to_string(),
        AgentError::AgentLoopExhausted(_) => "agent_loop_exhausted".to_string(),
        AgentError::Io(_) | AgentError::IoContext { .. } => "io_error".to_string(),
        AgentError::Serde(_) => "serde_error".to_string(),
        AgentError::UnknownAgent(_) => "unknown_agent".to_string(),
    }
}

fn agent_error_from_worker_loop_error(
    err: crate::runtime::agent_loop::AgentLoopError,
) -> AgentError {
    match err {
        crate::runtime::agent_loop::AgentLoopError::ProviderError(provider_error) => {
            AgentError::ProviderExecution(provider_error.to_string())
        }
        crate::runtime::agent_loop::AgentLoopError::MaxIterationsExceeded { .. }
        | crate::runtime::agent_loop::AgentLoopError::BudgetExhausted { .. } => {
            AgentError::AgentLoopExhausted(err.to_string())
        }
        crate::runtime::agent_loop::AgentLoopError::Cancelled { .. } => {
            AgentError::ProviderExecution(err.to_string())
        }
    }
}

fn worker_loop_counts_from_loop_error(
    err: &crate::runtime::agent_loop::AgentLoopError,
) -> (usize, usize) {
    match err {
        crate::runtime::agent_loop::AgentLoopError::MaxIterationsExceeded {
            iterations,
            tool_calls_made,
            ..
        }
        | crate::runtime::agent_loop::AgentLoopError::BudgetExhausted {
            iterations,
            tool_calls_made,
            ..
        } => (*iterations, *tool_calls_made),
        crate::runtime::agent_loop::AgentLoopError::Cancelled { iterations, .. } => {
            (*iterations, 0)
        }
        crate::runtime::agent_loop::AgentLoopError::ProviderError(_) => (0, 0),
    }
}

fn worker_loop_counts_from_agent_error(err: &AgentError) -> (usize, usize) {
    let AgentError::AgentLoopExhausted(message) = err else {
        return (0, 0);
    };
    (
        parse_usize_after(message, "after ").unwrap_or(0),
        parse_usize_before(message, " tool call").unwrap_or(0),
    )
}

fn parse_usize_after(message: &str, marker: &str) -> Option<usize> {
    let tail = message.split_once(marker)?.1;
    let digits = tail
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn parse_usize_before(message: &str, marker: &str) -> Option<usize> {
    let before = message.split_once(marker)?.0;
    let digits = before
        .chars()
        .rev()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.chars().rev().collect::<String>().parse().ok()
}

pub fn list(data_dir: &Path) -> Result<AgentListResult, AgentError> {
    let mut agents = Vec::new();
    for agent_id in agent_ids(data_dir)? {
        let Some((packet, runtime)) = try_load_agent_list_entry(data_dir, &agent_id)? else {
            continue;
        };
        agents.push(AgentListEntry {
            agent_id: agent_id.clone(),
            lifecycle_status: runtime.lifecycle_status,
            runner_kind: runtime.runner_kind,
            intent: packet.intent,
            role_profile: packet.role_profile,
            created_at: runtime.created_at,
            updated_at: runtime.updated_at,
            task_packet_ref: runtime.task_packet_ref,
            output_manifest_ref: runtime.output_manifest_ref,
            workspace_binding_ref: runtime.workspace_binding_ref,
        });
    }
    agents.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(AgentListResult {
        total_count: agents.len(),
        agents,
    })
}

fn try_load_agent_list_entry(
    data_dir: &Path,
    agent_id: &str,
) -> Result<Option<(TaskPacket, AgentRuntimeRecord)>, AgentError> {
    match (
        load_task_packet(data_dir, agent_id),
        load_runtime(data_dir, agent_id),
    ) {
        (Ok(packet), Ok(runtime)) => Ok(Some((packet, runtime))),
        (Err(AgentError::UnknownAgent(_)), _) | (_, Err(AgentError::UnknownAgent(_))) => Ok(None),
        (Err(err), _) | (_, Err(err)) => Err(err),
    }
}

pub fn inspect(data_dir: &Path, agent_id: &str) -> Result<AgentInspectionResult, AgentError> {
    if !agent_dir(data_dir, agent_id).exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    Ok(AgentInspectionResult {
        agent_id: agent_id.to_string(),
        agent_root: agent_dir(data_dir, agent_id).display().to_string(),
        task_packet: load_task_packet(data_dir, agent_id)?,
        runtime_record: load_runtime(data_dir, agent_id)?,
        output_manifest: load_output_manifest(data_dir, agent_id)?,
        trace: load_trace(data_dir, agent_id)?,
        workspace_binding: load_workspace_binding(data_dir, agent_id)?,
    })
}

pub fn traces(data_dir: &Path, agent_id: &str) -> Result<AgentTracesResult, AgentError> {
    if !agent_dir(data_dir, agent_id).exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    let trace_path = trace_path(data_dir, agent_id);
    Ok(AgentTracesResult {
        agent_id: agent_id.to_string(),
        trace_path: trace_path.display().to_string(),
        trace: load_trace(data_dir, agent_id)?,
    })
}

pub fn runtime_identity(
    data_dir: &Path,
    agent_id: &str,
) -> Result<AgentRuntimeIdentity, AgentError> {
    if !agent_dir(data_dir, agent_id).exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    load_runtime_identity(data_dir, agent_id)
}

pub fn reclaim_stale_running_agent(
    data_dir: &Path,
    agent_id: &str,
    stale_after_ms: u128,
) -> Result<AgentStaleRunningReclaimResult, AgentError> {
    let runtime_record = load_runtime(data_dir, agent_id)?;
    if runtime_record.lifecycle_status != "running" {
        return Ok(AgentStaleRunningReclaimResult {
            agent_id: agent_id.to_string(),
            reclaimed: false,
            reason: "agent_not_running".to_string(),
            runtime_record,
        });
    }
    let task_budget_ms = load_task_packet(data_dir, agent_id)
        .ok()
        .map(|packet| u128::from(packet.budget.max_runtime_ms));
    let stale_after_ms = task_budget_ms
        .map(|budget| budget.saturating_add(30_000))
        .unwrap_or(stale_after_ms);
    let supervisor_alive = status_ref_supervisor_pid_is_alive(runtime_record.status_ref.as_deref());
    if supervisor_alive {
        return Ok(AgentStaleRunningReclaimResult {
            agent_id: agent_id.to_string(),
            reclaimed: false,
            reason: "supervisor_process_alive".to_string(),
            runtime_record,
        });
    }
    let heartbeat_stale = running_agent_heartbeat_is_stale(&runtime_record, stale_after_ms);
    if !heartbeat_stale {
        return Ok(AgentStaleRunningReclaimResult {
            agent_id: agent_id.to_string(),
            reclaimed: false,
            reason: "heartbeat_not_stale".to_string(),
            runtime_record,
        });
    }

    let updated_at = timestamp_string();
    let message = format!(
        "running worker heartbeat became stale or its supervisor process exited before recording final output; agent_id={agent_id}"
    );
    let runtime_record = persist_agent_lifecycle_failure(
        data_dir,
        agent_id,
        "stale_running_worker",
        &message,
        &updated_at,
    )?;
    Ok(AgentStaleRunningReclaimResult {
        agent_id: agent_id.to_string(),
        reclaimed: true,
        reason: "stale_running_worker".to_string(),
        runtime_record,
    })
}

pub fn stop(data_dir: &Path, agent_id: &str) -> Result<AgentStopResult, AgentError> {
    let mut runtime_record = load_runtime(data_dir, agent_id)?;
    let idempotent = is_terminal_status(&runtime_record.lifecycle_status);
    if !idempotent {
        let updated_at = timestamp_string();
        runtime_record.lifecycle_status = "stopped".to_string();
        runtime_record.updated_at = updated_at.clone();
        runtime_record.heartbeat_at = updated_at;
        runtime_record.stop_reason = "requested".to_string();
        atomic_write(
            &runtime_path(data_dir, agent_id),
            &serde_json::to_string_pretty(&runtime_record)?,
        )?;
    }

    Ok(AgentStopResult {
        agent_id: agent_id.to_string(),
        idempotent,
        runtime_record,
    })
}

fn persist_agent_lifecycle_failure(
    data_dir: &Path,
    agent_id: &str,
    failure_code: &str,
    message: &str,
    updated_at: &str,
) -> Result<AgentRuntimeRecord, AgentError> {
    let mut runtime_record = load_runtime(data_dir, agent_id)?;
    if is_terminal_status(&runtime_record.lifecycle_status) {
        return Ok(runtime_record);
    }
    runtime_record.lifecycle_status = "failed".to_string();
    runtime_record.updated_at = updated_at.to_string();
    runtime_record.heartbeat_at = updated_at.to_string();
    runtime_record.stop_reason = failure_code.to_string();
    runtime_record.failure_code = failure_code.to_string();

    let mut manifest = load_output_manifest(data_dir, agent_id).unwrap_or(AgentOutputManifest {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.to_string(),
        manifest_id: format!("manifest_{updated_at}"),
        status: "failed".to_string(),
        output_refs: Vec::new(),
        validation_status: "invalid".to_string(),
        validation_errors: Vec::new(),
        created_at: updated_at.to_string(),
    });
    manifest.status = "failed".to_string();
    manifest.validation_status = "invalid".to_string();
    if !manifest
        .validation_errors
        .iter()
        .any(|error| error == message)
    {
        manifest.validation_errors.push(message.to_string());
    }
    let stderr_path = stderr_path(data_dir, agent_id);
    if !manifest
        .output_refs
        .iter()
        .any(|output| output.kind == "stderr")
    {
        manifest.output_refs.push(AgentOutputRef {
            kind: "stderr".to_string(),
            r#ref: stderr_path.display().to_string(),
            summary: "agent lifecycle failure detail".to_string(),
        });
    }

    let mut trace = load_trace(data_dir, agent_id).unwrap_or(AgentTrace {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.to_string(),
        trace_id: format!("trace_{updated_at}"),
        task_packet_ref: runtime_record.task_packet_ref.clone(),
        runtime_events: Vec::new(),
        tool_actions: Vec::new(),
        permission_decisions: Vec::new(),
        output_records: Vec::new(),
        final_status: "failed".to_string(),
        created_at: runtime_record.created_at.clone(),
        trace_path: trace_path(data_dir, agent_id).display().to_string(),
    });
    trace.runtime_events.push(trace_event(
        "agent_stale_running_reclaimed",
        message,
        updated_at,
    ));
    trace.output_records.push(trace_event(
        "output_manifest_failed",
        &output_manifest_path(data_dir, agent_id)
            .display()
            .to_string(),
        updated_at,
    ));
    trace.final_status = "failed".to_string();

    if let Ok(mut identity) = load_runtime_identity(data_dir, agent_id) {
        identity.lifecycle_status = "failed".to_string();
        identity.updated_at = updated_at.to_string();
        atomic_write(
            &runtime_identity_path(data_dir, agent_id),
            &serde_json::to_string_pretty(&identity)?,
        )?;
    }

    atomic_write(&stderr_path, message)?;
    atomic_write(
        &runtime_path(data_dir, agent_id),
        &serde_json::to_string_pretty(&runtime_record)?,
    )?;
    atomic_write(
        &output_manifest_path(data_dir, agent_id),
        &serde_json::to_string_pretty(&manifest)?,
    )?;
    atomic_write(
        &trace_path(data_dir, agent_id),
        &serde_json::to_string_pretty(&trace)?,
    )?;
    Ok(runtime_record)
}

fn running_agent_heartbeat_is_stale(
    runtime_record: &AgentRuntimeRecord,
    stale_after_ms: u128,
) -> bool {
    let heartbeat_ms = normalize_timestamp_to_millis(&runtime_record.heartbeat_at)
        .or_else(|| normalize_timestamp_to_millis(&runtime_record.updated_at));
    let Some(heartbeat_ms) = heartbeat_ms else {
        return !status_ref_supervisor_pid_is_alive(runtime_record.status_ref.as_deref());
    };
    current_timestamp_millis().saturating_sub(heartbeat_ms) >= stale_after_ms
}

fn status_ref_supervisor_pid_is_alive(status_ref: Option<&str>) -> bool {
    let Some(status_ref) = status_ref else {
        return false;
    };
    let Ok(contents) = fs::read_to_string(status_ref) else {
        return false;
    };
    let Some(pid) = parse_supervisor_pid_from_status(&contents) else {
        return false;
    };
    let status_starttime = parse_supervisor_starttime_from_status(&contents);
    process_id_matches_starttime(pid, status_starttime)
}

fn parse_supervisor_pid_from_status(contents: &str) -> Option<u32> {
    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed
            .strip_prefix("supervisor_pid:")
            .or_else(|| trimmed.strip_prefix("pid:"))?
            .trim()
            .trim_matches('"')
            .trim_matches('\'');
        value.parse::<u32>().ok()
    })
}

fn parse_supervisor_starttime_from_status(contents: &str) -> Option<u128> {
    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed
            .strip_prefix("supervisor_starttime:")
            .or_else(|| trimmed.strip_prefix("supervisor_starttime_ms:"))?
            .trim()
            .trim_matches('"')
            .trim_matches('\'');
        value.parse::<u128>().ok()
    })
}

fn process_id_matches_starttime(pid: u32, expected_starttime: Option<u128>) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        if let Some(expected_starttime) = expected_starttime {
            let pid = pid as libc::pid_t;
            let Ok(actual_starttime) = process_starttime(pid) else {
                return false;
            };
            actual_starttime == expected_starttime
        } else {
            process_id_is_alive(pid)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = expected_starttime;
        process_id_is_alive(pid)
    }
}

fn child_starttime(pid: u32) -> Option<u128> {
    process_starttime(pid as libc::pid_t).ok()
}

#[cfg(unix)]
fn process_starttime(pid: libc::pid_t) -> Result<u128, std::io::Error> {
    let stat_path = format!("/proc/{pid}/stat");
    let contents = fs::read_to_string(stat_path)?;
    let Some(after_comm) = contents.rsplit_once(") ") else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "malformed process stat",
        ));
    };
    let fields = after_comm.1.split_whitespace().collect::<Vec<_>>();
    let Some(starttime) = fields.get(19) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing process starttime",
        ));
    };
    starttime.parse::<u128>().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid starttime: {err}"),
        )
    })
}

#[cfg(not(unix))]
fn process_starttime(_pid: libc::pid_t) -> Result<u128, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process starttime unavailable",
    ))
}

fn process_id_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let pid = pid as libc::pid_t;
        unsafe {
            let result = libc::kill(pid, 0);
            if result == 0 {
                true
            } else {
                matches!(std::io::Error::last_os_error().raw_os_error(), Some(code) if code == libc::EPERM)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

fn process_unique_suffix() -> String {
    format!(
        "{}_{}_{}",
        std::process::id(),
        current_timestamp_millis(),
        format!("{:?}", thread::current().id()).replace(['(', ')', ' ', '"'], "")
    )
}

fn normalize_timestamp_to_millis(value: &str) -> Option<u128> {
    let parsed = value.trim().parse::<u128>().ok()?;
    if parsed >= 1_000_000_000_000_000_000 {
        Some(parsed / 1_000_000)
    } else if parsed >= 1_000_000_000_000_000 {
        Some(parsed / 1_000)
    } else if parsed >= 1_000_000_000_000 {
        Some(parsed)
    } else if parsed >= 1_000_000_000 {
        Some(parsed * 1_000)
    } else {
        Some(parsed)
    }
}

fn load_task_packet(data_dir: &Path, agent_id: &str) -> Result<TaskPacket, AgentError> {
    let path = task_packet_path(data_dir, agent_id);
    if !path.exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_runtime(data_dir: &Path, agent_id: &str) -> Result<AgentRuntimeRecord, AgentError> {
    let path = runtime_path(data_dir, agent_id);
    if !path.exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_runtime_identity(
    data_dir: &Path,
    agent_id: &str,
) -> Result<AgentRuntimeIdentity, AgentError> {
    let path = runtime_identity_path(data_dir, agent_id);
    if !path.exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_output_manifest(
    data_dir: &Path,
    agent_id: &str,
) -> Result<AgentOutputManifest, AgentError> {
    let path = output_manifest_path(data_dir, agent_id);
    if !path.exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_trace(data_dir: &Path, agent_id: &str) -> Result<AgentTrace, AgentError> {
    let path = trace_path(data_dir, agent_id);
    if !path.exists() {
        return Err(AgentError::UnknownAgent(agent_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_workspace_binding(
    data_dir: &Path,
    agent_id: &str,
) -> Result<Option<AgentWorkspaceBinding>, AgentError> {
    let path = workspace_binding_path(data_dir, agent_id);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

fn agent_ids(data_dir: &Path) -> Result<Vec<String>, AgentError> {
    let root = agents_root(data_dir);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut ids = fs::read_dir(root)?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

fn trace_event(event: &str, detail: &str, timestamp: &str) -> AgentTraceEvent {
    AgentTraceEvent {
        event: event.to_string(),
        detail: detail.to_string(),
        timestamp: timestamp.to_string(),
    }
}

fn provider_worker_empty_tool_receipts(
    agent_id: &str,
    session_ref: Option<String>,
) -> AgentToolReceiptManifest {
    provider_worker_tool_receipt_manifest(agent_id, session_ref, Vec::new())
}

fn provider_worker_tool_receipt_manifest(
    agent_id: &str,
    session_ref: Option<String>,
    receipts: Vec<AgentToolReceipt>,
) -> AgentToolReceiptManifest {
    AgentToolReceiptManifest {
        schema_version: "agent_tool_receipt_manifest.v1".to_string(),
        agent_id: agent_id.to_string(),
        authority_scope: "worker_evidence_only".to_string(),
        session_ref,
        receipt_count: receipts.len(),
        receipts,
        generated_at: timestamp_string(),
    }
}

fn provider_worker_tool_receipt_from_event(
    agent_id: &str,
    data_dir: &Path,
    call_index: usize,
    event: &crate::runtime::agent_loop::AgentLoopToolResultEvent,
    recorded_at: &str,
) -> AgentToolReceipt {
    let output = event
        .result
        .as_ref()
        .and_then(|result| result.output.as_ref())
        .filter(|output| !output.is_empty())
        .cloned()
        .unwrap_or_else(|| event.output_text.clone());
    let output_bytes = output.len();
    let output_sha256 = sha256_bytes(output.as_bytes());
    let output_preview = safe_char_prefix(&output, PROVIDER_TOOL_RECEIPT_PREVIEW_CHARS);
    let (snapshot_ref, snapshot_bytes, snapshot_truncated) =
        write_provider_tool_receipt_snapshot(agent_id, data_dir, call_index, event, &output);
    AgentToolReceipt {
        call_index,
        call_id: Some(event.call_id.clone()).filter(|call_id| !call_id.trim().is_empty()),
        tool_name: event.tool_name.clone(),
        status: event.status.clone(),
        arguments: sanitize_provider_tool_receipt_arguments(&event.tool_name, &event.arguments),
        output_bytes,
        output_sha256,
        output_preview,
        snapshot_ref,
        snapshot_bytes,
        snapshot_truncated,
        structured_summary: event
            .result
            .as_ref()
            .and_then(provider_tool_receipt_structured_summary),
        recorded_at: recorded_at.to_string(),
    }
}

fn write_provider_tool_receipt_snapshot(
    agent_id: &str,
    data_dir: &Path,
    call_index: usize,
    event: &crate::runtime::agent_loop::AgentLoopToolResultEvent,
    output: &str,
) -> (Option<String>, usize, bool) {
    if output.trim().is_empty() {
        return (None, 0, false);
    }
    let snapshot_dir = tool_receipt_snapshot_dir(data_dir, agent_id);
    let filename = format!(
        "{:03}_{}_{}.txt",
        call_index,
        safe_archive_relative_path(&event.tool_name),
        safe_archive_relative_path(&event.call_id)
    );
    let snapshot_path = snapshot_dir.join(filename);
    let mut snapshot = output
        .chars()
        .take(PROVIDER_TOOL_RECEIPT_SNAPSHOT_MAX_BYTES)
        .collect::<String>();
    let snapshot_truncated = snapshot.len() < output.len();
    if snapshot_truncated {
        snapshot.push_str("\n\n[tool receipt snapshot truncated]");
    }
    let snapshot_bytes = snapshot.len();
    match atomic_write(&snapshot_path, &snapshot) {
        Ok(()) => (
            Some(snapshot_path.display().to_string()),
            snapshot_bytes,
            snapshot_truncated,
        ),
        Err(_) => (None, 0, false),
    }
}

fn sanitize_provider_tool_receipt_arguments(tool_name: &str, arguments: &str) -> serde_json::Value {
    let parsed = serde_json::from_str::<serde_json::Value>(arguments).unwrap_or_else(|_| {
        serde_json::json!({
            "raw_arguments_preview": safe_char_prefix(arguments, 1024),
            "raw_arguments_sha256": sha256_bytes(arguments.as_bytes()),
            "parse_status": "invalid_json"
        })
    });
    match tool_name {
        "write_file" => sanitize_write_like_tool_arguments(parsed, "content"),
        "apply_patch" => sanitize_write_like_tool_arguments(parsed, "patch"),
        _ => parsed,
    }
}

fn sanitize_write_like_tool_arguments(
    mut parsed: serde_json::Value,
    content_field: &str,
) -> serde_json::Value {
    let Some(object) = parsed.as_object_mut() else {
        return parsed;
    };
    if let Some(content) = object.remove(content_field) {
        let text = content.as_str().unwrap_or_default();
        object.insert(
            format!("{content_field}_bytes"),
            serde_json::json!(text.len()),
        );
        object.insert(
            format!("{content_field}_sha256"),
            serde_json::json!(sha256_bytes(text.as_bytes())),
        );
    }
    parsed
}

fn safe_char_prefix(value: &str, max_chars: usize) -> String {
    if max_chars == 0 || value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn provider_tool_receipt_structured_summary(
    result: &crate::tools::ToolResult,
) -> Option<serde_json::Value> {
    let structured = result.structured.as_ref()?;
    let summary = match result.tool_name.as_str() {
        "fetch" => serde_json::json!({
            "url": structured.get("url").cloned().unwrap_or(serde_json::Value::Null),
            "final_url": structured.get("final_url").cloned().unwrap_or(serde_json::Value::Null),
            "status_code": structured.get("status_code").cloned().unwrap_or(serde_json::Value::Null),
            "bytes": structured.get("bytes").cloned().unwrap_or(serde_json::Value::Null),
            "redirect_count": structured.get("redirect_count").cloned().unwrap_or(serde_json::Value::Null),
            "network_policy": structured.get("network_policy").cloned().unwrap_or(serde_json::Value::Null),
        }),
        "read_file" => serde_json::json!({
            "path": structured.get("path").cloned().unwrap_or(serde_json::Value::Null),
            "bytes": structured.get("bytes").cloned().unwrap_or(serde_json::Value::Null),
            "offset": structured.get("offset").cloned().unwrap_or(serde_json::Value::Null),
            "returned_chars": structured.get("returned_chars").cloned().unwrap_or(serde_json::Value::Null),
            "total_chars": structured.get("total_chars").cloned().unwrap_or(serde_json::Value::Null),
        }),
        "worker_shell" | "shell" => serde_json::json!({
            "command": structured.get("command").cloned().unwrap_or(serde_json::Value::Null),
            "exit_code": structured.get("exit_code").cloned().unwrap_or(serde_json::Value::Null),
            "duration_ms": structured.get("duration_ms").cloned().unwrap_or(serde_json::Value::Null),
            "timeout_ms": structured.get("timeout_ms").cloned().unwrap_or(serde_json::Value::Null),
            "timed_out": structured.get("timed_out").cloned().unwrap_or(serde_json::Value::Null),
        }),
        _ => return Some(structured.clone()),
    };
    Some(summary)
}

fn runtime_identity_for_task_packet(
    packet: &TaskPacket,
    lifecycle_status: &str,
    authority_scope: &str,
    runtime_record: &AgentRuntimeRecord,
    output_manifest_path: Option<&Path>,
    updated_at: &str,
) -> AgentRuntimeIdentity {
    AgentRuntimeIdentity {
        schema_version: "agent_runtime_identity.v1".to_string(),
        runtime_id: packet.agent_id.clone(),
        agent_id: packet.agent_id.clone(),
        runtime_kind: "delegated_worker".to_string(),
        role_kind: role_kind_from_profile(&packet.role_profile).to_string(),
        role_profile: packet.role_profile.clone(),
        runner_kind: packet
            .runner_kind
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        authority_scope: authority_scope.to_string(),
        lifecycle_status: lifecycle_status.to_string(),
        session_ref: runtime_record.session_ref.clone(),
        task_packet_ref: Some(runtime_record.task_packet_ref.clone()),
        output_manifest_ref: output_manifest_path.map(|path| path.display().to_string()),
        provider_id: None,
        model: None,
        trace_refs: runtime_record.trace_refs.clone(),
        tool_scope: delegated_worker_tool_scope(packet),
        evidence_refs: output_manifest_path
            .map(|path| vec![path.display().to_string()])
            .unwrap_or_default(),
        created_at: packet.created_at.clone(),
        updated_at: updated_at.to_string(),
    }
}

fn run_provider_worker(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: &TaskPacket,
    provider_id: &str,
    model: &str,
) -> Result<ProviderWorkerRun, ProviderWorkerFailure> {
    let provider_trace = crate::providers::resolve_provider_trace(
        Some(provider_id),
        Some(model),
        Some("agent_team_worker_policy"),
        Some("agent_team_worker_policy"),
    )
    .map_err(|err| ProviderWorkerFailure::new(AgentError::ProviderExecution(err.to_string())))?;
    if provider_trace.auth_status != "configured" {
        return Err(ProviderWorkerFailure::new(AgentError::ProviderExecution(
            format!(
                "provider {} is not configured; missing {}",
                provider_trace.resolved_provider, provider_trace.auth_env_var
            ),
        )));
    }
    let messages = provider_worker_messages(data_dir, workspace_root, task_packet);
    let tool_defs = provider_worker_tool_definitions(task_packet);
    let tool_scope = tool_defs
        .iter()
        .map(|definition| definition.name.clone())
        .collect::<Vec<_>>();
    let tool_defs_json = crate::tools::tool_definitions_to_openai_json(&tool_defs);
    let tool_executor = crate::tools::LocalToolExecutor::new(workspace_root);
    let permission_policy = crate::permissions::PermissionPolicy::new(
        crate::permissions::PermissionMode::WorkspaceWrite,
        workspace_root,
    );
    let store = crate::session::store::SessionStore::new(data_dir.to_path_buf());
    let session = store
        .create_session_with_kind(
            Some(format!("Agent-team worker {}", task_packet.agent_id)),
            "agent_team_worker",
        )
        .map_err(|err| {
            ProviderWorkerFailure::new(AgentError::ProviderExecution(err.to_string()))
        })?;
    let session_id = session.session_id;
    store
        .append_line(
            &session_id,
            crate::session::transcript::TranscriptLine::Message {
                role: "user".to_string(),
                content: task_packet.message.clone(),
            },
        )
        .map_err(|err| {
            ProviderWorkerFailure::new(AgentError::ProviderExecution(err.to_string()))
        })?;
    let cancel_token = crate::runtime::cancel::RuntimeCancelToken::default();
    let mut text_buffer = String::new();
    let tool_action_events = std::cell::RefCell::new(Vec::<AgentTraceEvent>::new());
    let tool_receipts = std::cell::RefCell::new(Vec::<AgentToolReceipt>::new());
    let mut on_delta = |delta: &str| {
        text_buffer.push_str(delta);
    };
    let mut on_tool_start = |name: &str, id: &str| {
        tool_action_events.borrow_mut().push(trace_event(
            "provider_tool_started",
            &format!("tool={name}; call_id={id}"),
            &timestamp_string(),
        ));
    };
    let mut on_tool_result = |event: crate::runtime::agent_loop::AgentLoopToolResultEvent| {
        let recorded_at = timestamp_string();
        tool_action_events.borrow_mut().push(trace_event(
            "provider_tool_completed",
            &format!(
                "tool={}; call_id={}; status={}",
                event.tool_name, event.call_id, event.status
            ),
            &recorded_at,
        ));
        let call_index = tool_receipts.borrow().len() + 1;
        tool_receipts
            .borrow_mut()
            .push(provider_worker_tool_receipt_from_event(
                &task_packet.agent_id,
                data_dir,
                call_index,
                &event,
                &recorded_at,
            ));
    };
    let loop_result =
        match crate::runtime::agent_loop::run_agent_loop_with_budget_and_timeout_policy(
            &provider_trace,
            &messages,
            tool_defs_json,
            &tool_executor,
            &permission_policy,
            &store,
            &session_id,
            &cancel_token,
            &mut on_delta,
            &mut on_tool_start,
            &mut on_tool_result,
            None,
            provider_worker_loop_budget(task_packet),
            provider_worker_timeout_policy(task_packet),
        ) {
            Ok(result) => result,
            Err(err) => {
                let (iterations, tool_calls_made) = worker_loop_counts_from_loop_error(&err);
                let receipts = tool_receipts.take();
                return Err(ProviderWorkerFailure {
                    error: agent_error_from_worker_loop_error(err),
                    iterations,
                    tool_calls_made,
                    tool_actions: tool_action_events.take(),
                    tool_receipts: Some(provider_worker_tool_receipt_manifest(
                        &task_packet.agent_id,
                        Some(session_id.clone()),
                        receipts,
                    )),
                });
            }
        };
    let output = if loop_result.final_content.trim().is_empty() {
        text_buffer
    } else {
        loop_result.final_content
    };
    store
        .append_line(
            &session_id,
            crate::session::transcript::TranscriptLine::Message {
                role: "assistant".to_string(),
                content: output.clone(),
            },
        )
        .map_err(|err| {
            ProviderWorkerFailure::new(AgentError::ProviderExecution(err.to_string()))
        })?;
    let tool_receipts = provider_worker_tool_receipt_manifest(
        &task_packet.agent_id,
        Some(session_id.clone()),
        tool_receipts.into_inner(),
    );
    Ok(ProviderWorkerRun {
        output,
        session_id,
        iterations: loop_result.iterations,
        tool_calls_made: loop_result.tool_calls_made,
        finish_reason: loop_result.finish_reason,
        tool_scope,
        tool_actions: tool_action_events.into_inner(),
        tool_receipts,
    })
}

fn provider_worker_loop_budget(
    task_packet: &TaskPacket,
) -> crate::runtime::agent_loop::AgentLoopBudget {
    let runtime_floor = crate::runtime::agent_loop::AgentLoopBudget::default().max_iterations;
    let runtime_budget = (task_packet.budget.max_runtime_ms / 1_000) as usize;
    let max_iterations = runtime_budget.max(runtime_floor);
    let max_tool_calls = provider_worker_max_tool_calls(task_packet.budget.max_runtime_ms);
    crate::runtime::agent_loop::AgentLoopBudget::new(max_iterations, max_tool_calls)
        .with_max_elapsed(Duration::from_millis(task_packet.budget.max_runtime_ms))
        .with_tool_budget_finalization(provider_worker_closeout_tool_threshold(max_tool_calls))
}

fn provider_worker_max_tool_calls(max_runtime_ms: u64) -> usize {
    let runtime_seconds = (max_runtime_ms / 1_000).max(1) as usize;
    (runtime_seconds / 5).clamp(12, 60)
}

fn provider_worker_closeout_tool_threshold(max_tool_calls: usize) -> usize {
    (max_tool_calls / 3)
        .max(4)
        .min(max_tool_calls.saturating_sub(1))
        .max(1)
}

fn provider_worker_budget_for_stage_task(contract: Option<&AgentStageTaskContract>) -> AgentBudget {
    AgentBudget {
        max_turns: 1,
        max_runtime_ms: provider_worker_max_runtime_ms_for_stage_task(contract),
    }
}

fn provider_worker_max_runtime_ms_for_stage_task(contract: Option<&AgentStageTaskContract>) -> u64 {
    if contract
        .map(provider_worker_stage_task_needs_extended_runtime)
        .unwrap_or(false)
    {
        EXTENDED_PROVIDER_WORKER_MAX_RUNTIME_MS
    } else {
        DEFAULT_PROVIDER_WORKER_MAX_RUNTIME_MS
    }
}

fn provider_worker_stage_task_needs_extended_runtime(contract: &AgentStageTaskContract) -> bool {
    contract.input_artifact_refs.len() >= PROVIDER_WORKER_EXTENDED_INPUT_REF_THRESHOLD
        || provider_worker_stage_task_structured_complexity(contract)
            >= PROVIDER_WORKER_EXTENDED_TASK_COMPLEXITY_THRESHOLD
        || provider_worker_stage_task_has_wide_output_review_surface(contract)
}

fn provider_worker_stage_task_has_wide_output_review_surface(
    contract: &AgentStageTaskContract,
) -> bool {
    contract.required_output_fields.len() >= PROVIDER_WORKER_EXTENDED_OUTPUT_FIELD_THRESHOLD
        && contract
            .acceptance_checks
            .len()
            .saturating_add(contract.failure_signals.len())
            >= PROVIDER_WORKER_EXTENDED_REVIEW_SURFACE_THRESHOLD
}

fn provider_worker_stage_task_structured_complexity(contract: &AgentStageTaskContract) -> usize {
    contract.input_artifact_refs.len()
        + contract.required_canonical_artifacts.len()
        + contract.required_output_fields.len()
        + contract.acceptance_checks.len()
        + contract.failure_signals.len()
        + contract.depends_on_task_ids.len()
        + contract.review_findings_refs.len()
        + contract.blocker_refs.len()
        + contract.review_target_task_ids.len()
        + contract.review_target_evidence_refs.len()
        + contract.supersedes_task_ids.len()
        + contract.replacement_of_task_ids.len()
        + usize::from(contract.current_evidence_set_id.is_some())
}

fn provider_run_class(automation_mode: Option<&str>) -> &'static str {
    match automation_mode {
        Some("full_auto") => "full_auto_blocking",
        _ => "bounded",
    }
}

fn provider_worker_timeout_policy(
    task_packet: &TaskPacket,
) -> crate::providers::ProviderTimeoutPolicy {
    if task_packet.run_class == "full_auto_blocking" {
        crate::providers::ProviderTimeoutPolicy::TaskBudget(Duration::from_millis(
            task_packet.budget.max_runtime_ms,
        ))
    } else {
        crate::providers::ProviderTimeoutPolicy::Interactive
    }
}

fn provider_worker_messages(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: &TaskPacket,
) -> Vec<crate::providers::ChatMessage> {
    let role_soul = render_worker_role_soul_system_context(workspace_root, task_packet);
    let assigned_skills = render_worker_skill_refs_for_prompt(task_packet);
    let tool_policy = serde_json::to_string_pretty(&effective_worker_tool_policy(task_packet))
        .unwrap_or_else(|_| "unavailable".to_string());
    let mounted_tools = provider_worker_tool_scope(task_packet).join(", ");
    let mounted_inputs =
        render_worker_input_artifacts_for_prompt(data_dir, workspace_root, task_packet);
    let synthesis_protocol = render_worker_synthesis_protocol_for_prompt(task_packet);
    let standard_setting_protocol = render_worker_standard_setting_protocol_for_prompt(task_packet);
    let task_artifact_protocol = render_worker_task_artifact_protocol_for_prompt(task_packet);
    let consumed_input_protocol = render_worker_consumed_input_protocol_for_prompt(task_packet);
    let max_tool_calls = provider_worker_max_tool_calls(task_packet.budget.max_runtime_ms);
    let closeout_threshold = provider_worker_closeout_tool_threshold(max_tool_calls);
    vec![
        crate::providers::ChatMessage {
            role: "system".to_string(),
            content: format!(
                "You are an Astra delegated agent-team worker. Execute only the task packet. You produce evidence for the main agent and strict reviewer. You do not decide stage advancement, rollback, cleanup, board-task publication, obligation closure, or research strategy outside the assigned task. Do not invent citations, files, experiments, or results. Use only your worker-scoped tools. Canonical project paths are authoritative for import, execution, tests, and repairs. `.pmcli/input-bundles` is provenance/context only and must not be treated as canonical project source. You may write task-local artifacts in your isolated worker worktree; they are candidate-only and are not canonical project state until the main agent adopts them and runtime makes them baseline-visible.\n\n{}",
                role_soul
            ),
            tool_calls: None,
            tool_call_id: None,
        },
        crate::providers::ChatMessage {
            role: "user".to_string(),
            content: format!(
                concat!(
                    "Worker workspace root: {}\n\n",
                    "Task packet:\n{}\n\n",
                    "Structured stage task contract:\n{}\n\n",
                    "Mounted input artifacts:\n{}\n\n",
                    "Astra collaboration protocol:\n{}\n\n",
                    "Assigned worker skills:\n{}\n\n",
                    "Worker tool policy:\n{}\n\n",
                    "Mounted worker tools: {}\n\n",
                    "Worker execution budget:\n",
                    "- Wall-clock budget: {} ms.\n",
                    "- Tool-call budget: at most {} calls.\n",
                    "- When roughly {} tool call(s) remain, stop broad exploration, write a task-local artifact if needed, and return final evidence.\n",
                    "- For source-heavy tasks, prefer fewer high-quality source refs over unbounded retrieval; explicitly list missing evidence and repair tasks rather than exhausting the run.\n\n",
                    "Synthesis protocol:\n{}\n\n",
                    "Standard-setting protocol:\n{}\n\n",
                    "Task artifact protocol:\n{}\n\n",
                    "Consumed-input protocol:\n{}\n\n",
                    "Worker requirements:\n",
                    "- Complete the assigned objective as far as possible from the task context.\n",
                    "- Treat `stage_task_contract`, when present, as the authoritative machine-readable task contract.\n",
                    "- Treat `stage_task_contract.required_canonical_artifacts` as canonical project path dependencies that must already exist in this worker workspace at the required status.\n",
                    "- Treat `stage_task_contract.input_artifact_refs` as mounted historical evidence or provenance context, not as proof that project files are canonical or runnable.\n",
                    "- Treat `stage_task_contract.review_target_evidence_refs` as non-authoritative material under repair or review. If it is rejected or superseded evidence, quarantine it explicitly and never cite it as support for the new output.\n",
                    "- Read mounted input artifacts before declaring task inputs missing.\n",
                    "- Treat mounted input artifacts as read-only provenance/context; do not claim them as worker-created outputs.\n",
                    "- Do not import, execute, test, or repair project source from `.pmcli/input-bundles`; use canonical project paths in the worker workspace.\n",
                    "- If a required canonical project path is missing from the worker workspace, report a canonical artifact dependency blocker instead of copying from an input bundle.\n",
                    "- If `workspace_binding.json` records `baseline_mode=overlay`, trust the files already present at canonical project paths after overlay application and cite the binding ref when reporting baseline visibility.\n",
                    "- Treat `collaboration_protocol` as the authority boundary and output contract.\n",
                    "- Treat assigned skills as task method and output guidance, not as authority to change the task.\n",
                    "- Use only mounted tools; tools absent from the mounted list are unavailable even if mentioned in a skill.\n",
                    "- Demonstrate the required worker capabilities with concrete tool-backed evidence when the task requires it.\n",
                    "- Write any task-local artifacts under the worker workspace when useful.\n",
                    "- Return a concrete evidence artifact in Markdown.\n",
                    "- Your final assistant message after all tool calls is mandatory and must be non-empty Markdown evidence.\n",
                    "- Do not finish with only tool calls or file writes; summarize what you inspected, what you produced, and what remains blocked.\n",
                    "- If the task contract requires exact content to be reviewable from mounted evidence, paste that exact content into the final evidence message.\n",
                    "- If the consumed-input protocol is active, include a `## Consumed Input Refs` section in the final evidence message and in any candidate Markdown artifact you write.\n",
                    "- Include exact refs or explicit missing-evidence risks.\n",
                    "- Include repair tasks when evidence cannot be completed.\n",
                    "- Do not mark the research stage complete."
                ),
                workspace_root.display(),
                serde_json::to_string_pretty(task_packet)
                    .unwrap_or_else(|_| task_packet.message.clone()),
                task_packet
                    .stage_task_contract
                    .as_ref()
                    .and_then(|contract| serde_json::to_string_pretty(contract).ok())
                    .unwrap_or_else(|| "none".to_string()),
                mounted_inputs,
                task_packet
                    .collaboration_protocol
                    .as_ref()
                    .and_then(|protocol| serde_json::to_string_pretty(protocol).ok())
                    .unwrap_or_else(|| "none".to_string()),
                assigned_skills,
                tool_policy,
                mounted_tools,
                task_packet.budget.max_runtime_ms,
                max_tool_calls,
                closeout_threshold,
                synthesis_protocol,
                standard_setting_protocol,
                task_artifact_protocol,
                consumed_input_protocol
            ),
            tool_calls: None,
            tool_call_id: None,
        },
    ]
}

fn render_worker_consumed_input_protocol_for_prompt(task_packet: &TaskPacket) -> String {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return "none".to_string();
    };
    if !contract.consumed_input_required {
        return "none".to_string();
    }
    let refs = provider_worker_contract_input_refs_for_report(task_packet);
    let refs_text = if refs.is_empty() {
        "- `.pmcli/worker-input-artifacts.json`".to_string()
    } else {
        refs.iter()
            .map(|reference| format!("- `{}`", reference))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Consumed-input protocol:\n\
         - This task has `consumed_input_required=true`; after reading mounted inputs, your final evidence must include a section exactly named `## Consumed Input Refs`.\n\
         - In that section, cite `.pmcli/worker-input-artifacts.json` and at least one concrete ref from the list below that you actually used.\n\
         - If you write a candidate Markdown artifact, include the same `## Consumed Input Refs` section in that artifact body so runtime can mechanically verify provenance without judging semantics.\n\
         - Treat accepted upstream evidence as accepted task evidence and provenance, not as automatic full verification of every inherited source row. Preserve or downgrade each inherited row's verification status, metadata confidence, and claim-support boundary before using it for new comparison, clustering, synthesis, or gap claims.\n\
         - Available refs:\n{}",
        refs_text
    )
}

fn render_worker_task_artifact_protocol_for_prompt(task_packet: &TaskPacket) -> String {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return "none".to_string();
    };
    if is_standard_setting_task(contract) || is_synthesis_task_type(&contract.task_type) {
        return "none".to_string();
    }
    if contract.stage_id != "literature" {
        return "none".to_string();
    }
    let required_output = contract.required_output_artifact_type.as_str();
    if !matches!(
        required_output,
        "literature_matrix"
            | "open_problem_matrix"
            | "method_comparison_matrix"
            | "literature_cluster_matrix"
            | "closest_family_survey"
            | "citation_ledger"
            | "source_verification_ledger"
    ) {
        return "none".to_string();
    }
    let mut lines = vec![
        "Literature task artifact protocol:".to_string(),
        format!(
            "- The required output artifact type is `{required_output}`; the task is not complete if you only say that mounted input artifacts already contain the evidence."
        ),
        "- Mounted input bundles are context and provenance. Do not count copied input-bundle files as worker-created candidate artifacts.".to_string(),
        "- When using accepted upstream literature evidence, keep inherited verification limits explicit. `accepted_worker_evidence:*` proves the upstream task was accepted, not that every inherited title, venue, source id, or abstract-derived claim is fully verified for your new claim.".to_string(),
        "- For any inherited row used positively, restate canonical title, exact source ref/id, source verification status, metadata confidence, and claim-support boundary in your own artifact. Quarantine rows whose identity or support boundary is still unclear.".to_string(),
        "- The final evidence must show the actual rows, not only describe that rows exist elsewhere.".to_string(),
    ];
    if provider_worker_requires_safe_candidate_artifact(contract) {
        lines.push(
            "- This required output is a durable row-level artifact: write a clean candidate Markdown file in the worker workspace and cite its path. The final evidence message is a receipt and summary, not a substitute for the candidate file.".to_string(),
        );
    } else {
        lines.push(
            "- Produce your own self-contained task-local evidence body. Either write a clean candidate Markdown file in the worker workspace and cite its path, or paste the complete reviewable body in the final evidence message.".to_string(),
        );
    }
    if is_open_problem_extraction_task(contract) {
        lines.extend([
            "- For open-problem extraction, include at least five row-level open-problem or gap rows when the task contract asks for multiple gaps.".to_string(),
            "- Each open-problem row must name source ids or refs, canonical verified paper titles, source verification status, metadata confidence, closest existing coverage, coverage boundary, why the gap matters, relation to the active objective, and claim support boundary.".to_string(),
            "- If the mounted evidence cannot support five grounded gaps, write a blocked open-problem matrix with the rows that are supportable plus explicit missing-source repair tasks; do not report the task as completed by citing a prior review verdict.".to_string(),
        ]);
    }
    lines.join("\n")
}

fn render_worker_synthesis_protocol_for_prompt(task_packet: &TaskPacket) -> String {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return "none".to_string();
    };
    if !is_synthesis_task_type(&contract.task_type) {
        return "none".to_string();
    }
    let evidence_binding_contract = render_worker_stage_evidence_binding_contract(contract);
    format!(
        "Synthesis task protocol:\n\
     - This is a stage synthesis task. Treat mounted input artifacts and `.pmcli/worker-input-artifacts.json` as the primary source of truth for synthesis evidence, not as canonical project source.\n\
     - First read the mounted refs listed in `stage_task_contract.input_artifact_refs`; use list/search only to resolve missing mounted refs, not for broad workspace exploration.\n\
     - Synthesize accepted worker evidence into one standalone candidate artifact that covers the required output fields and cites exact input refs.\n\
     - Include a section exactly named `Evidence Binding Ledger` in the candidate artifact body. Use this section to bind every future-dependent unit in the artifact body to accepted evidence, canonical artifacts, or explicit missing-evidence risk.\n\
     - The `Evidence Binding Ledger` must include mechanically readable columns or keys named `unit_id`, `unit_kind`, `statement_or_target`, `support_status`, `accepted_evidence_refs`, `main_agent_decision_refs`, `canonical_artifact_refs`, `support_scope`, and `limitations_or_missing_risks`.\n\
     - Use `support_status` values `supported`, `partial`, `provisional`, `missing`, or `rejected`. Do not use `.pmcli/input-bundles` paths as canonical support; cite exact `accepted_worker_evidence_task:*`, `main_agent_worker_artifact_decision::*`, or canonical project refs.\n\
{evidence_binding_contract}\n\
     - The candidate artifact file that may be adopted as the canonical stage artifact must be a clean review body, not a repair memo. Put worker lifecycle notes such as candidate_artifact_path, retention_note, preflight pass/fail, and adoption handoff instructions in the worker evidence message, not in the candidate artifact body.\n\
     - For literature-stage artifacts, include an explicit `## Citation Ledger` section in the candidate body before row-level synthesis tables.\n\
     - If required accepted evidence is missing or inconsistent, return explicit repair tasks and missing-evidence risks in the evidence artifact instead of looping.\n\
     - Do not decide stage completion, candidate adoption, review verdicts, route changes, cleanup, or board-task publication."
    )
}

fn render_worker_stage_evidence_binding_contract(contract: &AgentStageTaskContract) -> String {
    let unit_kinds = crate::evidence_binding::stage_evidence_binding_unit_kinds(&contract.stage_id)
        .into_iter()
        .map(|kind| format!("       - {kind}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "     - Stage-generic evidence binding unit kinds for `{}`:\n{}",
        contract.stage_id, unit_kinds
    )
}

fn render_worker_standard_setting_protocol_for_prompt(task_packet: &TaskPacket) -> String {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return "none".to_string();
    };
    if !is_standard_setting_task(contract) {
        return "none".to_string();
    }
    let artifact_path = standard_setting_candidate_artifact_path(contract);
    format!(
        "Stage-standard-setting task protocol:\n\
     - Produce a strict stage acceptance rubric and a separate `## CandidateStageEvidencePlan` section in a clean task-local candidate file named `{artifact_path}`.\n\
     - This task is not complete if no safe candidate artifact file is written; the final Markdown message is a receipt and summary, not a substitute for the rubric artifact.\n\
     - Do not write the candidate artifact under `.pmcli`; `.pmcli` paths are runtime/input provenance, not worker-created outputs.\n\
     - If this is a repair of a mounted prior rubric, preserve valid prior content by inspecting the mounted input. If `read_file` is truncated, continue with `read_file` `offset`/`max_chars` ranges before claiming all valid content was preserved.\n\
     - The candidate plan is not adopted. It is evidence for the main agent to inspect, raise, reject, or pass to `record_stage_evidence_plan`.\n\
     - In `CandidateStageEvidencePlan`, include `stage_id`, `stage_execution_id`, `plan_source_refs`, `rationale`, and an `evidence_requirements` list.\n\
     - For every evidence requirement, include exactly these fields in readable Markdown or JSON-like blocks: `task_type`, `worker_role`, `objective`, `required_output_artifact_type`, `required_output_fields`, `acceptance_checks`, `failure_signals`, and `evidence_standard`.\n\
     - Do not publish board tasks, mark required evidence as satisfied, accept worker artifacts, decide review readiness, or say the stage can pass.\n\
     - If evidence requirements are uncertain, make them stricter and label the uncertainty instead of omitting the requirement."
    )
}

fn is_standard_setting_task(contract: &AgentStageTaskContract) -> bool {
    contract.task_type == "acceptance standard setting"
        || contract.worker_role == "stage_standard_setter"
        || contract
            .required_output_artifact_type
            .contains("stage_acceptance_rubric")
}

fn standard_setting_candidate_artifact_path(contract: &AgentStageTaskContract) -> String {
    let stage = contract
        .stage_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if stage.is_empty() {
        "stage_acceptance_rubric.md".to_string()
    } else {
        format!("stage_acceptance_rubric_{stage}.md")
    }
}

fn is_synthesis_task_type(task_type: &str) -> bool {
    let normalized = task_type
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>();
    normalized.contains("synthesis") || normalized.contains("artifact repair")
}

fn render_worker_skill_refs_for_prompt(task_packet: &TaskPacket) -> String {
    if task_packet.skill_refs.is_empty() {
        return "none".to_string();
    }
    task_packet
        .skill_refs
        .iter()
        .map(|skill| {
            format!(
                "### {}\nsource: `{}`\n\n{}",
                skill.skill_id,
                skill.source,
                skill.content.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_worker_input_artifacts_for_prompt(
    data_dir: &Path,
    workspace_root: &Path,
    task_packet: &TaskPacket,
) -> String {
    if let Some(manifest) = read_worker_input_mount_manifest(workspace_root) {
        return render_worker_input_mount_manifest_for_prompt(&manifest);
    }
    let input_refs = task_packet
        .stage_task_contract
        .as_ref()
        .map(|contract| contract.input_artifact_refs.as_slice())
        .unwrap_or(&[]);
    if input_refs.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::new();
    for input_ref in input_refs {
        if let Some(resolved) = resolve_input_artifact_ref(data_dir, workspace_root, input_ref) {
            let mounted = resolved
                .source_paths
                .iter()
                .filter_map(|path| path.strip_prefix(workspace_root).ok())
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            if !mounted.is_empty() {
                lines.push(format!(
                    "- input_ref: `{}` kind: `{}` mounted_paths: `{}` status: `available`",
                    input_ref,
                    resolved.bundle_kind,
                    mounted.join(", ")
                ));
                continue;
            }
        }
        lines.push(format!(
            "- input_ref: `{}` mounted_path: `unmounted` status: `unresolved_or_symbolic_ref`",
            input_ref
        ));
    }
    lines.join("\n")
}

fn render_worker_input_mount_manifest_for_prompt(
    manifest: &AgentInputArtifactMountManifest,
) -> String {
    let visibility = &manifest.workspace_visibility;
    let visible_entries = if visibility.visible_root_entries.is_empty() {
        "none".to_string()
    } else {
        visibility.visible_root_entries.join(", ")
    };
    let tracked_entries = if visibility.tracked_root_entries.is_empty() {
        "none".to_string()
    } else {
        visibility.tracked_root_entries.join(", ")
    };
    let project_markers = if visibility.project_marker_paths_visible.is_empty() {
        "none".to_string()
    } else {
        visibility.project_marker_paths_visible.join(", ")
    };
    let unmounted = if manifest.unmounted_input_refs.is_empty() {
        "none".to_string()
    } else {
        manifest
            .unmounted_input_refs
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = vec![
        "- manifest: `.pmcli/worker-input-artifacts.json`".to_string(),
        format!(
            "- workspace_visibility_status: `{}`",
            empty_as_unknown(&visibility.project_marker_status)
        ),
        format!("- visible_root_entries: `{visible_entries}`"),
        format!("- tracked_root_entries: `{tracked_entries}`"),
        format!("- project_marker_paths_visible: `{project_markers}`"),
        format!("- mounted_input_count: `{}`", manifest.mounted_inputs.len()),
        format!(
            "- unmounted_input_ref_count: `{}`",
            manifest.unmounted_input_refs.len()
        ),
        format!("- unmounted_input_refs: `{unmounted}`"),
        "- mounted inputs:".to_string(),
    ];
    if manifest.mounted_inputs.is_empty() {
        lines.push("  - none".to_string());
    } else {
        for input in manifest.mounted_inputs.iter().take(20) {
            let bundled = if input.bundled_paths.is_empty() {
                "none".to_string()
            } else {
                input
                    .bundled_paths
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            lines.push(format!(
                "  - input_ref: `{}` mounted_path: `{}` bundled_paths: `{}` status: `available`",
                input.input_ref, input.mounted_path, bundled
            ));
        }
        if manifest.mounted_inputs.len() > 20 {
            lines.push(format!(
                "  - ... {} more mounted inputs",
                manifest.mounted_inputs.len() - 20
            ));
        }
    }
    if !visibility.notes.is_empty() {
        lines.push("- visibility_notes:".to_string());
        for note in visibility.notes.iter().take(4) {
            lines.push(format!("  - {}", note.trim()));
        }
    }
    lines.join("\n")
}

fn empty_as_unknown(value: &str) -> &str {
    if value.trim().is_empty() {
        "unknown"
    } else {
        value
    }
}

fn comma_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn render_worker_role_soul_system_context(
    workspace_root: &Path,
    task_packet: &TaskPacket,
) -> String {
    match crate::session::context_pack::resolve_soul(
        workspace_root,
        std::env::var("HOME").ok().map(PathBuf::from).as_deref(),
        Some(&task_packet.role_profile),
    ) {
        Ok(soul) => soul.render_section(),
        Err(err) => format!(
            "<soul-context degraded=\"true\">\nRole Soul assembly failed: {err}\n</soul-context>"
        ),
    }
}

fn render_provider_worker_evidence(
    task_packet: &TaskPacket,
    provider_id: &str,
    model: &str,
    worker_output: &str,
    validation_errors: &[String],
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
    tool_calls_made: usize,
    tool_receipts: Option<&AgentToolReceiptManifest>,
) -> String {
    let validation_section = if validation_errors.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Runtime Output Validation\n\n{}\n",
            validation_errors
                .iter()
                .map(|error| format!("- invalid: {}", error.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let mechanical_report = render_provider_worker_mechanical_report(
        task_packet,
        worker_output,
        validation_errors,
        worktree_candidates,
        tool_calls_made,
    );
    let input_receipt_section =
        render_provider_worker_input_receipt_section(task_packet, worktree_candidates);
    let tool_receipt_section = render_provider_worker_tool_receipt_section(tool_receipts);
    format!(
        "# Provider-Backed Agent-Team Worker Evidence\n\n\
         - agent_id: `{}`\n\
         - task_packet_id: `{}`\n\
         - runner_kind: `provider_agent_worker`\n\
         - provider: `{}`\n\
         - model: `{}`\n\
         - authority_scope: `worker_evidence_only`\n\
         - role_profile: `{}`\n\
         - intent: {}\n\n\
         ## Worker Output\n\n{}{}{}{}\n\n{}\n",
        task_packet.agent_id,
        task_packet.task_packet_id,
        provider_id,
        model,
        task_packet.role_profile,
        task_packet.intent,
        worker_output.trim(),
        validation_section,
        input_receipt_section,
        tool_receipt_section,
        mechanical_report
    )
}

fn render_provider_worker_tool_receipt_section(
    tool_receipts: Option<&AgentToolReceiptManifest>,
) -> String {
    let Some(manifest) = tool_receipts else {
        return String::new();
    };
    let mut lines = vec![
        "\n\n## Runtime Tool Receipts".to_string(),
        "- manifest_schema: `agent_tool_receipt_manifest.v1`".to_string(),
        format!("- receipt_count: `{}`", manifest.receipt_count),
        "- semantic_result_owner: `main_agent_or_reviewer`".to_string(),
        "- runtime_boundary: `tool_call_facts_only`".to_string(),
    ];
    if let Some(session_ref) = manifest.session_ref.as_ref() {
        lines.push(format!("- session_ref: `{}`", session_ref));
    }
    for receipt in manifest.receipts.iter().take(12) {
        lines.push(format!(
            "\n### Tool Receipt {}: `{}`",
            receipt.call_index, receipt.tool_name
        ));
        if let Some(call_id) = receipt.call_id.as_ref() {
            lines.push(format!("- call_id: `{}`", call_id));
        }
        lines.push(format!("- status: `{}`", receipt.status));
        lines.push(format!("- arguments: `{}`", receipt.arguments));
        lines.push(format!("- output_bytes: `{}`", receipt.output_bytes));
        lines.push(format!("- output_sha256: `{}`", receipt.output_sha256));
        if let Some(snapshot_ref) = receipt.snapshot_ref.as_ref() {
            lines.push(format!("- snapshot_ref: `{}`", snapshot_ref));
            lines.push(format!("- snapshot_bytes: `{}`", receipt.snapshot_bytes));
            lines.push(format!(
                "- snapshot_truncated: `{}`",
                receipt.snapshot_truncated
            ));
        }
        if let Some(summary) = receipt.structured_summary.as_ref() {
            lines.push(format!("- structured_summary: `{}`", summary));
        }
        if !receipt.output_preview.trim().is_empty() {
            lines.push(format!(
                "- output_preview:\n\n```text\n{}\n```",
                receipt.output_preview.trim()
            ));
        }
    }
    if manifest.receipts.len() > 12 {
        lines.push(format!(
            "\n- omitted_receipts: `{}`",
            manifest.receipts.len() - 12
        ));
    }
    lines.join("\n")
}

fn render_provider_worker_input_receipt_section(
    task_packet: &TaskPacket,
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
) -> String {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return String::new();
    };
    if !contract.consumed_input_required {
        return String::new();
    }
    let refs = provider_worker_contract_input_refs_for_report(task_packet);
    let mut lines = vec![
        "\n\n## Runtime Input Mount Receipt".to_string(),
        "- manifest: `.pmcli/worker-input-artifacts.json`".to_string(),
    ];
    if let Some(manifest) =
        read_worker_input_mount_manifest(Path::new(&worktree_candidates.worktree_path))
    {
        let visibility = &manifest.workspace_visibility;
        lines.push(format!(
            "- workspace_visibility_status: `{}`",
            empty_as_unknown(&visibility.project_marker_status)
        ));
        lines.push(format!(
            "- visible_root_entries: `{}`",
            comma_or_none(&visibility.visible_root_entries)
        ));
        lines.push(format!(
            "- tracked_root_entries: `{}`",
            comma_or_none(&visibility.tracked_root_entries)
        ));
        lines.push(format!(
            "- project_marker_paths_visible: `{}`",
            comma_or_none(&visibility.project_marker_paths_visible)
        ));
        lines.push(format!(
            "- mounted_input_count: `{}`",
            manifest.mounted_inputs.len()
        ));
        lines.push(format!(
            "- unmounted_input_ref_count: `{}`",
            manifest.unmounted_input_refs.len()
        ));
        if !manifest.unmounted_input_refs.is_empty() {
            lines.push(format!(
                "- unmounted_input_refs: `{}`",
                manifest
                    .unmounted_input_refs
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if refs.is_empty() {
        lines.push("- consumed_input_required: `true`".to_string());
    } else {
        lines.push("- mechanically accepted refs:".to_string());
        for reference in refs.iter().take(12) {
            lines.push(format!("  - `{reference}`"));
        }
        if refs.len() > 12 {
            lines.push(format!("  - ... {} more", refs.len() - 12));
        }
    }
    lines.join("\n")
}

fn render_provider_worker_mechanical_report(
    task_packet: &TaskPacket,
    worker_output: &str,
    validation_errors: &[String],
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
    tool_calls_made: usize,
) -> String {
    let safe_candidate_files = worktree_candidates
        .candidate_entries
        .iter()
        .filter(|entry| {
            entry.safe_status == "safe" && entry.path_kind == "file" && !entry.from_input_bundle
        })
        .collect::<Vec<_>>();
    let candidate_total_bytes = safe_candidate_files
        .iter()
        .map(|entry| entry.size_bytes)
        .sum::<u64>();
    let unsafe_candidate_count = worktree_candidates
        .candidate_entries
        .iter()
        .filter(|entry| entry.safe_status != "safe")
        .count();
    let manifest_complete = !worktree_candidates.schema_version.trim().is_empty()
        && !worktree_candidates.agent_id.trim().is_empty()
        && !worktree_candidates.authority_scope.trim().is_empty()
        && !worktree_candidates.adoption_status.trim().is_empty()
        && !worktree_candidates.workspace_binding_ref.trim().is_empty()
        && !worktree_candidates.generated_at.trim().is_empty();
    let path_safety_ok = unsafe_candidate_count == 0;
    let hash_applicable = !safe_candidate_files.is_empty();
    let hash_recorded = hash_applicable
        && safe_candidate_files
            .iter()
            .all(|entry| !entry.sha256.trim().is_empty() && entry.candidate_archive_ref.is_some());
    let input_refs = provider_worker_contract_input_refs_for_report(task_packet);
    let input_ref_mention_count =
        provider_worker_input_ref_mention_count(worker_output, &input_refs);
    let input_mount_manifest =
        read_worker_input_mount_manifest(Path::new(&worktree_candidates.worktree_path));
    let workspace_visibility_status = input_mount_manifest
        .as_ref()
        .map(|manifest| empty_as_unknown(&manifest.workspace_visibility.project_marker_status))
        .unwrap_or("unknown");
    let visible_root_entry_count = input_mount_manifest
        .as_ref()
        .map(|manifest| manifest.workspace_visibility.visible_root_entries.len())
        .unwrap_or(0);
    let project_marker_count = input_mount_manifest
        .as_ref()
        .map(|manifest| {
            manifest
                .workspace_visibility
                .project_marker_paths_visible
                .len()
        })
        .unwrap_or(0);
    let mounted_input_count = input_mount_manifest
        .as_ref()
        .map(|manifest| manifest.mounted_inputs.len())
        .unwrap_or(0);
    let unmounted_input_ref_count = input_mount_manifest
        .as_ref()
        .map(|manifest| manifest.unmounted_input_refs.len())
        .unwrap_or(0);
    format!(
        "## Runtime Mechanical Report\n\n\
         - validation_status: `{}`\n\
         - output_bytes: `{}`\n\
         - output_line_count: `{}`\n\
         - candidate_file_count: `{}`\n\
         - candidate_total_bytes: `{}`\n\
         - unsafe_candidate_count: `{}`\n\
         - input_ref_count: `{}`\n\
         - input_ref_mention_count: `{}`\n\
         - mounted_input_count: `{}`\n\
         - unmounted_input_ref_count: `{}`\n\
         - workspace_visibility_status: `{}`\n\
         - visible_root_entry_count: `{}`\n\
         - project_marker_path_count: `{}`\n\
         - tool_call_count: `{}`\n\
         - manifest_complete: `{}`\n\
         - path_safety_ok: `{}`\n\
         - hash_applicable: `{}`\n\
         - hash_recorded: `{}`\n\
         - empty_output_detected: `{}`\n\
         - prompt_echo_detected: `{}`\n\
         - runtime_diagnostic_detected: `{}`\n\
         - semantic_result_owner: `main_agent_or_reviewer`\n\
         - runtime_boundary: `quantitative_delivery_facts_only`",
        if validation_errors.is_empty() {
            "valid"
        } else {
            "invalid"
        },
        worker_output.len(),
        worker_output.lines().count(),
        safe_candidate_files.len(),
        candidate_total_bytes,
        unsafe_candidate_count,
        input_refs.len(),
        input_ref_mention_count,
        mounted_input_count,
        unmounted_input_ref_count,
        workspace_visibility_status,
        visible_root_entry_count,
        project_marker_count,
        tool_calls_made,
        manifest_complete,
        path_safety_ok,
        hash_applicable,
        hash_recorded,
        worker_output.trim().is_empty(),
        provider_worker_prompt_echo_detected(worker_output),
        provider_worker_runtime_diagnostic_detected(worker_output),
    )
}

fn provider_worker_contract_input_refs_for_report(task_packet: &TaskPacket) -> Vec<String> {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return Vec::new();
    };
    let mut refs = Vec::new();
    refs.extend(contract.input_artifact_refs.iter().cloned());
    refs.extend(contract.depends_on_task_ids.iter().cloned());
    refs.extend(contract.review_target_task_ids.iter().cloned());
    refs.extend(contract.review_target_evidence_refs.iter().cloned());
    refs.extend(contract.blocker_refs.iter().cloned());
    refs.extend(contract.review_findings_refs.iter().cloned());
    if let Some(current_evidence_set_id) = contract.current_evidence_set_id.as_ref() {
        refs.push(current_evidence_set_id.clone());
    }
    refs.retain(|reference| !reference.trim().is_empty());
    refs.sort();
    refs.dedup();
    refs
}

fn provider_worker_input_ref_mention_count(worker_output: &str, refs: &[String]) -> usize {
    let lower = worker_output.to_ascii_lowercase();
    let mut count = 0;
    if lower.contains(".pmcli/worker-input-artifacts.json")
        || lower.contains("worker-input-artifacts")
        || lower.contains("bundle-manifest.json")
    {
        count += 1;
    }
    for reference in refs {
        let trimmed = reference.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mentioned = worker_output.contains(trimmed)
            || lower.contains(&trimmed.to_ascii_lowercase())
            || consumed_input_ref_tail(trimmed)
                .map(|tail| lower.contains(&tail.to_ascii_lowercase()))
                .unwrap_or(false);
        if mentioned {
            count += 1;
        }
    }
    count
}

fn provider_worker_prompt_echo_detected(worker_output: &str) -> bool {
    let lower = worker_output.to_ascii_lowercase();
    [
        "astra collaboration protocol",
        "worker authority boundary",
        "system role soul",
        "structured authority tools",
        "you are astra",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn provider_worker_runtime_diagnostic_detected(worker_output: &str) -> bool {
    let lower = worker_output.to_ascii_lowercase();
    (lower.contains("runtime diagnostic") && lower.contains("not a completed research artifact"))
        || lower.contains("main agent output rejected by runtime boundary")
        || lower.contains("provider_finalize_failed")
}

fn validate_provider_worker_final_output(
    task_packet: &TaskPacket,
    worker_output: &str,
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
) -> Vec<String> {
    let mut errors = Vec::new();
    let validation_text = provider_worker_validation_text(worker_output, Some(worktree_candidates));

    let task_ref = task_packet
        .stage_task_contract
        .as_ref()
        .map(|contract| contract.task_id.as_str())
        .unwrap_or(task_packet.task_packet_id.as_str());
    if worker_output.trim().is_empty() {
        errors.push(format!(
            "provider worker `{}` completed task `{}` without final Markdown evidence text; delegated workers must return inspectable evidence after tool use",
            task_packet.agent_id, task_ref
        ));
    }
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return errors;
    };
    if contract.consumed_input_required
        && !worker_output_refs_consumed_input(contract, &validation_text)
    {
        errors.push(format!(
            "provider worker `{}` completed task `{}` without explicit consumed-input evidence refs; tasks with consumed_input_required=true must cite at least one mounted input ref, dependency task id, review target, current evidence set id, or `.pmcli/worker-input-artifacts.json`",
            task_packet.agent_id, task_ref
        ));
    }
    let unsafe_candidate_files = worktree_candidates
        .candidate_entries
        .iter()
        .filter(|entry| entry.safe_status != "safe" && entry.path_kind == "file")
        .collect::<Vec<_>>();
    if !unsafe_candidate_files.is_empty() {
        errors.push(format!(
            "provider worker `{}` completed task `{}` with {} unsafe candidate file(s); worker-created outputs must be safe task-local files outside runtime/input-bundle paths",
            task_packet.agent_id,
            task_ref,
            unsafe_candidate_files.len()
        ));
    }
    if provider_worker_requires_safe_candidate_artifact(contract)
        && !worktree_candidate_manifest_has_safe_file(worktree_candidates)
    {
        errors.push(format!(
            "provider worker `{}` completed task `{}` without a safe task-local candidate artifact file for required output artifact type `{}`; final prose evidence cannot substitute for the required artifact delivery",
            task_packet.agent_id,
            task_ref,
            contract.required_output_artifact_type
        ));
    }
    errors
}

fn validate_provider_worker_tool_action_delivery(
    task_packet: &TaskPacket,
    tool_actions: &[AgentTraceEvent],
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
) -> Vec<String> {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return Vec::new();
    };
    if !provider_worker_requires_safe_candidate_artifact(contract)
        || worktree_candidate_manifest_has_safe_file(worktree_candidates)
    {
        return Vec::new();
    }
    let failed_write_file_count = tool_actions
        .iter()
        .filter(|event| {
            event.event == "provider_tool_completed"
                && event.detail.contains("tool=write_file")
                && event.detail.contains("status=error")
        })
        .count();
    if failed_write_file_count == 0 {
        return Vec::new();
    }
    vec![format!(
        "provider worker `{}` completed task `{}` after {} failed write_file attempt(s) and no safe task-local candidate artifact file was captured for required output artifact type `{}`",
        task_packet.agent_id,
        contract.task_id,
        failed_write_file_count,
        contract.required_output_artifact_type
    )]
}

fn provider_worker_validation_text(
    worker_output: &str,
    worktree_candidates: Option<&AgentWorktreeArtifactCandidateManifest>,
) -> String {
    let Some(worktree_candidates) = worktree_candidates else {
        return worker_output.to_string();
    };
    let mut text = worker_output.to_string();
    for entry in &worktree_candidates.candidate_entries {
        if entry.safe_status != "safe" || entry.path_kind != "file" || entry.from_input_bundle {
            continue;
        }
        let Some(archive_ref) = entry.candidate_archive_ref.as_ref() else {
            continue;
        };
        let Ok(contents) = fs::read_to_string(archive_ref) else {
            continue;
        };
        if contents.trim().is_empty() {
            continue;
        }
        text.push_str("\n\n## Runtime Captured Candidate Artifact\n\n");
        text.push_str("- candidate_path: `");
        text.push_str(&entry.relative_path);
        text.push_str("`\n\n");
        text.push_str(&contents.chars().take(64 * 1024).collect::<String>());
    }
    text
}

fn worktree_candidate_manifest_has_safe_file(
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
) -> bool {
    worktree_candidates.candidate_entries.iter().any(|entry| {
        entry.safe_status == "safe"
            && entry.path_kind == "file"
            && !entry.from_input_bundle
            && !entry.relative_path.trim().is_empty()
            && entry.candidate_archive_ref.is_some()
    })
}

fn provider_worker_requires_safe_candidate_artifact(contract: &AgentStageTaskContract) -> bool {
    is_standard_setting_task(contract)
        || contract.required_output_artifact_type == "source_verification_ledger"
        || matches!(
            contract.task_type.as_str(),
            "source verification" | "citation verification"
        )
}

fn render_provider_worker_partial_candidate_evidence(
    task_packet: &TaskPacket,
    provider_id: &str,
    model: &str,
    failure: &str,
    worktree_candidates: &AgentWorktreeArtifactCandidateManifest,
) -> String {
    let mut text = format!(
        "# Provider-Backed Agent-Team Worker Candidate Evidence\n\n\
- agent_id: `{}`\n\
- task_packet_id: `{}`\n\
- runner_kind: `provider_agent_worker`\n\
- provider: `{provider_id}`\n\
- model: `{model}`\n\
- authority_scope: `worker_evidence_only`\n\
- role_profile: `{}`\n\
- provider_finalize_failed: `true`\n\
- provider_failure: {}\n\n\
## Runtime Note\n\n\
The provider failed while finalizing the worker run, but the runtime captured safe candidate files written by the worker before the provider fault. These files are candidate-only evidence for main-agent and reviewer inspection; this note is not a scientific acceptance decision.\n\n\
## Candidate Artifact Refs\n",
        task_packet.agent_id,
        task_packet.task_packet_id,
        task_packet.role_profile,
        failure.trim()
    );
    for entry in &worktree_candidates.candidate_entries {
        if entry.safe_status != "safe" || entry.path_kind != "file" || entry.from_input_bundle {
            continue;
        }
        text.push_str(&format!(
            "- `{}` ({}, {} bytes, sha256 `{}`)",
            entry.relative_path, entry.artifact_kind, entry.size_bytes, entry.sha256
        ));
        if let Some(archive_ref) = entry.candidate_archive_ref.as_ref() {
            text.push_str(&format!("; archive `{archive_ref}`"));
        }
        text.push('\n');
    }
    text.push_str(
        "\n## Required Review Boundary\n\n\
- Main agent must decide whether to accept, reject, retry, or route repair for these candidate artifacts.\n\
- Runtime only confirms that candidate files were captured and were not input-bundle files.\n\
- Stage review must still judge whether the candidate satisfies the task contract.\n",
    );
    text
}

fn worker_output_refs_consumed_input(
    contract: &AgentStageTaskContract,
    worker_output: &str,
) -> bool {
    let lower = worker_output.to_ascii_lowercase();
    if lower.contains(".pmcli/worker-input-artifacts.json")
        || lower.contains("worker-input-artifacts")
        || lower.contains("bundle-manifest.json")
    {
        return true;
    }
    let refs = contract
        .input_artifact_refs
        .iter()
        .chain(contract.depends_on_task_ids.iter())
        .chain(contract.review_target_task_ids.iter())
        .chain(contract.review_target_evidence_refs.iter())
        .chain(contract.blocker_refs.iter())
        .chain(contract.review_findings_refs.iter())
        .chain(contract.current_evidence_set_id.iter())
        .collect::<Vec<_>>();
    refs.iter().any(|reference| {
        let trimmed = reference.trim();
        !trimmed.is_empty()
            && (worker_output.contains(trimmed)
                || lower.contains(&trimmed.to_ascii_lowercase())
                || consumed_input_ref_tail(trimmed)
                    .map(|tail| lower.contains(&tail.to_ascii_lowercase()))
                    .unwrap_or(false))
    })
}

fn is_open_problem_extraction_task(contract: &AgentStageTaskContract) -> bool {
    let normalized = contract
        .task_type
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>();
    normalized.contains("open")
        && normalized.contains("problem")
        && normalized.contains("extraction")
}

fn consumed_input_ref_tail(reference: &str) -> Option<&str> {
    reference
        .rsplit([':', '/', '\\'])
        .find(|part| !part.trim().is_empty())
}

fn role_kind_from_profile(role_profile: &str) -> &str {
    let lowered = role_profile.to_ascii_lowercase();
    match lowered.as_str() {
        "stage_standard_setter" => "stage_standard_setter",
        "goal_dispatcher" => "goal_dispatcher",
        "research_operator" => "research_operator",
        "research_worker" => "research_worker",
        "research_synthesizer" => "research_synthesizer",
        "literature_researcher" => "literature_researcher",
        "citation_auditor" => "citation_auditor",
        "literature_comparison_researcher" => "literature_comparison_researcher",
        "literature_gap_analyst" => "literature_gap_analyst",
        "research_planner" => "research_planner",
        "research_critic" => "research_critic",
        "novelty_reviewer" => "novelty_reviewer",
        "experiment_designer" => "experiment_designer",
        "code_reviewer" => "code_reviewer",
        "experiment_operator" => "experiment_operator",
        "result_analyst" => "result_analyst",
        "claim_auditor" => "claim_auditor",
        "paper_planner" => "paper_planner",
        "latex_writer" => "latex_writer",
        "paper_build_operator" => "paper_build_operator",
        "hard_reviewer" => "hard_reviewer",
        "reviewer" => "reviewer",
        "repair_router" => "repair_router",
        "process_diagnostician" => "process_diagnostician",
        "implementation_worker" => "implementation_worker",
        "literature_repair_worker" => "literature_repair_worker",
        "novelty_repair_reviewer" => "novelty_repair_reviewer",
        "experiment_plan_repair_worker" => "experiment_plan_repair_worker",
        "implementation_repair_worker" => "implementation_repair_worker",
        "experiment_run_repair_worker" => "experiment_run_repair_worker",
        "result_analysis_repair_worker" => "result_analysis_repair_worker",
        "claim_repair_auditor" => "claim_repair_auditor",
        "latex_repair_writer" => "latex_repair_writer",
        "paper_build_repair_operator" => "paper_build_repair_operator",
        "hard_review_repair_router" => "hard_review_repair_router",
        "stage_repair_worker" => "stage_repair_worker",
        "repair_worker" => "repair_worker",
        "goal_worker" => "goal_worker",
        _ if lowered.contains("synthes") => "research_synthesizer",
        _ if lowered.contains("review") => "reviewer",
        _ if lowered.contains("implement")
            || lowered.contains("code")
            || lowered.contains("experiment") =>
        {
            "implementation_worker"
        }
        _ if lowered.contains("literature")
            || lowered.contains("research")
            || lowered.contains("paper") =>
        {
            "research_worker"
        }
        _ if lowered.contains("repair") => "repair_worker",
        _ if lowered.contains("main") => "main_agent",
        _ => "goal_worker",
    }
}

fn resolve_worker_role_package(
    workspace_root: &Path,
    role_profile: &str,
) -> Result<crate::session::role_package::RolePackage, AgentError> {
    crate::session::role_package::resolve_role_package(
        workspace_root,
        std::env::var("HOME").ok().map(PathBuf::from).as_deref(),
        role_profile,
        role_kind_from_profile(role_profile),
    )
    .map_err(|err| AgentError::InvalidTaskPacket {
        reason: err.to_string(),
    })
}

fn role_package_skill_refs(
    refs: Vec<crate::session::role_package::RolePackageSkillRef>,
) -> Vec<AgentSkillRef> {
    refs.into_iter()
        .map(|skill| AgentSkillRef {
            skill_id: skill.skill_id,
            source: skill.source,
            content: skill.content,
        })
        .collect()
}

fn role_package_tool_policy(
    policy: crate::session::role_package::RolePackageToolPolicy,
) -> AgentToolPolicy {
    AgentToolPolicy {
        schema_version: policy.schema_version,
        allowed_tools: policy.allowed_tools,
    }
}

fn effective_worker_tool_policy(packet: &TaskPacket) -> AgentToolPolicy {
    packet.tool_policy.clone().unwrap_or_else(|| {
        let role_package = resolve_worker_role_package(
            Path::new(&packet.scope.workspace_root),
            &packet.role_profile,
        );
        role_package
            .map(|package| role_package_tool_policy(package.tool_policy))
            .unwrap_or_else(|_| AgentToolPolicy {
                schema_version: "agent_tool_policy.v1".to_string(),
                allowed_tools: vec![
                    "read_file".to_string(),
                    "list_files".to_string(),
                    "search".to_string(),
                    "fetch".to_string(),
                    "write_file".to_string(),
                    "worker_shell".to_string(),
                ],
            })
    })
}

fn provider_worker_tool_definitions(packet: &TaskPacket) -> Vec<crate::tools::ToolDefinition> {
    let policy = effective_worker_tool_policy(packet);
    crate::tools::agent_team_worker_tool_definitions_for_policy(&policy.allowed_tools)
}

fn delegated_worker_tool_scope(packet: &TaskPacket) -> Vec<String> {
    match packet.runner_kind.as_deref() {
        Some("local") => vec!["local_command".to_string(), "git_worktree".to_string()],
        Some("mock") => vec!["mock_lifecycle".to_string()],
        Some(other) => vec![other.to_string()],
        None => Vec::new(),
    }
}

fn default_agent_collaboration_protocol() -> AgentCollaborationProtocol {
    AgentCollaborationProtocol {
        schema_version: "agent_collaboration_protocol.v1".to_string(),
        protocol_id: "astra_main_runtime_worker_v1".to_string(),
        main_agent_authority: "owns research strategy, board task publication, route changes, cleanup decisions, review readiness, and candidate artifact adoption decisions".to_string(),
        runtime_authority: "persists state, enforces schemas and tool scope, dispatches published tasks, records evidence, and projects status without inventing research decisions".to_string(),
        worker_authority: "executes only the assigned TaskPacket, gathers evidence, writes task-local artifacts in its isolated worktree, and reports repair needs without changing stage route or canonical project口径".to_string(),
        required_worker_capabilities: vec![
            "read assigned context and input artifacts".to_string(),
            "use scoped tools to gather source-grounded or experiment-grounded evidence".to_string(),
            "produce the required output artifact type and required fields".to_string(),
            "surface missing evidence, blocked dependencies, and repair tasks explicitly".to_string(),
            "keep candidate artifacts isolated until main-agent decision and review".to_string(),
        ],
        required_worker_output_sections: vec![
            "task contract summary".to_string(),
            "evidence produced".to_string(),
            "tool-backed source or experiment refs".to_string(),
            "candidate artifact refs".to_string(),
            "acceptance-check coverage".to_string(),
            "failure signals observed or avoided".to_string(),
            "repair tasks or missing-evidence risks".to_string(),
        ],
        forbidden_worker_actions: vec![
            "publish or mutate board tasks".to_string(),
            "close obligations".to_string(),
            "request route changes".to_string(),
            "request cleanup plans".to_string(),
            "mark a stage complete".to_string(),
            "apply worker artifacts to canonical workspace".to_string(),
            "invent citations, experiments, files, or results".to_string(),
        ],
        artifact_flow: "main_agent publishes TaskPacket -> runtime dispatches worker -> worker returns evidence and candidate-only worktree artifacts -> runtime records refs -> main_agent decides accept/reject/defer -> strict review and cleanup gates decide canonical adoption".to_string(),
        adoption_rule: "worker artifacts remain candidate-only until the main agent records an explicit worker artifact decision and any required review/adoption path applies them".to_string(),
        review_rule: "stage advancement requires accepted worker evidence to satisfy the stage contract and pass strict review; prose-only summaries cannot substitute for missing worker evidence".to_string(),
        cleanup_rule: "runtime may record cleanup requests, but only main-agent route or project口径 decisions decide when cleanup is required".to_string(),
    }
}

fn provider_worker_tool_scope(packet: &TaskPacket) -> Vec<String> {
    provider_worker_tool_definitions(packet)
        .into_iter()
        .map(|definition| definition.name)
        .collect()
}

fn render_collaboration_protocol_for_prompt(packet: &TaskPacket) -> String {
    packet
        .collaboration_protocol
        .as_ref()
        .and_then(|protocol| serde_json::to_string_pretty(protocol).ok())
        .unwrap_or_else(|| "none".to_string())
}

fn sanitize_runtime_id_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
}

struct LocalCommandRun {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: Option<ExitStatus>,
    timed_out: bool,
}

fn run_local_command(
    command: &str,
    cwd: &Path,
    max_runtime_ms: u64,
) -> Result<LocalCommandRun, AgentError> {
    let mut command_builder = Command::new("sh");
    command_builder
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_child_process_group(&mut command_builder);
    let mut child = command_builder
        .spawn()
        .map_err(|err| AgentError::LocalCommand(err.to_string()))?;

    let mut stdout = child.stdout.take().ok_or_else(|| {
        AgentError::LocalCommand("local runner stdout pipe was unavailable".to_string())
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        AgentError::LocalCommand("local runner stderr pipe was unavailable".to_string())
    })?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });

    let deadline = Instant::now() + Duration::from_millis(max_runtime_ms);
    let mut timed_out = false;
    let status = loop {
        match child
            .try_wait()
            .map_err(|err| AgentError::LocalCommand(err.to_string()))?
        {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                timed_out = true;
                kill_child_process_group(&mut child);
                break child
                    .wait()
                    .map_err(|err| AgentError::LocalCommand(err.to_string()))?;
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(LocalCommandRun {
        stdout,
        stderr,
        status: Some(status),
        timed_out,
    })
}

#[cfg(unix)]
fn configure_child_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(not(unix))]
fn configure_child_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_child_process_group(child: &mut std::process::Child) {
    let pgid = -(child.id() as i32);
    unsafe {
        libc::kill(pgid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_child_process_group(child: &mut std::process::Child) {
    let _ = child.kill();
}

fn local_failure_code(command_output: &LocalCommandRun) -> String {
    if command_output.timed_out {
        return "timeout".to_string();
    }
    let Some(status) = command_output.status.as_ref() else {
        return "terminated".to_string();
    };
    if status.success() {
        return String::new();
    }
    if let Some(code) = status.code() {
        return format!("exit_{code}");
    }
    if let Some(signal) = exit_signal(status) {
        return format!("signal_{signal}");
    }
    "terminated".to_string()
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "stopped" | "canceled" | "timeout"
    )
}

fn create_git_worktree(
    data_dir: &Path,
    workspace_root: &Path,
    agent_id: &str,
    created_at: &str,
) -> Result<AgentWorkspaceBinding, AgentError> {
    let git_head_output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: "rev-parse".to_string(),
            stderr: err.to_string(),
        })?;
    if !git_head_output.status.success() {
        return Err(AgentError::GitCommand {
            action: "rev-parse".to_string(),
            stderr: String::from_utf8_lossy(&git_head_output.stderr).to_string(),
        });
    }
    let git_head = String::from_utf8_lossy(&git_head_output.stdout)
        .trim()
        .to_string();

    let base = data_dir.join("worktrees").join("agents");
    fs::create_dir_all(&base)?;
    let worktree_path = base.join(agent_id);
    let add_output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["worktree", "add", "--detach"])
        .arg(&worktree_path)
        .arg("HEAD")
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: "worktree add".to_string(),
            stderr: err.to_string(),
        })?;
    if !add_output.status.success() {
        return Err(AgentError::GitCommand {
            action: "worktree add".to_string(),
            stderr: String::from_utf8_lossy(&add_output.stderr).to_string(),
        });
    }
    let dirty_overlay = apply_dirty_worktree_overlay(
        data_dir,
        workspace_root,
        &worktree_path,
        agent_id,
        &git_head,
    )?;
    let applied_baseline = canonical_artifacts::apply_current_baseline_overlay(
        data_dir,
        workspace_root,
        &worktree_path,
    )
    .map_err(|err| AgentError::LocalCommand(format!("canonical baseline overlay failed: {err}")))?;
    let baseline_ref = applied_baseline
        .as_ref()
        .map(|baseline| baseline.baseline_ref.clone())
        .unwrap_or_else(|| git_head.clone());
    let baseline_mode = applied_baseline
        .as_ref()
        .map(|baseline| baseline.baseline_mode.clone())
        .unwrap_or_else(|| {
            if dirty_overlay.applied {
                "dirty_worktree_overlay".to_string()
            } else {
                "initial_head".to_string()
            }
        });
    let overlay_ref = applied_baseline
        .as_ref()
        .map(|baseline| baseline.overlay_ref.clone())
        .or_else(|| dirty_overlay.manifest_ref.clone());
    let ledger_version = canonical_artifacts::load_ledger(data_dir)
        .ok()
        .map(|ledger| ledger.updated_at);

    Ok(AgentWorkspaceBinding {
        schema_version: "v1alpha1".to_string(),
        agent_id: agent_id.to_string(),
        mode: "git_worktree".to_string(),
        source_workspace_root: workspace_root.display().to_string(),
        worktree_path: worktree_path.display().to_string(),
        git_head: git_head.clone(),
        baseline_ref: Some(baseline_ref),
        baseline_mode: Some(baseline_mode),
        canonical_artifact_ledger_ref: Some(
            data_dir
                .join("canonical-artifacts")
                .join("ledger.json")
                .display()
                .to_string(),
        ),
        canonical_artifact_ledger_version: ledger_version,
        overlay_ref,
        dirty_overlay_ref: dirty_overlay.manifest_ref,
        dirty_patch_ref: dirty_overlay.patch_ref,
        untracked_manifest_ref: dirty_overlay.untracked_manifest_ref,
        created_at: created_at.to_string(),
    })
}

#[derive(Debug, Clone)]
struct DirtyOverlayResult {
    applied: bool,
    manifest_ref: Option<String>,
    patch_ref: Option<String>,
    untracked_manifest_ref: Option<String>,
}

fn apply_dirty_worktree_overlay(
    data_dir: &Path,
    workspace_root: &Path,
    worktree_path: &Path,
    agent_id: &str,
    git_head: &str,
) -> Result<DirtyOverlayResult, AgentError> {
    let overlay_dir = agent_dir(data_dir, agent_id).join("source_overlay");
    fs::create_dir_all(&overlay_dir)?;

    let status_entries = git_output_lines(
        workspace_root,
        &["status", "--porcelain", "--untracked-files=normal"],
        "status dirty overlay",
    )?;
    let staged_patch = git_output_bytes(
        workspace_root,
        &["diff", "--cached", "--binary"],
        "diff cached dirty overlay",
    )?;
    let tracked_patch =
        git_output_bytes(workspace_root, &["diff", "--binary"], "diff dirty overlay")?;
    let untracked_paths = git_output_nul_strings(
        workspace_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        "list untracked dirty overlay",
    )?
    .into_iter()
    .filter(|path| dirty_overlay_path_is_allowed(path))
    .collect::<Vec<_>>();

    let staged_patch_ref = write_and_apply_patch_if_present(
        worktree_path,
        &overlay_dir.join("staged.patch"),
        &staged_patch,
        "apply staged dirty overlay",
    )?;
    let tracked_patch_ref = write_and_apply_patch_if_present(
        worktree_path,
        &overlay_dir.join("tracked.patch"),
        &tracked_patch,
        "apply tracked dirty overlay",
    )?;
    let untracked_manifest = copy_untracked_overlay_files(
        workspace_root,
        worktree_path,
        &overlay_dir,
        agent_id,
        &untracked_paths,
    )?;

    let applied = staged_patch_ref.is_some()
        || tracked_patch_ref.is_some()
        || !untracked_manifest.files.is_empty();
    let untracked_manifest_ref = if untracked_manifest.files.is_empty() {
        None
    } else {
        let path = overlay_dir.join("untracked_manifest.json");
        atomic_write(&path, &serde_json::to_string_pretty(&untracked_manifest)?)?;
        Some(path.display().to_string())
    };
    let manifest = AgentDirtyWorkspaceOverlayManifest {
        schema_version: "agent_dirty_workspace_overlay.v1".to_string(),
        agent_id: agent_id.to_string(),
        source_workspace_root: workspace_root.display().to_string(),
        worktree_path: worktree_path.display().to_string(),
        git_head: git_head.to_string(),
        status_entries,
        tracked_patch_ref: tracked_patch_ref
            .as_ref()
            .map(|path| path.display().to_string()),
        tracked_patch_sha256: patch_sha256(&tracked_patch_ref),
        staged_patch_ref: staged_patch_ref
            .as_ref()
            .map(|path| path.display().to_string()),
        staged_patch_sha256: patch_sha256(&staged_patch_ref),
        untracked_manifest_ref: untracked_manifest_ref.clone(),
        untracked_paths,
        applied,
        generated_at: timestamp_string(),
    };
    let manifest_path = overlay_dir.join("manifest.json");
    atomic_write(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;

    Ok(DirtyOverlayResult {
        applied,
        manifest_ref: Some(manifest_path.display().to_string()),
        patch_ref: tracked_patch_ref
            .or(staged_patch_ref)
            .map(|path| path.display().to_string()),
        untracked_manifest_ref,
    })
}

fn git_output_bytes(
    workspace_root: &Path,
    args: &[&str],
    action: &str,
) -> Result<Vec<u8>, AgentError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: action.to_string(),
            stderr: err.to_string(),
        })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(AgentError::GitCommand {
            action: action.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn git_output_lines(
    workspace_root: &Path,
    args: &[&str],
    action: &str,
) -> Result<Vec<String>, AgentError> {
    Ok(
        String::from_utf8_lossy(&git_output_bytes(workspace_root, args, action)?)
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

fn git_output_nul_strings(
    workspace_root: &Path,
    args: &[&str],
    action: &str,
) -> Result<Vec<String>, AgentError> {
    Ok(git_output_bytes(workspace_root, args, action)?
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect())
}

fn write_and_apply_patch_if_present(
    worktree_path: &Path,
    patch_path: &Path,
    patch: &[u8],
    action: &str,
) -> Result<Option<PathBuf>, AgentError> {
    if patch.is_empty() {
        return Ok(None);
    }
    if let Some(parent) = patch_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(patch_path, patch)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["apply", "--binary"])
        .arg(patch_path)
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: action.to_string(),
            stderr: err.to_string(),
        })?;
    if output.status.success() {
        Ok(Some(patch_path.to_path_buf()))
    } else {
        Err(AgentError::GitCommand {
            action: action.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn copy_untracked_overlay_files(
    workspace_root: &Path,
    worktree_path: &Path,
    overlay_dir: &Path,
    agent_id: &str,
    untracked_paths: &[String],
) -> Result<AgentUntrackedWorkspaceFileManifest, AgentError> {
    let mut files = Vec::new();
    for relative_path in untracked_paths {
        let source_path = workspace_root.join(relative_path);
        let target_path = worktree_path.join(relative_path);
        if !source_path.is_file() {
            continue;
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source_path, &target_path)?;
        let bytes = fs::read(&source_path)?;
        files.push(AgentUntrackedWorkspaceFileEntry {
            relative_path: relative_path.clone(),
            size_bytes: bytes.len() as u64,
            sha256: sha256_bytes(&bytes),
        });
    }
    let manifest = AgentUntrackedWorkspaceFileManifest {
        schema_version: "agent_untracked_workspace_files.v1".to_string(),
        agent_id: agent_id.to_string(),
        files,
        generated_at: timestamp_string(),
    };
    if !manifest.files.is_empty() {
        fs::create_dir_all(overlay_dir)?;
    }
    Ok(manifest)
}

fn dirty_overlay_path_is_allowed(path: &str) -> bool {
    let path = Path::new(path);
    if path.is_absolute() {
        return false;
    }
    let mut components = path.components();
    let Some(std::path::Component::Normal(first)) = components.next() else {
        return false;
    };
    let first = first.to_string_lossy();
    !matches!(
        first.as_ref(),
        ".git" | ".pmcli" | ".research-cli-agent-worktrees"
    ) && !path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn patch_sha256(path: &Option<PathBuf>) -> Option<String> {
    path.as_ref()
        .and_then(|path| fs::read(path).ok())
        .map(|bytes| sha256_bytes(&bytes))
}

fn validate_local_task_packet(
    data_dir: &Path,
    workspace_root: &Path,
    packet: &TaskPacket,
) -> Result<(), AgentError> {
    validate_task_packet_common(data_dir, workspace_root, packet)?;
    if packet.runner_kind.as_deref() != Some("local") {
        return Err(AgentError::InvalidTaskPacket {
            reason: "local runner requires runner_kind=local".to_string(),
        });
    }
    if packet.command.as_deref().unwrap_or("").trim().is_empty() {
        return Err(AgentError::InvalidTaskPacket {
            reason: "local runner requires a non-empty command".to_string(),
        });
    }
    if packet.write_authority != "workspace_write" {
        return Err(AgentError::MissingWriteAuthority {
            required: "workspace_write".to_string(),
            actual: packet.write_authority.clone(),
        });
    }
    Ok(())
}

fn validate_provider_task_packet(
    data_dir: &Path,
    workspace_root: &Path,
    packet: &TaskPacket,
) -> Result<(), AgentError> {
    validate_task_packet_common(data_dir, workspace_root, packet)?;
    if packet.runner_kind.as_deref() != Some("provider") {
        return Err(AgentError::InvalidTaskPacket {
            reason: "provider runner requires runner_kind=provider".to_string(),
        });
    }
    if packet.command.is_some() {
        return Err(AgentError::InvalidTaskPacket {
            reason: "provider runner task packets must not include a shell command".to_string(),
        });
    }
    if packet.write_authority != "workspace_write" {
        return Err(AgentError::MissingWriteAuthority {
            required: "workspace_write".to_string(),
            actual: packet.write_authority.clone(),
        });
    }
    Ok(())
}

fn validate_provider_task_packet_for_persisted_run(
    workspace_root: &Path,
    packet: &TaskPacket,
) -> Result<(), AgentError> {
    if packet.runner_kind.as_deref() != Some("provider") {
        return Err(AgentError::InvalidTaskPacket {
            reason: "provider runner requires runner_kind=provider".to_string(),
        });
    }
    if packet.command.is_some() {
        return Err(AgentError::InvalidTaskPacket {
            reason: "provider runner task packets must not include a shell command".to_string(),
        });
    }
    if packet.write_authority != "workspace_write" {
        return Err(AgentError::MissingWriteAuthority {
            required: "workspace_write".to_string(),
            actual: packet.write_authority.clone(),
        });
    }
    if packet.schema_version.trim().is_empty()
        || packet.task_packet_id.trim().is_empty()
        || packet.agent_id.trim().is_empty()
        || packet.intent.trim().is_empty()
        || packet.role_profile.trim().is_empty()
        || packet.message.trim().is_empty()
    {
        return Err(AgentError::InvalidTaskPacket {
            reason: "required identity and intent fields must be non-empty".to_string(),
        });
    }
    if packet.budget.max_turns == 0 || packet.budget.max_runtime_ms == 0 {
        return Err(AgentError::InvalidTaskPacket {
            reason: "budget max_turns and max_runtime_ms must be positive".to_string(),
        });
    }
    if packet.scope.allowed_paths.is_empty() {
        return Err(AgentError::InvalidTaskPacket {
            reason: "scope.allowed_paths must not be empty".to_string(),
        });
    }
    if !packet.output_manifest_required {
        return Err(AgentError::InvalidTaskPacket {
            reason: "output_manifest_required must be true for M6 agents".to_string(),
        });
    }
    if let Some(contract) = packet.stage_task_contract.as_ref() {
        validate_stage_task_contract(contract)?;
    }
    if let Some(protocol) = packet.collaboration_protocol.as_ref() {
        validate_collaboration_protocol(protocol)?;
    }
    validate_skill_refs(&packet.skill_refs)?;
    if let Some(tool_policy) = packet.tool_policy.as_ref() {
        validate_tool_policy(tool_policy)?;
    }
    let expected = workspace_root
        .canonicalize()
        .map_err(|err| AgentError::StaleScope {
            expected: workspace_root.display().to_string(),
            actual: format!("unreadable workspace root: {err}"),
        })?;
    let actual_path = PathBuf::from(&packet.scope.workspace_root);
    let actual = actual_path
        .canonicalize()
        .map_err(|_| AgentError::StaleScope {
            expected: expected.display().to_string(),
            actual: packet.scope.workspace_root.clone(),
        })?;
    if actual != expected {
        return Err(AgentError::StaleScope {
            expected: expected.display().to_string(),
            actual: actual.display().to_string(),
        });
    }
    Ok(())
}

fn validate_task_packet_common(
    data_dir: &Path,
    workspace_root: &Path,
    packet: &TaskPacket,
) -> Result<(), AgentError> {
    if agent_dir(data_dir, &packet.agent_id).exists() {
        return Err(AgentError::InvalidTaskPacket {
            reason: format!("agent id already exists: {}", packet.agent_id),
        });
    }
    if packet.schema_version.trim().is_empty()
        || packet.task_packet_id.trim().is_empty()
        || packet.agent_id.trim().is_empty()
        || packet.intent.trim().is_empty()
        || packet.role_profile.trim().is_empty()
        || packet.message.trim().is_empty()
    {
        return Err(AgentError::InvalidTaskPacket {
            reason: "required identity and intent fields must be non-empty".to_string(),
        });
    }
    if packet.budget.max_turns == 0 || packet.budget.max_runtime_ms == 0 {
        return Err(AgentError::InvalidTaskPacket {
            reason: "budget max_turns and max_runtime_ms must be positive".to_string(),
        });
    }
    if packet.scope.allowed_paths.is_empty() {
        return Err(AgentError::InvalidTaskPacket {
            reason: "scope.allowed_paths must not be empty".to_string(),
        });
    }
    if !packet.output_manifest_required {
        return Err(AgentError::InvalidTaskPacket {
            reason: "output_manifest_required must be true for M6 agents".to_string(),
        });
    }
    if let Some(contract) = packet.stage_task_contract.as_ref() {
        validate_stage_task_contract(contract)?;
    }
    if let Some(protocol) = packet.collaboration_protocol.as_ref() {
        validate_collaboration_protocol(protocol)?;
    }
    validate_skill_refs(&packet.skill_refs)?;
    if let Some(tool_policy) = packet.tool_policy.as_ref() {
        validate_tool_policy(tool_policy)?;
    }
    let expected = workspace_root
        .canonicalize()
        .map_err(|err| AgentError::StaleScope {
            expected: workspace_root.display().to_string(),
            actual: format!("unreadable workspace root: {err}"),
        })?;
    let actual_path = PathBuf::from(&packet.scope.workspace_root);
    let actual = actual_path
        .canonicalize()
        .map_err(|_| AgentError::StaleScope {
            expected: expected.display().to_string(),
            actual: packet.scope.workspace_root.clone(),
        })?;
    if actual != expected {
        return Err(AgentError::StaleScope {
            expected: expected.display().to_string(),
            actual: actual.display().to_string(),
        });
    }

    Ok(())
}

fn validate_skill_refs(skill_refs: &[AgentSkillRef]) -> Result<(), AgentError> {
    for skill in skill_refs {
        if skill.skill_id.trim().is_empty()
            || skill.source.trim().is_empty()
            || skill.content.trim().is_empty()
        {
            return Err(AgentError::InvalidTaskPacket {
                reason: "skill_refs entries must include non-empty skill_id, source, and content"
                    .to_string(),
            });
        }
    }
    Ok(())
}

fn validate_tool_policy(tool_policy: &AgentToolPolicy) -> Result<(), AgentError> {
    if tool_policy.schema_version.trim().is_empty() || tool_policy.allowed_tools.is_empty() {
        return Err(AgentError::InvalidTaskPacket {
            reason: "tool_policy must include schema_version and at least one allowed tool"
                .to_string(),
        });
    }
    let safe_tools = crate::tools::agent_team_worker_safe_tool_names();
    for tool in &tool_policy.allowed_tools {
        let tool = tool.trim();
        if tool.is_empty() || !safe_tools.contains(&tool) {
            return Err(AgentError::InvalidTaskPacket {
                reason: format!(
                    "tool_policy contains non-worker or forbidden tool `{tool}`; workers may only use agent-team worker safe tools"
                ),
            });
        }
    }
    Ok(())
}

fn validate_collaboration_protocol(
    protocol: &AgentCollaborationProtocol,
) -> Result<(), AgentError> {
    if protocol.schema_version.trim().is_empty()
        || protocol.protocol_id.trim().is_empty()
        || protocol.main_agent_authority.trim().is_empty()
        || protocol.runtime_authority.trim().is_empty()
        || protocol.worker_authority.trim().is_empty()
        || protocol.artifact_flow.trim().is_empty()
        || protocol.adoption_rule.trim().is_empty()
        || protocol.review_rule.trim().is_empty()
        || protocol.cleanup_rule.trim().is_empty()
    {
        return Err(AgentError::InvalidTaskPacket {
            reason: "collaboration_protocol authority and flow fields must be non-empty"
                .to_string(),
        });
    }
    if protocol.required_worker_capabilities.is_empty()
        || protocol.required_worker_output_sections.is_empty()
        || protocol.forbidden_worker_actions.is_empty()
    {
        return Err(AgentError::InvalidTaskPacket {
            reason: "collaboration_protocol must include worker capabilities, output sections, and forbidden actions".to_string(),
        });
    }
    Ok(())
}

fn validate_stage_task_contract(contract: &AgentStageTaskContract) -> Result<(), AgentError> {
    if contract.schema_version.trim().is_empty()
        || contract.task_id.trim().is_empty()
        || contract.stage_execution_id.trim().is_empty()
        || contract.stage_id.trim().is_empty()
        || contract.task_type.trim().is_empty()
        || contract.worker_role.trim().is_empty()
        || contract.objective.trim().is_empty()
        || contract.required_output_artifact_type.trim().is_empty()
    {
        return Err(AgentError::InvalidTaskPacket {
            reason: "stage_task_contract required fields must be non-empty".to_string(),
        });
    }
    if contract.required_output_fields.is_empty()
        || contract.acceptance_checks.is_empty()
        || contract.failure_signals.is_empty()
    {
        return Err(AgentError::InvalidTaskPacket {
            reason: "stage_task_contract must include required_output_fields, acceptance_checks, and failure_signals".to_string(),
        });
    }
    Ok(())
}

fn validate_output_manifest_refs(manifest: &AgentOutputManifest) -> Vec<String> {
    let mut errors = Vec::new();
    if manifest.output_refs.is_empty() {
        errors.push("output_refs must not be empty".to_string());
    }
    for output_ref in &manifest.output_refs {
        if output_ref.kind.trim().is_empty() || output_ref.r#ref.trim().is_empty() {
            errors.push("output ref kind and ref must be non-empty".to_string());
            continue;
        }
        if !Path::new(&output_ref.r#ref).exists() {
            errors.push(format!("output ref does not exist: {}", output_ref.r#ref));
        }
    }
    errors
}

fn capture_worktree_artifact_candidates(
    agent_id: &str,
    task_packet: &TaskPacket,
    workspace_binding: &AgentWorkspaceBinding,
    workspace_binding_path: &Path,
    manifest_path: &Path,
    candidate_archive_dir: &Path,
    patch_path: &Path,
    generated_at: &str,
) -> Result<AgentWorktreeArtifactCandidateManifest, AgentError> {
    let worktree_path = Path::new(&workspace_binding.worktree_path);
    let status_output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: "status worker worktree".to_string(),
            stderr: err.to_string(),
        })?;
    if !status_output.status.success() {
        return Err(AgentError::GitCommand {
            action: "status worker worktree".to_string(),
            stderr: String::from_utf8_lossy(&status_output.stderr).to_string(),
        });
    }

    let mut status_entries = String::from_utf8_lossy(&status_output.stdout)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mounted_inputs = read_worker_input_mounts(worktree_path);
    for ignored_entry in
        ignored_worker_candidate_status_entries(worktree_path, task_packet, &mounted_inputs)?
    {
        push_unique_string(&mut status_entries, ignored_entry);
    }
    let workspace_baseline = read_worker_workspace_baseline(workspace_binding_path);
    let stage_output_path_hints =
        stage_task_candidate_output_path_hints(task_packet, &status_entries);
    let mut observed_paths = status_entries
        .iter()
        .filter_map(|line| status_porcelain_path(line))
        .collect::<Vec<_>>();
    if let Some(workspace_baseline) = workspace_baseline.as_ref() {
        for entry in &workspace_baseline.entries {
            push_unique_string(&mut observed_paths, entry.relative_path.clone());
        }
    }
    let changed_paths = observed_paths
        .into_iter()
        .filter(|path| {
            should_capture_worker_candidate_path(
                path,
                &mounted_inputs,
                &stage_output_path_hints,
                worktree_path,
            )
        })
        .filter(|path| {
            workspace_baseline.as_ref().is_none_or(|baseline| {
                worker_workspace_path_differs_from_baseline(worktree_path, path, baseline)
            })
        })
        .collect::<Vec<_>>();
    let untracked_paths = status_entries
        .iter()
        .filter(|line| line.starts_with("?? "))
        .filter_map(|line| status_porcelain_path(line))
        .filter(|path| {
            should_capture_worker_candidate_path(
                path,
                &mounted_inputs,
                &stage_output_path_hints,
                worktree_path,
            )
        })
        .filter(|path| changed_paths.iter().any(|changed| changed == path))
        .collect::<Vec<_>>();
    let candidate_entries = build_worktree_candidate_entries(
        worktree_path,
        &changed_paths,
        candidate_archive_dir,
        generated_at,
    )?;

    let includes_modified_inherited_path = workspace_baseline.as_ref().is_some_and(|baseline| {
        changed_paths.iter().any(|path| {
            baseline
                .entries
                .iter()
                .any(|entry| entry.relative_path == *path)
        })
    });
    let diff_text = if changed_paths.is_empty() || includes_modified_inherited_path {
        String::new()
    } else {
        let diff_output = Command::new("git")
            .arg("-C")
            .arg(worktree_path)
            .args(["diff", "--binary", "HEAD", "--"])
            .args(&changed_paths)
            .output()
            .map_err(|err| AgentError::GitCommand {
                action: "diff worker worktree".to_string(),
                stderr: err.to_string(),
            })?;
        if !diff_output.status.success() {
            return Err(AgentError::GitCommand {
                action: "diff worker worktree".to_string(),
                stderr: String::from_utf8_lossy(&diff_output.stderr).to_string(),
            });
        }
        String::from_utf8_lossy(&diff_output.stdout).to_string()
    };
    let patch_ref = if diff_text.trim().is_empty() {
        None
    } else {
        atomic_write(patch_path, &diff_text)?;
        Some(patch_path.display().to_string())
    };

    let manifest = AgentWorktreeArtifactCandidateManifest {
        schema_version: "agent_worktree_artifact_candidate_manifest.v1".to_string(),
        agent_id: agent_id.to_string(),
        authority_scope: "worker_evidence_only".to_string(),
        adoption_status: "candidate_only".to_string(),
        workspace_binding_ref: workspace_binding_path.display().to_string(),
        source_workspace_root: workspace_binding.source_workspace_root.clone(),
        worktree_path: workspace_binding.worktree_path.clone(),
        git_head: workspace_binding.git_head.clone(),
        status_entries,
        changed_paths,
        untracked_paths,
        candidate_entries,
        patch_ref,
        generated_at: generated_at.to_string(),
    };
    atomic_write(manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
    Ok(manifest)
}

fn record_worker_workspace_baseline(
    data_dir: &Path,
    workspace_binding: &AgentWorkspaceBinding,
    task_packet: &TaskPacket,
    generated_at: &str,
) -> Result<AgentWorkerWorkspaceBaselineManifest, AgentError> {
    let worktree_path = Path::new(&workspace_binding.worktree_path);
    let mounted_inputs = read_worker_input_mounts(worktree_path);
    let mut status_entries = git_output_lines(
        worktree_path,
        &["status", "--porcelain", "--untracked-files=all"],
        "status worker workspace baseline",
    )?;
    for ignored_entry in
        ignored_worker_candidate_status_entries(worktree_path, task_packet, &mounted_inputs)?
    {
        push_unique_string(&mut status_entries, ignored_entry);
    }
    let mut baseline_paths = status_entries
        .iter()
        .filter_map(|line| status_porcelain_path(line))
        .collect::<Vec<_>>();
    for input in &mounted_inputs {
        push_unique_string(&mut baseline_paths, input.mounted_path.clone());
        for bundled_path in &input.bundled_paths {
            push_unique_string(&mut baseline_paths, bundled_path.clone());
        }
    }
    if let Ok(Some(canonical_baseline)) = canonical_artifacts::load_current_baseline(data_dir) {
        if workspace_binding.baseline_ref.as_deref()
            == Some(canonical_baseline.baseline_ref.as_str())
        {
            for entry in canonical_baseline.entries {
                for relative_path in
                    worker_workspace_baseline_file_paths(worktree_path, &entry.target_artifact_path)
                {
                    push_unique_string(&mut baseline_paths, relative_path);
                }
            }
        }
    }
    let mut entries = Vec::new();
    for relative_path in baseline_paths {
        let path = worktree_path.join(&relative_path);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let bytes = fs::read(&path)?;
        let sha256 = sha256_bytes(&bytes);
        entries.push(AgentWorkerWorkspaceBaselineEntry {
            relative_path,
            path_kind: "file".to_string(),
            size_bytes: bytes.len() as u64,
            sha256,
        });
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let manifest = AgentWorkerWorkspaceBaselineManifest {
        schema_version: "agent_worker_workspace_baseline.v1".to_string(),
        agent_id: workspace_binding.agent_id.clone(),
        authority_scope: "worker_session_provenance_only".to_string(),
        workspace_binding_ref: workspace_binding_path(data_dir, &workspace_binding.agent_id)
            .display()
            .to_string(),
        worktree_path: workspace_binding.worktree_path.clone(),
        entries,
        generated_at: generated_at.to_string(),
    };
    atomic_write(
        &worker_workspace_baseline_path(data_dir, &workspace_binding.agent_id),
        &serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn worker_workspace_baseline_file_paths(worktree_path: &Path, relative_path: &str) -> Vec<String> {
    let path = worktree_path.join(relative_path);
    if path.is_file() {
        return vec![relative_path.to_string()];
    }
    if !path.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let mut pending = vec![path];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.is_file() {
                if let Ok(relative) = path.strip_prefix(worktree_path) {
                    push_unique_string(
                        &mut files,
                        relative.display().to_string().replace('\\', "/"),
                    );
                }
            }
        }
    }
    files.sort();
    files
}

fn read_worker_workspace_baseline(
    workspace_binding_path: &Path,
) -> Option<AgentWorkerWorkspaceBaselineManifest> {
    let path = workspace_binding_path
        .parent()?
        .join("worker_workspace_baseline.json");
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn worker_workspace_path_differs_from_baseline(
    worktree_path: &Path,
    relative_path: &str,
    baseline: &AgentWorkerWorkspaceBaselineManifest,
) -> bool {
    let Some(entry) = baseline
        .entries
        .iter()
        .find(|entry| entry.relative_path == relative_path)
    else {
        return true;
    };
    let path = worktree_path.join(relative_path);
    let Ok(metadata) = fs::metadata(&path) else {
        return true;
    };
    if !metadata.is_file() || entry.path_kind != "file" {
        return true;
    }
    fs::read(path)
        .map(|bytes| sha256_bytes(&bytes) != entry.sha256)
        .unwrap_or(true)
}

fn read_worker_input_mount_manifest(
    worktree_path: &Path,
) -> Option<AgentInputArtifactMountManifest> {
    let manifest_path = worktree_path
        .join(".pmcli")
        .join("worker-input-artifacts.json");
    let contents = fs::read_to_string(manifest_path).ok()?;
    serde_json::from_str::<AgentInputArtifactMountManifest>(&contents).ok()
}

fn read_worker_input_mounts(worktree_path: &Path) -> Vec<AgentInputArtifactMount> {
    read_worker_input_mount_manifest(worktree_path)
        .map(|manifest| manifest.mounted_inputs)
        .unwrap_or_default()
}

fn ignored_worker_candidate_status_entries(
    worktree_path: &Path,
    task_packet: &TaskPacket,
    mounted_inputs: &[AgentInputArtifactMount],
) -> Result<Vec<String>, AgentError> {
    let pathspecs = ignored_worker_candidate_pathspecs(task_packet, mounted_inputs);
    if pathspecs.is_empty() {
        return Ok(Vec::new());
    }
    let status_output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args([
            "status",
            "--porcelain",
            "--ignored",
            "--untracked-files=all",
            "--",
        ])
        .args(pathspecs)
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: "status ignored worker candidate paths".to_string(),
            stderr: err.to_string(),
        })?;
    if !status_output.status.success() {
        return Err(AgentError::GitCommand {
            action: "status ignored worker candidate paths".to_string(),
            stderr: String::from_utf8_lossy(&status_output.stderr).to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&status_output.stdout)
        .lines()
        .map(str::trim_end)
        .filter(|line| line.starts_with("!! "))
        .map(str::to_string)
        .collect())
}

fn ignored_worker_candidate_pathspecs(
    task_packet: &TaskPacket,
    mounted_inputs: &[AgentInputArtifactMount],
) -> Vec<String> {
    if task_packet.stage_task_contract.is_none() {
        return Vec::new();
    }
    let mut pathspecs = Vec::new();
    for input in mounted_inputs {
        push_parent_candidate_pathspec(&mut pathspecs, &input.mounted_path);
    }
    if pathspecs.is_empty() {
        push_unique_string(&mut pathspecs, "research".to_string());
    }
    pathspecs
}

fn push_parent_candidate_pathspec(pathspecs: &mut Vec<String>, relative_path: &str) {
    let normalized = relative_path.replace('\\', "/");
    let Some(parent) = Path::new(&normalized).parent() else {
        return;
    };
    if parent.as_os_str().is_empty() {
        return;
    }
    let pathspec = parent.display().to_string().replace('\\', "/");
    if worker_candidate_path_safety(&pathspec).0 == "safe" {
        push_unique_string(pathspecs, pathspec);
    }
}

fn should_capture_worker_candidate_path(
    path: &str,
    mounted_inputs: &[AgentInputArtifactMount],
    stage_output_path_hints: &[String],
    worktree_path: &Path,
) -> bool {
    let normalized = path.replace('\\', "/");
    if normalized == ".pmcli/worker-input-artifacts.json" {
        return false;
    }
    if is_worker_task_packet_echo_path(&normalized) {
        return false;
    }
    if normalized.starts_with(".pmcli/input-bundles/") {
        return false;
    }
    let mounted_input = mounted_inputs
        .iter()
        .find(|input| input.mounted_path == path);
    let Some(mounted_input) = mounted_input else {
        return true;
    };
    stage_output_path_hints.iter().any(|hint| hint == path)
        && mounted_stage_output_candidate_differs_from_source(
            worktree_path,
            path,
            &mounted_input.source_path,
        )
}

fn build_worktree_candidate_entries(
    worktree_path: &Path,
    changed_paths: &[String],
    candidate_archive_dir: &Path,
    captured_at: &str,
) -> Result<Vec<AgentWorktreeArtifactCandidateEntry>, AgentError> {
    let mut entries = Vec::new();
    let mut candidate_paths = changed_paths.to_vec();
    for relative_path in changed_paths {
        for parent in worker_candidate_parent_directories(relative_path) {
            if !candidate_paths.iter().any(|path| path == &parent) {
                candidate_paths.push(parent);
            }
        }
    }
    candidate_paths.sort();
    for relative_path in &candidate_paths {
        let path = Path::new(relative_path);
        let (safe_status, unsafe_reason) = worker_candidate_path_safety(relative_path);
        let candidate_path = worktree_path.join(path);
        let metadata = fs::metadata(&candidate_path).ok();
        let is_directory = metadata.as_ref().is_some_and(|metadata| metadata.is_dir());
        let is_file = metadata.as_ref().is_some_and(|metadata| metadata.is_file());
        let (size_bytes, sha256, candidate_archive_ref) = if is_file {
            let bytes = fs::read(&candidate_path)?;
            let sha256 = sha256_bytes(&bytes);
            let archive_path = archive_worker_candidate_file(
                candidate_archive_dir,
                relative_path,
                &sha256,
                &bytes,
            )?;
            (
                bytes.len() as u64,
                sha256,
                Some(archive_path.display().to_string()),
            )
        } else {
            (0, String::new(), None)
        };
        let directory_manifest_ref = if is_directory && safe_status == "safe" {
            Some(
                write_worker_directory_candidate_manifest(
                    worktree_path,
                    relative_path,
                    candidate_archive_dir,
                    captured_at,
                )?
                .display()
                .to_string(),
            )
        } else {
            None
        };
        entries.push(AgentWorktreeArtifactCandidateEntry {
            relative_path: relative_path.clone(),
            path_kind: if is_directory {
                "directory".to_string()
            } else if is_file {
                "file".to_string()
            } else {
                "missing".to_string()
            },
            artifact_kind: worker_candidate_artifact_kind(relative_path, is_directory),
            size_bytes,
            sha256,
            candidate_archive_ref,
            safe_status,
            unsafe_reason,
            from_input_bundle: relative_path
                .replace('\\', "/")
                .starts_with(".pmcli/input-bundles/"),
            is_directory,
            directory_manifest_ref,
            captured_at: captured_at.to_string(),
        });
    }
    Ok(entries)
}

fn worker_candidate_parent_directories(relative_path: &str) -> Vec<String> {
    let mut parents = Vec::new();
    let mut current = Path::new(relative_path).parent();
    while let Some(parent) = current {
        if parent.as_os_str().is_empty() {
            break;
        }
        let parent_ref = parent.display().to_string().replace('\\', "/");
        if worker_candidate_path_safety(&parent_ref).0 == "safe" {
            parents.push(parent_ref);
        }
        current = parent.parent();
    }
    parents
}

fn archive_worker_candidate_file(
    candidate_archive_dir: &Path,
    relative_path: &str,
    sha256: &str,
    bytes: &[u8],
) -> Result<PathBuf, AgentError> {
    let archive_path = candidate_archive_dir
        .join(sha256)
        .join(safe_archive_relative_path(relative_path));
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&archive_path, bytes)?;
    let mut permissions = fs::metadata(&archive_path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&archive_path, permissions)?;
    Ok(archive_path)
}

fn write_worker_directory_candidate_manifest(
    worktree_path: &Path,
    root_relative_path: &str,
    candidate_archive_dir: &Path,
    captured_at: &str,
) -> Result<PathBuf, AgentError> {
    let root_path = worktree_path.join(root_relative_path);
    let mut entries = Vec::new();
    collect_directory_candidate_manifest_entries(&root_path, root_relative_path, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let total_size_bytes = entries.iter().map(|entry| entry.size_bytes).sum();
    let manifest = AgentWorktreeDirectoryCandidateManifest {
        schema_version: "agent_worktree_directory_candidate_manifest.v1".to_string(),
        root_relative_path: root_relative_path.to_string(),
        file_count: entries.len(),
        entries,
        total_size_bytes,
        captured_at: captured_at.to_string(),
    };
    let manifest_path = candidate_archive_dir
        .join("directory-manifests")
        .join(format!(
            "{}.json",
            safe_archive_relative_path(root_relative_path)
        ));
    atomic_write(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
    Ok(manifest_path)
}

fn collect_directory_candidate_manifest_entries(
    root_path: &Path,
    root_relative_path: &str,
    entries: &mut Vec<AgentWorktreeDirectoryCandidateEntry>,
) -> Result<(), AgentError> {
    for item in fs::read_dir(root_path)? {
        let item = item?;
        let path = item.path();
        let relative_path = Path::new(root_relative_path)
            .join(item.file_name())
            .display()
            .to_string()
            .replace('\\', "/");
        let (safe_status, _) = worker_candidate_path_safety(&relative_path);
        if safe_status != "safe" {
            continue;
        }
        let metadata = item.metadata()?;
        if metadata.is_dir() {
            collect_directory_candidate_manifest_entries(&path, &relative_path, entries)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let bytes = fs::read(&path)?;
        entries.push(AgentWorktreeDirectoryCandidateEntry {
            relative_path,
            size_bytes: bytes.len() as u64,
            sha256: sha256_bytes(&bytes),
        });
    }
    Ok(())
}

fn safe_archive_relative_path(relative_path: &str) -> String {
    relative_path
        .replace('\\', "/")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

fn worker_candidate_path_safety(path: &str) -> (String, Option<String>) {
    let normalized = path.replace('\\', "/");
    let path = Path::new(path);
    if normalized.trim().is_empty() {
        return (
            "unsafe".to_string(),
            Some("candidate path is empty".to_string()),
        );
    }
    if path.is_absolute() {
        return (
            "unsafe".to_string(),
            Some("candidate path is absolute".to_string()),
        );
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::Prefix(_)
                | std::path::Component::RootDir
        )
    }) {
        return (
            "unsafe".to_string(),
            Some("candidate path escapes the worker worktree".to_string()),
        );
    }
    if normalized == ".pmcli/worker-input-artifacts.json"
        || is_worker_task_packet_echo_path(&normalized)
        || normalized.starts_with(".pmcli/input-bundles/")
        || normalized.starts_with(".pmcli/agents/")
        || normalized.starts_with(".pmcli/reviews/")
    {
        return (
            "unsafe".to_string(),
            Some("candidate path is runtime or evidence-bundle state".to_string()),
        );
    }
    ("safe".to_string(), None)
}

fn is_worker_task_packet_echo_path(normalized_path: &str) -> bool {
    if normalized_path.contains('/') {
        return false;
    }
    normalized_path == "task_packet.json"
        || (normalized_path.starts_with("task_") && normalized_path.ends_with("_packet.json"))
}

fn worker_candidate_artifact_kind(relative_path: &str, is_directory: bool) -> String {
    if is_directory {
        return "directory".to_string();
    }
    match Path::new(relative_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
    {
        "py" => "python_source",
        "rs" | "ts" | "tsx" | "js" | "jsx" => "source_file",
        "json" => "json",
        "jsonl" => "jsonl",
        "md" => "markdown",
        "tex" => "latex_source",
        "bib" => "bibtex",
        "csv" => "csv",
        "tsv" => "tsv",
        "txt" => "text",
        _ => "file",
    }
    .to_string()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn mounted_stage_output_candidate_differs_from_source(
    worktree_path: &Path,
    mounted_path: &str,
    source_path: &str,
) -> bool {
    let candidate_path = worktree_path.join(mounted_path);
    let Ok(candidate) = fs::read(candidate_path) else {
        return false;
    };
    let Ok(source) = fs::read(source_path) else {
        return false;
    };
    candidate != source
}

fn stage_task_candidate_output_path_hints(
    task_packet: &TaskPacket,
    status_entries: &[String],
) -> Vec<String> {
    let Some(contract) = task_packet.stage_task_contract.as_ref() else {
        return Vec::new();
    };
    let artifact_name = stage_task_required_output_artifact_filename(
        &contract.required_output_artifact_type,
        &contract.task_type,
    );
    if artifact_name.is_empty() {
        return Vec::new();
    }
    let mut hints = Vec::new();
    for relative in status_entries
        .iter()
        .filter_map(|line| status_porcelain_path(line))
    {
        if relative.ends_with(&artifact_name) && relative.contains("research/stages/") {
            push_unique_string(&mut hints, relative);
        }
    }
    hints
}

fn stage_task_required_output_artifact_filename(
    required_output_artifact_type: &str,
    task_type: &str,
) -> String {
    let normalized = required_output_artifact_type.trim().replace('-', "_");
    if !normalized.is_empty() {
        return format!("{}.md", normalized);
    }
    format!("{}.md", task_type.trim().replace([' ', '-'], "_"))
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

const WORKSPACE_VISIBILITY_ENTRY_LIMIT: usize = 40;
const WORKSPACE_PROJECT_MARKER_PATHS: &[&str] = &[
    "src",
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "package.json",
    "pnpm-lock.yaml",
    "go.mod",
    "README.md",
    "tests",
    "scripts",
    "bin",
    "share",
    "research",
];

fn collect_agent_workspace_visibility(
    workspace_root: &Path,
    workspace_binding: &AgentWorkspaceBinding,
) -> AgentWorkspaceVisibility {
    let worktree_path = Path::new(&workspace_binding.worktree_path);
    let visible_root_entries = visible_root_entries(worktree_path);
    let tracked_root_entries = tracked_root_entries(worktree_path);
    let project_marker_paths_visible = project_marker_paths_visible(&tracked_root_entries);
    let project_marker_status = if project_marker_paths_visible.is_empty() {
        "no_project_markers_visible"
    } else {
        "project_markers_visible"
    };
    let mut notes = vec![
        "worker workspace is an isolated git worktree plus runtime-mounted input bundles"
            .to_string(),
        "input bundles are provenance/context and are not canonical project source".to_string(),
    ];
    if project_marker_paths_visible.is_empty() {
        notes.push(
            "no common project/source marker paths are visible in this worker worktree; tasks that require implementation evidence need mounted canonical paths or an explicit input-blocker route"
                .to_string(),
        );
    }
    AgentWorkspaceVisibility {
        schema_version: "agent_workspace_visibility.v1".to_string(),
        visibility_scope: "worker_git_worktree_plus_mounted_inputs".to_string(),
        source_workspace_root: workspace_root.display().to_string(),
        worktree_path: workspace_binding.worktree_path.clone(),
        baseline_mode: workspace_binding.baseline_mode.clone(),
        git_head: workspace_binding.git_head.clone(),
        tracked_root_entries,
        visible_root_entries,
        project_marker_paths_visible,
        project_marker_status: project_marker_status.to_string(),
        notes,
    }
}

fn visible_root_entries(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut values = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.trim().is_empty() {
                return None;
            }
            let suffix = entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| "/")
                .unwrap_or("");
            Some(format!("{name}{suffix}"))
        })
        .collect::<Vec<_>>();
    values.sort();
    values.truncate(WORKSPACE_VISIBILITY_ENTRY_LIMIT);
    values
}

fn tracked_root_entries(worktree_path: &Path) -> Vec<String> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["ls-tree", "--name-only", "HEAD"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut values = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    values.sort();
    values.truncate(WORKSPACE_VISIBILITY_ENTRY_LIMIT);
    values
}

fn project_marker_paths_visible(tracked_root_entries: &[String]) -> Vec<String> {
    WORKSPACE_PROJECT_MARKER_PATHS
        .iter()
        .filter(|marker| {
            tracked_root_entries
                .iter()
                .any(|entry| entry.trim_end_matches('/') == **marker)
        })
        .map(|marker| (*marker).to_string())
        .collect()
}

fn mount_task_input_artifacts(
    agent_id: &str,
    data_dir: &Path,
    workspace_root: &Path,
    workspace_binding: &AgentWorkspaceBinding,
    task_packet: &TaskPacket,
    manifest_path: &Path,
    generated_at: &str,
) -> Result<AgentInputArtifactMountManifest, AgentError> {
    let worktree_path = Path::new(&workspace_binding.worktree_path);
    let input_refs = task_packet
        .stage_task_contract
        .as_ref()
        .map(|contract| contract.input_artifact_refs.as_slice())
        .unwrap_or(&[]);
    let mut mounted_inputs = Vec::new();
    let mut unmounted_input_refs = Vec::new();
    let mut missing_required_refs = Vec::new();
    for input_ref in input_refs {
        let Some(resolved) = resolve_input_artifact_ref(data_dir, workspace_root, input_ref) else {
            if input_artifact_ref_is_required_for_dispatch(input_ref) {
                missing_required_refs.push(input_ref.clone());
            } else {
                push_unique_string(&mut unmounted_input_refs, input_ref.clone());
            }
            continue;
        };
        if resolved.source_paths.is_empty() {
            if resolved.fail_if_missing {
                missing_required_refs.push(input_ref.clone());
            } else {
                push_unique_string(&mut unmounted_input_refs, input_ref.clone());
            }
            continue;
        }
        let should_bundle = resolved.source_paths.len() > 1
            || resolved.bundle_kind != "path"
            || resolved
                .source_paths
                .iter()
                .any(|path| input_artifact_source_requires_bundle(workspace_root, path));
        if should_bundle {
            let bundle_dir = PathBuf::from(".pmcli")
                .join("input-bundles")
                .join(sanitize_input_bundle_component(&resolved.input_ref));
            let mounted_bundle_dir = worktree_path.join(&bundle_dir);
            fs::create_dir_all(&mounted_bundle_dir).map_err(|err| {
                AgentError::io_context("create input bundle directory", &mounted_bundle_dir, err)
            })?;
            let mut bundled_paths = Vec::new();
            let mut source_paths = Vec::new();
            for source_path in &resolved.source_paths {
                if !source_path.is_file() {
                    continue;
                }
                let relative_source = source_path
                    .strip_prefix(workspace_root)
                    .ok()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| source_path.display().to_string());
                let file_name = source_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("input-artifact");
                let target_relative = bundle_dir.join(format!(
                    "{}-{}",
                    bundled_paths.len(),
                    sanitize_input_bundle_component(file_name)
                ));
                let target_path = worktree_path.join(&target_relative);
                fs::copy(source_path, &target_path).map_err(|err| {
                    AgentError::io_context(
                        &format!(
                            "copy input artifact `{}` from `{}` to",
                            resolved.input_ref,
                            source_path.display()
                        ),
                        &target_path,
                        err,
                    )
                })?;
                bundled_paths.push(target_relative.display().to_string());
                source_paths.push(relative_source);
            }
            if bundled_paths.is_empty() {
                if resolved.fail_if_missing {
                    missing_required_refs.push(input_ref.clone());
                } else {
                    push_unique_string(&mut unmounted_input_refs, input_ref.clone());
                }
                continue;
            }
            let bundle_manifest_relative = bundle_dir.join("bundle-manifest.json");
            let bundle_manifest = json!({
                "schema_version": "agent_input_artifact_bundle.v1",
                "input_ref": resolved.input_ref,
                "bundle_kind": resolved.bundle_kind,
                "source_workspace_root": workspace_root.display().to_string(),
                "source_paths": source_paths,
                "mounted_paths": bundled_paths,
                "generated_at": generated_at,
                "authority_scope": "runtime_input_mount_only"
            });
            atomic_write(
                &worktree_path.join(&bundle_manifest_relative),
                &serde_json::to_string_pretty(&bundle_manifest)?,
            )?;
            mounted_inputs.push(AgentInputArtifactMount {
                input_ref: input_ref.clone(),
                source_path: source_paths.join(","),
                mounted_path: bundle_dir.display().to_string(),
                bundle_manifest_path: Some(bundle_manifest_relative.display().to_string()),
                bundled_paths,
            });
            continue;
        }
        let source_path = &resolved.source_paths[0];
        if !source_path.is_file() {
            if resolved.fail_if_missing {
                missing_required_refs.push(input_ref.clone());
            } else {
                push_unique_string(&mut unmounted_input_refs, input_ref.clone());
            }
            continue;
        }
        let Ok(relative_path) = source_path.strip_prefix(workspace_root) else {
            if resolved.fail_if_missing {
                missing_required_refs.push(input_ref.clone());
            } else {
                push_unique_string(&mut unmounted_input_refs, input_ref.clone());
            }
            continue;
        };
        let mounted_path = worktree_path.join(relative_path);
        if let Some(parent) = mounted_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                AgentError::io_context("create mounted input artifact parent", parent, err)
            })?;
        }
        fs::copy(source_path, &mounted_path).map_err(|err| {
            AgentError::io_context(
                &format!(
                    "copy input artifact `{}` from `{}` to",
                    input_ref,
                    source_path.display()
                ),
                &mounted_path,
                err,
            )
        })?;
        mounted_inputs.push(AgentInputArtifactMount {
            input_ref: input_ref.clone(),
            source_path: source_path.display().to_string(),
            mounted_path: relative_path.display().to_string(),
            bundle_manifest_path: None,
            bundled_paths: Vec::new(),
        });
    }
    if !missing_required_refs.is_empty() {
        return Err(AgentError::InvalidTaskPacket {
            reason: format!(
                "required input artifact refs could not be resolved for dispatch: {}",
                missing_required_refs.join(", ")
            ),
        });
    }
    let manifest_path_in_worktree = ".pmcli/worker-input-artifacts.json";
    let manifest = AgentInputArtifactMountManifest {
        schema_version: "agent_input_artifact_mount_manifest.v1".to_string(),
        agent_id: agent_id.to_string(),
        authority_scope: "runtime_input_mount_only".to_string(),
        source_workspace_root: workspace_root.display().to_string(),
        worktree_path: workspace_binding.worktree_path.clone(),
        manifest_path_in_worktree: manifest_path_in_worktree.to_string(),
        workspace_visibility: collect_agent_workspace_visibility(workspace_root, workspace_binding),
        mounted_inputs,
        unmounted_input_refs,
        generated_at: generated_at.to_string(),
    };
    let serialized = serde_json::to_string_pretty(&manifest)?;
    atomic_write(manifest_path, &serialized)?;
    atomic_write(&worktree_path.join(manifest_path_in_worktree), &serialized)?;
    Ok(manifest)
}

impl AgentError {
    fn io_context(action: impl Into<String>, path: &Path, source: std::io::Error) -> Self {
        Self::IoContext {
            action: action.into(),
            path: path.display().to_string(),
            source,
        }
    }
}

fn input_artifact_source_requires_bundle(workspace_root: &Path, source_path: &Path) -> bool {
    let Ok(relative_path) = source_path.strip_prefix(workspace_root) else {
        return true;
    };
    input_artifact_relative_path_requires_bundle(&relative_path.display().to_string())
}

fn input_artifact_relative_path_requires_bundle(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    normalized == ".pmcli" || normalized.starts_with(".pmcli/")
}

fn resolve_input_artifact_ref(
    data_dir: &Path,
    workspace_root: &Path,
    input_ref: &str,
) -> Option<ResolvedInputArtifact> {
    let trimmed = input_ref.trim();
    if board_task_refs::input_ref_is_context_only(trimmed)
        || board_task_refs::dispatch_input_ref_policy(trimmed)
            == board_task_refs::DispatchInputRefPolicy::InvalidForWorkerInput
    {
        return None;
    }
    if let Some(task_id) = trimmed.strip_prefix("accepted_worker_evidence_task:") {
        return resolve_accepted_worker_evidence_task_bundle(
            data_dir,
            workspace_root,
            trimmed,
            task_id,
        );
    }
    if let Some(path_ref) = review_refs::review_artifact_path_in_data_dir(data_dir, trimmed) {
        return resolve_artifact_path(data_dir, workspace_root, &path_ref).map(|path| {
            ResolvedInputArtifact {
                input_ref: trimmed.to_string(),
                source_paths: vec![path],
                bundle_kind: "review_artifact".to_string(),
                fail_if_missing: input_artifact_ref_is_required_for_dispatch(trimmed),
            }
        });
    }
    let path_ref = trimmed
        .strip_prefix("accepted_worker_evidence_index:")
        .or_else(|| trimmed.strip_prefix("readiness_ref:"))
        .unwrap_or(trimmed);
    resolve_artifact_path(data_dir, workspace_root, Path::new(path_ref)).map(|path| {
        ResolvedInputArtifact {
            input_ref: trimmed.to_string(),
            source_paths: vec![path],
            bundle_kind: if trimmed.starts_with("accepted_worker_evidence_index:") {
                "accepted_worker_evidence_index".to_string()
            } else {
                "path".to_string()
            },
            fail_if_missing: input_artifact_ref_is_required_for_dispatch(trimmed),
        }
    })
}

fn resolve_artifact_path(
    data_dir: &Path,
    workspace_root: &Path,
    path_ref: &Path,
) -> Option<PathBuf> {
    if !path_ref.is_absolute() {
        return resolve_workspace_path(workspace_root, &path_ref.to_string_lossy());
    }
    let resolved = path_ref.canonicalize().ok()?;
    let workspace = workspace_root.canonicalize().ok()?;
    let runtime_data = data_dir.canonicalize().ok()?;
    (resolved.starts_with(workspace) || resolved.starts_with(runtime_data)).then_some(resolved)
}

fn resolve_workspace_path(workspace_root: &Path, path_ref: &str) -> Option<PathBuf> {
    let candidate = Path::new(path_ref);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace_root.join(candidate)
    };
    let workspace = workspace_root.canonicalize().ok()?;
    let resolved = absolute.canonicalize().ok()?;
    if resolved.starts_with(&workspace) {
        Some(resolved)
    } else {
        None
    }
}

fn resolve_accepted_worker_evidence_task_bundle(
    data_dir: &Path,
    workspace_root: &Path,
    input_ref: &str,
    task_id: &str,
) -> Option<ResolvedInputArtifact> {
    let mut source_paths = Vec::new();
    for index_path in accepted_worker_evidence_index_paths(workspace_root) {
        let Ok(content) = fs::read_to_string(&index_path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(entries) = value.get("entries").and_then(|entries| entries.as_array()) else {
            continue;
        };
        for entry in entries {
            if !crate::accepted_worker_evidence::json_entry_is_main_agent_accepted(entry) {
                continue;
            }
            if entry.get("task_id").and_then(|value| value.as_str()) != Some(task_id) {
                continue;
            }
            push_unique_path(&mut source_paths, index_path.clone());
            for key in ["task_packet_ref", "output_manifest_ref"] {
                if let Some(reference) = entry.get(key).and_then(|value| value.as_str()) {
                    if let Some(path) =
                        resolve_artifact_path(data_dir, workspace_root, Path::new(reference))
                    {
                        push_unique_path(&mut source_paths, path);
                    }
                }
            }
            if let Some(refs) = entry
                .get("evidence_refs")
                .and_then(|value| value.as_array())
            {
                for reference in refs.iter().filter_map(|value| value.as_str()) {
                    if let Some(path_ref) =
                        review_refs::review_artifact_path_in_data_dir(data_dir, reference)
                    {
                        if let Some(path) =
                            resolve_artifact_path(data_dir, workspace_root, &path_ref)
                        {
                            push_unique_path(&mut source_paths, path);
                        }
                        continue;
                    }
                    if let Some(path) =
                        resolve_artifact_path(data_dir, workspace_root, Path::new(reference))
                    {
                        let candidate_paths =
                            worktree_candidate_files_from_manifest_path(workspace_root, &path);
                        push_unique_path(&mut source_paths, path);
                        for candidate_path in candidate_paths {
                            push_unique_path(&mut source_paths, candidate_path);
                        }
                    }
                }
            }
            for review_key in ["semantic_review", "main_agent_acceptance"] {
                if let Some(review) = entry.get(review_key) {
                    for key in ["review_packet_ref", "review_trace_ref"] {
                        if let Some(reference) = review.get(key).and_then(|value| value.as_str()) {
                            if let Some(path) = resolve_artifact_path(
                                data_dir,
                                workspace_root,
                                Path::new(reference),
                            ) {
                                push_unique_path(&mut source_paths, path);
                            }
                        }
                    }
                }
            }
        }
    }
    if source_paths.is_empty() {
        return None;
    }
    Some(ResolvedInputArtifact {
        input_ref: input_ref.to_string(),
        source_paths,
        bundle_kind: "accepted_worker_evidence_task".to_string(),
        fail_if_missing: true,
    })
}

fn worktree_candidate_files_from_manifest_path(
    workspace_root: &Path,
    manifest_path: &Path,
) -> Vec<PathBuf> {
    if manifest_path.file_name().and_then(|name| name.to_str())
        != Some("worktree_artifact_candidates.json")
    {
        return Vec::new();
    }
    let Ok(content) = fs::read_to_string(manifest_path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    if value
        .get("authority_scope")
        .and_then(|value| value.as_str())
        != Some("worker_evidence_only")
    {
        return Vec::new();
    }
    let Some(worktree_path) = value.get("worktree_path").and_then(|value| value.as_str()) else {
        return Vec::new();
    };
    let worktree = Path::new(worktree_path);
    let mut relative_paths = json_string_array(&value, "changed_paths");
    for relative_path in json_string_array(&value, "untracked_paths") {
        push_unique_string(&mut relative_paths, relative_path);
    }
    let workspace_canonical = workspace_root.canonicalize().ok();
    let worktree_canonical = worktree.canonicalize().ok();
    let mut source_paths = Vec::new();
    for relative_path in relative_paths {
        if !worker_candidate_relative_path_is_safe(&relative_path) {
            continue;
        }
        let candidate_path = worktree.join(&relative_path);
        if !candidate_path.is_file() {
            continue;
        }
        let Some(resolved) = candidate_path.canonicalize().ok() else {
            continue;
        };
        let under_workspace = workspace_canonical
            .as_ref()
            .map(|workspace| resolved.starts_with(workspace))
            .unwrap_or(false);
        let under_worktree = worktree_canonical
            .as_ref()
            .map(|worktree| resolved.starts_with(worktree))
            .unwrap_or(false);
        if under_workspace || under_worktree {
            push_unique_path(&mut source_paths, resolved);
        }
    }
    source_paths
}

fn json_string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn worker_candidate_relative_path_is_safe(path: &str) -> bool {
    let candidate = Path::new(path);
    !candidate.is_absolute()
        && candidate
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn accepted_worker_evidence_index_paths(workspace_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_files_named(
        &workspace_root.join("research").join("stages"),
        "index.json",
        &mut paths,
    );
    collect_files_named(
        &workspace_root
            .join(".pmcli")
            .join("research")
            .join("stages"),
        "index.json",
        &mut paths,
    );
    paths
        .into_iter()
        .filter(|path| {
            path.to_string_lossy().contains("accepted_worker_evidence")
                || fs::read_to_string(path)
                    .map(|content| {
                        content.contains("autonomous_research_accepted_worker_evidence")
                            || content.contains("\"entries\"")
                                && content.contains("\"output_manifest_ref\"")
                                && content.contains("\"task_packet_ref\"")
                    })
                    .unwrap_or(false)
        })
        .collect()
}

fn collect_files_named(root: &Path, file_name: &str, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_named(&path, file_name, paths);
        } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            paths.push(path);
        }
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn input_artifact_ref_is_required_for_dispatch(input_ref: &str) -> bool {
    board_task_refs::dispatch_input_ref_is_required(input_ref)
}

fn sanitize_input_bundle_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "input_artifact".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn ensure_source_git_repository(workspace_root: &Path) -> Result<(), AgentError> {
    let probe = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: "rev-parse".to_string(),
            stderr: err.to_string(),
        })?;
    if probe.status.success() {
        return Ok(());
    }

    run_git_command(workspace_root, &["init", "-q"], "init")
}

fn ensure_source_worktree_has_head(workspace_root: &Path) -> Result<(), AgentError> {
    let head_output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: "rev-parse".to_string(),
            stderr: err.to_string(),
        })?;
    if head_output.status.success() {
        return Ok(());
    }
    if !git_head_failure_is_unborn(&head_output.stderr) {
        return Err(AgentError::GitCommand {
            action: "rev-parse".to_string(),
            stderr: String::from_utf8_lossy(&head_output.stderr).to_string(),
        });
    }

    let bootstrap_path = workspace_root.join(".astra").join("bootstrap.md");
    let bootstrap_parent = bootstrap_path
        .parent()
        .ok_or_else(|| AgentError::LocalCommand("invalid bootstrap path".to_string()))?;
    fs::create_dir_all(bootstrap_parent)?;
    if !bootstrap_path.exists() {
        atomic_write(
            &bootstrap_path,
            "# Astra Workspace Baseline\n\nThis file gives a new Astra project an initial Git commit so autonomous agents can use Git worktrees, rollback, and cleanup governance from the first run.\n",
        )?;
    }

    run_git_command(
        workspace_root,
        &["add", ".astra/bootstrap.md"],
        "add bootstrap",
    )?;
    run_git_command(
        workspace_root,
        &[
            "-c",
            "user.name=Astra",
            "-c",
            "user.email=astra@example.invalid",
            "commit",
            "-m",
            "Initialize Astra workspace baseline",
        ],
        "commit bootstrap",
    )?;
    Ok(())
}

fn git_head_failure_is_unborn(stderr: &[u8]) -> bool {
    let message = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    message.contains("needed a single revision")
        || message.contains("unknown revision")
        || message.contains("ambiguous argument 'head'")
        || message.contains("bad revision 'head'")
}

fn run_git_command(workspace_root: &Path, args: &[&str], action: &str) -> Result<(), AgentError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .output()
        .map_err(|err| AgentError::GitCommand {
            action: action.to_string(),
            stderr: err.to_string(),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AgentError::GitCommand {
            action: action.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    fn assert_round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_value(value).expect("value should serialize");
        let decoded = serde_json::from_value::<T>(json).expect("value should deserialize");
        let decoded_json = serde_json::to_value(decoded).expect("decoded value should serialize");
        let original_json = serde_json::to_value(value).expect("original value should serialize");
        assert_eq!(decoded_json, original_json);
    }

    fn unique_test_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "research_cli_agents_{}_{}_{}",
            label,
            std::process::id(),
            timestamp_string()
        ));
        fs::create_dir_all(&root).expect("workspace should be created");
        root
    }

    #[test]
    fn resolves_review_packet_input_refs_for_dispatch() {
        let workspace_root = unique_test_workspace("review_packet_input_refs");
        let review_dir = workspace_root
            .join(".pmcli")
            .join("reviews")
            .join("rev_123");
        fs::create_dir_all(&review_dir).expect("review dir should exist");
        let packet_path = review_dir.join("packet.json");
        fs::write(&packet_path, "{}").expect("packet should write");

        let logical = resolve_input_artifact_ref(
            &workspace_root.join(".pmcli"),
            &workspace_root,
            "review_packet:rev_123",
        )
        .expect("logical review packet ref should resolve");
        assert_eq!(logical.bundle_kind, "review_artifact");
        assert_eq!(
            logical.source_paths,
            vec![packet_path
                .canonicalize()
                .expect("packet should canonicalize")]
        );
        assert!(logical.fail_if_missing);

        let mixed = resolve_input_artifact_ref(
            &workspace_root.join(".pmcli"),
            &workspace_root,
            "review_packet:.pmcli/reviews/rev_123/packet.json",
        )
        .expect("legacy mixed review packet ref should resolve");
        assert_eq!(mixed.source_paths, logical.source_paths);
    }

    #[test]
    fn resolves_review_trace_id_input_refs_for_dispatch() {
        let workspace_root = unique_test_workspace("review_trace_id_input_refs");
        let review_dir = workspace_root
            .join(".pmcli")
            .join("reviews")
            .join("rev_123");
        fs::create_dir_all(review_dir.join("traces")).expect("review trace dir should exist");
        let trace_path = review_dir.join("trace.latest.json");
        fs::write(
            &trace_path,
            r#"{"review_id":"rev_123","trace_id":"trc_456"}"#,
        )
        .expect("trace should write");

        let logical = resolve_input_artifact_ref(
            &workspace_root.join(".pmcli"),
            &workspace_root,
            "review_trace:trc_456",
        )
        .expect("trace id review ref should resolve");
        assert_eq!(logical.bundle_kind, "review_artifact");
        assert_eq!(
            logical.source_paths,
            vec![trace_path
                .canonicalize()
                .expect("trace should canonicalize")]
        );
        assert!(logical.fail_if_missing);

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn worker_input_artifact_mounts_review_refs_from_separate_runtime_data_dir() {
        let workspace_root = unique_test_workspace("review_refs_separate_workspace");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = unique_test_workspace("review_refs_separate_data");
        let review_dir = data_dir.join("reviews").join("rev_123");
        fs::create_dir_all(&review_dir).expect("review dir should exist");
        fs::write(review_dir.join("packet.json"), "{}").expect("packet should write");
        fs::write(
            review_dir.join("trace.latest.json"),
            r#"{"review_id":"rev_123","trace_id":"trc_456"}"#,
        )
        .expect("trace should write");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_reviews", "1")
            .expect("fixture worktree should bind");
        let mut packet = test_task_packet(
            &workspace_root,
            "provider",
            "literature_comparison_researcher",
        );
        packet.agent_id = "agent_reviews".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "board::stage_fixture::source_verification".to_string(),
            stage_execution_id: "stage_fixture".to_string(),
            stage_id: "literature".to_string(),
            task_type: "source verification".to_string(),
            worker_role: "literature_comparison_researcher".to_string(),
            objective: "Repair evidence using review context.".to_string(),
            input_artifact_refs: vec![
                "review_packet:rev_123".to_string(),
                "review_trace:rev_123".to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "source_verification_ledger".to_string(),
            required_output_fields: vec!["verified sources".to_string()],
            acceptance_checks: vec!["review findings are addressed".to_string()],
            failure_signals: vec!["review context is missing".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: vec!["review:rev_123:wrong-stage-evidence".to_string()],
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: vec!["review_packet:rev_123".to_string()],
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        let manifest_path = input_mounts_path(&data_dir, "agent_reviews");

        let manifest = mount_task_input_artifacts(
            "agent_reviews",
            &data_dir,
            &workspace_root,
            &binding,
            &packet,
            &manifest_path,
            "2",
        )
        .expect("review artifacts from runtime data dir should mount");

        assert_eq!(manifest.mounted_inputs.len(), 2);
        assert!(manifest
            .mounted_inputs
            .iter()
            .all(|input| input.bundle_manifest_path.is_some()));

        let _ = fs::remove_dir_all(workspace_root);
        let _ = fs::remove_dir_all(data_dir);
    }

    fn test_role_package_skill_refs(role_profile: &str) -> Vec<AgentSkillRef> {
        let package = crate::session::role_package::resolve_role_package(
            Path::new("/tmp"),
            None,
            role_profile,
            role_kind_from_profile(role_profile),
        )
        .expect("builtin role package should resolve");
        role_package_skill_refs(package.skill_refs)
    }

    fn test_role_package_tool_policy(role_profile: &str) -> AgentToolPolicy {
        let package = crate::session::role_package::resolve_role_package(
            Path::new("/tmp"),
            None,
            role_profile,
            role_kind_from_profile(role_profile),
        )
        .expect("builtin role package should resolve");
        role_package_tool_policy(package.tool_policy)
    }

    fn empty_candidate_manifest(agent_id: &str) -> AgentWorktreeArtifactCandidateManifest {
        AgentWorktreeArtifactCandidateManifest {
            schema_version: "agent_worktree_artifact_candidate_manifest.v1".to_string(),
            agent_id: agent_id.to_string(),
            authority_scope: "worker_evidence_only".to_string(),
            adoption_status: "candidate_only".to_string(),
            workspace_binding_ref: String::new(),
            source_workspace_root: String::new(),
            worktree_path: String::new(),
            git_head: String::new(),
            status_entries: Vec::new(),
            changed_paths: Vec::new(),
            untracked_paths: Vec::new(),
            candidate_entries: Vec::new(),
            patch_ref: None,
            generated_at: "0".to_string(),
        }
    }

    fn test_task_packet(
        workspace_root: &Path,
        runner_kind: &str,
        role_profile: &str,
    ) -> TaskPacket {
        let stage_task_contract = None;
        TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: format!("task_{runner_kind}_{role_profile}"),
            agent_id: format!("agent_{runner_kind}_{role_profile}"),
            runner_kind: Some(runner_kind.to_string()),
            intent: "execute test worker task".to_string(),
            role_profile: role_profile.to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 120_000,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs(role_profile),
            tool_policy: Some(test_role_package_tool_policy(role_profile)),
            command: None,
            message: "produce evidence".to_string(),
            created_at: "1".to_string(),
        }
    }

    fn source_verification_task_packet(workspace_root: &Path) -> TaskPacket {
        let mut packet = test_task_packet(workspace_root, "provider", "citation_auditor");
        packet.agent_id = "agent_source_verification".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::source_verification".to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "source verification".to_string(),
            worker_role: "citation_auditor".to_string(),
            objective: "Build a row-level source verification ledger.".to_string(),
            input_artifact_refs: vec![
                "accepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search"
                    .to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "source_verification_ledger".to_string(),
            required_output_fields: vec![
                "source id".to_string(),
                "canonical verified title".to_string(),
                "source verification status".to_string(),
                "metadata confidence".to_string(),
                "claim support boundary".to_string(),
                "missing-source risks".to_string(),
            ],
            acceptance_checks: vec!["all upstream rows have ledger rows".to_string()],
            failure_signals: vec!["ledger is truncated".to_string()],
            depends_on_task_ids: vec![
                "research_stage_task::stage_literature_1::paper_search".to_string(),
            ],
            priority: 1,
            review_findings_refs: vec!["review:rev_fixture".to_string()],
            blocker_refs: vec!["worker_review_failure_route::rev_fixture".to_string()],
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: vec!["review_packet:rev_fixture".to_string()],
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        packet
    }

    #[test]
    fn local_agent_bootstraps_unborn_git_workspace_before_worktree_binding() {
        let workspace_root = unique_test_workspace("unborn_git");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");

        let result = start_local(
            &data_dir,
            &workspace_root,
            LocalAgentStartRequest {
                intent: "exercise unborn repository bootstrap".to_string(),
                role_profile: "research-worker".to_string(),
                message: "produce a bounded status artifact".to_string(),
                command: "printf 'bootstrap-ok\\n'".to_string(),
                stage_task_contract: None,
            },
        )
        .expect("local agent should bootstrap an unborn git repo");

        assert_eq!(result.status, "succeeded");
        assert_eq!(result.runtime_record.lifecycle_status, "succeeded");
        assert_eq!(result.output_manifest.status, "complete");
        let identity_ref = result
            .runtime_record
            .runtime_identity_ref
            .as_ref()
            .expect("local worker should write runtime identity");
        let identity: AgentRuntimeIdentity = serde_json::from_str(
            &fs::read_to_string(identity_ref).expect("runtime identity should be readable"),
        )
        .expect("runtime identity should parse");
        assert_eq!(identity.runtime_kind, "delegated_worker");
        assert_eq!(identity.runner_kind, "local");
        assert_eq!(identity.authority_scope, "worker_evidence_only");
        assert_eq!(
            identity.task_packet_ref.as_deref(),
            Some(result.task_packet_path.as_str())
        );
        let binding = result
            .workspace_binding
            .expect("workspace binding should be present");
        assert!(!binding.git_head.trim().is_empty());
        assert!(workspace_root.join(".astra").join("bootstrap.md").exists());

        let head = Command::new("git")
            .current_dir(&workspace_root)
            .args(["rev-parse", "--verify", "HEAD"])
            .status()
            .expect("git rev-parse should execute");
        assert!(head.success());
    }

    #[test]
    fn local_agent_bootstraps_plain_directory_before_worktree_binding() {
        let workspace_root = unique_test_workspace("plain_directory");
        let data_dir = workspace_root.join(".pmcli");

        let result = start_local(
            &data_dir,
            &workspace_root,
            LocalAgentStartRequest {
                intent: "exercise plain directory bootstrap".to_string(),
                role_profile: "research-worker".to_string(),
                message: "produce a bounded status artifact".to_string(),
                command: "printf 'plain-bootstrap-ok\\n'".to_string(),
                stage_task_contract: None,
            },
        )
        .expect("local agent should initialize and bootstrap a plain directory");

        assert_eq!(result.status, "succeeded");
        assert_eq!(result.runtime_record.lifecycle_status, "succeeded");
        assert_eq!(result.output_manifest.status, "complete");
        let binding = result
            .workspace_binding
            .expect("workspace binding should be present");
        assert!(!binding.git_head.trim().is_empty());
        assert!(workspace_root.join(".git").exists());
        assert!(workspace_root.join(".astra").join("bootstrap.md").exists());
    }

    #[test]
    fn local_agent_rejects_hollow_stage_task_contract() {
        let workspace_root = unique_test_workspace("hollow_stage_task_contract");
        let data_dir = workspace_root.join(".pmcli");
        let err = start_local(
            &data_dir,
            &workspace_root,
            LocalAgentStartRequest {
                intent: "execute hollow contract test".to_string(),
                role_profile: "research-worker".to_string(),
                message: "this should not run".to_string(),
                command: "printf should-not-run".to_string(),
                stage_task_contract: Some(AgentStageTaskContract {
                    schema_version: "agent_stage_task_contract.v1".to_string(),
                    task_id: "main_agent_task/hollow".to_string(),
                    stage_execution_id: "stage_hollow".to_string(),
                    stage_id: "literature".to_string(),
                    task_type: "paper search".to_string(),
                    worker_role: "literature_research_worker".to_string(),
                    objective: "Find papers without a real contract.".to_string(),
                    input_artifact_refs: Vec::new(),
                    required_canonical_artifacts: Vec::new(),
                    required_output_artifact_type: "literature_evidence_packet".to_string(),
                    required_output_fields: Vec::new(),
                    acceptance_checks: vec!["closest-family coverage".to_string()],
                    failure_signals: vec!["generic prose".to_string()],
                    depends_on_task_ids: Vec::new(),
                    priority: 1,
                    review_findings_refs: Vec::new(),
                    blocker_refs: Vec::new(),
                    review_target_task_ids: Vec::new(),
                    review_target_evidence_refs: Vec::new(),
                    supersedes_task_ids: Vec::new(),
                    replacement_of_task_ids: Vec::new(),
                    current_evidence_set_id: None,
                    consumed_input_required: false,
                }),
            },
        )
        .expect_err("hollow stage task contract should be rejected");

        assert!(matches!(err, AgentError::InvalidTaskPacket { .. }));
        assert!(!workspace_root.join(".astra").join("bootstrap.md").exists());
    }

    #[test]
    fn provider_agent_worker_records_provider_runtime_identity_for_live_route_failure() {
        std::env::set_var("OPENAI_API_KEY", "sk-test");
        std::env::remove_var("RESEARCH_CLI_LIVE_PROVIDER");
        let workspace_root = unique_test_workspace("provider_worker");
        let data_dir = workspace_root.join(".pmcli");

        let result = start_provider(
            &data_dir,
            &workspace_root,
            ProviderAgentStartRequest {
                intent: "execute provider worker test task".to_string(),
                role_profile: "research-worker".to_string(),
                message: "Produce a concise evidence note for a test task.".to_string(),
                provider_id: "openai".to_string(),
                automation_mode: None,
                stage_task_contract: None,
                model: "gpt-5.5".to_string(),
            },
        )
        .expect("provider worker should persist a structured failed run when live tools are unavailable");

        assert_eq!(result.status, "failed");
        assert_eq!(result.task_packet.runner_kind.as_deref(), Some("provider"));
        assert_eq!(result.task_packet.run_class, "bounded");
        assert_eq!(
            provider_worker_timeout_policy(&result.task_packet),
            crate::providers::ProviderTimeoutPolicy::Interactive
        );
        let protocol = result
            .task_packet
            .collaboration_protocol
            .as_ref()
            .expect("provider worker task packet should carry collaboration protocol");
        assert_eq!(protocol.protocol_id, "astra_main_runtime_worker_v1");
        assert!(protocol.main_agent_authority.contains("research strategy"));
        assert!(protocol
            .runtime_authority
            .contains("without inventing research decisions"));
        assert!(protocol
            .worker_authority
            .contains("executes only the assigned TaskPacket"));
        assert!(protocol
            .required_worker_capabilities
            .iter()
            .any(|capability| capability.contains("source-grounded")));
        assert!(protocol
            .forbidden_worker_actions
            .iter()
            .any(|action| action.contains("publish")));
        let identity_ref = result
            .runtime_record
            .runtime_identity_ref
            .as_ref()
            .expect("provider worker should write runtime identity");
        let identity: AgentRuntimeIdentity = serde_json::from_str(
            &fs::read_to_string(identity_ref).expect("runtime identity should be readable"),
        )
        .expect("runtime identity should parse");
        assert_eq!(identity.runtime_kind, "delegated_worker");
        assert_eq!(identity.runner_kind, "provider_agent_worker");
        assert_eq!(identity.provider_id.as_deref(), Some("openai"));
        assert_eq!(identity.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(identity.lifecycle_status, "failed");
        assert_eq!(identity.authority_scope, "worker_evidence_only");
        assert!(identity.session_ref.is_none());
        assert!(identity.tool_scope.iter().any(|tool| tool == "read_file"));
        assert!(identity.tool_scope.iter().any(|tool| tool == "fetch"));
        assert!(identity.tool_scope.iter().any(|tool| tool == "write_file"));
        assert!(identity
            .tool_scope
            .iter()
            .any(|tool| tool == "worker_shell"));
        assert!(!identity.tool_scope.iter().any(|tool| tool == "apply_patch"));
        assert!(!identity.tool_scope.iter().any(|tool| tool == "shell"));
        assert!(!identity
            .tool_scope
            .iter()
            .any(|tool| tool == "publish_board_tasks"));
        assert!(!identity
            .tool_scope
            .iter()
            .any(|tool| tool == "record_obligation_decision"));
        assert!(result
            .output_manifest
            .output_refs
            .iter()
            .any(|output| output.kind == "provider_worker_evidence"));
        assert_eq!(result.output_manifest.status, "failed");
        assert_eq!(result.output_manifest.validation_status, "invalid");
        let status_ref = result
            .runtime_record
            .status_ref
            .as_ref()
            .expect("provider worker should write terminal status");
        let status_text =
            fs::read_to_string(status_ref).expect("terminal status should be readable");
        assert!(status_text.contains("status: \"failed\""));
        assert!(status_text.contains("provider runner exited"));
        assert!(fs::read_to_string(
            result
                .output_manifest
                .output_refs
                .iter()
                .find(|output| output.kind == "provider_worker_evidence")
                .expect("provider worker evidence ref should exist")
                .r#ref
                .as_str(),
        )
        .expect("provider worker evidence should be readable")
        .contains("tool-capable agent loops require a live provider route"));
        let candidate_ref = result
            .output_manifest
            .output_refs
            .iter()
            .find(|output| output.kind == "worker_worktree_artifact_candidates")
            .expect("provider worker should expose candidate-only worktree artifacts");
        let candidates: AgentWorktreeArtifactCandidateManifest = serde_json::from_str(
            &fs::read_to_string(&candidate_ref.r#ref)
                .expect("candidate artifact manifest should be readable"),
        )
        .expect("candidate artifact manifest should parse");
        assert_eq!(candidates.adoption_status, "candidate_only");
        assert_eq!(candidates.authority_scope, "worker_evidence_only");
        assert_eq!(candidates.agent_id, result.agent_id);
        assert!(identity
            .evidence_refs
            .iter()
            .any(|reference| reference == &candidate_ref.r#ref));
        let binding = result
            .workspace_binding
            .expect("provider worker should bind an isolated worktree");
        assert!(Path::new(&binding.worktree_path).exists());
        assert_ne!(binding.worktree_path, workspace_root.display().to_string());
    }

    #[test]
    fn provider_start_failure_persists_structured_failure_record_after_partial_agent_dir_created() {
        let workspace_root = unique_test_workspace("provider_background_start_failure");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "research-worker");
        let failure_dir = agent_dir(&data_dir, &packet.agent_id);
        fs::create_dir_all(failure_dir.join("traces"))
            .expect("partial agent dir should be created");

        persist_provider_start_failure(
            &data_dir,
            &packet,
            &AgentError::ProviderExecution("No such file or directory (os error 2)".to_string()),
            "openai",
            "gpt-5.5",
        )
        .expect("provider start failure should persist after partial agent dir exists");

        let runtime: AgentRuntimeRecord = serde_json::from_str(
            &fs::read_to_string(failure_dir.join("runtime.json"))
                .expect("failure runtime should be written"),
        )
        .expect("failure runtime should parse");
        assert_eq!(runtime.failure_code, "provider_execution_failed");
        let manifest = fs::read_to_string(failure_dir.join("output_manifest.json"))
            .expect("failure manifest should be written");
        assert!(manifest.contains("provider_start_failure"));
        let stderr = fs::read_to_string(failure_dir.join("stderr.txt"))
            .expect("failure stderr should be written");
        assert!(stderr.contains("No such file or directory"));
    }

    #[test]
    fn provider_worker_persists_running_record_before_agent_loop_finishes() {
        let workspace_root = unique_test_workspace("provider_worker_running_record");
        ensure_source_git_repository(&workspace_root).expect("git repo should initialize");
        ensure_source_worktree_has_head(&workspace_root).expect("git head should bootstrap");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let agent_id = packet.agent_id.clone();
        let root = agent_dir(&data_dir, &agent_id);
        let task_packet_path = task_packet_path(&data_dir, &agent_id);
        let runtime_record_path = runtime_path(&data_dir, &agent_id);
        let output_manifest_path = output_manifest_path(&data_dir, &agent_id);
        let trace_path = trace_path(&data_dir, &agent_id);
        let runtime_identity_path = runtime_identity_path(&data_dir, &agent_id);
        let workspace_binding_path = workspace_binding_path(&data_dir, &agent_id);
        let input_mounts_path = input_mounts_path(&data_dir, &agent_id);
        let status_path = status_path(&data_dir, &agent_id);
        fs::create_dir_all(root.join("traces")).expect("agent trace dir should create");
        let workspace_binding =
            create_git_worktree(&data_dir, &workspace_root, &agent_id, &packet.created_at)
                .expect("worktree should create");
        atomic_write(
            &task_packet_path,
            &serde_json::to_string_pretty(&packet).expect("packet should serialize"),
        )
        .expect("packet should write");
        atomic_write(
            &workspace_binding_path,
            &serde_json::to_string_pretty(&workspace_binding)
                .expect("workspace binding should serialize"),
        )
        .expect("workspace binding should write");
        atomic_write(&input_mounts_path, "{}").expect("input mounts should write");

        persist_provider_running_record(
            &packet,
            "openai",
            "gpt-5.4",
            &workspace_binding,
            &task_packet_path,
            &runtime_record_path,
            &output_manifest_path,
            &trace_path,
            &runtime_identity_path,
            &workspace_binding_path,
            &input_mounts_path,
            &status_path,
            "2",
            "trace_running",
            "manifest_running",
            None,
            None,
        )
        .expect("running record should persist");

        let list_result = list(&data_dir).expect("agent list should load running record");
        assert_eq!(list_result.total_count, 1);
        assert_eq!(list_result.agents[0].agent_id, agent_id);
        assert_eq!(list_result.agents[0].lifecycle_status, "running");
        let inspection = inspect(&data_dir, &list_result.agents[0].agent_id)
            .expect("running provider worker should inspect");
        assert_eq!(inspection.runtime_record.lifecycle_status, "running");
        assert_eq!(
            inspection.runtime_record.status_ref.as_deref(),
            Some(status_path.display().to_string().as_str())
        );
        assert_eq!(inspection.output_manifest.status, "running");
        assert_eq!(inspection.output_manifest.validation_status, "pending");
        assert!(inspection
            .output_manifest
            .output_refs
            .iter()
            .any(|output| output.kind == "status"));
        assert!(fs::read_to_string(&status_path)
            .expect("running status should be readable")
            .contains("supervisor_pid:"));
        assert_eq!(inspection.trace.final_status, "running");
        let identity = runtime_identity(&data_dir, &inspection.agent_id)
            .expect("runtime identity should load");
        assert_eq!(identity.runner_kind, "provider_agent_worker");
        assert_eq!(identity.authority_scope, "worker_evidence_only");
    }

    #[test]
    fn provider_running_status_with_missing_supervisor_pid_marks_unknown_pid() {
        let workspace_root = unique_test_workspace("provider_running_status_missing_pid");
        ensure_source_git_repository(&workspace_root).expect("git repo should initialize");
        ensure_source_worktree_has_head(&workspace_root).expect("git head should bootstrap");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let workspace_binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            &packet.agent_id,
            &packet.created_at,
        )
        .expect("worktree should create");
        let task_packet_path = task_packet_path(&data_dir, &packet.agent_id);
        let runtime_record_path = runtime_path(&data_dir, &packet.agent_id);
        let output_manifest_path = output_manifest_path(&data_dir, &packet.agent_id);
        let trace_path = trace_path(&data_dir, &packet.agent_id);
        let runtime_identity_path = runtime_identity_path(&data_dir, &packet.agent_id);
        let workspace_binding_path = workspace_binding_path(&data_dir, &packet.agent_id);
        let input_mounts_path = input_mounts_path(&data_dir, &packet.agent_id);
        let status_path = status_path(&data_dir, &packet.agent_id);
        fs::create_dir_all(agent_dir(&data_dir, &packet.agent_id).join("traces"))
            .expect("trace dir should create");
        atomic_write(
            &task_packet_path,
            &serde_json::to_string_pretty(&packet).expect("packet should serialize"),
        )
        .expect("packet should write");
        atomic_write(
            &workspace_binding_path,
            &serde_json::to_string_pretty(&workspace_binding)
                .expect("workspace binding should serialize"),
        )
        .expect("workspace binding should write");
        atomic_write(&input_mounts_path, "{}").expect("input mounts should write");

        persist_provider_running_record(
            &packet,
            "openai",
            "gpt-5.4",
            &workspace_binding,
            &task_packet_path,
            &runtime_record_path,
            &output_manifest_path,
            &trace_path,
            &runtime_identity_path,
            &workspace_binding_path,
            &input_mounts_path,
            &status_path,
            "2",
            "trace_running_missing_pid",
            "manifest_running_missing_pid",
            None,
            None,
        )
        .expect("running record should persist");

        let status = fs::read_to_string(status_path).expect("status should read");
        assert!(status.contains("supervisor_pid: 0"));
        assert!(status.contains("supervisor_starttime: \"unknown\""));
    }

    #[test]
    fn running_agent_heartbeat_without_parseable_timestamps_relies_on_supervisor_state() {
        let workspace_root = unique_test_workspace("heartbeat_missing_timestamp");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let agent_id = packet.agent_id.clone();
        let root = agent_dir(&data_dir, &agent_id);
        fs::create_dir_all(root.join("traces")).expect("trace dir should create");
        let status_path = root.join("status.txt");
        let current_pid = std::process::id();
        let runtime_record = AgentRuntimeRecord {
            schema_version: "v1alpha1".to_string(),
            agent_id: agent_id.clone(),
            runner_kind: Some("provider".to_string()),
            runtime_identity_ref: None,
            role_kind: Some("literature_researcher".to_string()),
            authority_scope: Some("worker_evidence_only".to_string()),
            session_ref: None,
            task_packet_ref: "task.json".to_string(),
            lifecycle_status: "running".to_string(),
            created_at: "1".to_string(),
            updated_at: "not-a-timestamp".to_string(),
            heartbeat_at: "also-not-a-timestamp".to_string(),
            output_manifest_ref: "manifest.json".to_string(),
            workspace_binding_ref: None,
            directive_ref: None,
            status_ref: Some(status_path.display().to_string()),
            trace_refs: Vec::new(),
            stop_reason: String::new(),
            failure_code: String::new(),
        };
        atomic_write(
            &runtime_path(&data_dir, &agent_id),
            &serde_json::to_string_pretty(&runtime_record).expect("runtime should serialize"),
        )
        .expect("runtime should write");
        atomic_write(
            &status_path,
            &format!("---\nsupervisor_pid: {current_pid}\n---\n"),
        )
        .expect("status should write");

        let result = reclaim_stale_running_agent(&data_dir, &agent_id, 60_000)
            .expect("reclaim should succeed");

        assert!(!result.reclaimed);
        assert_eq!(result.reason, "supervisor_process_alive");
    }

    #[test]
    fn stale_heartbeat_does_not_reclaim_when_supervisor_process_is_alive() {
        let workspace_root = unique_test_workspace("heartbeat_stale_supervisor_alive");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let agent_id = packet.agent_id.clone();
        let root = agent_dir(&data_dir, &agent_id);
        fs::create_dir_all(root.join("traces")).expect("trace dir should create");
        let status_path = root.join("STATUS.md");
        let task_packet_path = task_packet_path(&data_dir, &agent_id);
        let current_pid = std::process::id();
        let current_starttime = child_starttime(current_pid);
        let runtime_record = AgentRuntimeRecord {
            schema_version: "v1alpha1".to_string(),
            agent_id: agent_id.clone(),
            runner_kind: Some("provider".to_string()),
            runtime_identity_ref: None,
            role_kind: Some("literature_researcher".to_string()),
            authority_scope: Some("worker_evidence_only".to_string()),
            session_ref: None,
            task_packet_ref: task_packet_path.display().to_string(),
            lifecycle_status: "running".to_string(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            heartbeat_at: "1".to_string(),
            output_manifest_ref: root.join("output_manifest.json").display().to_string(),
            workspace_binding_ref: None,
            directive_ref: None,
            status_ref: Some(status_path.display().to_string()),
            trace_refs: Vec::new(),
            stop_reason: String::new(),
            failure_code: String::new(),
        };
        atomic_write(
            &task_packet_path,
            &serde_json::to_string_pretty(&packet).expect("task packet should serialize"),
        )
        .expect("task packet should write");
        atomic_write(
            &runtime_path(&data_dir, &agent_id),
            &serde_json::to_string_pretty(&runtime_record).expect("runtime should serialize"),
        )
        .expect("runtime should write");
        atomic_write(
            &status_path,
            &render_provider_running_status(
                &packet,
                "openai",
                "gpt-5.4",
                "1",
                "1",
                current_pid,
                current_starttime,
            ),
        )
        .expect("status should write");

        let result = reclaim_stale_running_agent(&data_dir, &agent_id, 1)
            .expect("reclaim should inspect supervisor before failing stale heartbeat");

        assert!(!result.reclaimed);
        assert_eq!(result.reason, "supervisor_process_alive");
        let runtime_after = load_runtime(&data_dir, &agent_id).expect("runtime should load");
        assert_eq!(runtime_after.lifecycle_status, "running");
    }

    #[test]
    fn provider_running_status_persists_supervisor_starttime_when_available() {
        let workspace_root = unique_test_workspace("provider_running_status_starttime");
        ensure_source_git_repository(&workspace_root).expect("git repo should initialize");
        ensure_source_worktree_has_head(&workspace_root).expect("git head should bootstrap");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let workspace_binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            &packet.agent_id,
            &packet.created_at,
        )
        .expect("worktree should create");
        let task_packet_path = task_packet_path(&data_dir, &packet.agent_id);
        let runtime_record_path = runtime_path(&data_dir, &packet.agent_id);
        let output_manifest_path = output_manifest_path(&data_dir, &packet.agent_id);
        let trace_path = trace_path(&data_dir, &packet.agent_id);
        let runtime_identity_path = runtime_identity_path(&data_dir, &packet.agent_id);
        let workspace_binding_path = workspace_binding_path(&data_dir, &packet.agent_id);
        let input_mounts_path = input_mounts_path(&data_dir, &packet.agent_id);
        let status_path = status_path(&data_dir, &packet.agent_id);
        fs::create_dir_all(agent_dir(&data_dir, &packet.agent_id).join("traces"))
            .expect("trace dir should create");
        atomic_write(
            &task_packet_path,
            &serde_json::to_string_pretty(&packet).expect("packet should serialize"),
        )
        .expect("packet should write");
        atomic_write(
            &workspace_binding_path,
            &serde_json::to_string_pretty(&workspace_binding)
                .expect("workspace binding should serialize"),
        )
        .expect("workspace binding should write");
        atomic_write(&input_mounts_path, "{}").expect("input mounts should write");

        persist_provider_running_record(
            &packet,
            "openai",
            "gpt-5.4",
            &workspace_binding,
            &task_packet_path,
            &runtime_record_path,
            &output_manifest_path,
            &trace_path,
            &runtime_identity_path,
            &workspace_binding_path,
            &input_mounts_path,
            &status_path,
            "2",
            "trace_running_starttime",
            "manifest_running_starttime",
            Some(1234),
            Some(5678),
        )
        .expect("running record should persist");

        let status = fs::read_to_string(status_path).expect("status should read");
        assert!(status.contains("supervisor_pid: 1234"));
        assert!(status.contains("supervisor_starttime: \"5678\""));
    }

    #[test]
    fn provider_background_start_reaps_failed_worker_and_clears_proc_entry() {
        let workspace_root = unique_test_workspace("provider_background_reap");
        ensure_source_git_repository(&workspace_root).expect("git repo should initialize");
        ensure_source_worktree_has_head(&workspace_root).expect("git head should bootstrap");
        let data_dir = workspace_root.join(".pmcli");
        let result = start_provider_background(
            &data_dir,
            &workspace_root,
            ProviderAgentStartRequest {
                intent: "exercise background provider lifecycle".to_string(),
                role_profile: "literature_researcher".to_string(),
                message: "produce background lifecycle evidence".to_string(),
                provider_id: "not-a-real-provider".to_string(),
                model: "not-a-real-model".to_string(),
                automation_mode: None,
                stage_task_contract: None,
            },
        )
        .expect("background provider start should succeed");

        let status_path = std::path::PathBuf::from(
            result
                .runtime_record
                .status_ref
                .clone()
                .expect("status ref should exist"),
        );
        let mut supervisor_pid = None;
        let mut supervisor_starttime = None;
        let initial_status = fs::read_to_string(&status_path).expect("status should read");
        if let Some(pid) = parse_supervisor_pid_from_status(&initial_status) {
            supervisor_pid = Some(pid);
        }
        if let Some(starttime) = parse_supervisor_starttime_from_status(&initial_status) {
            supervisor_starttime = Some(starttime);
        }
        assert!(supervisor_pid.is_some(), "supervisor pid should be present");
        assert!(
            supervisor_starttime.is_some(),
            "supervisor starttime should be present"
        );
        let supervisor_pid = supervisor_pid.expect("supervisor pid should exist");
        let supervisor_starttime = supervisor_starttime.expect("supervisor starttime should exist");

        let proc_status_path = std::path::PathBuf::from(format!("/proc/{supervisor_pid}/status"));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if std::time::Instant::now() >= deadline {
                panic!("background provider supervisor did not exit or get reaped");
            }
            match fs::read_to_string(&proc_status_path) {
                Ok(contents) => {
                    if contents.contains("State:\tZ") || contents.contains("State: Z") {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    if process_id_matches_starttime(supervisor_pid, Some(supervisor_starttime)) {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    break;
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
                Err(err) => panic!("failed to inspect supervisor process status: {err}"),
            }
        }
    }

    #[test]
    fn process_id_matches_starttime_requires_exact_match_when_starttime_is_known() {
        let pid = std::process::id();
        let actual_starttime =
            child_starttime(pid).expect("current process starttime should exist");
        assert!(process_id_matches_starttime(pid, Some(actual_starttime)));
        assert!(!process_id_matches_starttime(
            pid,
            Some(actual_starttime.saturating_add(1))
        ));
    }

    #[test]
    fn persist_provider_running_record_records_supervisor_state_fields() {
        let workspace_root = unique_test_workspace("provider_running_record_fields");
        ensure_source_git_repository(&workspace_root).expect("git repo should initialize");
        ensure_source_worktree_has_head(&workspace_root).expect("git head should bootstrap");
        let data_dir = workspace_root.join(".pmcli");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let workspace_binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            &packet.agent_id,
            &packet.created_at,
        )
        .expect("worktree should create");
        let task_packet_path = task_packet_path(&data_dir, &packet.agent_id);
        let runtime_record_path = runtime_path(&data_dir, &packet.agent_id);
        let output_manifest_path = output_manifest_path(&data_dir, &packet.agent_id);
        let trace_path = trace_path(&data_dir, &packet.agent_id);
        let runtime_identity_path = runtime_identity_path(&data_dir, &packet.agent_id);
        let workspace_binding_path = workspace_binding_path(&data_dir, &packet.agent_id);
        let input_mounts_path = input_mounts_path(&data_dir, &packet.agent_id);
        let status_path = status_path(&data_dir, &packet.agent_id);
        fs::create_dir_all(agent_dir(&data_dir, &packet.agent_id).join("traces"))
            .expect("trace dir should create");
        atomic_write(
            &task_packet_path,
            &serde_json::to_string_pretty(&packet).expect("packet should serialize"),
        )
        .expect("packet should write");
        atomic_write(
            &workspace_binding_path,
            &serde_json::to_string_pretty(&workspace_binding)
                .expect("workspace binding should serialize"),
        )
        .expect("workspace binding should write");
        atomic_write(&input_mounts_path, "{}").expect("input mounts should write");

        persist_provider_running_record(
            &packet,
            "openai",
            "gpt-5.4",
            &workspace_binding,
            &task_packet_path,
            &runtime_record_path,
            &output_manifest_path,
            &trace_path,
            &runtime_identity_path,
            &workspace_binding_path,
            &input_mounts_path,
            &status_path,
            "2",
            "trace_provider_record",
            "manifest_provider_record",
            Some(1234),
            Some(5678),
        )
        .expect("running record should persist");

        let runtime_record: AgentRuntimeRecord = serde_json::from_str(
            &fs::read_to_string(runtime_record_path).expect("runtime record should read"),
        )
        .expect("runtime record should parse");
        assert_eq!(runtime_record.lifecycle_status, "running");
        let status_ref = status_path.display().to_string();
        assert_eq!(
            runtime_record.status_ref.as_deref(),
            Some(status_ref.as_str())
        );
        let status = fs::read_to_string(status_path).expect("status should read");
        assert!(status.contains("supervisor_pid: 1234"));
        assert!(status.contains("supervisor_starttime: \"5678\""));
    }

    #[test]
    fn provider_worker_full_auto_uses_blocking_timeout_policy() {
        let workspace_root = unique_test_workspace("provider_worker_full_auto_blocking");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_full_auto_blocking".to_string(),
            agent_id: "agent_full_auto_blocking".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute provider timeout policy test".to_string(),
            role_profile: "literature_researcher".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: provider_run_class(Some("full_auto")).to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 120_000,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("literature_researcher"),
            tool_policy: Some(test_role_package_tool_policy("literature_researcher")),
            command: None,
            message: "produce evidence".to_string(),
            created_at: "1".to_string(),
        };

        assert_eq!(packet.run_class, "full_auto_blocking");
        match provider_worker_timeout_policy(&packet) {
            crate::providers::ProviderTimeoutPolicy::TaskBudget(limit) => {
                assert_eq!(limit, Duration::from_millis(120_000));
            }
            other => panic!("expected task-budget timeout policy, got {other:?}"),
        }
        assert_eq!(provider_run_class(Some("human_in_the_loop")), "bounded");
        assert_eq!(provider_run_class(Some("high_autonomy")), "bounded");
        assert_eq!(provider_run_class(None), "bounded");
    }

    #[test]
    fn agent_result_types_round_trip_through_serde_json() {
        let runtime_record = AgentRuntimeRecord {
            schema_version: "agent_runtime_record.v1".to_string(),
            agent_id: "agent_fixture".to_string(),
            runner_kind: Some("provider".to_string()),
            runtime_identity_ref: Some("runtime_identity_ref".to_string()),
            role_kind: Some("researcher".to_string()),
            authority_scope: Some("workspace_write".to_string()),
            session_ref: Some("session_fixture".to_string()),
            task_packet_ref: "task_packet_ref".to_string(),
            lifecycle_status: "running".to_string(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            heartbeat_at: "1".to_string(),
            output_manifest_ref: "manifest_ref".to_string(),
            workspace_binding_ref: Some("workspace_binding_ref".to_string()),
            directive_ref: Some("directive_ref".to_string()),
            status_ref: Some("status_ref".to_string()),
            trace_refs: vec!["trace_ref".to_string()],
            stop_reason: "stop".to_string(),
            failure_code: String::new(),
        };
        let output_manifest = AgentOutputManifest {
            schema_version: "agent_output_manifest.v1".to_string(),
            agent_id: "agent_fixture".to_string(),
            manifest_id: "manifest_fixture".to_string(),
            status: "ok".to_string(),
            output_refs: vec![AgentOutputRef {
                kind: "stdout".to_string(),
                r#ref: "stdout_ref".to_string(),
                summary: "summary".to_string(),
            }],
            validation_status: "valid".to_string(),
            validation_errors: Vec::new(),
            created_at: "1".to_string(),
        };
        let trace = AgentTrace {
            schema_version: "agent_trace.v1".to_string(),
            agent_id: "agent_fixture".to_string(),
            trace_id: "trace_fixture".to_string(),
            task_packet_ref: "task_packet_ref".to_string(),
            runtime_events: vec![AgentTraceEvent {
                event: "start".to_string(),
                detail: "detail".to_string(),
                timestamp: "1".to_string(),
            }],
            tool_actions: Vec::new(),
            permission_decisions: Vec::new(),
            output_records: Vec::new(),
            final_status: "ok".to_string(),
            created_at: "1".to_string(),
            trace_path: "trace.json".to_string(),
        };
        let workspace_binding = AgentWorkspaceBinding {
            schema_version: "agent_workspace_binding.v1".to_string(),
            agent_id: "agent_fixture".to_string(),
            mode: "worktree".to_string(),
            source_workspace_root: "/workspace".to_string(),
            worktree_path: "/workspace/.worktree".to_string(),
            git_head: "abc123".to_string(),
            baseline_ref: Some("baseline_ref".to_string()),
            baseline_mode: Some("full".to_string()),
            canonical_artifact_ledger_ref: Some("ledger_ref".to_string()),
            canonical_artifact_ledger_version: Some("1".to_string()),
            overlay_ref: Some("overlay_ref".to_string()),
            dirty_overlay_ref: None,
            dirty_patch_ref: None,
            untracked_manifest_ref: None,
            created_at: "1".to_string(),
        };
        let start_result = AgentStartResult {
            status: "started".to_string(),
            agent_id: "agent_fixture".to_string(),
            task_packet_path: "task_packet.json".to_string(),
            runtime_record_path: "runtime.json".to_string(),
            output_manifest_path: "output_manifest.json".to_string(),
            trace_path: "trace.json".to_string(),
            workspace_binding: Some(workspace_binding.clone()),
            task_packet: TaskPacket {
                schema_version: "v1alpha1".to_string(),
                task_packet_id: "task_packet_fixture".to_string(),
                agent_id: "agent_fixture".to_string(),
                runner_kind: Some("provider".to_string()),
                intent: "fixture".to_string(),
                role_profile: "research-worker".to_string(),
                retention_policy: "ephemeral".to_string(),
                run_class: "bounded".to_string(),
                io_mode: "request_response".to_string(),
                resume_policy: "fresh_thread".to_string(),
                replay_seed_ref: String::new(),
                budget: AgentBudget {
                    max_turns: 1,
                    max_runtime_ms: 1,
                },
                scope: AgentScope {
                    workspace_root: "/workspace".to_string(),
                    allowed_paths: vec![".".to_string()],
                },
                write_authority: "workspace_write".to_string(),
                success_criteria: vec!["evidence".to_string()],
                output_manifest_required: true,
                review_gate_required: true,
                stage_task_contract: None,
                collaboration_protocol: None,
                skill_refs: Vec::new(),
                tool_policy: None,
                command: None,
                message: "hello".to_string(),
                created_at: "1".to_string(),
            },
            runtime_record: runtime_record.clone(),
            output_manifest: output_manifest.clone(),
            trace: trace.clone(),
        };
        let list_result = AgentListResult {
            agents: vec![AgentListEntry {
                agent_id: "agent_fixture".to_string(),
                lifecycle_status: "running".to_string(),
                runner_kind: Some("provider".to_string()),
                intent: "fixture".to_string(),
                role_profile: "research-worker".to_string(),
                created_at: "1".to_string(),
                updated_at: "1".to_string(),
                task_packet_ref: "task_packet.json".to_string(),
                output_manifest_ref: "output_manifest.json".to_string(),
                workspace_binding_ref: Some("workspace_binding.json".to_string()),
            }],
            total_count: 1,
        };
        let inspection_result = AgentInspectionResult {
            agent_id: "agent_fixture".to_string(),
            agent_root: "/workspace/.pmcli/agents/agent_fixture".to_string(),
            task_packet: start_result.task_packet.clone(),
            runtime_record: runtime_record.clone(),
            output_manifest: output_manifest.clone(),
            trace: trace.clone(),
            workspace_binding: Some(workspace_binding.clone()),
        };
        let stop_result = AgentStopResult {
            agent_id: "agent_fixture".to_string(),
            idempotent: false,
            runtime_record: runtime_record.clone(),
        };
        let reclaim_result = AgentStaleRunningReclaimResult {
            agent_id: "agent_fixture".to_string(),
            reclaimed: true,
            reason: "stale heartbeat".to_string(),
            runtime_record: runtime_record.clone(),
        };
        let traces_result = AgentTracesResult {
            agent_id: "agent_fixture".to_string(),
            trace_path: "trace.json".to_string(),
            trace: trace.clone(),
        };

        assert_round_trip(&start_result);
        assert_round_trip(&list_result);
        assert_round_trip(&inspection_result);
        assert_round_trip(&stop_result);
        assert_round_trip(&reclaim_result);
        assert_round_trip(&traces_result);
        assert_round_trip(&workspace_binding);
        assert_round_trip(&runtime_record);
        assert_round_trip(&output_manifest);
        assert_round_trip(&trace);
    }

    #[test]
    fn provider_worker_prompt_includes_shared_collaboration_protocol() {
        let workspace_root = unique_test_workspace("provider_worker_prompt_protocol");
        let protocol = default_agent_collaboration_protocol();
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_protocol".to_string(),
            agent_id: "agent_protocol".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute protocol prompt test".to_string(),
            role_profile: "literature_researcher".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 1,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(protocol),
            skill_refs: test_role_package_skill_refs("literature_researcher"),
            tool_policy: Some(test_role_package_tool_policy("literature_researcher")),
            command: None,
            message: "produce evidence".to_string(),
            created_at: "1".to_string(),
        };

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Astra collaboration protocol"));
        assert!(rendered.contains("astra_main_runtime_worker_v1"));
        assert!(rendered.contains("collaboration_protocol"));
        assert!(rendered.contains("authority boundary and output contract"));
        assert!(rendered.contains("executes only the assigned TaskPacket"));
        assert!(rendered.contains("candidate-only"));
        assert!(rendered.contains("Do not mark the research stage complete"));
    }

    #[test]
    fn provider_worker_prompt_separates_canonical_paths_from_input_bundles() {
        let workspace_root = unique_test_workspace("provider_worker_prompt_canonical_paths");
        let packet = test_task_packet(&workspace_root, "provider", "implementation");

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Canonical project paths are authoritative"));
        assert!(rendered.contains("`.pmcli/input-bundles` is provenance/context only"));
        assert!(rendered.contains("must not be treated as canonical project source"));
        assert!(rendered.contains("report a canonical artifact dependency blocker"));
        assert!(rendered.contains("baseline_mode=overlay"));
    }

    #[test]
    fn worker_stage_task_contract_includes_required_canonical_artifacts() {
        let workspace_root = unique_test_workspace("provider_worker_required_canonical_artifacts");
        let mut packet = test_task_packet(&workspace_root, "provider", "implementation");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_fixture::runner_check".to_string(),
            stage_execution_id: "stage_fixture".to_string(),
            stage_id: "implement-solution".to_string(),
            task_type: "integration check".to_string(),
            worker_role: "implementation_worker".to_string(),
            objective: "Use the canonical runner as the only project source.".to_string(),
            input_artifact_refs: vec!["accepted_worker_evidence_task:runner_build".to_string()],
            required_canonical_artifacts: vec![canonical_artifacts::RequiredCanonicalArtifact {
                target_artifact_path: "baseline_runner.py".to_string(),
                required_status: "integration_verified".to_string(),
                dependency_kind: "runnable".to_string(),
                reason: Some("downstream checks must execute the canonical runner".to_string()),
            }],
            required_output_artifact_type: "integration_report".to_string(),
            required_output_fields: vec!["command".to_string(), "result".to_string()],
            acceptance_checks: vec!["uses canonical project path".to_string()],
            failure_signals: vec!["uses input bundle as project source".to_string()],
            depends_on_task_ids: vec!["runner_build".to_string()],
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("required_canonical_artifacts"));
        assert!(rendered.contains("baseline_runner.py"));
        assert!(rendered.contains("integration_verified"));
        assert!(rendered.contains("runnable"));
        assert!(rendered.contains("input_artifact_refs"));
        assert!(rendered.contains("historical evidence or provenance context"));
    }

    #[test]
    fn provider_worker_prompt_includes_consumed_input_output_contract() {
        let workspace_root = unique_test_workspace("provider_worker_consumed_input_prompt");
        let mut packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::paper_search".to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "paper search".to_string(),
            worker_role: "literature_researcher".to_string(),
            objective: "Search papers from mounted rubric evidence.".to_string(),
            input_artifact_refs: vec![
                "accepted_worker_evidence_task:research_stage_task::stage_literature_1::acceptance_standard_setting".to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["citation ledger".to_string()],
            acceptance_checks: vec!["uses mounted evidence".to_string()],
            failure_signals: vec!["ignores accepted rubric".to_string()],
            depends_on_task_ids: vec![
                "research_stage_task::stage_literature_1::acceptance_standard_setting".to_string(),
            ],
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Consumed-input protocol"));
        assert!(rendered.contains("consumed_input_required=true"));
        assert!(rendered.contains("## Consumed Input Refs"));
        assert!(rendered.contains(".pmcli/worker-input-artifacts.json"));
        assert!(rendered.contains(
            "accepted_worker_evidence_task:research_stage_task::stage_literature_1::acceptance_standard_setting"
        ));
    }

    #[test]
    fn source_verification_worker_prompt_requires_durable_ledger_candidate() {
        let workspace_root = unique_test_workspace("source_verification_ledger_prompt");
        let packet = source_verification_task_packet(&workspace_root);

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Literature task artifact protocol"));
        assert!(rendered.contains("`source_verification_ledger`"));
        assert!(rendered.contains("durable row-level artifact"));
        assert!(rendered.contains("write a clean candidate Markdown file"));
        assert!(rendered.contains("not a substitute for the candidate file"));
    }

    #[test]
    fn source_verification_prompt_requires_candidate_even_with_literature_matrix_output_type() {
        let workspace_root = unique_test_workspace("source_verification_matrix_prompt");
        let mut packet = source_verification_task_packet(&workspace_root);
        packet
            .stage_task_contract
            .as_mut()
            .expect("source verification contract")
            .required_output_artifact_type = "literature_matrix".to_string();

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("`literature_matrix`"));
        assert!(rendered.contains("durable row-level artifact"));
        assert!(rendered.contains("write a clean candidate Markdown file"));
    }

    #[test]
    fn provider_worker_prompt_includes_assigned_skill_refs_and_tool_policy() {
        let workspace_root = unique_test_workspace("provider_worker_prompt_skill_policy");
        let packet = test_task_packet(&workspace_root, "provider", "citation_auditor");

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Assigned worker skills"));
        assert!(rendered.contains("citation_auditor"));
        assert!(rendered.contains("builtin://roles/citation_auditor/SKILL.md"));
        assert!(rendered.contains("Worker tool policy"));
        assert!(rendered.contains("Mounted worker tools"));
        assert!(rendered.contains("Use only mounted tools"));
        assert!(rendered.contains(
            "read_file, list_files, search, fetch, paper_search, write_file, worker_shell"
        ));
        assert!(!provider_worker_tool_scope(&packet)
            .iter()
            .any(|tool| tool == "apply_patch"));
    }

    #[test]
    fn literature_comparison_worker_prompt_requires_clean_stage_artifact_candidate() {
        let workspace_root = unique_test_workspace("literature_comparison_clean_candidate_prompt");
        let packet = test_task_packet(
            &workspace_root,
            "provider",
            "literature_comparison_researcher",
        );

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("clean review target body"));
        assert!(rendered.contains("Do not put candidate-only lifecycle metadata"));
        assert!(rendered.contains("paper_search"));
    }

    #[test]
    fn provider_worker_prompt_includes_tool_budget_closeout_contract() {
        let workspace_root = unique_test_workspace("provider_worker_prompt_budget_closeout");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Worker execution budget"));
        assert!(rendered.contains("Tool-call budget: at most 24 calls"));
        assert!(rendered.contains("stop broad exploration"));
        assert!(rendered.contains("write a task-local artifact"));
        assert!(rendered.contains("missing evidence and repair tasks"));
    }

    #[test]
    fn provider_worker_prompt_includes_standard_setting_evidence_plan_protocol() {
        let workspace_root = unique_test_workspace("provider_worker_standard_setting_protocol");
        let mut packet = test_task_packet(&workspace_root, "provider", "stage_standard_setter");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::acceptance_standard_setting"
                .to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "acceptance standard setting".to_string(),
            worker_role: "stage_standard_setter".to_string(),
            objective: "Draft strict literature acceptance criteria.".to_string(),
            input_artifact_refs: Vec::new(),
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "stage_acceptance_rubric".to_string(),
            required_output_fields: vec!["CandidateStageEvidencePlan".to_string()],
            acceptance_checks: vec!["candidate plan is reviewable".to_string()],
            failure_signals: vec!["candidate plan missing".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: false,
        });

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Stage-standard-setting task protocol"));
        assert!(rendered.contains("stage_acceptance_rubric_literature.md"));
        assert!(rendered.contains("not complete if no safe candidate artifact file is written"));
        assert!(rendered.contains("read_file` `offset`/`max_chars`"));
        assert!(rendered.contains("CandidateStageEvidencePlan"));
        assert!(rendered.contains("evidence_requirements"));
        assert!(rendered.contains("record_stage_evidence_plan"));
        assert!(rendered.contains("The candidate plan is not adopted"));
        assert!(rendered.contains("Do not publish board tasks"));
    }

    #[test]
    fn provider_worker_tool_policy_filters_mounted_tools() {
        let workspace_root = unique_test_workspace("provider_worker_tool_policy");
        let mut packet = test_task_packet(&workspace_root, "provider", "implementation");
        packet.tool_policy = Some(AgentToolPolicy {
            schema_version: "agent_tool_policy.v1".to_string(),
            allowed_tools: vec!["read_file".to_string(), "fetch".to_string()],
        });

        let tool_scope = provider_worker_tool_scope(&packet);

        assert_eq!(
            tool_scope,
            vec!["read_file".to_string(), "fetch".to_string()]
        );
    }

    #[test]
    fn provider_worker_synthesis_prompt_uses_mounted_inputs() {
        let workspace_root = unique_test_workspace("provider_worker_synthesis_prompt");
        fs::create_dir_all(
            workspace_root
                .join("research")
                .join("stages")
                .join("stage_experiment_1")
                .join("experiment-plan"),
        )
        .expect("stage dir should write");
        fs::write(
            workspace_root
                .join("research")
                .join("stages")
                .join("stage_experiment_1")
                .join("experiment-plan")
                .join("experiment_plan.md"),
            "# Existing Experiment Plan\n",
        )
        .expect("input artifact should write");
        let mut packet = test_task_packet(&workspace_root, "provider", "research_synthesizer");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "board::stage_experiment_1::experiment_plan_synthesis".to_string(),
            stage_execution_id: "stage_experiment_1".to_string(),
            stage_id: "experiment-plan".to_string(),
            task_type: "experiment plan synthesis".to_string(),
            worker_role: "research_synthesizer".to_string(),
            objective: "Synthesize accepted worker evidence into a standalone plan.".to_string(),
            input_artifact_refs: vec![
                "research/stages/stage_experiment_1/experiment-plan/experiment_plan.md".to_string(),
                ".pmcli/agents/agent_budget_plan/provider_worker_evidence.md".to_string(),
                "accepted_worker_evidence_index:/tmp/workspace/research/stages/job_1/stage_experiment_1/accepted_worker_evidence/index.json".to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "experiment_plan".to_string(),
            required_output_fields: vec!["run matrix".to_string(), "budget plan".to_string()],
            acceptance_checks: vec!["uses accepted worker evidence".to_string()],
            failure_signals: vec!["broad workspace search".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Synthesis protocol"));
        assert!(rendered.contains("This is a stage synthesis task"));
        assert!(rendered.contains("Astra Research Synthesizer Soul"));
        assert!(rendered.contains("builtin://roles/research_synthesizer/SKILL.md"));
        assert!(rendered.contains(".pmcli/worker-input-artifacts.json"));
        assert!(rendered.contains("stage_task_contract.input_artifact_refs"));
        assert!(rendered.contains("not for broad workspace exploration"));
        assert!(rendered.contains("one standalone candidate artifact"));
        assert!(rendered.contains("explicit repair tasks"));
        assert!(rendered.contains("Do not decide stage completion"));
        assert!(rendered.contains("status: `available`"));
        assert!(rendered
            .contains("research/stages/stage_experiment_1/experiment-plan/experiment_plan.md"));
        assert!(rendered.contains(
            "accepted_worker_evidence_index:/tmp/workspace/research/stages/job_1/stage_experiment_1/accepted_worker_evidence/index.json"
        ));
    }

    #[test]
    fn task_packet_rejects_forbidden_worker_tool_policy() {
        let workspace_root = unique_test_workspace("forbidden_worker_tool_policy");
        let data_dir = workspace_root.join(".pmcli");
        let mut packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        packet.tool_policy = Some(AgentToolPolicy {
            schema_version: "agent_tool_policy.v1".to_string(),
            allowed_tools: vec!["read_file".to_string(), "publish_board_tasks".to_string()],
        });

        let err = validate_provider_task_packet(&data_dir, &workspace_root, &packet)
            .expect_err("worker task packet must reject main-agent tools");

        match err {
            AgentError::InvalidTaskPacket { reason } => {
                assert!(reason.contains("forbidden tool `publish_board_tasks`"));
            }
            other => panic!("expected invalid task packet, got {other:?}"),
        }
    }

    #[test]
    fn provider_worker_role_soul_stays_in_system_message_only() {
        let workspace_root = unique_test_workspace("provider_worker_role_soul");
        fs::create_dir_all(
            workspace_root
                .join(".research-cli")
                .join("roles")
                .join("literature_researcher"),
        )
        .expect("role dir should write");
        fs::write(
            workspace_root
                .join(".research-cli")
                .join("roles")
                .join("literature_researcher")
                .join("SOUL.md"),
            "# Role Soul\n\nProject-specific literature worker identity.",
        )
        .expect("role soul should write");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_role_soul".to_string(),
            agent_id: "agent_role_soul".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute role soul prompt test".to_string(),
            role_profile: "literature_researcher".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 1,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("literature_researcher"),
            tool_policy: Some(test_role_package_tool_policy("literature_researcher")),
            command: None,
            message: "produce evidence".to_string(),
            created_at: "1".to_string(),
        };

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);

        assert!(messages[0]
            .content
            .contains("Project-specific literature worker identity."));
        assert!(!messages[1]
            .content
            .contains("Project-specific literature worker identity."));
    }

    #[test]
    fn provider_worker_uses_profile_specific_builtin_role_soul() {
        let workspace_root = unique_test_workspace("provider_worker_builtin_profile_soul");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_builtin_role_soul".to_string(),
            agent_id: "agent_builtin_role_soul".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute built-in role soul prompt test".to_string(),
            role_profile: "citation_auditor".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 1,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("citation_auditor"),
            tool_policy: Some(test_role_package_tool_policy("citation_auditor")),
            command: None,
            message: "audit citation metadata".to_string(),
            created_at: "1".to_string(),
        };

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);

        assert!(messages[0].content.contains("Astra Citation Auditor Soul"));
        assert!(messages[0]
            .content
            .contains("builtin://roles/citation_auditor/SOUL.md"));
        assert!(!messages[1].content.contains("Astra Citation Auditor Soul"));
        assert!(!messages[1]
            .content
            .contains("builtin://roles/citation_auditor/SOUL.md"));
    }

    #[test]
    fn provider_worker_loop_budget_scales_from_task_runtime_budget() {
        let workspace_root = unique_test_workspace("provider_worker_loop_budget");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_loop_budget".to_string(),
            agent_id: "agent_loop_budget".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute provider loop budget test".to_string(),
            role_profile: "literature_researcher".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 120_000,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("literature_researcher"),
            tool_policy: Some(test_role_package_tool_policy("literature_researcher")),
            command: None,
            message: "produce evidence".to_string(),
            created_at: "1".to_string(),
        };

        let budget = provider_worker_loop_budget(&packet);

        assert!(budget.max_iterations > 8);
        assert_eq!(budget.max_iterations, 120);
        assert_eq!(budget.max_tool_calls, 24);
        assert_eq!(budget.max_elapsed, Some(Duration::from_millis(120_000)));
        assert!(budget.finalize_on_tool_budget_exhaustion);
        assert_eq!(budget.closeout_tool_call_threshold, 8);
    }

    #[test]
    fn provider_worker_closeout_threshold_scales_early_enough_for_retrieval() {
        assert_eq!(provider_worker_closeout_tool_threshold(12), 4);
        assert_eq!(provider_worker_closeout_tool_threshold(24), 8);
        assert_eq!(provider_worker_closeout_tool_threshold(60), 20);
    }

    fn provider_budget_test_contract(
        task_type: &str,
        input_ref_count: usize,
    ) -> AgentStageTaskContract {
        provider_budget_test_contract_with_counts(task_type, input_ref_count, 1, 1, 1)
    }

    fn provider_budget_test_contract_with_counts(
        task_type: &str,
        input_ref_count: usize,
        required_output_field_count: usize,
        acceptance_check_count: usize,
        failure_signal_count: usize,
    ) -> AgentStageTaskContract {
        AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: format!(
                "research_stage_task::stage_budget_test::{}",
                task_type.trim().replace([' ', '-'], "_")
            ),
            stage_execution_id: "stage_budget_test".to_string(),
            stage_id: "literature".to_string(),
            task_type: task_type.to_string(),
            worker_role: "research_worker".to_string(),
            objective: "Exercise provider worker budget derivation.".to_string(),
            input_artifact_refs: (0..input_ref_count)
                .map(|index| format!("accepted_worker_evidence_ref_{index}"))
                .collect(),
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: (0..required_output_field_count)
                .map(|index| format!("field_{index}"))
                .collect(),
            acceptance_checks: (0..acceptance_check_count)
                .map(|index| format!("check_{index}"))
                .collect(),
            failure_signals: (0..failure_signal_count)
                .map(|index| format!("failure_{index}"))
                .collect(),
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        }
    }

    #[test]
    fn provider_worker_stage_task_budget_keeps_default_for_small_worker_tasks() {
        let contract = provider_budget_test_contract("paper search", 3);

        let budget = provider_worker_budget_for_stage_task(Some(&contract));

        assert_eq!(budget.max_turns, 1);
        assert_eq!(
            budget.max_runtime_ms,
            DEFAULT_PROVIDER_WORKER_MAX_RUNTIME_MS
        );
    }

    #[test]
    fn provider_worker_stage_task_budget_does_not_extend_by_task_name() {
        let contract = provider_budget_test_contract("open-problem extraction", 3);

        let budget = provider_worker_budget_for_stage_task(Some(&contract));

        assert_eq!(budget.max_turns, 1);
        assert_eq!(
            budget.max_runtime_ms,
            DEFAULT_PROVIDER_WORKER_MAX_RUNTIME_MS
        );
    }

    #[test]
    fn provider_worker_stage_task_budget_extends_for_large_input_ref_sets() {
        let contract = provider_budget_test_contract(
            "method comparison",
            PROVIDER_WORKER_EXTENDED_INPUT_REF_THRESHOLD,
        );

        let budget = provider_worker_budget_for_stage_task(Some(&contract));

        assert_eq!(
            budget.max_runtime_ms,
            EXTENDED_PROVIDER_WORKER_MAX_RUNTIME_MS
        );
    }

    #[test]
    fn provider_worker_stage_task_budget_extends_for_high_structured_complexity() {
        let contract =
            provider_budget_test_contract_with_counts("acceptance standard setting", 3, 15, 7, 6);

        let budget = provider_worker_budget_for_stage_task(Some(&contract));

        assert_eq!(
            provider_worker_stage_task_structured_complexity(&contract),
            PROVIDER_WORKER_EXTENDED_TASK_COMPLEXITY_THRESHOLD + 1
        );
        assert_eq!(
            budget.max_runtime_ms,
            EXTENDED_PROVIDER_WORKER_MAX_RUNTIME_MS
        );
    }

    #[test]
    fn provider_worker_stage_task_budget_extends_for_wide_output_review_surface() {
        let contract = provider_budget_test_contract_with_counts(
            "literature source verification",
            2,
            15,
            6,
            5,
        );

        let budget = provider_worker_budget_for_stage_task(Some(&contract));

        assert!(
            provider_worker_stage_task_structured_complexity(&contract)
                < PROVIDER_WORKER_EXTENDED_TASK_COMPLEXITY_THRESHOLD
        );
        assert_eq!(
            budget.max_runtime_ms,
            EXTENDED_PROVIDER_WORKER_MAX_RUNTIME_MS
        );
    }

    #[test]
    fn create_git_worktree_applies_current_canonical_overlay_baseline() {
        let workspace_root = unique_test_workspace("canonical_overlay_worktree");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = workspace_root.join(".pmcli");
        let target_path = workspace_root.join("baseline_runner.py");
        fs::write(&target_path, "def run():\n    return 1\n").expect("target should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_overlay".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_overlay".to_string(),
            source_agent_id: "agent_source".to_string(),
            source_task_id: Some("task_source".to_string()),
            source_ref: ".pmcli/agents/agent_source/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("baseline_runner.py".to_string()),
            target_artifact_path: "baseline_runner.py".to_string(),
            artifact_kind: "python_source".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::overlay".to_string(),
            source_sha256: None,
        };
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            seed,
            "main agent requested overlay adoption",
        )
        .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            canonical_artifacts::sha256_file(&target_path).expect("target sha should compute"),
        )
        .expect("materialized should record");
        let baseline = canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline should promote");

        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_overlay", "1")
            .expect("worker worktree should create");

        assert_eq!(
            binding.baseline_ref.as_deref(),
            Some(baseline.baseline_ref.as_str())
        );
        assert_eq!(binding.baseline_mode.as_deref(), Some("overlay"));
        let copied = Path::new(&binding.worktree_path).join("baseline_runner.py");
        assert_eq!(
            fs::read_to_string(copied).expect("overlay file should be visible"),
            "def run():\n    return 1\n"
        );
    }

    #[test]
    fn create_git_worktree_fails_when_canonical_overlay_checksum_mismatches() {
        let workspace_root = unique_test_workspace("canonical_overlay_checksum_mismatch");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = workspace_root.join(".pmcli");
        let target_path = workspace_root.join("baseline_runner.py");
        fs::write(&target_path, "def run():\n    return 1\n").expect("target should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_overlay_bad_checksum".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_overlay_bad_checksum".to_string(),
            source_agent_id: "agent_source".to_string(),
            source_task_id: Some("task_source".to_string()),
            source_ref: ".pmcli/agents/agent_source/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("baseline_runner.py".to_string()),
            target_artifact_path: "baseline_runner.py".to_string(),
            artifact_kind: "python_source".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::overlay_bad_checksum".to_string(),
            source_sha256: None,
        };
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            seed,
            "main agent requested overlay adoption",
        )
        .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            canonical_artifacts::sha256_file(&target_path).expect("target sha should compute"),
        )
        .expect("materialized should record");
        let baseline = canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline should promote");
        let overlay_entry = baseline
            .entries
            .iter()
            .find(|entry| entry.target_artifact_path == "baseline_runner.py")
            .expect("overlay entry should exist");
        fs::write(
            workspace_root.join(&overlay_entry.overlay_artifact_path),
            "def run():\n    return 999\n",
        )
        .expect("overlay corruption should write");

        let err = create_git_worktree(&data_dir, &workspace_root, "agent_overlay_bad", "1")
            .expect_err("corrupt overlay must fail worker creation");

        match err {
            AgentError::LocalCommand(message) => {
                assert!(message.contains("canonical baseline overlay failed"));
                assert!(message.contains("checksum mismatch"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn canonical_overlay_baseline_contains_only_adopted_artifacts() {
        let workspace_root = unique_test_workspace("canonical_overlay_excludes_noise");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = workspace_root.join(".pmcli");
        let adopted_path = workspace_root.join("baseline_runner.py");
        let noise_path = workspace_root.join("unadopted_noise.py");
        fs::write(&adopted_path, "def run():\n    return 1\n").expect("target should write");
        fs::write(&noise_path, "def noisy():\n    return 0\n").expect("noise should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_overlay_no_noise".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_overlay_no_noise".to_string(),
            source_agent_id: "agent_source".to_string(),
            source_task_id: Some("task_source".to_string()),
            source_ref: ".pmcli/agents/agent_source/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("baseline_runner.py".to_string()),
            target_artifact_path: "baseline_runner.py".to_string(),
            artifact_kind: "python_source".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::overlay_no_noise".to_string(),
            source_sha256: None,
        };
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            seed,
            "main agent requested overlay adoption",
        )
        .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            canonical_artifacts::sha256_file(&adopted_path).expect("target sha should compute"),
        )
        .expect("materialized should record");
        let baseline = canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline should promote");

        assert!(baseline
            .entries
            .iter()
            .any(|entry| entry.target_artifact_path == "baseline_runner.py"));
        assert!(!baseline
            .entries
            .iter()
            .any(|entry| entry.target_artifact_path == "unadopted_noise.py"));
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_overlay_clean", "1")
            .expect("worker worktree should create");
        assert!(Path::new(&binding.worktree_path)
            .join("baseline_runner.py")
            .exists());
        assert_eq!(binding.baseline_mode.as_deref(), Some("overlay"));
        assert!(binding.dirty_overlay_ref.is_some());
        assert!(
            Path::new(&binding.worktree_path)
                .join("unadopted_noise.py")
                .exists(),
            "dirty user workspace files should be visible to worker overlay"
        );
    }

    #[test]
    fn user_branch_is_not_polluted_by_overlay_baseline_promotion() {
        let workspace_root = unique_test_workspace("canonical_overlay_no_user_branch_pollution");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = workspace_root.join(".pmcli");
        let original_head = String::from_utf8(
            Command::new("git")
                .current_dir(&workspace_root)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("git rev-parse should execute")
                .stdout,
        )
        .expect("HEAD should be utf8")
        .trim()
        .to_string();
        fs::write(
            workspace_root.join("user_uncommitted_notes.md"),
            "local user draft that must not enter Astra baseline\n",
        )
        .expect("user uncommitted note should write");
        let adopted_path = workspace_root.join("baseline_runner.py");
        fs::write(&adopted_path, "def run():\n    return 1\n").expect("target should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_overlay_no_pollution".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_overlay_no_pollution".to_string(),
            source_agent_id: "agent_source".to_string(),
            source_task_id: Some("task_source".to_string()),
            source_ref: ".pmcli/agents/agent_source/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("baseline_runner.py".to_string()),
            target_artifact_path: "baseline_runner.py".to_string(),
            artifact_kind: "python_source".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::overlay_no_pollution".to_string(),
            source_sha256: None,
        };
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            seed,
            "main agent requested overlay adoption",
        )
        .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            canonical_artifacts::sha256_file(&adopted_path).expect("target sha should compute"),
        )
        .expect("materialized should record");
        let baseline = canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline should promote");

        let after_head = String::from_utf8(
            Command::new("git")
                .current_dir(&workspace_root)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("git rev-parse should execute")
                .stdout,
        )
        .expect("HEAD should be utf8")
        .trim()
        .to_string();
        assert_eq!(
            after_head, original_head,
            "Astra overlay promotion must not create a user-visible commit"
        );
        let staged = Command::new("git")
            .current_dir(&workspace_root)
            .args(["diff", "--cached", "--name-only"])
            .output()
            .expect("git diff --cached should execute");
        assert!(staged.status.success());
        assert!(
            String::from_utf8_lossy(&staged.stdout).trim().is_empty(),
            "Astra overlay promotion must not alter the user index"
        );
        assert!(!baseline
            .entries
            .iter()
            .any(|entry| entry.target_artifact_path == "user_uncommitted_notes.md"));
        let binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            "agent_overlay_no_pollution",
            "1",
        )
        .expect("worker worktree should create");
        let worktree = Path::new(&binding.worktree_path);
        assert!(worktree.join("baseline_runner.py").exists());
        assert_eq!(binding.baseline_mode.as_deref(), Some("overlay"));
        assert!(binding.dirty_overlay_ref.is_some());
        assert!(
            worktree.join("user_uncommitted_notes.md").exists(),
            "uncommitted user files should be visible in the worker dirty overlay"
        );
    }

    #[test]
    fn create_git_worktree_fails_when_directory_overlay_checksum_mismatches() {
        let workspace_root = unique_test_workspace("canonical_overlay_directory_checksum_mismatch");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = workspace_root.join(".pmcli");
        let package_root = workspace_root.join("experiments").join("bench_pkg");
        fs::create_dir_all(package_root.join("nested")).expect("package directory should create");
        fs::write(package_root.join("__init__.py"), "VALUE = 1\n")
            .expect("package init should write");
        fs::write(
            package_root.join("nested").join("runner.py"),
            "def run():\n    return 1\n",
        )
        .expect("package runner should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_overlay_directory_bad_checksum".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_overlay_directory_bad_checksum".to_string(),
            source_agent_id: "agent_source".to_string(),
            source_task_id: Some("task_source".to_string()),
            source_ref: ".pmcli/agents/agent_source/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("experiments/bench_pkg".to_string()),
            target_artifact_path: "experiments/bench_pkg".to_string(),
            artifact_kind: "directory".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::overlay_directory_bad_checksum"
                .to_string(),
            source_sha256: None,
        };
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            seed,
            "main agent requested directory overlay adoption",
        )
        .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            "directory-manifest-fixture".to_string(),
        )
        .expect("materialized should record");
        let baseline = canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline should promote");
        let overlay_entry = baseline
            .entries
            .iter()
            .find(|entry| entry.target_artifact_path == "experiments/bench_pkg")
            .expect("directory overlay entry should exist");
        fs::write(
            workspace_root
                .join(&overlay_entry.overlay_artifact_path)
                .join("__init__.py"),
            "VALUE = 999\n",
        )
        .expect("overlay corruption should write");

        let err = create_git_worktree(&data_dir, &workspace_root, "agent_overlay_dir_bad", "1")
            .expect_err("corrupt directory overlay must fail worker creation");

        match err {
            AgentError::LocalCommand(message) => {
                assert!(message.contains("canonical baseline overlay failed"));
                assert!(message.contains("checksum mismatch"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn provider_worker_empty_final_output_is_invalid_evidence() {
        let workspace_root = unique_test_workspace("provider_worker_empty_output");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_empty_output".to_string(),
            agent_id: "agent_empty_output".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute empty output validation test".to_string(),
            role_profile: "research_quality_reviewer".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 120_000,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: Some(AgentStageTaskContract {
                schema_version: "agent_stage_task_contract.v1".to_string(),
                task_id: "research_stage_task::stage_fixture::semantic_review".to_string(),
                stage_execution_id: "stage_fixture".to_string(),
                stage_id: "literature".to_string(),
                task_type: "independent semantic review".to_string(),
                worker_role: "research_quality_reviewer".to_string(),
                objective: "Review mounted evidence.".to_string(),
                input_artifact_refs: Vec::new(),
                required_canonical_artifacts: Vec::new(),
                required_output_artifact_type: "independent_semantic_review".to_string(),
                required_output_fields: vec!["verdict".to_string()],
                acceptance_checks: vec!["returns a verdict".to_string()],
                failure_signals: vec!["empty output".to_string()],
                depends_on_task_ids: Vec::new(),
                priority: 1,
                review_findings_refs: Vec::new(),
                blocker_refs: Vec::new(),
                review_target_task_ids: vec![
                    "research_stage_task::stage_fixture::paper_search".to_string()
                ],
                review_target_evidence_refs: Vec::new(),
                supersedes_task_ids: Vec::new(),
                replacement_of_task_ids: Vec::new(),
                current_evidence_set_id: None,
                consumed_input_required: false,
            }),
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("research_quality_reviewer"),
            tool_policy: Some(test_role_package_tool_policy("research_quality_reviewer")),
            command: None,
            message: "review evidence".to_string(),
            created_at: "1".to_string(),
        };

        let errors = validate_provider_worker_final_output(
            &packet,
            "   \n",
            &empty_candidate_manifest(&packet.agent_id),
        );

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("without final Markdown evidence text"));
        assert!(errors[0].contains("research_stage_task::stage_fixture::semantic_review"));
    }

    #[test]
    fn provider_worker_final_output_requires_consumed_input_ref_when_contract_requires_it() {
        let workspace_root = unique_test_workspace("provider_worker_consumed_input_required");
        let mut packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_consumed_input_required".to_string(),
            agent_id: "agent_consumed_input_required".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute consumed input validation test".to_string(),
            role_profile: "research_worker".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 1_000,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: Some(AgentStageTaskContract {
                schema_version: "agent_stage_task_contract.v1".to_string(),
                task_id: "research_stage_task::stage_fixture::repair".to_string(),
                stage_execution_id: "stage_fixture".to_string(),
                stage_id: "literature".to_string(),
                task_type: "repair synthesis".to_string(),
                worker_role: "literature_repair_worker".to_string(),
                objective: "Repair mounted evidence.".to_string(),
                input_artifact_refs: vec![
                    "accepted_worker_evidence_task:task_paper_search".to_string()
                ],
                required_canonical_artifacts: Vec::new(),
                required_output_artifact_type: "repair_report".to_string(),
                required_output_fields: vec!["repair".to_string()],
                acceptance_checks: vec!["uses mounted evidence".to_string()],
                failure_signals: vec!["ignores mounted evidence".to_string()],
                depends_on_task_ids: vec!["task_paper_search".to_string()],
                priority: 1,
                review_findings_refs: Vec::new(),
                blocker_refs: Vec::new(),
                review_target_task_ids: Vec::new(),
                review_target_evidence_refs: Vec::new(),
                supersedes_task_ids: Vec::new(),
                replacement_of_task_ids: Vec::new(),
                current_evidence_set_id: Some("stage_fixture::task_paper_search".to_string()),
                consumed_input_required: true,
            }),
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("literature_repair_worker"),
            tool_policy: Some(test_role_package_tool_policy("literature_repair_worker")),
            command: None,
            message: "repair evidence".to_string(),
            created_at: "1".to_string(),
        };

        let errors = validate_provider_worker_final_output(
            &packet,
            "I produced a repair report from general context but do not cite inputs.",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("consumed_input_required=true"));

        let errors = validate_provider_worker_final_output(
            &packet,
            "I consumed `.pmcli/worker-input-artifacts.json` and accepted_worker_evidence_task:task_paper_search before writing the repair.",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(errors.is_empty());

        packet
            .stage_task_contract
            .as_mut()
            .expect("contract")
            .input_artifact_refs
            .clear();
        let errors = validate_provider_worker_final_output(
            &packet,
            "I consumed stage_fixture::task_paper_search before writing the repair.",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn provider_worker_standard_setting_output_requires_safe_candidate_file() {
        let workspace_root = unique_test_workspace("provider_worker_standard_setting_validation");
        let mut packet = test_task_packet(&workspace_root, "provider", "stage_standard_setter");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::acceptance_standard_setting"
                .to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "acceptance standard setting".to_string(),
            worker_role: "stage_standard_setter".to_string(),
            objective: "Draft strict literature acceptance criteria.".to_string(),
            input_artifact_refs: Vec::new(),
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "stage_acceptance_rubric".to_string(),
            required_output_fields: vec!["CandidateStageEvidencePlan".to_string()],
            acceptance_checks: vec!["candidate plan is reviewable".to_string()],
            failure_signals: vec!["candidate plan missing".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: false,
        });

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Stage Acceptance Rubric Draft\n\nexpert acceptance target: strict\nstage-specific pass criteria: strong\nreview-to-task routing: repair",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("without a safe task-local candidate artifact file")),
            "{errors:?}"
        );

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Stage Acceptance Rubric Draft\n\nexpert acceptance target: strict\nstage-specific pass criteria: strong\nreview-to-task routing: repair\n\n## CandidateStageEvidencePlan\nstage_id: literature\nstage_execution_id: stage_literature_1\nplan_source_refs: accepted_worker_evidence:acceptance standard setting\nrationale: candidate only for main-agent review\nevidence_requirements:\n- task_type: paper search\n  worker_role: literature_researcher\n  objective: retrieve prior work\n  required_output_artifact_type: literature_matrix\n  required_output_fields: source entries\n  acceptance_checks: row-level source entries\n  failure_signals: generic summary\n  evidence_standard: strict reviewer can trace claims",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("without a safe task-local candidate artifact file")),
            "{errors:?}"
        );
    }

    #[test]
    fn provider_worker_literature_matrix_shape_is_not_runtime_hard_failure() {
        let workspace_root = unique_test_workspace("provider_worker_literature_matrix_validation");
        let mut packet = test_task_packet(&workspace_root, "provider", "literature_gap_analyst");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::open_problem_extraction".to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "open-problem extraction".to_string(),
            worker_role: "literature_gap_analyst".to_string(),
            objective: "Extract grounded literature gaps.".to_string(),
            input_artifact_refs: vec![
                ".pmcli/reviews/rev_fixture/packet.json".to_string(),
                ".pmcli/agents/agent_fixture/provider_worker_evidence.md".to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec![
                "research question".to_string(),
                "citation ledger".to_string(),
                "source entries".to_string(),
                "canonical verified title".to_string(),
                "source verification status".to_string(),
                "metadata confidence".to_string(),
                "open problem".to_string(),
                "closest existing coverage".to_string(),
                "coverage boundary".to_string(),
                "why gap matters".to_string(),
                "relation to current objective".to_string(),
                "claim support boundary".to_string(),
            ],
            acceptance_checks: vec![
                "row-level source grounding".to_string(),
                "each gap cites representative literature rows".to_string(),
            ],
            failure_signals: vec!["generic summary".to_string()],
            depends_on_task_ids: vec![
                "research_stage_task::stage_literature_1::paper_search".to_string()
            ],
            priority: 1,
            review_findings_refs: vec!["rev_fixture".to_string()],
            blocker_refs: Vec::new(),
            review_target_task_ids: vec![
                "research_stage_task::stage_literature_1::paper_search".to_string()
            ],
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: Some("stage_fixture::evidence".to_string()),
            consumed_input_required: true,
        });

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Literature Matrix\n\nThis is a weak meta-review of earlier outputs, but it is still inspectable worker evidence.\n\n## Evidence Produced\n- Read `.pmcli/worker-input-artifacts.json`\n- Reviewed accepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search\n- Confirmed the stage is hard to close\n",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(
            errors.is_empty(),
            "runtime must not hard-fail worker evidence because it lacks a task-specific semantic template; semantic adequacy belongs to main-agent/reviewer judgment: {errors:?}"
        );

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Literature Matrix\n\nThis is a meta-review of earlier outputs, but I do not cite mounted inputs.",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("consumed_input_required=true")),
            "mechanical consumed-input checks still apply: {errors:?}"
        );

        let errors = validate_provider_worker_final_output(
            &packet,
            "   \n",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("without final Markdown evidence text")),
            "empty output remains mechanically invalid: {errors:?}"
        );

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Literature Matrix\n\n### Citation Ledger\n- source_id: s1\n  canonical verified title: Paper A\n  source verification status: VERIFIED\n  metadata confidence: high\n  claim support boundary: only supports search-space framing\n- source_id: s2\n  canonical verified title: Paper B\n  source verification status: PROVISIONAL_ABSTRACT_ONLY\n  metadata confidence: medium\n  claim support boundary: limited to abstract-level support\n\n### Source Entries\n- row 1: source_id=s1 method family=benchmarking\n- row 2: source_id=s2 method family=decision-making\n\n### Open Problems\n| gap_id | open problem | source_id | canonical verified title | source verification status | metadata confidence | closest existing coverage | coverage boundary | why gap matters | relation to current objective | claim support boundary |\n| gap_1 | no row-level benchmark framing | s1 | Paper A | VERIFIED | high | benchmark family covers generic tasks | not chaotic-context optimal decision | keeps claims bounded | benchmark under chaotic context | supports only search-space framing |\n| gap_2 | missing closest-family boundary | s1 | Paper A | VERIFIED | high | broad benchmark harness | no disorder axis | prevents overclaim | benchmark under chaotic context | supports comparison only |\n| gap_3 | no verified citation ledger | s2 | Paper B | PROVISIONAL_ABSTRACT_ONLY | medium | decision benchmark abstract | metadata-only support | blocks final novelty | benchmark under chaotic context | provisional only |\n| gap_4 | no optimality oracle | s1 | Paper A | VERIFIED | high | task evaluation | lacks optimal decision criterion | blocks benchmark scoring | benchmark under chaotic context | supports need for oracle audit |\n| gap_5 | no context chaos taxonomy | s2 | Paper B | PROVISIONAL_ABSTRACT_ONLY | medium | noisy input study | not systematic chaos taxonomy | guides experiment design | benchmark under chaotic context | provisional search constraint |\n\n### Why Gap Matters\n- keeps claims bounded\n\n### Relation to Current Objective\n- benchmark under chaotic context\n\n### Claim Support Boundary\n- provisional rows do not support strong novelty\n\n### Consumed Input Refs\n- accepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search",
            &empty_candidate_manifest(&packet.agent_id),
        );
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn provider_worker_open_problem_extraction_meta_summary_is_reviewable_evidence() {
        let workspace_root = unique_test_workspace("provider_worker_open_problem_meta_summary");
        let mut packet = test_task_packet(&workspace_root, "provider", "literature_gap_analyst");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::open_problem_extraction".to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "open-problem extraction".to_string(),
            worker_role: "literature_gap_analyst".to_string(),
            objective: "Repair literature-stage open-problem extraction.".to_string(),
            input_artifact_refs: vec![
                "accepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search"
                    .to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec![
                "citation ledger".to_string(),
                "source entries".to_string(),
                "open problem".to_string(),
                "coverage boundary".to_string(),
                "claim support boundary".to_string(),
            ],
            acceptance_checks: vec![
                "Evidence itself must exhibit >=5 open problems/gaps with row-level source grounding."
                    .to_string(),
            ],
            failure_signals: vec![
                "Gap claims are plausible but not tied to exhibited literature rows.".to_string(),
            ],
            depends_on_task_ids: vec![
                "research_stage_task::stage_literature_1::paper_search".to_string(),
            ],
            priority: 1,
            review_findings_refs: vec!["rev_fixture".to_string()],
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });

        let output = "## Task contract summary\nCompleted open-problem extraction review.\n\n\
            ## Evidence produced\n\
            - Read accepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search\n\
            - Prior worker evidence confirms source entries and a citation ledger exist.\n\
            - The previous matrix has canonical verified title, source verification status, metadata confidence, and claim support boundary fields.\n\n\
            ## Acceptance-check coverage\n\
            - Evidence shows a row-level matrix with 15 sources.\n\
            - It still needs final main-agent judgment on open problems.\n";

        let errors = validate_provider_worker_final_output(
            &packet,
            output,
            &empty_candidate_manifest(&packet.agent_id),
        );

        assert!(
            errors.is_empty(),
            "runtime records mechanically valid worker evidence even when semantic adequacy is doubtful: {errors:?}"
        );
    }

    #[test]
    fn source_verification_validation_requires_safe_candidate_file() {
        let workspace_root = unique_test_workspace("source_verification_candidate_validation");
        let packet = source_verification_task_packet(&workspace_root);
        let candidates = empty_candidate_manifest(&packet.agent_id);

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Source Verification Ledger\n\nsource id: s1\ncanonical verified title: Paper A\nsource verification status: verified\nmetadata confidence: high\nclaim support boundary: bounded\nmissing-source risks: none\n\n## Consumed Input Refs\naccepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search",
            &candidates,
        );

        assert!(
            errors.iter().any(|error| {
                error.contains("without a safe task-local candidate artifact file")
                    && error.contains("source_verification_ledger")
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn source_verification_validation_requires_candidate_for_literature_matrix_output_type() {
        let workspace_root = unique_test_workspace("source_verification_matrix_validation");
        let mut packet = source_verification_task_packet(&workspace_root);
        packet
            .stage_task_contract
            .as_mut()
            .expect("source verification contract")
            .required_output_artifact_type = "literature_matrix".to_string();
        let candidates = empty_candidate_manifest(&packet.agent_id);

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Source Verification\n\nsource id: s1\ncanonical verified title: Paper A\nsource verification status: verified\nmetadata confidence: high\nclaim support boundary: bounded\nmissing-source risks: none\n\n## Consumed Input Refs\naccepted_worker_evidence_task:research_stage_task::stage_literature_1::paper_search",
            &candidates,
        );

        assert!(
            errors.iter().any(|error| {
                error.contains("without a safe task-local candidate artifact file")
                    && error.contains("literature_matrix")
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn source_verification_validation_reports_failed_write_file_delivery() {
        let workspace_root = unique_test_workspace("source_verification_write_file_validation");
        let packet = source_verification_task_packet(&workspace_root);
        let candidates = empty_candidate_manifest(&packet.agent_id);
        let tool_actions = vec![trace_event(
            "provider_tool_completed",
            "tool=write_file; call_id=call_fixture; status=error",
            "1",
        )];

        let errors =
            validate_provider_worker_tool_action_delivery(&packet, &tool_actions, &candidates);

        assert!(
            errors.iter().any(|error| {
                error.contains("failed write_file attempt")
                    && error.contains("source_verification_ledger")
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn provider_worker_standard_setting_validation_reads_candidate_files() {
        let workspace_root =
            unique_test_workspace("provider_worker_standard_setting_candidate_file_validation");
        let mut packet = test_task_packet(&workspace_root, "provider", "stage_standard_setter");
        packet.agent_id = "agent_standard_candidate".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::acceptance_standard_setting"
                .to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "acceptance standard setting".to_string(),
            worker_role: "stage_standard_setter".to_string(),
            objective: "Draft strict literature acceptance criteria.".to_string(),
            input_artifact_refs: Vec::new(),
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "stage_acceptance_rubric".to_string(),
            required_output_fields: vec!["CandidateStageEvidencePlan".to_string()],
            acceptance_checks: vec!["candidate plan is reviewable".to_string()],
            failure_signals: vec!["candidate plan missing".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: false,
        });
        let archive_dir = workspace_root.join(".pmcli").join("candidate_archive");
        fs::create_dir_all(&archive_dir).expect("archive dir should write");
        let archive_ref = archive_dir.join("stage_acceptance_rubric_literature.md");
        fs::write(
            &archive_ref,
            "## CandidateStageEvidencePlan\nstage_id: literature\nstage_execution_id: stage_literature_1\nplan_source_refs: accepted_worker_evidence:acceptance standard setting\nrationale: candidate only for main-agent review\nevidence_requirements:\n- task_type: paper search\n  worker_role: literature_researcher\n  objective: retrieve prior work\n  required_output_artifact_type: literature_matrix\n  required_output_fields: source entries\n  acceptance_checks: row-level source entries\n  failure_signals: generic summary\n  evidence_standard: strict reviewer can trace claims\n",
        )
        .expect("candidate archive should write");
        let mut candidates = empty_candidate_manifest(&packet.agent_id);
        candidates
            .candidate_entries
            .push(AgentWorktreeArtifactCandidateEntry {
                relative_path: "stage_acceptance_rubric_literature.md".to_string(),
                path_kind: "file".to_string(),
                artifact_kind: "stage_acceptance_rubric".to_string(),
                size_bytes: fs::metadata(&archive_ref).expect("metadata").len(),
                sha256: "fixture".to_string(),
                candidate_archive_ref: Some(archive_ref.display().to_string()),
                safe_status: "safe".to_string(),
                unsafe_reason: None,
                from_input_bundle: false,
                is_directory: false,
                directory_manifest_ref: None,
                captured_at: "1".to_string(),
            });

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Stage Acceptance Rubric Draft\n\nSee candidate file for the structured plan.",
            &candidates,
        );

        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn provider_worker_validation_rejects_unsafe_candidate_files() {
        let workspace_root = unique_test_workspace("provider_worker_unsafe_candidate_validation");
        let mut packet = test_task_packet(&workspace_root, "provider", "stage_standard_setter");
        packet.agent_id = "agent_unsafe_candidate".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::acceptance_standard_setting"
                .to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "acceptance standard setting".to_string(),
            worker_role: "stage_standard_setter".to_string(),
            objective: "Draft strict literature acceptance criteria.".to_string(),
            input_artifact_refs: Vec::new(),
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "stage_acceptance_rubric".to_string(),
            required_output_fields: vec!["CandidateStageEvidencePlan".to_string()],
            acceptance_checks: vec!["candidate plan is reviewable".to_string()],
            failure_signals: vec!["candidate plan missing".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: false,
        });
        let mut candidates = empty_candidate_manifest(&packet.agent_id);
        candidates
            .candidate_entries
            .push(AgentWorktreeArtifactCandidateEntry {
                relative_path: ".pmcli/input-bundles/rev_fixture/0-packet.json".to_string(),
                path_kind: "file".to_string(),
                artifact_kind: "json".to_string(),
                size_bytes: 32,
                sha256: "fixture".to_string(),
                candidate_archive_ref: Some("archive.json".to_string()),
                safe_status: "unsafe".to_string(),
                unsafe_reason: Some(
                    "candidate path is runtime or evidence-bundle state".to_string(),
                ),
                from_input_bundle: true,
                is_directory: false,
                directory_manifest_ref: None,
                captured_at: "1".to_string(),
            });

        let errors = validate_provider_worker_final_output(
            &packet,
            "## Stage Acceptance Rubric Draft\n\nSee candidate file.",
            &candidates,
        );

        assert!(
            errors
                .iter()
                .any(|error| error.contains("unsafe candidate file")),
            "{errors:?}"
        );
    }

    #[test]
    fn provider_worker_evidence_renders_runtime_validation_errors() {
        let workspace_root = unique_test_workspace("provider_worker_validation_render");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_validation_render".to_string(),
            agent_id: "agent_validation_render".to_string(),
            runner_kind: Some("provider".to_string()),
            intent: "execute validation render test".to_string(),
            role_profile: "research_worker".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 120_000,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("research_worker"),
            tool_policy: Some(test_role_package_tool_policy("research_worker")),
            command: None,
            message: "produce evidence".to_string(),
            created_at: "1".to_string(),
        };

        let evidence = render_provider_worker_evidence(
            &packet,
            "openai",
            "gpt-5.4",
            "",
            &["provider worker returned empty output".to_string()],
            &empty_candidate_manifest(&packet.agent_id),
            0,
            None,
        );

        assert!(evidence.contains("## Worker Output"));
        assert!(evidence.contains("## Runtime Output Validation"));
        assert!(evidence.contains("provider worker returned empty output"));
        assert!(evidence.contains("## Runtime Mechanical Report"));
        assert!(evidence.contains("- output_bytes: `0`"));
        assert!(evidence.contains("- semantic_result_owner: `main_agent_or_reviewer`"));
        assert!(evidence.contains("- runtime_boundary: `quantitative_delivery_facts_only`"));
    }

    #[test]
    fn provider_worker_evidence_renders_quantitative_mechanical_report() {
        let workspace_root = unique_test_workspace("provider_worker_mechanical_report");
        let mut packet = test_task_packet(&workspace_root, "provider", "literature_gap_analyst");
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::synthesis".to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "stage artifact synthesis".to_string(),
            worker_role: "research_synthesizer".to_string(),
            objective: "Synthesize accepted evidence into a candidate artifact.".to_string(),
            input_artifact_refs: vec!["accepted_worker_evidence_task:paper_search".to_string()],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["stage artifact body".to_string()],
            acceptance_checks: vec!["uses mounted evidence".to_string()],
            failure_signals: vec!["ignores mounted evidence".to_string()],
            depends_on_task_ids: vec!["paper_search".to_string()],
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: Some("stage_literature_1::paper_search".to_string()),
            consumed_input_required: true,
        });
        let mut candidates = empty_candidate_manifest(&packet.agent_id);
        candidates
            .candidate_entries
            .push(AgentWorktreeArtifactCandidateEntry {
                relative_path: "research/stages/job/literature/literature_matrix.md".to_string(),
                path_kind: "file".to_string(),
                artifact_kind: "markdown".to_string(),
                size_bytes: 321,
                sha256: "fixture-sha".to_string(),
                candidate_archive_ref: Some(
                    workspace_root
                        .join(".pmcli")
                        .join("agents")
                        .join("agent_fixture")
                        .join("candidate_archive")
                        .join("literature_matrix.md")
                        .display()
                        .to_string(),
                ),
                safe_status: "safe".to_string(),
                unsafe_reason: None,
                from_input_bundle: false,
                is_directory: false,
                directory_manifest_ref: None,
                captured_at: "1".to_string(),
            });

        let evidence = render_provider_worker_evidence(
            &packet,
            "openai",
            "gpt-5.4",
            "I consumed `.pmcli/worker-input-artifacts.json` and accepted_worker_evidence_task:paper_search.\nThe candidate artifact is inspectable.",
            &[],
            &candidates,
            3,
            None,
        );

        assert!(evidence.contains("## Runtime Mechanical Report"));
        assert!(evidence.contains("- validation_status: `valid`"));
        assert!(evidence.contains("- candidate_file_count: `1`"));
        assert!(evidence.contains("- candidate_total_bytes: `321`"));
        assert!(evidence.contains("- input_ref_count: `3`"));
        assert!(evidence.contains("- input_ref_mention_count: `4`"));
        assert!(evidence.contains("- tool_call_count: `3`"));
        assert!(evidence.contains("- path_safety_ok: `true`"));
        assert!(evidence.contains("- hash_applicable: `true`"));
        assert!(evidence.contains("- hash_recorded: `true`"));
        assert!(evidence.contains("- runtime_boundary: `quantitative_delivery_facts_only`"));
    }

    #[test]
    fn provider_worker_mechanical_report_does_not_record_hash_for_empty_candidates() {
        let workspace_root = unique_test_workspace("provider_worker_no_candidate_hash");
        let packet = test_task_packet(&workspace_root, "provider", "literature_gap_analyst");
        let candidates = empty_candidate_manifest(&packet.agent_id);

        let evidence = render_provider_worker_evidence(
            &packet,
            "openai",
            "gpt-5.4",
            "I inspected the mounted input and returned evidence text without a candidate file.",
            &[],
            &candidates,
            1,
            None,
        );

        assert!(evidence.contains("- candidate_file_count: `0`"));
        assert!(evidence.contains("- hash_applicable: `false`"));
        assert!(evidence.contains("- hash_recorded: `false`"));
    }

    #[test]
    fn provider_worker_evidence_renders_runtime_tool_receipts() {
        let workspace_root = unique_test_workspace("provider_worker_tool_receipts");
        let packet = test_task_packet(&workspace_root, "provider", "literature_gap_analyst");
        let candidates = empty_candidate_manifest(&packet.agent_id);
        let receipts = AgentToolReceiptManifest {
            schema_version: "agent_tool_receipt_manifest.v1".to_string(),
            agent_id: packet.agent_id.clone(),
            authority_scope: "worker_evidence_only".to_string(),
            session_ref: Some("sess_fixture".to_string()),
            receipt_count: 1,
            receipts: vec![AgentToolReceipt {
                call_index: 1,
                call_id: Some("call_fetch".to_string()),
                tool_name: "fetch".to_string(),
                status: "succeeded".to_string(),
                arguments: serde_json::json!({"url": "https://example.test/paper"}),
                output_bytes: 42,
                output_sha256: "fixture-output-sha".to_string(),
                output_preview: "Fetched title and abstract.".to_string(),
                snapshot_ref: Some(
                    workspace_root
                        .join(".pmcli/agents/agent_fixture/tool_receipts/snapshots/001_fetch.txt")
                        .display()
                        .to_string(),
                ),
                snapshot_bytes: 27,
                snapshot_truncated: false,
                structured_summary: Some(serde_json::json!({
                    "url": "https://example.test/paper",
                    "status_code": 200,
                    "bytes": 42
                })),
                recorded_at: "1".to_string(),
            }],
            generated_at: "1".to_string(),
        };

        let evidence = render_provider_worker_evidence(
            &packet,
            "openai",
            "gpt-5.4",
            "Worker completed source verification.",
            &[],
            &candidates,
            1,
            Some(&receipts),
        );

        assert!(evidence.contains("## Runtime Tool Receipts"));
        assert!(evidence.contains("Tool Receipt 1: `fetch`"));
        assert!(evidence.contains("https://example.test/paper"));
        assert!(evidence.contains("fixture-output-sha"));
        assert!(evidence.contains("snapshot_ref"));
        assert!(evidence.contains("runtime_boundary: `tool_call_facts_only`"));
    }

    #[test]
    fn provider_worker_partial_candidate_evidence_preserves_artifacts_after_finalize_failure() {
        let workspace_root = unique_test_workspace("provider_worker_partial_candidate");
        let packet = test_task_packet(&workspace_root, "provider", "literature_gap_analyst");
        let mut candidates = empty_candidate_manifest(&packet.agent_id);
        candidates
            .candidate_entries
            .push(AgentWorktreeArtifactCandidateEntry {
                relative_path: "literature_matrix.md".to_string(),
                path_kind: "file".to_string(),
                artifact_kind: "markdown".to_string(),
                size_bytes: 128,
                sha256: "fixture-sha".to_string(),
                candidate_archive_ref: Some(
                    workspace_root
                        .join(".pmcli")
                        .join("agents")
                        .join("agent_fixture")
                        .join("candidate_archive")
                        .join("literature_matrix.md")
                        .display()
                        .to_string(),
                ),
                safe_status: "safe".to_string(),
                unsafe_reason: None,
                from_input_bundle: false,
                is_directory: false,
                directory_manifest_ref: None,
                captured_at: "1".to_string(),
            });

        assert!(worktree_candidate_manifest_has_safe_file(&candidates));
        let evidence = render_provider_worker_partial_candidate_evidence(
            &packet,
            "openai",
            "gpt-5.4",
            "provider returned HTTP 500",
            &candidates,
        );

        assert!(evidence.contains("provider_finalize_failed: `true`"));
        assert!(evidence.contains("literature_matrix.md"));
        assert!(evidence.contains("candidate-only evidence"));
        assert!(evidence.contains("not a scientific acceptance decision"));
        assert!(evidence.contains("Stage review must still judge"));
    }

    #[test]
    fn worker_input_artifact_mounts_untracked_stage_files_into_worktree() {
        let workspace_root = unique_test_workspace("input_mounts");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let input_path = workspace_root
            .join("research")
            .join("stages")
            .join("job_fixture")
            .join("literature")
            .join("literature_matrix.md");
        fs::create_dir_all(input_path.parent().expect("input parent should exist"))
            .expect("input parent should write");
        fs::write(&input_path, "# Literature Matrix\n\nsource rows\n")
            .expect("input artifact should write");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_inputs", "1")
            .expect("fixture worktree should bind");
        let mut packet = test_task_packet(
            &workspace_root,
            "provider",
            "literature_comparison_researcher",
        );
        packet.agent_id = "agent_inputs".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "board::stage_fixture::paper_clustering".to_string(),
            stage_execution_id: "stage_fixture".to_string(),
            stage_id: "literature".to_string(),
            task_type: "paper clustering".to_string(),
            worker_role: "literature_comparison_researcher".to_string(),
            objective: "Cluster source rows.".to_string(),
            input_artifact_refs: vec![
                "research/stages/job_fixture/literature/literature_matrix.md".to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["source entries".to_string()],
            acceptance_checks: vec!["source rows are available".to_string()],
            failure_signals: vec!["missing input artifact".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        let manifest_path = input_mounts_path(&data_dir, "agent_inputs");

        let manifest = mount_task_input_artifacts(
            "agent_inputs",
            &data_dir,
            &workspace_root,
            &binding,
            &packet,
            &manifest_path,
            "2",
        )
        .expect("input artifacts should mount");

        assert_eq!(manifest.mounted_inputs.len(), 1);
        assert_eq!(
            manifest.mounted_inputs[0].mounted_path,
            "research/stages/job_fixture/literature/literature_matrix.md"
        );
        assert!(Path::new(&binding.worktree_path)
            .join("research/stages/job_fixture/literature/literature_matrix.md")
            .exists());
        assert!(Path::new(&binding.worktree_path)
            .join(".pmcli/worker-input-artifacts.json")
            .exists());
        let binding_path = workspace_binding_path(&data_dir, "agent_inputs");
        let candidates_path = worktree_candidates_path(&data_dir, "agent_inputs");
        let candidate_archive_dir = worktree_candidate_archive_dir(&data_dir, "agent_inputs");
        let patch_path = worktree_patch_path(&data_dir, "agent_inputs");
        let candidates = capture_worktree_artifact_candidates(
            "agent_inputs",
            &packet,
            &binding,
            &binding_path,
            &candidates_path,
            &candidate_archive_dir,
            &patch_path,
            "3",
        )
        .expect("candidate manifest should capture");
        assert!(!candidates
            .changed_paths
            .iter()
            .any(|path| path == "research/stages/job_fixture/literature/literature_matrix.md"));
        assert!(!candidates
            .untracked_paths
            .iter()
            .any(|path| path == "research/stages/job_fixture/literature/literature_matrix.md"));

        let rewritten_stage_artifact = Path::new(&binding.worktree_path)
            .join("research/stages/job_fixture/literature/literature_matrix.md");
        fs::write(
            &rewritten_stage_artifact,
            "# Literature Matrix\n\nWorker synthesized replacement artifact.\n",
        )
        .expect("worker should rewrite stage artifact candidate");
        let rewritten_candidates = capture_worktree_artifact_candidates(
            "agent_inputs",
            &packet,
            &binding,
            &binding_path,
            &candidates_path,
            &candidate_archive_dir,
            &patch_path,
            "4",
        )
        .expect("rewritten candidate manifest should capture");
        assert!(rewritten_candidates
            .changed_paths
            .iter()
            .any(|path| path == "research/stages/job_fixture/literature/literature_matrix.md"));
        assert!(rewritten_candidates
            .untracked_paths
            .iter()
            .any(|path| path == "research/stages/job_fixture/literature/literature_matrix.md"));

        let messages =
            provider_worker_messages(&workspace_root.join(".pmcli"), &workspace_root, &packet);
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Mounted input artifacts"));
        assert!(rendered.contains("status: `available`"));
        assert!(
            rendered.contains("Read mounted input artifacts before declaring task inputs missing")
        );
    }

    #[test]
    fn worker_input_mount_manifest_records_workspace_visibility_boundary() {
        let workspace_root = unique_test_workspace("input_mount_visibility_boundary");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a bootstrap-only baseline commit");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_visibility", "1")
            .expect("fixture worktree should bind");
        let mut packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        packet.agent_id = "agent_visibility".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "research_stage_task::stage_literature_1::paper_search".to_string(),
            stage_execution_id: "stage_literature_1".to_string(),
            stage_id: "literature".to_string(),
            task_type: "paper search".to_string(),
            worker_role: "literature_researcher".to_string(),
            objective: "Find Astra-specific source evidence.".to_string(),
            input_artifact_refs: vec!["accepted_worker_evidence:paper_search".to_string()],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["source entries".to_string()],
            acceptance_checks: vec!["uses Astra-specific evidence".to_string()],
            failure_signals: vec!["only reports blocker rows".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: vec!["review_finding:rev_fixture:missing_sources".to_string()],
            blocker_refs: vec!["worker_review_failure_route::rev_fixture".to_string()],
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        let manifest_path = input_mounts_path(&data_dir, "agent_visibility");

        let manifest = mount_task_input_artifacts(
            "agent_visibility",
            &data_dir,
            &workspace_root,
            &binding,
            &packet,
            &manifest_path,
            "2",
        )
        .expect("symbolic context refs should not block dispatch");

        assert_eq!(
            manifest.workspace_visibility.project_marker_status,
            "no_project_markers_visible"
        );
        assert!(manifest
            .workspace_visibility
            .tracked_root_entries
            .iter()
            .any(|entry| entry == ".astra"));
        assert!(manifest
            .unmounted_input_refs
            .iter()
            .any(|reference| reference == "accepted_worker_evidence:paper_search"));
        assert!(manifest
            .workspace_visibility
            .project_marker_paths_visible
            .is_empty());

        let rendered = render_worker_input_artifacts_for_prompt(
            &data_dir,
            Path::new(&binding.worktree_path),
            &packet,
        );
        assert!(rendered.contains("workspace_visibility_status: `no_project_markers_visible`"));
        assert!(rendered.contains("project_marker_paths_visible: `none`"));
        assert!(rendered.contains("unmounted_input_ref_count: `1`"));
    }

    #[test]
    fn worker_input_artifact_mounts_runtime_agent_artifacts_as_bundle() {
        let workspace_root = unique_test_workspace("runtime_input_bundle");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let source_path = data_dir
            .join("agents")
            .join("agent_source")
            .join("candidate_archive")
            .join("sha_fixture")
            .join("literature_matrix_task.md");
        fs::create_dir_all(source_path.parent().expect("source parent should exist"))
            .expect("source parent should write");
        fs::write(&source_path, "# Source candidate\n").expect("source candidate should write");
        let mut readonly = fs::metadata(&source_path)
            .expect("source metadata should read")
            .permissions();
        readonly.set_readonly(true);
        fs::set_permissions(&source_path, readonly).expect("source should become readonly");

        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_runtime_bundle", "1")
            .expect("fixture worktree should bind");
        let mirrored_runtime_path = Path::new(&binding.worktree_path)
            .join(".pmcli")
            .join("agents")
            .join("agent_source")
            .join("candidate_archive")
            .join("sha_fixture")
            .join("literature_matrix_task.md");
        fs::create_dir_all(
            mirrored_runtime_path
                .parent()
                .expect("mirrored runtime parent should exist"),
        )
        .expect("mirrored runtime parent should write");
        fs::write(&mirrored_runtime_path, "# Existing readonly mirror\n")
            .expect("mirrored runtime artifact should write");
        let mut mirrored_readonly = fs::metadata(&mirrored_runtime_path)
            .expect("mirrored runtime metadata should read")
            .permissions();
        mirrored_readonly.set_readonly(true);
        fs::set_permissions(&mirrored_runtime_path, mirrored_readonly)
            .expect("mirrored runtime artifact should become readonly");

        let mut packet = test_task_packet(
            &workspace_root,
            "provider",
            "literature_comparison_researcher",
        );
        packet.agent_id = "agent_runtime_bundle".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "board::stage_fixture::stage_artifact_synthesis".to_string(),
            stage_execution_id: "stage_fixture".to_string(),
            stage_id: "literature".to_string(),
            task_type: "stage artifact synthesis".to_string(),
            worker_role: "research_synthesizer".to_string(),
            objective: "Repair synthesis using prior candidate evidence.".to_string(),
            input_artifact_refs: vec![
                ".pmcli/agents/agent_source/candidate_archive/sha_fixture/literature_matrix_task.md"
                    .to_string(),
            ],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["citation ledger".to_string()],
            acceptance_checks: vec!["prior candidate is consumed as evidence".to_string()],
            failure_signals: vec!["missing prior candidate".to_string()],
            depends_on_task_ids: Vec::new(),
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        let manifest_path = input_mounts_path(&data_dir, "agent_runtime_bundle");

        let manifest = mount_task_input_artifacts(
            "agent_runtime_bundle",
            &data_dir,
            &workspace_root,
            &binding,
            &packet,
            &manifest_path,
            "2",
        )
        .expect("runtime agent artifact should mount as input bundle");

        assert_eq!(manifest.mounted_inputs.len(), 1);
        assert!(manifest.mounted_inputs[0]
            .mounted_path
            .starts_with(".pmcli/input-bundles/"));
        assert_eq!(
            fs::read_to_string(&mirrored_runtime_path).expect("mirror should remain readable"),
            "# Existing readonly mirror\n"
        );
        assert!(manifest.mounted_inputs[0]
            .bundled_paths
            .iter()
            .any(|path| path.contains("literature_matrix_task")));
    }

    #[test]
    fn input_bundle_files_are_not_captured_as_worker_candidates() {
        let workspace_root = unique_test_workspace("input_bundle_candidate_filter");
        let packet = test_task_packet(&workspace_root, "provider", "stage_standard_setter");

        assert!(!should_capture_worker_candidate_path(
            ".pmcli/input-bundles/rev_fixture/0-packet.json",
            &[],
            &stage_task_candidate_output_path_hints(&packet, &[]),
            &workspace_root,
        ));
    }

    #[test]
    fn task_packet_echo_files_are_not_worker_candidates() {
        let workspace_root = unique_test_workspace("task_packet_echo_candidate_filter");
        let packet = test_task_packet(&workspace_root, "provider", "literature_researcher");
        let hints = stage_task_candidate_output_path_hints(&packet, &[]);

        for path in ["task_packet.json", "task_1782933994483837_packet.json"] {
            assert!(
                !should_capture_worker_candidate_path(path, &[], &hints, &workspace_root),
                "{path} should not be captured as worker evidence"
            );
            let (safe_status, unsafe_reason) = worker_candidate_path_safety(path);
            assert_eq!(safe_status, "unsafe");
            assert_eq!(
                unsafe_reason.as_deref(),
                Some("candidate path is runtime or evidence-bundle state")
            );
        }

        let mut candidates = empty_candidate_manifest(&packet.agent_id);
        candidates
            .candidate_entries
            .push(AgentWorktreeArtifactCandidateEntry {
                relative_path: "task_1782933994483837_packet.json".to_string(),
                path_kind: "file".to_string(),
                artifact_kind: "json".to_string(),
                size_bytes: 848,
                sha256: "fixture-sha".to_string(),
                candidate_archive_ref: Some(
                    "archive/task_1782933994483837_packet.json".to_string(),
                ),
                safe_status: "unsafe".to_string(),
                unsafe_reason: Some(
                    "candidate path is runtime or evidence-bundle state".to_string(),
                ),
                from_input_bundle: false,
                is_directory: false,
                directory_manifest_ref: None,
                captured_at: "1".to_string(),
            });

        assert!(!worktree_candidate_manifest_has_safe_file(&candidates));
    }

    #[test]
    fn worker_input_artifact_mounts_accepted_worker_evidence_task_bundle() {
        let workspace_root = unique_test_workspace("input_bundle");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        fs::create_dir_all(data_dir.join("agents").join("agent_source"))
            .expect("source agent dir should write");
        fs::write(
            data_dir
                .join("agents")
                .join("agent_source")
                .join("task_packet.json"),
            "{}",
        )
        .expect("source task packet should write");
        fs::write(
            data_dir
                .join("agents")
                .join("agent_source")
                .join("output_manifest.json"),
            "{}",
        )
        .expect("source output manifest should write");
        fs::write(
            data_dir
                .join("agents")
                .join("agent_source")
                .join("provider_worker_evidence.md"),
            "# Worker Evidence\n",
        )
        .expect("source evidence should write");
        let source_worktree = unique_test_workspace("input_bundle_source_worktree");
        fs::create_dir_all(source_worktree.join("research").join("literature"))
            .expect("source candidate parent should write");
        fs::write(
            source_worktree
                .join("research")
                .join("literature")
                .join("paper_audit.md"),
            "# Paper Audit\n\nAccepted candidate artifact from the worker worktree.\n",
        )
        .expect("source candidate artifact should write");
        fs::write(
            data_dir
                .join("agents")
                .join("agent_source")
                .join("worktree_artifact_candidates.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "agent_worktree_artifact_candidate_manifest.v1",
                "agent_id": "agent_source",
                "authority_scope": "worker_evidence_only",
                "adoption_status": "candidate_only",
                "workspace_binding_ref": ".pmcli/agents/agent_source/workspace_binding.json",
                "source_workspace_root": workspace_root.display().to_string(),
                "worktree_path": source_worktree.display().to_string(),
                "git_head": "fixture",
                "status_entries": ["?? research/literature/paper_audit.md"],
                "changed_paths": [],
                "untracked_paths": ["research/literature/paper_audit.md"],
                "generated_at": "1"
            }))
            .expect("candidate manifest should serialize"),
        )
        .expect("candidate manifest should write");
        let index_path = workspace_root
            .join("research")
            .join("stages")
            .join("stage_fixture")
            .join("accepted_worker_evidence")
            .join("index.json");
        fs::create_dir_all(index_path.parent().expect("index parent should exist"))
            .expect("index parent should write");
        fs::write(
            &index_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research_accepted_worker_evidence_index.v1",
                "entries": [{
                    "task_id": "paper_search",
                    "task_type": "paper search",
                    "active_status": "active",
                    "main_agent_decision_ref": "main_agent_worker_artifact_decision::accept_paper_search",
                    "task_packet_ref": ".pmcli/agents/agent_source/task_packet.json",
                    "output_manifest_ref": ".pmcli/agents/agent_source/output_manifest.json",
                    "evidence_refs": [
                        ".pmcli/agents/agent_source/provider_worker_evidence.md",
                        ".pmcli/agents/agent_source/worktree_artifact_candidates.json"
                    ]
                }]
            }))
            .expect("index should serialize"),
        )
        .expect("index should write");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_bundle", "1")
            .expect("fixture worktree should bind");
        let mut packet = test_task_packet(
            &workspace_root,
            "provider",
            "literature_comparison_researcher",
        );
        packet.agent_id = "agent_bundle".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "board::stage_fixture::paper_clustering".to_string(),
            stage_execution_id: "stage_fixture".to_string(),
            stage_id: "literature".to_string(),
            task_type: "paper clustering".to_string(),
            worker_role: "literature_comparison_researcher".to_string(),
            objective: "Cluster source rows.".to_string(),
            input_artifact_refs: vec!["accepted_worker_evidence_task:paper_search".to_string()],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["source entries".to_string()],
            acceptance_checks: vec!["source rows are available".to_string()],
            failure_signals: vec!["missing input artifact".to_string()],
            depends_on_task_ids: vec!["paper_search".to_string()],
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        let manifest_path = input_mounts_path(&data_dir, "agent_bundle");

        let manifest = mount_task_input_artifacts(
            "agent_bundle",
            &data_dir,
            &workspace_root,
            &binding,
            &packet,
            &manifest_path,
            "2",
        )
        .expect("accepted worker evidence bundle should mount");

        assert_eq!(manifest.mounted_inputs.len(), 1);
        assert_eq!(
            manifest.mounted_inputs[0].input_ref,
            "accepted_worker_evidence_task:paper_search"
        );
        assert!(manifest.mounted_inputs[0]
            .bundle_manifest_path
            .as_deref()
            .unwrap_or_default()
            .ends_with("bundle-manifest.json"));
        assert!(manifest.mounted_inputs[0]
            .bundled_paths
            .iter()
            .any(|path| path.contains("provider_worker_evidence")));
        assert!(manifest.mounted_inputs[0]
            .bundled_paths
            .iter()
            .any(|path| path.contains("worktree_artifact_candidates")));
        assert!(manifest.mounted_inputs[0]
            .bundled_paths
            .iter()
            .any(|path| path.contains("paper_audit")));
        assert!(Path::new(&binding.worktree_path)
            .join(
                manifest.mounted_inputs[0]
                    .bundle_manifest_path
                    .as_ref()
                    .expect("bundle manifest path")
            )
            .exists());
    }

    #[test]
    fn worker_input_artifact_mounts_reject_rejected_accepted_worker_evidence_task_bundle() {
        let workspace_root = unique_test_workspace("input_bundle_rejected");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        fs::create_dir_all(data_dir.join("agents").join("agent_source"))
            .expect("source agent dir should write");
        fs::write(
            data_dir
                .join("agents")
                .join("agent_source")
                .join("provider_worker_evidence.md"),
            "# Rejected Worker Evidence\n",
        )
        .expect("source evidence should write");
        let index_path = workspace_root
            .join("research")
            .join("stages")
            .join("stage_fixture")
            .join("accepted_worker_evidence")
            .join("index.json");
        fs::create_dir_all(index_path.parent().expect("index parent should exist"))
            .expect("index parent should write");
        fs::write(
            &index_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research_accepted_worker_evidence_index.v1",
                "entries": [{
                    "task_id": "paper_search",
                    "task_type": "paper search",
                    "active_status": "rejected",
                    "main_agent_decision_ref": "main_agent_worker_artifact_decision::reject_paper_search",
                    "evidence_refs": [
                        ".pmcli/agents/agent_source/provider_worker_evidence.md"
                    ]
                }]
            }))
            .expect("index should serialize"),
        )
        .expect("index should write");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_bundle", "1")
            .expect("fixture worktree should bind");
        let mut packet = test_task_packet(
            &workspace_root,
            "provider",
            "literature_comparison_researcher",
        );
        packet.agent_id = "agent_bundle".to_string();
        packet.stage_task_contract = Some(AgentStageTaskContract {
            schema_version: "agent_stage_task_contract.v1".to_string(),
            task_id: "board::stage_fixture::paper_clustering".to_string(),
            stage_execution_id: "stage_fixture".to_string(),
            stage_id: "literature".to_string(),
            task_type: "paper clustering".to_string(),
            worker_role: "literature_comparison_researcher".to_string(),
            objective: "Cluster source rows.".to_string(),
            input_artifact_refs: vec!["accepted_worker_evidence_task:paper_search".to_string()],
            required_canonical_artifacts: Vec::new(),
            required_output_artifact_type: "literature_matrix".to_string(),
            required_output_fields: vec!["source entries".to_string()],
            acceptance_checks: vec!["source rows are available".to_string()],
            failure_signals: vec!["missing input artifact".to_string()],
            depends_on_task_ids: vec!["paper_search".to_string()],
            priority: 1,
            review_findings_refs: Vec::new(),
            blocker_refs: Vec::new(),
            review_target_task_ids: Vec::new(),
            review_target_evidence_refs: Vec::new(),
            supersedes_task_ids: Vec::new(),
            replacement_of_task_ids: Vec::new(),
            current_evidence_set_id: None,
            consumed_input_required: true,
        });
        let manifest_path = input_mounts_path(&data_dir, "agent_bundle");

        let err = mount_task_input_artifacts(
            "agent_bundle",
            &data_dir,
            &workspace_root,
            &binding,
            &packet,
            &manifest_path,
            "2",
        )
        .expect_err("rejected accepted-worker evidence must not mount as a usable bundle");

        assert!(err
            .to_string()
            .contains("required input artifact refs could not be resolved"));
        assert!(err
            .to_string()
            .contains("accepted_worker_evidence_task:paper_search"));
    }

    #[test]
    fn worktree_candidate_manifest_records_untracked_worker_artifacts_without_adoption() {
        let workspace_root = unique_test_workspace("candidate_manifest");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_candidate", "1")
            .expect("fixture worktree should bind");
        let worker_artifact_path = Path::new(&binding.worktree_path)
            .join("research")
            .join("candidate.md");
        fs::create_dir_all(
            worker_artifact_path
                .parent()
                .expect("candidate artifact should have parent"),
        )
        .expect("candidate artifact directory should write");
        fs::write(
            &worker_artifact_path,
            "# Candidate\n\nWorker-only draft artifact.\n",
        )
        .expect("candidate artifact should write");

        let binding_path = workspace_binding_path(&data_dir, "agent_candidate");
        let candidates_path = worktree_candidates_path(&data_dir, "agent_candidate");
        let candidate_archive_dir = worktree_candidate_archive_dir(&data_dir, "agent_candidate");
        let patch_path = worktree_patch_path(&data_dir, "agent_candidate");
        let manifest = capture_worktree_artifact_candidates(
            "agent_candidate",
            &test_task_packet(&workspace_root, "provider", "research-worker"),
            &binding,
            &binding_path,
            &candidates_path,
            &candidate_archive_dir,
            &patch_path,
            "2",
        )
        .expect("candidate manifest should be captured");

        assert_eq!(manifest.adoption_status, "candidate_only");
        assert!(manifest
            .untracked_paths
            .iter()
            .any(|path| path == "research/candidate.md"));
        assert!(manifest
            .changed_paths
            .iter()
            .any(|path| path == "research/candidate.md"));
        assert_eq!(manifest.patch_ref, None);
        let candidate_entry = manifest
            .candidate_entries
            .iter()
            .find(|entry| entry.relative_path == "research/candidate.md")
            .expect("candidate entry should exist");
        assert_eq!(candidate_entry.safe_status, "safe");
        assert_eq!(candidate_entry.path_kind, "file");
        let archive_ref = candidate_entry
            .candidate_archive_ref
            .as_ref()
            .expect("file candidate should have an archive ref");
        assert_eq!(
            fs::read_to_string(archive_ref).expect("candidate archive should read"),
            "# Candidate\n\nWorker-only draft artifact.\n"
        );
        assert!(
            !workspace_root
                .join("research")
                .join("candidate.md")
                .exists(),
            "worker artifact must remain isolated from the canonical workspace"
        );
        assert!(candidates_path.exists());
    }

    #[test]
    fn worktree_candidate_manifest_excludes_unchanged_inherited_workspace_files() {
        let workspace_root = unique_test_workspace("candidate_inherited_workspace_file");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let inherited_relative = "research/auto/job_fixture/continuity/continuity.json";
        let inherited_path = workspace_root.join(inherited_relative);
        fs::create_dir_all(
            inherited_path
                .parent()
                .expect("inherited parent should exist"),
        )
        .expect("inherited parent should write");
        fs::write(&inherited_path, "{\"status\":\"running\"}\n")
            .expect("inherited runtime context should write");

        let binding =
            create_git_worktree(&data_dir, &workspace_root, "agent_inherited_candidate", "1")
                .expect("fixture worktree should bind");
        let packet = test_task_packet(&workspace_root, "provider", "research-worker");
        record_worker_workspace_baseline(&data_dir, &binding, &packet, "1")
            .expect("worker workspace baseline should record");
        assert!(Path::new(&binding.worktree_path)
            .join(inherited_relative)
            .exists());

        let manifest = capture_worktree_artifact_candidates(
            "agent_inherited_candidate",
            &packet,
            &binding,
            &workspace_binding_path(&data_dir, "agent_inherited_candidate"),
            &worktree_candidates_path(&data_dir, "agent_inherited_candidate"),
            &worktree_candidate_archive_dir(&data_dir, "agent_inherited_candidate"),
            &worktree_patch_path(&data_dir, "agent_inherited_candidate"),
            "2",
        )
        .expect("candidate manifest should be captured");

        assert!(!manifest
            .changed_paths
            .iter()
            .any(|path| path == inherited_relative));
        assert!(!manifest
            .candidate_entries
            .iter()
            .any(|entry| entry.relative_path == inherited_relative));
    }

    #[test]
    fn worktree_candidate_manifest_captures_modified_inherited_workspace_files() {
        let workspace_root = unique_test_workspace("candidate_modified_inherited_file");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let inherited_relative = "research/auto/job_fixture/continuity/continuity.json";
        let inherited_path = workspace_root.join(inherited_relative);
        fs::create_dir_all(
            inherited_path
                .parent()
                .expect("inherited parent should exist"),
        )
        .expect("inherited parent should write");
        fs::write(&inherited_path, "{\"status\":\"running\"}\n")
            .expect("inherited runtime context should write");

        let binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            "agent_modified_inherited_candidate",
            "1",
        )
        .expect("fixture worktree should bind");
        let packet = test_task_packet(&workspace_root, "provider", "research-worker");
        record_worker_workspace_baseline(&data_dir, &binding, &packet, "1")
            .expect("worker workspace baseline should record");
        fs::write(
            Path::new(&binding.worktree_path).join(inherited_relative),
            "{\"status\":\"repaired\"}\n",
        )
        .expect("worker should modify inherited runtime context");

        let manifest = capture_worktree_artifact_candidates(
            "agent_modified_inherited_candidate",
            &packet,
            &binding,
            &workspace_binding_path(&data_dir, "agent_modified_inherited_candidate"),
            &worktree_candidates_path(&data_dir, "agent_modified_inherited_candidate"),
            &worktree_candidate_archive_dir(&data_dir, "agent_modified_inherited_candidate"),
            &worktree_patch_path(&data_dir, "agent_modified_inherited_candidate"),
            "2",
        )
        .expect("candidate manifest should be captured");

        assert!(manifest
            .changed_paths
            .iter()
            .any(|path| path == inherited_relative));
        assert!(manifest
            .candidate_entries
            .iter()
            .any(|entry| entry.relative_path == inherited_relative));
    }

    #[test]
    fn worktree_candidate_manifest_tracks_session_changes_on_inherited_tracked_files() {
        let workspace_root = unique_test_workspace("candidate_inherited_tracked_file");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let tracked_relative = "src/session_owned.rs";
        let tracked_path = workspace_root.join(tracked_relative);
        fs::create_dir_all(tracked_path.parent().expect("tracked parent should exist"))
            .expect("tracked parent should write");
        fs::write(&tracked_path, "pub fn value() -> u32 { 1 }\n")
            .expect("tracked file should write");
        run_git_command(
            &workspace_root,
            &["add", tracked_relative],
            "add tracked fixture",
        )
        .expect("tracked fixture should add");
        run_git_command(
            &workspace_root,
            &[
                "-c",
                "user.name=Astra",
                "-c",
                "user.email=astra@example.invalid",
                "commit",
                "-m",
                "Add tracked fixture",
            ],
            "commit tracked fixture",
        )
        .expect("tracked fixture should commit");
        fs::write(&tracked_path, "pub fn value() -> u32 { 2 }\n")
            .expect("user dirty change should write");
        let data_dir = workspace_root.join(".pmcli");
        let binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            "agent_inherited_tracked_candidate",
            "1",
        )
        .expect("fixture worktree should bind");
        let packet = test_task_packet(&workspace_root, "provider", "implementation");
        record_worker_workspace_baseline(&data_dir, &binding, &packet, "1")
            .expect("worker workspace baseline should record");

        let unchanged = capture_worktree_artifact_candidates(
            "agent_inherited_tracked_candidate",
            &packet,
            &binding,
            &workspace_binding_path(&data_dir, "agent_inherited_tracked_candidate"),
            &worktree_candidates_path(&data_dir, "agent_inherited_tracked_candidate"),
            &worktree_candidate_archive_dir(&data_dir, "agent_inherited_tracked_candidate"),
            &worktree_patch_path(&data_dir, "agent_inherited_tracked_candidate"),
            "2",
        )
        .expect("unchanged inherited file should capture");
        assert!(!unchanged
            .changed_paths
            .iter()
            .any(|path| path == tracked_relative));

        fs::write(
            Path::new(&binding.worktree_path).join(tracked_relative),
            "pub fn value() -> u32 { 3 }\n",
        )
        .expect("worker tracked change should write");
        let modified = capture_worktree_artifact_candidates(
            "agent_inherited_tracked_candidate",
            &packet,
            &binding,
            &workspace_binding_path(&data_dir, "agent_inherited_tracked_candidate"),
            &worktree_candidates_path(&data_dir, "agent_inherited_tracked_candidate"),
            &worktree_candidate_archive_dir(&data_dir, "agent_inherited_tracked_candidate"),
            &worktree_patch_path(&data_dir, "agent_inherited_tracked_candidate"),
            "3",
        )
        .expect("modified inherited file should capture");
        assert!(modified
            .changed_paths
            .iter()
            .any(|path| path == tracked_relative));
        assert_eq!(
            modified.patch_ref, None,
            "a HEAD-relative patch must not claim the user's inherited dirty edit"
        );
    }

    #[test]
    fn worktree_candidate_manifest_tracks_session_changes_on_canonical_overlay_files() {
        let workspace_root = unique_test_workspace("candidate_canonical_overlay_file");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let data_dir = workspace_root.join(".pmcli");
        let overlay_relative = "research/stages/job_fixture/literature/literature_matrix.md";
        let target_path = workspace_root.join(overlay_relative);
        fs::create_dir_all(target_path.parent().expect("overlay parent should exist"))
            .expect("overlay parent should write");
        fs::write(&target_path, "# Canonical Literature Matrix\n")
            .expect("canonical target should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_candidate_overlay".to_string(),
            stage_id: "literature".to_string(),
            stage_execution_id: "stage_candidate_overlay".to_string(),
            source_agent_id: "agent_source".to_string(),
            source_task_id: Some("task_source".to_string()),
            source_ref: ".pmcli/agents/agent_source/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some(overlay_relative.to_string()),
            target_artifact_path: overlay_relative.to_string(),
            artifact_kind: "markdown".to_string(),
            task_type: "stage artifact synthesis".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::candidate_overlay".to_string(),
            source_sha256: None,
        };
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            seed,
            "main agent requested overlay adoption",
        )
        .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            canonical_artifacts::sha256_file(&target_path).expect("target sha should compute"),
        )
        .expect("materialized should record");
        canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline should promote");

        let binding = create_git_worktree(
            &data_dir,
            &workspace_root,
            "agent_canonical_overlay_candidate",
            "1",
        )
        .expect("fixture worktree should bind");
        let packet = test_task_packet(&workspace_root, "provider", "research_synthesizer");
        record_worker_workspace_baseline(&data_dir, &binding, &packet, "1")
            .expect("worker workspace baseline should record");
        let unchanged = capture_worktree_artifact_candidates(
            "agent_canonical_overlay_candidate",
            &packet,
            &binding,
            &workspace_binding_path(&data_dir, "agent_canonical_overlay_candidate"),
            &worktree_candidates_path(&data_dir, "agent_canonical_overlay_candidate"),
            &worktree_candidate_archive_dir(&data_dir, "agent_canonical_overlay_candidate"),
            &worktree_patch_path(&data_dir, "agent_canonical_overlay_candidate"),
            "2",
        )
        .expect("unchanged canonical overlay should capture");
        assert!(!unchanged
            .changed_paths
            .iter()
            .any(|path| path == overlay_relative));

        fs::write(
            Path::new(&binding.worktree_path).join(overlay_relative),
            "# Repaired Canonical Literature Matrix\n",
        )
        .expect("worker should modify canonical overlay file");
        let modified = capture_worktree_artifact_candidates(
            "agent_canonical_overlay_candidate",
            &packet,
            &binding,
            &workspace_binding_path(&data_dir, "agent_canonical_overlay_candidate"),
            &worktree_candidates_path(&data_dir, "agent_canonical_overlay_candidate"),
            &worktree_candidate_archive_dir(&data_dir, "agent_canonical_overlay_candidate"),
            &worktree_patch_path(&data_dir, "agent_canonical_overlay_candidate"),
            "3",
        )
        .expect("modified canonical overlay should capture");
        assert!(modified
            .changed_paths
            .iter()
            .any(|path| path == overlay_relative));
    }

    #[test]
    fn worktree_candidate_manifest_records_ignored_research_worker_artifacts() {
        let workspace_root = unique_test_workspace("ignored_research_candidate_manifest");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        fs::write(workspace_root.join(".gitignore"), "/research/\n")
            .expect("gitignore should write");
        run_git_command(&workspace_root, &["add", ".gitignore"], "add gitignore")
            .expect("gitignore should add");
        run_git_command(
            &workspace_root,
            &[
                "-c",
                "user.name=Astra",
                "-c",
                "user.email=astra@example.invalid",
                "commit",
                "-m",
                "Ignore runtime research artifacts",
            ],
            "commit gitignore",
        )
        .expect("gitignore should commit");

        let data_dir = workspace_root.join(".pmcli");
        let binding =
            create_git_worktree(&data_dir, &workspace_root, "agent_ignored_candidate", "1")
                .expect("fixture worktree should bind");
        let relative_candidate =
            "research/stages/job_fixture/literature/source_verification_ledger.md";
        let worker_artifact_path = Path::new(&binding.worktree_path).join(relative_candidate);
        fs::create_dir_all(
            worker_artifact_path
                .parent()
                .expect("candidate artifact should have parent"),
        )
        .expect("candidate artifact directory should write");
        fs::write(
            &worker_artifact_path,
            "# Source Verification Ledger\n\nWorker-only ignored candidate artifact.\n",
        )
        .expect("candidate artifact should write");

        let binding_path = workspace_binding_path(&data_dir, "agent_ignored_candidate");
        let candidates_path = worktree_candidates_path(&data_dir, "agent_ignored_candidate");
        let candidate_archive_dir =
            worktree_candidate_archive_dir(&data_dir, "agent_ignored_candidate");
        let patch_path = worktree_patch_path(&data_dir, "agent_ignored_candidate");
        let manifest = capture_worktree_artifact_candidates(
            "agent_ignored_candidate",
            &source_verification_task_packet(&workspace_root),
            &binding,
            &binding_path,
            &candidates_path,
            &candidate_archive_dir,
            &patch_path,
            "2",
        )
        .expect("ignored candidate manifest should be captured");

        assert!(manifest
            .status_entries
            .iter()
            .any(|entry| entry == &format!("!! {relative_candidate}")));
        assert!(manifest
            .changed_paths
            .iter()
            .any(|path| path == relative_candidate));
        assert!(!manifest
            .untracked_paths
            .iter()
            .any(|path| path == relative_candidate));
        let candidate_entry = manifest
            .candidate_entries
            .iter()
            .find(|entry| entry.relative_path == relative_candidate)
            .expect("ignored candidate entry should exist");
        assert_eq!(candidate_entry.safe_status, "safe");
        assert_eq!(candidate_entry.path_kind, "file");
        let archive_ref = candidate_entry
            .candidate_archive_ref
            .as_ref()
            .expect("ignored file candidate should have an archive ref");
        assert_eq!(
            fs::read_to_string(archive_ref).expect("candidate archive should read"),
            "# Source Verification Ledger\n\nWorker-only ignored candidate artifact.\n"
        );
    }

    #[test]
    fn worktree_candidate_manifest_records_directory_manifest_for_directory_candidates() {
        let workspace_root = unique_test_workspace("worktree_candidate_directory_manifest");
        let init = Command::new("git")
            .current_dir(&workspace_root)
            .args(["init", "-q"])
            .status()
            .expect("git init should execute");
        assert!(init.success());
        let data_dir = workspace_root.join(".pmcli");
        ensure_source_worktree_has_head(&workspace_root)
            .expect("fixture workspace should have a baseline commit");
        let binding = create_git_worktree(&data_dir, &workspace_root, "agent_dir_candidate", "1")
            .expect("fixture worktree should bind");
        let package_root = Path::new(&binding.worktree_path)
            .join("experiments")
            .join("bench_pkg");
        fs::create_dir_all(package_root.join("nested"))
            .expect("candidate package directories should write");
        fs::write(package_root.join("__init__.py"), "VALUE = 1\n")
            .expect("candidate package file should write");
        fs::write(
            package_root.join("nested").join("runner.py"),
            "def run():\n    return VALUE\n",
        )
        .expect("nested candidate package file should write");

        let binding_path = workspace_binding_path(&data_dir, "agent_dir_candidate");
        let candidates_path = worktree_candidates_path(&data_dir, "agent_dir_candidate");
        let candidate_archive_dir =
            worktree_candidate_archive_dir(&data_dir, "agent_dir_candidate");
        let patch_path = worktree_patch_path(&data_dir, "agent_dir_candidate");
        let manifest = capture_worktree_artifact_candidates(
            "agent_dir_candidate",
            &test_task_packet(&workspace_root, "provider", "research-worker"),
            &binding,
            &binding_path,
            &candidates_path,
            &candidate_archive_dir,
            &patch_path,
            "2",
        )
        .expect("candidate manifest should be captured");

        let directory_entry = manifest
            .candidate_entries
            .iter()
            .find(|entry| entry.relative_path == "experiments/bench_pkg")
            .expect("directory candidate entry should exist");
        assert!(directory_entry.is_directory);
        let directory_manifest_ref = directory_entry
            .directory_manifest_ref
            .as_ref()
            .expect("directory candidate should have manifest ref");
        let directory_manifest: AgentWorktreeDirectoryCandidateManifest = serde_json::from_str(
            &fs::read_to_string(directory_manifest_ref).expect("directory manifest should read"),
        )
        .expect("directory manifest should deserialize");
        assert_eq!(
            directory_manifest.root_relative_path,
            "experiments/bench_pkg"
        );
        assert_eq!(directory_manifest.file_count, 2);
        assert!(directory_manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "experiments/bench_pkg/__init__.py"));
        assert!(directory_manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "experiments/bench_pkg/nested/runner.py"));
    }

    #[test]
    fn main_agent_runtime_identity_records_provider_loop_authority() {
        let workspace_root = unique_test_workspace("main_agent_identity");
        let data_dir = workspace_root.join(".pmcli");

        let result = write_main_agent_runtime_identity(
            &data_dir,
            MainAgentRuntimeIdentityRequest {
                job_id: "job_fixture".to_string(),
                session_id: "session_fixture".to_string(),
                round_index: 3,
                provider_id: "provider_fixture".to_string(),
                model: "model_fixture".to_string(),
                artifact_ref: "research/auto/job_fixture/main-agent-round-3.md".to_string(),
                lifecycle_status: "succeeded".to_string(),
                tool_scope: vec![
                    "publish_board_tasks".to_string(),
                    "record_obligation_decision".to_string(),
                ],
                created_at: "1".to_string(),
                updated_at: "2".to_string(),
            },
        )
        .expect("main agent identity should write");

        let identity: AgentRuntimeIdentity = serde_json::from_str(
            &fs::read_to_string(&result.runtime_identity_ref)
                .expect("main agent identity should be readable"),
        )
        .expect("main agent identity should parse");
        assert_eq!(identity.runtime_kind, "main_agent");
        assert_eq!(identity.runner_kind, "provider_agent_loop");
        assert_eq!(identity.authority_scope, "research_semantic_authority");
        assert_eq!(identity.session_ref.as_deref(), Some("session_fixture"));
        assert_eq!(identity.provider_id.as_deref(), Some("provider_fixture"));
        assert_eq!(identity.task_packet_ref, None);
        assert!(identity
            .tool_scope
            .iter()
            .any(|tool| tool == "publish_board_tasks"));
    }

    #[test]
    fn local_directive_references_system_prompt_instead_of_embedding_role_soul() {
        let workspace_root = unique_test_workspace("local_directive_role_soul");
        let system_prompt = workspace_root
            .join(".pmcli")
            .join("agents")
            .join("agent_role_soul")
            .join("SYSTEM_PROMPT.md");
        let packet = TaskPacket {
            schema_version: "v1alpha1".to_string(),
            task_packet_id: "task_role_soul".to_string(),
            agent_id: "agent_role_soul".to_string(),
            runner_kind: Some("local".to_string()),
            intent: "execute local directive prompt test".to_string(),
            role_profile: "implementation".to_string(),
            retention_policy: "ephemeral".to_string(),
            run_class: "bounded".to_string(),
            io_mode: "request_response".to_string(),
            resume_policy: "fresh_thread".to_string(),
            replay_seed_ref: String::new(),
            budget: AgentBudget {
                max_turns: 1,
                max_runtime_ms: 1,
            },
            scope: AgentScope {
                workspace_root: workspace_root.display().to_string(),
                allowed_paths: vec![".".to_string()],
            },
            write_authority: "workspace_write".to_string(),
            success_criteria: vec!["evidence returned".to_string()],
            output_manifest_required: true,
            review_gate_required: true,
            stage_task_contract: None,
            collaboration_protocol: Some(default_agent_collaboration_protocol()),
            skill_refs: test_role_package_skill_refs("implementation"),
            tool_policy: Some(test_role_package_tool_policy("implementation")),
            command: Some("true".to_string()),
            message: "produce implementation evidence".to_string(),
            created_at: "1".to_string(),
        };

        let directive = render_directive(&packet, "1", &system_prompt);

        assert!(directive.contains("system_prompt_ref:"));
        assert!(directive.contains("The role Soul is a system-layer identity file"));
        assert!(
            directive.contains("Canonical project paths in the worker workspace are authoritative")
        );
        assert!(directive.contains("`.pmcli/input-bundles` is read-only provenance/context"));
        assert!(directive.contains("report a canonical artifact dependency blocker"));
        assert!(directive.contains("overlay baseline"));
        assert!(!directive.contains("Astra Implementation Worker Soul"));
    }
}

fn status_porcelain_path(line: &str) -> Option<String> {
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(
        path.rsplit_once(" -> ")
            .map(|(_, new_path)| new_path)
            .unwrap_or(path)
            .trim_matches('"')
            .to_string(),
    )
}

fn render_directive(packet: &TaskPacket, created_at: &str, system_prompt_path: &Path) -> String {
    let protocol = render_collaboration_protocol_for_prompt(packet);
    format!(
        "---\ndirective_id: \"{}\"\nagent: \"{}\"\nintent: \"{}\"\nrole_profile: \"{}\"\naction: \"execute\"\npriority: \"normal\"\ncreated_at: \"{}\"\ntimeout_seconds: {}\nmax_attempts: 1\nattempt: 1\ndecision: \"PROCEED\"\ndecision_reason: \"packet-bound local runner\"\nsystem_prompt_ref: \"{}\"\ndepends_on: []\n---\n\n## System Role Soul\n\nThe role Soul is a system-layer identity file, not task content. Load it from `system_prompt_ref` before executing this directive.\n\n## Task\n\n{}\n\n## Astra Collaboration Protocol\n\n```json\n{}\n```\n\n## Worker Authority Boundary\n\n- Execute only this TaskPacket and attached stage task contract.\n- Produce candidate evidence and task-local artifacts only.\n- Do not publish board tasks, close obligations, change routes, request cleanup, mark stages complete, or adopt artifacts into canonical project state.\n- Canonical project paths in the worker workspace are authoritative for imports, execution, tests, and repairs.\n- `.pmcli/input-bundles` is read-only provenance/context, not canonical project source.\n- If a required canonical project path is missing, report a canonical artifact dependency blocker instead of copying source from an input bundle.\n- If `workspace_binding.json` records an overlay baseline, trust the already-applied canonical project files and cite the binding ref in your evidence.\n\n## Acceptance Criteria\n\n{}\n",
        packet.task_packet_id,
        packet.agent_id,
        packet.intent,
        packet.role_profile,
        created_at,
        packet.budget.max_runtime_ms / 1000,
        system_prompt_path.display(),
        packet.message,
        protocol,
        packet.success_criteria.join("\n")
    )
}

fn render_status(
    packet: &TaskPacket,
    started_at: &str,
    completed_at: &str,
    lifecycle_status: &str,
    exit_reason: &str,
    failure_code: &str,
    artifacts: &[String],
) -> String {
    let status = if lifecycle_status == "succeeded" {
        "completed"
    } else {
        "failed"
    };
    let artifact_lines = artifacts
        .iter()
        .map(|artifact| format!("  - \"{artifact}\""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "---\ndirective_id: \"{}\"\nagent: \"{}\"\nstatus: \"{}\"\nstarted_at: \"{}\"\ncompleted_at: \"{}\"\nduration_seconds: 0\nexit_reason: \"{}\"\nerror_message: \"{}\"\nartifacts:\n{}\nquality_self_assessment: {}\n---\n\n## Summary\n\nLocal runner {}.\n\n## Issues\n\n{}\n\n## Recommendations\n\nPersist trace and require director review before promotion.\n",
        packet.task_packet_id,
        packet.agent_id,
        status,
        started_at,
        completed_at,
        exit_reason,
        failure_code,
        artifact_lines,
        if lifecycle_status == "succeeded" { 4 } else { 1 },
        lifecycle_status,
        if failure_code.is_empty() {
            "None."
        } else {
            failure_code
        }
    )
}

fn agents_root(data_dir: &Path) -> PathBuf {
    data_dir.join("agents")
}

fn agent_dir(data_dir: &Path, agent_id: &str) -> PathBuf {
    agents_root(data_dir).join(agent_id)
}

fn task_packet_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("task_packet.json")
}

fn runtime_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("runtime.json")
}

fn runtime_identity_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("runtime_identity.json")
}

fn directive_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("DIRECTIVE.md")
}

fn system_prompt_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("SYSTEM_PROMPT.md")
}

fn status_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("STATUS.md")
}

fn stdout_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("stdout.txt")
}

fn stderr_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("stderr.txt")
}

fn workspace_binding_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("workspace_binding.json")
}

fn input_mounts_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("input_artifact_mounts.json")
}

fn worker_workspace_baseline_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("worker_workspace_baseline.json")
}

fn worktree_candidates_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("worktree_artifact_candidates.json")
}

fn worktree_patch_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("worktree_diff.patch")
}

fn worktree_candidate_archive_dir(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("candidate_archive")
}

fn output_manifest_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("output_manifest.json")
}

fn tool_receipts_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id).join("tool_receipts.json")
}

fn tool_receipt_snapshot_dir(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id)
        .join("tool_receipts")
        .join("snapshots")
}

fn trace_path(data_dir: &Path, agent_id: &str) -> PathBuf {
    agent_dir(data_dir, agent_id)
        .join("traces")
        .join("trace.json")
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), AgentError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", process_unique_suffix()));
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }
    Ok(())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_micros()
        .to_string()
}
