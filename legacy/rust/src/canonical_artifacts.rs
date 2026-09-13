use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LEDGER_SCHEMA_VERSION: &str = "canonical_artifact_ledger.v1";
pub const EVENT_SCHEMA_VERSION: &str = "canonical_artifact_event.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalArtifactStatus {
    CandidateOnly,
    AcceptedEvidence,
    AdoptionRequested,
    MaterializationBlocked,
    Materialized,
    BaselinePromotionBlocked,
    BaselineVisible,
    IntegrationFailed,
    IntegrationVerified,
    ActiveStageEvidence,
    RetiredOrSuperseded,
}

impl CanonicalArtifactStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CandidateOnly => "candidate_only",
            Self::AcceptedEvidence => "accepted_evidence",
            Self::AdoptionRequested => "adoption_requested",
            Self::MaterializationBlocked => "materialization_blocked",
            Self::Materialized => "materialized",
            Self::BaselinePromotionBlocked => "baseline_promotion_blocked",
            Self::BaselineVisible => "baseline_visible",
            Self::IntegrationFailed => "integration_failed",
            Self::IntegrationVerified => "integration_verified",
            Self::ActiveStageEvidence => "active_stage_evidence",
            Self::RetiredOrSuperseded => "retired_or_superseded",
        }
    }

    fn can_transition_to(&self, next: &Self) -> bool {
        if matches!(next, Self::RetiredOrSuperseded) {
            return true;
        }
        matches!(
            (self, next),
            (Self::CandidateOnly, Self::AcceptedEvidence)
                | (Self::AcceptedEvidence, Self::AdoptionRequested)
                | (Self::AdoptionRequested, Self::MaterializationBlocked)
                | (Self::AdoptionRequested, Self::Materialized)
                | (Self::Materialized, Self::BaselinePromotionBlocked)
                | (Self::Materialized, Self::BaselineVisible)
                | (Self::BaselineVisible, Self::IntegrationFailed)
                | (Self::BaselineVisible, Self::IntegrationVerified)
                | (Self::IntegrationFailed, Self::IntegrationVerified)
                | (Self::IntegrationVerified, Self::ActiveStageEvidence)
        )
    }

    pub fn satisfies_file_dependency(&self) -> bool {
        matches!(
            self,
            Self::BaselineVisible | Self::IntegrationVerified | Self::ActiveStageEvidence
        )
    }

    pub fn satisfies_runnable_dependency(&self) -> bool {
        matches!(self, Self::IntegrationVerified | Self::ActiveStageEvidence)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactLedger {
    pub schema_version: String,
    pub entries: Vec<CanonicalArtifactEntry>,
    pub updated_at: String,
}

impl Default for CanonicalArtifactLedger {
    fn default() -> Self {
        Self {
            schema_version: LEDGER_SCHEMA_VERSION.to_string(),
            entries: Vec::new(),
            updated_at: timestamp_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactEntry {
    pub artifact_id: String,
    pub job_id: String,
    pub stage_id: String,
    pub stage_execution_id: String,
    pub source_agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_task_id: Option<String>,
    pub source_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_artifact_path: Option<String>,
    pub target_artifact_path: String,
    pub artifact_kind: String,
    pub task_type: String,
    pub decision_ref: String,
    pub status: CanonicalArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialization_blocker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_visible_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_blocker: Option<String>,
    #[serde(default)]
    pub integration_checks: Vec<CanonicalArtifactIntegrationCheck>,
    #[serde(default)]
    pub consumed_by_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_by_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactIntegrationCheck {
    pub check_id: String,
    pub status: String,
    pub command: String,
    pub detail: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactEvent {
    pub schema_version: String,
    pub event_id: String,
    pub artifact_id: String,
    pub previous_status: Option<CanonicalArtifactStatus>,
    pub next_status: CanonicalArtifactStatus,
    pub authority_boundary: String,
    pub detail: String,
    pub created_at: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactBaseline {
    pub schema_version: String,
    pub baseline_ref: String,
    pub baseline_mode: String,
    pub overlay_ref: String,
    pub ledger_ref: String,
    pub artifact_ids: Vec<String>,
    pub entries: Vec<CanonicalArtifactBaselineEntry>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactBaselineEntry {
    pub artifact_id: String,
    pub target_artifact_path: String,
    pub overlay_artifact_path: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct CanonicalArtifactSeed {
    pub job_id: String,
    pub stage_id: String,
    pub stage_execution_id: String,
    pub source_agent_id: String,
    pub source_task_id: Option<String>,
    pub source_ref: String,
    pub source_artifact_path: Option<String>,
    pub target_artifact_path: String,
    pub artifact_kind: String,
    pub task_type: String,
    pub decision_ref: String,
    pub source_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredCanonicalArtifact {
    pub target_artifact_path: String,
    pub required_status: String,
    pub dependency_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalArtifactDependencyBlocker {
    pub artifact_id: String,
    pub target_artifact_path: String,
    pub requested_ref: String,
    pub current_status: String,
    pub required_status: String,
    pub dependency_kind: String,
    pub source_agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_task_id: Option<String>,
    pub decision_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_ref: Option<String>,
}

impl CanonicalArtifactDependencyBlocker {
    pub fn render_for_prompt(&self) -> String {
        let source_task = self
            .source_task_id
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let baseline_ref = self
            .baseline_ref
            .clone()
            .unwrap_or_else(|| "none".to_string());
        format!(
            "canonical artifact `{}` is `{}` but `{}` dependency requires `{}`; artifact_id=`{}` requested_ref=`{}` source_agent=`{}` source_task=`{}` decision_ref=`{}` baseline_ref=`{}`",
            self.target_artifact_path,
            self.current_status,
            self.dependency_kind,
            self.required_status,
            self.artifact_id,
            self.requested_ref,
            self.source_agent_id,
            source_task,
            self.decision_ref,
            baseline_ref
        )
    }
}

#[derive(Debug)]
pub enum CanonicalArtifactError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    UnknownArtifact(String),
    UnsafePath(String),
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    InvalidTransition {
        artifact_id: String,
        from: CanonicalArtifactStatus,
        to: CanonicalArtifactStatus,
    },
}

impl std::fmt::Display for CanonicalArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "canonical artifact IO failed: {err}"),
            Self::Serde(err) => write!(f, "canonical artifact serialization failed: {err}"),
            Self::UnknownArtifact(artifact_id) => {
                write!(f, "unknown canonical artifact: {artifact_id}")
            }
            Self::UnsafePath(path) => write!(f, "unsafe canonical artifact path: {path}"),
            Self::ChecksumMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "canonical artifact checksum mismatch for {path}: expected {expected}, got {actual}"
            ),
            Self::InvalidTransition {
                artifact_id,
                from,
                to,
            } => write!(
                f,
                "invalid canonical artifact transition for {artifact_id}: {} -> {}",
                from.as_str(),
                to.as_str()
            ),
        }
    }
}

impl std::error::Error for CanonicalArtifactError {}

impl From<std::io::Error> for CanonicalArtifactError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CanonicalArtifactError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join("canonical-artifacts").join("ledger.json")
}

pub fn events_path(data_dir: &Path) -> PathBuf {
    data_dir.join("canonical-artifacts").join("events.jsonl")
}

pub fn current_baseline_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("canonical-artifacts")
        .join("current-baseline.json")
}

pub fn overlays_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("canonical-artifacts").join("overlays")
}

pub fn load_ledger(data_dir: &Path) -> Result<CanonicalArtifactLedger, CanonicalArtifactError> {
    let path = ledger_path(data_dir);
    if !path.exists() {
        return Ok(CanonicalArtifactLedger::default());
    }
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn write_ledger(
    data_dir: &Path,
    ledger: &CanonicalArtifactLedger,
) -> Result<(), CanonicalArtifactError> {
    write_text_atomic(
        &ledger_path(data_dir),
        &serde_json::to_string_pretty(ledger)?,
    )
}

pub fn upsert_adoption_requested(
    data_dir: &Path,
    seed: CanonicalArtifactSeed,
    detail: &str,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    let mut ledger = load_ledger(data_dir)?;
    let artifact_id = artifact_id_for_seed(&seed);
    let now = timestamp_string();
    let mut previous_status = None;
    let entry = if let Some(existing) = ledger
        .entries
        .iter_mut()
        .find(|entry| entry.artifact_id == artifact_id)
    {
        previous_status = Some(existing.status.clone());
        if existing.status != CanonicalArtifactStatus::AdoptionRequested {
            transition_entry(existing, CanonicalArtifactStatus::AdoptionRequested)?;
        }
        existing.source_sha256 = seed.source_sha256.clone();
        existing.updated_at = now.clone();
        existing.clone()
    } else {
        let entry = CanonicalArtifactEntry {
            artifact_id: artifact_id.clone(),
            job_id: seed.job_id,
            stage_id: seed.stage_id,
            stage_execution_id: seed.stage_execution_id,
            source_agent_id: seed.source_agent_id,
            source_task_id: seed.source_task_id,
            source_ref: seed.source_ref,
            source_artifact_path: seed.source_artifact_path,
            target_artifact_path: seed.target_artifact_path,
            artifact_kind: seed.artifact_kind,
            task_type: seed.task_type,
            decision_ref: seed.decision_ref,
            status: CanonicalArtifactStatus::AdoptionRequested,
            source_sha256: seed.source_sha256,
            target_sha256: None,
            materialized_at: None,
            materialization_blocker: None,
            baseline_ref: None,
            baseline_visible_at: None,
            baseline_blocker: None,
            integration_checks: Vec::new(),
            consumed_by_task_ids: Vec::new(),
            retired_by_ref: None,
            cleanup_ref: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        ledger.entries.push(entry.clone());
        entry
    };
    ledger.updated_at = now;
    write_ledger(data_dir, &ledger)?;
    append_event(
        data_dir,
        CanonicalArtifactEvent {
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
            event_id: format!("canonical_artifact_event_{}", timestamp_string()),
            artifact_id: artifact_id.clone(),
            previous_status,
            next_status: CanonicalArtifactStatus::AdoptionRequested,
            authority_boundary: "main_agent_decision_recorded_runtime_projection".to_string(),
            detail: detail.to_string(),
            created_at: timestamp_string(),
            metadata: BTreeMap::new(),
        },
    )?;
    Ok(entry)
}

pub fn record_adoption_snapshot_rebound(
    data_dir: &Path,
    artifact_id: &str,
    adoption_ref: &str,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    let mut ledger = load_ledger(data_dir)?;
    let entry = ledger
        .entries
        .iter_mut()
        .find(|entry| entry.artifact_id == artifact_id)
        .ok_or_else(|| CanonicalArtifactError::UnknownArtifact(artifact_id.to_string()))?;
    if !entry.status.satisfies_file_dependency() {
        return Err(CanonicalArtifactError::InvalidTransition {
            artifact_id: artifact_id.to_string(),
            from: entry.status.clone(),
            to: CanonicalArtifactStatus::BaselineVisible,
        });
    }
    let status = entry.status.clone();
    entry.updated_at = timestamp_string();
    let updated = entry.clone();
    ledger.updated_at = timestamp_string();
    write_ledger(data_dir, &ledger)?;
    append_event(
        data_dir,
        CanonicalArtifactEvent {
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
            event_id: format!("canonical_artifact_event_{}", timestamp_string()),
            artifact_id: artifact_id.to_string(),
            previous_status: Some(status.clone()),
            next_status: status,
            authority_boundary: "main_agent_snapshot_rebind_runtime_projection".to_string(),
            detail: "main agent rebound the visible canonical artifact to a fresh accepted evidence snapshot without changing artifact content".to_string(),
            created_at: timestamp_string(),
            metadata: BTreeMap::from([("adoption_ref".to_string(), adoption_ref.to_string())]),
        },
    )?;
    Ok(updated)
}

pub fn record_materialization_blocked(
    data_dir: &Path,
    artifact_id: &str,
    reason: &str,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::MaterializationBlocked,
        |entry| {
            entry.materialization_blocker = Some(reason.to_string());
        },
    )
}

pub fn record_materialized(
    data_dir: &Path,
    artifact_id: &str,
    target_sha256: String,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::Materialized,
        |entry| {
            entry.target_sha256 = Some(target_sha256.clone());
            entry.materialized_at = Some(timestamp_string());
            entry.materialization_blocker = None;
        },
    )
}

pub fn record_baseline_promotion_blocked(
    data_dir: &Path,
    artifact_id: &str,
    reason: &str,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::BaselinePromotionBlocked,
        |entry| {
            entry.baseline_blocker = Some(reason.to_string());
        },
    )
}

pub fn record_baseline_visible(
    data_dir: &Path,
    artifact_id: &str,
    baseline_ref: String,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::BaselineVisible,
        |entry| {
            entry.baseline_ref = Some(baseline_ref.clone());
            entry.baseline_visible_at = Some(timestamp_string());
            entry.baseline_blocker = None;
        },
    )
}

pub fn record_integration_failed(
    data_dir: &Path,
    artifact_id: &str,
    check_id: String,
    command: String,
    detail: String,
    evidence_refs: Vec<String>,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::IntegrationFailed,
        |entry| {
            entry
                .integration_checks
                .push(CanonicalArtifactIntegrationCheck {
                    check_id: check_id.clone(),
                    status: "failed".to_string(),
                    command: command.clone(),
                    detail: detail.clone(),
                    evidence_refs: evidence_refs.clone(),
                    checked_at: timestamp_string(),
                });
        },
    )
}

pub fn record_integration_verified(
    data_dir: &Path,
    artifact_id: &str,
    check_id: String,
    command: String,
    detail: String,
    evidence_refs: Vec<String>,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::IntegrationVerified,
        |entry| {
            entry
                .integration_checks
                .push(CanonicalArtifactIntegrationCheck {
                    check_id: check_id.clone(),
                    status: "passed".to_string(),
                    command: command.clone(),
                    detail: detail.clone(),
                    evidence_refs: evidence_refs.clone(),
                    checked_at: timestamp_string(),
                });
        },
    )
}

pub fn record_active_stage_evidence(
    data_dir: &Path,
    artifact_id: &str,
    stage_gate_ref: String,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::ActiveStageEvidence,
        |entry| {
            push_unique_string(&mut entry.consumed_by_task_ids, stage_gate_ref.clone());
        },
    )
}

pub fn promote_materialized_artifact_to_overlay_baseline(
    data_dir: &Path,
    workspace_root: &Path,
    artifact_id: &str,
) -> Result<CanonicalArtifactBaseline, CanonicalArtifactError> {
    let mut ledger = load_ledger(data_dir)?;
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.artifact_id == artifact_id)
        .cloned()
        .ok_or_else(|| CanonicalArtifactError::UnknownArtifact(artifact_id.to_string()))?;
    if entry.status != CanonicalArtifactStatus::Materialized {
        return Err(CanonicalArtifactError::InvalidTransition {
            artifact_id: artifact_id.to_string(),
            from: entry.status,
            to: CanonicalArtifactStatus::BaselineVisible,
        });
    }
    let target_path = workspace_root.join(&entry.target_artifact_path);
    let overlay_file = overlays_dir(data_dir)
        .join(artifact_id)
        .join(&entry.target_artifact_path);
    let target_sha256 = if target_path.is_dir() {
        copy_exact_directory(&target_path, &overlay_file)?;
        sha256_directory_manifest(&target_path)?
    } else {
        let target_sha256 = sha256_file(&target_path)?;
        copy_exact_file(&target_path, &overlay_file)?;
        target_sha256
    };
    let mut baseline =
        load_current_baseline(data_dir)?.unwrap_or_else(|| CanonicalArtifactBaseline {
            schema_version: "canonical_artifact_baseline.v1".to_string(),
            baseline_ref: format!("astra-overlay-baseline-{}", timestamp_string()),
            baseline_mode: "overlay".to_string(),
            overlay_ref: relative_or_display(workspace_root, &overlays_dir(data_dir)),
            ledger_ref: relative_or_display(workspace_root, &ledger_path(data_dir)),
            artifact_ids: Vec::new(),
            entries: Vec::new(),
            created_at: timestamp_string(),
        });
    baseline.baseline_ref = format!("astra-overlay-baseline-{}", timestamp_string());
    baseline.baseline_mode = "overlay".to_string();
    baseline.overlay_ref = relative_or_display(workspace_root, &overlays_dir(data_dir));
    baseline.ledger_ref = relative_or_display(workspace_root, &ledger_path(data_dir));
    baseline.artifact_ids.retain(|existing_artifact_id| {
        if existing_artifact_id == artifact_id {
            return false;
        }
        ledger.entries.iter().any(|ledger_entry| {
            ledger_entry.artifact_id == *existing_artifact_id
                && ledger_entry.target_artifact_path != entry.target_artifact_path
                && ledger_entry.status != CanonicalArtifactStatus::RetiredOrSuperseded
        })
    });
    push_unique_string(&mut baseline.artifact_ids, artifact_id.to_string());
    baseline.entries.retain(|baseline_entry| {
        baseline_entry.artifact_id != artifact_id
            && baseline_entry.target_artifact_path != entry.target_artifact_path
            && ledger.entries.iter().any(|ledger_entry| {
                ledger_entry.artifact_id == baseline_entry.artifact_id
                    && ledger_entry.status != CanonicalArtifactStatus::RetiredOrSuperseded
            })
    });
    baseline.entries.push(CanonicalArtifactBaselineEntry {
        artifact_id: artifact_id.to_string(),
        target_artifact_path: entry.target_artifact_path.clone(),
        overlay_artifact_path: relative_or_display(workspace_root, &overlay_file),
        sha256: target_sha256.clone(),
    });
    write_text_atomic(
        &current_baseline_path(data_dir),
        &serde_json::to_string_pretty(&baseline)?,
    )?;
    let updated = update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::BaselineVisible,
        |entry| {
            entry.baseline_ref = Some(baseline.baseline_ref.clone());
            entry.baseline_visible_at = Some(timestamp_string());
            entry.baseline_blocker = None;
            entry.target_sha256 = Some(target_sha256.clone());
        },
    )?;
    ledger = load_ledger(data_dir)?;
    if !ledger
        .entries
        .iter()
        .any(|entry| entry.artifact_id == updated.artifact_id)
    {
        return Err(CanonicalArtifactError::UnknownArtifact(
            artifact_id.to_string(),
        ));
    }
    Ok(baseline)
}

pub fn load_current_baseline(
    data_dir: &Path,
) -> Result<Option<CanonicalArtifactBaseline>, CanonicalArtifactError> {
    let path = current_baseline_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&content)?))
}

pub fn canonical_artifact_dependency_blockers(
    data_dir: &Path,
    target_refs: &[String],
    require_runnable: bool,
) -> Result<Vec<String>, CanonicalArtifactError> {
    Ok(
        canonical_artifact_dependency_blocker_details(data_dir, target_refs, require_runnable)?
            .into_iter()
            .map(|blocker| blocker.render_for_prompt())
            .collect(),
    )
}

pub fn canonical_artifact_dependency_blocker_details(
    data_dir: &Path,
    target_refs: &[String],
    require_runnable: bool,
) -> Result<Vec<CanonicalArtifactDependencyBlocker>, CanonicalArtifactError> {
    let ledger = load_ledger(data_dir)?;
    let baseline = load_current_baseline(data_dir)?;
    let mut blockers = Vec::new();
    for target_ref in target_refs {
        let normalized = normalize_artifact_ref(target_ref);
        let Some(entry) =
            canonical_artifact_dependency_entry(&ledger, baseline.as_ref(), &normalized)
        else {
            continue;
        };
        let satisfied = if require_runnable {
            entry.status.satisfies_runnable_dependency()
        } else {
            entry.status.satisfies_file_dependency()
        };
        if !satisfied {
            blockers.push(CanonicalArtifactDependencyBlocker {
                artifact_id: entry.artifact_id.clone(),
                target_artifact_path: entry.target_artifact_path.clone(),
                requested_ref: target_ref.clone(),
                current_status: entry.status.as_str().to_string(),
                required_status: if require_runnable {
                    "integration_verified"
                } else {
                    "baseline_visible"
                }
                .to_string(),
                dependency_kind: if require_runnable { "runnable" } else { "file" }.to_string(),
                source_agent_id: entry.source_agent_id.clone(),
                source_task_id: entry.source_task_id.clone(),
                decision_ref: entry.decision_ref.clone(),
                baseline_ref: entry.baseline_ref.clone(),
            });
        }
    }
    Ok(blockers)
}

pub fn required_canonical_artifact_dependency_blocker_details(
    data_dir: &Path,
    requirements: &[RequiredCanonicalArtifact],
) -> Result<Vec<CanonicalArtifactDependencyBlocker>, CanonicalArtifactError> {
    let ledger = load_ledger(data_dir)?;
    let baseline = load_current_baseline(data_dir)?;
    let mut blockers = Vec::new();
    for requirement in requirements {
        let normalized = normalize_artifact_ref(&requirement.target_artifact_path);
        let required_status = normalize_required_status(&requirement.required_status);
        let dependency_kind = normalize_dependency_kind(&requirement.dependency_kind);
        let Some(entry) =
            canonical_artifact_dependency_entry(&ledger, baseline.as_ref(), &normalized)
        else {
            blockers.push(CanonicalArtifactDependencyBlocker {
                artifact_id: "missing".to_string(),
                target_artifact_path: requirement.target_artifact_path.clone(),
                requested_ref: requirement.target_artifact_path.clone(),
                current_status: "missing".to_string(),
                required_status,
                dependency_kind,
                source_agent_id: "unknown".to_string(),
                source_task_id: None,
                decision_ref: "none".to_string(),
                baseline_ref: None,
            });
            continue;
        };
        if !required_status_satisfied(&entry.status, &required_status) {
            blockers.push(CanonicalArtifactDependencyBlocker {
                artifact_id: entry.artifact_id.clone(),
                target_artifact_path: entry.target_artifact_path.clone(),
                requested_ref: requirement.target_artifact_path.clone(),
                current_status: entry.status.as_str().to_string(),
                required_status,
                dependency_kind,
                source_agent_id: entry.source_agent_id.clone(),
                source_task_id: entry.source_task_id.clone(),
                decision_ref: entry.decision_ref.clone(),
                baseline_ref: entry.baseline_ref.clone(),
            });
        }
    }
    Ok(blockers)
}

fn canonical_artifact_dependency_entry<'a>(
    ledger: &'a CanonicalArtifactLedger,
    baseline: Option<&CanonicalArtifactBaseline>,
    normalized_target_ref: &str,
) -> Option<&'a CanonicalArtifactEntry> {
    baseline
        .and_then(|baseline| {
            baseline.entries.iter().find(|baseline_entry| {
                normalize_artifact_ref(&baseline_entry.target_artifact_path)
                    == normalized_target_ref
            })
        })
        .and_then(|baseline_entry| {
            ledger.entries.iter().find(|entry| {
                entry.artifact_id == baseline_entry.artifact_id
                    && normalize_artifact_ref(&entry.target_artifact_path) == normalized_target_ref
            })
        })
        .or_else(|| {
            ledger.entries.iter().rev().find(|entry| {
                normalize_artifact_ref(&entry.target_artifact_path) == normalized_target_ref
            })
        })
}

pub fn apply_current_baseline_overlay(
    data_dir: &Path,
    workspace_root: &Path,
    worktree_path: &Path,
) -> Result<Option<CanonicalArtifactBaseline>, CanonicalArtifactError> {
    let Some(baseline) = load_current_baseline(data_dir)? else {
        return Ok(None);
    };
    for entry in &baseline.entries {
        if !safe_relative_path(&entry.target_artifact_path) {
            return Err(CanonicalArtifactError::UnsafePath(
                entry.target_artifact_path.clone(),
            ));
        }
        let overlay_path =
            resolve_workspace_or_absolute_path(workspace_root, &entry.overlay_artifact_path);
        let target_path = worktree_path.join(&entry.target_artifact_path);
        let copied_sha256 = if overlay_path.is_dir() {
            copy_exact_directory(&overlay_path, &target_path)?;
            sha256_directory_manifest(&target_path)?
        } else {
            copy_exact_file(&overlay_path, &target_path)?;
            sha256_file(&target_path)?
        };
        if copied_sha256 != entry.sha256 {
            return Err(CanonicalArtifactError::ChecksumMismatch {
                path: entry.target_artifact_path.clone(),
                expected: entry.sha256.clone(),
                actual: copied_sha256,
            });
        }
    }
    Ok(Some(baseline))
}

pub fn record_retired_or_superseded(
    data_dir: &Path,
    artifact_id: &str,
    retired_by_ref: String,
    cleanup_ref: Option<String>,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError> {
    let entry = update_status(
        data_dir,
        artifact_id,
        CanonicalArtifactStatus::RetiredOrSuperseded,
        |entry| {
            entry.retired_by_ref = Some(retired_by_ref.clone());
            entry.cleanup_ref = cleanup_ref.clone();
        },
    )?;
    prune_artifact_from_current_baseline(data_dir, artifact_id)?;
    Ok(entry)
}

fn prune_artifact_from_current_baseline(
    data_dir: &Path,
    artifact_id: &str,
) -> Result<(), CanonicalArtifactError> {
    let Some(mut baseline) = load_current_baseline(data_dir)? else {
        return Ok(());
    };
    let original_entry_count = baseline.entries.len();
    let original_artifact_count = baseline.artifact_ids.len();
    baseline
        .entries
        .retain(|entry| entry.artifact_id != artifact_id);
    baseline
        .artifact_ids
        .retain(|existing_artifact_id| existing_artifact_id != artifact_id);
    if baseline.entries.len() == original_entry_count
        && baseline.artifact_ids.len() == original_artifact_count
    {
        return Ok(());
    }
    baseline.baseline_ref = format!("astra-overlay-baseline-{}", timestamp_string());
    write_text_atomic(
        &current_baseline_path(data_dir),
        &serde_json::to_string_pretty(&baseline)?,
    )
}

fn update_status<F>(
    data_dir: &Path,
    artifact_id: &str,
    next_status: CanonicalArtifactStatus,
    mut apply: F,
) -> Result<CanonicalArtifactEntry, CanonicalArtifactError>
where
    F: FnMut(&mut CanonicalArtifactEntry),
{
    let mut ledger = load_ledger(data_dir)?;
    let entry = ledger
        .entries
        .iter_mut()
        .find(|entry| entry.artifact_id == artifact_id)
        .ok_or_else(|| CanonicalArtifactError::UnknownArtifact(artifact_id.to_string()))?;
    let previous_status = entry.status.clone();
    transition_entry(entry, next_status.clone())?;
    apply(entry);
    entry.updated_at = timestamp_string();
    let updated = entry.clone();
    ledger.updated_at = timestamp_string();
    write_ledger(data_dir, &ledger)?;
    append_event(
        data_dir,
        CanonicalArtifactEvent {
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
            event_id: format!("canonical_artifact_event_{}", timestamp_string()),
            artifact_id: artifact_id.to_string(),
            previous_status: Some(previous_status),
            next_status,
            authority_boundary: "runtime_mechanical_state_projection".to_string(),
            detail: "canonical artifact status updated".to_string(),
            created_at: timestamp_string(),
            metadata: BTreeMap::new(),
        },
    )?;
    Ok(updated)
}

fn transition_entry(
    entry: &mut CanonicalArtifactEntry,
    next_status: CanonicalArtifactStatus,
) -> Result<(), CanonicalArtifactError> {
    if entry.status == next_status {
        return Ok(());
    }
    if !entry.status.can_transition_to(&next_status) {
        return Err(CanonicalArtifactError::InvalidTransition {
            artifact_id: entry.artifact_id.clone(),
            from: entry.status.clone(),
            to: next_status,
        });
    }
    entry.status = next_status;
    Ok(())
}

pub fn append_event(
    data_dir: &Path,
    event: CanonicalArtifactEvent,
) -> Result<(), CanonicalArtifactError> {
    let path = events_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

pub fn artifact_id_for_seed(seed: &CanonicalArtifactSeed) -> String {
    let key = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        seed.job_id,
        seed.stage_execution_id,
        seed.source_agent_id,
        seed.source_task_id.as_deref().unwrap_or_default(),
        seed.source_ref,
        seed.target_artifact_path
    );
    format!("canonical_artifact_{}", short_sha256_hex(&key))
}

pub fn sha256_file(path: &Path) -> Result<String, CanonicalArtifactError> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn short_sha256_hex(value: &str) -> String {
    sha256_bytes(value.as_bytes())
        .chars()
        .take(16)
        .collect::<String>()
}

fn write_text_atomic(path: &Path, contents: &str) -> Result<(), CanonicalArtifactError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn copy_exact_file(source: &Path, target: &Path) -> Result<(), CanonicalArtifactError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target)?;
    Ok(())
}

fn copy_exact_directory(source: &Path, target: &Path) -> Result<(), CanonicalArtifactError> {
    if target.exists() {
        if target.is_dir() {
            fs::remove_dir_all(target)?;
        } else {
            fs::remove_file(target)?;
        }
    }
    fs::create_dir_all(target)?;
    copy_directory_contents(source, target)
}

fn copy_directory_contents(source: &Path, target: &Path) -> Result<(), CanonicalArtifactError> {
    for item in fs::read_dir(source)? {
        let item = item?;
        let source_path = item.path();
        let target_path = target.join(item.file_name());
        let metadata = item.metadata()?;
        if metadata.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_directory_contents(&source_path, &target_path)?;
        } else if metadata.is_file() {
            copy_exact_file(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn sha256_directory_manifest(root: &Path) -> Result<String, CanonicalArtifactError> {
    let mut entries = Vec::new();
    collect_directory_sha_entries(root, root, &mut entries)?;
    entries.sort();
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.as_bytes());
        hasher.update(b"\n");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_directory_sha_entries(
    root: &Path,
    current: &Path,
    entries: &mut Vec<String>,
) -> Result<(), CanonicalArtifactError> {
    for item in fs::read_dir(current)? {
        let item = item?;
        let path = item.path();
        let metadata = item.metadata()?;
        if metadata.is_dir() {
            collect_directory_sha_entries(root, &path, entries)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            entries.push(format!("{}\t{}", relative, sha256_file(&path)?));
        }
    }
    Ok(())
}

fn resolve_workspace_or_absolute_path(workspace_root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn relative_or_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
}

pub fn required_canonical_artifact_is_valid(requirement: &RequiredCanonicalArtifact) -> bool {
    safe_relative_path(&requirement.target_artifact_path)
        && !normalize_artifact_ref(&requirement.target_artifact_path)
            .starts_with(".pmcli/input-bundles/")
        && matches!(
            normalize_required_status(&requirement.required_status).as_str(),
            "baseline_visible" | "integration_verified" | "active_stage_evidence"
        )
        && matches!(
            normalize_dependency_kind(&requirement.dependency_kind).as_str(),
            "file" | "runnable" | "stage_evidence"
        )
}

fn required_status_satisfied(status: &CanonicalArtifactStatus, required_status: &str) -> bool {
    match normalize_required_status(required_status).as_str() {
        "baseline_visible" => status.satisfies_file_dependency(),
        "integration_verified" => status.satisfies_runnable_dependency(),
        "active_stage_evidence" => matches!(status, CanonicalArtifactStatus::ActiveStageEvidence),
        _ => false,
    }
}

fn normalize_required_status(value: &str) -> String {
    match value.trim() {
        "" => "baseline_visible".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn normalize_dependency_kind(value: &str) -> String {
    match value.trim() {
        "" => "file".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn normalize_artifact_ref(value: &str) -> String {
    value
        .trim()
        .trim_matches('`')
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_micros()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "research_cli_canonical_artifacts_{}_{}",
            name,
            timestamp_string()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn seed() -> CanonicalArtifactSeed {
        CanonicalArtifactSeed {
            job_id: "job_1".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_exec_1".to_string(),
            source_agent_id: "agent_a".to_string(),
            source_task_id: Some("task_a".to_string()),
            source_ref: ".pmcli/agents/agent_a/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("baseline_runner.py".to_string()),
            target_artifact_path: "baseline_runner.py".to_string(),
            artifact_kind: "python_source".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::adopt_a".to_string(),
            source_sha256: Some("abc".to_string()),
        }
    }

    #[test]
    fn records_adoption_requested_entries() {
        let data_dir = temp_dir("records_adoption_requested_entries");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        assert_eq!(entry.status, CanonicalArtifactStatus::AdoptionRequested);
        assert_eq!(entry.target_artifact_path, "baseline_runner.py");
        let ledger = load_ledger(&data_dir).expect("ledger should load");
        assert_eq!(ledger.entries.len(), 1);
        assert!(events_path(&data_dir).exists());
    }

    #[test]
    fn rejects_illegal_status_jumps() {
        let data_dir = temp_dir("rejects_illegal_status_jumps");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let err =
            record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
                .expect_err("baseline-visible requires materialized first");
        assert!(err
            .to_string()
            .contains("invalid canonical artifact transition"));
        let ledger = load_ledger(&data_dir).expect("ledger should load");
        assert_eq!(
            ledger.entries[0].status,
            CanonicalArtifactStatus::AdoptionRequested
        );
    }

    #[test]
    fn allows_materialized_then_baseline_visible() {
        let data_dir = temp_dir("allows_materialized_then_baseline_visible");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(&data_dir, &entry.artifact_id, "def".to_string())
            .expect("materialized should be recorded");
        assert_eq!(entry.status, CanonicalArtifactStatus::Materialized);
        let entry =
            record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
                .expect("baseline visibility should be recorded");
        assert_eq!(entry.status, CanonicalArtifactStatus::BaselineVisible);
        assert_eq!(entry.baseline_ref.as_deref(), Some("refs/astra/test"));
    }

    #[test]
    fn dependency_blockers_require_baseline_visible_for_file_dependencies() {
        let data_dir = temp_dir("dependency_blockers_require_baseline_visible");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(&data_dir, &entry.artifact_id, "def".to_string())
            .expect("materialized should be recorded");

        let blockers = canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("./baseline_runner.py")],
            false,
        )
        .expect("dependency blockers should resolve");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("materialized"));

        record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
            .expect("baseline visibility should be recorded");
        let blockers = canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            false,
        )
        .expect("dependency blockers should resolve");
        assert!(blockers.is_empty());
    }

    #[test]
    fn file_dependency_uses_current_baseline_when_newer_adoption_is_blocked() {
        let workspace_root = temp_dir("file_dependency_uses_current_baseline");
        let data_dir = workspace_root.join(".pmcli");
        let target = workspace_root.join("baseline_runner.py");
        fs::write(&target, "def run():\n    return 1\n").expect("baseline target should write");
        let baseline = upsert_adoption_requested(&data_dir, seed(), "main agent adopted baseline")
            .expect("baseline adoption should record");
        let baseline = record_materialized(
            &data_dir,
            &baseline.artifact_id,
            sha256_file(&target).expect("baseline sha should compute"),
        )
        .expect("baseline should materialize");
        promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &baseline.artifact_id,
        )
        .expect("baseline should promote");

        let mut blocked_seed = seed();
        blocked_seed.source_agent_id = "agent_b".to_string();
        blocked_seed.source_task_id = Some("task_b".to_string());
        blocked_seed.source_ref =
            ".pmcli/agents/agent_b/worktree_artifact_candidates.json".to_string();
        blocked_seed.decision_ref = "main_agent_stage_artifact_adoption::adopt_b".to_string();
        let blocked =
            upsert_adoption_requested(&data_dir, blocked_seed, "main agent adopted repair")
                .expect("blocked adoption should record");
        record_materialization_blocked(
            &data_dir,
            &blocked.artifact_id,
            "candidate is not a clean canonical artifact",
        )
        .expect("blocked materialization should record");

        let blockers = canonical_artifact_dependency_blocker_details(
            &data_dir,
            &["baseline_runner.py".to_string()],
            false,
        )
        .expect("file dependency should resolve through the current baseline");

        assert!(
            blockers.is_empty(),
            "a blocked newer adoption must not hide the current baseline: {blockers:?}"
        );
    }

    #[test]
    fn required_dependency_uses_current_baseline_when_newer_adoption_is_blocked() {
        let workspace_root = temp_dir("required_dependency_uses_current_baseline");
        let data_dir = workspace_root.join(".pmcli");
        let target = workspace_root.join("baseline_runner.py");
        fs::write(&target, "def run():\n    return 1\n").expect("baseline target should write");
        let baseline = upsert_adoption_requested(&data_dir, seed(), "main agent adopted baseline")
            .expect("baseline adoption should record");
        let baseline = record_materialized(
            &data_dir,
            &baseline.artifact_id,
            sha256_file(&target).expect("baseline sha should compute"),
        )
        .expect("baseline should materialize");
        promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &baseline.artifact_id,
        )
        .expect("baseline should promote");

        let mut blocked_seed = seed();
        blocked_seed.source_agent_id = "agent_b".to_string();
        blocked_seed.source_task_id = Some("task_b".to_string());
        blocked_seed.source_ref =
            ".pmcli/agents/agent_b/worktree_artifact_candidates.json".to_string();
        blocked_seed.decision_ref = "main_agent_stage_artifact_adoption::adopt_b".to_string();
        let blocked =
            upsert_adoption_requested(&data_dir, blocked_seed, "main agent adopted repair")
                .expect("blocked adoption should record");
        record_materialization_blocked(
            &data_dir,
            &blocked.artifact_id,
            "candidate is not a clean canonical artifact",
        )
        .expect("blocked materialization should record");

        let blockers = required_canonical_artifact_dependency_blocker_details(
            &data_dir,
            &[RequiredCanonicalArtifact {
                target_artifact_path: "baseline_runner.py".to_string(),
                required_status: "baseline_visible".to_string(),
                dependency_kind: "file".to_string(),
                reason: None,
            }],
        )
        .expect("required dependency should resolve through the current baseline");

        assert!(
            blockers.is_empty(),
            "a blocked newer adoption must not hide the current baseline: {blockers:?}"
        );
    }

    #[test]
    fn runnable_dependencies_require_integration_verified() {
        let data_dir = temp_dir("runnable_dependencies_require_integration_verified");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(&data_dir, &entry.artifact_id, "def".to_string())
            .expect("materialized should be recorded");
        let entry =
            record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
                .expect("baseline visibility should be recorded");

        let blockers = canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            true,
        )
        .expect("dependency blockers should resolve");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("baseline_visible"));
        assert!(blockers[0].contains("integration_verified"));

        let entry = record_integration_verified(
            &data_dir,
            &entry.artifact_id,
            "py_compile".to_string(),
            "python -m py_compile baseline_runner.py".to_string(),
            "syntax check passed".to_string(),
            vec!["worker_evidence::py_compile_pass".to_string()],
        )
        .expect("integration verification should record");
        assert_eq!(entry.status, CanonicalArtifactStatus::IntegrationVerified);
        assert_eq!(entry.integration_checks.len(), 1);

        let blockers = canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            true,
        )
        .expect("dependency blockers should resolve");
        assert!(blockers.is_empty());
    }

    #[test]
    fn active_stage_evidence_requires_integration_verified() {
        let data_dir = temp_dir("active_stage_evidence_requires_integration_verified");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(&data_dir, &entry.artifact_id, "def".to_string())
            .expect("materialized should be recorded");
        let entry =
            record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
                .expect("baseline visibility should be recorded");

        record_active_stage_evidence(
            &data_dir,
            &entry.artifact_id,
            "stage_gate::implement_solution".to_string(),
        )
        .expect_err("stage evidence must not skip integration verification");

        let entry = record_integration_verified(
            &data_dir,
            &entry.artifact_id,
            "smoke".to_string(),
            "python baseline_runner.py".to_string(),
            "smoke check passed".to_string(),
            vec!["worker_evidence::smoke_pass".to_string()],
        )
        .expect("integration verification should record");
        let entry = record_active_stage_evidence(
            &data_dir,
            &entry.artifact_id,
            "stage_gate::implement_solution".to_string(),
        )
        .expect("active stage evidence should record after integration verification");
        assert_eq!(entry.status, CanonicalArtifactStatus::ActiveStageEvidence);
        assert!(entry
            .consumed_by_task_ids
            .contains(&"stage_gate::implement_solution".to_string()));
    }

    #[test]
    fn failed_integration_blocks_runnable_dependencies_until_repaired() {
        let data_dir = temp_dir("failed_integration_blocks_runnable_dependencies");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(&data_dir, &entry.artifact_id, "def".to_string())
            .expect("materialized should be recorded");
        let entry =
            record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
                .expect("baseline visibility should be recorded");
        let entry = record_integration_failed(
            &data_dir,
            &entry.artifact_id,
            "py_compile".to_string(),
            "python -m py_compile baseline_runner.py".to_string(),
            "syntax error".to_string(),
            vec!["worker_evidence::py_compile_fail".to_string()],
        )
        .expect("failed integration should record");
        assert_eq!(entry.status, CanonicalArtifactStatus::IntegrationFailed);

        let blockers = canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            true,
        )
        .expect("dependency blockers should resolve");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("integration_failed"));

        record_integration_verified(
            &data_dir,
            &entry.artifact_id,
            "py_compile_retry".to_string(),
            "python -m py_compile baseline_runner.py".to_string(),
            "syntax check passed after repair".to_string(),
            vec!["worker_evidence::py_compile_retry_pass".to_string()],
        )
        .expect("verified integration should record");
        let blockers = canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            true,
        )
        .expect("dependency blockers should resolve");
        assert!(blockers.is_empty());
    }

    #[test]
    fn retired_state_is_allowed_from_current_states() {
        let data_dir = temp_dir("retired_state_is_allowed_from_current_states");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_retired_or_superseded(
            &data_dir,
            &entry.artifact_id,
            "route_change::pivot".to_string(),
            Some("cleanup_plan::1".to_string()),
        )
        .expect("retirement should be recorded");
        assert_eq!(entry.status, CanonicalArtifactStatus::RetiredOrSuperseded);
        assert_eq!(entry.cleanup_ref.as_deref(), Some("cleanup_plan::1"));
    }

    #[test]
    fn retired_artifact_no_longer_satisfies_file_or_runnable_dependencies() {
        let data_dir = temp_dir("retired_artifact_no_longer_satisfies_dependencies");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(&data_dir, &entry.artifact_id, "def".to_string())
            .expect("materialized should be recorded");
        let entry =
            record_baseline_visible(&data_dir, &entry.artifact_id, "refs/astra/test".to_string())
                .expect("baseline visibility should be recorded");
        let entry = record_integration_verified(
            &data_dir,
            &entry.artifact_id,
            "py_compile".to_string(),
            "python -m py_compile baseline_runner.py".to_string(),
            "syntax check passed".to_string(),
            vec!["worker_evidence::py_compile_pass".to_string()],
        )
        .expect("integration verification should record");

        assert!(canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            false,
        )
        .expect("file blockers should resolve")
        .is_empty());
        assert!(canonical_artifact_dependency_blockers(
            &data_dir,
            &[String::from("baseline_runner.py")],
            true,
        )
        .expect("runnable blockers should resolve")
        .is_empty());

        record_retired_or_superseded(
            &data_dir,
            &entry.artifact_id,
            "main_agent_route_change_request::pivot_to_v2".to_string(),
            Some("cleanup_plan::replace_baseline_runner_v1".to_string()),
        )
        .expect("retirement should be recorded");

        let file_blockers = canonical_artifact_dependency_blocker_details(
            &data_dir,
            &[String::from("baseline_runner.py")],
            false,
        )
        .expect("file blockers should resolve after retirement");
        assert_eq!(file_blockers.len(), 1);
        assert_eq!(file_blockers[0].current_status, "retired_or_superseded");
        assert_eq!(file_blockers[0].required_status, "baseline_visible");

        let runnable_blockers = canonical_artifact_dependency_blocker_details(
            &data_dir,
            &[String::from("baseline_runner.py")],
            true,
        )
        .expect("runnable blockers should resolve after retirement");
        assert_eq!(runnable_blockers.len(), 1);
        assert_eq!(runnable_blockers[0].current_status, "retired_or_superseded");
        assert_eq!(runnable_blockers[0].required_status, "integration_verified");
    }

    #[test]
    fn overlay_baseline_makes_materialized_artifact_visible_in_worker_tree() {
        let workspace_root = temp_dir("overlay_workspace");
        let data_dir = workspace_root.join(".pmcli");
        let target = workspace_root.join("baseline_runner.py");
        fs::write(&target, "def run():\n    return 1\n").expect("target should write");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(
            &data_dir,
            &entry.artifact_id,
            sha256_file(&target).expect("target sha should compute"),
        )
        .expect("materialized should record");

        let baseline = promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline promotion should succeed");

        assert_eq!(baseline.baseline_mode, "overlay");
        let worker = temp_dir("overlay_worker");
        apply_current_baseline_overlay(&data_dir, &workspace_root, &worker)
            .expect("overlay should apply")
            .expect("baseline should exist");
        let copied = worker.join("baseline_runner.py");
        assert_eq!(
            fs::read_to_string(copied).expect("copied file should read"),
            "def run():\n    return 1\n"
        );
        let ledger = load_ledger(&data_dir).expect("ledger should load");
        assert_eq!(
            ledger.entries[0].status,
            CanonicalArtifactStatus::BaselineVisible
        );
        assert!(ledger.entries[0].baseline_ref.is_some());
    }

    #[test]
    fn overlay_baseline_replaces_same_target_with_latest_artifact_entry() {
        let workspace_root = temp_dir("overlay_replaces_same_target");
        let data_dir = workspace_root.join(".pmcli");
        let target = workspace_root.join("baseline_runner.py");

        fs::write(&target, "def run():\n    return 1\n").expect("v1 target should write");
        let v1 = upsert_adoption_requested(&data_dir, seed(), "main agent adopted v1")
            .expect("v1 adoption should record");
        let v1 = record_materialized(
            &data_dir,
            &v1.artifact_id,
            sha256_file(&target).expect("v1 sha should compute"),
        )
        .expect("v1 materialized should record");
        promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &v1.artifact_id,
        )
        .expect("v1 baseline promotion should succeed");

        let mut v2_seed = seed();
        v2_seed.source_agent_id = "agent_b".to_string();
        v2_seed.source_task_id = Some("task_b".to_string());
        v2_seed.source_ref = ".pmcli/agents/agent_b/worktree_artifact_candidates.json".to_string();
        v2_seed.decision_ref = "main_agent_stage_artifact_adoption::adopt_b".to_string();
        fs::write(&target, "def run():\n    return 2\n").expect("v2 target should write");
        let v2 = upsert_adoption_requested(&data_dir, v2_seed, "main agent adopted v2")
            .expect("v2 adoption should record");
        let v2 = record_materialized(
            &data_dir,
            &v2.artifact_id,
            sha256_file(&target).expect("v2 sha should compute"),
        )
        .expect("v2 materialized should record");
        record_retired_or_superseded(
            &data_dir,
            &v1.artifact_id,
            "main_agent_stage_artifact_adoption::adopt_b replaces v1".to_string(),
            Some("cleanup_plan::replace_baseline_runner_v1".to_string()),
        )
        .expect("v1 should retire before v2 promotion");

        let baseline = promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &v2.artifact_id,
        )
        .expect("v2 baseline promotion should succeed");

        assert_eq!(baseline.entries.len(), 1);
        assert_eq!(baseline.entries[0].artifact_id, v2.artifact_id);
        assert_eq!(
            baseline.entries[0].target_artifact_path,
            "baseline_runner.py"
        );
        assert_eq!(baseline.artifact_ids, vec![v2.artifact_id.clone()]);

        let worker = temp_dir("overlay_replaces_same_target_worker");
        apply_current_baseline_overlay(&data_dir, &workspace_root, &worker)
            .expect("overlay should apply")
            .expect("baseline should exist");
        assert_eq!(
            fs::read_to_string(worker.join("baseline_runner.py"))
                .expect("worker should read current baseline file"),
            "def run():\n    return 2\n"
        );
    }

    #[test]
    fn retiring_artifact_removes_it_from_current_overlay_baseline() {
        let workspace_root = temp_dir("retire_prunes_current_overlay");
        let data_dir = workspace_root.join(".pmcli");
        let target = workspace_root.join("baseline_runner.py");
        fs::write(&target, "def run():\n    return 1\n").expect("target should write");
        let entry = upsert_adoption_requested(&data_dir, seed(), "main agent adopted candidate")
            .expect("adoption request should be recorded");
        let entry = record_materialized(
            &data_dir,
            &entry.artifact_id,
            sha256_file(&target).expect("target sha should compute"),
        )
        .expect("materialized should record");
        promote_materialized_artifact_to_overlay_baseline(
            &data_dir,
            &workspace_root,
            &entry.artifact_id,
        )
        .expect("baseline promotion should succeed");

        record_retired_or_superseded(
            &data_dir,
            &entry.artifact_id,
            "cleanup_plan::remove_baseline_runner".to_string(),
            Some("cleanup_plan::remove_baseline_runner".to_string()),
        )
        .expect("retirement should record");

        let baseline = load_current_baseline(&data_dir)
            .expect("baseline should load")
            .expect("baseline should still exist for audit");
        assert!(baseline.entries.is_empty());
        assert!(baseline.artifact_ids.is_empty());
        let worker = temp_dir("retire_prunes_current_overlay_worker");
        apply_current_baseline_overlay(&data_dir, &workspace_root, &worker)
            .expect("empty overlay should apply")
            .expect("baseline should exist");
        assert!(
            !worker.join("baseline_runner.py").exists(),
            "retired artifact must not be copied into new worker worktrees"
        );
    }
}
