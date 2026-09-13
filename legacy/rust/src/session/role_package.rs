use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePackage {
    pub role_profile: String,
    pub role_kind: String,
    pub skill_refs: Vec<RolePackageSkillRef>,
    pub tool_policy: RolePackageToolPolicy,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePackageSkillRef {
    pub skill_id: String,
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePackageToolPolicy {
    pub schema_version: String,
    pub source: String,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug)]
pub enum RolePackageError {
    Io { path: PathBuf, message: String },
    InvalidTools { source: String, message: String },
}

impl std::fmt::Display for RolePackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => {
                write!(
                    f,
                    "failed to read role package file {}: {message}",
                    path.display()
                )
            }
            Self::InvalidTools { source, message } => {
                write!(f, "invalid role package tool policy {source}: {message}")
            }
        }
    }
}

impl std::error::Error for RolePackageError {}

pub fn resolve_role_package(
    workspace_root: &Path,
    user_home: Option<&Path>,
    role_profile: &str,
    role_kind: &str,
) -> Result<RolePackage, RolePackageError> {
    let normalized_profile = normalize_role_component(role_profile);
    let normalized_kind = normalize_role_component(role_kind);
    let mut warnings = Vec::new();

    let skill = resolve_skill_source(
        workspace_root,
        user_home,
        &normalized_profile,
        &normalized_kind,
        role_profile,
        role_kind,
        &mut warnings,
    )?;
    let tool_policy = resolve_tool_policy_source(
        workspace_root,
        user_home,
        &normalized_profile,
        &normalized_kind,
        role_profile,
        role_kind,
        &mut warnings,
    )?;

    Ok(RolePackage {
        role_profile: role_profile.to_string(),
        role_kind: role_kind.to_string(),
        skill_refs: vec![skill],
        tool_policy,
        warnings,
    })
}

fn resolve_skill_source(
    workspace_root: &Path,
    user_home: Option<&Path>,
    normalized_profile: &str,
    normalized_kind: &str,
    role_profile: &str,
    role_kind: &str,
    warnings: &mut Vec<String>,
) -> Result<RolePackageSkillRef, RolePackageError> {
    let mut seen = BTreeSet::new();
    for path in role_package_file_candidates(
        workspace_root,
        user_home,
        normalized_profile,
        normalized_kind,
        "SKILL.md",
    ) {
        if !seen.insert(path.clone()) {
            continue;
        }
        if !path.exists() {
            continue;
        }
        let content = read_text_file(&path)?;
        let skill_id = path
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or(normalized_profile)
            .to_string();
        return Ok(RolePackageSkillRef {
            skill_id,
            source: path.display().to_string(),
            content,
        });
    }

    let (source, content) = builtin_skill_text(role_profile)
        .or_else(|| builtin_skill_text(role_kind))
        .unwrap_or_else(|| builtin_skill_text("goal_worker").expect("goal worker skill exists"));
    warnings.push(format!(
        "using built-in role package skill fallback for role_profile `{role_profile}` ({role_kind})"
    ));
    Ok(RolePackageSkillRef {
        skill_id: builtin_skill_id_from_source(source),
        source: source.to_string(),
        content: content.trim().to_string(),
    })
}

fn resolve_tool_policy_source(
    workspace_root: &Path,
    user_home: Option<&Path>,
    normalized_profile: &str,
    normalized_kind: &str,
    role_profile: &str,
    role_kind: &str,
    warnings: &mut Vec<String>,
) -> Result<RolePackageToolPolicy, RolePackageError> {
    let mut seen = BTreeSet::new();
    for path in role_package_file_candidates(
        workspace_root,
        user_home,
        normalized_profile,
        normalized_kind,
        "TOOLS.json",
    ) {
        if !seen.insert(path.clone()) {
            continue;
        }
        if !path.exists() {
            continue;
        }
        let contents = read_text_file(&path)?;
        let policy = parse_tool_policy(path.display().to_string(), &contents)?;
        return Ok(policy);
    }

    let (source, content) = builtin_tools_text(role_profile)
        .or_else(|| builtin_tools_text(role_kind))
        .unwrap_or_else(|| builtin_tools_text("goal_worker").expect("goal worker tools exist"));
    warnings.push(format!(
        "using built-in role package tool fallback for role_profile `{role_profile}` ({role_kind})"
    ));
    parse_tool_policy(source.to_string(), content)
}

fn role_package_file_candidates(
    workspace_root: &Path,
    user_home: Option<&Path>,
    normalized_profile: &str,
    normalized_kind: &str,
    file_name: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = user_home {
        candidates.push(
            home.join(".research-cli")
                .join("roles")
                .join(normalized_profile)
                .join(file_name),
        );
        candidates.push(
            home.join(".research-cli")
                .join("roles")
                .join(normalized_kind)
                .join(file_name),
        );
        candidates.push(
            home.join(".config")
                .join("research-cli")
                .join("roles")
                .join(normalized_profile)
                .join(file_name),
        );
        candidates.push(
            home.join(".config")
                .join("research-cli")
                .join("roles")
                .join(normalized_kind)
                .join(file_name),
        );
    }
    candidates.push(
        workspace_root
            .join("roles")
            .join(normalized_profile)
            .join(file_name),
    );
    candidates.push(
        workspace_root
            .join("roles")
            .join(normalized_kind)
            .join(file_name),
    );
    candidates.push(
        workspace_root
            .join(".research-cli")
            .join("roles")
            .join(normalized_profile)
            .join(file_name),
    );
    candidates.push(
        workspace_root
            .join(".research-cli")
            .join("roles")
            .join(normalized_kind)
            .join(file_name),
    );
    candidates
}

fn read_text_file(path: &Path) -> Result<String, RolePackageError> {
    fs::read_to_string(path).map_err(|err| RolePackageError::Io {
        path: path.to_path_buf(),
        message: err.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct RawRoleToolPolicy {
    schema_version: Option<String>,
    allowed_tools: Vec<String>,
}

fn parse_tool_policy(
    source: String,
    contents: &str,
) -> Result<RolePackageToolPolicy, RolePackageError> {
    let raw: RawRoleToolPolicy =
        serde_json::from_str(contents).map_err(|err| RolePackageError::InvalidTools {
            source: source.clone(),
            message: err.to_string(),
        })?;
    if raw.allowed_tools.is_empty() {
        return Err(RolePackageError::InvalidTools {
            source,
            message: "allowed_tools must not be empty".to_string(),
        });
    }
    Ok(RolePackageToolPolicy {
        schema_version: raw
            .schema_version
            .unwrap_or_else(|| "agent_tool_policy.v1".to_string()),
        source,
        allowed_tools: raw.allowed_tools,
    })
}

fn builtin_skill_id_from_source(source: &str) -> String {
    source
        .trim_end_matches("/SKILL.md")
        .rsplit('/')
        .next()
        .unwrap_or("general_worker_execution")
        .to_string()
}

fn normalize_role_component(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if normalized.is_empty() {
        "goal_worker".to_string()
    } else {
        normalized
    }
}

fn builtin_skill_text(role: &str) -> Option<(&'static str, &'static str)> {
    match role {
        "stage_standard_setter" => Some((
            "builtin://roles/stage_standard_setter/SKILL.md",
            include_str!("role_packages/stage_standard_setter/SKILL.md"),
        )),
        "goal_dispatcher" => Some((
            "builtin://roles/goal_dispatcher/SKILL.md",
            include_str!("role_packages/goal_dispatcher/SKILL.md"),
        )),
        "research_operator" => Some((
            "builtin://roles/research_operator/SKILL.md",
            include_str!("role_packages/research_operator/SKILL.md"),
        )),
        "research_worker" => Some((
            "builtin://roles/research_worker/SKILL.md",
            include_str!("role_packages/research_worker/SKILL.md"),
        )),
        "research_synthesizer" => Some((
            "builtin://roles/research_synthesizer/SKILL.md",
            include_str!("role_packages/research_synthesizer/SKILL.md"),
        )),
        "literature_researcher" => Some((
            "builtin://roles/literature_researcher/SKILL.md",
            include_str!("role_packages/literature_researcher/SKILL.md"),
        )),
        "citation_auditor" => Some((
            "builtin://roles/citation_auditor/SKILL.md",
            include_str!("role_packages/citation_auditor/SKILL.md"),
        )),
        "literature_comparison_researcher" => Some((
            "builtin://roles/literature_comparison_researcher/SKILL.md",
            include_str!("role_packages/literature_comparison_researcher/SKILL.md"),
        )),
        "literature_gap_analyst" => Some((
            "builtin://roles/literature_gap_analyst/SKILL.md",
            include_str!("role_packages/literature_gap_analyst/SKILL.md"),
        )),
        "research_planner" => Some((
            "builtin://roles/research_planner/SKILL.md",
            include_str!("role_packages/research_planner/SKILL.md"),
        )),
        "research_critic" => Some((
            "builtin://roles/research_critic/SKILL.md",
            include_str!("role_packages/research_critic/SKILL.md"),
        )),
        "novelty_reviewer" => Some((
            "builtin://roles/novelty_reviewer/SKILL.md",
            include_str!("role_packages/novelty_reviewer/SKILL.md"),
        )),
        "experiment_designer" => Some((
            "builtin://roles/experiment_designer/SKILL.md",
            include_str!("role_packages/experiment_designer/SKILL.md"),
        )),
        "code_reviewer" => Some((
            "builtin://roles/code_reviewer/SKILL.md",
            include_str!("role_packages/code_reviewer/SKILL.md"),
        )),
        "experiment_operator" => Some((
            "builtin://roles/experiment_operator/SKILL.md",
            include_str!("role_packages/experiment_operator/SKILL.md"),
        )),
        "result_analyst" => Some((
            "builtin://roles/result_analyst/SKILL.md",
            include_str!("role_packages/result_analyst/SKILL.md"),
        )),
        "claim_auditor" => Some((
            "builtin://roles/claim_auditor/SKILL.md",
            include_str!("role_packages/claim_auditor/SKILL.md"),
        )),
        "paper_planner" => Some((
            "builtin://roles/paper_planner/SKILL.md",
            include_str!("role_packages/paper_planner/SKILL.md"),
        )),
        "latex_writer" => Some((
            "builtin://roles/latex_writer/SKILL.md",
            include_str!("role_packages/latex_writer/SKILL.md"),
        )),
        "paper_build_operator" => Some((
            "builtin://roles/paper_build_operator/SKILL.md",
            include_str!("role_packages/paper_build_operator/SKILL.md"),
        )),
        "hard_reviewer" => Some((
            "builtin://roles/hard_reviewer/SKILL.md",
            include_str!("role_packages/hard_reviewer/SKILL.md"),
        )),
        "reviewer" => Some((
            "builtin://roles/reviewer/SKILL.md",
            include_str!("role_packages/reviewer/SKILL.md"),
        )),
        "repair_router" => Some((
            "builtin://roles/repair_router/SKILL.md",
            include_str!("role_packages/repair_router/SKILL.md"),
        )),
        "process_diagnostician" => Some((
            "builtin://roles/process_diagnostician/SKILL.md",
            include_str!("role_packages/process_diagnostician/SKILL.md"),
        )),
        "implementation_worker" => Some((
            "builtin://roles/implementation_worker/SKILL.md",
            include_str!("role_packages/implementation_worker/SKILL.md"),
        )),
        "literature_repair_worker" => Some((
            "builtin://roles/literature_repair_worker/SKILL.md",
            include_str!("role_packages/literature_repair_worker/SKILL.md"),
        )),
        "novelty_repair_reviewer" => Some((
            "builtin://roles/novelty_repair_reviewer/SKILL.md",
            include_str!("role_packages/novelty_repair_reviewer/SKILL.md"),
        )),
        "experiment_plan_repair_worker" => Some((
            "builtin://roles/experiment_plan_repair_worker/SKILL.md",
            include_str!("role_packages/experiment_plan_repair_worker/SKILL.md"),
        )),
        "implementation_repair_worker" => Some((
            "builtin://roles/implementation_repair_worker/SKILL.md",
            include_str!("role_packages/implementation_repair_worker/SKILL.md"),
        )),
        "experiment_run_repair_worker" => Some((
            "builtin://roles/experiment_run_repair_worker/SKILL.md",
            include_str!("role_packages/experiment_run_repair_worker/SKILL.md"),
        )),
        "result_analysis_repair_worker" => Some((
            "builtin://roles/result_analysis_repair_worker/SKILL.md",
            include_str!("role_packages/result_analysis_repair_worker/SKILL.md"),
        )),
        "claim_repair_auditor" => Some((
            "builtin://roles/claim_repair_auditor/SKILL.md",
            include_str!("role_packages/claim_repair_auditor/SKILL.md"),
        )),
        "latex_repair_writer" => Some((
            "builtin://roles/latex_repair_writer/SKILL.md",
            include_str!("role_packages/latex_repair_writer/SKILL.md"),
        )),
        "paper_build_repair_operator" => Some((
            "builtin://roles/paper_build_repair_operator/SKILL.md",
            include_str!("role_packages/paper_build_repair_operator/SKILL.md"),
        )),
        "hard_review_repair_router" => Some((
            "builtin://roles/hard_review_repair_router/SKILL.md",
            include_str!("role_packages/hard_review_repair_router/SKILL.md"),
        )),
        "stage_repair_worker" => Some((
            "builtin://roles/stage_repair_worker/SKILL.md",
            include_str!("role_packages/stage_repair_worker/SKILL.md"),
        )),
        "repair_worker" => Some((
            "builtin://roles/repair_worker/SKILL.md",
            include_str!("role_packages/repair_worker/SKILL.md"),
        )),
        "goal_worker" => Some((
            "builtin://roles/goal_worker/SKILL.md",
            include_str!("role_packages/goal_worker/SKILL.md"),
        )),
        _ => None,
    }
}

fn builtin_tools_text(role: &str) -> Option<(&'static str, &'static str)> {
    match role {
        "stage_standard_setter" => Some((
            "builtin://roles/stage_standard_setter/TOOLS.json",
            include_str!("role_packages/stage_standard_setter/TOOLS.json"),
        )),
        "goal_dispatcher" => Some((
            "builtin://roles/goal_dispatcher/TOOLS.json",
            include_str!("role_packages/goal_dispatcher/TOOLS.json"),
        )),
        "research_operator" => Some((
            "builtin://roles/research_operator/TOOLS.json",
            include_str!("role_packages/research_operator/TOOLS.json"),
        )),
        "research_worker" => Some((
            "builtin://roles/research_worker/TOOLS.json",
            include_str!("role_packages/research_worker/TOOLS.json"),
        )),
        "research_synthesizer" => Some((
            "builtin://roles/research_synthesizer/TOOLS.json",
            include_str!("role_packages/research_synthesizer/TOOLS.json"),
        )),
        "literature_researcher" => Some((
            "builtin://roles/literature_researcher/TOOLS.json",
            include_str!("role_packages/literature_researcher/TOOLS.json"),
        )),
        "citation_auditor" => Some((
            "builtin://roles/citation_auditor/TOOLS.json",
            include_str!("role_packages/citation_auditor/TOOLS.json"),
        )),
        "literature_comparison_researcher" => Some((
            "builtin://roles/literature_comparison_researcher/TOOLS.json",
            include_str!("role_packages/literature_comparison_researcher/TOOLS.json"),
        )),
        "literature_gap_analyst" => Some((
            "builtin://roles/literature_gap_analyst/TOOLS.json",
            include_str!("role_packages/literature_gap_analyst/TOOLS.json"),
        )),
        "research_planner" => Some((
            "builtin://roles/research_planner/TOOLS.json",
            include_str!("role_packages/research_planner/TOOLS.json"),
        )),
        "research_critic" => Some((
            "builtin://roles/research_critic/TOOLS.json",
            include_str!("role_packages/research_critic/TOOLS.json"),
        )),
        "novelty_reviewer" => Some((
            "builtin://roles/novelty_reviewer/TOOLS.json",
            include_str!("role_packages/novelty_reviewer/TOOLS.json"),
        )),
        "experiment_designer" => Some((
            "builtin://roles/experiment_designer/TOOLS.json",
            include_str!("role_packages/experiment_designer/TOOLS.json"),
        )),
        "code_reviewer" => Some((
            "builtin://roles/code_reviewer/TOOLS.json",
            include_str!("role_packages/code_reviewer/TOOLS.json"),
        )),
        "experiment_operator" => Some((
            "builtin://roles/experiment_operator/TOOLS.json",
            include_str!("role_packages/experiment_operator/TOOLS.json"),
        )),
        "result_analyst" => Some((
            "builtin://roles/result_analyst/TOOLS.json",
            include_str!("role_packages/result_analyst/TOOLS.json"),
        )),
        "claim_auditor" => Some((
            "builtin://roles/claim_auditor/TOOLS.json",
            include_str!("role_packages/claim_auditor/TOOLS.json"),
        )),
        "paper_planner" => Some((
            "builtin://roles/paper_planner/TOOLS.json",
            include_str!("role_packages/paper_planner/TOOLS.json"),
        )),
        "latex_writer" => Some((
            "builtin://roles/latex_writer/TOOLS.json",
            include_str!("role_packages/latex_writer/TOOLS.json"),
        )),
        "paper_build_operator" => Some((
            "builtin://roles/paper_build_operator/TOOLS.json",
            include_str!("role_packages/paper_build_operator/TOOLS.json"),
        )),
        "hard_reviewer" => Some((
            "builtin://roles/hard_reviewer/TOOLS.json",
            include_str!("role_packages/hard_reviewer/TOOLS.json"),
        )),
        "reviewer" => Some((
            "builtin://roles/reviewer/TOOLS.json",
            include_str!("role_packages/reviewer/TOOLS.json"),
        )),
        "repair_router" => Some((
            "builtin://roles/repair_router/TOOLS.json",
            include_str!("role_packages/repair_router/TOOLS.json"),
        )),
        "process_diagnostician" => Some((
            "builtin://roles/process_diagnostician/TOOLS.json",
            include_str!("role_packages/process_diagnostician/TOOLS.json"),
        )),
        "implementation_worker" => Some((
            "builtin://roles/implementation_worker/TOOLS.json",
            include_str!("role_packages/implementation_worker/TOOLS.json"),
        )),
        "literature_repair_worker" => Some((
            "builtin://roles/literature_repair_worker/TOOLS.json",
            include_str!("role_packages/literature_repair_worker/TOOLS.json"),
        )),
        "novelty_repair_reviewer" => Some((
            "builtin://roles/novelty_repair_reviewer/TOOLS.json",
            include_str!("role_packages/novelty_repair_reviewer/TOOLS.json"),
        )),
        "experiment_plan_repair_worker" => Some((
            "builtin://roles/experiment_plan_repair_worker/TOOLS.json",
            include_str!("role_packages/experiment_plan_repair_worker/TOOLS.json"),
        )),
        "implementation_repair_worker" => Some((
            "builtin://roles/implementation_repair_worker/TOOLS.json",
            include_str!("role_packages/implementation_repair_worker/TOOLS.json"),
        )),
        "experiment_run_repair_worker" => Some((
            "builtin://roles/experiment_run_repair_worker/TOOLS.json",
            include_str!("role_packages/experiment_run_repair_worker/TOOLS.json"),
        )),
        "result_analysis_repair_worker" => Some((
            "builtin://roles/result_analysis_repair_worker/TOOLS.json",
            include_str!("role_packages/result_analysis_repair_worker/TOOLS.json"),
        )),
        "claim_repair_auditor" => Some((
            "builtin://roles/claim_repair_auditor/TOOLS.json",
            include_str!("role_packages/claim_repair_auditor/TOOLS.json"),
        )),
        "latex_repair_writer" => Some((
            "builtin://roles/latex_repair_writer/TOOLS.json",
            include_str!("role_packages/latex_repair_writer/TOOLS.json"),
        )),
        "paper_build_repair_operator" => Some((
            "builtin://roles/paper_build_repair_operator/TOOLS.json",
            include_str!("role_packages/paper_build_repair_operator/TOOLS.json"),
        )),
        "hard_review_repair_router" => Some((
            "builtin://roles/hard_review_repair_router/TOOLS.json",
            include_str!("role_packages/hard_review_repair_router/TOOLS.json"),
        )),
        "stage_repair_worker" => Some((
            "builtin://roles/stage_repair_worker/TOOLS.json",
            include_str!("role_packages/stage_repair_worker/TOOLS.json"),
        )),
        "repair_worker" => Some((
            "builtin://roles/repair_worker/TOOLS.json",
            include_str!("role_packages/repair_worker/TOOLS.json"),
        )),
        "goal_worker" => Some((
            "builtin://roles/goal_worker/TOOLS.json",
            include_str!("role_packages/goal_worker/TOOLS.json"),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "research_cli_role_package_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("workspace should be created");
        root
    }

    #[test]
    fn builtin_role_package_resolves_skill_and_tools_by_profile() {
        let workspace_root = unique_test_workspace("builtin");

        let literature = resolve_role_package(
            &workspace_root,
            None,
            "literature_researcher",
            "research_worker",
        )
        .expect("literature role package should resolve");
        assert_eq!(literature.skill_refs[0].skill_id, "literature_researcher");
        assert!(literature.skill_refs[0]
            .source
            .contains("builtin://roles/literature_researcher/SKILL.md"));
        assert!(literature
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "fetch"));
        assert!(literature
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "paper_search"));
        assert!(literature.skill_refs[0]
            .content
            .contains("Do not guess arXiv identifiers"));
        assert!(!literature
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "apply_patch"));

        let stage_standard = resolve_role_package(
            &workspace_root,
            None,
            "stage_standard_setter",
            "goal_worker",
        )
        .expect("stage standard setter role package should resolve");
        assert_eq!(
            stage_standard.skill_refs[0].skill_id,
            "stage_standard_setter"
        );
        assert!(stage_standard.skill_refs[0]
            .source
            .contains("builtin://roles/stage_standard_setter/SKILL.md"));
        assert!(stage_standard.skill_refs[0]
            .content
            .contains("expert-level pass criteria"));
        assert!(!stage_standard
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "worker_shell"));

        let synthesizer = resolve_role_package(
            &workspace_root,
            None,
            "research_synthesizer",
            "research_worker",
        )
        .expect("research synthesizer role package should resolve");
        assert_eq!(synthesizer.skill_refs[0].skill_id, "research_synthesizer");
        assert!(synthesizer.skill_refs[0]
            .source
            .contains("builtin://roles/research_synthesizer/SKILL.md"));
        assert!(synthesizer.skill_refs[0]
            .content
            .contains("Synthesize already accepted stage-local worker evidence"));
        assert!(synthesizer
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "write_file"));
        assert!(!synthesizer
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "paper_search"));

        let implementation = resolve_role_package(
            &workspace_root,
            None,
            "implementation_worker",
            "implementation_worker",
        )
        .expect("implementation role package should resolve");
        assert!(implementation
            .tool_policy
            .allowed_tools
            .iter()
            .any(|tool| tool == "apply_patch"));
    }

    #[test]
    fn project_role_package_overrides_builtin_skill_and_tools() {
        let workspace_root = unique_test_workspace("project_override");
        let role_dir = workspace_root
            .join(".research-cli")
            .join("roles")
            .join("literature_researcher");
        fs::create_dir_all(&role_dir).expect("role package dir should write");
        fs::write(
            role_dir.join("SKILL.md"),
            "# Project Skill\n\nProject-specific literature method.",
        )
        .expect("skill should write");
        fs::write(
            role_dir.join("TOOLS.json"),
            r#"{
  "schema_version": "agent_tool_policy.v1",
  "allowed_tools": ["read_file", "fetch"]
}"#,
        )
        .expect("tools should write");

        let package = resolve_role_package(
            &workspace_root,
            None,
            "literature_researcher",
            "research_worker",
        )
        .expect("project role package should resolve");

        assert!(package.skill_refs[0].content.contains("Project-specific"));
        assert!(package.skill_refs[0]
            .source
            .ends_with(".research-cli/roles/literature_researcher/SKILL.md"));
        assert_eq!(
            package.tool_policy.allowed_tools,
            vec!["read_file".to_string(), "fetch".to_string()]
        );
    }
}
