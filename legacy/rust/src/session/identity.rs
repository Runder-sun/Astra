use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIdentity {
    pub session_id: String,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub session_kind: String,
}
