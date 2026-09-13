use super::registry::{
    CurrentProjectPointer, ProjectRegistry, ProjectRegistryEntry, RegistryError,
};
use crate::workspace::resolve::{resolve_workspace_from, WorkspaceResolveError};
use serde::Serialize;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProject {
    pub project_id: String,
    pub workspace_root: PathBuf,
    pub workspace_hash: String,
    pub data_dir: PathBuf,
    pub resolution_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectResolutionTrace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_path: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resolution_source: String,
    pub resolution_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_project: Option<ProjectRegistryEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_pointer: Option<CurrentProjectPointer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_projects: Vec<ProjectRegistryEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProjectWithTrace {
    pub resolved: ResolvedProject,
    pub trace: ProjectResolutionTrace,
}

#[derive(Debug)]
pub struct ResolveCurrentProjectWithTraceError {
    pub error: ResolveCurrentProjectError,
    pub trace: ProjectResolutionTrace,
}

pub fn resolve_current_project(
    registry: &ProjectRegistry,
    explicit_project: Option<&str>,
    explicit_cwd: Option<&Path>,
    cwd: &Path,
) -> Result<ResolvedProject, ResolveCurrentProjectError> {
    resolve_current_project_with_trace(registry, explicit_project, explicit_cwd, cwd)
        .map(|value| value.resolved)
        .map_err(|value| value.error)
}

pub fn resolve_current_project_with_trace(
    registry: &ProjectRegistry,
    explicit_project: Option<&str>,
    explicit_cwd: Option<&Path>,
    cwd: &Path,
) -> Result<ResolvedProjectWithTrace, ResolveCurrentProjectWithTraceError> {
    let mut trace = ProjectResolutionTrace {
        requested_project_id: explicit_project.map(ToOwned::to_owned),
        requested_cwd: Some(explicit_cwd.unwrap_or(cwd).display().to_string()),
        requested_path: None,
        resolution_source: String::new(),
        resolution_status: "unresolved".to_string(),
        resolved_project_id: None,
        resolved_project: None,
        current_pointer: None,
        candidate_projects: Vec::new(),
        warnings: Vec::new(),
    };

    if let Some(project_id) = explicit_project {
        trace.resolution_source = "explicit_project".to_string();
        let entry = match registry.get_by_project_id(project_id) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                trace.warnings.push("unknown_project".to_string());
                return Err(ResolveCurrentProjectWithTraceError {
                    error: ResolveCurrentProjectError::UnknownProject(project_id.to_string()),
                    trace,
                });
            }
            Err(err) => {
                trace
                    .warnings
                    .push("registry_resolution_failed".to_string());
                return Err(ResolveCurrentProjectWithTraceError {
                    error: ResolveCurrentProjectError::Registry(err),
                    trace,
                });
            }
        };
        return Ok(resolved_with_trace(entry, "explicit_project", trace));
    }

    if let Some(explicit_cwd) = explicit_cwd {
        trace.resolution_source = "explicit_cwd".to_string();
        let binding = match resolve_workspace_from(explicit_cwd) {
            Ok(binding) => binding,
            Err(err) => {
                trace
                    .warnings
                    .push("workspace_resolution_failed".to_string());
                return Err(ResolveCurrentProjectWithTraceError {
                    error: ResolveCurrentProjectError::Workspace(err),
                    trace,
                });
            }
        };
        let entry = match registry.get_by_workspace_root(&binding.workspace_root) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                trace.warnings.push("unknown_workspace".to_string());
                return Err(ResolveCurrentProjectWithTraceError {
                    error: ResolveCurrentProjectError::UnknownWorkspace(binding.workspace_root),
                    trace,
                });
            }
            Err(err) => {
                trace
                    .warnings
                    .push("registry_resolution_failed".to_string());
                return Err(ResolveCurrentProjectWithTraceError {
                    error: ResolveCurrentProjectError::Registry(err),
                    trace,
                });
            }
        };
        return Ok(resolved_with_trace(entry, "explicit_cwd", trace));
    }

    if let Ok(binding) = resolve_workspace_from(cwd) {
        trace.resolution_source = "workspace_detection".to_string();
        match registry.get_by_workspace_root(&binding.workspace_root) {
            Ok(Some(entry)) => return Ok(resolved_with_trace(entry, "workspace_detection", trace)),
            Ok(None) => {}
            Err(err) => {
                trace
                    .warnings
                    .push("registry_resolution_failed".to_string());
                return Err(ResolveCurrentProjectWithTraceError {
                    error: ResolveCurrentProjectError::Registry(err),
                    trace,
                });
            }
        }
    }

    trace.resolution_source = "current_project_pointer".to_string();
    let pointer = match registry.get_current_project() {
        Ok(Some(pointer)) => pointer,
        Ok(None) => {
            trace.warnings.push("no_project".to_string());
            return Err(ResolveCurrentProjectWithTraceError {
                error: ResolveCurrentProjectError::NoProject,
                trace,
            });
        }
        Err(err) => {
            trace
                .warnings
                .push("registry_resolution_failed".to_string());
            return Err(ResolveCurrentProjectWithTraceError {
                error: ResolveCurrentProjectError::Registry(err),
                trace,
            });
        }
    };
    trace.current_pointer = Some(pointer.clone());

    if !pointer.workspace_root.exists() {
        trace
            .warnings
            .push("stale_current_project_pointer".to_string());
        return Err(ResolveCurrentProjectWithTraceError {
            error: ResolveCurrentProjectError::StaleCurrentProjectPointer(pointer.workspace_root),
            trace,
        });
    }

    let entry = match registry.get_by_project_id(&pointer.project_id) {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            trace.warnings.push("unknown_project".to_string());
            return Err(ResolveCurrentProjectWithTraceError {
                error: ResolveCurrentProjectError::UnknownProject(pointer.project_id),
                trace,
            });
        }
        Err(err) => {
            trace
                .warnings
                .push("registry_resolution_failed".to_string());
            return Err(ResolveCurrentProjectWithTraceError {
                error: ResolveCurrentProjectError::Registry(err),
                trace,
            });
        }
    };
    Ok(resolved_with_trace(entry, "current_project_pointer", trace))
}

fn from_entry(entry: ProjectRegistryEntry, source: &str) -> ResolvedProject {
    ResolvedProject {
        project_id: entry.project_id,
        workspace_root: entry.workspace_root,
        workspace_hash: entry.workspace_hash,
        data_dir: entry.data_dir,
        resolution_source: source.to_string(),
    }
}

fn resolved_with_trace(
    entry: ProjectRegistryEntry,
    source: &str,
    mut trace: ProjectResolutionTrace,
) -> ResolvedProjectWithTrace {
    trace.resolution_source = source.to_string();
    trace.resolution_status = "resolved".to_string();
    trace.resolved_project_id = Some(entry.project_id.clone());
    trace.resolved_project = Some(entry.clone());

    ResolvedProjectWithTrace {
        resolved: from_entry(entry, source),
        trace,
    }
}

#[derive(Debug)]
pub enum ResolveCurrentProjectError {
    Registry(RegistryError),
    Workspace(WorkspaceResolveError),
    UnknownProject(String),
    UnknownWorkspace(PathBuf),
    StaleCurrentProjectPointer(PathBuf),
    NoProject,
}

impl fmt::Display for ResolveCurrentProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(err) => write!(f, "{err}"),
            Self::Workspace(err) => write!(f, "{err}"),
            Self::UnknownProject(project_id) => {
                write!(f, "unknown project in registry: {project_id}")
            }
            Self::UnknownWorkspace(path) => {
                write!(f, "workspace is not registered: {}", path.display())
            }
            Self::StaleCurrentProjectPointer(path) => write!(
                f,
                "stale current project pointer: {} no longer exists",
                path.display()
            ),
            Self::NoProject => write!(f, "no project could be resolved from scope or pointer"),
        }
    }
}

impl std::error::Error for ResolveCurrentProjectError {}

impl From<RegistryError> for ResolveCurrentProjectError {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

impl From<WorkspaceResolveError> for ResolveCurrentProjectError {
    fn from(value: WorkspaceResolveError) -> Self {
        Self::Workspace(value)
    }
}
