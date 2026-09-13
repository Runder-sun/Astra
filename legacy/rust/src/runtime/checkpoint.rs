use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectState {
    pub project_id: String,
    pub workspace_root: String,
    pub workspace_hash: String,
    pub protocol_version: String,
    pub active_session_id: Option<String>,
    pub current_permission_mode: Option<String>,
    pub mission_frame_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRuntimeState {
    pub session_id: Option<String>,
    pub status: String,
    pub transcript_path: Option<String>,
    pub summary_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnRuntimeState {
    pub turn_id: Option<String>,
    pub status: String,
    pub active_provider_route: Option<String>,
    pub pending_permission_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntimeState {
    pub status: String,
    pub active_agent_ids: Vec<String>,
    pub pending_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRuntimeState {
    pub status: String,
    pub open_review_ids: Vec<String>,
    pub pending_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRuntimeState {
    pub status: String,
    pub working_set_count: u64,
    pub pending_queue_count: u64,
    pub trusted_count: u64,
    pub contested_count: u64,
    pub pending_promotion_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRuntimeState {
    pub status: String,
    pub repo_health: Option<String>,
    pub cleanup_proposal_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRuntimeState {
    pub status: String,
    pub family_count: u64,
    pub canonical_family_count: u64,
    pub latest_promotion_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOpsRuntimeState {
    pub status: String,
    pub last_digest_ref: Option<String>,
    pub pending_research_runs: u64,
    pub cleanup_queue_count: u64,
    pub active_tick_id: Option<String>,
    pub pending_digest_count: u64,
    pub wake_queue_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRuntimeState {
    pub status: String,
    pub pending_request_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureRuntimeState {
    pub status: String,
    pub degraded_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackRuntimeState {
    pub status: String,
    pub last_restore_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRuntimeState {
    pub status: String,
    pub active_batch_ids: Vec<String>,
    pub promotable_branch_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRuntimeState {
    pub status: String,
    pub active_control_session_id: Option<String>,
    pub connected_client_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelStateBundle {
    pub project_state: ProjectState,
    pub session_state: SessionRuntimeState,
    pub turn_state: TurnRuntimeState,
    pub agent_state: AgentRuntimeState,
    pub review_state: ReviewRuntimeState,
    pub memory_state: MemoryRuntimeState,
    pub repo_state: RepoRuntimeState,
    pub artifact_state: ArtifactRuntimeState,
    pub projectops_state: ProjectOpsRuntimeState,
    pub permission_state: PermissionRuntimeState,
    pub feature_state: FeatureRuntimeState,
    pub rollback_state: RollbackRuntimeState,
    pub branch_state: BranchRuntimeState,
    pub remote_state: RemoteRuntimeState,
    pub seq_cursor: u64,
    pub checkpoint_epoch: u64,
}

impl Default for AgentRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            active_agent_ids: Vec::new(),
            pending_count: 0,
        }
    }
}

impl Default for ReviewRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            open_review_ids: Vec::new(),
            pending_count: 0,
        }
    }
}

impl Default for MemoryRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            working_set_count: 0,
            pending_queue_count: 0,
            trusted_count: 0,
            contested_count: 0,
            pending_promotion_count: 0,
        }
    }
}

impl Default for RepoRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            repo_health: None,
            cleanup_proposal_ref: None,
        }
    }
}

impl Default for ArtifactRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            family_count: 0,
            canonical_family_count: 0,
            latest_promotion_ref: None,
        }
    }
}

impl Default for ProjectOpsRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            last_digest_ref: None,
            pending_research_runs: 0,
            cleanup_queue_count: 0,
            active_tick_id: None,
            pending_digest_count: 0,
            wake_queue_count: 0,
        }
    }
}

impl Default for PermissionRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            pending_request_count: 0,
        }
    }
}

impl Default for FeatureRuntimeState {
    fn default() -> Self {
        Self {
            status: "ready".to_string(),
            degraded_features: Vec::new(),
        }
    }
}

impl Default for RollbackRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            last_restore_ref: None,
        }
    }
}

impl Default for BranchRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            active_batch_ids: Vec::new(),
            promotable_branch_ids: Vec::new(),
        }
    }
}

impl Default for RemoteRuntimeState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            active_control_session_id: None,
            connected_client_count: 0,
        }
    }
}

impl KernelStateBundle {
    pub fn save_to<P>(&self, path: P) -> Result<(), CheckpointError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let serialized = serde_json::to_string_pretty(self)?;
        let tmp_path = path.with_extension(format!(
            "tmp{}",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_millis()
        ));
        fs::write(&tmp_path, serialized)?;
        fs::rename(tmp_path, path)?;

        Ok(())
    }

    pub fn load_from<P>(path: P) -> Result<Self, CheckpointError>
    where
        P: AsRef<Path>,
    {
        let contents = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&contents)?)
    }
}

#[derive(Debug)]
pub enum CheckpointError {
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "checkpoint io failed: {err}"),
            Self::Serde(err) => write!(f, "checkpoint serialization failed: {err}"),
        }
    }
}

impl std::error::Error for CheckpointError {}

impl From<std::io::Error> for CheckpointError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CheckpointError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}
