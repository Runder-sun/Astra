use crate::agents;
use crate::branches;
use crate::config;
use crate::goals;
use crate::memory;
use crate::orchestration::OrchestrationProgressProjection;
use crate::projects::current::ResolvedProject;
use crate::remote;
use crate::research;
use crate::reviews;
use crate::routines;
use crate::session::identity::SessionIdentity;
use crate::skills;
use crate::surface_commands::product_command_specs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSurfaceStatusReport {
    pub schema_version: String,
    pub project_id: String,
    pub workspace_root: String,
    pub projection: HostSurfaceProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSurfaceProjection {
    pub schema_version: String,
    pub projection_id: String,
    pub project_id: String,
    pub source: String,
    pub authority_model: String,
    pub updated_at: String,
    pub surfaces: HostSurfaceReadiness,
    pub pane_status: HostPaneStatus,
    pub actions: Vec<HostSurfaceAction>,
    pub remote: remote::RemoteProjectionSnapshot,
    pub workbench: remote::RemoteWorkbenchProjection,
    pub sessions: HostSessionSummary,
    pub permissions: remote::RemoteWorkbenchPermissionSummary,
    pub memory: HostCountSummary,
    pub branches: HostCountSummary,
    pub reviews: HostCountSummary,
    pub research: HostResearchSummary,
    pub control_owner: String,
    pub control_lease_id: String,
    pub control_lease_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSurfaceReadiness {
    pub local_tui: SurfaceCapability,
    pub web: SurfaceCapability,
    pub mobile: SurfaceCapability,
    pub semantic_lane: SurfaceCapability,
    pub terminal_lane: SurfaceCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceCapability {
    pub readiness: String,
    pub transport: String,
    pub authority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPaneStatus {
    pub panes: Vec<HostPane>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPane {
    pub pane_id: String,
    pub status: String,
    pub source: String,
    pub default_visibility: String,
    pub data_ref: String,
    pub item_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSurfaceAction {
    pub action_id: String,
    pub label: String,
    pub surface: String,
    pub gate: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_contract: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_control_contract: Option<String>,
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "bool_is_false")]
    pub requires_trigger_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSessionSummary {
    pub total_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<String>,
    pub recent: Vec<SessionIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCountSummary {
    pub status: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostResearchSummary {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_thread_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_stage_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_stage_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_deliberation_mode: Option<String>,
    pub open_questions: Vec<String>,
    pub agreed_decisions: Vec<String>,
    pub evidence_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    pub pending_operations: Vec<String>,
    pub next_recommended_action: String,
    pub recovery_governance_status: String,
    pub open_recovery_wake_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_governance: Option<routines::RoutineRecoveryGovernanceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_closure: Option<goals::GoalLoopClosureProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_watch: Option<goals::GoalWatchHealthProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_plan: Option<goals::GoalWatchPlanProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_install_status: Option<goals::GoalWatchInstallStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_decision_policy: Option<goals::GoalStageDecisionPolicyProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_pool: Option<goals::GoalTaskPoolSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_pool_maintenance: Option<goals::GoalTaskPoolMaintenanceProjection>,
    #[serde(default = "default_compact_research_line")]
    pub compact_research_line: String,
    #[serde(default = "default_active_question_label")]
    pub active_question_label: String,
    #[serde(default = "default_stage_label")]
    pub stage_label: String,
    #[serde(default = "default_evidence_label")]
    pub evidence_label: String,
    #[serde(default)]
    pub evidence_count: usize,
    #[serde(default = "default_open_gap_label")]
    pub open_gap_label: String,
    #[serde(default = "default_next_action_label")]
    pub next_action_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_run_progress: Option<OrchestrationProgressProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autonomous_job: Option<HostAutonomousResearchJobSummary>,
    #[serde(default)]
    pub board: research::ResearchBoardProjection,
    #[serde(default, skip_serializing_if = "bool_is_false")]
    pub board_inspector_ready: bool,
    #[serde(default = "default_board_inspector_command")]
    pub board_inspector_command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostAutonomousResearchJobSummary {
    pub job_id: String,
    pub status: String,
    pub phase: String,
    pub automation_mode: String,
    pub active_stage_id: Option<String>,
    pub active_stage_execution_id: Option<String>,
    pub open_blocking_obligation_count: usize,
    pub active_blocking_obligation_count: usize,
    pub latest_continuity_packet_ref: Option<String>,
    pub resume_command: Vec<String>,
    pub tick_command: Vec<String>,
    pub stop_command: Vec<String>,
    pub next_required_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiLaunchResult {
    pub schema_version: String,
    pub launch_mode: String,
    pub runtime: String,
    pub projection: HostSurfaceProjection,
    pub inline_view: TuiInlineView,
    pub degradation_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiInlineView {
    pub schema_version: String,
    pub renderer_mode: String,
    pub layout: String,
    pub authority_model: String,
    pub capabilities: Vec<TuiRendererCapability>,
    pub command_model: TuiCommandModel,
    pub skill_model: TuiSkillModel,
    pub status_hud: Vec<TuiStatusBadge>,
    pub pane_plans: Vec<TuiPaneRenderPlan>,
    pub keymap: Vec<TuiKeyBinding>,
    pub theme: TuiThemePlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiRendererCapability {
    pub capability_id: String,
    pub status: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiCommandModel {
    pub command_prefix: String,
    pub groups: Vec<TuiCommandGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiCommandGroup {
    pub group_id: String,
    pub label: String,
    pub commands: Vec<TuiCommandEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiCommandEntry {
    pub typed: String,
    pub action_id: String,
    pub label: String,
    pub summary: String,
    pub gate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiSkillModel {
    pub skill_prefix: String,
    pub total_count: usize,
    pub degraded_count: usize,
    pub entries: Vec<TuiSkillEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiSkillEntry {
    pub typed: String,
    pub skill_id: String,
    pub description: String,
    pub enabled: bool,
    pub degraded: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiStatusBadge {
    pub badge_id: String,
    pub label: String,
    pub state: String,
    pub data_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiPaneRenderPlan {
    pub pane_id: String,
    pub renderer: String,
    pub collapse_state: String,
    pub pager: String,
    pub data_ref: String,
    pub empty_state: String,
    pub action_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiKeyBinding {
    pub key: String,
    pub action_id: String,
    pub gate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiThemePlan {
    pub theme_id: String,
    pub mode: String,
    pub supports_dark_light: bool,
    pub accent_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiActionList {
    pub schema_version: String,
    pub project_id: String,
    pub projection_id: String,
    pub actions: Vec<HostSurfaceAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleStatusReport {
    pub schema_version: String,
    pub overlay_detected: bool,
    pub detection_source: String,
    pub addresses: Vec<String>,
    pub suggested_bind_host: String,
    pub network_cidr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleServePlan {
    pub schema_version: String,
    pub overlay_detected: bool,
    pub direct_url: String,
    pub bind_host: String,
    pub daemon_port: u16,
    pub serve_command: Vec<String>,
    pub fallback_url: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernedTerminalProjection {
    pub schema_version: String,
    pub terminal_id: String,
    pub project_id: String,
    pub lease_state: String,
    pub lane_state: String,
    pub authority: String,
    pub owner_id: String,
    pub control_lease_id: String,
    pub active_session_id: Option<String>,
    pub input_policy: String,
    pub resize_policy: String,
    pub signal_policy: String,
    pub scrollback_cursor: String,
    pub mobile_extra_keys: Vec<String>,
    pub xterm_bridge_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalAttachResult {
    pub schema_version: String,
    pub action: String,
    pub terminal: GovernedTerminalProjection,
    pub projection: HostSurfaceProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalReplayResult {
    pub schema_version: String,
    pub action: String,
    pub replay: TerminalReplay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalReplay {
    pub schema_version: String,
    pub replay_policy: String,
    pub cursor: String,
    pub events: Vec<Value>,
}

#[derive(Debug)]
pub enum HostSurfaceError {
    Io(std::io::Error),
    Remote(remote::RemoteError),
    Session(crate::session::store::SessionStoreError),
    Branch(branches::BranchError),
    Agent(agents::AgentError),
    Review(reviews::ReviewError),
    Memory(memory::MemoryError),
    ProjectOps(crate::projectops::ProjectOpsError),
    Research(research::ResearchError),
}

impl std::fmt::Display for HostSurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "host surface IO failed: {err}"),
            Self::Remote(err) => write!(f, "host surface remote status failed: {err}"),
            Self::Session(err) => write!(f, "host surface session status failed: {err}"),
            Self::Branch(err) => write!(f, "host surface branch status failed: {err}"),
            Self::Agent(err) => write!(f, "host surface agent status failed: {err}"),
            Self::Review(err) => write!(f, "host surface review status failed: {err}"),
            Self::Memory(err) => write!(f, "host surface memory status failed: {err}"),
            Self::ProjectOps(err) => write!(f, "host surface projectops status failed: {err}"),
            Self::Research(err) => write!(f, "host surface research status failed: {err}"),
        }
    }
}

impl std::error::Error for HostSurfaceError {}

impl From<std::io::Error> for HostSurfaceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<remote::RemoteError> for HostSurfaceError {
    fn from(value: remote::RemoteError) -> Self {
        Self::Remote(value)
    }
}

pub fn status(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<HostSurfaceStatusReport, HostSurfaceError> {
    Ok(HostSurfaceStatusReport {
        schema_version: "host_surface_status.v1".to_string(),
        project_id: resolved.project_id.clone(),
        workspace_root: resolved.workspace_root.display().to_string(),
        projection: projection(state_home, resolved, active_session_id)?,
    })
}

pub fn projection(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<HostSurfaceProjection, HostSurfaceError> {
    let remote_status = remote::status(state_home, resolved, active_session_id.clone())?;
    let store = crate::session::store::SessionStore::new(resolved.data_dir.clone());
    let sessions_all = store.list_sessions().map_err(HostSurfaceError::Session)?;
    let session_count = sessions_all.len();
    let sessions = sessions_all.into_iter().take(8).collect::<Vec<_>>();
    let session_summary = HostSessionSummary {
        total_count: session_count,
        active_session_id: active_session_id.clone(),
        recent: sessions,
    };
    let memory_status =
        memory::status(&resolved.data_dir, None).map_err(HostSurfaceError::Memory)?;
    let branch_status = branches::list(&resolved.data_dir).map_err(HostSurfaceError::Branch)?;
    let agent_status = agents::list(&resolved.data_dir).map_err(HostSurfaceError::Agent)?;
    let review_status =
        reviews::list(&resolved.data_dir, None, None).map_err(HostSurfaceError::Review)?;
    let projectops_status = crate::projectops::status(&resolved.data_dir, &resolved.project_id)
        .map_err(HostSurfaceError::ProjectOps)?;
    let research_projection = research::projection(&resolved.data_dir, &resolved.project_id)
        .map_err(HostSurfaceError::Research)?;
    let mut research_summary =
        host_research_summary_from_projection(&resolved.project_id, research_projection.clone());
    let research_status = research::status(&resolved.data_dir, &resolved.project_id)
        .map_err(HostSurfaceError::Research)?;
    let active_run = crate::orchestration::load_active_run(&resolved.data_dir)
        .ok()
        .flatten();
    let active_run_progress = active_run.as_ref().map(|run| run.progress_projection());
    let automation_policy = active_run
        .as_ref()
        .map(|run| run.automation_policy_projection());
    research_summary.board = research_projection
        .as_ref()
        .map(|projection| {
            research::ResearchBoardProjection::from_context_projection(
                &resolved.project_id,
                projection,
            )
        })
        .unwrap_or_else(|| {
            research::ResearchBoardProjection::empty(
                &resolved.project_id,
                "start_or_select_research_thread",
            )
        });
    if active_run_progress.is_some() || automation_policy.is_some() {
        research_summary.board = research::ResearchBoardProjection::from_status_with_external_state(
            &research_status,
            active_run_progress.as_ref(),
            active_run.as_ref(),
            automation_policy.as_ref(),
            Some(&agent_status),
            Some(&review_status),
            Some(&projectops_status),
        );
    } else {
        research_summary.board = research::ResearchBoardProjection::from_status_with_external_state(
            &research_status,
            None,
            None,
            None,
            Some(&agent_status),
            Some(&review_status),
            Some(&projectops_status),
        );
    }
    let recovery_governance = crate::routines::recovery_governance(&resolved.data_dir).ok();
    let goal_status = crate::goals::status(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    )
    .ok();
    let loop_closure = goal_status.as_ref().map(|status| {
        crate::goals::loop_closure_projection(
            &status.task_pool,
            recovery_governance.as_ref(),
            0,
            false,
        )
    });
    let open_recovery_wake_count = recovery_governance
        .as_ref()
        .map(|summary| summary.pending_ingress_failure_count + summary.failed_trigger_count)
        .unwrap_or_else(|| {
            projectops_status
                .wake_events
                .iter()
                .filter(|wake| wake.state != "resolved")
                .count()
        });
    research_summary.open_recovery_wake_count = open_recovery_wake_count;
    research_summary.recovery_governance = recovery_governance.clone();
    let task_pool_maintenance = goal_status.as_ref().map(|status| {
        crate::goals::task_pool_maintenance_projection(&status.task_pool, loop_closure.as_ref())
    });
    research_summary.loop_closure = loop_closure;
    research_summary.goal_watch = crate::goals::load_goal_watch_health(&resolved.data_dir);
    research_summary.watch_plan = goal_status.as_ref().map(|status| status.watch_plan.clone());
    research_summary.watch_install_status = goal_status.as_ref().and_then(|status| {
        crate::goals::goal_watch_install_status(
            &resolved.data_dir,
            &resolved.project_id,
            &resolved.workspace_root,
            status.task_pool.automation_mode,
        )
        .ok()
    });
    research_summary.stage_decision_policy = goal_status
        .as_ref()
        .map(|status| status.stage_decision_policy.clone());
    research_summary.task_pool = goal_status.as_ref().map(|status| status.task_pool.clone());
    research_summary.task_pool_maintenance = task_pool_maintenance;
    research_summary.autonomous_job = active_autonomous_research_job_summary(&resolved.data_dir);
    research_summary.recovery_governance_status = recovery_governance
        .as_ref()
        .map(|summary| summary.status.clone())
        .unwrap_or_else(|| {
            if projectops_status.escalated_wake_count > 0 {
                "escalated".to_string()
            } else if open_recovery_wake_count > 0 {
                "attention_required".to_string()
            } else {
                "healthy".to_string()
            }
        });
    research_summary.active_run_progress = active_run_progress;
    let permissions = remote_status.workbench.permissions.clone();
    let mut actions = default_actions(
        remote_status.remote_ready,
        !permissions.pending.is_empty(),
        active_session_id.as_deref(),
        research_summary.autonomous_job.as_ref(),
    );
    actions.sort_by(|left, right| left.action_id.cmp(&right.action_id));

    Ok(HostSurfaceProjection {
        schema_version: "host_surface_projection.v1".to_string(),
        projection_id: format!("host_projection_{}", resolved.project_id),
        project_id: resolved.project_id.clone(),
        source: "kernel_state_bundle_and_project_indexes".to_string(),
        authority_model: "kernel_authoritative_projection".to_string(),
        updated_at: timestamp_string(),
        surfaces: readiness(remote_status.remote_ready),
        pane_status: default_panes(
            session_count,
            permissions.pending_count,
            memory_status.durable_count,
            branch_status.total_count,
            review_status.total_count,
            research_summary.status.as_str(),
        ),
        actions,
        remote: remote_status.projection,
        workbench: remote_status.workbench,
        sessions: session_summary,
        permissions,
        memory: HostCountSummary {
            status: memory_status.durable_status,
            count: memory_status.durable_count,
        },
        branches: HostCountSummary {
            status: "available".to_string(),
            count: branch_status.total_count,
        },
        reviews: HostCountSummary {
            status: "available".to_string(),
            count: review_status.total_count,
        },
        research: research_summary,
        control_owner: remote_status.control_owner.owner_id,
        control_lease_id: remote_status.lease.lease_id,
        control_lease_state: remote_status.lease.lease_state,
    })
}

pub fn tui_launch(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<TuiLaunchResult, HostSurfaceError> {
    tui_launch_with_display_mode(state_home, resolved, active_session_id, false)
}

pub fn tui_launch_with_display_mode(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
    fullscreen: bool,
) -> Result<TuiLaunchResult, HostSurfaceError> {
    let projection = projection(state_home, resolved, active_session_id)?;
    let inline_view = inline_view(state_home, resolved, &projection, fullscreen);
    Ok(TuiLaunchResult {
        schema_version: "tui_launch_result.v1".to_string(),
        launch_mode: if fullscreen {
            "fullscreen_split_pane_projection_renderer"
        } else {
            "inline_projection_renderer"
        }
        .to_string(),
        runtime: "projection_first_terminal_contract".to_string(),
        projection,
        inline_view,
        degradation_policy: "fallback_to_json_snapshot_on_unsupported_terminal".to_string(),
    })
}

pub fn tui_actions(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<TuiActionList, HostSurfaceError> {
    let projection = projection(state_home, resolved, active_session_id)?;
    Ok(TuiActionList {
        schema_version: "tui_action_list.v1".to_string(),
        project_id: resolved.project_id.clone(),
        projection_id: projection.projection_id,
        actions: projection.actions,
    })
}

pub fn tailscale_status() -> TailscaleStatusReport {
    if let Ok(value) = env::var("RESEARCH_CLI_TAILSCALE_IP") {
        let addresses = parse_tailscale_addresses(&value);
        return TailscaleStatusReport {
            schema_version: "tailscale_status.v1".to_string(),
            overlay_detected: !addresses.is_empty(),
            detection_source: "env:RESEARCH_CLI_TAILSCALE_IP".to_string(),
            addresses,
            suggested_bind_host: "0.0.0.0".to_string(),
            network_cidr: "100.64.0.0/10".to_string(),
        };
    }

    let output = Command::new("tailscale").args(["ip", "-4"]).output();
    if let Ok(output) = output {
        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout);
            let addresses = parse_tailscale_addresses(&raw);
            return TailscaleStatusReport {
                schema_version: "tailscale_status.v1".to_string(),
                overlay_detected: !addresses.is_empty(),
                detection_source: "tailscale ip -4".to_string(),
                addresses,
                suggested_bind_host: "0.0.0.0".to_string(),
                network_cidr: "100.64.0.0/10".to_string(),
            };
        }
    }

    let interface_addresses = detect_tailscale_interface_addresses();
    TailscaleStatusReport {
        schema_version: "tailscale_status.v1".to_string(),
        overlay_detected: !interface_addresses.is_empty(),
        detection_source: if interface_addresses.is_empty() {
            "not_detected".to_string()
        } else {
            "ip_addr_interface_scan".to_string()
        },
        addresses: interface_addresses,
        suggested_bind_host: "0.0.0.0".to_string(),
        network_cidr: "100.64.0.0/10".to_string(),
    }
}

pub fn tailscale_serve_plan(daemon_port: u16) -> TailscaleServePlan {
    let status = tailscale_status();
    let address = status
        .addresses
        .first()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    TailscaleServePlan {
        schema_version: "tailscale_serve_plan.v1".to_string(),
        overlay_detected: status.overlay_detected,
        direct_url: format!("http://{address}:{daemon_port}/"),
        bind_host: status.suggested_bind_host,
        daemon_port,
        serve_command: vec![
            "tailscale".to_string(),
            "serve".to_string(),
            format!("http://127.0.0.1:{daemon_port}"),
        ],
        fallback_url: format!("http://127.0.0.1:{daemon_port}/"),
        notes: vec![
            "Start `research-cli remote daemon --host 0.0.0.0 --port <port>` on the server."
                .to_string(),
            "Use the direct_url from a paired device on the same Tailscale tailnet.".to_string(),
            "Tailscale is transport only; control still requires the research-cli remote lease."
                .to_string(),
        ],
    }
}

pub fn terminal_attach(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<TerminalAttachResult, HostSurfaceError> {
    let projection = projection(state_home, resolved, active_session_id.clone())?;
    let terminal = terminal_projection(&projection, active_session_id);
    write_terminal_projection(&resolved.data_dir, &terminal)?;
    Ok(TerminalAttachResult {
        schema_version: "terminal_attach_result.v1".to_string(),
        action: "terminal_attach".to_string(),
        terminal,
        projection,
    })
}

pub fn terminal_replay(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<TerminalReplayResult, HostSurfaceError> {
    let projection = projection(state_home, resolved, active_session_id.clone())?;
    let terminal = load_terminal_projection(&resolved.data_dir)?
        .unwrap_or_else(|| terminal_projection(&projection, active_session_id));
    Ok(TerminalReplayResult {
        schema_version: "terminal_replay_result.v1".to_string(),
        action: "terminal_replay".to_string(),
        replay: TerminalReplay {
            schema_version: "terminal_replay.v1".to_string(),
            replay_policy: "cursor_based_scrollback".to_string(),
            cursor: terminal.scrollback_cursor,
            events: vec![json!({
                "event_kind": "projection_cursor",
                "project_id": resolved.project_id,
                "active_session_id": terminal.active_session_id,
                "source": "kernel_events_jsonl"
            })],
        },
    })
}

fn terminal_projection(
    projection: &HostSurfaceProjection,
    active_session_id: Option<String>,
) -> GovernedTerminalProjection {
    GovernedTerminalProjection {
        schema_version: "governed_terminal_projection.v1".to_string(),
        terminal_id: format!("terminal_{}", projection.project_id),
        project_id: projection.project_id.clone(),
        lease_state: projection.control_lease_state.clone(),
        lane_state: "governed_pty_projection".to_string(),
        authority: "control_lease_required".to_string(),
        owner_id: projection.control_owner.clone(),
        control_lease_id: projection.control_lease_id.clone(),
        active_session_id,
        input_policy: "typed_action_then_pty_bytes".to_string(),
        resize_policy: "capture_resize_events".to_string(),
        signal_policy: "allow_interrupt_through_control_gate".to_string(),
        scrollback_cursor: format!("scrollback_{}", projection.project_id),
        mobile_extra_keys: vec![
            "esc".to_string(),
            "tab".to_string(),
            "ctrl-c".to_string(),
            "ctrl-d".to_string(),
            "arrows".to_string(),
        ],
        xterm_bridge_state: "host_pty_byte_stream".to_string(),
    }
}

fn host_research_summary_from_projection(
    project_id: &str,
    projection: Option<research::ResearchContextProjection>,
) -> HostResearchSummary {
    let Some(projection) = projection else {
        let active_question_label = default_active_question_label();
        let stage_label = default_stage_label();
        let evidence_label = default_evidence_label();
        let open_gap_label = default_open_gap_label();
        let next_action_label = default_next_action_label();
        return HostResearchSummary {
            status: "no_active_thread".to_string(),
            active_thread_id: None,
            active_thread_title: None,
            active_stage_id: None,
            active_stage_class: None,
            active_deliberation_mode: None,
            open_questions: Vec::new(),
            agreed_decisions: Vec::new(),
            evidence_refs: Vec::new(),
            confidence: None,
            pending_operations: Vec::new(),
            next_recommended_action: "start_or_select_research_thread".to_string(),
            recovery_governance_status: "healthy".to_string(),
            open_recovery_wake_count: 0,
            compact_research_line: compact_research_line(
                &active_question_label,
                &stage_label,
                &evidence_label,
                &open_gap_label,
                &next_action_label,
            ),
            active_question_label,
            stage_label,
            evidence_label,
            evidence_count: 0,
            open_gap_label,
            next_action_label,
            active_run_progress: None,
            recovery_governance: None,
            loop_closure: None,
            goal_watch: None,
            watch_plan: None,
            watch_install_status: None,
            stage_decision_policy: None,
            task_pool: None,
            task_pool_maintenance: None,
            autonomous_job: None,
            board: research::ResearchBoardProjection::empty(
                project_id,
                "start_or_select_research_thread",
            ),
            board_inspector_ready: true,
            board_inspector_command: default_board_inspector_command(),
        };
    };

    let active_question_label = format!(
        "问题：{}",
        product_copy_value(&projection.active_thread_title, "未命名研究问题")
    );
    let stage_label = research_stage_label(
        &projection.active_stage_id,
        &projection.active_stage_class,
        &projection.active_deliberation_mode,
    );
    let evidence_count = projection.evidence_refs.len();
    let evidence_label = research_evidence_label(&projection.evidence_refs);
    let open_gap_label = research_open_gap_label(&projection.open_questions);
    let next_action_label = research_next_action_label(&projection.next_recommended_action);
    let compact_research_line = compact_research_line(
        &active_question_label,
        &stage_label,
        &evidence_label,
        &open_gap_label,
        &next_action_label,
    );
    let board = research::ResearchBoardProjection::from_context_projection(project_id, &projection);

    HostResearchSummary {
        status: "available".to_string(),
        active_thread_id: Some(projection.active_thread_id),
        active_thread_title: Some(projection.active_thread_title),
        active_stage_id: Some(projection.active_stage_id),
        active_stage_class: Some(projection.active_stage_class),
        active_deliberation_mode: Some(projection.active_deliberation_mode),
        open_questions: projection.open_questions,
        agreed_decisions: projection.agreed_decisions,
        evidence_refs: projection.evidence_refs,
        confidence: Some(projection.confidence),
        pending_operations: projection.pending_operations,
        next_recommended_action: projection.next_recommended_action,
        recovery_governance_status: "healthy".to_string(),
        open_recovery_wake_count: 0,
        compact_research_line,
        active_question_label,
        stage_label,
        evidence_label,
        evidence_count,
        open_gap_label,
        next_action_label,
        active_run_progress: None,
        recovery_governance: None,
        loop_closure: None,
        goal_watch: None,
        watch_plan: None,
        watch_install_status: None,
        stage_decision_policy: None,
        task_pool: None,
        task_pool_maintenance: None,
        autonomous_job: None,
        board,
        board_inspector_ready: true,
        board_inspector_command: default_board_inspector_command(),
    }
}

fn active_autonomous_research_job_summary(
    data_dir: &Path,
) -> Option<HostAutonomousResearchJobSummary> {
    let jobs_dir = data_dir.join("research").join("jobs");
    let mut jobs = Vec::new();
    if let Ok(entries) = fs::read_dir(&jobs_dir) {
        for entry in entries.flatten() {
            let path = entry.path().join("job.json");
            if !path.exists() {
                continue;
            }
            let Ok(value) = fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str::<Value>(&content).ok())
                .ok_or(())
            else {
                continue;
            };
            jobs.push(value);
        }
    }
    jobs.sort_by(|left, right| {
        value_string(right, "updated_at").cmp(&value_string(left, "updated_at"))
    });
    let job = jobs
        .iter()
        .find(|job| !autonomous_job_terminal(value_string(job, "status").as_str()))
        .or_else(|| jobs.first())?;
    let job_id = value_string(job, "job_id");
    if job_id.is_empty() {
        return None;
    }
    let obligations = job
        .get("obligations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let open_blocking_obligation_count = obligations
        .iter()
        .filter(|obligation| {
            value_bool(obligation, "blocking") && value_string(obligation, "status") == "open"
        })
        .count();
    let active_blocking_obligation_count = obligations
        .iter()
        .filter(|obligation| {
            value_bool(obligation, "blocking")
                && !obligation_status_is_closed(value_string(obligation, "status").as_str())
        })
        .count();
    let active_stage_id = value_string(job, "stage_execution_id")
        .strip_prefix("stage_")
        .and_then(|rest| rest.rsplit_once('_').map(|(stage, _)| stage.to_string()))
        .filter(|stage| !stage.chars().all(|ch| ch.is_ascii_digit()))
        .or_else(|| active_stage_id_from_artifact_refs(job));
    let latest_continuity_packet_ref = job
        .get("continuity_packets")
        .and_then(Value::as_array)
        .and_then(|packets| packets.last())
        .map(|packet| value_string(packet, "artifact_path"))
        .filter(|value| !value.is_empty());
    let next_required_action = if open_blocking_obligation_count > 0 {
        "Main agent must handle open blocking obligations before repeated review.".to_string()
    } else if active_blocking_obligation_count > 0 {
        "Review can rerun, but stage advance/final completion still requires obligation closure."
            .to_string()
    } else {
        "Resume or tick the autonomous research job through budget and policy gates.".to_string()
    };
    let stage_execution_id = value_string(job, "stage_execution_id");
    Some(HostAutonomousResearchJobSummary {
        job_id: job_id.clone(),
        status: value_string(job, "status"),
        phase: value_string(job, "phase"),
        automation_mode: value_string(job, "automation_mode"),
        active_stage_id,
        active_stage_execution_id: (!stage_execution_id.is_empty()).then_some(stage_execution_id),
        open_blocking_obligation_count,
        active_blocking_obligation_count,
        latest_continuity_packet_ref,
        resume_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "resume".to_string(),
            job_id.clone(),
        ],
        tick_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "tick".to_string(),
            job_id.clone(),
        ],
        stop_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "stop".to_string(),
            job_id,
        ],
        next_required_action,
    })
}

fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn value_bool(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn obligation_status_is_closed(status: &str) -> bool {
    matches!(
        status,
        "satisfied" | "superseded_by_rollback" | "rejected_with_rationale" | "human_gate_required"
    )
}

fn autonomous_job_terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "stopped")
}

fn active_stage_id_from_artifact_refs(job: &Value) -> Option<String> {
    let refs = job.get("artifact_refs")?.as_array()?;
    refs.iter().find_map(|value| {
        let reference = value.as_str()?;
        let marker = "/research/stages/";
        if !reference.contains("research/stages/") {
            return None;
        }
        let path = PathBuf::from(reference);
        path.components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .windows(4)
            .find_map(|window| {
                if window[0] == "research" && window[1] == "stages" {
                    Some(window[3].clone())
                } else {
                    None
                }
            })
            .or_else(|| {
                reference
                    .split(marker)
                    .nth(1)
                    .and_then(|rest| rest.split('/').nth(1).map(str::to_string))
            })
    })
}

fn compact_research_line(
    active_question_label: &str,
    stage_label: &str,
    evidence_label: &str,
    open_gap_label: &str,
    next_action_label: &str,
) -> String {
    format!(
        "研究 | {} | {} | {} | {} | {}",
        active_question_label, stage_label, evidence_label, open_gap_label, next_action_label
    )
}

fn research_stage_label(stage_id: &str, stage_class: &str, mode: &str) -> String {
    let stage = stage_id_label(stage_id);
    let class = stage_class_label(stage_class);
    let mode = mode_label(mode);
    if mode.is_empty() {
        format!("阶段：{stage} / {class}")
    } else {
        format!("阶段：{stage} / {class} / {mode}")
    }
}

fn research_evidence_label(evidence_refs: &[String]) -> String {
    if evidence_refs.is_empty() {
        return "证据：0 条".to_string();
    }
    let preview = evidence_refs
        .iter()
        .take(2)
        .map(|value| product_copy_value(value, "证据"))
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if evidence_refs.len() > 2 {
        format!("，另 {} 条", evidence_refs.len() - 2)
    } else {
        String::new()
    };
    format!("证据：{} 条（{}{}）", evidence_refs.len(), preview, suffix)
}

fn research_open_gap_label(open_questions: &[String]) -> String {
    match open_questions.first() {
        Some(question) if !question.trim().is_empty() => {
            format!("缺口：{}", product_copy_value(question, "待确认"))
        }
        _ => "缺口：暂无阻塞问题".to_string(),
    }
}

fn research_next_action_label(action: &str) -> String {
    let label = match action.trim() {
        "start_or_select_research_thread" | "record_or_resume_research_thread" => {
            "用 /research 选择现有线程，或把当前任务记录为研究问题"
        }
        "inspect_research_thread" => "检查当前研究线程，确认阶段、证据和开放问题",
        "inspect_or_advance_research_thread" => "检查研究状态；证据充分后推进到下一阶段",
        "record failure evidence before selecting repair, rerun, or pivot" => {
            "先记录失败证据，再决定修复、重跑或调整方向"
        }
        "resume_after_human_gate" => "等待人工确认后继续",
        "" => "继续澄清研究问题和下一步",
        other => return format!("下一步：{}", humanize_identifier(other)),
    };
    format!("下一步：{label}")
}

fn stage_id_label(value: &str) -> String {
    match value.trim() {
        "literature" => "文献调研".to_string(),
        "idea" => "想法形成".to_string(),
        "novelty" => "新颖性检查".to_string(),
        "refine" => "方案细化".to_string(),
        "design-doc-sync" => "设计文档同步".to_string(),
        "experiment-plan" | "experiment plan" => "实验计划".to_string(),
        "implement-solution" => "实现方案".to_string(),
        "run" => "运行实验".to_string(),
        "monitor" => "监控实验".to_string(),
        "result-to-claim" => "结果到论点".to_string(),
        "paper-plan" => "论文规划".to_string(),
        "paper-write" => "论文写作".to_string(),
        "paper-compile" => "论文编译".to_string(),
        "research-review" => "研究评审".to_string(),
        "rebuttal" => "回复评审".to_string(),
        "meta-optimize" => "元优化".to_string(),
        "" => "未确定".to_string(),
        other => humanize_identifier(other),
    }
}

fn stage_class_label(value: &str) -> String {
    match value.trim() {
        "survey" => "调研".to_string(),
        "idea_form" => "想法形成".to_string(),
        "idea_refine" => "想法细化".to_string(),
        "document" => "文档".to_string(),
        "experiment_design" => "实验设计".to_string(),
        "implement" => "实现".to_string(),
        "experiment_run" => "实验运行".to_string(),
        "result_to_claim" => "论点评估".to_string(),
        "publish" => "发布准备".to_string(),
        "repair" => "修复".to_string(),
        "" => "未确定".to_string(),
        other => humanize_identifier(other),
    }
}

fn mode_label(value: &str) -> String {
    match value.trim() {
        "exploring" => "探索中".to_string(),
        "comparing" => "比较中".to_string(),
        "reviewing" => "审阅中".to_string(),
        "debugging" => "调试中".to_string(),
        "interpreting_results" => "结果解读中".to_string(),
        "drafting" => "草拟中".to_string(),
        "awaiting_human_gate" => "等待确认".to_string(),
        "ready_to_record" => "可记录".to_string(),
        "ready_to_execute" => "可执行".to_string(),
        "" => String::new(),
        other => humanize_identifier(other),
    }
}

fn product_copy_value(value: &str, fallback: &str) -> String {
    let compacted = value
        .replace(['|', '\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if compacted.is_empty() {
        fallback.to_string()
    } else {
        compacted
    }
}

fn humanize_identifier(value: &str) -> String {
    product_copy_value(&value.replace(['_', '-'], " "), "未确定")
}

fn default_compact_research_line() -> String {
    compact_research_line(
        &default_active_question_label(),
        &default_stage_label(),
        &default_evidence_label(),
        &default_open_gap_label(),
        &default_next_action_label(),
    )
}

fn default_active_question_label() -> String {
    "问题：当前没有绑定研究问题".to_string()
}

fn default_stage_label() -> String {
    "阶段：未开始".to_string()
}

fn bool_is_false(value: &bool) -> bool {
    !*value
}

fn default_evidence_label() -> String {
    "证据：0 条".to_string()
}

fn default_open_gap_label() -> String {
    "缺口：先选择或记录一个研究线程".to_string()
}

fn default_next_action_label() -> String {
    "下一步：用 /research 选择现有线程，或把当前任务记录为研究问题".to_string()
}

fn default_board_inspector_command() -> Vec<String> {
    vec![
        "research".to_string(),
        "board".to_string(),
        "--json".to_string(),
    ]
}

fn readiness(remote_ready: bool) -> HostSurfaceReadiness {
    let remote_state = if remote_ready {
        "projection_ready"
    } else {
        "pairing_required"
    };
    let degraded_reason = if remote_ready {
        None
    } else {
        Some("remote client is not paired".to_string())
    };
    HostSurfaceReadiness {
        local_tui: SurfaceCapability {
            readiness: "projection_ready".to_string(),
            transport: "local_terminal".to_string(),
            authority: "kernel_projection_only".to_string(),
            degraded_reason: None,
        },
        web: SurfaceCapability {
            readiness: remote_state.to_string(),
            transport: "daemon_http_over_tailscale".to_string(),
            authority: "remote_control_lease".to_string(),
            degraded_reason: degraded_reason.clone(),
        },
        mobile: SurfaceCapability {
            readiness: remote_state.to_string(),
            transport: "tailscale_private_overlay".to_string(),
            authority: "remote_control_lease".to_string(),
            degraded_reason: degraded_reason.clone(),
        },
        semantic_lane: SurfaceCapability {
            readiness: remote_state.to_string(),
            transport: "typed_http_actions".to_string(),
            authority: "command_registry_and_kernel_gates".to_string(),
            degraded_reason: degraded_reason.clone(),
        },
        terminal_lane: SurfaceCapability {
            readiness: remote_state.to_string(),
            transport: "governed_pty_projection".to_string(),
            authority: "control_lease_required".to_string(),
            degraded_reason,
        },
    }
}

fn default_panes(
    session_count: usize,
    permission_count: usize,
    memory_count: usize,
    branch_count: usize,
    review_count: usize,
    research_status: &str,
) -> HostPaneStatus {
    fn pane(
        pane_id: &str,
        source: &str,
        data_ref: &str,
        item_count: usize,
        status: &str,
        degraded_reason: Option<&str>,
    ) -> HostPane {
        HostPane {
            pane_id: pane_id.to_string(),
            status: status.to_string(),
            source: source.to_string(),
            default_visibility: if pane_id == "conversation" || pane_id == "status_hud" {
                "visible".to_string()
            } else {
                "collapsible".to_string()
            },
            data_ref: data_ref.to_string(),
            item_count,
            degraded_reason: degraded_reason.map(ToString::to_string),
        }
    }

    let conversation_status = if session_count > 0 {
        "available"
    } else {
        "contract_ready"
    };
    let permission_status = if permission_count > 0 {
        "available"
    } else {
        "contract_ready"
    };
    let memory_status = if memory_count > 0 {
        "available"
    } else {
        "contract_ready"
    };
    let branch_review_count = branch_count + review_count;
    let branch_review_status = if branch_review_count > 0 {
        "available"
    } else {
        "contract_ready"
    };
    let research_ready = research_status == "available";

    HostPaneStatus {
        panes: vec![
            pane(
                "conversation",
                "sessions",
                "sessions.recent",
                session_count,
                conversation_status,
                (session_count == 0).then_some("no sessions recorded yet"),
            ),
            pane(
                "input_editor",
                "command_registry",
                "actions",
                0,
                "available",
                None,
            ),
            pane(
                "status_hud",
                "kernel_state_bundle",
                "surfaces",
                0,
                "available",
                None,
            ),
            pane(
                "tool_activity",
                "events_jsonl",
                "events.tool_activity",
                0,
                "contract_ready",
                Some("tool event slices are not materialized in this projection yet"),
            ),
            pane(
                "permission_overlay",
                "permission_queue",
                "permissions.pending",
                permission_count,
                permission_status,
                (permission_count == 0).then_some("no pending permission requests"),
            ),
            pane(
                "diff_artifact",
                "artifact_families",
                "workbench.artifacts",
                0,
                "contract_ready",
                Some("artifact diff slices are exposed by contract; no artifact selected"),
            ),
            pane(
                "memory",
                "memory_projection",
                "memory",
                memory_count,
                memory_status,
                (memory_count == 0).then_some("no durable memories recorded"),
            ),
            pane(
                "branch_review",
                "branch_and_review_indexes",
                "branches,reviews",
                branch_review_count,
                branch_review_status,
                (branch_review_count == 0).then_some("no branches or reviews recorded"),
            ),
            pane(
                "research_dag",
                "research_context_projection",
                "research",
                usize::from(research_ready),
                if research_ready {
                    "available"
                } else {
                    "contract_ready"
                },
                (!research_ready).then_some("no active research thread"),
            ),
            pane(
                "diagnostics",
                "doctor_setup_provider_mcp",
                "diagnostics",
                0,
                "contract_ready",
                Some("diagnostic slices are queried on demand"),
            ),
        ],
    }
}

fn inline_view(
    state_home: &Path,
    resolved: &ResolvedProject,
    projection: &HostSurfaceProjection,
    fullscreen: bool,
) -> TuiInlineView {
    TuiInlineView {
        schema_version: "tui_inline_view.v1".to_string(),
        renderer_mode: if fullscreen {
            "alternate_screen_split_pane"
        } else {
            "inline_terminal"
        }
        .to_string(),
        layout: if fullscreen {
            "advanced_fullscreen_chat_first"
        } else {
            "chat_first_repl"
        }
        .to_string(),
        authority_model: projection.authority_model.clone(),
        capabilities: inline_capabilities(),
        command_model: tui_command_model(&projection.actions),
        skill_model: tui_skill_model(state_home, &resolved.workspace_root),
        status_hud: inline_status_hud(projection, resolved),
        pane_plans: projection
            .pane_status
            .panes
            .iter()
            .map(inline_pane_plan)
            .collect(),
        keymap: inline_keymap(projection),
        theme: TuiThemePlan {
            theme_id: "light_default".to_string(),
            mode: "light_with_dark_available".to_string(),
            supports_dark_light: true,
            accent_source: "warm_orange_product_theme".to_string(),
        },
    }
}

fn inline_capabilities() -> Vec<TuiRendererCapability> {
    [
        (
            "composer_text_entry",
            "interactive",
            "prompt_turn_input_buffer",
        ),
        (
            "slash_command_router",
            "interactive",
            "canonical_command_registry",
        ),
        ("skill_invocation_router", "interactive", "skills.discover"),
        ("status_hud", "available", "kernel_state_bundle"),
        ("markdown_renderer", "available", "sessions.recent"),
        (
            "collapsible_tool_output",
            "projection_only",
            "events.tool_activity",
        ),
        ("colored_diff", "projection_only", "workbench.artifacts"),
        ("internal_pager", "projection_only", "pane_scroll_state"),
        (
            "permission_overlay",
            "projection_only",
            "permissions.pending",
        ),
        ("session_picker", "projection_only", "sessions.recent"),
        ("theme_selection", "available", "host_surface_theme_plan"),
    ]
    .into_iter()
    .map(|(capability_id, status, source)| TuiRendererCapability {
        capability_id: capability_id.to_string(),
        status: status.to_string(),
        source: source.to_string(),
    })
    .collect()
}

fn tui_command_model(actions: &[HostSurfaceAction]) -> TuiCommandModel {
    let lookup = |action_id: &str, fallback_label: &str, fallback_gate: &str| {
        actions
            .iter()
            .find(|action| action.action_id == action_id)
            .map(|action| (action.label.clone(), action.gate.clone()))
            .unwrap_or_else(|| (fallback_label.to_string(), fallback_gate.to_string()))
    };
    let specs = product_command_specs();
    let mut groups = Vec::<TuiCommandGroup>::new();

    for spec in specs {
        let group_index = groups
            .iter()
            .position(|group| group.group_id == spec.category)
            .unwrap_or_else(|| {
                groups.push(TuiCommandGroup {
                    group_id: spec.category.clone(),
                    label: tui_command_group_label(&spec.category),
                    commands: Vec::new(),
                });
                groups.len() - 1
            });
        let (label, gate) = lookup(&spec.action_id, &spec.label, &spec.category);
        groups[group_index].commands.push(TuiCommandEntry {
            typed: spec.typed,
            action_id: spec.action_id,
            label,
            summary: spec.summary,
            gate,
        });
    }

    TuiCommandModel {
        command_prefix: "/".to_string(),
        groups,
    }
}

fn tui_command_group_label(group_id: &str) -> String {
    match group_id {
        "conversation" => "Conversation".to_string(),
        "navigation" => "Navigation".to_string(),
        "session" => "Session".to_string(),
        "permissions" => "Permissions".to_string(),
        "source_control" => "Source Control".to_string(),
        "work" => "Work".to_string(),
        "output" => "Output".to_string(),
        "context" => "Context".to_string(),
        "usage" => "Usage".to_string(),
        "diagnostics" => "Diagnostics".to_string(),
        "configuration" => "Configuration".to_string(),
        "tools" => "Tools".to_string(),
        "research" => "Research".to_string(),
        _ => group_id
            .split('_')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn tui_skill_model(state_home: &Path, cwd: &Path) -> TuiSkillModel {
    let snapshot = skills::discover(state_home, cwd);
    let list = skills::list(&snapshot);
    TuiSkillModel {
        skill_prefix: "$".to_string(),
        total_count: list.total_count,
        degraded_count: list.degraded_count,
        entries: list
            .skills
            .into_iter()
            .map(|skill| TuiSkillEntry {
                typed: format!("${}", skill.skill_id),
                skill_id: skill.skill_id,
                description: skill.description,
                enabled: skill.enabled,
                degraded: skill.degraded,
                source: skill.source,
            })
            .collect(),
    }
}

fn inline_status_hud(
    projection: &HostSurfaceProjection,
    resolved: &ResolvedProject,
) -> Vec<TuiStatusBadge> {
    let effective = config::effective_config(resolved)
        .ok()
        .map(|report| report.effective);
    let permission_mode = effective
        .as_ref()
        .and_then(|config| config.permission_mode.clone())
        .unwrap_or_else(|| "read-only".to_string());
    let model_label = effective
        .as_ref()
        .and_then(|config| config.default_model.clone())
        .unwrap_or_else(|| "auto".to_string());
    let reasoning_effort = effective
        .as_ref()
        .and_then(|config| config.values.get("reasoning_effort"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    vec![
        TuiStatusBadge {
            badge_id: "project".to_string(),
            label: "Project".to_string(),
            state: projection.project_id.clone(),
            data_ref: "project_id".to_string(),
        },
        TuiStatusBadge {
            badge_id: "remote".to_string(),
            label: "Remote".to_string(),
            state: projection.surfaces.mobile.readiness.clone(),
            data_ref: "surfaces.mobile.readiness".to_string(),
        },
        TuiStatusBadge {
            badge_id: "permission_mode".to_string(),
            label: "Permission Mode".to_string(),
            state: permission_mode,
            data_ref: "config.permission_mode".to_string(),
        },
        TuiStatusBadge {
            badge_id: "model".to_string(),
            label: "Model".to_string(),
            state: model_label,
            data_ref: "config.default_model".to_string(),
        },
        TuiStatusBadge {
            badge_id: "reasoning".to_string(),
            label: "Reasoning".to_string(),
            state: reasoning_effort,
            data_ref: "config.values.reasoning_effort".to_string(),
        },
        TuiStatusBadge {
            badge_id: "theme".to_string(),
            label: "Theme".to_string(),
            state: "light_default".to_string(),
            data_ref: "inline_view.theme".to_string(),
        },
    ]
}

fn inline_pane_plan(pane: &HostPane) -> TuiPaneRenderPlan {
    TuiPaneRenderPlan {
        pane_id: pane.pane_id.clone(),
        renderer: renderer_for_pane(&pane.pane_id).to_string(),
        collapse_state: collapse_state_for_pane(pane).to_string(),
        pager: pager_for_pane(&pane.pane_id).to_string(),
        data_ref: pane.data_ref.clone(),
        empty_state: pane
            .degraded_reason
            .clone()
            .unwrap_or_else(|| "render projection slice".to_string()),
        action_refs: action_refs_for_pane(&pane.pane_id),
    }
}

fn renderer_for_pane(pane_id: &str) -> &'static str {
    match pane_id {
        "conversation" => "markdown_message_list",
        "input_editor" => "prompt_editor",
        "status_hud" => "status_hud",
        "tool_activity" => "collapsible_tool_output",
        "permission_overlay" => "modal_permission_overlay",
        "diff_artifact" => "colored_diff_view",
        "memory" => "memory_summary_table",
        "branch_review" => "branch_review_table",
        "research_dag" => "research_dag_timeline",
        "diagnostics" => "diagnostics_table",
        _ => "json_projection_table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_commands::product_command_specs;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    fn temp_host_surface_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "research_cli_host_surface_{label}_{}",
            timestamp_string()
        ));
        fs::create_dir_all(root.join(".pmcli")).expect("temp host surface dir should create");
        root
    }

    #[test]
    fn tui_command_model_exposes_every_product_slash_command() {
        let model = tui_command_model(&[]);
        let displayed = model
            .groups
            .iter()
            .flat_map(|group| group.commands.iter().map(|command| command.typed.as_str()))
            .collect::<BTreeSet<_>>();

        for spec in product_command_specs() {
            assert!(
                displayed.contains(spec.typed.as_str()),
                "missing slash command from TUI command model: {}",
                spec.typed
            );
        }
    }

    #[test]
    fn host_research_summary_no_active_thread_uses_product_brief_copy() {
        let summary = host_research_summary_from_projection("project", None);

        assert_eq!(summary.status, "no_active_thread");
        assert_eq!(summary.evidence_count, 0);
        assert!(summary.compact_research_line.contains("研究"));
        assert!(summary.active_question_label.contains("没有"));
        assert!(summary.stage_label.contains("未开始"));
        assert!(summary.evidence_label.contains("0"));
        assert!(summary.open_gap_label.contains("研究线程"));
        assert!(summary.next_action_label.contains("/research"));

        let visible_copy = [
            summary.compact_research_line.as_str(),
            summary.active_question_label.as_str(),
            summary.stage_label.as_str(),
            summary.evidence_label.as_str(),
            summary.open_gap_label.as_str(),
            summary.next_action_label.as_str(),
        ]
        .join(" ");
        assert!(!visible_copy.contains("record_or_resume_research_thread"));
        assert!(!visible_copy.contains("start_or_select_research_thread"));
        assert!(!visible_copy.contains("no_active_thread"));
    }

    #[test]
    fn host_research_summary_active_thread_projects_brief_not_debug_ids() {
        let summary = host_research_summary_from_projection(
            "project",
            Some(research::ResearchContextProjection {
                schema_version: "research_context_projection.v1".to_string(),
                conformance_line: "M10.research_runtime".to_string(),
                project_id: "project".to_string(),
                active_thread_id: "thread_123".to_string(),
                active_thread_title: "Balanced UX research brief".to_string(),
                active_stage_execution_id: "stage_123".to_string(),
                active_stage_id: "experiment-plan".to_string(),
                active_stage_class: "experiment_design".to_string(),
                active_deliberation_span_id: "span_123".to_string(),
                active_deliberation_mode: "reviewing".to_string(),
                pending_operations: Vec::new(),
                open_questions: vec!["TUI should show one open gap without pane noise".to_string()],
                agreed_decisions: vec!["Keep the first screen chat-first".to_string()],
                evidence_refs: vec![
                    "docs/superpowers/specs/ux.md#research-brief".to_string(),
                    "docs/deep_study/15.md#deliberation".to_string(),
                ],
                next_recommended_action: "inspect_or_advance_research_thread".to_string(),
                confidence: "medium".to_string(),
                projection_policy: "bounded_after_mission_frame_before_memory".to_string(),
            }),
        );

        assert_eq!(summary.status, "available");
        assert_eq!(summary.evidence_count, 2);
        assert_eq!(summary.board.schema_version, "research_board_projection.v1");
        assert_eq!(
            summary.board.authority_model,
            "derived_from_existing_research_authorities"
        );
        assert!(summary
            .board
            .entries
            .iter()
            .any(|entry| entry.bucket_id == "questions"));
        assert!(summary
            .board
            .entries
            .iter()
            .all(|entry| entry.write_authority == "read_only_projection"));
        assert_eq!(
            summary.active_question_label,
            "问题：Balanced UX research brief"
        );
        assert!(summary.stage_label.contains("实验计划"));
        assert!(summary.stage_label.contains("实验设计"));
        assert!(summary.evidence_label.contains("2"));
        assert!(summary.evidence_label.contains("ux.md#research-brief"));
        assert!(summary.open_gap_label.contains("open gap"));
        assert!(summary.next_action_label.contains("检查研究状态"));

        let visible_copy = [
            summary.compact_research_line.as_str(),
            summary.active_question_label.as_str(),
            summary.stage_label.as_str(),
            summary.evidence_label.as_str(),
            summary.open_gap_label.as_str(),
            summary.next_action_label.as_str(),
        ]
        .join(" ");
        assert!(!visible_copy.contains("inspect_or_advance_research_thread"));
        assert!(!visible_copy.contains("experiment-plan"));
        assert!(!visible_copy.contains("experiment_design"));
        assert!(!visible_copy.contains("thread_123"));
        assert!(!visible_copy.contains("span_123"));
    }

    #[test]
    fn host_surface_exposes_active_autonomous_research_job_and_resume_actions() {
        let root = temp_host_surface_dir("autonomous_job");
        let data_dir = root.join(".pmcli");
        let job_dir = data_dir.join("research").join("jobs").join("arj_active");
        fs::create_dir_all(&job_dir).expect("job dir should create");
        fs::write(
            job_dir.join("job.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": "autonomous_research_job.v1",
                "job_id": "arj_active",
                "status": "running",
                "phase": "stage_review",
                "automation_mode": "full_auto",
                "updated_at": "20",
                "stage_execution_id": "stage_literature_1",
                "artifact_refs": [
                    "research/stages/arj_active/literature/source_matrix.md"
                ],
                "obligations": [
                    {
                        "obligation_id": "obl_open",
                        "kind": "missing_stage_evidence",
                        "status": "open",
                        "blocking": true
                    },
                    {
                        "obligation_id": "obl_ack",
                        "kind": "stage_gate_blocking_failure",
                        "status": "acknowledged_by_main_agent",
                        "blocking": true
                    },
                    {
                        "obligation_id": "obl_done",
                        "kind": "missing_stage_evidence",
                        "status": "satisfied",
                        "blocking": true
                    }
                ],
                "continuity_packets": [
                    {
                        "artifact_path": "research/auto/arj_active/continuity/packet_1.json"
                    }
                ]
            }))
            .expect("job state should serialize"),
        )
        .expect("job state should write");

        let job = active_autonomous_research_job_summary(&data_dir)
            .expect("active autonomous research job should be projected");

        assert_eq!(job.job_id, "arj_active");
        assert_eq!(job.status, "running");
        assert_eq!(job.phase, "stage_review");
        assert_eq!(job.automation_mode, "full_auto");
        assert_eq!(job.active_stage_id.as_deref(), Some("literature"));
        assert_eq!(job.open_blocking_obligation_count, 1);
        assert_eq!(job.active_blocking_obligation_count, 2);
        assert_eq!(
            job.latest_continuity_packet_ref.as_deref(),
            Some("research/auto/arj_active/continuity/packet_1.json")
        );
        assert_eq!(
            job.resume_command,
            vec!["research", "jobs", "resume", "arj_active"]
        );
        assert!(job
            .next_required_action
            .contains("open blocking obligations"));

        let actions = default_actions(false, false, None, Some(&job));
        let action_ids = actions
            .iter()
            .map(|action| action.action_id.as_str())
            .collect::<BTreeSet<_>>();
        assert!(action_ids.contains("resume_autonomous_research"));
        assert!(action_ids.contains("tick_autonomous_research"));
        assert!(action_ids.contains("stop_autonomous_research"));
        assert!(actions.iter().any(|action| {
            action.action_id == "open_research_board" && action.label == "Open Research Board"
        }));
    }
}

fn collapse_state_for_pane(pane: &HostPane) -> &'static str {
    match pane.default_visibility.as_str() {
        "visible" => "expanded",
        "hidden" => "hidden",
        _ => "collapsed",
    }
}

fn pager_for_pane(pane_id: &str) -> &'static str {
    match pane_id {
        "conversation" | "tool_activity" | "diff_artifact" | "research_dag" => "internal_pager",
        _ => "none",
    }
}

fn action_refs_for_pane(pane_id: &str) -> Vec<String> {
    match pane_id {
        "conversation" | "input_editor" => {
            vec!["submit_prompt".to_string(), "steer_turn".to_string()]
        }
        "permission_overlay" => vec![
            "approve_permission".to_string(),
            "deny_permission".to_string(),
            "ack_hitl".to_string(),
        ],
        "diff_artifact" => vec!["open_artifact".to_string()],
        "memory" => vec!["inspect_memory".to_string()],
        "tool_activity" => vec!["interrupt_turn".to_string()],
        "status_hud" => vec!["terminal_attach".to_string(), "terminal_replay".to_string()],
        _ => Vec::new(),
    }
}

fn inline_keymap(projection: &HostSurfaceProjection) -> Vec<TuiKeyBinding> {
    projection
        .actions
        .iter()
        .filter_map(|action| {
            let key = match action.action_id.as_str() {
                "submit_prompt" => "ctrl+p",
                "switch_session" => "ctrl+s",
                "approve_permission" => "ctrl+a",
                "deny_permission" => "ctrl+x",
                "terminal_replay" => "ctrl+t",
                "inspect_memory" => "ctrl+m",
                _ => return None,
            };
            Some(TuiKeyBinding {
                key: key.to_string(),
                action_id: action.action_id.clone(),
                gate: action.gate.clone(),
            })
        })
        .collect()
}

fn default_actions(
    remote_ready: bool,
    permission_pending: bool,
    active_session_id: Option<&str>,
    autonomous_job: Option<&HostAutonomousResearchJobSummary>,
) -> Vec<HostSurfaceAction> {
    let session_arg = active_session_id.unwrap_or("latest").to_string();
    let active_session_ready = active_session_id.is_some();
    let mut actions = vec![
        HostSurfaceAction {
            action_id: "submit_prompt".to_string(),
            label: "Submit Prompt".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "prompt_turn".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec!["prompt".to_string(), "<text>".to_string()],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "steer_turn".to_string(),
            label: "Steer Turn".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "message_steering".to_string(),
            enabled: active_session_ready,
            disabled_reason: if active_session_ready {
                None
            } else {
                Some("no active session to steer".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "remote".to_string(),
                "notify".to_string(),
                "--session".to_string(),
                session_arg.clone(),
                "--message".to_string(),
                "<text>".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "interrupt_turn".to_string(),
            label: "Request Interrupt".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "control_lease_required".to_string(),
            enabled: active_session_ready && remote_ready,
            disabled_reason: if active_session_ready && remote_ready {
                None
            } else {
                Some("active session and paired remote control are required".to_string())
            },
            control_contract: Some("direct_runtime_cancellation".to_string()),
            fallback_control_contract: Some("notification_backed_interrupt_request".to_string()),
            command: vec![
                "remote".to_string(),
                "tui".to_string(),
                "action".to_string(),
                "interrupt_turn".to_string(),
                "<reason>".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "approve_permission".to_string(),
            label: "Approve Permission".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "permission_queue".to_string(),
            enabled: permission_pending,
            disabled_reason: if permission_pending {
                None
            } else {
                Some("no pending permission requests".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "permissions".to_string(),
                "approve".to_string(),
                "<request-id>".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "deny_permission".to_string(),
            label: "Deny Permission".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "permission_queue".to_string(),
            enabled: permission_pending,
            disabled_reason: if permission_pending {
                None
            } else {
                Some("no pending permission requests".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "permissions".to_string(),
                "deny".to_string(),
                "<request-id>".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "ack_hitl".to_string(),
            label: "Acknowledge HITL".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "hitl_checkpoint".to_string(),
            enabled: permission_pending,
            disabled_reason: if permission_pending {
                None
            } else {
                Some("no active HITL checkpoint".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "permissions".to_string(),
                "history".to_string(),
                "--limit".to_string(),
                "1".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "open_artifact".to_string(),
            label: "Open Artifact".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "artifact_registry".to_string(),
            enabled: false,
            disabled_reason: Some("select an artifact family before opening".to_string()),
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "artifacts".to_string(),
                "inspect".to_string(),
                "<artifact-id>".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "inspect_memory".to_string(),
            label: "Inspect Memory".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "memory_projection".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec!["memory".to_string(), "status".to_string()],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "open_research".to_string(),
            label: "Open Research".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "research_projection".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec!["research".to_string(), "status".to_string()],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "open_research_board".to_string(),
            label: "Open Research Board".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "research_board_projection".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "research".to_string(),
                "board".to_string(),
                "--json".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "open_routines".to_string(),
            label: "Open Routines".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "routine_projection".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec!["routines".to_string(), "list".to_string()],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "inspect_recovery_governance".to_string(),
            label: "Inspect Recovery Governance".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "routine_recovery_projection".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec!["routines".to_string(), "list".to_string()],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "retry_routine_trigger".to_string(),
            label: "Retry Routine Trigger".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "routine_recovery_policy".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: Some("routine_retry_budget_and_backoff".to_string()),
            fallback_control_contract: None,
            command: vec![
                "routines".to_string(),
                "retry".to_string(),
                "<trigger-id>".to_string(),
            ],
            requires_trigger_id: true,
        },
        HostSurfaceAction {
            action_id: "advance_research_loop".to_string(),
            label: "Advance Research Loop".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "goal_automation_policy".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: Some("goal_tick_existing_authorities".to_string()),
            fallback_control_contract: None,
            command: vec!["goals".to_string(), "tick".to_string()],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "decide_research_stage".to_string(),
            label: "Decide Research Stage".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "research_stage_decision_policy".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: Some("research_decide_existing_stage_gate".to_string()),
            fallback_control_contract: None,
            command: vec![
                "research".to_string(),
                "decide".to_string(),
                "--thread".to_string(),
                "<active-thread-id>".to_string(),
                "--operation".to_string(),
                "advance".to_string(),
                "--decision".to_string(),
                "approve".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "switch_session".to_string(),
            label: "Switch Session".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "session_registry".to_string(),
            enabled: true,
            disabled_reason: None,
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "sessions".to_string(),
                "resume".to_string(),
                "<session-id>".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "remote_attach".to_string(),
            label: "Attach Remote".to_string(),
            surface: "web_mobile".to_string(),
            gate: "remote_control_lease".to_string(),
            enabled: remote_ready,
            disabled_reason: if remote_ready {
                None
            } else {
                Some("pair a remote client first".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "remote".to_string(),
                "attach".to_string(),
                "--session".to_string(),
                session_arg.clone(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "terminal_attach".to_string(),
            label: "Attach Terminal".to_string(),
            surface: "web_mobile_tui".to_string(),
            gate: "remote_control_lease".to_string(),
            enabled: remote_ready,
            disabled_reason: if remote_ready {
                None
            } else {
                Some("terminal lane requires paired remote control".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "remote".to_string(),
                "terminal".to_string(),
                "attach".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "terminal_replay".to_string(),
            label: "Replay Terminal".to_string(),
            surface: "web_mobile_tui".to_string(),
            gate: "remote_control_lease".to_string(),
            enabled: remote_ready,
            disabled_reason: if remote_ready {
                None
            } else {
                Some("terminal replay requires paired remote control".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "remote".to_string(),
                "terminal".to_string(),
                "replay".to_string(),
            ],
            requires_trigger_id: false,
        },
        HostSurfaceAction {
            action_id: "resize_terminal".to_string(),
            label: "Resize Terminal".to_string(),
            surface: "web_mobile".to_string(),
            gate: "remote_control_lease".to_string(),
            enabled: remote_ready,
            disabled_reason: if remote_ready {
                None
            } else {
                Some("terminal resize requires paired remote control".to_string())
            },
            control_contract: None,
            fallback_control_contract: None,
            command: vec![
                "remote".to_string(),
                "terminal".to_string(),
                "resize".to_string(),
                "--cols".to_string(),
                "<cols>".to_string(),
                "--rows".to_string(),
                "<rows>".to_string(),
            ],
            requires_trigger_id: false,
        },
    ];
    if let Some(job) = autonomous_job {
        let running_job = !matches!(job.status.as_str(), "completed" | "failed" | "stopped");
        actions.push(HostSurfaceAction {
            action_id: "resume_autonomous_research".to_string(),
            label: "Resume Autonomous Research".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "autonomous_research_budget_policy_obligation_gate".to_string(),
            enabled: running_job,
            disabled_reason: if running_job {
                None
            } else {
                Some("autonomous research job is terminal".to_string())
            },
            control_contract: Some("research_jobs_resume_existing_supervisor_loop".to_string()),
            fallback_control_contract: None,
            command: job.resume_command.clone(),
            requires_trigger_id: false,
        });
        actions.push(HostSurfaceAction {
            action_id: "tick_autonomous_research".to_string(),
            label: "Tick Autonomous Research".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "autonomous_research_single_tick_policy_obligation_gate".to_string(),
            enabled: running_job,
            disabled_reason: if running_job {
                None
            } else {
                Some("autonomous research job is terminal".to_string())
            },
            control_contract: Some("research_jobs_tick_existing_supervisor_loop".to_string()),
            fallback_control_contract: None,
            command: job.tick_command.clone(),
            requires_trigger_id: false,
        });
        actions.push(HostSurfaceAction {
            action_id: "stop_autonomous_research".to_string(),
            label: "Stop Autonomous Research".to_string(),
            surface: "tui_web_mobile".to_string(),
            gate: "operator_stop".to_string(),
            enabled: running_job,
            disabled_reason: if running_job {
                None
            } else {
                Some("autonomous research job is terminal".to_string())
            },
            control_contract: Some("research_jobs_stop_process_group".to_string()),
            fallback_control_contract: None,
            command: job.stop_command.clone(),
            requires_trigger_id: false,
        });
    }
    actions
}

fn parse_tailscale_addresses(raw: &str) -> Vec<String> {
    raw.split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|part| is_tailscale_ipv4(part))
        .map(ToString::to_string)
        .collect()
}

fn detect_tailscale_interface_addresses() -> Vec<String> {
    let output = Command::new("ip").args(["-4", "addr"]).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    parse_tailscale_addresses(&raw.replace('/', " "))
}

fn is_tailscale_ipv4(value: &str) -> bool {
    let octets = value
        .split('.')
        .filter_map(|part| part.parse::<u8>().ok())
        .collect::<Vec<_>>();
    if octets.len() != 4 {
        return false;
    }
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn write_terminal_projection(
    data_dir: &Path,
    terminal: &GovernedTerminalProjection,
) -> Result<(), HostSurfaceError> {
    let path = data_dir.join("remote").join("terminal_projection.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(terminal).unwrap())?;
    Ok(())
}

fn load_terminal_projection(
    data_dir: &Path,
) -> Result<Option<GovernedTerminalProjection>, HostSurfaceError> {
    let path = data_dir.join("remote").join("terminal_projection.json");
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents).ok())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
