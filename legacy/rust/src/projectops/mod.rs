use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static PROJECTOPS_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressDigestCandidate {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub candidate_id: String,
    pub tick_id: String,
    pub project_id: String,
    pub session_id: String,
    pub summary_ref: String,
    pub source_artifact_path: String,
    pub status: String,
    pub support_refs: Vec<String>,
    pub confidence: String,
    pub degraded_reasons: Vec<String>,
    pub promotes_to_durable_memory: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOpsTick {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub tick_id: String,
    pub trigger: String,
    pub project_id: String,
    pub session_id: Option<String>,
    pub digest_candidate_id: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub actions_taken: Vec<String>,
    pub degraded_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentSupervisorLease {
    pub lease_id: String,
    pub run_id: String,
    pub owner_agent_id: String,
    pub state: String,
    pub claimed_at: String,
    pub heartbeat_at: String,
    pub stale_after_sec: u64,
    pub pending_wake_count: usize,
    pub last_summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reclaimed_from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeEvent {
    pub wake_id: String,
    pub run_id: String,
    pub owner_agent_id: String,
    pub kind: String,
    pub urgency: String,
    pub requires_main_system: bool,
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
    pub lease_id: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub escalation_reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub no_owner_reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub acknowledged_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub acknowledged_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resolved_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resolution: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub escalated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectOpsStatusReport {
    pub project_id: String,
    pub leases: Vec<ExperimentSupervisorLease>,
    pub wake_events: Vec<WakeEvent>,
    pub cleanup_proposals: Vec<ProjectOpsCleanupProposalSummary>,
    pub active_lease_count: usize,
    pub stale_lease_count: usize,
    pub pending_wake_count: usize,
    pub escalated_wake_count: usize,
    pub resolved_wake_count: usize,
    pub pending_cleanup_count: usize,
    pub blocked_cleanup_count: usize,
    pub applied_cleanup_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectOpsCleanupProposalSummary {
    pub plan_id: String,
    pub status: String,
    pub rollback_snapshot_ref: String,
    pub git_authority: String,
    pub git_commit_required: bool,
    pub expected_tracked_deletion_count: usize,
    pub proposed_action_count: usize,
    pub applied_action_count: usize,
    pub move_to_archive_count: usize,
    pub remove_count: usize,
    pub canonical_repair_count: usize,
    pub blocked_gate_count: usize,
    pub recommended_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DigestStagingResult {
    pub digest_candidate: ProgressDigestCandidate,
    pub projectops_tick: ProjectOpsTick,
}

pub fn acquire_supervisor_lease(
    data_dir: &Path,
    run_id: &str,
    owner_agent_id: &str,
    stale_after_sec: u64,
) -> Result<ExperimentSupervisorLease, ProjectOpsError> {
    refresh_supervision_state(data_dir)?;
    let existing_leases = read_json_dir::<ExperimentSupervisorLease>(&lease_dir(data_dir))?;
    if let Some(live) = existing_leases.iter().find(|lease| {
        lease.run_id == run_id && matches!(lease.state.as_str(), "active" | "reclaimed")
    }) {
        return Err(ProjectOpsError::Invariant(format!(
            "run {run_id} is already supervised by live lease {}",
            live.lease_id
        )));
    }
    let reclaimed_from = existing_leases
        .iter()
        .rev()
        .find(|lease| lease.run_id == run_id && lease.state == "stale")
        .map(|lease| lease.lease_id.clone())
        .unwrap_or_default();
    let now = timestamp_string();
    let lease = ExperimentSupervisorLease {
        lease_id: format!("lease_{run_id}_{}", next_sequence()),
        run_id: run_id.to_string(),
        owner_agent_id: owner_agent_id.to_string(),
        state: if reclaimed_from.is_empty() {
            "active".to_string()
        } else {
            "reclaimed".to_string()
        },
        claimed_at: now.clone(),
        heartbeat_at: now,
        stale_after_sec,
        pending_wake_count: 0,
        last_summary: if reclaimed_from.is_empty() {
            "lease acquired".to_string()
        } else {
            format!("lease reclaimed from {reclaimed_from}")
        },
        reclaimed_from,
    };
    write_json_atomic(&lease_path(data_dir, &lease.lease_id), &lease)?;
    Ok(lease)
}

pub fn emit_wake_event(
    data_dir: &Path,
    run_id: &str,
    lease_id: &str,
    owner_agent_id: &str,
    kind: &str,
    urgency: &str,
    summary: &str,
    requires_main_system: bool,
    no_owner_reason: &str,
) -> Result<WakeEvent, ProjectOpsError> {
    refresh_supervision_state(data_dir)?;
    if !matches!(urgency, "low" | "normal" | "high" | "critical") {
        return Err(ProjectOpsError::Invariant(format!(
            "invalid wake urgency {urgency}"
        )));
    }
    let now = timestamp_string();
    let mut state = "pending".to_string();
    let mut escalation_reason = String::new();
    let mut requires_main_system = requires_main_system;
    let no_owner_reason = no_owner_reason.trim().to_string();

    if lease_id.is_empty() {
        if no_owner_reason.is_empty() {
            return Err(ProjectOpsError::Invariant(
                "wake without lease_id must include --no-owner-reason".to_string(),
            ));
        }
        state = "escalated".to_string();
        escalation_reason = no_owner_reason.clone();
        requires_main_system = true;
    } else {
        let lease = load_lease(data_dir, lease_id).map_err(|_| {
            ProjectOpsError::Invariant(format!(
                "wake references missing supervisor lease {lease_id}"
            ))
        })?;
        if lease.run_id != run_id {
            return Err(ProjectOpsError::Invariant(format!(
                "wake run_id {run_id} does not match lease run_id {}",
                lease.run_id
            )));
        }
        match lease.state.as_str() {
            "active" | "reclaimed" => {}
            "stale" => {
                state = "escalated".to_string();
                escalation_reason = format!("supervisor lease {lease_id} is stale");
                requires_main_system = true;
            }
            other => {
                return Err(ProjectOpsError::Invariant(format!(
                    "wake cannot attach to lease {lease_id} in state {other}"
                )));
            }
        }
    }

    let escalated_at = if (requires_main_system && lease_id.is_empty()) || state == "escalated" {
        now.clone()
    } else {
        String::new()
    };

    let wake = WakeEvent {
        wake_id: format!("wake_{run_id}_{}", next_sequence()),
        run_id: run_id.to_string(),
        owner_agent_id: owner_agent_id.to_string(),
        kind: kind.to_string(),
        urgency: urgency.to_string(),
        requires_main_system,
        summary: summary.to_string(),
        details: String::new(),
        lease_id: lease_id.to_string(),
        state,
        escalation_reason,
        no_owner_reason,
        acknowledged_by: String::new(),
        acknowledged_at: String::new(),
        resolved_at: String::new(),
        resolution: String::new(),
        escalated_at,
    };
    write_json_atomic(&wake_path(data_dir, &wake.wake_id), &wake)?;
    if !lease_id.is_empty() && wake.state == "pending" {
        let mut lease = load_lease(data_dir, lease_id)?;
        lease.pending_wake_count += 1;
        lease.last_summary = summary.to_string();
        write_json_atomic(&lease_path(data_dir, lease_id), &lease)?;
    }
    Ok(wake)
}

pub fn acknowledge_wake_event(
    data_dir: &Path,
    wake_id: &str,
    actor: &str,
) -> Result<WakeEvent, ProjectOpsError> {
    refresh_supervision_state(data_dir)?;
    let mut wake = load_wake(data_dir, wake_id)?;
    if wake.state != "pending" {
        return Err(ProjectOpsError::Invariant(format!(
            "wake {wake_id} cannot be acknowledged from state {}",
            wake.state
        )));
    }
    wake.state = "acknowledged".to_string();
    wake.acknowledged_by = actor.to_string();
    wake.acknowledged_at = timestamp_string();
    write_json_atomic(&wake_path(data_dir, wake_id), &wake)?;
    Ok(wake)
}

pub fn resolve_wake_event(
    data_dir: &Path,
    wake_id: &str,
    resolution: &str,
) -> Result<WakeEvent, ProjectOpsError> {
    refresh_supervision_state(data_dir)?;
    let mut wake = load_wake(data_dir, wake_id)?;
    if !matches!(
        wake.state.as_str(),
        "pending" | "acknowledged" | "escalated"
    ) {
        return Err(ProjectOpsError::Invariant(format!(
            "wake {wake_id} cannot be resolved from state {}",
            wake.state
        )));
    }
    wake.state = "resolved".to_string();
    wake.resolution = resolution.to_string();
    wake.resolved_at = timestamp_string();
    write_json_atomic(&wake_path(data_dir, wake_id), &wake)?;
    Ok(wake)
}

pub fn escalate_wake_event(
    data_dir: &Path,
    wake_id: &str,
    reason: &str,
    requires_main_system: bool,
) -> Result<WakeEvent, ProjectOpsError> {
    refresh_supervision_state(data_dir)?;
    let mut wake = load_wake(data_dir, wake_id)?;
    if !matches!(wake.state.as_str(), "pending" | "acknowledged") {
        return Err(ProjectOpsError::Invariant(format!(
            "wake {wake_id} cannot be escalated from state {}",
            wake.state
        )));
    }
    wake.state = "escalated".to_string();
    wake.escalation_reason = reason.to_string();
    wake.requires_main_system = wake.requires_main_system || requires_main_system;
    wake.escalated_at = timestamp_string();
    write_json_atomic(&wake_path(data_dir, wake_id), &wake)?;
    Ok(wake)
}

pub fn status(
    data_dir: &Path,
    project_id: &str,
) -> Result<ProjectOpsStatusReport, ProjectOpsError> {
    refresh_supervision_state(data_dir)?;
    let mut leases = read_json_dir::<ExperimentSupervisorLease>(&lease_dir(data_dir))?;
    let mut wake_events = read_json_dir::<WakeEvent>(&wake_dir(data_dir))?;
    let mut cleanup_proposals = cleanup_proposal_summaries(data_dir)?;
    leases.sort_by(|left, right| left.lease_id.cmp(&right.lease_id));
    wake_events.sort_by(|left, right| left.wake_id.cmp(&right.wake_id));
    cleanup_proposals.sort_by(|left, right| left.plan_id.cmp(&right.plan_id));
    Ok(ProjectOpsStatusReport {
        project_id: project_id.to_string(),
        active_lease_count: leases
            .iter()
            .filter(|lease| matches!(lease.state.as_str(), "active" | "reclaimed"))
            .count(),
        stale_lease_count: leases.iter().filter(|lease| lease.state == "stale").count(),
        pending_wake_count: wake_events
            .iter()
            .filter(|wake| wake.state == "pending")
            .count(),
        escalated_wake_count: wake_events
            .iter()
            .filter(|wake| wake.state == "escalated")
            .count(),
        resolved_wake_count: wake_events
            .iter()
            .filter(|wake| wake.state == "resolved")
            .count(),
        pending_cleanup_count: cleanup_proposals
            .iter()
            .filter(|proposal| proposal.status == "pending_review")
            .count(),
        blocked_cleanup_count: cleanup_proposals
            .iter()
            .filter(|proposal| proposal.status == "blocked")
            .count(),
        applied_cleanup_count: cleanup_proposals
            .iter()
            .filter(|proposal| proposal.status == "applied")
            .count(),
        leases,
        wake_events,
        cleanup_proposals,
    })
}

pub fn stage_session_compaction_digest(
    data_dir: &Path,
    project_id: &str,
    session_id: &str,
    summary_ref: &str,
    summary_path: &Path,
) -> Result<DigestStagingResult, ProjectOpsError> {
    let now = timestamp_string();
    let sequence = next_sequence();
    let tick_id = format!("tick_{now}_{sequence}");
    let candidate_id = format!("dig_{now}_{sequence}");
    let candidate_path = digest_candidate_path(data_dir, &candidate_id);
    let tick_path = tick_path(data_dir, &tick_id);
    let support_refs = vec![
        summary_path.display().to_string(),
        data_dir
            .join("sessions")
            .join(session_id)
            .join("transcript.jsonl")
            .display()
            .to_string(),
    ];

    let digest_candidate = ProgressDigestCandidate {
        schema_version: "projectops.progress_digest_candidate.v1".to_string(),
        conformance_line: "M7.digest_candidate".to_string(),
        canonical_path: candidate_path.display().to_string(),
        retention_policy: "reviewable_candidate_until_explicit_promotion_or_rejection".to_string(),
        atomic_write_policy: "atomic_json_write".to_string(),
        candidate_id: candidate_id.clone(),
        tick_id: tick_id.clone(),
        project_id: project_id.to_string(),
        session_id: session_id.to_string(),
        summary_ref: summary_ref.to_string(),
        source_artifact_path: summary_path.display().to_string(),
        status: "pending_review".to_string(),
        support_refs,
        confidence: "bounded_session_summary".to_string(),
        degraded_reasons: Vec::new(),
        promotes_to_durable_memory: false,
        created_at: now.clone(),
    };

    let projectops_tick = ProjectOpsTick {
        schema_version: "projectops.tick.v1".to_string(),
        conformance_line: "M7.projectops_tick".to_string(),
        canonical_path: tick_path.display().to_string(),
        retention_policy: "audit_log_retained".to_string(),
        atomic_write_policy: "atomic_json_write".to_string(),
        tick_id,
        trigger: "session_compacted".to_string(),
        project_id: project_id.to_string(),
        session_id: Some(session_id.to_string()),
        digest_candidate_id: Some(candidate_id),
        started_at: now.clone(),
        finished_at: now,
        actions_taken: vec![
            "wrote_progress_digest_candidate".to_string(),
            "left_durable_memory_unmodified".to_string(),
        ],
        degraded_reasons: Vec::new(),
    };

    write_json_atomic(&candidate_path, &digest_candidate)?;
    write_json_atomic(&tick_path, &projectops_tick)?;
    Ok(DigestStagingResult {
        digest_candidate,
        projectops_tick,
    })
}

fn digest_candidate_path(data_dir: &Path, candidate_id: &str) -> PathBuf {
    data_dir
        .join("memory")
        .join("promotion_queue")
        .join(format!("{candidate_id}.json"))
}

fn tick_path(data_dir: &Path, tick_id: &str) -> PathBuf {
    data_dir
        .join("projectops")
        .join("ticks")
        .join(format!("{tick_id}.json"))
}

fn lease_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("projectops").join("leases")
}

fn lease_path(data_dir: &Path, lease_id: &str) -> PathBuf {
    lease_dir(data_dir).join(format!("{lease_id}.json"))
}

fn wake_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("projectops").join("wake_events")
}

fn wake_path(data_dir: &Path, wake_id: &str) -> PathBuf {
    wake_dir(data_dir).join(format!("{wake_id}.json"))
}

fn cleanup_plan_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("repo_governance").join("cleanup_plans")
}

fn cleanup_proposal_summaries(
    data_dir: &Path,
) -> Result<Vec<ProjectOpsCleanupProposalSummary>, ProjectOpsError> {
    read_json_dir::<crate::artifacts::RepoCleanupProposal>(&cleanup_plan_dir(data_dir)).map(
        |proposals| {
            proposals
                .into_iter()
                .map(cleanup_proposal_summary)
                .collect::<Vec<_>>()
        },
    )
}

fn cleanup_proposal_summary(
    proposal: crate::artifacts::RepoCleanupProposal,
) -> ProjectOpsCleanupProposalSummary {
    let proposed_action_count = proposal.proposed_actions.len();
    let applied_action_count = proposal.applied_actions.len();
    let move_to_archive_count = proposal
        .proposed_actions
        .iter()
        .filter(|action| action.action_type == "move_to_archive")
        .count();
    let remove_count = proposal
        .proposed_actions
        .iter()
        .filter(|action| action.action_type == "remove")
        .count();
    let canonical_repair_count = proposal
        .proposed_actions
        .iter()
        .filter(|action| action.family == "canonical_lineage")
        .count();
    let blocked_gate_count = proposal
        .review_gates
        .iter()
        .filter(|gate| gate.status == "blocked")
        .count();
    let status = if !proposal.dry_run {
        "applied"
    } else if blocked_gate_count > 0 {
        "blocked"
    } else if proposed_action_count == 0 {
        "clean"
    } else {
        "pending_review"
    };
    let command_name = if status == "applied" {
        "cleanup-restore"
    } else {
        "cleanup-apply"
    };
    ProjectOpsCleanupProposalSummary {
        plan_id: proposal.plan_id.clone(),
        status: status.to_string(),
        rollback_snapshot_ref: proposal.rollback_snapshot_ref,
        git_authority: proposal.git_governance.authority,
        git_commit_required: proposal.git_governance.commit_required,
        expected_tracked_deletion_count: proposal.git_governance.expected_tracked_deletions.len(),
        proposed_action_count,
        applied_action_count,
        move_to_archive_count,
        remove_count,
        canonical_repair_count,
        blocked_gate_count,
        recommended_command: vec![
            "repo".to_string(),
            command_name.to_string(),
            proposal.plan_id,
            "--json".to_string(),
        ],
    }
}

fn load_lease(
    data_dir: &Path,
    lease_id: &str,
) -> Result<ExperimentSupervisorLease, ProjectOpsError> {
    Ok(serde_json::from_str(&fs::read_to_string(lease_path(
        data_dir, lease_id,
    ))?)?)
}

fn load_wake(data_dir: &Path, wake_id: &str) -> Result<WakeEvent, ProjectOpsError> {
    Ok(serde_json::from_str(&fs::read_to_string(wake_path(
        data_dir, wake_id,
    ))?)?)
}

fn refresh_supervision_state(data_dir: &Path) -> Result<(), ProjectOpsError> {
    let now_ms = timestamp_string().parse::<u128>().unwrap_or_default();
    let mut stale_lease_ids = Vec::new();
    for mut lease in read_json_dir::<ExperimentSupervisorLease>(&lease_dir(data_dir))? {
        if matches!(lease.state.as_str(), "active" | "reclaimed") && lease_is_stale(&lease, now_ms)
        {
            lease.state = "stale".to_string();
            lease.last_summary = "supervisor heartbeat exceeded stale_after_sec".to_string();
            stale_lease_ids.push(lease.lease_id.clone());
            write_json_atomic(&lease_path(data_dir, &lease.lease_id), &lease)?;
        } else if lease.state == "stale" {
            stale_lease_ids.push(lease.lease_id.clone());
        }
    }

    for mut wake in read_json_dir::<WakeEvent>(&wake_dir(data_dir))? {
        if wake.state == "pending"
            && stale_lease_ids
                .iter()
                .any(|lease_id| lease_id == &wake.lease_id)
        {
            wake.state = "escalated".to_string();
            wake.requires_main_system = true;
            wake.escalation_reason = format!("supervisor lease {} is stale", wake.lease_id);
            wake.escalated_at = timestamp_string();
            write_json_atomic(&wake_path(data_dir, &wake.wake_id), &wake)?;
        }
    }
    recalculate_pending_wake_counts(data_dir)?;
    Ok(())
}

fn lease_is_stale(lease: &ExperimentSupervisorLease, now_ms: u128) -> bool {
    let heartbeat_ms = lease.heartbeat_at.parse::<u128>().unwrap_or_default();
    let stale_after_ms = u128::from(lease.stale_after_sec).saturating_mul(1000);
    heartbeat_ms > 0 && stale_after_ms > 0 && now_ms.saturating_sub(heartbeat_ms) > stale_after_ms
}

fn recalculate_pending_wake_counts(data_dir: &Path) -> Result<(), ProjectOpsError> {
    let wakes = read_json_dir::<WakeEvent>(&wake_dir(data_dir))?;
    for mut lease in read_json_dir::<ExperimentSupervisorLease>(&lease_dir(data_dir))? {
        let pending_count = wakes
            .iter()
            .filter(|wake| wake.lease_id == lease.lease_id && wake.state == "pending")
            .count();
        if lease.pending_wake_count != pending_count {
            lease.pending_wake_count = pending_count;
            write_json_atomic(&lease_path(data_dir, &lease.lease_id), &lease)?;
        }
    }
    Ok(())
}

fn read_json_dir<T>(dir: &Path) -> Result<Vec<T>, ProjectOpsError>
where
    T: for<'de> Deserialize<'de>,
{
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        values.push(serde_json::from_str(&fs::read_to_string(path)?)?);
    }
    Ok(values)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), ProjectOpsError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    fs::write(&tmp_path, serde_json::to_string_pretty(value)?)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}

fn next_sequence() -> u64 {
    PROJECTOPS_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
pub enum ProjectOpsError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invariant(String),
}

impl fmt::Display for ProjectOpsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "projectops io failed: {err}"),
            Self::Json(err) => write!(formatter, "projectops json failed: {err}"),
            Self::Invariant(reason) => write!(formatter, "projectops invariant failed: {reason}"),
        }
    }
}

impl std::error::Error for ProjectOpsError {}

impl From<std::io::Error> for ProjectOpsError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for ProjectOpsError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}
