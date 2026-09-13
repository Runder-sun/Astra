use crate::agents::{self, LocalAgentStartRequest};
use crate::projectops::{self, ExperimentSupervisorLease, WakeEvent};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ROUTINE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
pub(crate) const DEFAULT_MAX_RETRY_ATTEMPTS: u64 = 3;
pub(crate) const DEFAULT_RETRY_BACKOFF_MS: u128 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineDefinition {
    pub schema_version: String,
    pub routine_id: String,
    pub name: String,
    pub trigger_kind: String,
    pub intent: String,
    pub role_profile: String,
    pub message: String,
    pub command: String,
    pub delivery_target: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineTriggerRecord {
    pub schema_version: String,
    pub trigger_id: String,
    pub routine_id: String,
    pub trigger_kind: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of_trigger_id: Option<String>,
    #[serde(default)]
    pub attempt: u64,
    pub run_id: String,
    pub agent_id: String,
    pub lease_id: String,
    pub task_packet_path: String,
    pub runtime_record_path: String,
    pub output_manifest_path: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<String>,
    #[serde(default)]
    pub retry_budget_remaining: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recovery_decision: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineIngressRecord {
    pub schema_version: String,
    pub ingress_id: String,
    pub routine_id: String,
    pub trigger_kind: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dedupe_key: String,
    pub status: String,
    pub decision: String,
    pub received_at: String,
    pub ready_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub duplicate_of_ingress_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub triggered_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trigger_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lease_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trigger_record_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineCreateRequest {
    pub name: String,
    pub trigger_kind: String,
    pub intent: String,
    pub role_profile: String,
    pub message: String,
    pub command: String,
    pub delivery_target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineTriggerRequest {
    pub routine_id: String,
    pub trigger_kind: String,
    pub source: String,
    pub stale_after_sec: u64,
    pub max_retry_attempts: u64,
    pub retry_backoff_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineIngressRequest {
    pub routine_id: String,
    pub trigger_kind: String,
    pub source: String,
    pub dedupe_key: String,
    pub ready_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineRunDueRequest {
    pub trigger_kind: Option<String>,
    pub limit: Option<usize>,
    pub stale_after_sec: u64,
    pub max_retry_attempts: u64,
    pub retry_backoff_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineRetryRequest {
    pub trigger_id: String,
    pub source: String,
    pub stale_after_sec: u64,
    pub max_retry_attempts: u64,
    pub retry_backoff_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineCreateResult {
    pub status: String,
    pub definition_path: String,
    pub routine: RoutineDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineTriggerResult {
    pub status: String,
    pub routine: RoutineDefinition,
    pub trigger: RoutineTriggerRecord,
    pub trigger_record_path: String,
    pub agent: agents::AgentStartResult,
    pub lease: ExperimentSupervisorLease,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_wake: Option<WakeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineIngressResult {
    pub status: String,
    pub ingress: RoutineIngressRecord,
    pub ingress_record_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<RoutineIngressRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunDueOutcome {
    pub status: String,
    pub ingress: RoutineIngressRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_result: Option<RoutineTriggerResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRecoveryGovernanceSummary {
    pub schema_version: String,
    pub status: String,
    pub failed_trigger_count: usize,
    pub retryable_trigger_count: usize,
    pub budget_exhausted_count: usize,
    pub backoff_active_count: usize,
    pub pending_ingress_failure_count: usize,
    pub next_recommended_action: String,
    pub entries: Vec<RoutineRecoveryGovernanceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRecoveryGovernanceEntry {
    pub object_kind: String,
    pub object_id: String,
    pub routine_id: String,
    pub status: String,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<String>,
    pub retry_budget_remaining: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunDueResult {
    pub status: String,
    pub selected_count: usize,
    pub triggered_count: usize,
    pub failed_count: usize,
    pub outcomes: Vec<RoutineRunDueOutcome>,
    pub recovery_governance: RoutineRecoveryGovernanceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRetryResult {
    pub status: String,
    pub previous_trigger: RoutineTriggerRecord,
    pub retry: RoutineTriggerResult,
    pub recovery_governance: RoutineRecoveryGovernanceSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineListEntry {
    pub routine_id: String,
    pub name: String,
    pub trigger_kind: String,
    pub intent: String,
    pub role_profile: String,
    pub delivery_target: String,
    pub created_at: String,
    pub updated_at: String,
    pub trigger_count: usize,
    pub last_trigger: Option<RoutineTriggerRecord>,
    pub ingress_count: usize,
    pub last_ingress: Option<RoutineIngressRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineListResult {
    pub routines: Vec<RoutineListEntry>,
    pub total_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineInspectResult {
    pub routine: RoutineDefinition,
    pub definition_path: String,
    pub triggers: Vec<RoutineTriggerRecord>,
    pub trigger_count: usize,
    pub ingress: Vec<RoutineIngressRecord>,
    pub ingress_count: usize,
}

pub fn create(
    data_dir: &Path,
    request: RoutineCreateRequest,
) -> Result<RoutineCreateResult, RoutineError> {
    validate_trigger_kind(&request.trigger_kind)?;
    if request.name.trim().is_empty() {
        return Err(RoutineError::InvalidInput(
            "routine name must not be empty".to_string(),
        ));
    }
    if request.intent.trim().is_empty() {
        return Err(RoutineError::InvalidInput(
            "routine intent must not be empty".to_string(),
        ));
    }
    if request.role_profile.trim().is_empty() {
        return Err(RoutineError::InvalidInput(
            "routine role_profile must not be empty".to_string(),
        ));
    }
    if request.message.trim().is_empty() {
        return Err(RoutineError::InvalidInput(
            "routine message must not be empty".to_string(),
        ));
    }
    if request.command.trim().is_empty() {
        return Err(RoutineError::InvalidInput(
            "routine command must not be empty".to_string(),
        ));
    }

    let now = timestamp_string();
    let routine_id = format!(
        "routine_{}_{}",
        slugify(&request.name),
        next_unique_suffix()
    );
    let routine = RoutineDefinition {
        schema_version: "routine.definition.v1".to_string(),
        routine_id: routine_id.clone(),
        name: request.name,
        trigger_kind: request.trigger_kind,
        intent: request.intent,
        role_profile: request.role_profile,
        message: request.message,
        command: request.command,
        delivery_target: request.delivery_target,
        created_at: now.clone(),
        updated_at: now,
    };
    let path = definition_path(data_dir, &routine_id);
    write_json_atomic(&path, &routine)?;
    Ok(RoutineCreateResult {
        status: "created".to_string(),
        definition_path: path.display().to_string(),
        routine,
    })
}

pub fn trigger(
    data_dir: &Path,
    workspace_root: &Path,
    request: RoutineTriggerRequest,
) -> Result<RoutineTriggerResult, RoutineError> {
    trigger_with_context(data_dir, workspace_root, request, None, None)
}

fn trigger_with_context(
    data_dir: &Path,
    workspace_root: &Path,
    request: RoutineTriggerRequest,
    ingress_id: Option<String>,
    retry_of_trigger_id: Option<String>,
) -> Result<RoutineTriggerResult, RoutineError> {
    validate_trigger_kind(&request.trigger_kind)?;
    let routine = load_definition(data_dir, &request.routine_id)?;
    if routine.trigger_kind != request.trigger_kind {
        return Err(RoutineError::InvalidInput(format!(
            "routine {} is configured for {} triggers, got {}",
            routine.routine_id, routine.trigger_kind, request.trigger_kind
        )));
    }

    let started_at = timestamp_string();
    let trigger_id = format!(
        "routine_trigger_{}_{}",
        routine.routine_id,
        next_unique_suffix()
    );
    let attempt = retry_of_trigger_id
        .as_deref()
        .map(|previous_trigger_id| next_attempt(data_dir, previous_trigger_id))
        .transpose()?
        .unwrap_or(1);
    let agent = agents::start_local(
        data_dir,
        workspace_root,
        LocalAgentStartRequest {
            intent: routine.intent.clone(),
            role_profile: routine.role_profile.clone(),
            message: routine.message.clone(),
            command: routine.command.clone(),
            stage_task_contract: None,
        },
    )?;
    let run_id = format!(
        "routine_run::{}::trigger::{}",
        routine.routine_id, trigger_id
    );
    let lease = projectops::acquire_supervisor_lease(
        data_dir,
        &run_id,
        &agent.agent_id,
        request.stale_after_sec,
    )?;
    let trigger_status = if agent.status == "succeeded" {
        "agent_run_succeeded".to_string()
    } else {
        format!("agent_run_{}", agent.status)
    };
    let trigger = RoutineTriggerRecord {
        schema_version: "routine.trigger_record.v1".to_string(),
        trigger_id: trigger_id.clone(),
        routine_id: routine.routine_id.clone(),
        trigger_kind: request.trigger_kind,
        source: request.source,
        ingress_id,
        retry_of_trigger_id,
        attempt,
        run_id,
        agent_id: agent.agent_id.clone(),
        lease_id: lease.lease_id.clone(),
        task_packet_path: agent.task_packet_path.clone(),
        runtime_record_path: agent.runtime_record_path.clone(),
        output_manifest_path: agent.output_manifest_path.clone(),
        status: trigger_status,
        next_retry_at: if agent.status == "succeeded" {
            None
        } else {
            Some(timestamp_string_from_offset(request.retry_backoff_ms))
        },
        retry_budget_remaining: if agent.status == "succeeded" {
            0
        } else {
            request.max_retry_attempts.saturating_sub(attempt)
        },
        recovery_decision: if agent.status == "succeeded" {
            "completed".to_string()
        } else if request.max_retry_attempts.saturating_sub(attempt) == 0 {
            "retry_budget_exhausted".to_string()
        } else {
            "retry_available_after_backoff".to_string()
        },
        created_at: started_at,
    };
    let trigger_record_path = trigger_path(data_dir, &trigger_id);
    write_json_atomic(&trigger_record_path, &trigger)?;
    let recovery_wake = if agent.status == "succeeded" {
        None
    } else {
        Some(projectops::emit_wake_event(
            data_dir,
            &trigger.run_id,
            &trigger.lease_id,
            &trigger.agent_id,
            "routine_agent_failed",
            "high",
            &format!(
                "routine trigger {} ended with {}; retry is available",
                trigger.trigger_id, trigger.status
            ),
            true,
            "",
        )?)
    };

    Ok(RoutineTriggerResult {
        status: "triggered".to_string(),
        routine,
        trigger,
        trigger_record_path: trigger_record_path.display().to_string(),
        agent,
        lease,
        recovery_wake,
    })
}

pub fn ingress(
    data_dir: &Path,
    request: RoutineIngressRequest,
) -> Result<RoutineIngressResult, RoutineError> {
    let _guard = lock_routines_state(data_dir)?;
    validate_trigger_kind(&request.trigger_kind)?;
    let routine = load_definition(data_dir, &request.routine_id)?;
    if routine.trigger_kind != request.trigger_kind {
        return Err(RoutineError::InvalidInput(format!(
            "routine {} is configured for {} triggers, got {}",
            routine.routine_id, routine.trigger_kind, request.trigger_kind
        )));
    }
    if request.source.trim().is_empty() {
        return Err(RoutineError::InvalidInput(
            "routine ingress source must not be empty".to_string(),
        ));
    }
    if let Some(ready_at) = request.ready_at.as_deref() {
        validate_millis_timestamp("ready_at", ready_at)?;
    }

    let now = timestamp_string();
    let dedupe_key = request.dedupe_key.trim().to_string();
    let duplicate_of = if dedupe_key.is_empty() {
        None
    } else {
        find_dedupe_match(
            data_dir,
            &routine.routine_id,
            &request.trigger_kind,
            &dedupe_key,
        )?
    };
    let ingress_id = format!(
        "routine_ingress_{}_{}",
        routine.routine_id,
        next_unique_suffix()
    );
    let mut ingress = RoutineIngressRecord {
        schema_version: "routine.ingress_record.v1".to_string(),
        ingress_id: ingress_id.clone(),
        routine_id: routine.routine_id,
        trigger_kind: request.trigger_kind,
        source: request.source,
        dedupe_key,
        status: "ready".to_string(),
        decision: "accepted_ready".to_string(),
        received_at: now.clone(),
        ready_at: request.ready_at.unwrap_or(now),
        duplicate_of_ingress_id: String::new(),
        triggered_at: String::new(),
        trigger_id: String::new(),
        run_id: String::new(),
        agent_id: String::new(),
        lease_id: String::new(),
        trigger_record_path: String::new(),
    };
    if let Some(existing) = duplicate_of.clone() {
        ingress.status = "duplicate_ignored".to_string();
        ingress.decision = "dedupe_key_already_recorded".to_string();
        ingress.duplicate_of_ingress_id = existing.ingress_id.clone();
    }
    let ingress_record_path = ingress_path(data_dir, &ingress_id);
    write_json_atomic(&ingress_record_path, &ingress)?;

    Ok(RoutineIngressResult {
        status: ingress.status.clone(),
        ingress,
        ingress_record_path: ingress_record_path.display().to_string(),
        duplicate_of,
    })
}

pub fn run_due(
    data_dir: &Path,
    workspace_root: &Path,
    request: RoutineRunDueRequest,
) -> Result<RoutineRunDueResult, RoutineError> {
    let _guard = lock_routines_state(data_dir)?;
    if let Some(trigger_kind) = request.trigger_kind.as_deref() {
        validate_trigger_kind(trigger_kind)?;
    }
    let now = timestamp_string();
    let mut ready = read_json_dir::<RoutineIngressRecord>(&ingress_dir(data_dir))?
        .into_iter()
        .filter(|ingress| ingress.status == "ready")
        .filter(|ingress| {
            request
                .trigger_kind
                .as_ref()
                .map(|trigger_kind| &ingress.trigger_kind == trigger_kind)
                .unwrap_or(true)
        })
        .filter(|ingress| {
            parse_millis_timestamp(&ingress.ready_at)
                .map(|ready_at| ready_at <= parse_millis_timestamp(&now).unwrap_or_default())
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    ready.sort_by(|left, right| {
        left.ready_at
            .cmp(&right.ready_at)
            .then(left.received_at.cmp(&right.received_at))
            .then(left.ingress_id.cmp(&right.ingress_id))
    });
    if let Some(limit) = request.limit {
        ready.truncate(limit);
    }

    let mut outcomes = Vec::new();
    let mut triggered_count = 0usize;
    let mut failed_count = 0usize;
    for mut ingress in ready {
        let trigger_result = trigger_with_context(
            data_dir,
            workspace_root,
            RoutineTriggerRequest {
                routine_id: ingress.routine_id.clone(),
                trigger_kind: ingress.trigger_kind.clone(),
                source: format!("ingress:{}", ingress.ingress_id),
                stale_after_sec: request.stale_after_sec,
                max_retry_attempts: request.max_retry_attempts,
                retry_backoff_ms: request.retry_backoff_ms,
            },
            Some(ingress.ingress_id.clone()),
            None,
        );
        match trigger_result {
            Ok(result) => {
                let triggered_at = timestamp_string();
                ingress.status = "triggered".to_string();
                ingress.decision = "dispatched_to_routine_trigger".to_string();
                ingress.triggered_at = triggered_at;
                ingress.trigger_id = result.trigger.trigger_id.clone();
                ingress.run_id = result.trigger.run_id.clone();
                ingress.agent_id = result.trigger.agent_id.clone();
                ingress.lease_id = result.trigger.lease_id.clone();
                ingress.trigger_record_path = result.trigger_record_path.clone();
                write_json_atomic(&ingress_path(data_dir, &ingress.ingress_id), &ingress)?;
                triggered_count += 1;
                outcomes.push(RoutineRunDueOutcome {
                    status: "triggered".to_string(),
                    ingress,
                    trigger_result: Some(result),
                    error: None,
                });
            }
            Err(err) => {
                ingress.status = "trigger_failed".to_string();
                ingress.decision = format!("trigger_failed:{err}");
                write_json_atomic(&ingress_path(data_dir, &ingress.ingress_id), &ingress)?;
                failed_count += 1;
                outcomes.push(RoutineRunDueOutcome {
                    status: "trigger_failed".to_string(),
                    ingress,
                    trigger_result: None,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    Ok(RoutineRunDueResult {
        status: if failed_count == 0 {
            "completed".to_string()
        } else {
            "completed_with_failures".to_string()
        },
        selected_count: outcomes.len(),
        triggered_count,
        failed_count,
        outcomes,
        recovery_governance: build_recovery_governance_summary(data_dir)?,
    })
}

pub fn retry_trigger(
    data_dir: &Path,
    workspace_root: &Path,
    request: RoutineRetryRequest,
) -> Result<RoutineRetryResult, RoutineError> {
    let _guard = lock_routines_state(data_dir)?;
    let previous_trigger = load_trigger(data_dir, &request.trigger_id)?;
    if previous_trigger.status == "agent_run_succeeded" {
        return Err(RoutineError::InvalidInput(format!(
            "trigger {} already succeeded and does not need retry",
            previous_trigger.trigger_id
        )));
    }
    if !matches!(
        previous_trigger.status.as_str(),
        "agent_run_failed" | "agent_run_timeout"
    ) {
        return Err(RoutineError::InvalidInput(format!(
            "trigger {} is in status {} and is not retryable",
            previous_trigger.trigger_id, previous_trigger.status
        )));
    }
    if previous_trigger.retry_budget_remaining == 0 {
        return Err(RoutineError::InvalidInput(format!(
            "trigger {} retry budget is exhausted",
            previous_trigger.trigger_id
        )));
    }
    if let Some(next_retry_at) = previous_trigger.next_retry_at.as_deref() {
        let now = parse_millis_timestamp(&timestamp_string()).unwrap_or_default();
        let retry_at = parse_millis_timestamp(next_retry_at).unwrap_or_default();
        if retry_at > now {
            return Err(RoutineError::InvalidInput(format!(
                "trigger {} is in retry backoff until {}",
                previous_trigger.trigger_id, next_retry_at
            )));
        }
    }
    let retry = trigger_with_context(
        data_dir,
        workspace_root,
        RoutineTriggerRequest {
            routine_id: previous_trigger.routine_id.clone(),
            trigger_kind: previous_trigger.trigger_kind.clone(),
            source: request.source,
            stale_after_sec: request.stale_after_sec,
            max_retry_attempts: request.max_retry_attempts,
            retry_backoff_ms: request.retry_backoff_ms,
        },
        None,
        Some(previous_trigger.trigger_id.clone()),
    )?;
    Ok(RoutineRetryResult {
        status: "retried".to_string(),
        previous_trigger,
        retry,
        recovery_governance: build_recovery_governance_summary(data_dir)?,
    })
}

pub fn recovery_governance(
    data_dir: &Path,
) -> Result<RoutineRecoveryGovernanceSummary, RoutineError> {
    build_recovery_governance_summary(data_dir)
}

pub fn list(data_dir: &Path) -> Result<RoutineListResult, RoutineError> {
    let triggers = read_json_dir::<RoutineTriggerRecord>(&trigger_dir(data_dir))?;
    let ingress_records = read_json_dir::<RoutineIngressRecord>(&ingress_dir(data_dir))?;
    let mut routines = read_json_dir::<RoutineDefinition>(&definition_dir(data_dir))?
        .into_iter()
        .map(|routine| {
            let mut routine_triggers = triggers
                .iter()
                .filter(|trigger| trigger.routine_id == routine.routine_id)
                .cloned()
                .collect::<Vec<_>>();
            routine_triggers.sort_by(|left, right| right.created_at.cmp(&left.created_at));
            let mut routine_ingress = ingress_records
                .iter()
                .filter(|ingress| ingress.routine_id == routine.routine_id)
                .cloned()
                .collect::<Vec<_>>();
            routine_ingress.sort_by(|left, right| right.received_at.cmp(&left.received_at));
            RoutineListEntry {
                routine_id: routine.routine_id,
                name: routine.name,
                trigger_kind: routine.trigger_kind,
                intent: routine.intent,
                role_profile: routine.role_profile,
                delivery_target: routine.delivery_target,
                created_at: routine.created_at,
                updated_at: routine.updated_at,
                trigger_count: routine_triggers.len(),
                last_trigger: routine_triggers.into_iter().next(),
                ingress_count: routine_ingress.len(),
                last_ingress: routine_ingress.into_iter().next(),
            }
        })
        .collect::<Vec<_>>();
    routines.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(RoutineListResult {
        total_count: routines.len(),
        routines,
    })
}

pub fn inspect(data_dir: &Path, routine_id: &str) -> Result<RoutineInspectResult, RoutineError> {
    let routine = load_definition(data_dir, routine_id)?;
    let mut triggers = read_json_dir::<RoutineTriggerRecord>(&trigger_dir(data_dir))?
        .into_iter()
        .filter(|trigger| trigger.routine_id == routine.routine_id)
        .collect::<Vec<_>>();
    triggers.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    let mut ingress = read_json_dir::<RoutineIngressRecord>(&ingress_dir(data_dir))?
        .into_iter()
        .filter(|ingress| ingress.routine_id == routine.routine_id)
        .collect::<Vec<_>>();
    ingress.sort_by(|left, right| right.received_at.cmp(&left.received_at));
    Ok(RoutineInspectResult {
        definition_path: definition_path(data_dir, routine_id).display().to_string(),
        routine,
        trigger_count: triggers.len(),
        triggers,
        ingress_count: ingress.len(),
        ingress,
    })
}

fn load_definition(data_dir: &Path, routine_id: &str) -> Result<RoutineDefinition, RoutineError> {
    let path = definition_path(data_dir, routine_id);
    if !path.exists() {
        return Err(RoutineError::UnknownRoutine(routine_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_trigger(data_dir: &Path, trigger_id: &str) -> Result<RoutineTriggerRecord, RoutineError> {
    let path = trigger_path(data_dir, trigger_id);
    if !path.exists() {
        return Err(RoutineError::UnknownTrigger(trigger_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn next_attempt(data_dir: &Path, previous_trigger_id: &str) -> Result<u64, RoutineError> {
    let previous = load_trigger(data_dir, previous_trigger_id)?;
    let max_retry_attempt = read_json_dir::<RoutineTriggerRecord>(&trigger_dir(data_dir))?
        .into_iter()
        .filter(|trigger| trigger.retry_of_trigger_id.as_deref() == Some(previous_trigger_id))
        .map(|trigger| trigger.attempt)
        .max()
        .unwrap_or(previous.attempt.max(1));
    Ok(max_retry_attempt + 1)
}

fn find_dedupe_match(
    data_dir: &Path,
    routine_id: &str,
    trigger_kind: &str,
    dedupe_key: &str,
) -> Result<Option<RoutineIngressRecord>, RoutineError> {
    let mut matches = read_json_dir::<RoutineIngressRecord>(&ingress_dir(data_dir))?
        .into_iter()
        .filter(|ingress| ingress.routine_id == routine_id)
        .filter(|ingress| ingress.trigger_kind == trigger_kind)
        .filter(|ingress| ingress.dedupe_key == dedupe_key)
        .filter(|ingress| ingress.status != "duplicate_ignored")
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.received_at.cmp(&left.received_at));
    Ok(matches.into_iter().next())
}

fn validate_trigger_kind(trigger_kind: &str) -> Result<(), RoutineError> {
    if matches!(trigger_kind, "manual" | "schedule" | "webhook" | "api") {
        Ok(())
    } else {
        Err(RoutineError::InvalidInput(format!(
            "invalid trigger kind {trigger_kind}; expected manual, schedule, webhook, or api"
        )))
    }
}

fn validate_millis_timestamp(field: &str, value: &str) -> Result<(), RoutineError> {
    parse_millis_timestamp(value).map(|_| ()).map_err(|_| {
        RoutineError::InvalidInput(format!("{field} must be a unix timestamp in milliseconds"))
    })
}

fn parse_millis_timestamp(value: &str) -> Result<u128, std::num::ParseIntError> {
    value.parse::<u128>()
}

fn definition_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("routines").join("definitions")
}

fn trigger_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("routines").join("triggers")
}

fn ingress_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("routines").join("ingress")
}

fn definition_path(data_dir: &Path, routine_id: &str) -> PathBuf {
    definition_dir(data_dir).join(format!("{routine_id}.json"))
}

fn trigger_path(data_dir: &Path, trigger_id: &str) -> PathBuf {
    trigger_dir(data_dir).join(format!("{trigger_id}.json"))
}

fn ingress_path(data_dir: &Path, ingress_id: &str) -> PathBuf {
    ingress_dir(data_dir).join(format!("{ingress_id}.json"))
}

fn routines_lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join("routines").join("routines.lock")
}

struct RoutineStateLockGuard {
    file: File,
}

fn lock_routines_state(data_dir: &Path) -> Result<RoutineStateLockGuard, RoutineError> {
    let path = routines_lock_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(RoutineError::Io(std::io::Error::last_os_error()));
    }
    Ok(RoutineStateLockGuard { file })
}

impl Drop for RoutineStateLockGuard {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn read_json_dir<T>(dir: &Path) -> Result<Vec<T>, RoutineError>
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

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), RoutineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    fs::write(&tmp_path, serde_json::to_string_pretty(value)?)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn slugify(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "unnamed".to_string()
    } else {
        slug
    }
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}

fn next_sequence() -> u64 {
    ROUTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

fn next_unique_suffix() -> String {
    format!(
        "{}_{}_{}",
        timestamp_string(),
        std::process::id(),
        next_sequence()
    )
}

fn timestamp_string_from_offset(offset_ms: u128) -> String {
    let now = parse_millis_timestamp(&timestamp_string()).unwrap_or_default();
    now.saturating_add(offset_ms).to_string()
}

fn build_recovery_governance_summary(
    data_dir: &Path,
) -> Result<RoutineRecoveryGovernanceSummary, RoutineError> {
    let triggers = read_json_dir::<RoutineTriggerRecord>(&trigger_dir(data_dir))?;
    let ingress_records = read_json_dir::<RoutineIngressRecord>(&ingress_dir(data_dir))?;
    let now = parse_millis_timestamp(&timestamp_string()).unwrap_or_default();
    let mut entries = Vec::new();
    let mut failed_trigger_count = 0usize;
    let mut retryable_trigger_count = 0usize;
    let mut budget_exhausted_count = 0usize;
    let mut backoff_active_count = 0usize;
    let mut pending_ingress_failure_count = 0usize;

    for trigger in &triggers {
        if matches!(
            trigger.status.as_str(),
            "agent_run_failed" | "agent_run_timeout"
        ) {
            failed_trigger_count += 1;
            if trigger.retry_budget_remaining == 0 {
                budget_exhausted_count += 1;
            } else {
                retryable_trigger_count += 1;
            }
            if let Some(next_retry_at) = trigger.next_retry_at.as_deref() {
                if parse_millis_timestamp(next_retry_at).unwrap_or_default() > now {
                    backoff_active_count += 1;
                }
            }
            entries.push(RoutineRecoveryGovernanceEntry {
                object_kind: "trigger".to_string(),
                object_id: trigger.trigger_id.clone(),
                routine_id: trigger.routine_id.clone(),
                status: trigger.status.clone(),
                decision: trigger.recovery_decision.clone(),
                next_retry_at: trigger.next_retry_at.clone(),
                retry_budget_remaining: trigger.retry_budget_remaining,
                error: None,
            });
        }
    }

    for ingress in &ingress_records {
        if ingress.status == "trigger_failed" {
            pending_ingress_failure_count += 1;
            entries.push(RoutineRecoveryGovernanceEntry {
                object_kind: "ingress".to_string(),
                object_id: ingress.ingress_id.clone(),
                routine_id: ingress.routine_id.clone(),
                status: ingress.status.clone(),
                decision: ingress.decision.clone(),
                next_retry_at: None,
                retry_budget_remaining: 0,
                error: Some(ingress.decision.clone()),
            });
        }
    }

    let status = if failed_trigger_count == 0 && pending_ingress_failure_count == 0 {
        "healthy".to_string()
    } else if budget_exhausted_count > 0 {
        "budget_exhausted".to_string()
    } else if backoff_active_count > 0 {
        "backoff_active".to_string()
    } else {
        "needs_retry".to_string()
    };
    let next_recommended_action = if budget_exhausted_count > 0 {
        "inspect_and_replan_failed_routines".to_string()
    } else if backoff_active_count > 0 {
        "wait_for_retry_backoff".to_string()
    } else if pending_ingress_failure_count > 0 {
        "retry_failed_ingress".to_string()
    } else {
        "continue_goal_tick".to_string()
    };

    Ok(RoutineRecoveryGovernanceSummary {
        schema_version: "routine.recovery_governance_summary.v1".to_string(),
        status,
        failed_trigger_count,
        retryable_trigger_count,
        budget_exhausted_count,
        backoff_active_count,
        pending_ingress_failure_count,
        next_recommended_action,
        entries,
    })
}

#[derive(Debug)]
pub enum RoutineError {
    UnknownRoutine(String),
    UnknownTrigger(String),
    InvalidInput(String),
    Agent(agents::AgentError),
    ProjectOps(projectops::ProjectOpsError),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for RoutineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRoutine(routine_id) => write!(formatter, "unknown routine: {routine_id}"),
            Self::UnknownTrigger(trigger_id) => write!(formatter, "unknown trigger: {trigger_id}"),
            Self::InvalidInput(reason) => write!(formatter, "invalid routine input: {reason}"),
            Self::Agent(err) => write!(formatter, "{err}"),
            Self::ProjectOps(err) => write!(formatter, "{err}"),
            Self::Io(err) => write!(formatter, "routine io failed: {err}"),
            Self::Json(err) => write!(formatter, "routine json failed: {err}"),
        }
    }
}

impl std::error::Error for RoutineError {}

impl From<agents::AgentError> for RoutineError {
    fn from(err: agents::AgentError) -> Self {
        Self::Agent(err)
    }
}

impl From<projectops::ProjectOpsError> for RoutineError {
    fn from(err: projectops::ProjectOpsError) -> Self {
        Self::ProjectOps(err)
    }
}

impl From<std::io::Error> for RoutineError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for RoutineError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
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

    #[test]
    fn routine_result_types_round_trip_through_serde_json() {
        let routine = RoutineDefinition {
            schema_version: "routine_definition.v1".to_string(),
            routine_id: "routine_fixture".to_string(),
            name: "Fixture".to_string(),
            trigger_kind: "manual".to_string(),
            intent: "exercise round-trip".to_string(),
            role_profile: "research-worker".to_string(),
            message: "hello".to_string(),
            command: "echo ok".to_string(),
            delivery_target: "local".to_string(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
        };
        let trigger = RoutineTriggerRecord {
            schema_version: "routine_trigger.v1".to_string(),
            trigger_id: "trigger_fixture".to_string(),
            routine_id: routine.routine_id.clone(),
            trigger_kind: routine.trigger_kind.clone(),
            source: "operator".to_string(),
            ingress_id: Some("ingress_fixture".to_string()),
            retry_of_trigger_id: None,
            attempt: 1,
            run_id: "run_fixture".to_string(),
            agent_id: "agent_fixture".to_string(),
            lease_id: "lease_fixture".to_string(),
            task_packet_path: "task_packet.json".to_string(),
            runtime_record_path: "runtime.json".to_string(),
            output_manifest_path: "output_manifest.json".to_string(),
            status: "running".to_string(),
            next_retry_at: None,
            retry_budget_remaining: 3,
            recovery_decision: "continue".to_string(),
            created_at: "1".to_string(),
        };
        let ingress = RoutineIngressRecord {
            schema_version: "routine_ingress.v1".to_string(),
            ingress_id: "ingress_fixture".to_string(),
            routine_id: routine.routine_id.clone(),
            trigger_kind: routine.trigger_kind.clone(),
            source: "operator".to_string(),
            dedupe_key: "dedupe".to_string(),
            status: "ready".to_string(),
            decision: "accept".to_string(),
            received_at: "1".to_string(),
            ready_at: "1".to_string(),
            duplicate_of_ingress_id: String::new(),
            triggered_at: String::new(),
            trigger_id: trigger.trigger_id.clone(),
            run_id: trigger.run_id.clone(),
            agent_id: trigger.agent_id.clone(),
            lease_id: trigger.lease_id.clone(),
            trigger_record_path: trigger.task_packet_path.clone(),
        };
        let trigger_result = RoutineTriggerResult {
            status: "triggered".to_string(),
            routine: routine.clone(),
            trigger: trigger.clone(),
            trigger_record_path: "trigger.json".to_string(),
            agent: agents::AgentStartResult {
                status: "started".to_string(),
                agent_id: "agent_fixture".to_string(),
                task_packet_path: "task_packet.json".to_string(),
                runtime_record_path: "runtime.json".to_string(),
                output_manifest_path: "output_manifest.json".to_string(),
                trace_path: "trace.json".to_string(),
                workspace_binding: None,
                task_packet: agents::TaskPacket {
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
                    budget: agents::AgentBudget {
                        max_turns: 1,
                        max_runtime_ms: 1,
                    },
                    scope: agents::AgentScope {
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
                runtime_record: agents::AgentRuntimeRecord {
                    schema_version: "agent_runtime_record.v1".to_string(),
                    agent_id: "agent_fixture".to_string(),
                    runner_kind: Some("provider".to_string()),
                    runtime_identity_ref: None,
                    role_kind: None,
                    authority_scope: None,
                    session_ref: None,
                    task_packet_ref: "task_packet.json".to_string(),
                    lifecycle_status: "running".to_string(),
                    created_at: "1".to_string(),
                    updated_at: "1".to_string(),
                    heartbeat_at: "1".to_string(),
                    output_manifest_ref: "output_manifest.json".to_string(),
                    workspace_binding_ref: None,
                    directive_ref: None,
                    status_ref: None,
                    trace_refs: Vec::new(),
                    stop_reason: String::new(),
                    failure_code: String::new(),
                },
                output_manifest: agents::AgentOutputManifest {
                    schema_version: "agent_output_manifest.v1".to_string(),
                    agent_id: "agent_fixture".to_string(),
                    manifest_id: "manifest_fixture".to_string(),
                    status: "ok".to_string(),
                    output_refs: Vec::new(),
                    validation_status: "valid".to_string(),
                    validation_errors: Vec::new(),
                    created_at: "1".to_string(),
                },
                trace: agents::AgentTrace {
                    schema_version: "agent_trace.v1".to_string(),
                    agent_id: "agent_fixture".to_string(),
                    trace_id: "trace_fixture".to_string(),
                    task_packet_ref: "task_packet.json".to_string(),
                    runtime_events: Vec::new(),
                    tool_actions: Vec::new(),
                    permission_decisions: Vec::new(),
                    output_records: Vec::new(),
                    final_status: "ok".to_string(),
                    created_at: "1".to_string(),
                    trace_path: "trace.json".to_string(),
                },
            },
            lease: projectops::ExperimentSupervisorLease {
                lease_id: "lease_fixture".to_string(),
                run_id: "run_fixture".to_string(),
                owner_agent_id: "agent_fixture".to_string(),
                state: "active".to_string(),
                claimed_at: "1".to_string(),
                heartbeat_at: "1".to_string(),
                stale_after_sec: 60,
                pending_wake_count: 0,
                last_summary: "ok".to_string(),
                reclaimed_from: String::new(),
            },
            recovery_wake: Some(projectops::WakeEvent {
                wake_id: "wake_fixture".to_string(),
                run_id: "run_fixture".to_string(),
                owner_agent_id: "agent_fixture".to_string(),
                kind: "recovery".to_string(),
                urgency: "low".to_string(),
                requires_main_system: false,
                summary: "wake".to_string(),
                details: String::new(),
                lease_id: "lease_fixture".to_string(),
                state: "pending".to_string(),
                escalation_reason: String::new(),
                no_owner_reason: String::new(),
                acknowledged_by: String::new(),
                acknowledged_at: String::new(),
                resolved_at: String::new(),
                resolution: String::new(),
                escalated_at: String::new(),
            }),
        };
        let run_due = RoutineRunDueResult {
            status: "ok".to_string(),
            selected_count: 1,
            triggered_count: 1,
            failed_count: 0,
            outcomes: vec![RoutineRunDueOutcome {
                status: "triggered".to_string(),
                ingress: ingress.clone(),
                trigger_result: Some(trigger_result.clone()),
                error: None,
            }],
            recovery_governance: RoutineRecoveryGovernanceSummary {
                schema_version: "routine.recovery_governance_summary.v1".to_string(),
                status: "healthy".to_string(),
                failed_trigger_count: 0,
                retryable_trigger_count: 0,
                budget_exhausted_count: 0,
                backoff_active_count: 0,
                pending_ingress_failure_count: 0,
                next_recommended_action: "continue_goal_tick".to_string(),
                entries: Vec::new(),
            },
        };
        let retry = RoutineRetryResult {
            status: "ok".to_string(),
            previous_trigger: trigger.clone(),
            retry: trigger_result.clone(),
            recovery_governance: run_due.recovery_governance.clone(),
        };
        let list_result = RoutineListResult {
            routines: vec![RoutineListEntry {
                routine_id: routine.routine_id.clone(),
                name: routine.name.clone(),
                trigger_kind: routine.trigger_kind.clone(),
                intent: routine.intent.clone(),
                role_profile: routine.role_profile.clone(),
                delivery_target: routine.delivery_target.clone(),
                created_at: routine.created_at.clone(),
                updated_at: routine.updated_at.clone(),
                trigger_count: 1,
                last_trigger: Some(trigger.clone()),
                ingress_count: 1,
                last_ingress: Some(ingress.clone()),
            }],
            total_count: 1,
        };
        let inspect = RoutineInspectResult {
            routine: routine.clone(),
            definition_path: "routine.json".to_string(),
            triggers: vec![trigger.clone()],
            trigger_count: 1,
            ingress: vec![ingress.clone()],
            ingress_count: 1,
        };
        let ingress_result = RoutineIngressResult {
            status: "ok".to_string(),
            ingress: ingress.clone(),
            ingress_record_path: "ingress.json".to_string(),
            duplicate_of: None,
        };
        let create_result = RoutineCreateResult {
            status: "ok".to_string(),
            definition_path: "routine.json".to_string(),
            routine: routine.clone(),
        };

        assert_round_trip(&create_result);
        assert_round_trip(&trigger_result);
        assert_round_trip(&ingress_result);
        assert_round_trip(&run_due);
        assert_round_trip(&retry);
        assert_round_trip(&list_result);
        assert_round_trip(&inspect);
    }
}
