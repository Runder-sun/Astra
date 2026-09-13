use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_WORKING_MEMORY_LIMIT: usize = 20;
static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingMemoryRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub record_id: String,
    pub kind: String,
    pub content: String,
    pub support_refs: Vec<String>,
    pub pinned: bool,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkingMemoryAppendResult {
    pub record: WorkingMemoryRecord,
    pub status: MemoryStatusReport,
    pub evicted_record_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryStatusReport {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub limit: usize,
    pub active_count: usize,
    pub pinned_count: usize,
    pub evicted_count: usize,
    pub append_log_count: usize,
    pub promotion_queue_count: usize,
    pub durable_status: String,
    pub durable_count: usize,
    pub durable_injectable_count: usize,
    pub durable_invalidated_count: usize,
    pub active_records: Vec<WorkingMemoryRecord>,
    pub evicted_record_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingMemoryAppendRequest {
    pub kind: String,
    pub content: String,
    pub support_refs: Vec<String>,
    pub pinned: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub record_id: String,
    pub project_id: String,
    pub namespace: String,
    pub scope: String,
    pub kind: String,
    pub surface: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub provenance: Vec<String>,
    pub source_candidate_id: String,
    pub source_session_id: String,
    pub source_artifacts: Vec<String>,
    pub support_refs: Vec<String>,
    pub status: String,
    pub confidence: f64,
    pub valid_from: String,
    pub valid_to: String,
    pub supersedes: Vec<String>,
    pub superseded_by: String,
    pub usage_count: usize,
    pub last_used_at: String,
    pub injectable: bool,
    pub invalidation_reason: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryPromotionResult {
    pub record: MemoryRecord,
    pub promotion_status: String,
    pub candidate_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPromoteRequest {
    pub candidate_id: String,
    pub trust: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryQueryProvenance {
    pub source_candidate_id: String,
    pub source_session_id: String,
    pub source_artifacts: Vec<String>,
    pub support_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryQueryMatch {
    pub record: MemoryRecord,
    pub score: f64,
    pub auto_inject: bool,
    pub recall_reason: String,
    pub matched_via: Vec<String>,
    pub feedback_adjustments: Vec<String>,
    pub provenance: MemoryQueryProvenance,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryQueryResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub query_id: String,
    pub query: String,
    pub scope: String,
    pub route: String,
    pub budget_policy: RecallBudgetPolicy,
    pub budget_used: usize,
    pub auto_inject_count: usize,
    pub matched_records: Vec<MemoryQueryMatch>,
    pub degraded_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecallBudgetPolicy {
    pub max_records_auto_inject: usize,
    pub max_records_explain_only: usize,
    pub max_tokens_hydrated: usize,
    pub max_cross_project_records: usize,
    pub max_contested_records: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryQueryRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub include_inactive: bool,
}

#[derive(Debug, Clone)]
struct RetrievalCandidate {
    record: MemoryRecord,
    score: f64,
    lane_scores: HashMap<&'static str, f64>,
    matched_via: BTreeSet<String>,
    feedback_adjustments: Vec<String>,
    feedback_penalty: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryExplainRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub record_id: String,
    pub status: String,
    pub injection_decision: String,
    pub recall_reason: String,
    pub matched_via: Vec<String>,
    pub source_refs: Vec<String>,
    pub support_spans: Vec<String>,
    pub pointer_hydration: Vec<String>,
    pub confidence: f64,
    pub superseded_by: String,
    pub contested_by: String,
    pub invalidation_reason: String,
    pub usage_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryInvalidationResult {
    pub record: MemoryRecord,
    pub invalidation_status: String,
}

pub fn default_limit() -> usize {
    DEFAULT_WORKING_MEMORY_LIMIT
}

pub fn append(
    data_dir: &Path,
    request: WorkingMemoryAppendRequest,
) -> Result<WorkingMemoryAppendResult, MemoryError> {
    let limit = normalized_limit(request.limit);
    if request.content.trim().is_empty() {
        return Err(MemoryError::InvalidInput(
            "working-memory content must not be empty".to_string(),
        ));
    }
    if request.kind.trim().is_empty() {
        return Err(MemoryError::InvalidInput(
            "working-memory kind must not be empty".to_string(),
        ));
    }

    fs::create_dir_all(memory_root(data_dir))?;
    let now = timestamp_string();
    let record_id = format!("wm_{}_{}", now, next_sequence());
    let record = WorkingMemoryRecord {
        schema_version: "memory.working_record.v1".to_string(),
        conformance_line: "M7.working_memory".to_string(),
        canonical_path: active_log_path(data_dir).display().to_string(),
        retention_policy: format!("bounded_active_log_limit_{limit}_pin_protected"),
        atomic_write_policy: "append_then_atomic_active_snapshot".to_string(),
        record_id,
        kind: request.kind,
        content: request.content,
        support_refs: request.support_refs,
        pinned: request.pinned,
        status: "active".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    append_json_line(&append_log_path(data_dir), &record)?;
    let mut active_records = read_records(&active_log_path(data_dir))?;
    active_records.push(record.clone());
    let evicted_records = enforce_limit(&mut active_records, limit);
    write_records_atomic(&active_log_path(data_dir), &active_records)?;
    if !evicted_records.is_empty() {
        let mut evicted_log_records = evicted_records.clone();
        for evicted in &mut evicted_log_records {
            evicted.status = "evicted".to_string();
            evicted.updated_at = timestamp_string();
        }
        append_json_lines(&evicted_log_path(data_dir), &evicted_log_records)?;
    }

    let status = status_with_limit(data_dir, Some(limit))?;
    Ok(WorkingMemoryAppendResult {
        record,
        status,
        evicted_record_ids: evicted_records
            .into_iter()
            .map(|record| record.record_id)
            .collect(),
    })
}

pub fn status(data_dir: &Path, limit: Option<usize>) -> Result<MemoryStatusReport, MemoryError> {
    status_with_limit(data_dir, Some(normalized_limit(limit)))
}

pub fn promote(
    data_dir: &Path,
    project_id: &str,
    request: MemoryPromoteRequest,
) -> Result<MemoryPromotionResult, MemoryError> {
    if request.candidate_id.trim().is_empty() {
        return Err(MemoryError::InvalidInput(
            "memory promote requires candidate id".to_string(),
        ));
    }
    if !matches!(request.trust.as_str(), "supported" | "trusted") {
        return Err(MemoryError::InvalidInput(
            "--trust must be supported or trusted".to_string(),
        ));
    }

    let candidate_path =
        promotion_queue_dir(data_dir).join(format!("{}.json", request.candidate_id));
    if !candidate_path.exists() {
        return Err(MemoryError::InvalidInput(format!(
            "unknown memory promotion candidate: {}",
            request.candidate_id
        )));
    }
    let candidate: Value = serde_json::from_str(&fs::read_to_string(&candidate_path)?)?;
    let now = timestamp_string();
    let record_id = format!("mem_{}_{}", now, next_sequence());
    let support_refs = string_array(candidate.get("support_refs"));
    if support_refs.is_empty() {
        return Err(MemoryError::InvalidInput(
            "memory promotion requires support refs".to_string(),
        ));
    }
    let is_trajectory_candidate = candidate.get("schema_version").and_then(Value::as_str)
        == Some("trajectory.memory_candidate.v1");
    if is_trajectory_candidate {
        validate_trajectory_promotion_candidate(&candidate, &support_refs)?;
    }
    let source_artifact_path = candidate
        .get("source_artifact_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let candidate_body = candidate
        .get("body")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let body = candidate_body
        .map(ToOwned::to_owned)
        .or_else(|| read_optional_text(&source_artifact_path))
        .unwrap_or_else(|| candidate.to_string());
    let summary = first_non_empty(&[
        candidate
            .get("summary_ref")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        candidate
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        &source_artifact_path,
    ]);
    let source_session_id = candidate
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = if is_trajectory_candidate {
        first_non_empty(&[
            candidate
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            candidate
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            &request.candidate_id,
        ])
    } else {
        format!("Promoted digest {}", request.candidate_id)
    };
    let kind = candidate
        .get("memory_kind")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("progress_digest")
        .to_string();
    let mut source_artifacts = string_array(candidate.get("source_artifacts"));
    if source_artifacts.is_empty() && !source_artifact_path.is_empty() {
        source_artifacts.push(source_artifact_path.clone());
    }
    let record_path = durable_record_path(data_dir, &record_id);
    let record = MemoryRecord {
        schema_version: "memory.record.v1".to_string(),
        conformance_line: "M8.durable_memory".to_string(),
        canonical_path: record_path.display().to_string(),
        retention_policy: "durable_until_explicit_invalidation_or_supersession".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        record_id: record_id.clone(),
        project_id: project_id.to_string(),
        namespace: "project".to_string(),
        scope: "project".to_string(),
        kind,
        surface: "durable_memory".to_string(),
        title,
        summary,
        body,
        provenance: vec![candidate_path.display().to_string()],
        source_candidate_id: request.candidate_id.clone(),
        source_session_id,
        source_artifacts,
        support_refs,
        status: request.trust.clone(),
        confidence: if request.trust == "trusted" {
            1.0
        } else {
            0.75
        },
        valid_from: now.clone(),
        valid_to: String::new(),
        supersedes: Vec::new(),
        superseded_by: String::new(),
        usage_count: 0,
        last_used_at: String::new(),
        injectable: true,
        invalidation_reason: String::new(),
        created_at: now.clone(),
        updated_at: now,
    };

    append_json_line(&durable_records_log_path(data_dir), &record)?;
    write_json_atomic(&record_path, &record)?;
    append_json_line(
        &durable_history_log_path(data_dir),
        &serde_json::json!({
            "event": "promoted",
            "record_id": record.record_id,
            "candidate_id": request.candidate_id,
            "status": record.status,
            "created_at": record.created_at
        }),
    )?;
    Ok(MemoryPromotionResult {
        record,
        promotion_status: "promoted_to_durable_memory".to_string(),
        candidate_id: request.candidate_id,
    })
}

pub fn query(
    data_dir: &Path,
    request: MemoryQueryRequest,
) -> Result<MemoryQueryResult, MemoryError> {
    let limit = request.limit.unwrap_or(5).max(1);
    let budget = default_recall_budget(limit);
    let records = latest_durable_records(data_dir)?;
    let query_terms = query_terms(&request.query);
    let feedback_adjustments = feedback_memory_adjustments(data_dir)?;
    let seed_candidates = records
        .iter()
        .map(|record| {
            let mut candidate = score_record(record, &request.query, &query_terms);
            apply_feedback_calibration_lane(
                &mut candidate,
                feedback_adjustments.get(&record.record_id),
            );
            candidate
        })
        .filter(|candidate| candidate.score > 0.0 || query_terms.is_empty())
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for record in records {
        if !request.include_inactive && !is_auto_injectable(&record) {
            continue;
        }
        let mut candidate = score_record(&record, &request.query, &query_terms);
        apply_feedback_calibration_lane(
            &mut candidate,
            feedback_adjustments.get(&record.record_id),
        );
        apply_temporal_graph_lane(&mut candidate, &seed_candidates);
        if candidate.score <= 0.0 && !query_terms.is_empty() {
            continue;
        }
        candidates.push(candidate);
    }
    apply_rank_fusion(&mut candidates);
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.record.record_id.cmp(&right.record.record_id))
    });
    candidates.truncate(limit.min(budget.max_records_explain_only));

    let mut matches = Vec::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let auto_inject =
            is_auto_injectable(&candidate.record) && index < budget.max_records_auto_inject;
        matches.push(MemoryQueryMatch {
            provenance: MemoryQueryProvenance {
                source_candidate_id: candidate.record.source_candidate_id.clone(),
                source_session_id: candidate.record.source_session_id.clone(),
                source_artifacts: candidate.record.source_artifacts.clone(),
                support_refs: candidate.record.support_refs.clone(),
            },
            recall_reason: recall_reason_for(&candidate.record, auto_inject),
            matched_via: candidate.matched_via.into_iter().collect(),
            feedback_adjustments: candidate.feedback_adjustments,
            score: round_score(candidate.score),
            auto_inject,
            record: candidate.record,
        });
    }
    let auto_inject_count = matches.iter().filter(|entry| entry.auto_inject).count();
    Ok(MemoryQueryResult {
        schema_version: "memory.query_result.v1".to_string(),
        conformance_line: "M8.memory_query".to_string(),
        canonical_path: durable_records_log_path(data_dir).display().to_string(),
        query_id: format!("mq_{}_{}", timestamp_string(), next_sequence()),
        query: request.query,
        scope: "project".to_string(),
        route: if feedback_adjustments.is_empty() {
            "hybrid_vector_temporal_project_durable_memory".to_string()
        } else {
            "hybrid_vector_temporal_project_durable_memory_feedback_calibrated".to_string()
        },
        budget_policy: budget,
        budget_used: matches.len(),
        auto_inject_count,
        matched_records: matches,
        degraded_reasons: Vec::new(),
    })
}

pub fn explain(data_dir: &Path, record_id: &str) -> Result<MemoryExplainRecord, MemoryError> {
    let record = find_durable_record(data_dir, record_id)?;
    let auto_inject = is_auto_injectable(&record);
    Ok(MemoryExplainRecord {
        schema_version: "memory.explain_record.v1".to_string(),
        conformance_line: "M8.memory_explain".to_string(),
        canonical_path: durable_record_path(data_dir, &record.record_id)
            .display()
            .to_string(),
        record_id: record.record_id.clone(),
        status: record.status.clone(),
        injection_decision: if auto_inject {
            "auto_inject_allowed".to_string()
        } else {
            "explain_visible_only".to_string()
        },
        recall_reason: recall_reason_for(&record, auto_inject),
        matched_via: vec![
            "record_id".to_string(),
            "support_refs".to_string(),
            "source_candidate".to_string(),
        ],
        source_refs: record.source_artifacts.clone(),
        support_spans: record.support_refs.clone(),
        pointer_hydration: record.provenance.clone(),
        confidence: record.confidence,
        superseded_by: record.superseded_by.clone(),
        contested_by: String::new(),
        invalidation_reason: record.invalidation_reason.clone(),
        usage_count: record.usage_count,
    })
}

pub fn invalidate(
    data_dir: &Path,
    record_id: &str,
    status: &str,
    reason: &str,
) -> Result<MemoryInvalidationResult, MemoryError> {
    if !matches!(
        status,
        "invalidated" | "superseded" | "contested" | "expired"
    ) {
        return Err(MemoryError::InvalidInput(
            "--status must be invalidated, superseded, contested, or expired".to_string(),
        ));
    }
    if reason.trim().is_empty() {
        return Err(MemoryError::InvalidInput(
            "memory invalidate requires --reason".to_string(),
        ));
    }
    let mut records = latest_durable_records(data_dir)?;
    let mut updated = None;
    let now = timestamp_string();
    for record in &mut records {
        if record.record_id == record_id {
            record.status = status.to_string();
            record.injectable = false;
            record.valid_to = now.clone();
            record.updated_at = now.clone();
            record.invalidation_reason = reason.to_string();
            updated = Some(record.clone());
            break;
        }
    }
    let record = updated.ok_or_else(|| {
        MemoryError::InvalidInput(format!("unknown durable memory record: {record_id}"))
    })?;
    write_records_log_atomic(&durable_records_log_path(data_dir), &records)?;
    write_json_atomic(&durable_record_path(data_dir, record_id), &record)?;
    append_json_line(
        &durable_history_log_path(data_dir),
        &serde_json::json!({
            "event": status,
            "record_id": record_id,
            "reason": reason,
            "created_at": now
        }),
    )?;
    Ok(MemoryInvalidationResult {
        record,
        invalidation_status: status.to_string(),
    })
}

pub fn invalidate_records_referencing_paths(
    data_dir: &Path,
    paths: &[String],
    reason: &str,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    invalidate_records_referencing_paths_with_policy(data_dir, paths, reason, false)
}

pub fn refresh_invalidated_records_referencing_paths(
    data_dir: &Path,
    paths: &[String],
    reason: &str,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    invalidate_records_referencing_paths_with_policy(data_dir, paths, reason, true)
}

fn invalidate_records_referencing_paths_with_policy(
    data_dir: &Path,
    paths: &[String],
    reason: &str,
    include_already_invalidated: bool,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    let targets = paths
        .iter()
        .filter(|path| !path.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = latest_durable_records(data_dir)?;
    let now = timestamp_string();
    let mut invalidated = Vec::new();
    for record in &mut records {
        let can_refresh = include_already_invalidated && record.status == "invalidated";
        if !is_auto_injectable(record) && !can_refresh {
            continue;
        }
        if record_references_any_path(record, &targets) {
            record.status = "invalidated".to_string();
            record.injectable = false;
            record.valid_to = now.clone();
            record.updated_at = now.clone();
            record.invalidation_reason = reason.to_string();
            invalidated.push(record.clone());
        }
    }
    if invalidated.is_empty() {
        return Ok(invalidated);
    }
    write_records_log_atomic(&durable_records_log_path(data_dir), &records)?;
    for record in &invalidated {
        write_json_atomic(&durable_record_path(data_dir, &record.record_id), record)?;
        append_json_line(
            &durable_history_log_path(data_dir),
            &serde_json::json!({
                "event": "invalidated",
                "record_id": record.record_id,
                "reason": reason,
                "created_at": now
            }),
        )?;
    }
    Ok(invalidated)
}

fn status_with_limit(
    data_dir: &Path,
    normalized_limit: Option<usize>,
) -> Result<MemoryStatusReport, MemoryError> {
    let limit = normalized_limit.unwrap_or_else(default_limit);
    fs::create_dir_all(memory_root(data_dir))?;
    let mut active_records = read_records(&active_log_path(data_dir))?;
    let evicted_now = enforce_limit(&mut active_records, limit);
    if !evicted_now.is_empty() {
        write_records_atomic(&active_log_path(data_dir), &active_records)?;
        let mut evicted_log_records = evicted_now;
        for evicted in &mut evicted_log_records {
            evicted.status = "evicted".to_string();
            evicted.updated_at = timestamp_string();
        }
        append_json_lines(&evicted_log_path(data_dir), &evicted_log_records)?;
    }

    let evicted_records = read_records(&evicted_log_path(data_dir))?;
    let append_log_count = read_records(&append_log_path(data_dir))?.len();
    let pinned_count = active_records.iter().filter(|record| record.pinned).count();
    let durable_records = latest_durable_records(data_dir)?;
    let durable_injectable_count = durable_records
        .iter()
        .filter(|record| is_auto_injectable(record))
        .count();
    let durable_invalidated_count = durable_records
        .iter()
        .filter(|record| record.status == "invalidated")
        .count();
    Ok(MemoryStatusReport {
        schema_version: "memory.status_report.v1".to_string(),
        conformance_line: "M8.memory_status".to_string(),
        canonical_path: active_log_path(data_dir).display().to_string(),
        retention_policy: format!("bounded_active_log_limit_{limit}_pin_protected"),
        atomic_write_policy: "append_then_atomic_active_snapshot".to_string(),
        limit,
        active_count: active_records.len(),
        pinned_count,
        evicted_count: evicted_records.len(),
        append_log_count,
        promotion_queue_count: promotion_queue_count(data_dir)?,
        durable_status: "available".to_string(),
        durable_count: durable_records.len(),
        durable_injectable_count,
        durable_invalidated_count,
        active_records,
        evicted_record_ids: evicted_records
            .into_iter()
            .map(|record| record.record_id)
            .collect(),
    })
}

fn normalized_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or_else(default_limit).max(1)
}

fn enforce_limit(records: &mut Vec<WorkingMemoryRecord>, limit: usize) -> Vec<WorkingMemoryRecord> {
    let mut evicted = Vec::new();
    while records.len() > limit {
        if let Some(index) = records.iter().position(|record| !record.pinned) {
            evicted.push(records.remove(index));
        } else {
            break;
        }
    }
    evicted
}

fn read_records(path: &Path) -> Result<Vec<WorkingMemoryRecord>, MemoryError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line)?);
    }
    Ok(records)
}

fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<(), MemoryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

fn append_json_lines<T: Serialize>(path: &Path, values: &[T]) -> Result<(), MemoryError> {
    for value in values {
        append_json_line(path, value)?;
    }
    Ok(())
}

fn write_records_atomic(path: &Path, records: &[WorkingMemoryRecord]) -> Result<(), MemoryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    {
        let mut file = fs::File::create(&tmp_path)?;
        for record in records {
            writeln!(file, "{}", serde_json::to_string(record)?)?;
        }
    }
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn promotion_queue_count(data_dir: &Path) -> Result<usize, MemoryError> {
    let path = promotion_queue_dir(data_dir);
    if !path.exists() {
        return Ok(0);
    }
    Ok(fs::read_dir(path)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count())
}

fn latest_durable_records(data_dir: &Path) -> Result<Vec<MemoryRecord>, MemoryError> {
    let path = durable_records_log_path(data_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut records_by_id = HashMap::<String, MemoryRecord>::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: MemoryRecord = serde_json::from_str(&line)?;
        records_by_id.insert(record.record_id.clone(), record);
    }
    let mut records = records_by_id.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(records)
}

fn find_durable_record(data_dir: &Path, record_id: &str) -> Result<MemoryRecord, MemoryError> {
    latest_durable_records(data_dir)?
        .into_iter()
        .find(|record| record.record_id == record_id)
        .ok_or_else(|| {
            MemoryError::InvalidInput(format!("unknown durable memory record: {record_id}"))
        })
}

fn write_records_log_atomic(path: &Path, records: &[MemoryRecord]) -> Result<(), MemoryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    {
        let mut file = fs::File::create(&tmp_path)?;
        for record in records {
            writeln!(file, "{}", serde_json::to_string(record)?)?;
        }
    }
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), MemoryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    fs::write(&tmp_path, serde_json::to_string_pretty(value)?)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn default_recall_budget(limit: usize) -> RecallBudgetPolicy {
    RecallBudgetPolicy {
        max_records_auto_inject: limit.min(5),
        max_records_explain_only: limit.max(20),
        max_tokens_hydrated: 4000,
        max_cross_project_records: 0,
        max_contested_records: 1,
    }
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|term| term.len() > 2)
        .map(str::to_lowercase)
        .collect()
}

fn score_record(record: &MemoryRecord, query: &str, query_terms: &[String]) -> RetrievalCandidate {
    let mut lane_scores = HashMap::new();
    let mut matched_via = BTreeSet::new();
    if query_terms.is_empty() {
        lane_scores.insert("empty_query", 1.0);
        matched_via.insert("empty_query".to_string());
        return RetrievalCandidate {
            record: record.clone(),
            score: 1.0,
            lane_scores,
            matched_via,
            feedback_adjustments: Vec::new(),
            feedback_penalty: 0.0,
        };
    }

    let lexical_fields = [
        ("title", record.title.as_str()),
        ("summary", record.summary.as_str()),
        ("body", record.body.as_str()),
    ];
    let lexical = field_term_score(&lexical_fields, query_terms, &mut matched_via);
    insert_lane(&mut lane_scores, "lexical", lexical);

    let support_refs = record.support_refs.join(" ");
    let support_ref_score = text_term_score(&support_refs, query_terms)
        + exact_identifier_score(query, &[support_refs.as_str()]);
    if support_ref_score > 0.0 {
        lane_scores.insert("support_refs", support_ref_score);
        matched_via.insert("support_refs".to_string());
    }

    let source_artifacts = record.source_artifacts.join(" ");
    let provenance = record.provenance.join(" ");
    let identifier_fields = [
        source_artifacts.as_str(),
        provenance.as_str(),
        record.canonical_path.as_str(),
        record.source_candidate_id.as_str(),
        record.source_session_id.as_str(),
    ];
    let source_artifact_score = text_term_score(&source_artifacts, query_terms)
        + exact_identifier_score(query, &[source_artifacts.as_str()]);
    if source_artifact_score > 0.0 {
        lane_scores.insert("source_artifacts", source_artifact_score);
        matched_via.insert("source_artifacts".to_string());
    }
    let provenance_score = text_term_score(&provenance, query_terms)
        + text_term_score(&record.canonical_path, query_terms)
        + text_term_score(&record.source_candidate_id, query_terms)
        + text_term_score(&record.source_session_id, query_terms)
        + exact_identifier_score(query, &identifier_fields);
    if provenance_score > 0.0 {
        lane_scores.insert("provenance", provenance_score);
        matched_via.insert("provenance".to_string());
    }
    let exact_identifier = exact_identifier_score(query, &identifier_fields)
        + exact_identifier_score(query, &[support_refs.as_str()]);
    if exact_identifier > 0.0 {
        lane_scores.insert("exact_identifier", exact_identifier);
        matched_via.insert("exact_identifier".to_string());
    }

    let all_text = [
        record.title.as_str(),
        record.summary.as_str(),
        record.body.as_str(),
        support_refs.as_str(),
        source_artifacts.as_str(),
        provenance.as_str(),
    ]
    .join(" ");
    insert_lane(
        &mut lane_scores,
        "semantic_lite",
        semantic_lite_score(&all_text, query_terms),
    );
    let vector_score = vector_embedding_score(&all_text, query_terms);
    if vector_score > 0.20 {
        lane_scores.insert("vector_embedding", vector_score);
        matched_via.insert("vector_embedding".to_string());
    }

    if is_auto_injectable(record) {
        lane_scores.insert(
            "temporal_trust",
            (record.confidence.clamp(0.0, 1.0) + freshness_hint(record)) / 2.0,
        );
        matched_via.insert("temporal_trust".to_string());
    }

    let score = lane_scores.values().sum::<f64>();
    RetrievalCandidate {
        record: record.clone(),
        score,
        lane_scores,
        matched_via,
        feedback_adjustments: Vec::new(),
        feedback_penalty: 0.0,
    }
}

fn insert_lane(lane_scores: &mut HashMap<&'static str, f64>, lane: &'static str, score: f64) {
    if score > 0.0 {
        lane_scores.insert(lane, score);
    }
}

fn field_term_score(
    fields: &[(&str, &str)],
    query_terms: &[String],
    matched_via: &mut BTreeSet<String>,
) -> f64 {
    let mut score = 0.0;
    for (name, text) in fields {
        let hits = text_term_score(text, query_terms);
        if hits > 0.0 {
            score += hits;
            matched_via.insert((*name).to_string());
        }
    }
    score
}

fn text_term_score(text: &str, query_terms: &[String]) -> f64 {
    let lower = text.to_lowercase();
    query_terms
        .iter()
        .filter(|term| lower.contains(term.as_str()))
        .count() as f64
}

fn exact_identifier_score(query: &str, fields: &[&str]) -> f64 {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.len() < 4 || !looks_like_identifier(&normalized_query) {
        return 0.0;
    }
    fields
        .iter()
        .filter(|field| field.to_lowercase().contains(&normalized_query))
        .count() as f64
}

fn looks_like_identifier(query: &str) -> bool {
    query.contains('/')
        || query.contains('\\')
        || query.contains('.')
        || query.contains(':')
        || query.contains('_')
        || query.contains('-')
}

fn semantic_lite_score(text: &str, query_terms: &[String]) -> f64 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let lower = text.to_lowercase();
    let hits = query_terms
        .iter()
        .filter(|term| lower.contains(term.as_str()))
        .count();
    if hits == 0 {
        return 0.0;
    }
    hits as f64 / query_terms.len() as f64
}

fn freshness_hint(record: &MemoryRecord) -> f64 {
    if record.valid_to.is_empty() {
        1.0
    } else {
        0.25
    }
}

fn vector_embedding_score(text: &str, query_terms: &[String]) -> f64 {
    let query_tokens = expanded_semantic_tokens(query_terms);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let text_terms = query_terms_from_text(text);
    let text_tokens = expanded_semantic_tokens(&text_terms);
    cosine_similarity(
        &hashed_embedding(&query_tokens),
        &hashed_embedding(&text_tokens),
    )
}

fn query_terms_from_text(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|term| term.len() > 2)
        .map(str::to_lowercase)
        .collect()
}

fn expanded_semantic_tokens(terms: &[String]) -> Vec<String> {
    let mut tokens = BTreeSet::new();
    for term in terms {
        tokens.insert(term.to_string());
        for alias in semantic_aliases(term) {
            tokens.insert(alias.to_string());
        }
    }
    tokens.into_iter().collect()
}

fn semantic_aliases(term: &str) -> &'static [&'static str] {
    match term {
        "phone" => &["mobile", "app", "device"],
        "mobile" => &["phone", "app", "device"],
        "app" => &["mobile", "phone", "client"],
        "tunnel" => &["overlay", "route", "vpn"],
        "overlay" => &["tunnel", "route", "vpn"],
        "route" => &["path", "overlay", "tunnel"],
        "path" => &["route", "artifact", "file"],
        "restore" => &["rollback", "revert", "cleanup"],
        "rollback" => &["restore", "revert", "cleanup"],
        "memory" => &["recall", "remember", "durable"],
        "recall" => &["memory", "remember", "query"],
        "legacy" => &["old", "previous", "superseded"],
        "current" => &["latest", "canonical", "active"],
        "canonical" => &["current", "latest", "authoritative"],
        _ => &[],
    }
}

fn hashed_embedding(tokens: &[String]) -> Vec<f64> {
    const DIMENSIONS: usize = 64;
    let mut vector = vec![0.0; DIMENSIONS];
    for token in tokens {
        let digest = Sha256::digest(token.as_bytes());
        let index = (usize::from(digest[0]) << 8 | usize::from(digest[1])) % DIMENSIONS;
        let sign = if digest[2] & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    normalize_vector(vector)
}

fn normalize_vector(mut vector: Vec<f64>) -> Vec<f64> {
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm == 0.0 {
        return vector;
    }
    for value in &mut vector {
        *value /= norm;
    }
    vector
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f64>()
        .max(0.0)
}

fn apply_temporal_graph_lane(
    candidate: &mut RetrievalCandidate,
    seed_candidates: &[RetrievalCandidate],
) {
    if !is_auto_injectable(&candidate.record) {
        return;
    }
    let score = seed_candidates
        .iter()
        .filter(|seed| seed.record.record_id != candidate.record.record_id)
        .map(|seed| temporal_relation_score(&candidate.record, &seed.record) * seed.score.max(1.0))
        .fold(0.0, f64::max);
    if score > 0.0 {
        candidate.lane_scores.insert("temporal_graph", score);
        candidate.matched_via.insert("temporal_graph".to_string());
        candidate.score += score;
    }
}

fn temporal_relation_score(candidate: &MemoryRecord, seed: &MemoryRecord) -> f64 {
    let mut score: f64 = 0.0;
    if candidate.supersedes.iter().any(|id| id == &seed.record_id)
        || seed.superseded_by == candidate.record_id
    {
        score = score.max(4.0);
    }
    if !is_auto_injectable(seed) {
        score = score.max(temporal_overlap_score(candidate, seed) * 2.0);
    } else {
        score = score.max(temporal_overlap_score(candidate, seed));
    }
    score
}

fn temporal_overlap_score(left: &MemoryRecord, right: &MemoryRecord) -> f64 {
    let mut score = 0.0;
    if !left.source_session_id.is_empty() && left.source_session_id == right.source_session_id {
        score += 1.0;
    }
    if has_overlap(&left.support_refs, &right.support_refs) {
        score += 1.5;
    }
    if has_overlap(&left.source_artifacts, &right.source_artifacts) {
        score += 1.0;
    }
    if has_overlap(&left.provenance, &right.provenance) {
        score += 0.5;
    }
    score
}

fn has_overlap(left: &[String], right: &[String]) -> bool {
    left.iter().any(|left| {
        right.iter().any(|right| {
            left == right
                || (!left.is_empty() && right.contains(left))
                || (!right.is_empty() && left.contains(right))
        })
    })
}

fn apply_rank_fusion(candidates: &mut [RetrievalCandidate]) {
    let lanes = [
        ("temporal_graph", 5.0),
        ("exact_identifier", 4.0),
        ("vector_embedding", 3.0),
        ("source_artifacts", 2.5),
        ("provenance", 2.0),
        ("support_refs", 1.5),
        ("lexical", 1.0),
        ("semantic_lite", 0.75),
        ("feedback_calibration", 0.5),
        ("temporal_trust", 0.25),
        ("empty_query", 1.0),
    ];
    let mut fused_scores = HashMap::<String, f64>::new();
    for (lane, weight) in lanes {
        let mut ranked = candidates
            .iter()
            .filter_map(|candidate| {
                candidate
                    .lane_scores
                    .get(lane)
                    .copied()
                    .filter(|score| *score > 0.0)
                    .map(|score| (candidate.record.record_id.clone(), score))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        for (index, (record_id, _)) in ranked.into_iter().enumerate() {
            let rank = index + 1;
            let contribution = weight / (60.0 + rank as f64);
            *fused_scores.entry(record_id).or_insert(0.0) += contribution;
        }
    }

    for candidate in candidates {
        let fused = fused_scores
            .get(&candidate.record.record_id)
            .copied()
            .unwrap_or(0.0);
        let trust_multiplier = 0.75 + candidate.record.confidence.clamp(0.0, 1.0) * 0.25;
        candidate.score = ((fused * trust_multiplier) - candidate.feedback_penalty).max(0.0);
    }
}

fn apply_feedback_calibration_lane(
    candidate: &mut RetrievalCandidate,
    adjustment: Option<&FeedbackMemoryAdjustment>,
) {
    let Some(adjustment) = adjustment else {
        return;
    };
    if adjustment.total_weight == 0.0 {
        return;
    }
    let score = (adjustment.total_weight.abs() / 10.0).clamp(0.05, 1.0);
    if adjustment.total_weight > 0.0 {
        candidate.lane_scores.insert("feedback_calibration", score);
        candidate
            .matched_via
            .insert("feedback_calibration".to_string());
        candidate.feedback_adjustments.push(format!(
            "memory_adoption:positive_weight={}",
            round_score(adjustment.total_weight)
        ));
    } else {
        candidate
            .matched_via
            .insert("feedback_calibration".to_string());
        candidate.feedback_penalty += score;
        candidate.feedback_adjustments.push(format!(
            "memory_adoption:negative_weight={}",
            round_score(adjustment.total_weight)
        ));
    }
}

#[derive(Debug, Clone)]
struct FeedbackMemoryAdjustment {
    total_weight: f64,
}

fn feedback_memory_adjustments(
    data_dir: &Path,
) -> Result<HashMap<String, FeedbackMemoryAdjustment>, MemoryError> {
    if !feedback_calibration_enables_memory_retrieval(data_dir)? {
        return Ok(HashMap::new());
    }
    let path = memory_root(data_dir).join("adoption_events.jsonl");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut adjustments = HashMap::<String, FeedbackMemoryAdjustment>::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)?;
        let memory_id = value
            .get("memory_record_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if memory_id.is_empty() {
            continue;
        }
        let weight = value.get("weight").and_then(Value::as_f64).unwrap_or(0.0);
        adjustments
            .entry(memory_id.to_string())
            .and_modify(|entry| entry.total_weight += weight)
            .or_insert(FeedbackMemoryAdjustment {
                total_weight: weight,
            });
    }
    Ok(adjustments)
}

fn feedback_calibration_enables_memory_retrieval(data_dir: &Path) -> Result<bool, MemoryError> {
    let path = data_dir
        .join("feedback")
        .join("calibration")
        .join("latest.json");
    if !path.exists() {
        return Ok(false);
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    if value.get("authority_boundary").and_then(Value::as_str) != Some("advisory_only") {
        return Ok(false);
    }
    Ok(value
        .get("routing_adjustments")
        .and_then(Value::as_array)
        .map(|adjustments| {
            adjustments.iter().any(|adjustment| {
                adjustment.get("target_lane").and_then(Value::as_str) == Some("memory_retrieval")
            })
        })
        .unwrap_or(false))
}

fn round_score(score: f64) -> f64 {
    (score * 1_000_000.0).round() / 1_000_000.0
}

fn is_auto_injectable(record: &MemoryRecord) -> bool {
    record.injectable && matches!(record.status.as_str(), "supported" | "trusted")
}

fn record_references_any_path(record: &MemoryRecord, paths: &[String]) -> bool {
    record
        .support_refs
        .iter()
        .chain(record.source_artifacts.iter())
        .chain(record.provenance.iter())
        .any(|reference| {
            paths.iter().any(|path| {
                reference == path || reference.contains(path) || path.contains(reference)
            })
        })
}

fn recall_reason_for(record: &MemoryRecord, auto_inject: bool) -> String {
    if auto_inject {
        "trusted_or_supported_project_memory_with_support_refs".to_string()
    } else if record.status == "invalidated" {
        "invalidated_memory_is_explain_visible_only".to_string()
    } else if record.status == "superseded" {
        "superseded_memory_is_explain_visible_only".to_string()
    } else if record.status == "contested" {
        "contested_memory_is_explain_visible_only".to_string()
    } else {
        "memory_is_not_auto_injectable".to_string()
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn read_optional_text(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn validate_trajectory_promotion_candidate(
    candidate: &Value,
    support_refs: &[String],
) -> Result<(), MemoryError> {
    let segment_tag = required_candidate_string(candidate, "segment_tag")?;
    let _quality_id = required_candidate_string(candidate, "quality_id")?;
    if !support_refs
        .iter()
        .any(|support_ref| support_ref == segment_tag)
    {
        return Err(MemoryError::InvalidInput(
            "trajectory promotion requires support_refs to include segment_tag".to_string(),
        ));
    }
    if candidate
        .get("promotion_status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "invalidated" | "rejected"))
        || candidate
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "invalidated" | "rejected"))
    {
        return Err(MemoryError::InvalidInput(
            "trajectory promotion candidate is not promotable".to_string(),
        ));
    }
    Ok(())
}

fn required_candidate_string<'a>(
    candidate: &'a Value,
    field: &str,
) -> Result<&'a str, MemoryError> {
    candidate
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            MemoryError::InvalidInput(
                "trajectory promotion requires segment_tag and quality_id".to_string(),
            )
        })
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| (*value).to_string())
        .unwrap_or_else(|| "durable memory".to_string())
}

fn memory_root(data_dir: &Path) -> PathBuf {
    data_dir.join("memory")
}

fn durable_records_log_path(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("durable").join("records.jsonl")
}

fn durable_history_log_path(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("durable").join("history.jsonl")
}

fn durable_record_path(data_dir: &Path, record_id: &str) -> PathBuf {
    memory_root(data_dir)
        .join("durable")
        .join("records")
        .join(format!("{record_id}.json"))
}

fn active_log_path(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("working").join("active.jsonl")
}

fn append_log_path(data_dir: &Path) -> PathBuf {
    memory_root(data_dir)
        .join("working")
        .join("append_log.jsonl")
}

fn evicted_log_path(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("working").join("evicted.jsonl")
}

fn promotion_queue_dir(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("promotion_queue")
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}

fn next_sequence() -> u64 {
    RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
pub enum MemoryError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidInput(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "memory io failed: {err}"),
            Self::Json(err) => write!(formatter, "memory json failed: {err}"),
            Self::InvalidInput(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for MemoryError {}

impl From<std::io::Error> for MemoryError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for MemoryError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}
