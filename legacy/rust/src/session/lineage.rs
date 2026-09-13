use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineageRecord {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub resumed_from_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
