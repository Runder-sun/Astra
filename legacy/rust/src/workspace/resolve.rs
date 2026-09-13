use super::hash::hash_workspace_root;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBinding {
    pub workspace_root: PathBuf,
    pub workspace_hash: String,
    pub data_dir: PathBuf,
}

#[derive(Debug)]
pub enum WorkspaceResolveError {
    Io(std::io::Error),
    GitCommand { action: String, stderr: String },
    ForbiddenPath(PathBuf),
    WorkspaceMarkersNotFound(PathBuf),
}

impl fmt::Display for WorkspaceResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "workspace resolution io failed: {err}"),
            Self::GitCommand { action, stderr } => {
                write!(f, "workspace git command failed during {action}: {stderr}")
            }
            Self::ForbiddenPath(path) => {
                write!(
                    f,
                    "workspace resolution refused forbidden path: {}",
                    path.display()
                )
            }
            Self::WorkspaceMarkersNotFound(path) => write!(
                f,
                "workspace markers not found while resolving from {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for WorkspaceResolveError {}

impl From<std::io::Error> for WorkspaceResolveError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn resolve_workspace_from(path: &Path) -> Result<WorkspaceBinding, WorkspaceResolveError> {
    let canonical = canonical_input_path(path)?;
    reject_forbidden_path(&canonical)?;

    let mut cursor = canonical.as_path();
    loop {
        if has_workspace_markers(cursor) {
            let workspace_root = cursor.to_path_buf();
            return Ok(WorkspaceBinding {
                workspace_hash: hash_workspace_root(&workspace_root),
                data_dir: workspace_root.join(".pmcli"),
                workspace_root,
            });
        }

        match cursor.parent() {
            Some(parent) => cursor = parent,
            None => {
                return Err(WorkspaceResolveError::WorkspaceMarkersNotFound(
                    canonical.clone(),
                ))
            }
        }
    }
}

pub fn resolve_or_create_workspace_from(
    path: &Path,
) -> Result<WorkspaceBinding, WorkspaceResolveError> {
    match resolve_workspace_from(path) {
        Ok(binding) => Ok(binding),
        Err(WorkspaceResolveError::WorkspaceMarkersNotFound(_)) => {
            let workspace_root = canonical_input_path(path)?;
            reject_forbidden_path(&workspace_root)?;
            Ok(WorkspaceBinding {
                workspace_hash: hash_workspace_root(&workspace_root),
                data_dir: workspace_root.join(".pmcli"),
                workspace_root,
            })
        }
        Err(err) => Err(err),
    }
}

pub fn ensure_project_data_dir(binding: &WorkspaceBinding) -> Result<(), WorkspaceResolveError> {
    fs::create_dir_all(&binding.data_dir)?;
    Ok(())
}

pub fn ensure_git_workspace_baseline(workspace_root: &Path) -> Result<(), WorkspaceResolveError> {
    reject_forbidden_path(workspace_root)?;
    let inside_worktree = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !inside_worktree {
        run_git_command(workspace_root, &["init", "-q"], "init")?;
    }

    let head = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|err| WorkspaceResolveError::GitCommand {
            action: "rev-parse HEAD".to_string(),
            stderr: err.to_string(),
        })?;
    if head.status.success() {
        return Ok(());
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["commit", "--allow-empty", "-m", "Astra baseline"])
        .env("GIT_AUTHOR_NAME", "Astra")
        .env("GIT_AUTHOR_EMAIL", "astra@example.invalid")
        .env("GIT_COMMITTER_NAME", "Astra")
        .env("GIT_COMMITTER_EMAIL", "astra@example.invalid")
        .output()
        .map_err(|err| WorkspaceResolveError::GitCommand {
            action: "commit baseline".to_string(),
            stderr: err.to_string(),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorkspaceResolveError::GitCommand {
            action: "commit baseline".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn run_git_command(
    workspace_root: &Path,
    args: &[&str],
    action: &str,
) -> Result<(), WorkspaceResolveError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .output()
        .map_err(|err| WorkspaceResolveError::GitCommand {
            action: action.to_string(),
            stderr: err.to_string(),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorkspaceResolveError::GitCommand {
            action: action.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn canonical_input_path(path: &Path) -> Result<PathBuf, WorkspaceResolveError> {
    let candidate = if path.is_file() {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    Ok(candidate.canonicalize()?)
}

fn has_workspace_markers(path: &Path) -> bool {
    path.join(".git").exists() || path.join(".pmcli").exists()
}

fn reject_forbidden_path(path: &Path) -> Result<(), WorkspaceResolveError> {
    let forbidden = [
        Path::new("/"),
        Path::new("/proc"),
        Path::new("/sys"),
        Path::new("/dev"),
    ];
    if forbidden.contains(&path) {
        return Err(WorkspaceResolveError::ForbiddenPath(path.to_path_buf()));
    }
    Ok(())
}
