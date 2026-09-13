use super::checkpoint::{CheckpointError, KernelStateBundle};
use crate::events::{read_events_from, KernelEventEnvelope};
use std::fmt;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointPublishPlan {
    pub base_seq_cursor: u64,
    pub base_checkpoint_epoch: u64,
    pub touched_families: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCheckpointOutcome {
    pub bundle: KernelStateBundle,
    pub stale_base_detected: bool,
    pub applied_event_count: usize,
}

pub fn publish_checkpoint(
    checkpoint_path: &Path,
    events_path: &Path,
    plan: CheckpointPublishPlan,
) -> Result<PublishCheckpointOutcome, ReducerError> {
    let _write_lane = acquire_write_lane(checkpoint_path)?;
    let committed = if checkpoint_path.exists() {
        KernelStateBundle::load_from(checkpoint_path)?
    } else {
        KernelStateBundle::default()
    };

    let stale_base_detected = committed.seq_cursor != plan.base_seq_cursor
        || committed.checkpoint_epoch != plan.base_checkpoint_epoch;

    let events = read_events_from(events_path)?;
    validate_event_sequence(&events)?;
    let unapplied: Vec<_> = events
        .into_iter()
        .filter(|event| event.seq > committed.seq_cursor)
        .collect();

    let mut next_bundle = committed.clone();
    for event in &unapplied {
        apply_event(&mut next_bundle, event)?;
        next_bundle.seq_cursor = event.seq;
    }

    if !unapplied.is_empty() {
        next_bundle.checkpoint_epoch = committed.checkpoint_epoch + 1;
        next_bundle.save_to(checkpoint_path)?;
    }

    Ok(PublishCheckpointOutcome {
        bundle: next_bundle,
        stale_base_detected,
        applied_event_count: unapplied.len(),
    })
}

fn acquire_write_lane(checkpoint_path: &Path) -> Result<WriteLaneGuard, ReducerError> {
    let lane_path = checkpoint_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("project_state.lock");
    if let Some(parent) = lane_path.parent() {
        std::fs::create_dir_all(parent).map_err(ReducerError::WriteLaneIo)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lane_path)
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                ReducerError::WriteLaneBusy(lane_path.clone())
            } else {
                ReducerError::WriteLaneIo(err)
            }
        })?;
    Ok(WriteLaneGuard { lane_path })
}

fn validate_event_sequence(events: &[KernelEventEnvelope]) -> Result<(), ReducerError> {
    let mut previous_seq = None;
    for event in events {
        if let Some(previous) = previous_seq {
            if event.seq == previous {
                return Err(ReducerError::DuplicateEventSequence(event.seq));
            }
            if event.seq < previous {
                return Err(ReducerError::OutOfOrderEventSequence {
                    previous_seq: previous,
                    current_seq: event.seq,
                });
            }
        }
        previous_seq = Some(event.seq);
    }
    Ok(())
}

fn apply_event(
    bundle: &mut KernelStateBundle,
    event: &KernelEventEnvelope,
) -> Result<(), ReducerError> {
    if event.phase != "terminal" || event.terminal_outcome.as_deref() != Some("succeeded") {
        if event.event_name == "turn" && event.phase == "start" {
            bundle.turn_state.status =
                payload_string(event, "status").unwrap_or_else(|| "running".to_string());
            bundle.turn_state.turn_id =
                payload_optional_string(event, "turn_id").or_else(|| Some(event.object_id.clone()));
        }
        if event.event_name == "permission" && event.phase == "start" {
            bundle.permission_state.status = "pending".to_string();
            bundle.permission_state.pending_request_count += 1;
            bundle.turn_state.pending_permission_id = payload_optional_string(event, "request_id")
                .or_else(|| Some(event.object_id.clone()));
            bundle.turn_state.status = "awaiting_permission".to_string();
        }
        return Ok(());
    }

    match event.event_name.as_str() {
        "project" => {
            bundle.project_state.project_id = event
                .project_id
                .clone()
                .unwrap_or_else(|| event.object_id.clone());
            bundle.project_state.workspace_root =
                payload_string(event, "workspace_root").unwrap_or_default();
            bundle.project_state.workspace_hash =
                payload_string(event, "workspace_hash").unwrap_or_default();
            bundle.project_state.protocol_version =
                payload_string(event, "protocol_version").unwrap_or_else(|| "v1alpha1".to_string());
        }
        "session_open" => {
            if let Some(session_id) = &event.session_id {
                bundle.project_state.active_session_id = Some(session_id.clone());
                bundle.session_state.session_id = Some(session_id.clone());
            }
            bundle.session_state.status =
                payload_string(event, "status").unwrap_or_else(|| "active".to_string());
            bundle.session_state.transcript_path =
                payload_optional_string(event, "transcript_path");
        }
        "permission_request_pending" => {
            bundle.permission_state.status = "pending".to_string();
            bundle.permission_state.pending_request_count += 1;
            bundle.turn_state.pending_permission_id = payload_optional_string(event, "request_id");
        }
        "permission" => {
            bundle.permission_state.pending_request_count = bundle
                .permission_state
                .pending_request_count
                .saturating_sub(1);
            if bundle.permission_state.pending_request_count == 0 {
                bundle.permission_state.status = "idle".to_string();
            }
            bundle.turn_state.pending_permission_id = None;
        }
        "permission_request_resolved" => {
            bundle.permission_state.pending_request_count = bundle
                .permission_state
                .pending_request_count
                .saturating_sub(1);
            if bundle.permission_state.pending_request_count == 0 {
                bundle.permission_state.status = "idle".to_string();
            }
            bundle.turn_state.pending_permission_id = None;
        }
        "turn_started" => {
            bundle.turn_state.status = "running".to_string();
            bundle.turn_state.turn_id = payload_optional_string(event, "turn_id");
        }
        "turn" => {
            bundle.turn_state.status =
                payload_string(event, "status").unwrap_or_else(|| "succeeded".to_string());
            bundle.turn_state.turn_id =
                payload_optional_string(event, "turn_id").or_else(|| Some(event.object_id.clone()));
        }
        "turn_completed" => {
            bundle.turn_state.status = "idle".to_string();
        }
        "permission_mode" => {
            bundle.project_state.current_permission_mode =
                payload_optional_string(event, "permission_mode");
        }
        "mission_frame" => {
            bundle.project_state.mission_frame_ref =
                payload_optional_string(event, "mission_frame_ref");
        }
        "provider_resolution" => {
            bundle.turn_state.active_provider_route =
                payload_string(event, "provider_id").or_else(|| {
                    event
                        .payload
                        .get("provider_trace")?
                        .get("provider_id")?
                        .as_str()
                        .map(ToString::to_string)
                });
        }
        "session_resume" => {
            if let Some(session_id) = &event.session_id {
                bundle.project_state.active_session_id = Some(session_id.clone());
                bundle.session_state.session_id = Some(session_id.clone());
                bundle.session_state.status = "idle".to_string();
            }
        }
        "session_compaction" => {
            if let Some(session_id) = &event.session_id {
                bundle.project_state.active_session_id = Some(session_id.clone());
                bundle.session_state.session_id = Some(session_id.clone());
            }
            bundle.session_state.summary_ref = payload_optional_string(event, "summary_ref");
        }
        "session" | "repl" => {}
        "memory_query" => {
            bundle.memory_state.status =
                payload_string(event, "status").unwrap_or_else(|| "idle".to_string());
            if let Some(count) = payload_u64(event, "working_set_count") {
                bundle.memory_state.working_set_count = count;
            }
            if let Some(count) = payload_u64(event, "trusted_count") {
                bundle.memory_state.trusted_count = count;
            }
            if let Some(count) = payload_u64(event, "contested_count") {
                bundle.memory_state.contested_count = count;
            }
        }
        "memory_record" => {
            bundle.memory_state.status =
                payload_string(event, "status").unwrap_or_else(|| "recorded".to_string());
            if let Some(count) = payload_u64(event, "working_set_count") {
                bundle.memory_state.working_set_count = count;
            }
            if let Some(count) = payload_u64(event, "pending_queue_count") {
                bundle.memory_state.pending_queue_count = count;
            }
        }
        "memory_promotion" => {
            bundle.memory_state.status =
                payload_string(event, "status").unwrap_or_else(|| "promoted".to_string());
            if let Some(count) = payload_u64(event, "trusted_count") {
                bundle.memory_state.trusted_count = count;
            }
            if let Some(count) = payload_u64(event, "pending_promotion_count") {
                bundle.memory_state.pending_promotion_count = count;
            }
        }
        "memory_invalidation" => {
            bundle.memory_state.status =
                payload_string(event, "status").unwrap_or_else(|| "invalidated".to_string());
            if let Some(count) = payload_u64(event, "working_set_count") {
                bundle.memory_state.working_set_count = count;
            }
        }
        "projectops_tick" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| "ticked".to_string());
            bundle.projectops_state.active_tick_id =
                payload_optional_string(event, "tick_id").or_else(|| Some(event.object_id.clone()));
            if let Some(count) = payload_u64(event, "pending_research_runs") {
                bundle.projectops_state.pending_research_runs = count;
            }
            if let Some(count) = payload_u64(event, "wake_queue_count") {
                bundle.projectops_state.wake_queue_count = count;
            }
        }
        "digest_promotion" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| "digest_promoted".to_string());
            bundle.projectops_state.last_digest_ref = payload_optional_string(event, "digest_ref")
                .or_else(|| Some(event.object_id.clone()));
            if let Some(count) = payload_u64(event, "pending_digest_count") {
                bundle.projectops_state.pending_digest_count = count;
            }
        }
        "digest_rejection" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| "digest_rejected".to_string());
            if let Some(count) = payload_u64(event, "pending_digest_count") {
                bundle.projectops_state.pending_digest_count = count;
            }
        }
        "experiment_supervision" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| "supervising".to_string());
            if let Some(count) = payload_u64(event, "pending_research_runs") {
                bundle.projectops_state.pending_research_runs = count;
            }
        }
        "wake_event" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| "wake_queued".to_string());
            if let Some(count) = payload_u64(event, "wake_queue_count") {
                bundle.projectops_state.wake_queue_count = count;
            }
        }
        "setup" | "mcp_registry" => {}
        "cleanup_plan" => {
            bundle.repo_state.status = "planned".to_string();
            bundle.repo_state.cleanup_proposal_ref = payload_optional_string(event, "plan_id");
            bundle.projectops_state.cleanup_queue_count = event
                .payload
                .get("proposed_actions")
                .and_then(|value| value.as_array())
                .map(|items| items.len() as u64)
                .unwrap_or(0);
        }
        "cleanup_apply" => {
            bundle.repo_state.status = "idle".to_string();
            bundle.repo_state.cleanup_proposal_ref = payload_optional_string(event, "plan_id");
            bundle.projectops_state.cleanup_queue_count = 0;
        }
        "review" => {
            let review_id = payload_optional_string(event, "review_id")
                .or_else(|| Some(event.object_id.clone()))
                .unwrap_or_else(|| event.object_id.clone());
            let verdict = payload_string(event, "verdict").unwrap_or_else(|| "pending".to_string());
            if verdict == "pending" {
                if !bundle
                    .review_state
                    .open_review_ids
                    .iter()
                    .any(|id| id == &review_id)
                {
                    bundle.review_state.open_review_ids.push(review_id);
                    bundle.review_state.open_review_ids.sort();
                }
            } else {
                bundle
                    .review_state
                    .open_review_ids
                    .retain(|id| id != &review_id);
            }
            bundle.review_state.pending_count = bundle.review_state.open_review_ids.len() as u64;
            bundle.review_state.status = if bundle.review_state.pending_count == 0 {
                "idle".to_string()
            } else {
                "open".to_string()
            };
        }
        "review_open" | "review_retry" => {
            let review_id = payload_optional_string(event, "review_id")
                .or_else(|| Some(event.object_id.clone()))
                .unwrap_or_else(|| event.object_id.clone());
            push_sorted_unique(&mut bundle.review_state.open_review_ids, review_id);
            bundle.review_state.pending_count = bundle.review_state.open_review_ids.len() as u64;
            bundle.review_state.status = "open".to_string();
        }
        "agent_stop" => {
            let agent_id = payload_optional_string(event, "agent_id")
                .or_else(|| Some(event.object_id.clone()))
                .unwrap_or_else(|| event.object_id.clone());
            bundle
                .agent_state
                .active_agent_ids
                .retain(|id| id != &agent_id);
            bundle.agent_state.pending_count = bundle.agent_state.active_agent_ids.len() as u64;
            bundle.agent_state.status = if bundle.agent_state.active_agent_ids.is_empty() {
                "idle".to_string()
            } else {
                "running".to_string()
            };
        }
        "branch_cancel" => {
            let branch_id = payload_optional_string(event, "branch_id")
                .or_else(|| payload_optional_string(event, "batch_id"))
                .or_else(|| Some(event.object_id.clone()))
                .unwrap_or_else(|| event.object_id.clone());
            bundle
                .branch_state
                .active_batch_ids
                .retain(|id| id != &branch_id);
            bundle
                .branch_state
                .promotable_branch_ids
                .retain(|id| id != &branch_id);
            bundle.branch_state.status = if bundle.branch_state.active_batch_ids.is_empty()
                && bundle.branch_state.promotable_branch_ids.is_empty()
            {
                "idle".to_string()
            } else {
                "active".to_string()
            };
        }
        "branch_promote" => {
            let branch_id = payload_optional_string(event, "branch_id")
                .or_else(|| Some(event.object_id.clone()))
                .unwrap_or_else(|| event.object_id.clone());
            bundle
                .branch_state
                .promotable_branch_ids
                .retain(|id| id != &branch_id);
            bundle.branch_state.status =
                payload_string(event, "status").unwrap_or_else(|| "promoted".to_string());
        }
        "research_stage" => {
            bundle.projectops_state.status = match event.terminal_outcome.as_deref() {
                Some("blocked") => "awaiting_research_gate".to_string(),
                Some("failed") => "research_failed".to_string(),
                Some("cancelled") => "research_cancelled".to_string(),
                _ => payload_string(event, "status")
                    .unwrap_or_else(|| "research_updated".to_string()),
            };
            if let Some(count) = payload_u64(event, "pending_research_runs") {
                bundle.projectops_state.pending_research_runs = count;
            }
        }
        "research_stage_decision" | "research_repair" | "research_pivot" | "research_refine" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| event.event_name.clone());
            if let Some(count) = payload_u64(event, "pending_research_runs") {
                bundle.projectops_state.pending_research_runs = count;
            }
        }
        "routine_trigger" | "routine_run_due" | "routine_retry" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| event.event_name.clone());
            for agent_id in routine_agent_ids(event) {
                push_sorted_unique(&mut bundle.agent_state.active_agent_ids, agent_id);
            }
            bundle.agent_state.pending_count = bundle.agent_state.active_agent_ids.len() as u64;
            bundle.agent_state.status = if bundle.agent_state.active_agent_ids.is_empty() {
                "idle".to_string()
            } else {
                "running".to_string()
            };
            bundle.projectops_state.pending_research_runs = bundle
                .projectops_state
                .pending_research_runs
                .saturating_add(1);
        }
        "routine_ingress" => {
            bundle.projectops_state.status =
                payload_string(event, "status").unwrap_or_else(|| "routine_ingress".to_string());
        }
        "command"
        | "runtime_preflight"
        | "inspection"
        | "config"
        | "provider_auth"
        | "provider_test"
        | "provider_catalog"
        | "tool"
        | "tool_call"
        | "tool_result"
        | "goal_alignment_trace"
        | "goal_advance"
        | "goal_task_dispatch"
        | "goal_task_acceptance"
        | "goal_research_stage_decision"
        | "autonomous_research_job"
        | "routine_definition"
        | "remote_attach"
        | "remote_handoff"
        | "remote_takeover"
        | "remote_notify"
        | "remote_message_delta"
        | "remote_message"
        | "remote_attachment"
        | "remote_permission_response"
        | "remote_lease_revoked"
        | "remote_binding_revoked"
        | "remote_terminal_attach"
        | "remote_terminal_replay"
        | "remote_terminal_input"
        | "remote_terminal_output"
        | "remote_terminal_resize"
        | "remote_terminal_signal"
        | "remote_reconnect"
        | "ownership_change"
        | "permission_consumed" => {}
        other => {
            return Err(ReducerError::UnsupportedEventType(other.to_string()));
        }
    }
    Ok(())
}

fn payload_string(event: &KernelEventEnvelope, key: &str) -> Option<String> {
    event.payload.get(key)?.as_str().map(ToString::to_string)
}

fn payload_optional_string(event: &KernelEventEnvelope, key: &str) -> Option<String> {
    payload_string(event, key)
}

fn payload_u64(event: &KernelEventEnvelope, key: &str) -> Option<u64> {
    event.payload.get(key)?.as_u64()
}

fn routine_agent_ids(event: &KernelEventEnvelope) -> Vec<String> {
    let mut agent_ids = Vec::new();
    if let Some(agent_id) = event
        .payload
        .get("trigger")
        .and_then(|value| value.get("agent_id"))
        .and_then(|value| value.as_str())
    {
        agent_ids.push(agent_id.to_string());
    }
    if let Some(agent_id) = event
        .payload
        .get("retry")
        .and_then(|value| value.get("trigger"))
        .and_then(|value| value.get("agent_id"))
        .and_then(|value| value.as_str())
    {
        agent_ids.push(agent_id.to_string());
    }
    if let Some(outcomes) = event
        .payload
        .get("outcomes")
        .and_then(|value| value.as_array())
    {
        for outcome in outcomes {
            if let Some(agent_id) = outcome
                .get("trigger_result")
                .and_then(|value| value.get("trigger"))
                .and_then(|value| value.get("agent_id"))
                .and_then(|value| value.as_str())
            {
                agent_ids.push(agent_id.to_string());
            }
        }
    }
    agent_ids
}

fn push_sorted_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
        values.sort();
    }
}

#[derive(Debug)]
pub enum ReducerError {
    Checkpoint(CheckpointError),
    Event(crate::events::EventReadError),
    UnsupportedEventType(String),
    DuplicateEventSequence(u64),
    OutOfOrderEventSequence { previous_seq: u64, current_seq: u64 },
    WriteLaneBusy(PathBuf),
    WriteLaneIo(std::io::Error),
}

impl fmt::Display for ReducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Checkpoint(err) => write!(f, "{err}"),
            Self::Event(err) => write!(f, "{err}"),
            Self::UnsupportedEventType(event_type) => {
                write!(f, "unsupported event type for reducer replay: {event_type}")
            }
            Self::DuplicateEventSequence(seq) => {
                write!(f, "duplicate event sequence detected in event log: {seq}")
            }
            Self::OutOfOrderEventSequence {
                previous_seq,
                current_seq,
            } => write!(
                f,
                "out-of-order event sequence detected in event log: {} then {}",
                previous_seq, current_seq
            ),
            Self::WriteLaneBusy(path) => {
                write!(f, "checkpoint write lane is busy: {}", path.display())
            }
            Self::WriteLaneIo(err) => write!(f, "checkpoint write lane io failed: {err}"),
        }
    }
}

impl std::error::Error for ReducerError {}

impl From<CheckpointError> for ReducerError {
    fn from(value: CheckpointError) -> Self {
        Self::Checkpoint(value)
    }
}

impl From<crate::events::EventReadError> for ReducerError {
    fn from(value: crate::events::EventReadError) -> Self {
        Self::Event(value)
    }
}

struct WriteLaneGuard {
    lane_path: PathBuf,
}

impl Drop for WriteLaneGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lane_path);
    }
}
