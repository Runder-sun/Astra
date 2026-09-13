use crate::projects::current::ResolvedProject;
use crate::projects::registry::{ProjectRegistry, RegistryError};
use crate::session::store::{SessionStore, SessionStoreError};
use crate::session::transcript::TranscriptLine;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub scope: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
    pub estimated_cost: String,
    pub accounting_quality: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSummary {
    pub scope: String,
    pub session_count: i64,
    pub active_session_count: i64,
    pub project_count: i64,
}

#[derive(Debug)]
pub enum TelemetryError {
    SessionStore(SessionStoreError),
    Registry(RegistryError),
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionStore(err) => write!(f, "{err}"),
            Self::Registry(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TelemetryError {}

impl From<SessionStoreError> for TelemetryError {
    fn from(value: SessionStoreError) -> Self {
        Self::SessionStore(value)
    }
}

impl From<RegistryError> for TelemetryError {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

pub fn usage_summary(
    resolved: &ResolvedProject,
    scope: &str,
) -> Result<UsageSummary, TelemetryError> {
    let store = SessionStore::new(resolved.data_dir.clone());
    let counters = match scope {
        "project" => summarize_project(&store)?,
        "session" => summarize_latest_session(&store)?,
        "turn" => summarize_latest_turn(&store)?,
        _ => UsageCounters::default(),
    };
    Ok(build_usage_summary(scope, counters))
}

pub fn cost_summary(
    resolved: &ResolvedProject,
    scope: &str,
) -> Result<UsageSummary, TelemetryError> {
    usage_summary(resolved, scope)
}

pub fn stats_summary(
    registry: &ProjectRegistry,
    resolved: Option<&ResolvedProject>,
    scope: &str,
) -> Result<StatsSummary, TelemetryError> {
    match scope {
        "project" => {
            let resolved = resolved.expect("project scope requires resolved project");
            let stats = SessionStore::new(resolved.data_dir.clone()).stats()?;
            Ok(StatsSummary {
                scope: "project".to_string(),
                session_count: stats.session_count as i64,
                active_session_count: stats.active_count as i64,
                project_count: 1,
            })
        }
        _ => {
            let projects = registry.list()?;
            let mut session_count = 0i64;
            let mut active_session_count = 0i64;
            for project in &projects {
                let stats = SessionStore::new(project.data_dir.clone()).stats()?;
                session_count += stats.session_count as i64;
                active_session_count += stats.active_count as i64;
            }
            Ok(StatsSummary {
                scope: "runtime".to_string(),
                session_count,
                active_session_count,
                project_count: projects.len() as i64,
            })
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct UsageCounters {
    input_tokens: i64,
    output_tokens: i64,
    tool_calls: i64,
}

fn summarize_project(store: &SessionStore) -> Result<UsageCounters, SessionStoreError> {
    let mut counters = UsageCounters::default();
    for session in store.list_sessions()? {
        let transcript = store.read_transcript(&session.session_id)?;
        counters = counters + summarize_lines(&transcript);
    }
    Ok(counters)
}

fn summarize_latest_session(store: &SessionStore) -> Result<UsageCounters, SessionStoreError> {
    let Some(session) = store.list_sessions()?.into_iter().next() else {
        return Ok(UsageCounters::default());
    };
    let transcript = store.read_transcript(&session.session_id)?;
    Ok(summarize_lines(&transcript))
}

fn summarize_latest_turn(store: &SessionStore) -> Result<UsageCounters, SessionStoreError> {
    let Some(session) = store.list_sessions()?.into_iter().next() else {
        return Ok(UsageCounters::default());
    };
    let transcript = store.read_transcript(&session.session_id)?;
    let Some(start_index) = transcript
        .iter()
        .rposition(|line| matches!(line, TranscriptLine::Message { role, .. } if role == "user"))
    else {
        return Ok(UsageCounters::default());
    };
    Ok(summarize_lines(&transcript[start_index..]))
}

fn summarize_lines(lines: &[TranscriptLine]) -> UsageCounters {
    let mut counters = UsageCounters::default();
    for line in lines {
        match line {
            TranscriptLine::Message { role, content } if role == "user" => {
                counters.input_tokens += estimate_tokens(content);
            }
            TranscriptLine::Message { role, content } if role == "assistant" => {
                counters.output_tokens += estimate_tokens(content);
            }
            TranscriptLine::ToolCall { .. } => {
                counters.tool_calls += 1;
            }
            _ => {}
        }
    }
    counters
}

fn build_usage_summary(scope: &str, counters: UsageCounters) -> UsageSummary {
    let total_tokens = counters.input_tokens + counters.output_tokens;
    let estimated_cost = format!("{:.6}", (total_tokens as f64) * 0.000_002);
    UsageSummary {
        scope: scope.to_string(),
        input_tokens: counters.input_tokens,
        output_tokens: counters.output_tokens,
        tool_calls: counters.tool_calls,
        estimated_cost,
        accounting_quality: "estimated".to_string(),
    }
}

fn estimate_tokens(text: &str) -> i64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.chars().count().div_ceil(4) as i64
}

impl std::ops::Add for UsageCounters {
    type Output = UsageCounters;

    fn add(self, rhs: Self) -> Self::Output {
        UsageCounters {
            input_tokens: self.input_tokens + rhs.input_tokens,
            output_tokens: self.output_tokens + rhs.output_tokens,
            tool_calls: self.tool_calls + rhs.tool_calls,
        }
    }
}
