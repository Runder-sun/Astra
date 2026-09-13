use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    pub reason: String,
    pub requested_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u128>,
    pub permission_mode: String,
    pub status: PermissionRequestStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRequestStatus {
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDecision {
    pub request_id: String,
    pub decision: String,
    pub decided_by: String,
    pub decided_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDecisionTrace {
    pub request_id: String,
    pub tool_name: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_path: Option<String>,
    pub required_mode: String,
    pub current_mode: String,
    pub allowlist_match: bool,
    pub workspace_boundary_ok: bool,
    pub branch_boundary_ok: bool,
    pub destructive: bool,
    pub approved: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPendingList {
    pub requests: Vec<PermissionRequest>,
    pub total_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionHistoryResult {
    pub decisions: Vec<PermissionDecisionTrace>,
    pub total_count: usize,
}
