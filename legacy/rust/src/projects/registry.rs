use crate::workspace::hash::hash_workspace_root;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRegistryEntry {
    pub project_id: String,
    pub workspace_root: PathBuf,
    pub workspace_hash: String,
    pub data_dir: PathBuf,
    pub last_accessed_at: String,
    pub init_state: String,
    pub active_session_id: Option<String>,
    pub open_branch_count: Option<u64>,
    pub repo_health: Option<String>,
}

impl ProjectRegistryEntry {
    pub fn new(project_id: String, workspace_root: PathBuf) -> Result<Self, RegistryError> {
        let workspace_root = workspace_root.canonicalize()?;
        let data_dir = workspace_root.join(".pmcli");
        Self::new_with_data_dir(project_id, workspace_root, data_dir)
    }

    pub fn new_with_data_dir(
        project_id: String,
        workspace_root: PathBuf,
        data_dir: PathBuf,
    ) -> Result<Self, RegistryError> {
        let workspace_root = workspace_root.canonicalize()?;
        Ok(Self {
            project_id,
            workspace_hash: hash_workspace_root(&workspace_root),
            data_dir,
            workspace_root,
            last_accessed_at: timestamp_string(),
            init_state: "registered".to_string(),
            active_session_id: None,
            open_branch_count: None,
            repo_health: None,
        })
    }

    pub fn to_pointer(&self) -> CurrentProjectPointer {
        CurrentProjectPointer {
            project_id: self.project_id.clone(),
            workspace_root: self.workspace_root.clone(),
            updated_at: timestamp_string(),
        }
    }

    pub fn with_access_update(mut self, session_id: Option<String>) -> Self {
        self.last_accessed_at = timestamp_string();
        self.active_session_id = session_id;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentProjectPointer {
    pub project_id: String,
    pub workspace_root: PathBuf,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CanonicalObjectEnvelope<T> {
    kind: String,
    version: String,
    payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct ProjectRegistryPayload {
    entries: Vec<ProjectRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegistryPruneResult {
    pub dry_run: bool,
    pub candidate_refs: Vec<String>,
    pub pruned_refs: Vec<String>,
    pub blocking_conflicts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectRegistry {
    root: PathBuf,
}

impl ProjectRegistry {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn project_data_dir(&self, project_id: &str) -> PathBuf {
        self.state_root()
            .join("projects")
            .join(sanitize_project_path_component(project_id))
    }

    pub fn entry_for_workspace(
        &self,
        project_id: String,
        workspace_root: PathBuf,
    ) -> Result<ProjectRegistryEntry, RegistryError> {
        let data_dir = self.project_data_dir(&project_id);
        ProjectRegistryEntry::new_with_data_dir(project_id, workspace_root, data_dir)
    }

    pub fn register(&self, entry: ProjectRegistryEntry) -> Result<(), RegistryError> {
        let mut payload = self.load_payload()?;

        if payload
            .entries
            .iter()
            .any(|existing| existing.project_id == entry.project_id)
        {
            payload
                .entries
                .iter_mut()
                .filter(|existing| existing.project_id == entry.project_id)
                .for_each(|existing| *existing = entry.clone());
        } else if payload
            .entries
            .iter()
            .any(|existing| existing.workspace_root == entry.workspace_root)
        {
            return Err(RegistryError::DuplicateWorkspace(entry.workspace_root));
        } else {
            payload.entries.push(entry);
        }

        self.save_payload(&payload)
    }

    pub fn touch_project(
        &self,
        project_id: &str,
        session_id: Option<String>,
    ) -> Result<ProjectRegistryEntry, RegistryError> {
        let mut payload = self.load_payload()?;
        let updated = payload
            .entries
            .iter_mut()
            .find(|entry| entry.project_id == project_id)
            .ok_or_else(|| RegistryError::UnknownProject(project_id.to_string()))?;

        updated.last_accessed_at = timestamp_string();
        updated.active_session_id = session_id.clone();
        let updated_clone = updated.clone();
        self.save_payload(&payload)?;
        self.set_current_project(updated_clone.to_pointer())?;
        Ok(updated_clone)
    }

    pub fn list(&self) -> Result<Vec<ProjectRegistryEntry>, RegistryError> {
        let mut entries = self.load_payload()?.entries;
        entries.sort_by(|left, right| right.last_accessed_at.cmp(&left.last_accessed_at));
        Ok(entries)
    }

    pub fn prune_missing(&self, apply: bool) -> Result<RegistryPruneResult, RegistryError> {
        let mut payload = self.load_payload()?;
        let candidate_project_ids: Vec<String> = payload
            .entries
            .iter()
            .filter(|entry| !entry.workspace_root.exists() || !entry.data_dir.exists())
            .map(|entry| entry.project_id.clone())
            .collect();

        let candidate_refs = candidate_project_ids
            .iter()
            .map(|project_id| format!("project:{project_id}"))
            .collect::<Vec<_>>();

        if !apply || candidate_project_ids.is_empty() {
            return Ok(RegistryPruneResult {
                dry_run: !apply,
                candidate_refs,
                pruned_refs: Vec::new(),
                blocking_conflicts: Vec::new(),
            });
        }

        payload.entries.retain(|entry| {
            !candidate_project_ids
                .iter()
                .any(|id| id == &entry.project_id)
        });
        self.save_payload(&payload)?;

        let current_pointer = self.get_current_project()?;
        if current_pointer
            .as_ref()
            .map(|pointer| {
                candidate_project_ids
                    .iter()
                    .any(|project_id| project_id == &pointer.project_id)
            })
            .unwrap_or(false)
        {
            if let Some(next_pointer) = payload
                .entries
                .first()
                .map(ProjectRegistryEntry::to_pointer)
            {
                self.set_current_project(next_pointer)?;
            } else if self.current_project_file().exists() {
                fs::remove_file(self.current_project_file())?;
            }
        }

        Ok(RegistryPruneResult {
            dry_run: false,
            candidate_refs,
            pruned_refs: candidate_project_ids
                .into_iter()
                .map(|project_id| format!("project:{project_id}"))
                .collect(),
            blocking_conflicts: Vec::new(),
        })
    }

    pub fn get_by_project_id(
        &self,
        project_id: &str,
    ) -> Result<Option<ProjectRegistryEntry>, RegistryError> {
        Ok(self
            .load_payload()?
            .entries
            .into_iter()
            .find(|entry| entry.project_id == project_id))
    }

    pub fn get_by_workspace_root(
        &self,
        workspace_root: &Path,
    ) -> Result<Option<ProjectRegistryEntry>, RegistryError> {
        let workspace_root = workspace_root.canonicalize()?;
        Ok(self
            .load_payload()?
            .entries
            .into_iter()
            .find(|entry| entry.workspace_root == workspace_root))
    }

    pub fn set_current_project(&self, pointer: CurrentProjectPointer) -> Result<(), RegistryError> {
        fs::create_dir_all(&self.root)?;
        let envelope = CanonicalObjectEnvelope {
            kind: "CurrentProjectPointer".to_string(),
            version: "v1alpha1".to_string(),
            payload: pointer,
        };
        write_atomic_json(
            self.current_project_file(),
            &serde_json::to_string_pretty(&envelope)?,
        )?;
        Ok(())
    }

    pub fn get_current_project(&self) -> Result<Option<CurrentProjectPointer>, RegistryError> {
        if !self.current_project_file().exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(self.current_project_file())?;
        let envelope: CanonicalObjectEnvelope<CurrentProjectPointer> =
            serde_json::from_str(&contents)?;
        Ok(Some(envelope.payload))
    }

    fn load_payload(&self) -> Result<ProjectRegistryPayload, RegistryError> {
        if !self.projects_file().exists() {
            return Ok(ProjectRegistryPayload::default());
        }

        let contents = fs::read_to_string(self.projects_file())?;
        let envelope: CanonicalObjectEnvelope<ProjectRegistryPayload> =
            serde_json::from_str(&contents)?;
        Ok(envelope.payload)
    }

    fn save_payload(&self, payload: &ProjectRegistryPayload) -> Result<(), RegistryError> {
        fs::create_dir_all(&self.root)?;
        let envelope = CanonicalObjectEnvelope {
            kind: "ProjectRegistry".to_string(),
            version: "v1alpha1".to_string(),
            payload,
        };
        write_atomic_json(
            self.projects_file(),
            &serde_json::to_string_pretty(&envelope)?,
        )?;
        Ok(())
    }

    fn projects_file(&self) -> PathBuf {
        self.root.join("projects.json")
    }

    fn current_project_file(&self) -> PathBuf {
        self.root.join("current_project.json")
    }

    fn state_root(&self) -> PathBuf {
        if self.root.file_name().and_then(|name| name.to_str()) == Some("registry") {
            self.root
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone())
        } else {
            self.root.clone()
        }
    }
}

#[derive(Debug)]
pub enum RegistryError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    DuplicateWorkspace(PathBuf),
    UnknownProject(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "project registry io failed: {err}"),
            Self::Serde(err) => write!(f, "project registry serialization failed: {err}"),
            Self::DuplicateWorkspace(path) => {
                write!(
                    f,
                    "duplicate workspace root in registry: {}",
                    path.display()
                )
            }
            Self::UnknownProject(project_id) => {
                write!(f, "unknown project in registry: {project_id}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<std::io::Error> for RegistryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RegistryError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}

fn sanitize_project_path_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

pub fn stable_project_id(workspace_root: &Path) -> Result<String, RegistryError> {
    let canonical = workspace_root.canonicalize()?;
    let basename = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("project");
    let slug: String = basename
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let workspace_hash = hash_workspace_root(&canonical);
    Ok(format!("proj_{slug}_{}", &workspace_hash[..12]))
}

fn write_atomic_json(path: PathBuf, contents: &str) -> Result<(), RegistryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp{}", timestamp_string()));
    fs::write(&tmp_path, contents)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}
