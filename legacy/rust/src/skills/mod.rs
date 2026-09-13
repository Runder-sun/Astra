use crate::artifacts::{ArtifactFamily, ArtifactSupersessionLink};
use crate::{docs, research};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{self, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SKILL_RUN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
pub struct SkillRegistryEntry {
    pub skill_id: String,
    pub description: String,
    pub manifest_path: String,
    pub source: String,
    pub enabled: bool,
    pub degraded: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub disabled_reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stage_compatibility: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillAvailabilityRecord {
    pub skill_id: String,
    pub enabled: bool,
    pub degraded: bool,
    pub disabled_reason: String,
    pub stage_compatibility: Vec<String>,
    pub dependency_health: String,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillListResult {
    pub search_paths: Vec<String>,
    pub skills: Vec<SkillRegistryEntry>,
    pub total_count: usize,
    pub degraded_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SkillListFilter {
    pub stage: Option<String>,
    pub include_disabled: bool,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillInspectResult {
    pub skill_id: String,
    pub description: String,
    pub manifest_path: String,
    pub source: String,
    pub enabled: bool,
    pub degraded: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub disabled_reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stage_compatibility: Vec<String>,
    pub dependency_health: String,
    pub availability: SkillAvailabilityRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillPathWinner {
    pub skill_id: String,
    pub winning_manifest: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillPathsResult {
    pub search_paths: Vec<String>,
    pub winners: Vec<SkillPathWinner>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillValidationIssue {
    pub skill_id: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillValidationResult {
    pub scope: String,
    pub overall_status: String,
    pub validated_count: usize,
    pub availability_records: Vec<SkillAvailabilityRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub degraded_skill_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<SkillValidationIssue>,
}

#[derive(Debug, Clone)]
pub struct SkillOutputSubmitRequest {
    pub skill_id: String,
    pub output_kind: String,
    pub artifacts: Vec<SkillOutputArtifactInput>,
    pub artifact_family: String,
    pub doc_frame_path: Option<String>,
    pub canonicality_policy: String,
    pub human_gate_required: bool,
}

#[derive(Debug, Clone)]
pub struct SkillOutputArtifactInput {
    pub artifact_path: String,
    pub artifact_kind: String,
}

#[derive(Debug, Clone)]
pub struct SkillPublishRequest {
    pub execute: bool,
    pub human_gate_approved: bool,
}

#[derive(Debug, Clone)]
pub struct SkillRunRequest {
    pub skill_id: String,
    pub runner_adapter: String,
    pub command: String,
    pub submit_request: SkillOutputSubmitRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutputArtifact {
    pub artifact_id: String,
    pub artifact_path: String,
    pub artifact_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutputEnvelope {
    pub schema_version: String,
    pub envelope_id: String,
    pub skill_id: String,
    pub skill_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub output_kind: String,
    pub visibility: String,
    pub artifact_family: String,
    pub artifact_refs: Vec<SkillOutputArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_frame_candidate: Option<docs::DocFrame>,
    pub canonicality_policy: String,
    pub human_gate_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_packet_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_envelope_ref: Option<String>,
    pub supersedes: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPublicationGate {
    pub schema_version: String,
    pub gate_id: String,
    pub envelope_id: String,
    pub canonicality_policy: String,
    pub human_gate_required: bool,
    pub human_gate_approved: bool,
    pub decision: String,
    pub decided_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillOutputListResult {
    pub outputs: Vec<SkillOutputEnvelope>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillOutputInspectionResult {
    pub envelope: SkillOutputEnvelope,
    pub publication_state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillOutputSubmitResult {
    pub status: String,
    pub envelope: SkillOutputEnvelope,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillOutputPublishResult {
    pub publication_state: String,
    pub published_envelope: SkillOutputEnvelope,
    pub publication_gate: SkillPublicationGate,
    pub artifact_family: ArtifactFamily,
    pub canonical_surface_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillSideEffectAudit {
    pub git_status_before: Vec<String>,
    pub git_status_after: Vec<String>,
    pub mutated_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillRunResult {
    pub skill_id: String,
    pub runner_adapter: String,
    pub execution_status: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub side_effect_audit: SkillSideEffectAudit,
    pub submit_result: SkillOutputSubmitResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillOutputContextEntry {
    pub envelope_id: String,
    pub artifact_id: String,
    pub artifact_path: String,
    pub doc_frame_ref: String,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvolvedSkillRecommendation {
    pub candidate_id: String,
    pub cluster_id: String,
    pub skill_id: String,
    pub title: String,
    pub trigger: String,
    pub skill_artifact_path: String,
    pub verification_status: String,
    pub publication_status: String,
    pub recommendation_reason: String,
    pub support_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillOutputContextProjection {
    pub schema_version: String,
    pub conformance_line: String,
    pub projection_policy: String,
    pub public_latest: BTreeMap<String, SkillOutputContextEntry>,
    pub evolved_recommendations: Vec<EvolvedSkillRecommendation>,
    pub review_candidate_count: usize,
    pub private_candidate_count: usize,
    pub omitted_candidate_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct ResearchSkillContract {
    #[serde(default)]
    compatible_stage_classes: Vec<String>,
    #[serde(default)]
    allowed_output_kinds: Vec<String>,
    #[serde(default)]
    doc_frame_required: bool,
    #[serde(default)]
    canonicality_policy: String,
    #[serde(default)]
    human_gate_required: bool,
    #[serde(default)]
    allowed_write_scopes: Vec<String>,
    #[serde(default)]
    runner_adapter: String,
}

#[derive(Debug, Clone)]
pub struct SkillRegistrySnapshot {
    pub search_paths: Vec<PathBuf>,
    pub entries: Vec<SkillRegistryEntry>,
}

#[derive(Debug)]
pub enum SkillRegistryError {
    UnknownSkill(String),
}

impl std::fmt::Display for SkillRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSkill(skill_id) => write!(f, "unknown skill: {skill_id}"),
        }
    }
}

impl std::error::Error for SkillRegistryError {}

#[derive(Debug)]
pub enum SkillOutputError {
    UnknownSkill(String),
    UnknownEnvelope(String),
    ScopeViolation(String),
    InvalidInput(String),
    MissingDocFrame(String),
    SkillContractViolation(String),
    RunnerFailed(String),
    PublicationGateRequired(String),
    PublicationPolicyBlocked(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl std::fmt::Display for SkillOutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSkill(skill_id) => write!(f, "unknown skill: {skill_id}"),
            Self::UnknownEnvelope(envelope_id) => {
                write!(f, "unknown skill output envelope: {envelope_id}")
            }
            Self::ScopeViolation(message) => write!(f, "{message}"),
            Self::InvalidInput(message) => write!(f, "{message}"),
            Self::MissingDocFrame(path) => write!(f, "document has no valid DocFrame: {path}"),
            Self::SkillContractViolation(message) => write!(f, "{message}"),
            Self::RunnerFailed(message) => write!(f, "{message}"),
            Self::PublicationGateRequired(message) => write!(f, "{message}"),
            Self::PublicationPolicyBlocked(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "skill output IO failed: {err}"),
            Self::Serde(err) => write!(f, "skill output serialization failed: {err}"),
        }
    }
}

impl std::error::Error for SkillOutputError {}

impl From<std::io::Error> for SkillOutputError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for SkillOutputError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn discover(state_home: &Path, cwd: &Path) -> SkillRegistrySnapshot {
    let search_paths = skill_search_paths(state_home, cwd);
    let mut winners = BTreeMap::<String, SkillRegistryEntry>::new();

    for root in &search_paths {
        let Ok(children) = fs::read_dir(root) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            if !child_path.is_dir() {
                continue;
            }
            let manifest_path = child_path.join("SKILL.md");
            if !manifest_path.exists() {
                continue;
            }
            let skill_id = child.file_name().to_string_lossy().trim().to_string();
            if skill_id.is_empty() || winners.contains_key(&skill_id) {
                continue;
            }

            let contents = fs::read_to_string(&manifest_path).unwrap_or_default();
            let degraded = contents.trim().is_empty();
            let description = skill_description_from_manifest(&contents, &skill_id);
            let disabled_reason = if degraded {
                "SKILL.md is empty".to_string()
            } else {
                String::new()
            };

            winners.insert(
                skill_id.clone(),
                SkillRegistryEntry {
                    skill_id,
                    description,
                    manifest_path: manifest_path.display().to_string(),
                    source: root.display().to_string(),
                    enabled: !degraded,
                    degraded,
                    disabled_reason,
                    stage_compatibility: default_stage_compatibility(),
                },
            );
        }
    }

    let mut entries = winners.into_values().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.skill_id.cmp(&right.skill_id));

    SkillRegistrySnapshot {
        search_paths,
        entries,
    }
}

pub fn list(snapshot: &SkillRegistrySnapshot) -> SkillListResult {
    list_with_filter(snapshot, &SkillListFilter::default())
}

pub fn list_with_filter(
    snapshot: &SkillRegistrySnapshot,
    filter: &SkillListFilter,
) -> SkillListResult {
    let skills = snapshot
        .entries
        .iter()
        .filter(|entry| filter.include_disabled || entry.enabled)
        .filter(|entry| {
            filter
                .stage
                .as_deref()
                .map(|stage| {
                    entry
                        .stage_compatibility
                        .iter()
                        .any(|candidate| candidate == stage)
                })
                .unwrap_or(true)
        })
        .filter(|entry| {
            filter
                .source
                .as_deref()
                .map(|source| entry.source == source)
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();

    SkillListResult {
        search_paths: snapshot
            .search_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        total_count: skills.len(),
        degraded_count: skills.iter().filter(|entry| entry.degraded).count(),
        skills,
    }
}

pub fn inspect(
    snapshot: &SkillRegistrySnapshot,
    skill_id: &str,
) -> Result<SkillInspectResult, SkillRegistryError> {
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.skill_id == skill_id)
        .ok_or_else(|| SkillRegistryError::UnknownSkill(skill_id.to_string()))?;

    Ok(SkillInspectResult {
        skill_id: entry.skill_id.clone(),
        description: entry.description.clone(),
        manifest_path: entry.manifest_path.clone(),
        source: entry.source.clone(),
        enabled: entry.enabled,
        degraded: entry.degraded,
        disabled_reason: entry.disabled_reason.clone(),
        stage_compatibility: entry.stage_compatibility.clone(),
        dependency_health: dependency_health(entry).to_string(),
        availability: availability_record(entry),
    })
}

pub fn paths(snapshot: &SkillRegistrySnapshot) -> SkillPathsResult {
    SkillPathsResult {
        search_paths: snapshot
            .search_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        winners: snapshot
            .entries
            .iter()
            .map(|entry| SkillPathWinner {
                skill_id: entry.skill_id.clone(),
                winning_manifest: entry.manifest_path.clone(),
                source: entry.source.clone(),
            })
            .collect(),
        total_count: snapshot.entries.len(),
    }
}

pub fn validate(
    snapshot: &SkillRegistrySnapshot,
    skill_id: Option<&str>,
) -> Result<SkillValidationResult, SkillRegistryError> {
    let selected = match skill_id {
        Some(skill_id) => vec![snapshot
            .entries
            .iter()
            .find(|entry| entry.skill_id == skill_id)
            .cloned()
            .ok_or_else(|| SkillRegistryError::UnknownSkill(skill_id.to_string()))?],
        None => snapshot.entries.clone(),
    };

    let mut issues = Vec::new();
    let mut degraded_skill_ids = Vec::new();
    for entry in &selected {
        if !Path::new(&entry.manifest_path).exists() {
            degraded_skill_ids.push(entry.skill_id.clone());
            issues.push(SkillValidationIssue {
                skill_id: entry.skill_id.clone(),
                severity: "degraded".to_string(),
                message: "manifest is missing".to_string(),
                path: entry.manifest_path.clone(),
            });
        } else if entry.degraded {
            degraded_skill_ids.push(entry.skill_id.clone());
            issues.push(SkillValidationIssue {
                skill_id: entry.skill_id.clone(),
                severity: "degraded".to_string(),
                message: entry.disabled_reason.clone(),
                path: entry.manifest_path.clone(),
            });
        }
    }

    Ok(SkillValidationResult {
        scope: skill_id.unwrap_or("all").to_string(),
        overall_status: if degraded_skill_ids.is_empty() {
            "ready".to_string()
        } else {
            "degraded".to_string()
        },
        validated_count: selected.len(),
        availability_records: selected.iter().map(availability_record).collect(),
        degraded_skill_ids,
        issues,
    })
}

pub fn list_outputs(data_dir: &Path) -> Result<SkillOutputListResult, SkillOutputError> {
    let mut outputs = Vec::new();
    let dir = skill_outputs_dir(data_dir);
    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let envelope: SkillOutputEnvelope = serde_json::from_str(&fs::read_to_string(path)?)?;
            outputs.push(envelope);
        }
    }
    outputs.sort_by(|left, right| left.envelope_id.cmp(&right.envelope_id));
    Ok(SkillOutputListResult {
        total_count: outputs.len(),
        outputs,
    })
}

pub fn submit_output(
    snapshot: &SkillRegistrySnapshot,
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
    request: SkillOutputSubmitRequest,
) -> Result<SkillOutputSubmitResult, SkillOutputError> {
    let inspected = inspect(snapshot, &request.skill_id)
        .map_err(|_| SkillOutputError::UnknownSkill(request.skill_id.clone()))?;
    submit_output_inner(
        data_dir,
        workspace_root,
        Some(project_id),
        request,
        load_research_skill_contract(&inspected.manifest_path)?,
    )
}

pub fn submit_native_output(
    data_dir: &Path,
    workspace_root: &Path,
    request: SkillOutputSubmitRequest,
) -> Result<SkillOutputSubmitResult, SkillOutputError> {
    submit_output_inner(data_dir, workspace_root, None, request, None)
}

fn submit_output_inner(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: Option<&str>,
    request: SkillOutputSubmitRequest,
    contract: Option<ResearchSkillContract>,
) -> Result<SkillOutputSubmitResult, SkillOutputError> {
    if request.output_kind.trim().is_empty() {
        return Err(SkillOutputError::InvalidInput(
            "skills output submit requires --kind".to_string(),
        ));
    }
    if request.artifact_family.trim().is_empty() {
        return Err(SkillOutputError::InvalidInput(
            "skills output submit requires --family".to_string(),
        ));
    }
    if request.artifacts.is_empty() {
        return Err(SkillOutputError::InvalidInput(
            "skills output submit requires at least one --artifact".to_string(),
        ));
    }
    validate_research_skill_contract(data_dir, project_id, &request, contract.as_ref())?;

    let mut artifact_refs = Vec::new();
    let envelope_id = format!("skill_output_{}", timestamp_string());
    let artifact_suffix = envelope_id.trim_start_matches("skill_output_");
    for (index, artifact) in request.artifacts.iter().enumerate() {
        let artifact_path =
            workspace_relative_existing_path(workspace_root, &artifact.artifact_path)?;
        artifact_refs.push(SkillOutputArtifact {
            artifact_id: format!(
                "artifact:{}:{}:{}",
                request.artifact_family,
                artifact_suffix,
                index + 1
            ),
            artifact_path,
            artifact_kind: artifact.artifact_kind.clone(),
        });
    }
    let doc_frame_candidate = match request.doc_frame_path.as_deref() {
        Some(path) => {
            let inspection = docs::inspect(workspace_root, path).map_err(|err| {
                SkillOutputError::ScopeViolation(format!("doc frame path is invalid: {err}"))
            })?;
            match (inspection.status.as_str(), inspection.doc_frame) {
                ("valid", Some(frame)) => Some(frame),
                _ => return Err(SkillOutputError::MissingDocFrame(path.to_string())),
            }
        }
        None => None,
    };

    let envelope = SkillOutputEnvelope {
        schema_version: "1".to_string(),
        envelope_id,
        skill_id: request.skill_id,
        skill_version: "unknown".to_string(),
        thread_id: None,
        stage_execution_id: None,
        operation: None,
        output_kind: request.output_kind.clone(),
        visibility: "review_candidate".to_string(),
        artifact_family: request.artifact_family,
        artifact_refs,
        doc_frame_candidate,
        canonicality_policy: request.canonicality_policy,
        human_gate_required: request.human_gate_required,
        review_packet_ref: None,
        change_envelope_ref: None,
        supersedes: Vec::new(),
        created_at: timestamp_string(),
        published_at: None,
    };
    write_envelope(data_dir, &envelope)?;
    Ok(SkillOutputSubmitResult {
        status: "candidate_recorded".to_string(),
        envelope,
    })
}

pub fn inspect_output(
    data_dir: &Path,
    envelope_id: &str,
) -> Result<SkillOutputInspectionResult, SkillOutputError> {
    let envelope = load_envelope(data_dir, envelope_id)?;
    Ok(SkillOutputInspectionResult {
        publication_state: publication_state(&envelope),
        envelope,
    })
}

pub fn publish_output(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
    envelope_id: &str,
    request: SkillPublishRequest,
) -> Result<SkillOutputPublishResult, SkillOutputError> {
    let mut envelope = load_envelope(data_dir, envelope_id)?;
    let decision = if request.execute {
        "execute"
    } else {
        "inspect"
    };
    let gate = publication_gate(&envelope, decision, request.human_gate_approved);
    write_publication_gate(data_dir, &gate)?;
    if !request.execute {
        let artifact_family = load_or_default_artifact_family(data_dir, &envelope.artifact_family)?;
        return Ok(SkillOutputPublishResult {
            publication_state: publication_state(&envelope),
            published_envelope: envelope,
            publication_gate: gate,
            artifact_family,
            canonical_surface_path: ".pmcli/canonical_surface.json".to_string(),
        });
    }
    validate_publication_policy(&envelope, request.human_gate_approved)?;

    supersede_previous_public_outputs(data_dir, &mut envelope)?;
    envelope.visibility = "canonical_public".to_string();
    envelope.published_at = Some(timestamp_string());
    let artifact_family = update_artifact_family_manifest(data_dir, &envelope)?;
    let canonical_surface_path = update_canonical_surface(data_dir, &envelope)?;
    write_envelope(data_dir, &envelope)?;
    refresh_doc_index(data_dir, workspace_root, project_id)?;

    Ok(SkillOutputPublishResult {
        publication_state: publication_state(&envelope),
        published_envelope: envelope,
        publication_gate: gate,
        artifact_family,
        canonical_surface_path,
    })
}

pub fn run_skill(
    snapshot: &SkillRegistrySnapshot,
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
    request: SkillRunRequest,
) -> Result<SkillRunResult, SkillOutputError> {
    let inspected = inspect(snapshot, &request.skill_id)
        .map_err(|_| SkillOutputError::UnknownSkill(request.skill_id.clone()))?;
    let contract = load_research_skill_contract(&inspected.manifest_path)?;
    validate_research_skill_runner(&request, contract.as_ref())?;
    validate_research_skill_contract(
        data_dir,
        Some(project_id),
        &request.submit_request,
        contract.as_ref(),
    )?;
    validate_allowed_write_scopes(workspace_root, &request.submit_request, contract.as_ref())?;

    let isolated_root = isolated_workspace_root("skill_run");
    let cleanup_root = isolated_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| isolated_root.clone());
    copy_workspace_tree(workspace_root, &isolated_root)?;
    let result = (|| {
        let before = git_status_lines(&isolated_root)?;
        let output = Command::new("sh")
            .arg("-lc")
            .arg(&request.command)
            .current_dir(&isolated_root)
            .env(
                "RESEARCH_CLI_SKILL_DIR",
                Path::new(&inspected.manifest_path)
                    .parent()
                    .unwrap_or_else(|| Path::new(".")),
            )
            .output()?;
        let after = git_status_lines(&isolated_root)?;
        let side_effect_audit = SkillSideEffectAudit {
            mutated_paths: mutated_paths(&before, &after),
            git_status_before: before,
            git_status_after: after,
        };
        validate_mutated_paths_allowed(
            &isolated_root,
            &side_effect_audit.mutated_paths,
            contract.as_ref(),
        )?;

        let exit_code = output.status.code().unwrap_or(-1);
        if !output.status.success() {
            return Err(SkillOutputError::RunnerFailed(format!(
                "skill {} runner `{}` exited with code {}",
                request.skill_id, request.runner_adapter, exit_code
            )));
        }

        materialize_declared_outputs(workspace_root, &isolated_root, &request.submit_request)?;
        let submit_result = submit_output_inner(
            data_dir,
            workspace_root,
            Some(project_id),
            request.submit_request.clone(),
            contract,
        )?;

        Ok(SkillRunResult {
            skill_id: request.skill_id,
            runner_adapter: request.runner_adapter,
            execution_status: "succeeded".to_string(),
            exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            side_effect_audit,
            submit_result,
        })
    })();
    let _ = fs::remove_dir_all(&cleanup_root);
    result
}

pub fn context_projection(
    data_dir: &Path,
) -> Result<SkillOutputContextProjection, SkillOutputError> {
    let public_latest = read_public_latest_skill_outputs(data_dir)?;
    let outputs = list_outputs(data_dir)?.outputs;
    let private_candidate_count = outputs
        .iter()
        .filter(|output| {
            output.visibility == "review_candidate" && output.canonicality_policy == "private_only"
        })
        .count();
    let review_candidate_count = outputs
        .iter()
        .filter(|output| {
            output.visibility == "review_candidate" && output.canonicality_policy != "private_only"
        })
        .count();
    let evolved_recommendations = verified_evolved_skill_recommendations(data_dir)?;
    Ok(SkillOutputContextProjection {
        schema_version: "skill_output_context_projection.v1".to_string(),
        conformance_line: "M11.skill_publication".to_string(),
        projection_policy: "public_latest_plus_verified_evolution_recommendations".to_string(),
        public_latest,
        evolved_recommendations,
        review_candidate_count,
        private_candidate_count,
        omitted_candidate_count: review_candidate_count + private_candidate_count,
    })
}

fn verified_evolved_skill_recommendations(
    data_dir: &Path,
) -> Result<Vec<EvolvedSkillRecommendation>, SkillOutputError> {
    let candidates_dir = data_dir.join("skills").join("evolution").join("candidates");
    if !candidates_dir.exists() {
        return Ok(Vec::new());
    }

    let mut recommendations = Vec::new();
    for entry in fs::read_dir(candidates_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let candidate: EvolvedSkillCandidateSnapshot =
            serde_json::from_str(&fs::read_to_string(path)?)?;
        if candidate.schema_version != "skill.evolution_candidate.v1"
            || candidate.verification_status != "verified"
        {
            continue;
        }
        recommendations.push(EvolvedSkillRecommendation {
            candidate_id: candidate.candidate_id,
            cluster_id: candidate.cluster_id,
            skill_id: candidate.skill_id,
            title: candidate.title,
            trigger: candidate.trigger,
            skill_artifact_path: candidate.skill_artifact_path,
            verification_status: candidate.verification_status,
            publication_status: candidate.publication_status,
            recommendation_reason: "verified_evolved_skill_candidate_advisory_only".to_string(),
            support_refs: candidate.support_refs,
        });
    }
    recommendations.sort_by(|left, right| {
        left.skill_id
            .cmp(&right.skill_id)
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    recommendations.truncate(5);
    Ok(recommendations)
}

#[derive(Debug, Deserialize)]
struct EvolvedSkillCandidateSnapshot {
    schema_version: String,
    candidate_id: String,
    cluster_id: String,
    skill_id: String,
    title: String,
    trigger: String,
    skill_artifact_path: String,
    support_refs: Vec<String>,
    verification_status: String,
    publication_status: String,
}

pub fn search_paths(state_home: &Path, cwd: &Path) -> Vec<String> {
    skill_search_paths(state_home, cwd)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn skill_search_paths(state_home: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        cwd.join(".pmcli").join("skills"),
        cwd.join(".codex").join("skills"),
        cwd.join(".agents").join("skills"),
        state_home.join("skills"),
    ];

    if let Ok(home) = env::var("HOME") {
        paths.push(PathBuf::from(home).join(".codex").join("skills"));
    }

    dedupe_paths(paths)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if deduped.iter().any(|existing| existing == &path) {
            continue;
        }
        deduped.push(path);
    }
    deduped
}

fn default_stage_compatibility() -> Vec<String> {
    vec![
        "ideation".to_string(),
        "planning".to_string(),
        "implementation".to_string(),
        "review".to_string(),
        "experiments".to_string(),
    ]
}

fn skill_description_from_manifest(contents: &str, skill_id: &str) -> String {
    if let Some(description) = frontmatter_description(contents) {
        return description;
    }
    if let Some(description) = first_markdown_paragraph(contents) {
        return description;
    }
    format!("Use the {skill_id} skill")
}

fn frontmatter_description(contents: &str) -> Option<String> {
    let mut lines = contents.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut in_description_block = false;
    let mut block_lines = Vec::new();
    for line in lines {
        if line == "---" {
            break;
        }
        if in_description_block {
            if line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty() {
                block_lines.push(line.trim().to_string());
                continue;
            }
            break;
        }
        if let Some(raw) = line.strip_prefix("description:") {
            let raw = raw.trim();
            if raw == "|" || raw == ">" {
                in_description_block = true;
                continue;
            }
            let description = raw.trim_matches('"').trim_matches('\'').trim().to_string();
            if !description.is_empty() {
                return Some(description);
            }
        }
    }
    let description = block_lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if description.is_empty() {
        None
    } else {
        Some(description)
    }
}

fn first_markdown_paragraph(contents: &str) -> Option<String> {
    let mut in_frontmatter = false;
    let mut frontmatter_seen = false;
    let mut paragraph = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if !frontmatter_seen && trimmed == "---" {
            in_frontmatter = true;
            frontmatter_seen = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        paragraph.push(trimmed.to_string());
    }
    let description = paragraph.join(" ");
    if description.is_empty() {
        None
    } else {
        Some(description)
    }
}

fn dependency_health(entry: &SkillRegistryEntry) -> &'static str {
    if entry.degraded {
        "degraded"
    } else {
        "ready"
    }
}

fn availability_record(entry: &SkillRegistryEntry) -> SkillAvailabilityRecord {
    SkillAvailabilityRecord {
        skill_id: entry.skill_id.clone(),
        enabled: entry.enabled,
        degraded: entry.degraded,
        disabled_reason: entry.disabled_reason.clone(),
        stage_compatibility: entry.stage_compatibility.clone(),
        dependency_health: dependency_health(entry).to_string(),
        manifest_path: entry.manifest_path.clone(),
    }
}

fn skill_outputs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("skills").join("outputs")
}

fn envelope_path(data_dir: &Path, envelope_id: &str) -> PathBuf {
    skill_outputs_dir(data_dir).join(format!("{envelope_id}.json"))
}

fn load_envelope(
    data_dir: &Path,
    envelope_id: &str,
) -> Result<SkillOutputEnvelope, SkillOutputError> {
    let path = envelope_path(data_dir, envelope_id);
    if !path.exists() {
        return Err(SkillOutputError::UnknownEnvelope(envelope_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn write_envelope(data_dir: &Path, envelope: &SkillOutputEnvelope) -> Result<(), SkillOutputError> {
    let path = envelope_path(data_dir, &envelope.envelope_id);
    write_json(&path, &serde_json::to_value(envelope)?)?;
    Ok(())
}

fn workspace_relative_existing_path(
    workspace_root: &Path,
    value: &str,
) -> Result<String, SkillOutputError> {
    let requested = Path::new(value);
    if requested.is_absolute() {
        return Err(SkillOutputError::ScopeViolation(
            "skill output artifacts must be workspace-relative".to_string(),
        ));
    }
    let path = workspace_root.join(requested);
    if !path.exists() {
        return Err(SkillOutputError::InvalidInput(format!(
            "skill output artifact does not exist: {value}"
        )));
    }
    let canonical_workspace = workspace_root.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    if !canonical_path.starts_with(&canonical_workspace) {
        return Err(SkillOutputError::ScopeViolation(
            "skill output artifact must stay inside the workspace".to_string(),
        ));
    }
    Ok(relative_path(&canonical_workspace, &canonical_path))
}

fn publication_state(envelope: &SkillOutputEnvelope) -> String {
    if envelope.visibility == "canonical_public" {
        "public_latest".to_string()
    } else if envelope.visibility == "superseded" {
        "superseded".to_string()
    } else {
        "candidate".to_string()
    }
}

fn load_research_skill_contract(
    manifest_path: &str,
) -> Result<Option<ResearchSkillContract>, SkillOutputError> {
    let path = Path::new(manifest_path)
        .parent()
        .map(|parent| parent.join("research_skill_contract.json"));
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

fn validate_research_skill_contract(
    data_dir: &Path,
    project_id: Option<&str>,
    request: &SkillOutputSubmitRequest,
    contract: Option<&ResearchSkillContract>,
) -> Result<(), SkillOutputError> {
    let Some(contract) = contract else {
        return Ok(());
    };
    validate_active_stage_compatibility(data_dir, project_id, &request.skill_id, contract)?;
    if !contract.allowed_output_kinds.is_empty()
        && !contract
            .allowed_output_kinds
            .iter()
            .any(|kind| kind == &request.output_kind)
    {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {} contract does not allow output kind `{}`",
            request.skill_id, request.output_kind
        )));
    }
    if contract.doc_frame_required && request.doc_frame_path.is_none() {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {} contract requires a DocFrame candidate",
            request.skill_id
        )));
    }
    if !contract.canonicality_policy.is_empty()
        && request.canonicality_policy != contract.canonicality_policy
    {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {} contract requires canonicality policy `{}`",
            request.skill_id, contract.canonicality_policy
        )));
    }
    if contract.human_gate_required && !request.human_gate_required {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {} contract requires a human publication gate",
            request.skill_id
        )));
    }
    validate_allowed_write_scopes(Path::new("."), request, Some(contract))?;
    Ok(())
}

fn validate_active_stage_compatibility(
    data_dir: &Path,
    project_id: Option<&str>,
    skill_id: &str,
    contract: &ResearchSkillContract,
) -> Result<(), SkillOutputError> {
    if contract.compatible_stage_classes.is_empty() {
        return Ok(());
    }
    let Some(project_id) = project_id else {
        return Ok(());
    };
    let projection = research::projection(data_dir, project_id).map_err(|err| {
        SkillOutputError::InvalidInput(format!("research context unavailable: {err}"))
    })?;
    let Some(projection) = projection else {
        return Ok(());
    };
    if projection.active_stage_class.is_empty()
        || contract
            .compatible_stage_classes
            .iter()
            .any(|stage_class| stage_class == &projection.active_stage_class)
    {
        return Ok(());
    }
    Err(SkillOutputError::SkillContractViolation(format!(
        "skill {skill_id} contract does not allow active research stage class `{}`",
        projection.active_stage_class
    )))
}

fn validate_research_skill_runner(
    request: &SkillRunRequest,
    contract: Option<&ResearchSkillContract>,
) -> Result<(), SkillOutputError> {
    if request.runner_adapter != "local-command" {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {} runner adapter `{}` is not supported",
            request.skill_id, request.runner_adapter
        )));
    }
    if let Some(contract) = contract {
        if !contract.runner_adapter.is_empty() && request.runner_adapter != contract.runner_adapter
        {
            return Err(SkillOutputError::SkillContractViolation(format!(
                "skill {} contract requires runner adapter `{}`",
                request.skill_id, contract.runner_adapter
            )));
        }
    }
    Ok(())
}

fn validate_allowed_write_scopes(
    workspace_root: &Path,
    request: &SkillOutputSubmitRequest,
    contract: Option<&ResearchSkillContract>,
) -> Result<(), SkillOutputError> {
    let Some(contract) = contract else {
        return Ok(());
    };
    if contract.allowed_write_scopes.is_empty() {
        return Ok(());
    }
    for artifact in &request.artifacts {
        validate_path_allowed_by_contract(
            workspace_root,
            &artifact.artifact_path,
            &contract.allowed_write_scopes,
            &request.skill_id,
        )?;
    }
    if let Some(path) = request.doc_frame_path.as_deref() {
        validate_path_allowed_by_contract(
            workspace_root,
            path,
            &contract.allowed_write_scopes,
            &request.skill_id,
        )?;
    }
    Ok(())
}

fn validate_mutated_paths_allowed(
    workspace_root: &Path,
    mutated_paths: &[String],
    contract: Option<&ResearchSkillContract>,
) -> Result<(), SkillOutputError> {
    let Some(contract) = contract else {
        return Ok(());
    };
    if contract.allowed_write_scopes.is_empty() {
        return Ok(());
    }
    for path in mutated_paths {
        validate_path_allowed_by_contract(
            workspace_root,
            path,
            &contract.allowed_write_scopes,
            "skill runner",
        )?;
    }
    Ok(())
}

fn validate_path_allowed_by_contract(
    workspace_root: &Path,
    path: &str,
    allowed_scopes: &[String],
    skill_id: &str,
) -> Result<(), SkillOutputError> {
    if Path::new(path).is_absolute() {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {skill_id} contract does not allow absolute output path `{path}`"
        )));
    }
    let normalized = if workspace_root == Path::new(".") {
        path.replace('\\', "/")
    } else {
        let full_path = workspace_root.join(path);
        if full_path.exists() {
            relative_path(&workspace_root.canonicalize()?, &full_path.canonicalize()?)
        } else {
            path.replace('\\', "/")
        }
    };
    let allowed = allowed_scopes.iter().any(|scope| {
        let scope = scope.trim().trim_end_matches('/').replace('\\', "/");
        normalized == scope || normalized.starts_with(&format!("{scope}/"))
    });
    if !allowed {
        return Err(SkillOutputError::SkillContractViolation(format!(
            "skill {skill_id} contract does not allow output path `{normalized}`"
        )));
    }
    Ok(())
}

fn validate_publication_policy(
    envelope: &SkillOutputEnvelope,
    human_gate_approved: bool,
) -> Result<(), SkillOutputError> {
    if matches!(
        envelope.canonicality_policy.as_str(),
        "private_only" | "candidate_only"
    ) {
        return Err(SkillOutputError::PublicationPolicyBlocked(format!(
            "skill output {} has non-public publication policy `{}`",
            envelope.envelope_id, envelope.canonicality_policy
        )));
    }
    if envelope.human_gate_required && !human_gate_approved {
        return Err(SkillOutputError::PublicationGateRequired(format!(
            "skill output {} requires explicit human gate approval",
            envelope.envelope_id
        )));
    }
    Ok(())
}

fn refresh_doc_index(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
) -> Result<(), SkillOutputError> {
    docs::index(data_dir, workspace_root, project_id)
        .map(|_| ())
        .map_err(|err| match err {
            docs::DocFrameError::Io(inner) => SkillOutputError::Io(inner),
            docs::DocFrameError::Serde(inner) => SkillOutputError::Serde(inner),
            other => SkillOutputError::InvalidInput(format!("doc index refresh failed: {other}")),
        })
}

fn isolated_workspace_root(label: &str) -> PathBuf {
    let sequence = SKILL_RUN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    env::temp_dir()
        .join(format!(
            "research_cli_{label}_{}_{}_{}",
            timestamp_string(),
            process::id(),
            sequence
        ))
        .join("workspace")
}

fn copy_workspace_tree(source_root: &Path, target_root: &Path) -> Result<(), SkillOutputError> {
    if target_root.exists() {
        fs::remove_dir_all(target_root)?;
    }
    fs::create_dir_all(target_root)?;
    copy_workspace_entry(source_root, source_root, target_root)?;
    initialize_isolated_git_baseline(target_root)?;
    Ok(())
}

fn copy_workspace_entry(
    source_root: &Path,
    current_source: &Path,
    target_root: &Path,
) -> Result<(), SkillOutputError> {
    for entry in fs::read_dir(current_source)? {
        let entry = entry?;
        let source_path = entry.path();
        let relative = source_path
            .strip_prefix(source_root)
            .unwrap_or(&source_path)
            .to_path_buf();
        if relative == Path::new("target")
            || relative == Path::new(".git")
            || relative == Path::new(".pmcli")
        {
            continue;
        }
        let target_path = target_root.join(&relative);
        if source_path.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_workspace_entry(source_root, &source_path, target_root)?;
        } else if source_path.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn materialize_declared_outputs(
    workspace_root: &Path,
    isolated_root: &Path,
    request: &SkillOutputSubmitRequest,
) -> Result<(), SkillOutputError> {
    for artifact in &request.artifacts {
        copy_back_output_file(workspace_root, isolated_root, &artifact.artifact_path)?;
    }
    if let Some(doc_frame_path) = request.doc_frame_path.as_deref() {
        copy_back_output_file(workspace_root, isolated_root, doc_frame_path)?;
    }
    Ok(())
}

fn copy_back_output_file(
    workspace_root: &Path,
    isolated_root: &Path,
    relative_path_str: &str,
) -> Result<(), SkillOutputError> {
    validate_workspace_relative_input(relative_path_str)?;
    let source_path = isolated_root.join(relative_path_str);
    if !source_path.is_file() {
        return Err(SkillOutputError::InvalidInput(format!(
            "skill output artifact does not exist: {relative_path_str}"
        )));
    }
    let target_path = workspace_root.join(relative_path_str);
    let canonical_workspace = workspace_root.canonicalize()?;
    let canonical_source = source_path.canonicalize()?;
    if !canonical_source.starts_with(isolated_root.canonicalize()?) {
        return Err(SkillOutputError::ScopeViolation(
            "skill output artifact must stay inside the isolated workspace".to_string(),
        ));
    }
    if target_path.exists() {
        let canonical_target = target_path.canonicalize()?;
        if !canonical_target.starts_with(&canonical_workspace) {
            return Err(SkillOutputError::ScopeViolation(
                "skill output artifact must not overwrite a path outside the workspace".to_string(),
            ));
        }
    }
    let canonical_parent = target_path
        .parent()
        .unwrap_or(workspace_root)
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if !canonical_parent.starts_with(&canonical_workspace) {
        return Err(SkillOutputError::ScopeViolation(
            "skill output artifact must stay inside the workspace".to_string(),
        ));
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source_path, target_path)?;
    Ok(())
}

fn validate_workspace_relative_input(path: &str) -> Result<(), SkillOutputError> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(SkillOutputError::ScopeViolation(
            "skill output artifacts must be workspace-relative and must not contain `..`"
                .to_string(),
        ));
    }
    Ok(())
}

fn initialize_isolated_git_baseline(workspace_root: &Path) -> Result<(), SkillOutputError> {
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(workspace_root)
        .output()?;
    if !init.status.success() {
        return Ok(());
    }
    for (key, value) in [
        ("user.name", "Research CLI Skill Runner"),
        ("user.email", "research-cli-skill-runner@example.test"),
    ] {
        let _ = Command::new("git")
            .args(["config", key, value])
            .current_dir(workspace_root)
            .output()?;
    }
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workspace_root)
        .output()?;
    let _ = Command::new("git")
        .args([
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "skill runner isolation baseline",
        ])
        .current_dir(workspace_root)
        .output()?;
    Ok(())
}

fn git_status_lines(workspace_root: &Path) -> Result<Vec<String>, SkillOutputError> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(workspace_root)
        .output()?;
    if !output.status.success() {
        return workspace_file_snapshot(workspace_root);
    }
    let mut lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(git_status_path)
        .collect::<Vec<_>>();
    lines.sort();
    Ok(lines)
}

fn git_status_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let path = line[3..].trim();
    let path = path
        .rsplit_once(" -> ")
        .map(|(_, renamed)| renamed)
        .unwrap_or(path)
        .trim_matches('"')
        .replace('\\', "/");
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn workspace_file_snapshot(workspace_root: &Path) -> Result<Vec<String>, SkillOutputError> {
    let mut files = Vec::new();
    collect_workspace_files(workspace_root, workspace_root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_workspace_files(
    workspace_root: &Path,
    current: &Path,
    files: &mut Vec<String>,
) -> Result<(), SkillOutputError> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name == ".git" || file_name == ".pmcli" {
            continue;
        }
        if path.is_dir() {
            collect_workspace_files(workspace_root, &path, files)?;
        } else if path.is_file() {
            files.push(relative_path(workspace_root, &path));
        }
    }
    Ok(())
}

fn mutated_paths(before: &[String], after: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    for line in after {
        if before.iter().any(|existing| existing == line) {
            continue;
        }
        if !paths.iter().any(|existing| existing == line) {
            paths.push(line.clone());
        }
    }
    paths.sort();
    paths
}

fn publication_gate(
    envelope: &SkillOutputEnvelope,
    decision: &str,
    human_gate_approved: bool,
) -> SkillPublicationGate {
    let decided_at = timestamp_string();
    SkillPublicationGate {
        schema_version: "1".to_string(),
        gate_id: format!(
            "skill_publication_gate_{}_{}",
            envelope.envelope_id, decided_at
        ),
        envelope_id: envelope.envelope_id.clone(),
        canonicality_policy: envelope.canonicality_policy.clone(),
        human_gate_required: envelope.human_gate_required,
        human_gate_approved,
        decision: decision.to_string(),
        decided_at,
    }
}

fn write_publication_gate(
    data_dir: &Path,
    gate: &SkillPublicationGate,
) -> Result<(), SkillOutputError> {
    let path = data_dir
        .join("skills")
        .join("publication_gates")
        .join(format!("{}.json", gate.gate_id));
    write_json(&path, &serde_json::to_value(gate)?)?;
    Ok(())
}

fn supersede_previous_public_outputs(
    data_dir: &Path,
    next_envelope: &mut SkillOutputEnvelope,
) -> Result<(), SkillOutputError> {
    for mut existing in list_outputs(data_dir)?.outputs {
        if existing.envelope_id == next_envelope.envelope_id
            || existing.artifact_family != next_envelope.artifact_family
            || existing.visibility != "canonical_public"
        {
            continue;
        }
        if !next_envelope
            .supersedes
            .iter()
            .any(|value| value == &existing.envelope_id)
        {
            next_envelope.supersedes.push(existing.envelope_id.clone());
        }
        existing.visibility = "superseded".to_string();
        write_envelope(data_dir, &existing)?;
    }
    Ok(())
}

fn update_artifact_family_manifest(
    data_dir: &Path,
    envelope: &SkillOutputEnvelope,
) -> Result<ArtifactFamily, SkillOutputError> {
    let artifact = envelope.artifact_refs.first().ok_or_else(|| {
        SkillOutputError::InvalidInput("skill output envelope has no artifact_refs".to_string())
    })?;
    let mut manifest = load_or_default_artifact_family(data_dir, &envelope.artifact_family)?;
    if let Some(previous) = manifest.latest_id.clone() {
        if previous != artifact.artifact_id && !manifest.archive_ids.contains(&previous) {
            manifest.archive_ids.push(previous.clone());
        }
        if previous != artifact.artifact_id {
            manifest.supersession_chain.push(ArtifactSupersessionLink {
                from_id: previous,
                to_id: artifact.artifact_id.clone(),
                reason: "skill_output_publication".to_string(),
            });
        }
    }
    manifest.canonical_id = Some(artifact.artifact_id.clone());
    manifest.latest_id = Some(artifact.artifact_id.clone());
    let path = artifact_family_manifest_path(data_dir, &envelope.artifact_family);
    write_json(&path, &serde_json::to_value(&manifest)?)?;
    Ok(manifest)
}

fn load_or_default_artifact_family(
    data_dir: &Path,
    artifact_family: &str,
) -> Result<ArtifactFamily, SkillOutputError> {
    let path = artifact_family_manifest_path(data_dir, artifact_family);
    if path.exists() {
        return Ok(serde_json::from_str(&fs::read_to_string(path)?)?);
    }
    Ok(ArtifactFamily {
        schema_version: "v1alpha1".to_string(),
        artifact_family: artifact_family.to_string(),
        canonical_id: None,
        latest_id: None,
        archive_ids: Vec::new(),
        supersession_chain: Vec::new(),
        source_branch_ids: Vec::new(),
    })
}

fn artifact_family_manifest_path(data_dir: &Path, artifact_family: &str) -> PathBuf {
    data_dir
        .join("artifacts")
        .join("families")
        .join(format!("{artifact_family}.json"))
}

fn update_canonical_surface(
    data_dir: &Path,
    envelope: &SkillOutputEnvelope,
) -> Result<String, SkillOutputError> {
    let path = data_dir.join("canonical_surface.json");
    let mut surface = if path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(&path)?)?
    } else {
        json!({})
    };
    if !surface.is_object() {
        surface = json!({});
    }

    let object = surface.as_object_mut().expect("surface object checked");
    object.insert("schema_version".to_string(), json!("1"));
    object.insert("source".to_string(), json!("skill_output_publication"));
    let skill_outputs = object
        .entry("skill_outputs".to_string())
        .or_insert_with(|| json!({}));
    if !skill_outputs.is_object() {
        *skill_outputs = json!({});
    }
    let artifact = envelope.artifact_refs.first().ok_or_else(|| {
        SkillOutputError::InvalidInput("skill output envelope has no artifact_refs".to_string())
    })?;
    skill_outputs
        .as_object_mut()
        .expect("skill_outputs object checked")
        .insert(
            envelope.artifact_family.clone(),
            json!({
                "envelope_id": envelope.envelope_id,
                "artifact_id": artifact.artifact_id,
                "artifact_path": artifact.artifact_path,
                "doc_frame_ref": envelope
                    .doc_frame_candidate
                    .as_ref()
                    .map(|frame| frame.source_path.clone())
                    .unwrap_or_default(),
                "published_at": envelope.published_at.clone().unwrap_or_default()
            }),
        );
    write_json(&path, &surface)?;
    Ok(relative_data_path(data_dir, &path))
}

fn read_public_latest_skill_outputs(
    data_dir: &Path,
) -> Result<BTreeMap<String, SkillOutputContextEntry>, SkillOutputError> {
    let path = data_dir.join("canonical_surface.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let surface: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let Some(skill_outputs) = surface.get("skill_outputs").and_then(Value::as_object) else {
        return Ok(BTreeMap::new());
    };
    let mut public_latest = BTreeMap::new();
    for (family, entry) in skill_outputs {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let read_string = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let envelope_id = read_string("envelope_id");
        if envelope_id.is_empty() {
            continue;
        }
        public_latest.insert(
            family.clone(),
            SkillOutputContextEntry {
                envelope_id,
                artifact_id: read_string("artifact_id"),
                artifact_path: read_string("artifact_path"),
                doc_frame_ref: read_string("doc_frame_ref"),
                published_at: read_string("published_at"),
            },
        );
    }
    Ok(public_latest)
}

fn write_json(path: &Path, value: &Value) -> Result<(), SkillOutputError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(temp_path, path)?;
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn relative_data_path(data_dir: &Path, path: &Path) -> String {
    format!(".pmcli/{}", relative_path(data_dir, path))
}

fn timestamp_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.to_string()
}
