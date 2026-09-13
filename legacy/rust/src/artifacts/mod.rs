use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactFamilySummary {
    pub family: String,
    pub canonical_path: String,
    pub manifest_path: String,
    pub canonical_id: Option<String>,
    pub latest_id: Option<String>,
    pub archive_ids: Vec<String>,
    pub status: String,
    pub artifact_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactFamily {
    pub schema_version: String,
    pub artifact_family: String,
    pub canonical_id: Option<String>,
    pub latest_id: Option<String>,
    pub archive_ids: Vec<String>,
    pub supersession_chain: Vec<ArtifactSupersessionLink>,
    pub source_branch_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSupersessionLink {
    pub from_id: String,
    pub to_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactListResult {
    pub families: Vec<ArtifactFamilySummary>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactPromotionCandidate {
    pub candidate_id: String,
    pub artifact_family: String,
    pub artifact_id: String,
    pub source_session_id: String,
    pub source_summary_ref: String,
    pub source_artifact_path: String,
    pub status: String,
    pub created_at: String,
    pub promoted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactPromotionQueueResult {
    pub candidates: Vec<ArtifactPromotionCandidate>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactPromotionResult {
    pub candidate_id: String,
    pub artifact_family: String,
    pub promoted_artifact_id: String,
    pub source_artifact_path: String,
    pub manifest_path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactPromotionHistoryEntry {
    pub event: String,
    pub artifact_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactInspectionResult {
    pub target: String,
    pub canonical_path: String,
    pub manifest: ArtifactFamily,
    pub lineage: Vec<String>,
    pub promotion_history: Vec<ArtifactPromotionHistoryEntry>,
    pub review_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCleanupAction {
    pub action_id: String,
    pub action_type: String,
    pub family: String,
    pub target_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub destination_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub git_effect: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCleanupGate {
    pub gate_id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCleanupProposal {
    pub plan_id: String,
    pub dry_run: bool,
    pub fingerprint: String,
    pub rollback_snapshot_ref: String,
    pub git_governance: RepoCleanupGitGovernance,
    pub proposed_actions: Vec<RepoCleanupAction>,
    pub applied_actions: Vec<RepoCleanupAction>,
    pub review_gates: Vec<RepoCleanupGate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCleanupGitGovernance {
    pub authority: String,
    pub workspace_root: String,
    pub is_repository: bool,
    pub head_commit_before: String,
    pub branch_before: String,
    pub dirty_files_before: Vec<String>,
    pub staged_files_before: Vec<String>,
    pub untracked_files_before: Vec<String>,
    pub expected_tracked_deletions: Vec<String>,
    pub ignored_rollback_roots: Vec<String>,
    pub post_apply_dirty_files: Vec<String>,
    pub post_apply_staged_files: Vec<String>,
    pub post_apply_untracked_files: Vec<String>,
    pub commit_required: bool,
    pub recommended_commit_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCleanupRestoreResult {
    pub plan_id: String,
    pub rollback_snapshot_ref: String,
    pub restored_actions: Vec<RepoCleanupAction>,
}

#[derive(Debug)]
pub enum ArtifactError {
    UnknownTarget(String),
    UnknownPlan(String),
    UnknownCandidate(String),
    StalePlan(String),
    ScopeViolation(String),
    GateUnsatisfied(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTarget(target) => write!(f, "unknown artifact target: {target}"),
            Self::UnknownPlan(plan_id) => write!(f, "unknown cleanup plan: {plan_id}"),
            Self::UnknownCandidate(candidate_id) => {
                write!(f, "unknown promotion candidate: {candidate_id}")
            }
            Self::StalePlan(plan_id) => write!(f, "cleanup plan is stale: {plan_id}"),
            Self::ScopeViolation(message) => write!(f, "{message}"),
            Self::GateUnsatisfied(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "artifact IO failed: {err}"),
            Self::Serde(err) => write!(f, "artifact serialization failed: {err}"),
        }
    }
}

impl std::error::Error for ArtifactError {}

impl From<std::io::Error> for ArtifactError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ArtifactError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn list(
    data_dir: &Path,
    family_filter: Option<&str>,
    status_filter: Option<&str>,
    include_archive: bool,
) -> ArtifactListResult {
    let mut families = default_families(data_dir);
    if !include_archive {
        families.retain(|family| family.family != "archive");
    }
    if let Some(filter) = family_filter {
        families.retain(|family| family.family == filter);
    }
    if let Some(filter) = status_filter {
        families.retain(|family| family.status == filter);
    }
    ArtifactListResult {
        total_count: families.len(),
        families,
    }
}

pub fn inspect(
    data_dir: &Path,
    workspace_root: &Path,
    target: &str,
) -> Result<ArtifactInspectionResult, ArtifactError> {
    let family = default_families(data_dir)
        .into_iter()
        .find(|family| family.family == target);
    let (path, manifest_family) = if let Some(family) = family {
        (
            PathBuf::from(family.canonical_path),
            Some(family.family.clone()),
        )
    } else {
        let path = if let Ok(data_dir_candidate) =
            crate::workspace::path::resolve_existing_child_path(data_dir, target)
        {
            data_dir_candidate
        } else {
            crate::workspace::path::resolve_existing_workspace_path(workspace_root, target)
                .map_err(|err| match err {
                    crate::workspace::path::WorkspacePathError::AbsolutePath(_)
                    | crate::workspace::path::WorkspacePathError::ScopeViolation { .. } => {
                        ArtifactError::ScopeViolation(err.to_string())
                    }
                    _ => ArtifactError::UnknownTarget(target.to_string()),
                })?
        };
        (path, None)
    };

    let manifest = if let Some(family) = manifest_family {
        load_or_create_family_manifest(data_dir, &family, &path)
    } else if let Some(summary) = default_families(data_dir)
        .into_iter()
        .find(|family| family.canonical_path == path.display().to_string())
    {
        load_or_create_family_manifest(data_dir, &summary.family, &path)
    } else {
        synthetic_family_manifest(target, &path)
    }?;
    let promotion_history = promotion_history_for_manifest(&manifest);

    Ok(ArtifactInspectionResult {
        target: target.to_string(),
        canonical_path: path.display().to_string(),
        manifest,
        lineage: collect_lineage(&path),
        promotion_history,
        review_links: crate::reviews::review_links_for_target(data_dir, target, &path),
    })
}

pub fn stage_summary_candidate(
    data_dir: &Path,
    session_id: &str,
    summary_ref: &str,
    summary_path: &Path,
) -> Result<String, ArtifactError> {
    let candidate_id = format!("cand_{}", timestamp_string());
    let artifact_id = artifact_id_for_path("summaries", &summary_path.display().to_string());
    let candidate = ArtifactPromotionCandidate {
        candidate_id: candidate_id.clone(),
        artifact_family: "summaries".to_string(),
        artifact_id,
        source_session_id: session_id.to_string(),
        source_summary_ref: summary_ref.to_string(),
        source_artifact_path: summary_path.display().to_string(),
        status: "pending_review".to_string(),
        created_at: timestamp_string(),
        promoted_at: None,
    };
    persist_candidate(data_dir, &candidate)?;
    Ok(candidate_id)
}

pub fn promotion_candidates(
    data_dir: &Path,
) -> Result<ArtifactPromotionQueueResult, ArtifactError> {
    let mut candidates = Vec::new();
    let dir = promotion_queue_dir(data_dir);
    if dir.exists() {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                candidates.push(serde_json::from_str(&fs::read_to_string(path)?)?);
            }
        }
    }
    candidates.sort_by(|left: &ArtifactPromotionCandidate, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.candidate_id.cmp(&right.candidate_id))
    });
    Ok(ArtifactPromotionQueueResult {
        total_count: candidates.len(),
        candidates,
    })
}

pub fn promote_candidate(
    data_dir: &Path,
    candidate_id: &str,
) -> Result<ArtifactPromotionResult, ArtifactError> {
    let mut candidate = load_candidate(data_dir, candidate_id)?;
    if candidate.status == "promoted" {
        return Ok(ArtifactPromotionResult {
            candidate_id: candidate.candidate_id,
            artifact_family: candidate.artifact_family.clone(),
            promoted_artifact_id: candidate.artifact_id.clone(),
            source_artifact_path: candidate.source_artifact_path,
            manifest_path: family_manifest_path(data_dir, &candidate.artifact_family)
                .display()
                .to_string(),
            status: "promoted".to_string(),
        });
    }
    let source_path = PathBuf::from(&candidate.source_artifact_path);
    let data_root = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    let source_parent = source_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.to_path_buf());
    let canonical_parent = source_parent
        .canonicalize()
        .unwrap_or_else(|_| source_parent.clone());
    if !canonical_parent.starts_with(&data_root) {
        return Err(ArtifactError::ScopeViolation(format!(
            "promotion candidate escaped project data scope: {}",
            source_path.display()
        )));
    }
    if !source_path.exists() {
        return Err(ArtifactError::UnknownTarget(
            source_path.display().to_string(),
        ));
    }

    candidate.status = "promoted".to_string();
    candidate.promoted_at = Some(timestamp_string());
    persist_candidate(data_dir, &candidate)?;
    upsert_promoted_family_manifest(data_dir, &candidate)?;

    Ok(ArtifactPromotionResult {
        candidate_id: candidate.candidate_id,
        artifact_family: candidate.artifact_family.clone(),
        promoted_artifact_id: candidate.artifact_id,
        source_artifact_path: candidate.source_artifact_path,
        manifest_path: family_manifest_path(data_dir, &candidate.artifact_family)
            .display()
            .to_string(),
        status: "promoted".to_string(),
    })
}

pub fn cleanup_plan(data_dir: &Path) -> Result<RepoCleanupProposal, ArtifactError> {
    cleanup_plan_with_gates(data_dir, Vec::new())
}

pub fn cleanup_plan_with_gates(
    data_dir: &Path,
    extra_gates: Vec<RepoCleanupGate>,
) -> Result<RepoCleanupProposal, ArtifactError> {
    cleanup_plan_with_gates_and_actions(data_dir, extra_gates, Vec::new())
}

pub fn cleanup_plan_with_gates_and_actions(
    data_dir: &Path,
    extra_gates: Vec<RepoCleanupGate>,
    extra_actions: Vec<RepoCleanupAction>,
) -> Result<RepoCleanupProposal, ArtifactError> {
    let mut proposed_actions = collect_cleanup_actions(data_dir)?;
    proposed_actions.extend(extra_actions);
    proposed_actions.sort_by(|left, right| {
        left.target_path
            .cmp(&right.target_path)
            .then(left.action_type.cmp(&right.action_type))
            .then(left.destination_path.cmp(&right.destination_path))
    });
    proposed_actions.dedup_by(|left, right| {
        left.action_type == right.action_type
            && left.target_path == right.target_path
            && left.destination_path == right.destination_path
    });
    let plan_id = format!("cln_{}", timestamp_string());
    let rollback_snapshot_ref = create_cleanup_snapshot(data_dir, &plan_id, &proposed_actions)?;
    let git_governance =
        git_governance_for_plan(data_dir, &proposed_actions, &rollback_snapshot_ref);
    let mut review_gates = vec![
        RepoCleanupGate {
            gate_id: "dry_run_reviewed".to_string(),
            status: "satisfied".to_string(),
            detail: "review the proposed cleanup actions before apply".to_string(),
        },
        RepoCleanupGate {
            gate_id: "fingerprint_matches".to_string(),
            status: "required".to_string(),
            detail: "cleanup apply refuses stale plans".to_string(),
        },
        RepoCleanupGate {
            gate_id: "rollback_snapshot_created".to_string(),
            status: "satisfied".to_string(),
            detail: "cleanup plan captured a rollback snapshot before apply".to_string(),
        },
        RepoCleanupGate {
            gate_id: "git_native_commit_required".to_string(),
            status: "satisfied".to_string(),
            detail: if git_governance.commit_required {
                format!(
                    "cleanup apply will leave tracked public deletions in the git working tree; commit with: {}",
                    git_governance.recommended_commit_message
                )
            } else if git_governance.is_repository {
                "no tracked public cleanup diff is expected".to_string()
            } else {
                "workspace is not a git repository; cleanup remains snapshot-backed".to_string()
            },
        },
    ];
    review_gates.extend(extra_gates);
    let proposal = RepoCleanupProposal {
        plan_id: plan_id.clone(),
        dry_run: true,
        fingerprint: fingerprint_for_actions(&proposed_actions),
        rollback_snapshot_ref,
        git_governance,
        proposed_actions,
        applied_actions: Vec::new(),
        review_gates,
    };
    persist_cleanup_plan(data_dir, &proposal)?;
    Ok(proposal)
}

pub fn cleanup_apply(data_dir: &Path, plan_id: &str) -> Result<RepoCleanupProposal, ArtifactError> {
    let mut proposal = load_cleanup_plan(data_dir, plan_id)?;
    let current_actions = collect_cleanup_actions_for_plan(data_dir, &proposal)?;
    let current_fingerprint = fingerprint_for_actions(&current_actions);
    if proposal.fingerprint != current_fingerprint {
        return Err(ArtifactError::StalePlan(plan_id.to_string()));
    }
    if proposal.rollback_snapshot_ref.is_empty()
        || !PathBuf::from(&proposal.rollback_snapshot_ref).exists()
    {
        return Err(ArtifactError::GateUnsatisfied(format!(
            "cleanup plan lacks a rollback snapshot: {plan_id}"
        )));
    }

    for gate in &mut proposal.review_gates {
        if gate.gate_id == "fingerprint_matches" {
            gate.status = "satisfied".to_string();
        }
        if gate.gate_id == "git_native_commit_required" && proposal.git_governance.commit_required {
            gate.status = "satisfied".to_string();
            gate.detail = format!(
                "cleanup apply produced git working-tree diff; commit with: {}",
                proposal.git_governance.recommended_commit_message
            );
        }
    }
    if let Some(gate) = proposal
        .review_gates
        .iter()
        .find(|gate| gate.status != "satisfied")
    {
        return Err(ArtifactError::GateUnsatisfied(format!(
            "cleanup review gate not satisfied: {}",
            gate.gate_id
        )));
    }

    for action in &proposal.proposed_actions {
        apply_cleanup_action(data_dir, action)?;
    }

    proposal.dry_run = false;
    proposal.applied_actions = proposal.proposed_actions.clone();
    refresh_git_governance_after_apply(data_dir, &mut proposal.git_governance);
    persist_cleanup_plan(data_dir, &proposal)?;
    Ok(proposal)
}

pub fn cleanup_restore(
    data_dir: &Path,
    plan_id: &str,
) -> Result<RepoCleanupRestoreResult, ArtifactError> {
    let proposal = load_cleanup_plan(data_dir, plan_id)?;
    if proposal.rollback_snapshot_ref.is_empty() {
        return Err(ArtifactError::GateUnsatisfied(format!(
            "cleanup plan lacks a rollback snapshot: {plan_id}"
        )));
    }
    let snapshot_dir = PathBuf::from(&proposal.rollback_snapshot_ref);
    let manifest_path = snapshot_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(ArtifactError::UnknownPlan(plan_id.to_string()));
    }

    let manifest: CleanupSnapshotManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let archive_root = data_dir.join("archive").canonicalize()?;
    let workspace_root = workspace_root_for_data_dir(data_dir);
    let mut restored_actions = Vec::new();
    for entry in manifest.entries {
        let source = PathBuf::from(&entry.source_path);
        let snapshot = PathBuf::from(&entry.snapshot_path);
        if !snapshot.exists() {
            return Err(ArtifactError::UnknownTarget(snapshot.display().to_string()));
        }
        if let Some(action) = proposal
            .applied_actions
            .iter()
            .chain(proposal.proposed_actions.iter())
            .find(|action| action.action_id == entry.action_id)
        {
            validate_cleanup_restore_scope(
                data_dir,
                action,
                &source,
                &archive_root,
                &workspace_root,
            )?;
            restore_cleanup_snapshot_entry(&snapshot, &source)?;
            if action.action_type == "move_to_archive" && !action.destination_path.is_empty() {
                remove_path_if_exists(&PathBuf::from(&action.destination_path))?;
            }
            restored_actions.push(action.clone());
        } else {
            validate_archive_scoped_path(&source, &archive_root, "cleanup restore target")?;
            restore_cleanup_snapshot_entry(&snapshot, &source)?;
            restored_actions.push(RepoCleanupAction {
                action_id: entry.action_id,
                action_type: "restore".to_string(),
                family: "archive".to_string(),
                target_path: source.display().to_string(),
                destination_path: String::new(),
                git_effect: "rollback_restore".to_string(),
                rationale: "restored from rollback snapshot".to_string(),
            });
        }
    }

    Ok(RepoCleanupRestoreResult {
        plan_id: plan_id.to_string(),
        rollback_snapshot_ref: proposal.rollback_snapshot_ref,
        restored_actions,
    })
}

pub fn cleanup_actions_for_canonicality_report(
    data_dir: &Path,
    workspace_root: &Path,
    report: &crate::canonicality::CanonicalityAuditReport,
) -> Result<Vec<RepoCleanupAction>, ArtifactError> {
    let archive_root = data_dir
        .join("archive")
        .join("canonical_lineage")
        .join(timestamp_string());
    let workspace_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let mut actions = Vec::new();
    for violation in &report.superseded_public_artifacts {
        if violation.path.trim().is_empty() {
            continue;
        }
        let target = if Path::new(&violation.path).is_absolute() {
            PathBuf::from(&violation.path)
        } else {
            workspace_root.join(&violation.path)
        };
        let Ok(resolved_target) = target.canonicalize() else {
            continue;
        };
        if !resolved_target.starts_with(&workspace_root) {
            return Err(ArtifactError::ScopeViolation(format!(
                "canonical cleanup target escaped workspace: {}",
                target.display()
            )));
        }
        let relative = resolved_target
            .strip_prefix(&workspace_root)
            .unwrap_or(resolved_target.as_path());
        let destination = archive_root.join(relative);
        actions.push(RepoCleanupAction {
            action_id: format!(
                "act_canonical_lineage_{}_{}",
                timestamp_string(),
                actions.len()
            ),
            action_type: "move_to_archive".to_string(),
            family: "canonical_lineage".to_string(),
            target_path: resolved_target.display().to_string(),
            destination_path: destination.display().to_string(),
            git_effect: "tracked_public_deletion_with_ignored_rollback_copy".to_string(),
            rationale: violation.detail.clone(),
        });
    }
    Ok(actions)
}

pub fn canonicality_unresolved_cleanup_violation_count(
    report: &crate::canonicality::CanonicalityAuditReport,
) -> usize {
    report
        .blocking_violations
        .iter()
        .filter(|violation| violation.violation_type != "superseded_public_artifact")
        .count()
}

fn default_families(data_dir: &Path) -> Vec<ArtifactFamilySummary> {
    let mut families = vec![
        family_summary("sessions", data_dir.join("sessions")),
        family_summary("events", data_dir.join("events")),
        family_summary("checkpoint", data_dir.join("project_state.json")),
        family_summary("reviews", data_dir.join("reviews")),
        family_summary("summaries", data_dir.join("sessions")),
        family_summary("archive", data_dir.join("archive")),
        family_summary("promotion_queue", promotion_queue_dir(data_dir)),
        family_summary(
            "cleanup_plans",
            data_dir.join("repo_governance").join("cleanup_plans"),
        ),
    ];
    families.sort_by(|left, right| left.family.cmp(&right.family));
    families
}

fn family_summary(family: &str, path: PathBuf) -> ArtifactFamilySummary {
    let manifest = load_or_create_family_manifest_from_summary_path(family, &path);
    ArtifactFamilySummary {
        family: family.to_string(),
        canonical_path: path.display().to_string(),
        manifest_path: manifest_path_for_family(family, &path)
            .display()
            .to_string(),
        canonical_id: manifest.canonical_id,
        latest_id: manifest.latest_id,
        archive_ids: manifest.archive_ids,
        status: if path.exists() {
            "ready".to_string()
        } else {
            "empty".to_string()
        },
        artifact_count: count_artifacts(&path),
    }
}

fn load_or_create_family_manifest_from_summary_path(family: &str, path: &Path) -> ArtifactFamily {
    let Some(data_dir) = data_dir_from_family_path(family, path) else {
        return synthetic_family_manifest(family, path).expect("synthetic family should serialize");
    };
    load_or_create_family_manifest(&data_dir, family, path)
        .unwrap_or_else(|_| synthetic_family_manifest(family, path).expect("synthetic family"))
}

fn data_dir_from_family_path(family: &str, path: &Path) -> Option<PathBuf> {
    match family {
        "checkpoint" => path.parent().map(Path::to_path_buf),
        "cleanup_plans" => path.parent().and_then(Path::parent).map(Path::to_path_buf),
        _ => path.parent().map(Path::to_path_buf),
    }
}

fn family_manifest_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("artifacts").join("families")
}

fn family_manifest_path(data_dir: &Path, family: &str) -> PathBuf {
    family_manifest_dir(data_dir).join(format!("{family}.json"))
}

fn manifest_path_for_family(family: &str, path: &Path) -> PathBuf {
    data_dir_from_family_path(family, path)
        .map(|data_dir| family_manifest_path(&data_dir, family))
        .unwrap_or_else(|| path.join(".manifest.json"))
}

fn load_or_create_family_manifest(
    data_dir: &Path,
    family: &str,
    path: &Path,
) -> Result<ArtifactFamily, ArtifactError> {
    let manifest_path = family_manifest_path(data_dir, family);
    let refreshed = synthetic_family_manifest(family, path)?;
    if manifest_path.exists() {
        let current: ArtifactFamily = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        if family == "summaries" && current.latest_id != refreshed.latest_id {
            return Ok(current);
        }
        if current == refreshed {
            return Ok(current);
        }
        atomic_write(&manifest_path, &serde_json::to_string_pretty(&refreshed)?)?;
        return Ok(refreshed);
    }
    atomic_write(&manifest_path, &serde_json::to_string_pretty(&refreshed)?)?;
    Ok(refreshed)
}

fn promotion_queue_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("artifacts").join("promotion_queue")
}

fn candidate_path(data_dir: &Path, candidate_id: &str) -> PathBuf {
    promotion_queue_dir(data_dir).join(format!("{candidate_id}.json"))
}

fn persist_candidate(
    data_dir: &Path,
    candidate: &ArtifactPromotionCandidate,
) -> Result<(), ArtifactError> {
    atomic_write(
        &candidate_path(data_dir, &candidate.candidate_id),
        &serde_json::to_string_pretty(candidate)?,
    )
}

fn load_candidate(
    data_dir: &Path,
    candidate_id: &str,
) -> Result<ArtifactPromotionCandidate, ArtifactError> {
    let path = candidate_path(data_dir, candidate_id);
    if !path.exists() {
        return Err(ArtifactError::UnknownCandidate(candidate_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn upsert_promoted_family_manifest(
    data_dir: &Path,
    candidate: &ArtifactPromotionCandidate,
) -> Result<(), ArtifactError> {
    let manifest_path = family_manifest_path(data_dir, &candidate.artifact_family);
    let mut manifest = if manifest_path.exists() {
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?
    } else {
        synthetic_family_manifest(
            &candidate.artifact_family,
            &PathBuf::from(&candidate.source_artifact_path),
        )?
    };
    let previous_latest = manifest.latest_id.clone();
    if !manifest.archive_ids.contains(&candidate.artifact_id) {
        manifest.archive_ids.push(candidate.artifact_id.clone());
    }
    manifest.latest_id = Some(candidate.artifact_id.clone());
    if let Some(previous_latest) = previous_latest {
        if previous_latest != candidate.artifact_id {
            manifest.supersession_chain.push(ArtifactSupersessionLink {
                from_id: previous_latest,
                to_id: candidate.artifact_id.clone(),
                reason: format!("promoted_candidate:{}", candidate.candidate_id),
            });
        } else if !manifest.supersession_chain.iter().any(|link| {
            link.to_id == candidate.artifact_id && link.reason.starts_with("promoted_candidate:")
        }) {
            manifest.supersession_chain.push(ArtifactSupersessionLink {
                from_id: manifest
                    .canonical_id
                    .clone()
                    .unwrap_or_else(|| format!("artifact:{}:canonical", candidate.artifact_family)),
                to_id: candidate.artifact_id.clone(),
                reason: format!("promoted_candidate:{}", candidate.candidate_id),
            });
        }
    } else {
        manifest.supersession_chain.push(ArtifactSupersessionLink {
            from_id: manifest
                .canonical_id
                .clone()
                .unwrap_or_else(|| format!("artifact:{}:canonical", candidate.artifact_family)),
            to_id: candidate.artifact_id.clone(),
            reason: format!("promoted_candidate:{}", candidate.candidate_id),
        });
    }
    atomic_write(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
    Ok(())
}

fn synthetic_family_manifest(family: &str, path: &Path) -> Result<ArtifactFamily, ArtifactError> {
    let lineage = collect_lineage(path);
    let canonical_id = format!("artifact:{family}:canonical");
    let archive_ids = lineage
        .iter()
        .map(|entry| artifact_id_for_path(family, entry))
        .collect::<Vec<_>>();
    let latest_id = archive_ids
        .last()
        .cloned()
        .unwrap_or_else(|| canonical_id.clone());
    let supersession_chain = archive_ids
        .windows(2)
        .map(|pair| ArtifactSupersessionLink {
            from_id: pair[0].clone(),
            to_id: pair[1].clone(),
            reason: "lineage_order".to_string(),
        })
        .collect::<Vec<_>>();

    Ok(ArtifactFamily {
        schema_version: "v1alpha1".to_string(),
        artifact_family: family.to_string(),
        canonical_id: Some(canonical_id),
        latest_id: Some(latest_id),
        archive_ids,
        supersession_chain,
        source_branch_ids: Vec::new(),
    })
}

fn artifact_id_for_path(family: &str, path: &str) -> String {
    let leaf = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    format!("artifact:{family}:{leaf}")
}

fn promotion_history_for_manifest(manifest: &ArtifactFamily) -> Vec<ArtifactPromotionHistoryEntry> {
    let artifact_id = manifest
        .latest_id
        .clone()
        .or_else(|| manifest.canonical_id.clone())
        .unwrap_or_else(|| format!("artifact:{}:unknown", manifest.artifact_family));
    let mut entries = vec![ArtifactPromotionHistoryEntry {
        event: "artifact_family_manifested".to_string(),
        artifact_id: artifact_id.clone(),
        detail: format!("manifest tracks {} family", manifest.artifact_family),
    }];
    entries.extend(
        manifest
            .supersession_chain
            .iter()
            .map(|link| ArtifactPromotionHistoryEntry {
                event: if link.reason.starts_with("promoted_candidate:") {
                    "artifact_promoted".to_string()
                } else {
                    "artifact_superseded".to_string()
                },
                artifact_id: link.to_id.clone(),
                detail: format!("{} -> {}: {}", link.from_id, link.to_id, link.reason),
            }),
    );
    entries
}

fn count_artifacts(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return 1;
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .count()
}

fn collect_lineage(path: &Path) -> Vec<String> {
    if path.is_file() {
        return vec![path.display().to_string()];
    }
    let mut lineage = fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path().display().to_string())
        .collect::<Vec<_>>();
    lineage.sort();
    lineage
}

fn collect_cleanup_actions(data_dir: &Path) -> Result<Vec<RepoCleanupAction>, ArtifactError> {
    let mut actions = Vec::new();
    let archive_root = data_dir.join("archive");
    if archive_root.exists() {
        for entry in walk_paths(&archive_root)? {
            let file_name = entry
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if file_name.ends_with(".tmp")
                || file_name.ends_with(".bak")
                || file_name.ends_with(".old")
            {
                actions.push(RepoCleanupAction {
                    action_id: format!("act_{}", timestamp_string()),
                    action_type: "remove".to_string(),
                    family: "archive".to_string(),
                    target_path: entry.display().to_string(),
                    destination_path: String::new(),
                    git_effect: "ignored_archive_cleanup".to_string(),
                    rationale: "stale archived scratch artifact".to_string(),
                });
            }
        }
    }
    actions.sort_by(|left, right| left.target_path.cmp(&right.target_path));
    Ok(actions)
}

fn collect_cleanup_actions_for_plan(
    data_dir: &Path,
    proposal: &RepoCleanupProposal,
) -> Result<Vec<RepoCleanupAction>, ArtifactError> {
    let mut actions = collect_cleanup_actions(data_dir)?;
    actions.extend(
        proposal
            .proposed_actions
            .iter()
            .filter(|action| action.action_type != "remove")
            .cloned(),
    );
    actions.sort_by(|left, right| {
        left.target_path
            .cmp(&right.target_path)
            .then(left.action_type.cmp(&right.action_type))
            .then(left.destination_path.cmp(&right.destination_path))
    });
    actions.dedup_by(|left, right| {
        left.action_type == right.action_type
            && left.target_path == right.target_path
            && left.destination_path == right.destination_path
    });
    Ok(actions)
}

fn git_governance_for_plan(
    data_dir: &Path,
    actions: &[RepoCleanupAction],
    rollback_snapshot_ref: &str,
) -> RepoCleanupGitGovernance {
    let workspace_root = workspace_root_for_data_dir(data_dir);
    let status = git_status_snapshot(&workspace_root);
    let expected_tracked_deletions = actions
        .iter()
        .filter(|action| action.action_type == "move_to_archive")
        .filter_map(|action| {
            workspace_relative_path(&workspace_root, &PathBuf::from(&action.target_path))
        })
        .filter(|relative| git_path_is_tracked(&workspace_root, relative))
        .collect::<Vec<_>>();
    let commit_required = status.is_repository && !expected_tracked_deletions.is_empty();
    RepoCleanupGitGovernance {
        authority: "git_worktree_diff_is_canonical_cleanup_record".to_string(),
        workspace_root: workspace_root.display().to_string(),
        is_repository: status.is_repository,
        head_commit_before: status.head_commit,
        branch_before: status.branch,
        dirty_files_before: status.dirty_files,
        staged_files_before: status.staged_files,
        untracked_files_before: status.untracked_files,
        expected_tracked_deletions,
        ignored_rollback_roots: vec![rollback_snapshot_ref.to_string()],
        post_apply_dirty_files: Vec::new(),
        post_apply_staged_files: Vec::new(),
        post_apply_untracked_files: Vec::new(),
        commit_required,
        recommended_commit_message: "Canonical cleanup: remove superseded public artifacts"
            .to_string(),
    }
}

fn refresh_git_governance_after_apply(data_dir: &Path, governance: &mut RepoCleanupGitGovernance) {
    let workspace_root = workspace_root_for_data_dir(data_dir);
    let status = git_status_snapshot(&workspace_root);
    governance.post_apply_dirty_files = status.dirty_files;
    governance.post_apply_staged_files = status.staged_files;
    governance.post_apply_untracked_files = status.untracked_files;
}

#[derive(Debug, Default)]
struct GitStatusSnapshot {
    is_repository: bool,
    branch: String,
    head_commit: String,
    dirty_files: Vec<String>,
    staged_files: Vec<String>,
    untracked_files: Vec<String>,
}

fn git_status_snapshot(workspace_root: &Path) -> GitStatusSnapshot {
    let is_repository = git_output(workspace_root, &["rev-parse", "--is-inside-work-tree"])
        .map(|value| value == "true")
        .unwrap_or(false);
    if !is_repository {
        return GitStatusSnapshot::default();
    }
    let (dirty_files, staged_files, untracked_files) = git_status_files(workspace_root);
    GitStatusSnapshot {
        is_repository: true,
        branch: git_output(workspace_root, &["branch", "--show-current"]).unwrap_or_default(),
        head_commit: git_output(workspace_root, &["rev-parse", "HEAD"]).unwrap_or_default(),
        dirty_files,
        staged_files,
        untracked_files,
    }
}

fn git_status_files(workspace_root: &Path) -> (Vec<String>, Vec<String>, Vec<String>) {
    let status = git_raw_output(workspace_root, &["status", "--porcelain=v1"]).unwrap_or_default();
    let mut dirty = Vec::new();
    let mut staged = Vec::new();
    let mut untracked = Vec::new();
    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let code = &line[..2];
        let path = line[3..].to_string();
        if code == "??" {
            untracked.push(path);
        } else {
            dirty.push(path.clone());
            if !code.starts_with(' ') {
                staged.push(path);
            }
        }
    }
    dirty.sort();
    staged.sort();
    untracked.sort();
    (dirty, staged, untracked)
}

fn git_path_is_tracked(workspace_root: &Path, path: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["ls-files", "--error-unmatch", path])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    git_raw_output(workspace_root, args).map(|output| output.trim().to_string())
}

fn git_raw_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn workspace_relative_path(workspace_root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(workspace_root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

fn apply_cleanup_action(data_dir: &Path, action: &RepoCleanupAction) -> Result<(), ArtifactError> {
    match action.action_type.as_str() {
        "remove" => {
            let path = PathBuf::from(&action.target_path);
            let resolved = path.canonicalize()?;
            let archive_root = data_dir.join("archive").canonicalize()?;
            validate_archive_scoped_resolved_path(&resolved, &archive_root, "cleanup target")?;
            remove_path_if_exists(&path)?;
        }
        "move_to_archive" => {
            if action.destination_path.trim().is_empty() {
                return Err(ArtifactError::GateUnsatisfied(format!(
                    "cleanup move action lacks destination: {}",
                    action.action_id
                )));
            }
            let source = PathBuf::from(&action.target_path);
            let destination = PathBuf::from(&action.destination_path);
            let resolved_source = source.canonicalize()?;
            validate_workspace_cleanup_source(data_dir, &resolved_source)?;
            validate_archive_destination(data_dir, &destination)?;
            if destination.exists() {
                return Err(ArtifactError::GateUnsatisfied(format!(
                    "cleanup destination already exists: {}",
                    destination.display()
                )));
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&resolved_source, &destination)?;
        }
        other => {
            return Err(ArtifactError::GateUnsatisfied(format!(
                "unknown cleanup action type: {other}"
            )));
        }
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), ArtifactError> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn restore_cleanup_snapshot_entry(snapshot: &Path, source: &Path) -> Result<(), ArtifactError> {
    if let Some(parent) = source.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(snapshot, source)?;
    Ok(())
}

fn validate_cleanup_restore_scope(
    data_dir: &Path,
    action: &RepoCleanupAction,
    source: &Path,
    archive_root: &Path,
    workspace_root: &Path,
) -> Result<(), ArtifactError> {
    match action.action_type.as_str() {
        "remove" => validate_archive_scoped_path(source, archive_root, "cleanup restore target"),
        "move_to_archive" => {
            validate_workspace_cleanup_source_for_root(data_dir, source, workspace_root)
        }
        other => Err(ArtifactError::GateUnsatisfied(format!(
            "unknown cleanup action type: {other}"
        ))),
    }
}

fn validate_archive_scoped_path(
    path: &Path,
    archive_root: &Path,
    label: &str,
) -> Result<(), ArtifactError> {
    let resolved = if path.exists() {
        path.canonicalize()?
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| archive_root.to_path_buf())
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
    };
    validate_archive_scoped_resolved_path(&resolved, archive_root, label)
}

fn validate_archive_scoped_resolved_path(
    resolved: &Path,
    archive_root: &Path,
    label: &str,
) -> Result<(), ArtifactError> {
    if !resolved.starts_with(archive_root) {
        return Err(ArtifactError::ScopeViolation(format!(
            "{label} escaped archive scope: {}",
            resolved.display()
        )));
    }
    Ok(())
}

fn validate_archive_destination(data_dir: &Path, destination: &Path) -> Result<(), ArtifactError> {
    if destination.as_os_str().is_empty() {
        return Err(ArtifactError::GateUnsatisfied(
            "cleanup move action lacks destination".to_string(),
        ));
    }
    let archive_root = data_dir.join("archive");
    fs::create_dir_all(&archive_root)?;
    let resolved_archive_root = archive_root.canonicalize()?;
    let destination_parent = destination
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| archive_root.clone());
    fs::create_dir_all(&destination_parent)?;
    let resolved_destination_parent = destination_parent.canonicalize()?;
    if !resolved_destination_parent.starts_with(&resolved_archive_root) {
        return Err(ArtifactError::ScopeViolation(format!(
            "cleanup destination escaped archive scope: {}",
            destination.display()
        )));
    }
    Ok(())
}

fn validate_workspace_cleanup_source(
    data_dir: &Path,
    resolved_source: &Path,
) -> Result<(), ArtifactError> {
    let workspace_root = workspace_root_for_data_dir(data_dir);
    validate_workspace_cleanup_source_for_root(data_dir, resolved_source, &workspace_root)
}

fn validate_workspace_cleanup_source_for_root(
    data_dir: &Path,
    source: &Path,
    workspace_root: &Path,
) -> Result<(), ArtifactError> {
    let resolved_source = if source.exists() {
        source.canonicalize()?
    } else {
        source.to_path_buf()
    };
    let resolved_data_dir = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    if !resolved_source.starts_with(workspace_root)
        || resolved_source.starts_with(&resolved_data_dir)
    {
        return Err(ArtifactError::ScopeViolation(format!(
            "canonical cleanup source escaped public workspace scope: {}",
            source.display()
        )));
    }
    Ok(())
}

fn workspace_root_for_data_dir(data_dir: &Path) -> PathBuf {
    let resolved_data_dir = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    resolved_data_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(resolved_data_dir)
}

fn walk_paths(root: &Path) -> Result<Vec<PathBuf>, ArtifactError> {
    let mut stack = vec![root.to_path_buf()];
    let mut visited = Vec::new();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            for entry in fs::read_dir(&path)? {
                stack.push(entry?.path());
            }
        } else {
            visited.push(path);
        }
    }
    Ok(visited)
}

fn cleanup_plan_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("repo_governance").join("cleanup_plans")
}

fn cleanup_plan_path(data_dir: &Path, plan_id: &str) -> PathBuf {
    cleanup_plan_dir(data_dir).join(format!("{plan_id}.json"))
}

fn persist_cleanup_plan(
    data_dir: &Path,
    proposal: &RepoCleanupProposal,
) -> Result<(), ArtifactError> {
    fs::create_dir_all(cleanup_plan_dir(data_dir))?;
    atomic_write(
        &cleanup_plan_path(data_dir, &proposal.plan_id),
        &serde_json::to_string_pretty(proposal)?,
    )?;
    Ok(())
}

fn load_cleanup_plan(data_dir: &Path, plan_id: &str) -> Result<RepoCleanupProposal, ArtifactError> {
    let path = cleanup_plan_path(data_dir, plan_id);
    if !path.exists() {
        return Err(ArtifactError::UnknownPlan(plan_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn fingerprint_for_actions(actions: &[RepoCleanupAction]) -> String {
    actions
        .iter()
        .map(|action| {
            format!(
                "{}:{}:{}",
                action.action_type, action.target_path, action.destination_path
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn cleanup_snapshot_dir(data_dir: &Path, plan_id: &str) -> PathBuf {
    data_dir
        .join("repo_governance")
        .join("cleanup_snapshots")
        .join(plan_id)
}

fn create_cleanup_snapshot(
    data_dir: &Path,
    plan_id: &str,
    actions: &[RepoCleanupAction],
) -> Result<String, ArtifactError> {
    let snapshot_dir = cleanup_snapshot_dir(data_dir, plan_id);
    fs::create_dir_all(snapshot_dir.join("files"))?;
    let archive_root = data_dir
        .join("archive")
        .canonicalize()
        .unwrap_or_else(|_| data_dir.join("archive"));
    let workspace_root = workspace_root_for_data_dir(data_dir);
    let mut manifest_entries = Vec::new();

    for action in actions {
        let source = PathBuf::from(&action.target_path);
        let Ok(resolved_source) = source.canonicalize() else {
            continue;
        };
        validate_cleanup_snapshot_scope(
            data_dir,
            action,
            &resolved_source,
            &archive_root,
            &workspace_root,
        )?;
        if resolved_source.is_file() {
            snapshot_file_for_action(
                action,
                &resolved_source,
                &snapshot_dir,
                &archive_root,
                &workspace_root,
                &mut manifest_entries,
            )?;
        } else if resolved_source.is_dir() {
            for file in walk_paths(&resolved_source)? {
                let resolved_file = file.canonicalize()?;
                snapshot_file_for_action(
                    action,
                    &resolved_file,
                    &snapshot_dir,
                    &archive_root,
                    &workspace_root,
                    &mut manifest_entries,
                )?;
            }
        }
    }

    let manifest = serde_json::json!({
        "schema_version": "v1alpha1",
        "plan_id": plan_id,
        "created_at": timestamp_string(),
        "entries": manifest_entries
    });
    atomic_write(
        &snapshot_dir.join("manifest.json"),
        &serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(snapshot_dir.display().to_string())
}

fn validate_cleanup_snapshot_scope(
    data_dir: &Path,
    action: &RepoCleanupAction,
    resolved_source: &Path,
    archive_root: &Path,
    workspace_root: &Path,
) -> Result<(), ArtifactError> {
    match action.action_type.as_str() {
        "remove" => validate_archive_scoped_resolved_path(
            resolved_source,
            archive_root,
            "cleanup snapshot target",
        ),
        "move_to_archive" => {
            validate_workspace_cleanup_source_for_root(data_dir, resolved_source, workspace_root)?;
            validate_archive_destination(data_dir, &PathBuf::from(&action.destination_path))
        }
        other => Err(ArtifactError::GateUnsatisfied(format!(
            "unknown cleanup action type: {other}"
        ))),
    }
}

fn snapshot_file_for_action(
    action: &RepoCleanupAction,
    resolved_source: &Path,
    snapshot_dir: &Path,
    archive_root: &Path,
    workspace_root: &Path,
    manifest_entries: &mut Vec<serde_json::Value>,
) -> Result<(), ArtifactError> {
    let relative = if let Ok(relative) = resolved_source.strip_prefix(archive_root) {
        PathBuf::from("archive").join(relative)
    } else if let Ok(relative) = resolved_source.strip_prefix(workspace_root) {
        PathBuf::from("workspace").join(relative)
    } else {
        safe_snapshot_relative_path(resolved_source)
    };
    let snapshot_file = snapshot_dir.join("files").join(relative);
    if let Some(parent) = snapshot_file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(resolved_source, &snapshot_file)?;
    manifest_entries.push(serde_json::json!({
        "source_path": resolved_source.display().to_string(),
        "snapshot_path": snapshot_file.display().to_string(),
        "action_id": action.action_id
    }));
    Ok(())
}

fn safe_snapshot_relative_path(path: &Path) -> PathBuf {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => relative.push(value),
            std::path::Component::Prefix(prefix) => relative.push(prefix.as_os_str()),
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::RootDir => {}
        }
    }
    if relative.as_os_str().is_empty() {
        PathBuf::from("snapshot")
    } else {
        relative
    }
}

#[derive(Debug, Deserialize)]
struct CleanupSnapshotManifest {
    entries: Vec<CleanupSnapshotEntry>,
}

#[derive(Debug, Deserialize)]
struct CleanupSnapshotEntry {
    source_path: String,
    snapshot_path: String,
    action_id: String,
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), ArtifactError> {
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

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}
