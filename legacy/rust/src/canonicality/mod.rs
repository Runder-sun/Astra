use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitWorktreeRecord {
    pub path: String,
    pub branch: String,
    pub head_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStateSnapshot {
    pub is_repository: bool,
    pub workspace_root: String,
    pub branch: String,
    pub head_commit: String,
    pub base_branch: String,
    pub merge_base: String,
    pub dirty_files: Vec<String>,
    pub staged_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub worktrees: Vec<GitWorktreeRecord>,
    pub ahead_count: Option<u64>,
    pub behind_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSurfaceManifest {
    pub schema_version: String,
    pub source: String,
    pub active_status_doc: String,
    pub active_architecture_doc: String,
    pub active_implementation_plan: String,
    pub active_proof_packet: String,
    pub active_code_roots: Vec<String>,
    pub active_schema_roots: Vec<String>,
    pub archive_roots: Vec<String>,
    pub external_surface_files: Vec<String>,
    pub superseded_paths: Vec<String>,
    pub forbidden_public_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeOwnerManifest {
    pub schema_version: String,
    pub source: String,
    pub active_runtime_root: String,
    pub active_command_registry: String,
    pub active_tool_registry: String,
    pub active_schema_registry: String,
    pub active_provider_layer: String,
    pub active_session_layer: String,
    pub legacy_paths: Vec<String>,
    pub forbidden_duplicate_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalLineageManifest {
    pub schema_version: String,
    pub source: String,
    pub current_generation: String,
    pub policy: String,
    pub nodes: Vec<CanonicalLineageNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalLineageNode {
    pub artifact_id: String,
    pub family: String,
    pub role: String,
    pub path: String,
    pub status: String,
    pub generation: String,
    pub derived_from: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalityAuditRecord {
    pub violation_type: String,
    pub path: String,
    pub detail: String,
    pub severity: String,
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalityAuditReport {
    pub ok: bool,
    pub project_id: String,
    pub workspace_root: String,
    pub audit_mode: String,
    pub git_state: GitStateSnapshot,
    pub canonical_surface: CanonicalSurfaceManifest,
    pub code_owners: CodeOwnerManifest,
    pub canonical_lineage: CanonicalLineageManifest,
    pub duplicate_active_docs: Vec<CanonicalityAuditRecord>,
    pub stale_docs: Vec<CanonicalityAuditRecord>,
    pub ambiguous_code_owners: Vec<CanonicalityAuditRecord>,
    pub untracked_public_files: Vec<CanonicalityAuditRecord>,
    pub orphan_generated_files: Vec<CanonicalityAuditRecord>,
    pub multiple_latest_candidates: Vec<CanonicalityAuditRecord>,
    pub stale_lineage_refs: Vec<CanonicalityAuditRecord>,
    pub superseded_public_artifacts: Vec<CanonicalityAuditRecord>,
    pub multiple_current_lineage_artifacts: Vec<CanonicalityAuditRecord>,
    pub stale_readme_claims: Vec<CanonicalityAuditRecord>,
    pub cleanup_proposals: Vec<String>,
    pub blocking_violations: Vec<CanonicalityAuditRecord>,
}

#[derive(Debug)]
pub enum CanonicalityError {
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl std::fmt::Display for CanonicalityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "canonicality IO failed: {err}"),
            Self::Serde(err) => write!(f, "canonicality serialization failed: {err}"),
        }
    }
}

impl std::error::Error for CanonicalityError {}

impl From<std::io::Error> for CanonicalityError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CanonicalityError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn audit_canonical(
    workspace_root: &Path,
    data_dir: &Path,
    project_id: &str,
) -> Result<CanonicalityAuditReport, CanonicalityError> {
    audit_canonical_with_mode(workspace_root, data_dir, project_id, false)
}

pub fn audit_canonical_with_mode(
    workspace_root: &Path,
    data_dir: &Path,
    project_id: &str,
    require_explicit_manifests: bool,
) -> Result<CanonicalityAuditReport, CanonicalityError> {
    let git_state = git_state_snapshot(workspace_root);
    let canonical_surface = load_or_derive_canonical_surface(workspace_root, data_dir)?;
    let code_owners = load_or_derive_code_owners(workspace_root, data_dir)?;
    let canonical_lineage =
        load_or_derive_canonical_lineage(workspace_root, data_dir, &canonical_surface)?;
    let duplicate_active_docs = duplicate_active_doc_records(workspace_root)?;
    let ambiguous_code_owners = ambiguous_code_owner_records(workspace_root, &code_owners);
    let untracked_public_files = untracked_public_file_records(&git_state);
    let stale_readme_claims = stale_readme_claim_records(workspace_root, &canonical_surface)?;
    let stale_lineage_refs = stale_lineage_ref_records(&canonical_lineage);
    let superseded_public_artifacts =
        superseded_public_artifact_records(workspace_root, &canonical_lineage);
    let multiple_current_lineage_artifacts =
        multiple_current_lineage_artifact_records(&canonical_lineage);
    let missing_explicit_manifests = missing_explicit_manifest_records(
        &canonical_surface,
        &code_owners,
        &canonical_lineage,
        require_explicit_manifests,
    );

    let mut blocking_violations = Vec::new();
    blocking_violations.extend(missing_explicit_manifests);
    blocking_violations.extend(duplicate_active_docs.clone());
    blocking_violations.extend(ambiguous_code_owners.clone());
    blocking_violations.extend(untracked_public_files.clone());
    blocking_violations.extend(stale_readme_claims.clone());
    blocking_violations.extend(stale_lineage_refs.clone());
    blocking_violations.extend(superseded_public_artifacts.clone());
    blocking_violations.extend(multiple_current_lineage_artifacts.clone());
    blocking_violations.sort_by(|left, right| {
        left.violation_type
            .cmp(&right.violation_type)
            .then(left.path.cmp(&right.path))
    });

    let cleanup_proposals = blocking_violations
        .iter()
        .map(|violation| violation.recommended_action.clone())
        .collect::<Vec<_>>();

    Ok(CanonicalityAuditReport {
        ok: blocking_violations.is_empty(),
        project_id: project_id.to_string(),
        workspace_root: workspace_root.display().to_string(),
        audit_mode: if require_explicit_manifests {
            "canonical_release".to_string()
        } else {
            "canonical".to_string()
        },
        git_state,
        canonical_surface,
        code_owners,
        canonical_lineage,
        duplicate_active_docs,
        stale_docs: Vec::new(),
        ambiguous_code_owners,
        untracked_public_files,
        orphan_generated_files: Vec::new(),
        multiple_latest_candidates: Vec::new(),
        stale_lineage_refs,
        superseded_public_artifacts,
        multiple_current_lineage_artifacts,
        stale_readme_claims,
        cleanup_proposals,
        blocking_violations,
    })
}

fn git_state_snapshot(workspace_root: &Path) -> GitStateSnapshot {
    let is_repository = git_output(workspace_root, &["rev-parse", "--is-inside-work-tree"])
        .map(|value| value == "true")
        .unwrap_or(false);
    if !is_repository {
        return GitStateSnapshot {
            is_repository: false,
            workspace_root: workspace_root.display().to_string(),
            branch: String::new(),
            head_commit: String::new(),
            base_branch: String::new(),
            merge_base: String::new(),
            dirty_files: Vec::new(),
            staged_files: Vec::new(),
            untracked_files: Vec::new(),
            worktrees: Vec::new(),
            ahead_count: None,
            behind_count: None,
        };
    }

    let branch = git_output(workspace_root, &["branch", "--show-current"]).unwrap_or_default();
    let head_commit = git_output(workspace_root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let base_branch = infer_base_branch(workspace_root);
    let merge_base = if base_branch.is_empty() {
        String::new()
    } else {
        git_output(workspace_root, &["merge-base", "HEAD", &base_branch]).unwrap_or_default()
    };
    let (dirty_files, staged_files, untracked_files) = git_status_files(workspace_root);

    GitStateSnapshot {
        is_repository: true,
        workspace_root: workspace_root.display().to_string(),
        branch,
        head_commit,
        base_branch,
        merge_base,
        dirty_files,
        staged_files,
        untracked_files,
        worktrees: git_worktrees(workspace_root),
        ahead_count: None,
        behind_count: None,
    }
}

fn git_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn infer_base_branch(workspace_root: &Path) -> String {
    for branch in ["master", "main"] {
        if git_output(workspace_root, &["rev-parse", "--verify", branch]).is_some() {
            return branch.to_string();
        }
    }
    String::new()
}

fn git_status_files(workspace_root: &Path) -> (Vec<String>, Vec<String>, Vec<String>) {
    let status = git_output(workspace_root, &["status", "--porcelain=v1"]).unwrap_or_default();
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
            untracked.push(path.clone());
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

fn git_worktrees(workspace_root: &Path) -> Vec<GitWorktreeRecord> {
    let output =
        git_output(workspace_root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let mut records = Vec::new();
    let mut path = String::new();
    let mut branch = String::new();
    let mut head = String::new();
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !path.is_empty() {
                records.push(GitWorktreeRecord {
                    path: path.clone(),
                    branch: branch.clone(),
                    head_commit: head.clone(),
                });
            }
            path.clear();
            branch.clear();
            head.clear();
        } else if let Some(value) = line.strip_prefix("worktree ") {
            path = value.to_string();
        } else if let Some(value) = line.strip_prefix("branch ") {
            branch = value.trim_start_matches("refs/heads/").to_string();
        } else if let Some(value) = line.strip_prefix("HEAD ") {
            head = value.to_string();
        }
    }
    records
}

fn load_or_derive_canonical_surface(
    workspace_root: &Path,
    data_dir: &Path,
) -> Result<CanonicalSurfaceManifest, CanonicalityError> {
    let manifest_path = data_dir.join("canonical_surface.json");
    if manifest_path.exists() {
        let contents = fs::read_to_string(manifest_path)?;
        let mut manifest: CanonicalSurfaceManifest = serde_json::from_str(&contents)?;
        manifest.source = "explicit".to_string();
        return Ok(manifest);
    }

    Ok(CanonicalSurfaceManifest {
        schema_version: "1".to_string(),
        source: "derived_default".to_string(),
        active_status_doc: existing_path(workspace_root, "README.md"),
        active_architecture_doc: existing_path(
            workspace_root,
            "docs/deep_study/10-revised-final-architecture.md",
        ),
        active_implementation_plan: existing_path(
            workspace_root,
            "docs/deep_study/33-advanced-systems-implementation-plan.md",
        ),
        active_proof_packet: existing_path(
            workspace_root,
            "docs/deep_study/45-base-cli-proof-execution-matrix.md",
        ),
        active_code_roots: existing_paths(
            workspace_root,
            &["src/runtime", "src/commands", "src/tools"],
        ),
        active_schema_roots: existing_paths(workspace_root, &["schemas"]),
        archive_roots: existing_paths(workspace_root, &["docs/archive", ".pmcli/archive"]),
        external_surface_files: existing_paths(workspace_root, &["README.md", "Cargo.toml"]),
        superseded_paths: Vec::new(),
        forbidden_public_patterns: vec!["final_final".to_string(), "latest_new".to_string()],
    })
}

fn load_or_derive_code_owners(
    workspace_root: &Path,
    data_dir: &Path,
) -> Result<CodeOwnerManifest, CanonicalityError> {
    let manifest_path = data_dir.join("code_owner_manifest.json");
    if manifest_path.exists() {
        let contents = fs::read_to_string(manifest_path)?;
        let mut manifest: CodeOwnerManifest = serde_json::from_str(&contents)?;
        manifest.source = "explicit".to_string();
        return Ok(manifest);
    }
    Ok(CodeOwnerManifest {
        schema_version: "1".to_string(),
        source: "derived_default".to_string(),
        active_runtime_root: existing_path(workspace_root, "src/runtime"),
        active_command_registry: existing_path(workspace_root, "src/commands/help.rs"),
        active_tool_registry: existing_path(workspace_root, "src/tools"),
        active_schema_registry: existing_path(workspace_root, "schemas"),
        active_provider_layer: existing_path(workspace_root, "src/providers"),
        active_session_layer: existing_path(workspace_root, "src/session"),
        legacy_paths: existing_paths(workspace_root, &["cmd", "internal"]),
        forbidden_duplicate_modules: vec![
            "cmd/research-cli".to_string(),
            "internal/runtime".to_string(),
        ],
    })
}

fn load_or_derive_canonical_lineage(
    workspace_root: &Path,
    data_dir: &Path,
    surface: &CanonicalSurfaceManifest,
) -> Result<CanonicalLineageManifest, CanonicalityError> {
    let manifest_path = data_dir.join("canonical_lineage.json");
    if manifest_path.exists() {
        let contents = fs::read_to_string(manifest_path)?;
        let mut manifest: CanonicalLineageManifest = serde_json::from_str(&contents)?;
        manifest.source = "explicit".to_string();
        return Ok(manifest);
    }

    let mut nodes = Vec::new();
    push_lineage_node(&mut nodes, "doc", "status", &surface.active_status_doc);
    push_lineage_node(
        &mut nodes,
        "doc",
        "architecture",
        &surface.active_architecture_doc,
    );
    push_lineage_node(
        &mut nodes,
        "doc",
        "implementation_plan",
        &surface.active_implementation_plan,
    );
    push_lineage_node(
        &mut nodes,
        "doc",
        "proof_packet",
        &surface.active_proof_packet,
    );
    for path in &surface.active_code_roots {
        push_lineage_node(&mut nodes, "code", path, path);
    }
    for path in &surface.active_schema_roots {
        push_lineage_node(&mut nodes, "schema", path, path);
    }

    let generation = git_output(workspace_root, &["rev-parse", "HEAD"])
        .filter(|head| !head.trim().is_empty())
        .unwrap_or_else(|| "derived".to_string());
    Ok(CanonicalLineageManifest {
        schema_version: "canonical_lineage.v1".to_string(),
        source: "derived_from_canonical_surface".to_string(),
        current_generation: generation,
        policy: "current_nodes_must_not_depend_on_non_current_sources".to_string(),
        nodes,
    })
}

fn push_lineage_node(nodes: &mut Vec<CanonicalLineageNode>, family: &str, role: &str, path: &str) {
    if path.trim().is_empty() {
        return;
    }
    nodes.push(CanonicalLineageNode {
        artifact_id: lineage_artifact_id(family, role, path),
        family: family.to_string(),
        role: role.to_string(),
        path: path.to_string(),
        status: "current".to_string(),
        generation: "derived".to_string(),
        derived_from: Vec::new(),
        evidence_refs: Vec::new(),
    });
}

fn lineage_artifact_id(family: &str, role: &str, path: &str) -> String {
    let normalized_path = path.replace(['/', '.'], "_");
    format!("{family}:{role}:{normalized_path}")
}

fn existing_path(workspace_root: &Path, path: &str) -> String {
    if workspace_root.join(path).exists() {
        path.to_string()
    } else {
        String::new()
    }
}

fn existing_paths(workspace_root: &Path, paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| workspace_root.join(path).exists())
        .map(|path| (*path).to_string())
        .collect()
}

fn duplicate_active_doc_records(
    workspace_root: &Path,
) -> Result<Vec<CanonicalityAuditRecord>, CanonicalityError> {
    let mut by_role: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in markdown_files(workspace_root)? {
        let relative = relative_path(workspace_root, &path);
        if is_archive_or_hidden_public_path(&relative) {
            continue;
        }
        let contents = fs::read_to_string(&path)?;
        if let Some(frame) = parse_doc_frame_metadata(&contents) {
            if frame.lifecycle == "active" && canonical_doc_role(&frame.doc_type) {
                by_role.entry(frame.doc_type).or_default().push(relative);
            }
        }
    }

    let mut records = Vec::new();
    for (doc_type, mut paths) in by_role {
        paths.sort();
        if paths.len() > 1 {
            records.push(CanonicalityAuditRecord {
                violation_type: "duplicate_active_doc".to_string(),
                path: paths.join(","),
                detail: format!(
                    "multiple active {doc_type} documents are public: {}",
                    paths.join(", ")
                ),
                severity: "blocking".to_string(),
                recommended_action: format!(
                    "Keep one active {doc_type} document and mark the others superseded or archived."
                ),
            });
        }
    }
    Ok(records)
}

fn ambiguous_code_owner_records(
    workspace_root: &Path,
    owners: &CodeOwnerManifest,
) -> Vec<CanonicalityAuditRecord> {
    owners
        .legacy_paths
        .iter()
        .filter(|path| workspace_root.join(path).exists())
        .map(|path| CanonicalityAuditRecord {
            violation_type: "ambiguous_code_owner".to_string(),
            path: path.clone(),
            detail: format!("legacy code path {path} still exists beside active Rust owners"),
            severity: "blocking".to_string(),
            recommended_action: "Move legacy code under archive or delete it after review."
                .to_string(),
        })
        .collect()
}

fn untracked_public_file_records(git_state: &GitStateSnapshot) -> Vec<CanonicalityAuditRecord> {
    if !git_state.is_repository {
        return Vec::new();
    }
    git_state
        .untracked_files
        .iter()
        .filter(|path| is_public_project_path(path))
        .map(|path| CanonicalityAuditRecord {
            violation_type: "untracked_public_file".to_string(),
            path: path.clone(),
            detail: format!("public project file {path} is untracked in git"),
            severity: "blocking".to_string(),
            recommended_action: "Track the file, move it under an archive/ignored path, or add an explicit ignore rule.".to_string(),
        })
        .collect()
}

fn missing_explicit_manifest_records(
    surface: &CanonicalSurfaceManifest,
    owners: &CodeOwnerManifest,
    lineage: &CanonicalLineageManifest,
    require_explicit_manifests: bool,
) -> Vec<CanonicalityAuditRecord> {
    if !require_explicit_manifests {
        return Vec::new();
    }
    let mut records = Vec::new();
    if surface.source != "explicit" {
        records.push(CanonicalityAuditRecord {
            violation_type: "missing_explicit_manifest".to_string(),
            path: ".pmcli/canonical_surface.json".to_string(),
            detail: "release audit requires an explicit canonical surface manifest".to_string(),
            severity: "blocking".to_string(),
            recommended_action: "Write .pmcli/canonical_surface.json before promotion or release."
                .to_string(),
        });
    }
    if owners.source != "explicit" {
        records.push(CanonicalityAuditRecord {
            violation_type: "missing_explicit_manifest".to_string(),
            path: ".pmcli/code_owner_manifest.json".to_string(),
            detail: "release audit requires an explicit code owner manifest".to_string(),
            severity: "blocking".to_string(),
            recommended_action:
                "Write .pmcli/code_owner_manifest.json before promotion or release.".to_string(),
        });
    }
    if lineage.source != "explicit" {
        records.push(CanonicalityAuditRecord {
            violation_type: "missing_explicit_manifest".to_string(),
            path: ".pmcli/canonical_lineage.json".to_string(),
            detail: "release audit requires an explicit canonical lineage manifest".to_string(),
            severity: "blocking".to_string(),
            recommended_action: "Write .pmcli/canonical_lineage.json before promotion or release."
                .to_string(),
        });
    }
    records
}

fn stale_lineage_ref_records(lineage: &CanonicalLineageManifest) -> Vec<CanonicalityAuditRecord> {
    let by_id = lineage
        .nodes
        .iter()
        .map(|node| (node.artifact_id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut records = Vec::new();
    for node in lineage.nodes.iter().filter(|node| node.status == "current") {
        for source_id in &node.derived_from {
            match by_id.get(source_id.as_str()) {
                Some(source) if source.status == "current" => {}
                Some(source) => records.push(CanonicalityAuditRecord {
                    violation_type: "stale_lineage_ref".to_string(),
                    path: node.path.clone(),
                    detail: format!(
                        "current {} `{}` derives from {} `{}` with status `{}`",
                        node.family, node.artifact_id, source.family, source.artifact_id, source.status
                    ),
                    severity: "blocking".to_string(),
                    recommended_action: format!(
                        "Regenerate `{}` from current inputs or mark it non-current.",
                        node.path
                    ),
                }),
                None => records.push(CanonicalityAuditRecord {
                    violation_type: "missing_lineage_ref".to_string(),
                    path: node.path.clone(),
                    detail: format!(
                        "current {} `{}` references missing lineage source `{source_id}`",
                        node.family, node.artifact_id
                    ),
                    severity: "blocking".to_string(),
                    recommended_action:
                        "Add the missing lineage node or regenerate the artifact from current inputs."
                            .to_string(),
                }),
            }
        }
    }
    records
}

fn superseded_public_artifact_records(
    workspace_root: &Path,
    lineage: &CanonicalLineageManifest,
) -> Vec<CanonicalityAuditRecord> {
    lineage
        .nodes
        .iter()
        .filter(|node| node.status != "current")
        .filter(|node| !node.path.trim().is_empty())
        .filter(|node| !is_archive_or_hidden_public_path(&node.path))
        .filter(|node| is_public_project_path(&node.path))
        .filter(|node| lineage_path_exists(workspace_root, &node.path))
        .map(|node| CanonicalityAuditRecord {
            violation_type: "superseded_public_artifact".to_string(),
            path: node.path.clone(),
            detail: format!(
                "{} `{}` has status `{}` but still exists on the public project surface",
                node.family, node.artifact_id, node.status
            ),
            severity: "blocking".to_string(),
            recommended_action: format!(
                "Move `{}` under archive or promote it back to current in .pmcli/canonical_lineage.json.",
                node.path
            ),
        })
        .collect()
}

fn multiple_current_lineage_artifact_records(
    lineage: &CanonicalLineageManifest,
) -> Vec<CanonicalityAuditRecord> {
    let mut by_role: BTreeMap<String, Vec<&CanonicalLineageNode>> = BTreeMap::new();
    for node in lineage.nodes.iter().filter(|node| node.status == "current") {
        let role = if node.role.trim().is_empty() {
            node.family.clone()
        } else {
            format!("{}:{}", node.family, node.role)
        };
        by_role.entry(role).or_default().push(node);
    }
    by_role
        .into_iter()
        .filter_map(|(role, mut nodes)| {
            nodes.sort_by(|left, right| left.path.cmp(&right.path));
            if nodes.len() <= 1 {
                return None;
            }
            let paths = nodes
                .iter()
                .map(|node| node.path.clone())
                .collect::<Vec<_>>();
            Some(CanonicalityAuditRecord {
                violation_type: "multiple_current_lineage_artifacts".to_string(),
                path: paths.join(","),
                detail: format!(
                    "multiple current lineage artifacts claim role `{role}`: {}",
                    paths.join(", ")
                ),
                severity: "blocking".to_string(),
                recommended_action:
                    "Keep one current lineage artifact for the role and mark the rest superseded or archived."
                        .to_string(),
            })
        })
        .collect()
}

fn lineage_path_exists(workspace_root: &Path, path: &str) -> bool {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.exists()
    } else {
        workspace_root.join(candidate).exists()
    }
}

fn stale_readme_claim_records(
    workspace_root: &Path,
    surface: &CanonicalSurfaceManifest,
) -> Result<Vec<CanonicalityAuditRecord>, CanonicalityError> {
    let readme_path = workspace_root.join("README.md");
    if !readme_path.exists() {
        return Ok(Vec::new());
    }
    let readme = fs::read_to_string(readme_path)?;
    let mut records = Vec::new();
    if readme.contains("current code only covers the Rust bootstrap floor") {
        records.push(CanonicalityAuditRecord {
            violation_type: "stale_readme_claim".to_string(),
            path: "README.md".to_string(),
            detail: "README still claims only the Rust bootstrap floor is implemented".to_string(),
            severity: "blocking".to_string(),
            recommended_action: "Refresh README from the canonical surface before promotion."
                .to_string(),
        });
    }
    if !surface.active_status_doc.is_empty() && surface.active_status_doc != "README.md" {
        records.push(CanonicalityAuditRecord {
            violation_type: "stale_readme_claim".to_string(),
            path: "README.md".to_string(),
            detail: format!(
                "README exists but canonical status doc is {}",
                surface.active_status_doc
            ),
            severity: "blocking".to_string(),
            recommended_action:
                "Make README a projection of the canonical status doc or update the manifest."
                    .to_string(),
        });
    }
    Ok(records)
}

struct ParsedDocFrameMeta {
    doc_type: String,
    lifecycle: String,
}

fn parse_doc_frame_metadata(contents: &str) -> Option<ParsedDocFrameMeta> {
    if !contents.contains("doc_frame:") {
        return None;
    }
    let mut doc_type = String::new();
    let mut lifecycle = String::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("doc_type:") {
            doc_type = value.trim().trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("lifecycle:") {
            lifecycle = value.trim().trim_matches('"').to_string();
        }
    }
    if doc_type.is_empty() || lifecycle.is_empty() {
        None
    } else {
        Some(ParsedDocFrameMeta {
            doc_type,
            lifecycle,
        })
    }
}

fn canonical_doc_role(doc_type: &str) -> bool {
    matches!(
        doc_type,
        "status" | "architecture" | "implementation_plan" | "proof_packet"
    )
}

fn markdown_files(workspace_root: &Path) -> Result<Vec<PathBuf>, CanonicalityError> {
    let mut files = Vec::new();
    collect_markdown_files(workspace_root, workspace_root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_markdown_files(
    workspace_root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), CanonicalityError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | ".pmcli" | "target") {
            continue;
        }
        if path.is_dir() {
            collect_markdown_files(workspace_root, &path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    let _ = workspace_root;
    Ok(())
}

fn relative_path(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_archive_or_hidden_public_path(path: &str) -> bool {
    path.starts_with("docs/archive/") || path.starts_with("archive/")
}

fn is_public_project_path(path: &str) -> bool {
    if path.starts_with(".pmcli/") || path.starts_with("target/") || path.starts_with(".git/") {
        return false;
    }
    path.ends_with(".md")
        || path.ends_with(".rs")
        || path.ends_with(".toml")
        || path.ends_with(".json")
        || path.starts_with("src/")
        || path.starts_with("schemas/")
        || path.starts_with("docs/")
}
