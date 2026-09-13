use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Global flag set by SIGINT handler for the agent loop.
static AGENT_SIGINT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Check if SIGINT was requested (for agent loop cancellation).
pub fn sigint_requested() -> bool {
    AGENT_SIGINT_REQUESTED.load(Ordering::SeqCst)
}

/// Clear the SIGINT flag (call before starting agent loop).
pub fn clear_sigint() {
    AGENT_SIGINT_REQUESTED.store(false, Ordering::SeqCst);
}

/// Install a SIGINT handler that sets the global flag.
/// Call before starting an agent loop in non-TUI contexts.
pub fn install_agent_sigint_handler() {
    AGENT_SIGINT_REQUESTED.store(false, Ordering::SeqCst);
    unsafe {
        libc::signal(
            libc::SIGINT,
            agent_sigint_handler as *const () as libc::sighandler_t,
        );
    }
}

extern "C" fn agent_sigint_handler(_signal: libc::c_int) {
    AGENT_SIGINT_REQUESTED.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeCancelToken {
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct RuntimeInterruptHandle {
    token: RuntimeCancelToken,
}

pub fn runtime_interrupt_pair() -> (RuntimeCancelToken, RuntimeInterruptHandle) {
    let token = RuntimeCancelToken::default();
    let handle = RuntimeInterruptHandle {
        token: token.clone(),
    };
    (token, handle)
}

impl RuntimeCancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl RuntimeInterruptHandle {
    pub fn interrupt(&self) {
        self.token.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActiveRuntimeTurnKey {
    pub project_id: String,
    pub session_id: String,
    pub turn_id: String,
}

impl ActiveRuntimeTurnKey {
    pub fn new(
        project_id: impl Into<String>,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            session_id: session_id.into(),
            turn_id: turn_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRuntimeTurnCancellation {
    pub project_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ActiveRuntimeTurnRegistry {
    entries: Arc<Mutex<HashMap<ActiveRuntimeTurnKey, RuntimeInterruptHandle>>>,
}

impl ActiveRuntimeTurnRegistry {
    pub fn register(
        &self,
        key: ActiveRuntimeTurnKey,
        handle: RuntimeInterruptHandle,
    ) -> ActiveRuntimeTurnGuard {
        self.entries
            .lock()
            .expect("active runtime turn registry lock should not be poisoned")
            .insert(key.clone(), handle);
        ActiveRuntimeTurnGuard {
            registry: self.clone(),
            key,
        }
    }

    pub fn interrupt(&self, key: &ActiveRuntimeTurnKey) -> Option<ActiveRuntimeTurnCancellation> {
        let handle = self
            .entries
            .lock()
            .expect("active runtime turn registry lock should not be poisoned")
            .get(key)
            .cloned()?;
        handle.interrupt();
        Some(ActiveRuntimeTurnCancellation {
            project_id: key.project_id.clone(),
            session_id: key.session_id.clone(),
            turn_id: key.turn_id.clone(),
            cancelled: handle.is_cancelled(),
        })
    }

    pub fn interrupt_session_turn(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> Option<ActiveRuntimeTurnCancellation> {
        let key = self
            .entries
            .lock()
            .expect("active runtime turn registry lock should not be poisoned")
            .keys()
            .find(|key| key.project_id == project_id && key.session_id == session_id)
            .cloned()?;
        self.interrupt(&key)
    }

    fn unregister(&self, key: &ActiveRuntimeTurnKey) {
        self.entries
            .lock()
            .expect("active runtime turn registry lock should not be poisoned")
            .remove(key);
    }
}

#[derive(Debug)]
pub struct ActiveRuntimeTurnGuard {
    registry: ActiveRuntimeTurnRegistry,
    key: ActiveRuntimeTurnKey,
}

impl Drop for ActiveRuntimeTurnGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.key);
    }
}

pub fn active_runtime_turn_registry() -> ActiveRuntimeTurnRegistry {
    static REGISTRY: OnceLock<ActiveRuntimeTurnRegistry> = OnceLock::new();
    REGISTRY
        .get_or_init(ActiveRuntimeTurnRegistry::default)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_interrupt_handle_marks_shared_token_cancelled() {
        let (token, handle) = runtime_interrupt_pair();

        assert!(!token.is_cancelled());
        assert!(!handle.is_cancelled());

        handle.interrupt();

        assert!(token.is_cancelled());
        assert!(handle.is_cancelled());
    }

    #[test]
    fn active_runtime_turn_registry_cancels_and_unregisters_scoped_turns() {
        let registry = ActiveRuntimeTurnRegistry::default();
        let (token, handle) = runtime_interrupt_pair();
        let key = ActiveRuntimeTurnKey::new("project-a", "session-a", "turn-a");

        let guard = registry.register(key.clone(), handle);

        assert!(!token.is_cancelled());
        let cancelled = registry
            .interrupt(&key)
            .expect("active turn should be present");
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.project_id, "project-a");
        assert_eq!(cancelled.session_id, "session-a");
        assert_eq!(cancelled.turn_id, "turn-a");
        assert!(token.is_cancelled());

        drop(guard);

        assert!(
            registry.interrupt(&key).is_none(),
            "turn handle should be removed when the guard drops"
        );
    }

    #[test]
    fn active_runtime_turn_registry_cancels_by_project_session() {
        let registry = ActiveRuntimeTurnRegistry::default();
        let (token, handle) = runtime_interrupt_pair();
        let key = ActiveRuntimeTurnKey::new("project-a", "session-a", "turn-a");
        let _guard = registry.register(key, handle);

        let cancelled = registry
            .interrupt_session_turn("project-a", "session-a")
            .expect("active session turn should be present");

        assert_eq!(cancelled.turn_id, "turn-a");
        assert!(cancelled.cancelled);
        assert!(token.is_cancelled());
        assert!(registry
            .interrupt_session_turn("project-a", "session-missing")
            .is_none());
    }
}
