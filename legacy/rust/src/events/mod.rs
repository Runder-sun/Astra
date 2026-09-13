pub mod writer;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelEventEnvelope {
    pub event_id: String,
    pub seq: u64,
    pub event_name: String,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    pub object_kind: String,
    pub object_id: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub timestamp: String,
    pub payload: Value,
}

impl KernelEventEnvelope {
    pub fn validate(&self) -> Result<(), EventValidationError> {
        if self.event_id.trim().is_empty() {
            return Err(EventValidationError::MissingField("event_id"));
        }

        if self.event_name.trim().is_empty() {
            return Err(EventValidationError::MissingField("event_name"));
        }

        if self.phase.trim().is_empty() {
            return Err(EventValidationError::MissingField("phase"));
        }

        if self.object_kind.trim().is_empty() {
            return Err(EventValidationError::MissingField("object_kind"));
        }

        if self.object_id.trim().is_empty() {
            return Err(EventValidationError::MissingField("object_id"));
        }

        if self.timestamp.trim().is_empty() {
            return Err(EventValidationError::MissingField("timestamp"));
        }

        if !matches!(
            self.phase.as_str(),
            "start" | "update" | "terminal" | "control"
        ) {
            return Err(EventValidationError::InvalidPhase(self.phase.clone()));
        }

        if self.phase == "terminal" {
            let outcome = self
                .terminal_outcome
                .as_deref()
                .ok_or(EventValidationError::MissingField("terminal_outcome"))?;
            if !matches!(
                outcome,
                "succeeded" | "failed" | "cancelled" | "blocked" | "stale_conflict"
            ) {
                return Err(EventValidationError::InvalidTerminalOutcome(
                    outcome.to_string(),
                ));
            }
        } else if self.terminal_outcome.is_some() {
            return Err(EventValidationError::UnexpectedTerminalOutcome);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum EventValidationError {
    MissingField(&'static str),
    InvalidPhase(String),
    InvalidTerminalOutcome(String),
    UnexpectedTerminalOutcome,
}

impl fmt::Display for EventValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing required event field: {field}"),
            Self::InvalidPhase(phase) => write!(f, "invalid event phase: {phase}"),
            Self::InvalidTerminalOutcome(outcome) => {
                write!(f, "invalid terminal outcome: {outcome}")
            }
            Self::UnexpectedTerminalOutcome => {
                write!(
                    f,
                    "terminal outcome is only allowed on terminal-phase events"
                )
            }
        }
    }
}

impl std::error::Error for EventValidationError {}

#[derive(Debug)]
pub enum EventReadError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    Validation(EventValidationError),
}

impl fmt::Display for EventReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "event log io failed: {err}"),
            Self::Serde(err) => write!(f, "event log parse failed: {err}"),
            Self::Validation(err) => write!(f, "event validation failed: {err}"),
        }
    }
}

impl std::error::Error for EventReadError {}

impl From<std::io::Error> for EventReadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for EventReadError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

impl From<EventValidationError> for EventReadError {
    fn from(value: EventValidationError) -> Self {
        Self::Validation(value)
    }
}

pub fn read_events_from(path: &Path) -> Result<Vec<KernelEventEnvelope>, EventReadError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let event: KernelEventEnvelope = serde_json::from_str(line)?;
        event.validate()?;
        events.push(event);
    }
    Ok(events)
}
