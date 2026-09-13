use crate::goals::GoalAutomationMode;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ORCHESTRATION_SCHEMA_VERSION: &str = "orchestration.run.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationStepStatus {
    Pending,
    #[serde(alias = "active", alias = "started", alias = "reclaimed")]
    Running,
    Done,
    Blocked,
    Failed,
    Cancelled,
}

impl OrchestrationStepStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" | "active" | "started" | "reclaimed" => Some(Self::Running),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    fn checkbox(&self) -> &'static str {
        match self {
            Self::Done => "x",
            _ => " ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationStep {
    pub step_id: String,
    pub title: String,
    pub status: OrchestrationStepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    pub gates: Vec<String>,
    pub artifacts: Vec<String>,
    pub continuation_points: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weighted_progress: Option<u8>,
}

impl OrchestrationStep {
    pub fn new(
        step_id: impl Into<String>,
        title: impl Into<String>,
        status: OrchestrationStepStatus,
    ) -> Self {
        Self {
            step_id: step_id.into(),
            title: title.into(),
            status,
            worker: None,
            gates: Vec::new(),
            artifacts: Vec::new(),
            continuation_points: Vec::new(),
            weighted_progress: None,
        }
    }

    pub fn with_worker(mut self, worker: impl Into<String>) -> Self {
        self.worker = Some(worker.into());
        self
    }

    pub fn with_gate(mut self, gate: impl Into<String>) -> Self {
        self.gates.push(gate.into());
        self
    }

    pub fn with_artifact(mut self, artifact: impl Into<String>) -> Self {
        self.artifacts.push(artifact.into());
        self
    }

    pub fn with_continuation(mut self, continuation: impl Into<String>) -> Self {
        self.continuation_points.push(continuation.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunControlCommand {
    pub command_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    pub requested_by: String,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

impl RunControlCommand {
    pub fn new(command: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::with_step(command, None, reason)
    }

    pub fn with_step(
        command: impl Into<String>,
        step_id: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        let now = timestamp_string();
        Self {
            command_id: format!("rctl_{now}"),
            command: command.into(),
            step_id,
            requested_by: "main_agent".to_string(),
            reason: reason.into(),
            status: "pending".to_string(),
            created_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationRun {
    pub schema_version: String,
    pub run_id: String,
    pub objective: String,
    pub status: String,
    pub start_policy: String,
    #[serde(default)]
    pub automation_mode: GoalAutomationMode,
    pub steps: Vec<OrchestrationStep>,
    pub control_commands: Vec<RunControlCommand>,
    pub created_at: String,
    pub updated_at: String,
}

impl OrchestrationRun {
    pub fn new(
        run_id: impl Into<String>,
        objective: impl Into<String>,
        start_policy: impl Into<String>,
    ) -> Self {
        let now = timestamp_string();
        Self {
            schema_version: ORCHESTRATION_SCHEMA_VERSION.to_string(),
            run_id: run_id.into(),
            objective: objective.into(),
            status: "planning".to_string(),
            start_policy: start_policy.into(),
            automation_mode: GoalAutomationMode::default(),
            steps: Vec::new(),
            control_commands: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn progress_projection(&self) -> OrchestrationProgressProjection {
        let total_steps = self.steps.len();
        let done_steps = self
            .steps
            .iter()
            .filter(|step| step.status == OrchestrationStepStatus::Done)
            .count();
        let blocked_steps = self
            .steps
            .iter()
            .filter(|step| step.status == OrchestrationStepStatus::Blocked)
            .count();
        let current_step = self
            .steps
            .iter()
            .find(|step| step.status == OrchestrationStepStatus::Running)
            .or_else(|| {
                self.steps
                    .iter()
                    .find(|step| step.status == OrchestrationStepStatus::Blocked)
            })
            .or_else(|| {
                self.steps
                    .iter()
                    .find(|step| step.status == OrchestrationStepStatus::Pending)
            });
        let percent = if total_steps == 0 {
            0
        } else {
            ((done_steps * 100) / total_steps) as u8
        };
        let current_step_id = current_step.map(|step| step.step_id.clone());
        let current_step_title = current_step.map(|step| step.title.clone());
        let current_label = current_step_title
            .as_deref()
            .map(|title| format!("current: {title}"))
            .unwrap_or_else(|| "no active step".to_string());
        OrchestrationProgressProjection {
            schema_version: "orchestration.progress.v1".to_string(),
            run_id: self.run_id.clone(),
            objective: self.objective.clone(),
            status: self.status.clone(),
            done_steps,
            total_steps,
            blocked_steps,
            percent,
            current_step_id,
            current_step_title,
            compact_line: format!(
                "{} | {}/{} · {}% · {}",
                self.objective, done_steps, total_steps, percent, current_label
            ),
        }
    }

    pub fn automation_policy_projection(&self) -> GoalRunAutomationPolicyProjection {
        GoalRunAutomationPolicyProjection::from_mode(self.automation_mode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationProgressProjection {
    pub schema_version: String,
    pub run_id: String,
    pub objective: String,
    pub status: String,
    pub done_steps: usize,
    pub total_steps: usize,
    pub blocked_steps: usize,
    pub percent: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step_title: Option<String>,
    pub compact_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalRunActionPolicy {
    Allowed,
    RequiresApproval,
    HardBlocked,
}

impl GoalRunActionPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::RequiresApproval => "requires_approval",
            Self::HardBlocked => "hard_blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalRunAutomationActionPolicy {
    pub action: String,
    pub policy: GoalRunActionPolicy,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalRunAutomationPolicyProjection {
    pub schema_version: String,
    pub policy_id: String,
    pub automation_mode: GoalAutomationMode,
    pub source: String,
    pub action_policies: Vec<GoalRunAutomationActionPolicy>,
    pub hard_boundaries: Vec<String>,
}

impl GoalRunAutomationPolicyProjection {
    pub fn from_mode(automation_mode: GoalAutomationMode) -> Self {
        Self {
            schema_version: "goal_run_automation_policy.v1".to_string(),
            policy_id: format!("goal_run_policy_{}", automation_mode.as_str()),
            automation_mode,
            source: "mission_frame_orchestration_run".to_string(),
            action_policies: action_policies_for_mode(automation_mode),
            hard_boundaries: vec![
                "budget".to_string(),
                "permission".to_string(),
                "review".to_string(),
                "irreversible_operation".to_string(),
                "external_publish".to_string(),
            ],
        }
    }

    pub fn action_policy(&self, action: &str) -> Option<&'static str> {
        self.action_policies
            .iter()
            .find(|policy| policy.action == action)
            .map(|policy| policy.policy.as_str())
    }
}

fn action_policies_for_mode(
    automation_mode: GoalAutomationMode,
) -> Vec<GoalRunAutomationActionPolicy> {
    let policies = match automation_mode {
        GoalAutomationMode::HumanInTheLoop => vec![
            ("auto_decompose", GoalRunActionPolicy::RequiresApproval),
            (
                "create_or_update_orchestration_step",
                GoalRunActionPolicy::RequiresApproval,
            ),
            ("dispatch_agent_task", GoalRunActionPolicy::RequiresApproval),
            (
                "start_long_running_task",
                GoalRunActionPolicy::RequiresApproval,
            ),
            ("accept_normal_task", GoalRunActionPolicy::RequiresApproval),
            ("initiate_review", GoalRunActionPolicy::RequiresApproval),
            ("advance_stage", GoalRunActionPolicy::RequiresApproval),
            ("repair_failed_task", GoalRunActionPolicy::RequiresApproval),
            ("pivot", GoalRunActionPolicy::RequiresApproval),
            ("promote_memory", GoalRunActionPolicy::RequiresApproval),
            ("finish_goal", GoalRunActionPolicy::RequiresApproval),
            ("external_publish", GoalRunActionPolicy::HardBlocked),
        ],
        GoalAutomationMode::HighAutonomy => vec![
            ("auto_decompose", GoalRunActionPolicy::Allowed),
            (
                "create_or_update_orchestration_step",
                GoalRunActionPolicy::Allowed,
            ),
            ("dispatch_agent_task", GoalRunActionPolicy::Allowed),
            (
                "start_long_running_task",
                GoalRunActionPolicy::RequiresApproval,
            ),
            ("accept_normal_task", GoalRunActionPolicy::RequiresApproval),
            ("initiate_review", GoalRunActionPolicy::RequiresApproval),
            ("advance_stage", GoalRunActionPolicy::RequiresApproval),
            ("repair_failed_task", GoalRunActionPolicy::Allowed),
            ("pivot", GoalRunActionPolicy::RequiresApproval),
            ("promote_memory", GoalRunActionPolicy::RequiresApproval),
            ("finish_goal", GoalRunActionPolicy::RequiresApproval),
            ("external_publish", GoalRunActionPolicy::HardBlocked),
        ],
        GoalAutomationMode::FullAuto => vec![
            ("auto_decompose", GoalRunActionPolicy::Allowed),
            (
                "create_or_update_orchestration_step",
                GoalRunActionPolicy::Allowed,
            ),
            ("dispatch_agent_task", GoalRunActionPolicy::Allowed),
            ("start_long_running_task", GoalRunActionPolicy::Allowed),
            ("accept_normal_task", GoalRunActionPolicy::Allowed),
            ("initiate_review", GoalRunActionPolicy::Allowed),
            ("advance_stage", GoalRunActionPolicy::Allowed),
            ("repair_failed_task", GoalRunActionPolicy::Allowed),
            ("pivot", GoalRunActionPolicy::RequiresApproval),
            ("promote_memory", GoalRunActionPolicy::Allowed),
            ("finish_goal", GoalRunActionPolicy::Allowed),
            ("external_publish", GoalRunActionPolicy::HardBlocked),
        ],
    };

    policies
        .into_iter()
        .map(|(action, policy)| GoalRunAutomationActionPolicy {
            action: action.to_string(),
            policy,
            reason: action_policy_reason(automation_mode, action, policy).to_string(),
        })
        .collect()
}

fn action_policy_reason(
    _automation_mode: GoalAutomationMode,
    action: &str,
    policy: GoalRunActionPolicy,
) -> &'static str {
    match policy {
        GoalRunActionPolicy::Allowed => "within selected automation mode and existing gates",
        GoalRunActionPolicy::RequiresApproval => match action {
            "initiate_review" | "advance_stage" | "finish_goal" => {
                "review or completion boundary requires an explicit gate"
            }
            "start_long_running_task" => {
                "long-running work must respect budget and permission gates"
            }
            "pivot" => "direction changes remain user-visible approval points",
            _ => "selected automation mode keeps this action approval-gated",
        },
        GoalRunActionPolicy::HardBlocked => "hard boundary cannot be crossed by automation",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunControlAction {
    Status,
    Pause,
    Resume,
    Retry,
    Skip,
    Replan,
    Accept,
    Abort,
}

impl RunControlAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Retry => "retry",
            Self::Skip => "skip",
            Self::Replan => "replan",
            Self::Accept => "accept",
            Self::Abort => "abort",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "status" => Some(Self::Status),
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "retry" => Some(Self::Retry),
            "skip" => Some(Self::Skip),
            "replan" => Some(Self::Replan),
            "accept" => Some(Self::Accept),
            "abort" => Some(Self::Abort),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunControlResult {
    pub schema_version: String,
    pub action: RunControlAction,
    pub run: OrchestrationRun,
    pub progress: OrchestrationProgressProjection,
    pub command: RunControlCommand,
    pub recovery_context: RecoveryDecisionContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryDecisionContext {
    pub schema_version: String,
    pub snapshot_summary: Vec<String>,
    pub event_summary: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_run: Option<OrchestrationRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<OrchestrationProgressProjection>,
    pub control_commands: Vec<RunControlCommand>,
    pub permission_summary: Vec<String>,
    pub repo_summary: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_summary: Option<RecoverySessionSummary>,
    pub recommended_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverySessionSummary {
    pub session_id: String,
    pub transcript_line_count: usize,
    pub recent_lines: Vec<String>,
}

#[derive(Debug)]
pub enum OrchestrationError {
    InvalidMarkdown(String),
    Io { path: PathBuf, message: String },
    Serde(String),
    NoActiveRun(String),
    InvalidControl(String),
}

impl fmt::Display for OrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMarkdown(message) => {
                write!(f, "invalid orchestration markdown: {message}")
            }
            Self::Io { path, message } => {
                write!(f, "orchestration io error at {}: {message}", path.display())
            }
            Self::Serde(message) => write!(f, "orchestration serialization error: {message}"),
            Self::NoActiveRun(message) => write!(f, "{message}"),
            Self::InvalidControl(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for OrchestrationError {}

pub fn render_run_markdown(run: &OrchestrationRun) -> String {
    let mut lines = vec![
        format!("# Orchestration Run {}", run.run_id),
        String::new(),
        format!("- objective: {}", run.objective),
        format!("- status: {}", run.status),
        format!("- start_policy: {}", run.start_policy),
        String::new(),
        "## Checklist".to_string(),
    ];

    for step in &run.steps {
        lines.push(format!(
            "- [{}] <!-- step:{} status:{} --> {}",
            step.status.checkbox(),
            step.step_id,
            step.status.as_str(),
            step.title
        ));
        if let Some(worker) = step.worker.as_deref() {
            lines.push(format!("  - worker: {worker}"));
        }
        for gate in &step.gates {
            lines.push(format!("  - gate: {gate}"));
        }
        for artifact in &step.artifacts {
            lines.push(format!("  - artifact: {artifact}"));
        }
        for continuation in &step.continuation_points {
            lines.push(format!("  - continuation: {continuation}"));
        }
    }

    lines.push(String::new());
    lines.push("## Projection".to_string());
    lines.push(format!(
        "- progress: {}",
        run.progress_projection().compact_line
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goals::GoalAutomationMode;

    #[test]
    fn orchestration_run_projects_goal_automation_policy_by_mode() {
        let mut run = OrchestrationRun::new("run_auto_001", "Goal-driven automation", "approved");
        run.automation_mode = GoalAutomationMode::HighAutonomy;

        let policy = run.automation_policy_projection();

        assert_eq!(policy.automation_mode, GoalAutomationMode::HighAutonomy);
        assert_eq!(policy.action_policy("dispatch_agent_task"), Some("allowed"));
        assert_eq!(
            policy.action_policy("accept_normal_task"),
            Some("requires_approval")
        );
        assert_eq!(
            policy.action_policy("external_publish"),
            Some("hard_blocked")
        );

        let mut full_auto_run =
            OrchestrationRun::new("run_full_auto_001", "Unattended research", "approved");
        full_auto_run.automation_mode = GoalAutomationMode::FullAuto;
        let full_auto_policy = full_auto_run.automation_policy_projection();

        assert_eq!(
            full_auto_policy.action_policy("initiate_review"),
            Some("allowed")
        );
        assert_eq!(
            full_auto_policy.action_policy("external_publish"),
            Some("hard_blocked")
        );
    }
}

pub fn reconcile_run_from_markdown(
    base: &OrchestrationRun,
    markdown: &str,
) -> Result<OrchestrationRun, OrchestrationError> {
    let mut reconciled = base.clone();
    reconciled.steps.clear();
    let mut current: Option<OrchestrationStep> = None;

    for raw in markdown.lines() {
        let line = raw.trim_end();
        if let Some((step_id, status, title)) = parse_step_line(line)? {
            if let Some(step) = current.take() {
                reconciled.steps.push(step);
            }
            let mut step = base
                .steps
                .iter()
                .find(|candidate| candidate.step_id == step_id)
                .cloned()
                .unwrap_or_else(|| OrchestrationStep::new(&step_id, &title, status.clone()));
            step.step_id = step_id;
            step.title = title;
            step.status = status;
            step.worker = None;
            step.gates.clear();
            step.artifacts.clear();
            step.continuation_points.clear();
            current = Some(step);
            continue;
        }

        if let Some(step) = current.as_mut() {
            let trimmed = line.trim_start();
            if let Some(value) = trimmed.strip_prefix("- worker: ") {
                step.worker = Some(value.to_string());
            } else if let Some(value) = trimmed.strip_prefix("- gate: ") {
                push_unique(&mut step.gates, value.to_string());
            } else if let Some(value) = trimmed.strip_prefix("- artifact: ") {
                push_unique(&mut step.artifacts, value.to_string());
            } else if let Some(value) = trimmed.strip_prefix("- continuation: ") {
                push_unique(&mut step.continuation_points, value.to_string());
            }
        }
    }

    if let Some(step) = current.take() {
        reconciled.steps.push(step);
    }

    if reconciled.steps.is_empty() && !base.steps.is_empty() {
        return Err(OrchestrationError::InvalidMarkdown(
            "no checklist step markers found".to_string(),
        ));
    }
    reconciled.updated_at = timestamp_string();
    Ok(reconciled)
}

pub fn save_run(data_dir: &Path, run: &OrchestrationRun) -> Result<PathBuf, OrchestrationError> {
    let dir = orchestration_dir(data_dir);
    let run_dir = dir.join(&run.run_id);
    fs::create_dir_all(&run_dir).map_err(|err| OrchestrationError::Io {
        path: dir.clone(),
        message: err.to_string(),
    })?;
    let path = run_dir.join("orchestration.json");
    let content = serde_json::to_string_pretty(run)
        .map_err(|err| OrchestrationError::Serde(err.to_string()))?;
    fs::write(&path, content).map_err(|err| OrchestrationError::Io {
        path: path.clone(),
        message: err.to_string(),
    })?;
    let markdown_path = run_dir.join("orchestration.md");
    fs::write(&markdown_path, render_run_markdown(run)).map_err(|err| OrchestrationError::Io {
        path: markdown_path,
        message: err.to_string(),
    })?;
    fs::write(dir.join("active_run"), &run.run_id).map_err(|err| OrchestrationError::Io {
        path: dir.join("active_run"),
        message: err.to_string(),
    })?;
    Ok(path)
}

pub fn load_active_run(data_dir: &Path) -> Result<Option<OrchestrationRun>, OrchestrationError> {
    let dir = orchestration_dir(data_dir);
    let active_path = dir.join("active_run");
    if !active_path.exists() {
        return Ok(None);
    }
    let run_id = fs::read_to_string(&active_path)
        .map_err(|err| OrchestrationError::Io {
            path: active_path.clone(),
            message: err.to_string(),
        })?
        .trim()
        .to_string();
    if run_id.is_empty() {
        return Ok(None);
    }
    let path = dir.join(format!("{run_id}.json"));
    let legacy_path = path.clone();
    let path = {
        let run_dir = dir.join(&run_id);
        let candidate = run_dir.join("orchestration.json");
        if candidate.exists() {
            candidate
        } else {
            legacy_path
        }
    };
    let content = fs::read_to_string(&path).map_err(|err| OrchestrationError::Io {
        path: path.clone(),
        message: err.to_string(),
    })?;
    let run =
        serde_json::from_str(&content).map_err(|err| OrchestrationError::Serde(err.to_string()))?;
    Ok(Some(run))
}

pub fn append_active_run_step_artifacts(
    data_dir: &Path,
    step_id: &str,
    artifact_refs: &[String],
) -> Result<Option<OrchestrationRun>, OrchestrationError> {
    let Some(mut run) = load_active_run(data_dir)? else {
        return Ok(None);
    };
    if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
        for artifact in artifact_refs {
            push_unique(&mut step.artifacts, artifact.clone());
        }
    } else {
        let mut step = OrchestrationStep::new(
            step_id,
            "Main-agent structured board records",
            OrchestrationStepStatus::Running,
        )
        .with_worker("main-agent");
        for artifact in artifact_refs {
            step.artifacts.push(artifact.clone());
        }
        run.steps.push(step);
    }
    run.updated_at = timestamp_string();
    save_run(data_dir, &run)?;
    Ok(Some(run))
}

pub fn load_active_run_progress_summary(data_dir: &Path) -> Option<String> {
    load_active_run(data_dir)
        .ok()
        .flatten()
        .map(|run| run.progress_projection().compact_line)
}

pub fn apply_run_control(
    data_dir: &Path,
    action: RunControlAction,
    step_id: Option<&str>,
    requested_by: &str,
) -> Result<RunControlResult, OrchestrationError> {
    let mut run = load_active_run(data_dir)?.ok_or_else(|| {
        OrchestrationError::NoActiveRun("no active orchestration run found".to_string())
    })?;
    let mut command = RunControlCommand::with_step(
        action.as_str(),
        step_id.map(ToString::to_string),
        format!("{requested_by} requested {}", action.as_str()),
    );
    command.requested_by = requested_by.to_string();
    command.status = "applied".to_string();

    match action {
        RunControlAction::Status => {}
        RunControlAction::Pause => {
            run.status = "paused".to_string();
        }
        RunControlAction::Resume => {
            run.status = "running".to_string();
            for step in &mut run.steps {
                if step.status == OrchestrationStepStatus::Blocked {
                    step.status = OrchestrationStepStatus::Running;
                    break;
                }
            }
        }
        RunControlAction::Retry => {
            run.status = "running".to_string();
            require_target_step_status(
                &mut run,
                action,
                step_id,
                &[
                    OrchestrationStepStatus::Failed,
                    OrchestrationStepStatus::Blocked,
                ],
                OrchestrationStepStatus::Running,
            )?;
        }
        RunControlAction::Skip => {
            require_target_step_status(
                &mut run,
                action,
                step_id,
                &[
                    OrchestrationStepStatus::Pending,
                    OrchestrationStepStatus::Blocked,
                ],
                OrchestrationStepStatus::Cancelled,
            )?;
        }
        RunControlAction::Replan => {
            run.status = "replanning".to_string();
            if let Some(target) = step_id {
                set_step_status(&mut run, target, OrchestrationStepStatus::Blocked);
            }
        }
        RunControlAction::Accept => {
            run.status = "accepted".to_string();
        }
        RunControlAction::Abort => {
            run.status = "aborted".to_string();
            for step in &mut run.steps {
                if matches!(
                    step.status,
                    OrchestrationStepStatus::Running
                        | OrchestrationStepStatus::Pending
                        | OrchestrationStepStatus::Blocked
                ) {
                    step.status = OrchestrationStepStatus::Cancelled;
                }
            }
        }
    }

    run.control_commands.push(command.clone());
    run.updated_at = timestamp_string();
    save_run(data_dir, &run)?;
    let workspace_root = data_dir.parent().unwrap_or(data_dir);
    let recovery_context = build_recovery_decision_context(data_dir, workspace_root, None)?;
    Ok(RunControlResult {
        schema_version: "orchestration.run_control_result.v1".to_string(),
        action,
        progress: run.progress_projection(),
        run,
        command,
        recovery_context,
    })
}

pub fn build_recovery_decision_context(
    data_dir: &Path,
    workspace_root: &Path,
    session_id: Option<&str>,
) -> Result<RecoveryDecisionContext, OrchestrationError> {
    let active_run = load_active_run(data_dir)?;
    let progress = active_run
        .as_ref()
        .map(OrchestrationRun::progress_projection);
    let control_commands = active_run
        .as_ref()
        .map(|run| {
            run.control_commands
                .iter()
                .rev()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let snapshot_summary = active_run
        .as_ref()
        .map(snapshot_summary_for_run)
        .unwrap_or_else(|| vec!["no active orchestration run".to_string()]);
    let event_summary = read_event_summary(data_dir);
    let permission_summary = read_permission_summary(data_dir);
    let repo_summary = repo_summary(workspace_root);
    let session_summary = session_id.and_then(|id| recovery_session_summary(data_dir, id).ok());
    let recommended_actions = recommended_recovery_actions(
        active_run.as_ref(),
        progress.as_ref(),
        &control_commands,
        &permission_summary,
    );

    Ok(RecoveryDecisionContext {
        schema_version: "orchestration.recovery_decision_context.v1".to_string(),
        snapshot_summary,
        event_summary,
        active_run,
        progress,
        control_commands,
        permission_summary,
        repo_summary,
        session_summary,
        recommended_actions,
    })
}

pub fn active_orchestration_summary_markdown(data_dir: &Path) -> Option<String> {
    let run = load_active_run(data_dir).ok().flatten()?;
    let progress = run.progress_projection();
    let mut lines = vec![
        "## Active Orchestration".to_string(),
        String::new(),
        format!("- run_id: {}", run.run_id),
        format!("- status: {}", run.status),
        format!("- objective: {}", run.objective),
        format!(
            "- progress: {}/{} ({}%)",
            progress.done_steps, progress.total_steps, progress.percent
        ),
    ];
    if let Some(step_id) = progress.current_step_id.as_deref() {
        lines.push(format!("- current_step_id: {step_id}"));
    }
    if let Some(title) = progress.current_step_title.as_deref() {
        lines.push(format!("- current_step_title: {title}"));
    }
    lines.push(String::new());
    lines.push("### Checklist".to_string());
    for step in &run.steps {
        lines.push(format!(
            "- [{}] {} ({})",
            step.status.checkbox(),
            step.title,
            step.status.as_str()
        ));
    }
    if !run.control_commands.is_empty() {
        lines.push(String::new());
        lines.push("### Recent Control Commands".to_string());
        for command in run.control_commands.iter().rev().take(5) {
            let step = command
                .step_id
                .as_deref()
                .map(|id| format!(" step={id}"))
                .unwrap_or_default();
            lines.push(format!(
                "- {}{} by {}: {}",
                command.command, step, command.requested_by, command.status
            ));
        }
    }
    Some(lines.join("\n"))
}

fn orchestration_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("orchestrations")
}

fn require_target_step_status(
    run: &mut OrchestrationRun,
    action: RunControlAction,
    step_id: Option<&str>,
    preferred_current_statuses: &[OrchestrationStepStatus],
    status: OrchestrationStepStatus,
) -> Result<(), OrchestrationError> {
    if let Some(id) = step_id {
        let Some(step) = run.steps.iter_mut().find(|step| step.step_id == id) else {
            return Err(OrchestrationError::InvalidControl(format!(
                "unknown orchestration step for {}: {id}",
                action.as_str()
            )));
        };
        if !preferred_current_statuses.contains(&step.status) {
            return Err(OrchestrationError::InvalidControl(format!(
                "cannot {} orchestration step {id} while status is {}",
                action.as_str(),
                step.status.as_str()
            )));
        }
        step.status = status;
        return Ok(());
    }
    if let Some(step) = run
        .steps
        .iter_mut()
        .find(|step| preferred_current_statuses.contains(&step.status))
    {
        step.status = status;
        return Ok(());
    }
    Err(OrchestrationError::InvalidControl(format!(
        "no orchestration step matching {} control is available",
        action.as_str()
    )))
}

fn set_step_status(run: &mut OrchestrationRun, step_id: &str, status: OrchestrationStepStatus) {
    if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
        step.status = status;
    }
}

fn snapshot_summary_for_run(run: &OrchestrationRun) -> Vec<String> {
    let progress = run.progress_projection();
    vec![
        format!("run_id: {}", run.run_id),
        format!("status: {}", run.status),
        format!("objective: {}", run.objective),
        format!(
            "progress: {}/{} {}%",
            progress.done_steps, progress.total_steps, progress.percent
        ),
        format!(
            "current_step: {}",
            progress.current_step_title.as_deref().unwrap_or("none")
        ),
    ]
}

fn read_event_summary(data_dir: &Path) -> Vec<String> {
    let path = data_dir.join("events").join("events.jsonl");
    let Ok(content) = fs::read_to_string(path) else {
        return vec!["event log unavailable".to_string()];
    };
    let lines = content
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(8)
        .map(|line| line.chars().take(240).collect::<String>())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec!["event log empty".to_string()]
    } else {
        lines
    }
}

fn read_permission_summary(data_dir: &Path) -> Vec<String> {
    let permissions_dir = data_dir.join("permissions");
    if !permissions_dir.exists() {
        return vec!["no pending permission snapshot".to_string()];
    }
    let pending_count = fs::read_dir(&permissions_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains("pending"))
                .unwrap_or(false)
        })
        .count();
    vec![format!("pending_permission_files: {pending_count}")]
}

fn repo_summary(workspace_root: &Path) -> Vec<String> {
    let mut lines = vec![format!("Workspace root: {}", workspace_root.display())];
    if let Some(branch) = git_output(workspace_root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        lines.push(format!("Git branch: {}", branch.trim()));
    }
    if let Some(status) = git_output(workspace_root, &["status", "--short", "--branch"]) {
        lines.extend(status.lines().take(12).map(|line| format!("git: {line}")));
    }
    lines
}

fn recovery_session_summary(
    data_dir: &Path,
    session_id: &str,
) -> Result<RecoverySessionSummary, OrchestrationError> {
    let path = data_dir
        .join("sessions")
        .join(session_id)
        .join("transcript.jsonl");
    let content = fs::read_to_string(&path).map_err(|err| OrchestrationError::Io {
        path: path.clone(),
        message: err.to_string(),
    })?;
    let lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let recent_lines = lines
        .iter()
        .rev()
        .take(6)
        .map(|line| line.chars().take(240).collect::<String>())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Ok(RecoverySessionSummary {
        session_id: session_id.to_string(),
        transcript_line_count: lines.len(),
        recent_lines,
    })
}

fn recommended_recovery_actions(
    active_run: Option<&OrchestrationRun>,
    progress: Option<&OrchestrationProgressProjection>,
    control_commands: &[RunControlCommand],
    permission_summary: &[String],
) -> Vec<String> {
    let Some(run) = active_run else {
        return vec!["ask_user".to_string()];
    };
    if permission_summary.iter().any(|line| {
        line.strip_prefix("pending_permission_files: ")
            .and_then(|count| count.parse::<usize>().ok())
            .map(|count| count > 0)
            .unwrap_or(false)
    }) {
        return vec!["ask_user".to_string(), "continue".to_string()];
    }
    match run.status.as_str() {
        "paused" => vec!["continue".to_string(), "replan".to_string()],
        "blocked" | "replanning" => vec!["replan".to_string(), "ask_user".to_string()],
        "accepted" | "aborted" => vec!["continue".to_string()],
        _ => {
            if progress.map(|p| p.blocked_steps).unwrap_or_default() > 0 {
                vec![
                    "repair".to_string(),
                    "retry".to_string(),
                    "skip".to_string(),
                ]
            } else if control_commands
                .first()
                .map(|command| command.command.as_str())
                == Some("replan")
            {
                vec!["replan".to_string(), "continue".to_string()]
            } else {
                vec![
                    "continue".to_string(),
                    "retry".to_string(),
                    "replan".to_string(),
                ]
            }
        }
    }
}

fn git_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn parse_step_line(
    line: &str,
) -> Result<Option<(String, OrchestrationStepStatus, String)>, OrchestrationError> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("- [") {
        return Ok(None);
    }
    let checkbox_done = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
    let Some(marker_start) = trimmed.find("<!--") else {
        return Ok(None);
    };
    let Some(marker_end) = trimmed.find("-->") else {
        return Err(OrchestrationError::InvalidMarkdown(
            "step marker is missing closing `-->`".to_string(),
        ));
    };
    let marker = trimmed[marker_start + 4..marker_end].trim();
    let mut step_id = None;
    let mut status = None;
    for token in marker.split_whitespace() {
        if let Some(value) = token.strip_prefix("step:") {
            step_id = Some(value.to_string());
        } else if let Some(value) = token.strip_prefix("status:") {
            status = OrchestrationStepStatus::from_str(value);
        }
    }
    let step_id = step_id.ok_or_else(|| {
        OrchestrationError::InvalidMarkdown("step marker is missing `step:<id>`".to_string())
    })?;
    let status = status.unwrap_or(if checkbox_done {
        OrchestrationStepStatus::Done
    } else {
        OrchestrationStepStatus::Pending
    });
    let title = trimmed[marker_end + 3..].trim().to_string();
    Ok(Some((step_id, status, title)))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
