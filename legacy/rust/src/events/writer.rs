use super::{EventValidationError, KernelEventEnvelope};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;

#[derive(Debug)]
pub enum EventWriteError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    Validation(EventValidationError),
}

impl fmt::Display for EventWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "event log io failed: {err}"),
            Self::Serde(err) => write!(f, "event log serialization failed: {err}"),
            Self::Validation(err) => write!(f, "event validation failed: {err}"),
        }
    }
}

impl std::error::Error for EventWriteError {}

impl From<std::io::Error> for EventWriteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for EventWriteError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

impl From<EventValidationError> for EventWriteError {
    fn from(value: EventValidationError) -> Self {
        Self::Validation(value)
    }
}

pub fn append_event<P>(log_path: P, event: &KernelEventEnvelope) -> Result<(), EventWriteError>
where
    P: AsRef<Path>,
{
    let log_path = log_path.as_ref();
    let _guard = lock_event_log(log_path)?;
    append_event_unlocked(log_path, event)
}

pub fn append_event_unlocked<P>(
    log_path: P,
    event: &KernelEventEnvelope,
) -> Result<(), EventWriteError>
where
    P: AsRef<Path>,
{
    event.validate()?;

    let log_path = log_path.as_ref();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    file.write_all(&line)?;

    Ok(())
}

#[derive(Debug)]
pub struct EventLogLockGuard {
    file: File,
}

pub fn lock_event_log<P>(log_path: P) -> Result<EventLogLockGuard, EventWriteError>
where
    P: AsRef<Path>,
{
    let log_path = log_path.as_ref();
    let lock_path = event_lock_path(log_path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(EventWriteError::Io(std::io::Error::last_os_error()));
    }
    Ok(EventLogLockGuard { file })
}

fn event_lock_path(log_path: &Path) -> std::path::PathBuf {
    let file_name = log_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("events.jsonl");
    log_path.with_file_name(format!("{file_name}.lock"))
}

impl Drop for EventLogLockGuard {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}
