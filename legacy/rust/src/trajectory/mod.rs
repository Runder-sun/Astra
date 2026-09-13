use crate::events::writer::lock_event_log;
use crate::memory;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TRAJECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryIngestRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub ingest_id: String,
    pub project_id: String,
    pub source_kind: String,
    pub source_agent_id: String,
    pub source_session_id: String,
    pub source_path: String,
    pub source_digest: String,
    pub normalized_format: String,
    pub ingest_status: String,
    pub segment_count: usize,
    pub degraded_reasons: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSegmentRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub segment_id: String,
    pub ingest_id: String,
    pub project_id: String,
    pub segment_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub message_count: usize,
    pub fingerprint: String,
    pub topic: String,
    pub summary: String,
    pub segmentation_method: String,
    pub segmentation_model: String,
    pub segmentation_prompt_digest: String,
    pub quality_status: String,
    pub memory_extraction_status: String,
    pub segment_tag: String,
    pub status: String,
    pub supersedes: Vec<String>,
    pub superseded_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentQualityCriteria {
    pub task_completion: f64,
    pub evidence_density: f64,
    pub artifact_linkage: f64,
    pub contradiction_risk: f64,
    pub privacy_risk: f64,
    pub reuse_value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentQualityRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub quality_id: String,
    pub segment_id: String,
    pub provider: String,
    pub model: String,
    pub criteria: SegmentQualityCriteria,
    pub overall_score: f64,
    pub memory_eligible: bool,
    pub failure_reason: String,
    pub evidence_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentMemoryCandidate {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub candidate_id: String,
    pub segment_id: String,
    pub ingest_id: String,
    pub project_id: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub memory_kind: String,
    pub support_refs: Vec<String>,
    pub source_artifacts: Vec<String>,
    pub source_artifact_path: String,
    pub segment_tag: String,
    pub quality_id: String,
    pub promotion_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub session_id: String,
    pub summary_ref: String,
    pub status: String,
    pub confidence: String,
    pub degraded_reasons: Vec<String>,
    pub promotes_to_durable_memory: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryAdoptionEvent {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub event_id: String,
    pub memory_record_id: String,
    pub segment_id: String,
    pub event_kind: String,
    pub weight: f64,
    pub surface: String,
    pub actor: String,
    pub support_ref: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryTierExplanation {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub memory_record_id: String,
    pub tier: String,
    pub adoption_score: f64,
    pub event_counts: BTreeMap<String, usize>,
    pub last_positive_event_at: String,
    pub negative_event_count: usize,
    pub ranking_reason: String,
    pub governance_limits: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryIngestRequest {
    pub source_path: String,
    pub source_kind: String,
    pub source_agent_id: String,
    pub source_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrajectoryIngestResult {
    pub record: TrajectoryIngestRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrajectorySegmentResult {
    pub ingest: TrajectoryIngestRecord,
    pub segments: Vec<TaskSegmentRecord>,
    pub superseded_segment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentQualityResult {
    pub segment: TaskSegmentRecord,
    pub quality: SegmentQualityRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentExtractResult {
    pub segment: TaskSegmentRecord,
    pub quality: SegmentQualityRecord,
    pub candidate: SegmentMemoryCandidate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrajectoryStatusReport {
    pub schema_version: String,
    pub conformance_line: String,
    pub ingest_count: usize,
    pub segment_count: usize,
    pub quality_count: usize,
    pub candidate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryAdoptionResult {
    pub event: MemoryAdoptionEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryTierRebalanceResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub explanations: Vec<MemoryTierExplanation>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryTierExplainResult {
    pub explanation: MemoryTierExplanation,
}

#[derive(Debug, Clone)]
struct SessionMessage {
    role: String,
    content: String,
    line_number: usize,
}

#[derive(Debug, Clone)]
struct SegmentDraft {
    segment_index: usize,
    start_line: usize,
    end_line: usize,
    topic: String,
    method: String,
}

pub fn ingest(
    data_dir: &Path,
    project_id: &str,
    request: TrajectoryIngestRequest,
) -> Result<TrajectoryIngestResult, TrajectoryError> {
    validate_source_kind(&request.source_kind)?;
    let workspace_root = data_dir.parent().unwrap_or(data_dir);
    let source_path = workspace_relative_existing_file(workspace_root, &request.source_path)?;
    let relative_source_path = source_path
        .strip_prefix(workspace_root.canonicalize()?)
        .unwrap_or(&source_path)
        .to_string_lossy()
        .replace('\\', "/");
    if !source_path.exists() {
        return Err(TrajectoryError::InvalidInput(format!(
            "trajectory source does not exist: {}",
            request.source_path
        )));
    }
    let now = timestamp_string();
    let ingest_id = format!("traj_ingest_{}_{}", now, next_sequence());
    let canonical_path = ingest_record_path(data_dir, &ingest_id);
    let record = TrajectoryIngestRecord {
        schema_version: "trajectory.ingest_record.v1".to_string(),
        conformance_line: "M15.trajectory_ingest".to_string(),
        canonical_path: canonical_path.display().to_string(),
        retention_policy: "project_local_until_explicit_cleanup".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        ingest_id,
        project_id: project_id.to_string(),
        source_kind: request.source_kind,
        source_agent_id: request.source_agent_id,
        source_session_id: request.source_session_id,
        source_path: relative_source_path,
        source_digest: digest_file(&source_path)?,
        normalized_format: "jsonl.v1".to_string(),
        ingest_status: "pending_segmentation".to_string(),
        segment_count: 0,
        degraded_reasons: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
    };
    fs::create_dir_all(ingests_dir(data_dir))?;
    write_json_atomic(&canonical_path, &record)?;
    append_json_line(&ingests_log_path(data_dir), &record)?;
    Ok(TrajectoryIngestResult { record })
}

pub fn segment(
    data_dir: &Path,
    ingest_id: &str,
) -> Result<TrajectorySegmentResult, TrajectoryError> {
    let mut ingest = read_ingest(data_dir, ingest_id)?;
    let messages = read_session_messages(&ingest.source_path)?;
    if messages.is_empty() {
        return Err(TrajectoryError::InvalidInput(
            "trajectory source has no usable messages".to_string(),
        ));
    }
    let drafts = segment_drafts(&messages);
    fs::create_dir_all(segments_dir(data_dir))?;
    let now = timestamp_string();
    let mut segments = Vec::new();
    let mut superseded_segment_ids = Vec::new();
    for draft in drafts {
        let segment_messages = messages
            .iter()
            .filter(|message| {
                message.line_number >= draft.start_line && message.line_number <= draft.end_line
            })
            .cloned()
            .collect::<Vec<_>>();
        let fingerprint = compute_segment_fingerprint(&segment_messages);
        if let Some(existing) = find_reusable_segment(
            data_dir,
            &ingest.source_path,
            draft.segment_index,
            &fingerprint,
        )? {
            segments.push(existing);
            continue;
        }
        let segment_id = format!("traj_seg_{}_{}", now, next_sequence());
        let old_segments =
            active_segments_for_source_index(data_dir, &ingest.source_path, draft.segment_index)?
                .into_iter()
                .filter(|segment| segment.fingerprint != fingerprint)
                .collect::<Vec<_>>();
        let canonical_path = segment_record_path(data_dir, &segment_id);
        let segment_tag = format!("segment:{}", short_id(&segment_id));
        let record = TaskSegmentRecord {
            schema_version: "trajectory.task_segment.v1".to_string(),
            conformance_line: "M15.task_segment".to_string(),
            canonical_path: canonical_path.display().to_string(),
            retention_policy: "project_local_until_source_segment_superseded".to_string(),
            atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
            segment_id: segment_id.clone(),
            ingest_id: ingest_id.to_string(),
            project_id: ingest.project_id.clone(),
            segment_index: draft.segment_index,
            start_line: draft.start_line,
            end_line: draft.end_line,
            message_count: segment_messages.len(),
            fingerprint,
            topic: draft.topic.clone(),
            summary: draft.topic,
            segmentation_method: draft.method,
            segmentation_model: String::new(),
            segmentation_prompt_digest: String::new(),
            quality_status: "pending".to_string(),
            memory_extraction_status: "not_started".to_string(),
            segment_tag,
            status: "active".to_string(),
            supersedes: old_segments
                .iter()
                .map(|segment| segment.segment_id.clone())
                .collect(),
            superseded_by: String::new(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        superseded_segment_ids.extend(
            old_segments
                .iter()
                .map(|segment| segment.segment_id.clone()),
        );
        for old in old_segments {
            supersede_segment(data_dir, old, &segment_id)?;
        }
        write_json_atomic(&canonical_path, &record)?;
        append_json_line(&segments_log_path(data_dir), &record)?;
        segments.push(record);
    }
    ingest.ingest_status = "segmented".to_string();
    ingest.segment_count = segments.len();
    ingest.updated_at = timestamp_string();
    write_json_atomic(&ingest_record_path(data_dir, ingest_id), &ingest)?;
    append_json_line(&ingests_log_path(data_dir), &ingest)?;
    Ok(TrajectorySegmentResult {
        ingest,
        segments,
        superseded_segment_ids,
    })
}

pub fn label(data_dir: &Path, segment_id: &str) -> Result<SegmentQualityResult, TrajectoryError> {
    let mut segment = read_segment(data_dir, segment_id)?;
    let ingest = read_ingest(data_dir, &segment.ingest_id)?;
    let text = segment_text(&ingest.source_path, &segment)?;
    let evidence_refs = extract_support_refs(&text);
    let privacy_risk = if contains_secret_like_text(&text) {
        1.0
    } else {
        0.0
    };
    let evidence_density = if evidence_refs.is_empty() { 0.0 } else { 1.0 };
    let criteria = SegmentQualityCriteria {
        task_completion: 1.0,
        evidence_density,
        artifact_linkage: evidence_density,
        contradiction_risk: 0.0,
        privacy_risk,
        reuse_value: 0.8,
    };
    let positive = criteria.task_completion
        + criteria.evidence_density
        + criteria.artifact_linkage
        + criteria.reuse_value;
    let negative = criteria.privacy_risk + criteria.contradiction_risk;
    let overall_score = clamp_score((positive / 4.0) * (1.0 - (negative / 2.0)));
    let memory_eligible =
        overall_score >= 0.70 && privacy_risk <= 0.20 && !evidence_refs.is_empty();
    let failure_reason = if memory_eligible {
        String::new()
    } else if privacy_risk > 0.20 {
        "privacy_risk".to_string()
    } else if evidence_refs.is_empty() {
        "missing_support_refs".to_string()
    } else {
        "quality_below_threshold".to_string()
    };
    let now = timestamp_string();
    let quality_id = format!("traj_quality_{}_{}", now, next_sequence());
    let canonical_path = quality_record_path(data_dir, &quality_id);
    let quality = SegmentQualityRecord {
        schema_version: "trajectory.segment_quality.v1".to_string(),
        conformance_line: "M15.segment_quality".to_string(),
        canonical_path: canonical_path.display().to_string(),
        retention_policy: "append_only_quality_evidence".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        quality_id,
        segment_id: segment_id.to_string(),
        provider: "deterministic".to_string(),
        model: "rules.v1".to_string(),
        criteria,
        overall_score,
        memory_eligible,
        failure_reason,
        evidence_refs,
        created_at: now.clone(),
    };
    fs::create_dir_all(quality_dir(data_dir))?;
    write_json_atomic(&canonical_path, &quality)?;
    append_json_line(&quality_log_path(data_dir), &quality)?;
    segment.quality_status = if memory_eligible {
        "labeled".to_string()
    } else {
        "ineligible".to_string()
    };
    segment.updated_at = now;
    write_json_atomic(&segment_record_path(data_dir, segment_id), &segment)?;
    append_json_line(&segments_log_path(data_dir), &segment)?;
    Ok(SegmentQualityResult { segment, quality })
}

pub fn extract(data_dir: &Path, segment_id: &str) -> Result<SegmentExtractResult, TrajectoryError> {
    let mut segment = read_segment(data_dir, segment_id)?;
    let ingest = read_ingest(data_dir, &segment.ingest_id)?;
    let quality = latest_quality_for_segment(data_dir, segment_id)?.ok_or_else(|| {
        TrajectoryError::InvalidInput(format!("segment has no quality record: {segment_id}"))
    })?;
    if !quality.memory_eligible {
        return Err(TrajectoryError::InvalidInput(format!(
            "segment is not memory eligible: {}",
            quality.failure_reason
        )));
    }
    let text = segment_text(&ingest.source_path, &segment)?;
    let now = timestamp_string();
    let candidate_id = format!("traj_candidate_{}_{}", now, next_sequence());
    let canonical_path = promotion_queue_dir(data_dir).join(format!("{candidate_id}.json"));
    let mut support_refs = vec![segment.segment_tag.clone()];
    support_refs.extend(quality.evidence_refs.clone());
    let source_artifact_path = quality
        .evidence_refs
        .first()
        .cloned()
        .unwrap_or_else(|| ingest.source_path.clone());
    let candidate = SegmentMemoryCandidate {
        schema_version: "trajectory.memory_candidate.v1".to_string(),
        conformance_line: "M15.segment_memory_candidate".to_string(),
        canonical_path: canonical_path.display().to_string(),
        retention_policy: "reviewable_until_promoted_or_invalidated".to_string(),
        atomic_write_policy: "atomic_json_write".to_string(),
        candidate_id: candidate_id.clone(),
        segment_id: segment_id.to_string(),
        ingest_id: ingest.ingest_id.clone(),
        project_id: ingest.project_id.clone(),
        title: non_empty_or(&segment.topic, "trajectory segment memory"),
        summary: non_empty_or(&segment.summary, "trajectory segment memory"),
        body: text,
        memory_kind: "trajectory_segment".to_string(),
        support_refs,
        source_artifacts: quality.evidence_refs.clone(),
        source_artifact_path,
        segment_tag: segment.segment_tag.clone(),
        quality_id: quality.quality_id.clone(),
        promotion_status: "queued".to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
        session_id: ingest.source_session_id.clone(),
        summary_ref: non_empty_or(&segment.summary, "trajectory segment memory"),
        status: "pending_review".to_string(),
        confidence: "deterministic_quality_gate".to_string(),
        degraded_reasons: Vec::new(),
        promotes_to_durable_memory: false,
    };
    fs::create_dir_all(promotion_queue_dir(data_dir))?;
    write_json_atomic(&canonical_path, &candidate)?;
    segment.memory_extraction_status = "candidate_created".to_string();
    segment.updated_at = now;
    write_json_atomic(&segment_record_path(data_dir, segment_id), &segment)?;
    append_json_line(&segments_log_path(data_dir), &segment)?;
    Ok(SegmentExtractResult {
        segment,
        quality,
        candidate,
    })
}

pub fn status(data_dir: &Path) -> Result<TrajectoryStatusReport, TrajectoryError> {
    Ok(TrajectoryStatusReport {
        schema_version: "trajectory.status.v1".to_string(),
        conformance_line: "M15.trajectory_status".to_string(),
        ingest_count: read_jsonl_records::<TrajectoryIngestRecord>(&ingests_log_path(data_dir))?
            .len(),
        segment_count: all_segments(data_dir)?.len(),
        quality_count: read_jsonl_records::<SegmentQualityRecord>(&quality_log_path(data_dir))?
            .len(),
        candidate_count: promotion_queue_candidate_count(data_dir)?,
    })
}

pub fn explain_segment(
    data_dir: &Path,
    segment_id: &str,
) -> Result<TaskSegmentRecord, TrajectoryError> {
    read_segment(data_dir, segment_id)
}

pub fn record_adoption(
    data_dir: &Path,
    memory_record_id: &str,
    event_kind: &str,
) -> Result<MemoryAdoptionResult, TrajectoryError> {
    let weight = adoption_weight(event_kind)?;
    let memory_record = durable_memory_value(data_dir, memory_record_id)?;
    validate_adoption_memory_state(&memory_record, weight)?;
    let segment_id = segment_id_for_memory_value(data_dir, &memory_record)?.unwrap_or_default();
    let now = timestamp_string();
    let event = MemoryAdoptionEvent {
        schema_version: "memory.adoption_event.v1".to_string(),
        conformance_line: "M15.memory_adoption".to_string(),
        canonical_path: adoption_events_log_path(data_dir).display().to_string(),
        retention_policy: "append_only_adoption_evidence".to_string(),
        atomic_write_policy: "append_jsonl".to_string(),
        event_id: format!("memory_adoption_{}_{}", now, next_sequence()),
        memory_record_id: memory_record_id.to_string(),
        segment_id,
        event_kind: event_kind.to_string(),
        weight,
        surface: "cli".to_string(),
        actor: "operator".to_string(),
        support_ref: format!("memory adoption record --event {event_kind}"),
        created_at: now,
    };
    fs::create_dir_all(memory_root(data_dir))?;
    append_json_line(&adoption_events_log_path(data_dir), &event)?;
    Ok(MemoryAdoptionResult { event })
}

pub fn rebalance_tiers(data_dir: &Path) -> Result<MemoryTierRebalanceResult, TrajectoryError> {
    let events = read_jsonl_records::<MemoryAdoptionEvent>(&adoption_events_log_path(data_dir))?;
    let mut by_memory = BTreeMap::<String, Vec<MemoryAdoptionEvent>>::new();
    for event in events {
        by_memory
            .entry(event.memory_record_id.clone())
            .or_default()
            .push(event);
    }
    fs::create_dir_all(tiers_dir(data_dir))?;
    let mut explanations = Vec::new();
    for (memory_id, memory_events) in by_memory {
        let explanation = tier_explanation_for(data_dir, &memory_id, &memory_events)?;
        write_json_atomic(&tier_path(data_dir, &memory_id), &explanation)?;
        explanations.push(explanation);
    }
    Ok(MemoryTierRebalanceResult {
        schema_version: "memory.tier_rebalance.v1".to_string(),
        conformance_line: "M15.memory_tier_rebalance".to_string(),
        explanations,
    })
}

pub fn explain_tier(
    data_dir: &Path,
    memory_record_id: &str,
) -> Result<MemoryTierExplainResult, TrajectoryError> {
    let path = tier_path(data_dir, memory_record_id);
    if !path.exists() {
        return Err(TrajectoryError::InvalidInput(format!(
            "unknown memory tier explanation: {memory_record_id}"
        )));
    }
    let explanation: MemoryTierExplanation = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(MemoryTierExplainResult { explanation })
}

fn tier_explanation_for(
    data_dir: &Path,
    memory_id: &str,
    events: &[MemoryAdoptionEvent],
) -> Result<MemoryTierExplanation, TrajectoryError> {
    let mut event_counts = BTreeMap::<String, usize>::new();
    let mut adoption_score = 0.0;
    let mut last_positive_event_at = String::new();
    let mut negative_event_count = 0usize;
    for event in events {
        *event_counts.entry(event.event_kind.clone()).or_insert(0) += 1;
        adoption_score += event.weight;
        if event.weight > 0.0 {
            last_positive_event_at = event.created_at.clone();
        } else {
            negative_event_count += 1;
        }
    }
    let tier = if adoption_score >= 3.0 {
        "HOT"
    } else if adoption_score >= 1.0 {
        "WARM"
    } else {
        "COLD"
    };
    let now = timestamp_string();
    Ok(MemoryTierExplanation {
        schema_version: "memory.tier_explanation.v1".to_string(),
        conformance_line: "M15.memory_tier".to_string(),
        canonical_path: tier_path(data_dir, memory_id).display().to_string(),
        retention_policy: "derived_rebuildable_projection".to_string(),
        atomic_write_policy: "atomic_json_write".to_string(),
        memory_record_id: memory_id.to_string(),
        tier: tier.to_string(),
        adoption_score: round_score(adoption_score),
        event_counts,
        last_positive_event_at,
        negative_event_count,
        ranking_reason: format!("{tier} from adoption_score={}", round_score(adoption_score)),
        governance_limits: vec![
            "tier_does_not_override_invalidated_or_superseded_memory".to_string(),
            "tier_does_not_override_contested_expired_or_non_injectable_memory".to_string(),
            "tier_does_not_override_artifact_family_latest_gates".to_string(),
        ],
        created_at: now.clone(),
        updated_at: now,
    })
}

fn read_session_messages(path: &str) -> Result<Vec<SessionMessage>, TrajectoryError> {
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut messages = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)?;
        if value.get("_type").and_then(Value::as_str) == Some("metadata") {
            continue;
        }
        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if role.is_empty() && content.is_empty() {
            continue;
        }
        messages.push(SessionMessage {
            role,
            content,
            line_number: index + 1,
        });
    }
    Ok(messages)
}

fn segment_drafts(messages: &[SessionMessage]) -> Vec<SegmentDraft> {
    let marker_indices = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| task_marker_topic(&message.content).is_some())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if !marker_indices.is_empty() {
        return marker_indices
            .iter()
            .enumerate()
            .map(|(segment_index, marker_index)| {
                let start = messages[*marker_index].line_number;
                let end = marker_indices
                    .get(segment_index + 1)
                    .map(|next| messages[*next].line_number.saturating_sub(1))
                    .unwrap_or_else(|| messages.last().expect("messages not empty").line_number);
                SegmentDraft {
                    segment_index,
                    start_line: start,
                    end_line: end,
                    topic: task_marker_topic(&messages[*marker_index].content)
                        .unwrap_or_else(|| format!("task {}", segment_index + 1)),
                    method: "manual_fixture".to_string(),
                }
            })
            .collect();
    }
    let method = if messages.len() <= 2 {
        "deterministic_short_chat"
    } else {
        "deterministic_single_task"
    };
    vec![SegmentDraft {
        segment_index: 0,
        start_line: messages.first().expect("messages not empty").line_number,
        end_line: messages.last().expect("messages not empty").line_number,
        topic: "single task".to_string(),
        method: method.to_string(),
    }]
}

fn task_marker_topic(content: &str) -> Option<String> {
    content
        .trim()
        .strip_prefix("# task:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn compute_segment_fingerprint(messages: &[SessionMessage]) -> String {
    let mut hasher = Sha256::new();
    for message in messages {
        hasher.update(message.role.as_bytes());
        hasher.update(b"\x00");
        hasher.update(message.content.as_bytes());
        hasher.update(b"\x01");
    }
    let digest = hasher.finalize();
    let full = format!("{digest:x}");
    full[..16].to_string()
}

fn segment_text(path: &str, segment: &TaskSegmentRecord) -> Result<String, TrajectoryError> {
    let messages = read_session_messages(path)?;
    let body = messages
        .into_iter()
        .filter(|message| {
            message.line_number >= segment.start_line && message.line_number <= segment.end_line
        })
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(body)
}

fn extract_support_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for raw in text.split_whitespace() {
        let trimmed = raw
            .trim_matches(|ch: char| matches!(ch, ',' | '.' | ';' | ':' | ')' | '(' | '"' | '\''));
        if trimmed.starts_with("docs/")
            || trimmed.starts_with("src/")
            || trimmed.starts_with("schemas/")
            || trimmed.starts_with("tests/")
        {
            let value = trimmed.to_string();
            if !refs.contains(&value) {
                refs.push(value);
            }
        }
    }
    refs
}

fn contains_secret_like_text(text: &str) -> bool {
    text.contains("sk-")
        || text.contains("TOKEN=")
        || text.contains("API_KEY=")
        || text.contains("BEGIN PRIVATE KEY")
}

fn find_reusable_segment(
    data_dir: &Path,
    source_path: &str,
    segment_index: usize,
    fingerprint: &str,
) -> Result<Option<TaskSegmentRecord>, TrajectoryError> {
    Ok(
        active_segments_for_source_index(data_dir, source_path, segment_index)?
            .into_iter()
            .find(|segment| segment.fingerprint == fingerprint),
    )
}

fn active_segments_for_source_index(
    data_dir: &Path,
    source_path: &str,
    segment_index: usize,
) -> Result<Vec<TaskSegmentRecord>, TrajectoryError> {
    let mut matches = Vec::new();
    for segment in all_segments(data_dir)? {
        if segment.status != "active" || segment.segment_index != segment_index {
            continue;
        }
        let ingest = read_ingest(data_dir, &segment.ingest_id)?;
        if ingest.source_path == source_path {
            matches.push(segment);
        }
    }
    Ok(matches)
}

fn supersede_segment(
    data_dir: &Path,
    mut old: TaskSegmentRecord,
    new_segment_id: &str,
) -> Result<(), TrajectoryError> {
    old.status = "superseded".to_string();
    old.memory_extraction_status = if old.memory_extraction_status == "candidate_created" {
        "invalidated".to_string()
    } else {
        old.memory_extraction_status.clone()
    };
    old.superseded_by = new_segment_id.to_string();
    old.updated_at = timestamp_string();
    write_json_atomic(&segment_record_path(data_dir, &old.segment_id), &old)?;
    append_json_line(&segments_log_path(data_dir), &old)?;
    invalidate_candidates_by_segment_tag(data_dir, &old.segment_tag)?;
    invalidate_promoted_memories_by_segment_tag(
        data_dir,
        &old.segment_tag,
        &format!("trajectory_segment_superseded:{new_segment_id}"),
    )?;
    Ok(())
}

fn invalidate_candidates_by_segment_tag(
    data_dir: &Path,
    segment_tag: &str,
) -> Result<(), TrajectoryError> {
    let dir = promotion_queue_dir(data_dir);
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        if value.get("segment_tag").and_then(Value::as_str) != Some(segment_tag) {
            continue;
        }
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "promotion_status".to_string(),
                Value::String("invalidated".to_string()),
            );
            object.insert(
                "status".to_string(),
                Value::String("invalidated".to_string()),
            );
            object.insert("updated_at".to_string(), Value::String(timestamp_string()));
        }
        write_json_atomic(&path, &value)?;
    }
    Ok(())
}

fn invalidate_promoted_memories_by_segment_tag(
    data_dir: &Path,
    segment_tag: &str,
    reason: &str,
) -> Result<(), TrajectoryError> {
    let path = memory_root(data_dir).join("durable").join("records.jsonl");
    if !path.exists() {
        return Ok(());
    }
    let records = read_jsonl_values(&path)?;
    let mut ids = Vec::new();
    for record in records {
        let status = record
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            status,
            "invalidated" | "superseded" | "contested" | "expired"
        ) {
            continue;
        }
        let has_segment_ref = record
            .get("support_refs")
            .and_then(Value::as_array)
            .map(|refs| {
                refs.iter()
                    .filter_map(Value::as_str)
                    .any(|support_ref| support_ref == segment_tag)
            })
            .unwrap_or(false);
        if has_segment_ref {
            if let Some(record_id) = record.get("record_id").and_then(Value::as_str) {
                if !ids.iter().any(|existing| existing == record_id) {
                    ids.push(record_id.to_string());
                }
            }
        }
    }
    for record_id in ids {
        memory::invalidate(data_dir, &record_id, "invalidated", reason)
            .map_err(|err| TrajectoryError::Memory(err.to_string()))?;
    }
    Ok(())
}

fn latest_quality_for_segment(
    data_dir: &Path,
    segment_id: &str,
) -> Result<Option<SegmentQualityRecord>, TrajectoryError> {
    let mut matches = read_jsonl_records::<SegmentQualityRecord>(&quality_log_path(data_dir))?
        .into_iter()
        .filter(|quality| quality.segment_id == segment_id)
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(matches.pop())
}

fn durable_memory_value(data_dir: &Path, memory_record_id: &str) -> Result<Value, TrajectoryError> {
    let path = memory_root(data_dir)
        .join("durable")
        .join("records")
        .join(format!("{memory_record_id}.json"));
    if !path.exists() {
        return Err(TrajectoryError::InvalidInput(format!(
            "unknown durable memory record: {memory_record_id}"
        )));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn validate_adoption_memory_state(record: &Value, weight: f64) -> Result<(), TrajectoryError> {
    if weight <= 0.0 {
        return Ok(());
    }
    let status = record
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let injectable = record
        .get("injectable")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if matches!(
        status,
        "invalidated" | "superseded" | "contested" | "expired" | "rejected"
    ) || !injectable
    {
        return Err(TrajectoryError::InvalidInput(format!(
            "positive adoption requires active injectable memory, found status={status}"
        )));
    }
    Ok(())
}

fn segment_id_for_memory_value(
    data_dir: &Path,
    value: &Value,
) -> Result<Option<String>, TrajectoryError> {
    let Some(refs) = value.get("support_refs").and_then(Value::as_array) else {
        return Ok(None);
    };
    let Some(segment_tag) = refs
        .iter()
        .filter_map(Value::as_str)
        .find(|support_ref| support_ref.starts_with("segment:"))
    else {
        return Ok(None);
    };
    for segment in all_segments(data_dir)? {
        if segment.segment_tag == segment_tag {
            return Ok(Some(segment.segment_id));
        }
    }
    Ok(segment_tag.strip_prefix("segment:").map(ToOwned::to_owned))
}

fn all_segments(data_dir: &Path) -> Result<Vec<TaskSegmentRecord>, TrajectoryError> {
    let mut by_id = BTreeMap::<String, TaskSegmentRecord>::new();
    for segment in read_jsonl_records::<TaskSegmentRecord>(&segments_log_path(data_dir))? {
        by_id.insert(segment.segment_id.clone(), segment);
    }
    Ok(by_id.into_values().collect())
}

fn read_ingest(
    data_dir: &Path,
    ingest_id: &str,
) -> Result<TrajectoryIngestRecord, TrajectoryError> {
    let path = ingest_record_path(data_dir, ingest_id);
    if !path.exists() {
        return Err(TrajectoryError::InvalidInput(format!(
            "unknown trajectory ingest: {ingest_id}"
        )));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn read_segment(data_dir: &Path, segment_id: &str) -> Result<TaskSegmentRecord, TrajectoryError> {
    let path = segment_record_path(data_dir, segment_id);
    if !path.exists() {
        return Err(TrajectoryError::InvalidInput(format!(
            "unknown trajectory segment: {segment_id}"
        )));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn read_jsonl_records<T>(path: &Path) -> Result<Vec<T>, TrajectoryError>
where
    T: for<'de> Deserialize<'de>,
{
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

fn read_jsonl_values(path: &Path) -> Result<Vec<Value>, TrajectoryError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut values = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        values.push(serde_json::from_str(&line)?);
    }
    Ok(values)
}

fn validate_source_kind(source_kind: &str) -> Result<(), TrajectoryError> {
    if matches!(
        source_kind,
        "cli_session"
            | "tui_session"
            | "remote_session"
            | "agent_runtime"
            | "branch_run"
            | "research_stage"
    ) {
        Ok(())
    } else {
        Err(TrajectoryError::InvalidInput(format!(
            "unsupported trajectory source kind: {source_kind}"
        )))
    }
}

fn adoption_weight(event_kind: &str) -> Result<f64, TrajectoryError> {
    match event_kind {
        "retrieved" => Ok(1.0),
        "inspected" => Ok(2.0),
        "injected" => Ok(3.0),
        "cited_in_output" => Ok(3.0),
        "merged" => Ok(1.0),
        "superseded" | "invalidated" | "rejected" => Ok(-3.0),
        other => Err(TrajectoryError::InvalidInput(format!(
            "unsupported adoption event kind: {other}"
        ))),
    }
}

fn digest_file(path: &Path) -> Result<String, TrajectoryError> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn workspace_relative_existing_file(
    workspace_root: &Path,
    source_path: &str,
) -> Result<PathBuf, TrajectoryError> {
    let relative_path = Path::new(source_path);
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(TrajectoryError::InvalidInput(format!(
            "trajectory source path must be workspace-relative and stay inside workspace: {source_path}"
        )));
    }
    let workspace_root = workspace_root.canonicalize()?;
    let canonical_source = workspace_root.join(relative_path);
    if !canonical_source.is_file() {
        return Err(TrajectoryError::InvalidInput(format!(
            "trajectory source does not exist: {source_path}"
        )));
    }
    let canonical_source = canonical_source.canonicalize()?;
    if !canonical_source.starts_with(&workspace_root) {
        return Err(TrajectoryError::InvalidInput(format!(
            "trajectory source path must stay inside workspace: {source_path}"
        )));
    }
    Ok(canonical_source)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), TrajectoryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<(), TrajectoryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _guard = lock_event_log(path)
        .map_err(|err| TrajectoryError::Io(std::io::Error::other(err.to_string())))?;
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&line)?;
    Ok(())
}

fn promotion_queue_candidate_count(data_dir: &Path) -> Result<usize, TrajectoryError> {
    let dir = promotion_queue_dir(data_dir);
    if !dir.exists() {
        return Ok(0);
    }
    Ok(fs::read_dir(dir)?.filter_map(Result::ok).count())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn next_sequence() -> u64 {
    TRAJECTORY_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn clamp_score(value: f64) -> f64 {
    round_score(value.clamp(0.0, 1.0))
}

fn round_score(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn trajectory_root(data_dir: &Path) -> PathBuf {
    data_dir.join("trajectory")
}

fn ingests_dir(data_dir: &Path) -> PathBuf {
    trajectory_root(data_dir).join("ingests")
}

fn ingests_log_path(data_dir: &Path) -> PathBuf {
    trajectory_root(data_dir).join("ingests.jsonl")
}

fn ingest_record_path(data_dir: &Path, ingest_id: &str) -> PathBuf {
    ingests_dir(data_dir).join(format!("{ingest_id}.json"))
}

fn segments_dir(data_dir: &Path) -> PathBuf {
    trajectory_root(data_dir).join("segments")
}

fn segments_log_path(data_dir: &Path) -> PathBuf {
    trajectory_root(data_dir).join("segments.jsonl")
}

fn segment_record_path(data_dir: &Path, segment_id: &str) -> PathBuf {
    segments_dir(data_dir).join(format!("{segment_id}.json"))
}

fn quality_dir(data_dir: &Path) -> PathBuf {
    trajectory_root(data_dir).join("quality")
}

fn quality_log_path(data_dir: &Path) -> PathBuf {
    quality_dir(data_dir).join("quality.jsonl")
}

fn quality_record_path(data_dir: &Path, quality_id: &str) -> PathBuf {
    quality_dir(data_dir).join(format!("{quality_id}.json"))
}

fn memory_root(data_dir: &Path) -> PathBuf {
    data_dir.join("memory")
}

fn promotion_queue_dir(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("promotion_queue")
}

fn adoption_events_log_path(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("adoption_events.jsonl")
}

fn tiers_dir(data_dir: &Path) -> PathBuf {
    memory_root(data_dir).join("tiers")
}

fn tier_path(data_dir: &Path, memory_id: &str) -> PathBuf {
    tiers_dir(data_dir).join(format!("{memory_id}.json"))
}

#[derive(Debug)]
pub enum TrajectoryError {
    InvalidInput(String),
    Memory(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for TrajectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "{message}"),
            Self::Memory(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TrajectoryError {}

impl From<std::io::Error> for TrajectoryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for TrajectoryError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
