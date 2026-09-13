use crate::evolution::SkillProvenanceVerification;
use crate::trajectory::MemoryAdoptionEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FEEDBACK_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFeedbackSignal {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub signal_id: String,
    pub signal_kind: String,
    pub source_id: String,
    pub target_lane: String,
    pub polarity: String,
    pub weight: f64,
    pub authority_boundary: String,
    pub evidence_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRoutingAdjustment {
    pub target_lane: String,
    pub adjustment_kind: String,
    pub weight_delta: f64,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedbackCalibrationRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub calibration_id: String,
    pub authority_boundary: String,
    pub signal_count: usize,
    pub positive_signal_count: usize,
    pub negative_signal_count: usize,
    pub routing_adjustments: Vec<MemoryRoutingAdjustment>,
    pub skill_recommendation_hints: Vec<String>,
    pub verifier_calibration_hints: Vec<String>,
    pub governance_limits: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedbackCollectResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub signals: Vec<AgentFeedbackSignal>,
    pub total_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedbackCalibrationResult {
    pub calibration: FeedbackCalibrationRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedbackStatusResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub latest_calibration: FeedbackCalibrationRecord,
}

pub fn collect(data_dir: &Path) -> Result<FeedbackCollectResult, FeedbackError> {
    let mut signals = Vec::new();
    for event in read_jsonl_records::<MemoryAdoptionEvent>(&adoption_events_log_path(data_dir))? {
        signals.push(signal_from_adoption(data_dir, &event)?);
    }
    for verification in
        read_jsonl_records::<SkillProvenanceVerification>(&verifications_log_path(data_dir))?
    {
        signals.push(signal_from_skill_verification(data_dir, &verification)?);
    }
    for signal in &signals {
        write_json_atomic(&signal_path(data_dir, &signal.signal_id), signal)?;
        append_json_line(&signals_log_path(data_dir), signal)?;
    }
    Ok(FeedbackCollectResult {
        schema_version: "feedback.collect_result.v1".to_string(),
        conformance_line: "M18.feedback_collect".to_string(),
        total_count: signals.len(),
        signals,
    })
}

pub fn calibrate(data_dir: &Path) -> Result<FeedbackCalibrationResult, FeedbackError> {
    collect(data_dir)?;
    let mut latest_by_source = BTreeMap::<String, AgentFeedbackSignal>::new();
    for signal in read_jsonl_records::<AgentFeedbackSignal>(&signals_log_path(data_dir))? {
        latest_by_source.insert(
            format!("{}:{}", signal.signal_kind, signal.source_id),
            signal,
        );
    }
    let signals = latest_by_source.into_values().collect::<Vec<_>>();
    let positive_signal_count = signals
        .iter()
        .filter(|signal| signal.polarity == "positive")
        .count();
    let negative_signal_count = signals
        .iter()
        .filter(|signal| signal.polarity == "negative")
        .count();
    let mut routing_adjustments = Vec::new();
    let mut skill_recommendation_hints = Vec::new();
    let mut verifier_calibration_hints = Vec::new();
    if signals
        .iter()
        .any(|signal| signal.signal_kind == "memory_adoption")
    {
        routing_adjustments.push(MemoryRoutingAdjustment {
            target_lane: "memory_retrieval".to_string(),
            adjustment_kind: "boost_positive_adoption_explainably".to_string(),
            weight_delta: 0.1,
            reason: "memory adoption signals indicate useful retrieval, but remain advisory"
                .to_string(),
            evidence_refs: evidence_for_kind(&signals, "memory_adoption"),
        });
    }
    if signals
        .iter()
        .any(|signal| signal.signal_kind == "skill_verification")
    {
        routing_adjustments.push(MemoryRoutingAdjustment {
            target_lane: "skill_recommendation".to_string(),
            adjustment_kind: "prefer_verified_evolution_candidates".to_string(),
            weight_delta: 0.1,
            reason: "verified skill candidates can be recommended while publication remains gated"
                .to_string(),
            evidence_refs: evidence_for_kind(&signals, "skill_verification"),
        });
        skill_recommendation_hints
            .push("recommend verified evolved skills before unverified candidates".to_string());
    }
    if signals
        .iter()
        .any(|signal| signal.target_lane == "verifier")
    {
        verifier_calibration_hints.push(
            "calibrate verifier thresholds only from explicit tournament outcomes".to_string(),
        );
    }
    let now = timestamp_string();
    let calibration_id = format!("feedback_calibration_{}_{}", now, next_sequence());
    let path = latest_calibration_path(data_dir);
    let calibration = FeedbackCalibrationRecord {
        schema_version: "feedback.calibration_record.v1".to_string(),
        conformance_line: "M18.feedback_calibration".to_string(),
        canonical_path: path.display().to_string(),
        retention_policy: "project_local_until_next_calibration".to_string(),
        atomic_write_policy: "atomic_latest_projection_plus_append_log".to_string(),
        calibration_id,
        authority_boundary: "advisory_only".to_string(),
        signal_count: signals.len(),
        positive_signal_count,
        negative_signal_count,
        routing_adjustments,
        skill_recommendation_hints,
        verifier_calibration_hints,
        governance_limits: vec![
            "feedback_cannot_publish_project_truth".to_string(),
            "feedback_cannot_merge_or_promote_branches".to_string(),
            "feedback_cannot_bypass_skill_publication_gates".to_string(),
            "feedback_cannot_widen_tool_or_network_authority".to_string(),
        ],
        created_at: now,
    };
    write_json_atomic(&path, &calibration)?;
    append_json_line(&calibrations_log_path(data_dir), &calibration)?;
    Ok(FeedbackCalibrationResult { calibration })
}

pub fn status(data_dir: &Path) -> Result<FeedbackStatusResult, FeedbackError> {
    let path = latest_calibration_path(data_dir);
    let latest_calibration = if path.exists() {
        serde_json::from_str(&fs::read_to_string(path)?)?
    } else {
        empty_calibration(data_dir)
    };
    Ok(FeedbackStatusResult {
        schema_version: "feedback.status_result.v1".to_string(),
        conformance_line: "M18.feedback_status".to_string(),
        latest_calibration,
    })
}

fn signal_from_adoption(
    data_dir: &Path,
    event: &MemoryAdoptionEvent,
) -> Result<AgentFeedbackSignal, FeedbackError> {
    let now = timestamp_string();
    let signal_id = format!("feedback_signal_{}_{}", now, next_sequence());
    Ok(AgentFeedbackSignal {
        schema_version: "feedback.agent_signal.v1".to_string(),
        conformance_line: "M18.feedback_signal".to_string(),
        canonical_path: signal_path(data_dir, &signal_id).display().to_string(),
        retention_policy: "append_only_feedback_evidence".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        signal_id,
        signal_kind: "memory_adoption".to_string(),
        source_id: event.event_id.clone(),
        target_lane: "memory_retrieval".to_string(),
        polarity: if event.weight >= 0.0 {
            "positive".to_string()
        } else {
            "negative".to_string()
        },
        weight: event.weight,
        authority_boundary: "advisory_only".to_string(),
        evidence_refs: vec![event.support_ref.clone(), event.memory_record_id.clone()],
        created_at: now,
    })
}

fn signal_from_skill_verification(
    data_dir: &Path,
    verification: &SkillProvenanceVerification,
) -> Result<AgentFeedbackSignal, FeedbackError> {
    let now = timestamp_string();
    let signal_id = format!("feedback_signal_{}_{}", now, next_sequence());
    Ok(AgentFeedbackSignal {
        schema_version: "feedback.agent_signal.v1".to_string(),
        conformance_line: "M18.feedback_signal".to_string(),
        canonical_path: signal_path(data_dir, &signal_id).display().to_string(),
        retention_policy: "append_only_feedback_evidence".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        signal_id,
        signal_kind: "skill_verification".to_string(),
        source_id: verification.verification_id.clone(),
        target_lane: "skill_recommendation".to_string(),
        polarity: if verification.decision == "verified" {
            "positive".to_string()
        } else {
            "negative".to_string()
        },
        weight: verification.grounded_evidence_ratio,
        authority_boundary: "advisory_only".to_string(),
        evidence_refs: verification.evidence_refs.clone(),
        created_at: now,
    })
}

fn evidence_for_kind(signals: &[AgentFeedbackSignal], signal_kind: &str) -> Vec<String> {
    signals
        .iter()
        .filter(|signal| signal.signal_kind == signal_kind)
        .flat_map(|signal| signal.evidence_refs.clone())
        .collect::<Vec<_>>()
}

fn empty_calibration(data_dir: &Path) -> FeedbackCalibrationRecord {
    FeedbackCalibrationRecord {
        schema_version: "feedback.calibration_record.v1".to_string(),
        conformance_line: "M18.feedback_calibration".to_string(),
        canonical_path: latest_calibration_path(data_dir).display().to_string(),
        retention_policy: "project_local_until_next_calibration".to_string(),
        atomic_write_policy: "atomic_latest_projection_plus_append_log".to_string(),
        calibration_id: "feedback_calibration_empty".to_string(),
        authority_boundary: "advisory_only".to_string(),
        signal_count: 0,
        positive_signal_count: 0,
        negative_signal_count: 0,
        routing_adjustments: Vec::new(),
        skill_recommendation_hints: Vec::new(),
        verifier_calibration_hints: Vec::new(),
        governance_limits: vec![
            "feedback_cannot_publish_project_truth".to_string(),
            "feedback_cannot_merge_or_promote_branches".to_string(),
            "feedback_cannot_bypass_skill_publication_gates".to_string(),
            "feedback_cannot_widen_tool_or_network_authority".to_string(),
        ],
        created_at: timestamp_string(),
    }
}

fn feedback_root(data_dir: &Path) -> PathBuf {
    data_dir.join("feedback")
}

fn signals_dir(data_dir: &Path) -> PathBuf {
    feedback_root(data_dir).join("signals")
}

fn signal_path(data_dir: &Path, signal_id: &str) -> PathBuf {
    signals_dir(data_dir).join(format!("{signal_id}.json"))
}

fn signals_log_path(data_dir: &Path) -> PathBuf {
    feedback_root(data_dir).join("signals.jsonl")
}

fn calibrations_log_path(data_dir: &Path) -> PathBuf {
    feedback_root(data_dir).join("calibrations.jsonl")
}

fn latest_calibration_path(data_dir: &Path) -> PathBuf {
    feedback_root(data_dir)
        .join("calibration")
        .join("latest.json")
}

fn adoption_events_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("memory").join("adoption_events.jsonl")
}

fn verifications_log_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("skills")
        .join("evolution")
        .join("verifications.jsonl")
}

fn read_jsonl_records<T>(path: &Path) -> Result<Vec<T>, FeedbackError>
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

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), FeedbackError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<(), FeedbackError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&line)?;
    Ok(())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn next_sequence() -> u64 {
    FEEDBACK_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub enum FeedbackError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for FeedbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "{err}"),
            Self::Json(err) => write!(formatter, "{err}"),
        }
    }
}

impl std::error::Error for FeedbackError {}

impl From<std::io::Error> for FeedbackError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for FeedbackError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
