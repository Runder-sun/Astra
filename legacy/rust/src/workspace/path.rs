use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum WorkspacePathError {
    AbsolutePath(PathBuf),
    NotFound(PathBuf),
    ScopeViolation {
        requested: PathBuf,
        resolved: PathBuf,
    },
    Io(std::io::Error),
}

impl fmt::Display for WorkspacePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbsolutePath(path) => {
                write!(
                    f,
                    "absolute paths are outside the workspace contract: {}",
                    path.display()
                )
            }
            Self::NotFound(path) => write!(f, "workspace path does not exist: {}", path.display()),
            Self::ScopeViolation {
                requested,
                resolved,
            } => write!(
                f,
                "workspace path escaped project scope: {} -> {}",
                requested.display(),
                resolved.display()
            ),
            Self::Io(err) => write!(f, "workspace path resolution failed: {err}"),
        }
    }
}

impl std::error::Error for WorkspacePathError {}

impl From<std::io::Error> for WorkspacePathError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn resolve_existing_workspace_path(
    workspace_root: &Path,
    target: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        return Err(WorkspacePathError::AbsolutePath(target_path.to_path_buf()));
    }

    let workspace_root = workspace_root.canonicalize()?;
    let requested = workspace_root.join(target_path);
    if !requested.exists() {
        return Err(WorkspacePathError::NotFound(requested));
    }

    let resolved = requested.canonicalize()?;
    if !resolved.starts_with(&workspace_root) {
        return Err(WorkspacePathError::ScopeViolation {
            requested,
            resolved,
        });
    }

    Ok(resolved)
}

pub fn resolve_existing_child_path(
    root: &Path,
    target: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        return Err(WorkspacePathError::AbsolutePath(target_path.to_path_buf()));
    }

    let root = root.canonicalize()?;
    let requested = root.join(target_path);
    if !requested.exists() {
        return Err(WorkspacePathError::NotFound(requested));
    }

    let resolved = requested.canonicalize()?;
    if !resolved.starts_with(&root) {
        return Err(WorkspacePathError::ScopeViolation {
            requested,
            resolved,
        });
    }

    Ok(resolved)
}
