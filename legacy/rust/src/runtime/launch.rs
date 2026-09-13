use crate::doctor::RuntimePreflightReport;
use crate::goals::MissionFrameProjection;
use crate::projects::current::ProjectResolutionTrace;
use crate::providers::ProviderResolutionTrace;
use crate::research::ResearchContextProjection;
use serde::Serialize;

pub const TURN_OUTCOME_COMPLETED: &str = "completed";
pub const TURN_OUTCOME_CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileResolutionTrace {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub requested_profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resolved_profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inherited_scopes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InteractiveLaunchResult {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub project_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    pub preflight: RuntimePreflightReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_trace: Option<ProjectResolutionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_trace: Option<ProfileResolutionTrace>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub launch_disposition: String,
    pub interactive_eligible: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reused_session_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnResult {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub turn_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub project_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_trace: Option<ProjectResolutionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_trace: Option<ProfileResolutionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_frame: Option<MissionFrameProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_context: Option<crate::docs::DocContextProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research_context: Option<ResearchContextProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_outputs: Option<crate::skills::SkillOutputContextProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research_classification: Option<crate::research::ResearchTurnClassification>,
    pub provider_trace: ProviderResolutionTrace,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub assistant_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_iterations: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_tool_calls: Option<usize>,
}

pub fn resolve_profile_trace(requested_profile: Option<&str>) -> ProfileResolutionTrace {
    match requested_profile {
        Some(profile) => ProfileResolutionTrace {
            requested_profile: profile.to_string(),
            resolved_profile: profile.to_string(),
            source: "explicit".to_string(),
            inherited_scopes: vec!["global".to_string(), "project".to_string()],
            warnings: Vec::new(),
        },
        None => ProfileResolutionTrace {
            requested_profile: String::new(),
            resolved_profile: "default".to_string(),
            source: "default".to_string(),
            inherited_scopes: vec!["global".to_string()],
            warnings: Vec::new(),
        },
    }
}
