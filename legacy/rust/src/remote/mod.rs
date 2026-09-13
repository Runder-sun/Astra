use crate::branches;
use crate::memory;
use crate::projects::current::ResolvedProject;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod daemon;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineIdentity {
    pub schema_version: String,
    pub machine_id: String,
    pub status: String,
    pub key_algorithm: String,
    pub public_key_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteClientIdentity {
    pub schema_version: String,
    pub client_id: String,
    pub status: String,
    pub client_type: String,
    pub paired_at: String,
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairTicket {
    pub schema_version: String,
    pub ticket_id: String,
    pub status: String,
    pub client_id: String,
    pub machine_id: String,
    pub issued_at: String,
    pub consumed_at: String,
    #[serde(default)]
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteBinding {
    pub schema_version: String,
    pub project_id: String,
    pub workspace_root: String,
    pub status: String,
    pub binding_state: String,
    pub client_id: String,
    pub machine_id: String,
    pub scope: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCursor {
    pub schema_version: String,
    pub cursor_id: String,
    pub project_id: String,
    pub cursor_state: String,
    pub last_event_seq: u64,
    #[serde(default)]
    pub ownership_epoch: u64,
    #[serde(default)]
    pub replay_token: String,
    pub replay_policy: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlLease {
    pub schema_version: String,
    pub lease_id: String,
    pub project_id: String,
    pub owner_id: String,
    pub lease_state: String,
    pub terminal_scope: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteControlOwner {
    pub schema_version: String,
    pub project_id: String,
    pub owner_state: String,
    pub owner_id: String,
    pub control_lease: ControlLease,
    pub ownership_epoch: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteMachineMetadata {
    pub schema_version: String,
    pub machine_id: String,
    pub display_name: String,
    pub last_known_state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDaemonState {
    pub schema_version: String,
    pub machine_id: String,
    pub daemon_state: String,
    pub last_shutdown_state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCapabilitySet {
    pub schema_version: String,
    pub scope: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCapabilityMatrix {
    pub schema_version: String,
    pub machine: RemoteCapabilitySet,
    pub project: RemoteCapabilitySet,
    pub session: RemoteCapabilitySet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineActionPolicy {
    pub schema_version: String,
    pub default_policy: String,
    pub allowed_actions: Vec<String>,
    pub rejected_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRemoteSwitchPolicy {
    pub schema_version: String,
    pub active_plane: String,
    pub switch_policy: String,
    pub local_authority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFeatureAdvertisement {
    pub schema_version: String,
    pub substrate: String,
    pub relay_transport: String,
    pub attach: String,
    pub handoff: String,
    pub takeover: String,
    pub notify: String,
    pub permission_response: String,
    pub message_steering: String,
    pub workbench: String,
    pub revocation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkbenchProjection {
    pub schema_version: String,
    pub project_id: String,
    pub source: String,
    pub authoritative: bool,
    pub updated_at: String,
    pub transport: RemoteWorkbenchTransport,
    pub sections: RemoteWorkbenchSections,
    pub permissions: RemoteWorkbenchPermissionSummary,
    pub reviews: RemoteWorkbenchCountSummary,
    pub artifacts: RemoteWorkbenchCountSummary,
    pub memory: RemoteWorkbenchCountSummary,
    pub branches: RemoteWorkbenchCountSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkbenchTransport {
    pub mode: String,
    pub authority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkbenchSections {
    pub project_status: String,
    pub sessions: String,
    pub permissions: String,
    pub reviews: String,
    pub artifacts: String,
    pub memory: String,
    pub branches: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkbenchPermissionSummary {
    pub pending_count: usize,
    pub pending: Vec<RemoteWorkbenchPermissionRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkbenchPermissionRequest {
    pub request_id: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub target_path: Option<String>,
    pub permission_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkbenchCountSummary {
    pub status: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntimeDescriptor {
    pub schema_version: String,
    pub project_id: String,
    pub active_session_id: Option<String>,
    pub runtime_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default)]
    pub workspace_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnvelopeProjection {
    pub schema_version: String,
    pub projection_id: String,
    pub project_id: String,
    pub source: String,
    pub active_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProjectionSnapshot {
    pub schema_version: String,
    pub project_id: String,
    pub projection_state: String,
    pub regeneration_policy: String,
    pub runtime_descriptor: SessionRuntimeDescriptor,
    pub envelope_projection: SessionEnvelopeProjection,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTerminalLease {
    pub schema_version: String,
    pub terminal_lease_id: String,
    pub lease_state: String,
    pub policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteStatusReport {
    pub schema_version: String,
    pub project_id: String,
    pub workspace_root: String,
    pub remote_ready: bool,
    pub machine_identity: MachineIdentity,
    pub client_identity: Option<RemoteClientIdentity>,
    pub binding: RemoteBinding,
    pub cursor: RemoteCursor,
    pub control_owner: RemoteControlOwner,
    pub machine_metadata: RemoteMachineMetadata,
    pub daemon_state: RemoteDaemonState,
    pub capability_matrix: RemoteCapabilityMatrix,
    pub offline_action_policy: OfflineActionPolicy,
    pub switch_policy: LocalRemoteSwitchPolicy,
    pub feature_advertisement: RemoteFeatureAdvertisement,
    pub projection: RemoteProjectionSnapshot,
    pub terminal_lease: RemoteTerminalLease,
    pub lease: ControlLease,
    pub projection_regenerated: bool,
    pub revocation_reason: Option<String>,
    pub revoked_at: Option<String>,
    pub workbench: RemoteWorkbenchProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteActionRejection {
    pub schema_version: String,
    pub action: String,
    pub rejection_code: String,
    pub reason: String,
    pub retryable: bool,
    pub required_state: String,
    pub current_state: String,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAttachEligibility {
    pub schema_version: String,
    pub eligible: bool,
    pub strategy: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAttachResult {
    pub schema_version: String,
    pub action: String,
    pub execution_mode: String,
    pub session_id: String,
    pub strategy: String,
    pub eligibility: RemoteAttachEligibility,
    pub control_owner: RemoteControlOwner,
    pub projection: RemoteProjectionSnapshot,
    pub ownership_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHandoffResult {
    pub schema_version: String,
    pub action: String,
    pub execution_mode: String,
    pub session_id: String,
    pub target_client: String,
    pub previous_owner: RemoteControlOwner,
    pub control_owner: RemoteControlOwner,
    pub ownership_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTakeoverResult {
    pub schema_version: String,
    pub action: String,
    pub session_id: String,
    pub previous_owner: RemoteControlOwner,
    pub new_owner: RemoteControlOwner,
    pub ownership_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteNotification {
    pub schema_version: String,
    pub notification_id: String,
    pub kind: String,
    pub message: String,
    pub project_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteNotificationReceipt {
    pub schema_version: String,
    pub client_id: String,
    pub delivery_state: String,
    pub queued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteNotifyResult {
    pub schema_version: String,
    pub action: String,
    pub notification: RemoteNotification,
    pub receipts: Vec<RemoteNotificationReceipt>,
}

#[derive(Debug)]
pub enum RemoteError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    InvalidInput(String),
    ActionRejected(RemoteActionRejection),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "remote IO failed: {err}"),
            Self::Serde(err) => write!(f, "remote serialization failed: {err}"),
            Self::InvalidInput(message) => write!(f, "invalid remote input: {message}"),
            Self::ActionRejected(rejection) => {
                write!(f, "remote action rejected: {}", rejection.reason)
            }
        }
    }
}

impl std::error::Error for RemoteError {}

impl From<std::io::Error> for RemoteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RemoteError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn status(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<RemoteStatusReport, RemoteError> {
    let machine = load_machine_identity(state_home)?.unwrap_or_else(missing_machine_identity);
    let binding = load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    let client_identity = if binding.client_id.is_empty() {
        None
    } else {
        load_client_identity(state_home, &binding.client_id)?
    };
    status_from_parts(
        state_home,
        resolved,
        active_session_id,
        machine,
        client_identity,
        binding,
    )
}

pub fn issue_pair_ticket(
    state_home: &Path,
    resolved: &ResolvedProject,
    client_id: &str,
    ticket_id: &str,
    ticket_expires_at: Option<&str>,
) -> Result<PairTicket, RemoteError> {
    let client_id = sanitize_identifier(client_id)?;
    let ticket_id = sanitize_identifier(ticket_id)?;
    if let Some(existing) = load_pair_ticket(state_home, &ticket_id)? {
        if existing.status == "consumed" {
            return Err(RemoteError::ActionRejected(action_rejection(
                "pair",
                "remote_pair_ticket_replayed",
                "remote pair ticket was already consumed",
                "fresh_pair_ticket",
                "consumed",
                "Create a new pair ticket and retry pairing.",
            )));
        }
    }
    let now = timestamp_string();
    let machine = load_machine_identity(state_home)?.unwrap_or_else(|| MachineIdentity {
        schema_version: "1".to_string(),
        machine_id: format!("machine_{}", resolved.workspace_hash),
        status: "ready".to_string(),
        key_algorithm: "ed25519-placeholder".to_string(),
        public_key_ref: "machine_public_key_placeholder".to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
    });
    write_json(&machine_identity_path(state_home), &machine)?;
    let ticket = PairTicket {
        schema_version: "1".to_string(),
        ticket_id,
        status: "issued".to_string(),
        client_id,
        machine_id: machine.machine_id,
        issued_at: now,
        consumed_at: String::new(),
        expires_at: ticket_expires_at.unwrap_or("").to_string(),
    };
    write_json(&pair_ticket_path(state_home, &ticket.ticket_id), &ticket)?;
    Ok(ticket)
}

pub fn pair(
    state_home: &Path,
    resolved: &ResolvedProject,
    client_id: &str,
    ticket_id: &str,
    lease_expires_at: Option<&str>,
    ticket_expires_at: Option<&str>,
    active_session_id: Option<String>,
) -> Result<RemoteStatusReport, RemoteError> {
    pair_with_policy(
        state_home,
        resolved,
        client_id,
        ticket_id,
        lease_expires_at,
        ticket_expires_at,
        active_session_id,
        false,
    )
}

pub fn pair_daemon(
    state_home: &Path,
    resolved: &ResolvedProject,
    client_id: &str,
    ticket_id: &str,
    lease_expires_at: Option<&str>,
    ticket_expires_at: Option<&str>,
    active_session_id: Option<String>,
) -> Result<RemoteStatusReport, RemoteError> {
    pair_with_policy(
        state_home,
        resolved,
        client_id,
        ticket_id,
        lease_expires_at,
        ticket_expires_at,
        active_session_id,
        true,
    )
}

pub fn pair_daemon_automatic(
    state_home: &Path,
    resolved: &ResolvedProject,
    client_id: &str,
    ticket_id: &str,
    lease_expires_at: Option<&str>,
    ticket_expires_at: Option<&str>,
    active_session_id: Option<String>,
) -> Result<RemoteStatusReport, RemoteError> {
    pair_with_policy(
        state_home,
        resolved,
        client_id,
        ticket_id,
        lease_expires_at,
        ticket_expires_at,
        active_session_id,
        false,
    )
}

fn pair_with_policy(
    state_home: &Path,
    resolved: &ResolvedProject,
    client_id: &str,
    ticket_id: &str,
    lease_expires_at: Option<&str>,
    ticket_expires_at: Option<&str>,
    active_session_id: Option<String>,
    require_preissued_ticket: bool,
) -> Result<RemoteStatusReport, RemoteError> {
    let client_id = sanitize_identifier(client_id)?;
    let ticket_id = sanitize_identifier(ticket_id)?;
    let now = timestamp_string();
    if let Some(expires_at) = ticket_expires_at {
        if timestamp_is_expired(expires_at) {
            return Err(RemoteError::ActionRejected(action_rejection(
                "pair",
                "remote_pair_ticket_expired",
                "remote pair ticket expired",
                "fresh_pair_ticket",
                "expired",
                "Create a new pair ticket and retry pairing.",
            )));
        }
    }
    if let Some(existing_ticket) = load_pair_ticket(state_home, &ticket_id)? {
        if timestamp_is_expired(&existing_ticket.expires_at) {
            return Err(RemoteError::ActionRejected(action_rejection(
                "pair",
                "remote_pair_ticket_expired",
                "remote pair ticket expired",
                "fresh_pair_ticket",
                "expired",
                "Create a new pair ticket and retry pairing.",
            )));
        }
        if existing_ticket.status == "consumed" {
            return Err(RemoteError::ActionRejected(action_rejection(
                "pair",
                "remote_pair_ticket_replayed",
                "remote pair ticket was already consumed",
                "fresh_pair_ticket",
                "consumed",
                "Create a new pair ticket and retry pairing.",
            )));
        }
        if !existing_ticket.client_id.is_empty() && existing_ticket.client_id != client_id {
            return Err(RemoteError::ActionRejected(action_rejection(
                "pair",
                "remote_pair_ticket_client_mismatch",
                "remote pair ticket was issued for a different client",
                "matching_client",
                "client_mismatch",
                "Pair with the client id recorded on the ticket.",
            )));
        }
    } else if require_preissued_ticket {
        return Err(RemoteError::ActionRejected(action_rejection(
            "pair",
            "remote_pair_ticket_not_found",
            "remote pair ticket was not pre-issued",
            "fresh_pair_ticket",
            "missing",
            "Create a pair ticket from the local CLI before daemon pairing.",
        )));
    }
    let machine = load_machine_identity(state_home)?.unwrap_or_else(|| MachineIdentity {
        schema_version: "1".to_string(),
        machine_id: format!("machine_{}", resolved.workspace_hash),
        status: "ready".to_string(),
        key_algorithm: "ed25519-placeholder".to_string(),
        public_key_ref: "machine_public_key_placeholder".to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
    });
    write_json(&machine_identity_path(state_home), &machine)?;

    let client = RemoteClientIdentity {
        schema_version: "1".to_string(),
        client_id: client_id.clone(),
        status: "paired".to_string(),
        client_type: "operator".to_string(),
        paired_at: now.clone(),
        machine_id: machine.machine_id.clone(),
        revoked_at: None,
    };
    write_json(&client_identity_path(state_home, &client_id), &client)?;

    let ticket = PairTicket {
        schema_version: "1".to_string(),
        ticket_id,
        status: "consumed".to_string(),
        client_id: client_id.clone(),
        machine_id: machine.machine_id.clone(),
        issued_at: now.clone(),
        consumed_at: now.clone(),
        expires_at: ticket_expires_at.unwrap_or("").to_string(),
    };
    write_json(&pair_ticket_path(state_home, &ticket.ticket_id), &ticket)?;

    let binding = RemoteBinding {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        workspace_root: resolved.workspace_root.display().to_string(),
        status: "ready".to_string(),
        binding_state: "paired".to_string(),
        client_id,
        machine_id: machine.machine_id.clone(),
        scope: "project".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    write_machine_remote_files(
        state_home,
        &machine,
        Some(&binding),
        lease_expires_at.unwrap_or(""),
    )?;
    write_project_remote_files(
        &resolved.data_dir,
        resolved,
        &binding,
        lease_expires_at.unwrap_or(""),
        active_session_id.clone(),
    )?;
    status_from_parts(
        state_home,
        resolved,
        active_session_id,
        machine,
        Some(client),
        binding,
    )
}

pub fn attach(
    state_home: &Path,
    resolved: &ResolvedProject,
    session_id: &str,
    strategy: Option<&str>,
    execute: bool,
) -> Result<RemoteAttachResult, RemoteError> {
    let session_id = sanitize_identifier(session_id)?;
    let strategy = strategy.unwrap_or("terminal_host");
    if strategy != "terminal_host" {
        return Err(RemoteError::InvalidInput(format!(
            "unsupported remote attach strategy: {strategy}"
        )));
    }
    ensure_pairing(state_home, resolved, "attach")?;
    let binding = load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    let eligibility = RemoteAttachEligibility {
        schema_version: "1".to_string(),
        eligible: true,
        strategy: strategy.to_string(),
        reason: "session is available through the local terminal host control substrate"
            .to_string(),
    };
    let current_owner = load_control_owner(&resolved.data_dir)?
        .unwrap_or_else(|| default_control_owner(resolved, &binding));
    let next_epoch = if execute {
        current_ownership_epoch(&current_owner) + 1
    } else {
        current_ownership_epoch(&current_owner)
    };
    let control_owner = if execute {
        let owner = control_owner_for(
            resolved,
            &binding.client_id,
            "attached",
            next_epoch,
            load_lease(state_home)?
                .map(|lease| lease.expires_at)
                .unwrap_or_default(),
        );
        write_json(&control_owner_path(&resolved.data_dir), &owner)?;
        write_json(&lease_path(state_home), &owner.control_lease)?;
        owner
    } else {
        current_owner
    };
    let projection = regenerate_projection(&resolved.data_dir, resolved, Some(session_id.clone()))?;
    Ok(RemoteAttachResult {
        schema_version: "1".to_string(),
        action: "attach".to_string(),
        execution_mode: if execute { "execute" } else { "inspect" }.to_string(),
        session_id,
        strategy: strategy.to_string(),
        eligibility,
        control_owner,
        projection,
        ownership_epoch: next_epoch,
    })
}

pub fn handoff(
    state_home: &Path,
    resolved: &ResolvedProject,
    session_id: &str,
    target_client: &str,
    execute: bool,
) -> Result<RemoteHandoffResult, RemoteError> {
    let session_id = sanitize_identifier(session_id)?;
    let target_client = sanitize_identifier(target_client)?;
    ensure_pairing(state_home, resolved, "handoff")?;
    let binding = load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    let previous_owner = load_control_owner(&resolved.data_dir)?
        .unwrap_or_else(|| default_control_owner(resolved, &binding));
    let next_epoch = if execute {
        current_ownership_epoch(&previous_owner) + 1
    } else {
        current_ownership_epoch(&previous_owner)
    };
    let control_owner = if execute {
        let owner = control_owner_for(
            resolved,
            &target_client,
            "handoff",
            next_epoch,
            load_lease(state_home)?
                .map(|lease| lease.expires_at)
                .unwrap_or_default(),
        );
        write_json(&control_owner_path(&resolved.data_dir), &owner)?;
        write_json(&lease_path(state_home), &owner.control_lease)?;
        owner
    } else {
        previous_owner.clone()
    };
    Ok(RemoteHandoffResult {
        schema_version: "1".to_string(),
        action: "handoff".to_string(),
        execution_mode: if execute { "execute" } else { "inspect" }.to_string(),
        session_id,
        target_client,
        previous_owner,
        control_owner,
        ownership_epoch: next_epoch,
    })
}

pub fn takeover(
    state_home: &Path,
    resolved: &ResolvedProject,
    session_id: &str,
    client_id: &str,
) -> Result<RemoteTakeoverResult, RemoteError> {
    let session_id = sanitize_identifier(session_id)?;
    let client_id = sanitize_identifier(client_id)?;
    ensure_pairing(state_home, resolved, "takeover")?;
    let binding = load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    let previous_owner = load_control_owner(&resolved.data_dir)?
        .unwrap_or_else(|| default_control_owner(resolved, &binding));
    let ownership_epoch = current_ownership_epoch(&previous_owner) + 1;
    let new_owner = control_owner_for(
        resolved,
        &client_id,
        "takeover",
        ownership_epoch,
        load_lease(state_home)?
            .map(|lease| lease.expires_at)
            .unwrap_or_default(),
    );
    write_json(&control_owner_path(&resolved.data_dir), &new_owner)?;
    write_json(&lease_path(state_home), &new_owner.control_lease)?;
    Ok(RemoteTakeoverResult {
        schema_version: "1".to_string(),
        action: "takeover".to_string(),
        session_id,
        previous_owner,
        new_owner,
        ownership_epoch,
    })
}

pub fn notify(
    state_home: &Path,
    resolved: &ResolvedProject,
    kind: &str,
    message: &str,
) -> Result<RemoteNotifyResult, RemoteError> {
    let kind = sanitize_identifier(kind)?;
    ensure_pairing(state_home, resolved, "notify")?;
    let binding = load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    let now = timestamp_string();
    let notification = RemoteNotification {
        schema_version: "1".to_string(),
        notification_id: format!("remote_note_{}_{}", resolved.project_id, now),
        kind,
        message: message.to_string(),
        project_id: resolved.project_id.clone(),
        created_at: now.clone(),
    };
    let receipt_client = load_control_owner(&resolved.data_dir)?
        .map(|owner| owner.owner_id)
        .filter(|owner_id| owner_id != "none")
        .unwrap_or(binding.client_id);
    let receipts = vec![RemoteNotificationReceipt {
        schema_version: "1".to_string(),
        client_id: receipt_client,
        delivery_state: "queued".to_string(),
        queued_at: now,
    }];
    Ok(RemoteNotifyResult {
        schema_version: "1".to_string(),
        action: "notify".to_string(),
        notification,
        receipts,
    })
}

pub fn revoke(
    state_home: &Path,
    resolved: &ResolvedProject,
    client_id: &str,
) -> Result<RemoteStatusReport, RemoteError> {
    let client_id = sanitize_identifier(client_id)?;
    let now = timestamp_string();
    let mut machine = load_machine_identity(state_home)?.unwrap_or_else(missing_machine_identity);
    if machine.status == "missing" {
        return Err(RemoteError::ActionRejected(attach_rejection("revoke")));
    }
    machine.updated_at = now.clone();
    write_json(&machine_identity_path(state_home), &machine)?;

    if let Some(mut client) = load_client_identity(state_home, &client_id)? {
        client.status = "revoked".to_string();
        client.revoked_at = Some(now.clone());
        write_json(&client_identity_path(state_home, &client_id), &client)?;
    }

    let mut binding =
        load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    binding.status = "revoked".to_string();
    binding.binding_state = "revoked".to_string();
    binding.updated_at = now.clone();
    write_json(
        &remote_dir(&resolved.data_dir).join("binding.json"),
        &binding,
    )?;

    let owner = control_owner_for(
        resolved,
        &client_id,
        "revoked",
        load_control_owner(&resolved.data_dir)?
            .map(|owner| owner.ownership_epoch + 1)
            .unwrap_or(1),
        now.clone(),
    );
    let mut revoked_owner = owner;
    revoked_owner.control_lease.lease_state = "revoked".to_string();
    write_json(&control_owner_path(&resolved.data_dir), &revoked_owner)?;
    write_json(&lease_path(state_home), &revoked_owner.control_lease)?;

    status_from_parts(
        state_home,
        resolved,
        None,
        machine,
        load_client_identity(state_home, &client_id)?,
        binding,
    )
}

pub fn attach_rejection(action: &str) -> RemoteActionRejection {
    action_rejection(
        action,
        "remote_not_paired",
        "remote control requires a paired client and project binding",
        "paired",
        "missing",
        "Run `research-cli remote pair --client <id> --ticket <ticket> --json`.",
    )
}

pub fn lease_expired_rejection(action: &str) -> RemoteActionRejection {
    action_rejection(
        action,
        "remote_lease_expired",
        "remote control lease expired",
        "active_lease",
        "expired",
        "Pair again or renew the lease before retrying remote control.",
    )
}

pub fn binding_revoked_rejection(action: &str) -> RemoteActionRejection {
    action_rejection(
        action,
        "remote_binding_revoked",
        "remote binding was revoked",
        "paired",
        "revoked",
        "Pair this client again with a fresh ticket.",
    )
}

fn ensure_pairing(
    state_home: &Path,
    resolved: &ResolvedProject,
    action: &str,
) -> Result<(), RemoteError> {
    let machine = load_machine_identity(state_home)?.unwrap_or_else(missing_machine_identity);
    let binding = load_binding(&resolved.data_dir)?.unwrap_or_else(|| missing_binding(resolved));
    if binding.binding_state == "revoked" || binding.status == "revoked" {
        return Err(RemoteError::ActionRejected(action_rejection(
            action,
            "remote_binding_revoked",
            "remote binding was revoked",
            "paired",
            "revoked",
            "Pair this client again with a fresh ticket.",
        )));
    }
    if let Some(client) = load_client_identity(state_home, &binding.client_id)? {
        if client.status == "revoked" {
            return Err(RemoteError::ActionRejected(action_rejection(
                action,
                "remote_client_revoked",
                "remote client identity was revoked",
                "paired",
                "revoked",
                "Pair this client again with a fresh ticket.",
            )));
        }
    }
    if let Some(lease) = load_lease(state_home)? {
        if lease_is_expired(&lease) || lease.lease_state == "expired" {
            return Err(RemoteError::ActionRejected(action_rejection(
                action,
                "remote_lease_expired",
                "remote control lease expired",
                "active_lease",
                "expired",
                "Pair again or renew the lease before retrying remote control.",
            )));
        }
    }
    if machine.status == "ready" && binding.binding_state == "paired" {
        return Ok(());
    }
    Err(RemoteError::ActionRejected(attach_rejection(action)))
}

pub fn not_graduated_rejection(action: &str) -> RemoteActionRejection {
    action_rejection(
        action,
        &format!("remote_{action}_not_graduated"),
        "remote command is advertised but not graduated for execution",
        "graduated_remote_command",
        "not_graduated",
        "Use `research-cli remote status --json` to inspect feature advertisements.",
    )
}

fn action_rejection(
    action: &str,
    rejection_code: &str,
    reason: &str,
    required_state: &str,
    current_state: &str,
    next_step: &str,
) -> RemoteActionRejection {
    RemoteActionRejection {
        schema_version: "1".to_string(),
        action: action.to_string(),
        rejection_code: rejection_code.to_string(),
        reason: reason.to_string(),
        retryable: false,
        required_state: required_state.to_string(),
        current_state: current_state.to_string(),
        next_steps: vec![next_step.to_string()],
    }
}

fn status_from_parts(
    state_home: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
    machine: MachineIdentity,
    client_identity: Option<RemoteClientIdentity>,
    binding: RemoteBinding,
) -> Result<RemoteStatusReport, RemoteError> {
    let client_revoked = client_identity
        .as_ref()
        .map(|client| client.status == "revoked")
        .unwrap_or(false);
    let persisted_lease = load_lease(state_home)?;
    let lease_expired = persisted_lease
        .as_ref()
        .map(lease_is_expired)
        .unwrap_or(false);
    let revoked = binding.binding_state == "revoked" || client_revoked;
    let paired = machine.status == "ready"
        && binding.binding_state == "paired"
        && !client_revoked
        && !lease_expired;
    let cursor = load_cursor(&resolved.data_dir)?.unwrap_or_else(|| default_cursor(resolved));
    let mut control_owner = load_control_owner(&resolved.data_dir)?
        .unwrap_or_else(|| default_control_owner(resolved, &binding));
    if lease_expired && control_owner.control_lease.lease_state != "expired" {
        control_owner.control_lease.lease_state = "expired".to_string();
        control_owner.owner_state = "expired".to_string();
        write_json(&control_owner_path(&resolved.data_dir), &control_owner)?;
        write_json(&lease_path(state_home), &control_owner.control_lease)?;
    } else if paired && control_owner.control_lease.lease_state != "active" {
        control_owner = control_owner_for(
            resolved,
            &binding.client_id,
            &control_owner.owner_state,
            current_ownership_epoch(&control_owner),
            persisted_lease
                .as_ref()
                .map(|lease| lease.expires_at.clone())
                .unwrap_or_default(),
        );
        write_json(&control_owner_path(&resolved.data_dir), &control_owner)?;
        write_json(&lease_path(state_home), &control_owner.control_lease)?;
    }
    let (projection, projection_regenerated) =
        projection_for_status(&resolved.data_dir, resolved, active_session_id)?;
    let capability_matrix = if paired {
        graduated_capability_matrix()
    } else {
        default_capability_matrix()
    };
    let feature_advertisement = if paired {
        graduated_feature_advertisement()
    } else {
        default_feature_advertisement()
    };
    let revoked_at = client_identity
        .as_ref()
        .and_then(|client| client.revoked_at.clone());
    let workbench = workbench_projection(resolved, &cursor);
    Ok(RemoteStatusReport {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        workspace_root: resolved.workspace_root.display().to_string(),
        remote_ready: paired,
        machine_metadata: load_machine_metadata(state_home)?
            .unwrap_or_else(|| default_machine_metadata(&machine)),
        daemon_state: load_daemon_state(state_home)?
            .unwrap_or_else(|| default_daemon_state(&machine)),
        capability_matrix,
        offline_action_policy: if paired {
            graduated_offline_action_policy()
        } else {
            default_offline_action_policy()
        },
        switch_policy: default_switch_policy(),
        feature_advertisement,
        terminal_lease: default_terminal_lease(&control_owner),
        lease: control_owner.control_lease.clone(),
        projection_regenerated,
        machine_identity: machine,
        client_identity,
        binding,
        cursor,
        control_owner,
        projection,
        revocation_reason: if revoked {
            Some("revoked".to_string())
        } else if lease_expired {
            Some("lease_expired".to_string())
        } else {
            None
        },
        revoked_at,
        workbench,
    })
}

fn write_project_remote_files(
    data_dir: &Path,
    resolved: &ResolvedProject,
    binding: &RemoteBinding,
    lease_expires_at: &str,
    active_session_id: Option<String>,
) -> Result<(), RemoteError> {
    write_json(&remote_dir(data_dir).join("binding.json"), binding)?;
    write_json(
        &remote_dir(data_dir).join("cursor.json"),
        &default_cursor(resolved),
    )?;
    write_json(
        &control_owner_path(data_dir),
        &control_owner_for(
            resolved,
            &binding.client_id,
            "available",
            0,
            lease_expires_at.to_string(),
        ),
    )?;
    regenerate_projection(data_dir, resolved, active_session_id)?;
    Ok(())
}

fn write_machine_remote_files(
    state_home: &Path,
    machine: &MachineIdentity,
    binding: Option<&RemoteBinding>,
    lease_expires_at: &str,
) -> Result<(), RemoteError> {
    write_json(
        &remote_root(state_home).join("machine_metadata.json"),
        &default_machine_metadata(machine),
    )?;
    write_json(
        &remote_root(state_home).join("daemon_state.json"),
        &default_daemon_state(machine),
    )?;
    let lease = binding
        .map(|binding| ControlLease {
            schema_version: "1".to_string(),
            lease_id: format!("lease_{}", binding.project_id),
            project_id: binding.project_id.clone(),
            owner_id: binding.client_id.clone(),
            lease_state: "active".to_string(),
            terminal_scope: "project".to_string(),
            expires_at: lease_expires_at.to_string(),
        })
        .unwrap_or_else(|| ControlLease {
            schema_version: "1".to_string(),
            lease_id: format!("lease_{}", machine.machine_id),
            project_id: "unbound".to_string(),
            owner_id: "none".to_string(),
            lease_state: "inactive".to_string(),
            terminal_scope: "machine".to_string(),
            expires_at: String::new(),
        });
    write_json(&lease_path(state_home), &lease)?;
    write_json(
        &remote_root(state_home).join("capabilities.json"),
        &graduated_capability_matrix(),
    )?;
    Ok(())
}

fn load_machine_identity(state_home: &Path) -> Result<Option<MachineIdentity>, RemoteError> {
    read_json_optional(&machine_identity_path(state_home))
}

fn load_client_identity(
    state_home: &Path,
    client_id: &str,
) -> Result<Option<RemoteClientIdentity>, RemoteError> {
    read_json_optional(&client_identity_path(state_home, client_id))
}

fn load_binding(data_dir: &Path) -> Result<Option<RemoteBinding>, RemoteError> {
    read_json_optional(&remote_dir(data_dir).join("binding.json"))
}

fn load_cursor(data_dir: &Path) -> Result<Option<RemoteCursor>, RemoteError> {
    read_json_optional(&remote_dir(data_dir).join("cursor.json"))
}

fn load_pair_ticket(state_home: &Path, ticket_id: &str) -> Result<Option<PairTicket>, RemoteError> {
    read_json_optional(&pair_ticket_path(state_home, ticket_id))
}

fn load_lease(state_home: &Path) -> Result<Option<ControlLease>, RemoteError> {
    read_json_optional(&lease_path(state_home))
}

pub fn load_control_owner(data_dir: &Path) -> Result<Option<RemoteControlOwner>, RemoteError> {
    read_json_optional(&control_owner_path(data_dir))
}

fn load_projection(data_dir: &Path) -> Result<Option<RemoteProjectionSnapshot>, RemoteError> {
    read_json_optional(&remote_dir(data_dir).join("projection.json"))
}

fn load_machine_metadata(state_home: &Path) -> Result<Option<RemoteMachineMetadata>, RemoteError> {
    read_json_optional(&remote_root(state_home).join("machine_metadata.json"))
}

fn load_daemon_state(state_home: &Path) -> Result<Option<RemoteDaemonState>, RemoteError> {
    read_json_optional(&remote_root(state_home).join("daemon_state.json"))
}

fn read_json_optional<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, RemoteError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), RemoteError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn remote_root(state_home: &Path) -> PathBuf {
    state_home.join("remote")
}

fn remote_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("remote")
}

fn machine_identity_path(state_home: &Path) -> PathBuf {
    remote_root(state_home).join("machine_identity.json")
}

fn client_identity_path(state_home: &Path, client_id: &str) -> PathBuf {
    remote_root(state_home)
        .join("clients")
        .join(format!("{client_id}.json"))
}

fn pair_ticket_path(state_home: &Path, ticket_id: &str) -> PathBuf {
    remote_root(state_home)
        .join("pair_tickets")
        .join(format!("{ticket_id}.json"))
}

fn lease_path(state_home: &Path) -> PathBuf {
    remote_root(state_home).join("lease.json")
}

fn control_owner_path(data_dir: &Path) -> PathBuf {
    remote_dir(data_dir).join("control_owner.json")
}

fn missing_machine_identity() -> MachineIdentity {
    MachineIdentity {
        schema_version: "1".to_string(),
        machine_id: String::new(),
        status: "missing".to_string(),
        key_algorithm: String::new(),
        public_key_ref: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn missing_binding(resolved: &ResolvedProject) -> RemoteBinding {
    RemoteBinding {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        workspace_root: resolved.workspace_root.display().to_string(),
        binding_state: "missing".to_string(),
        status: "missing".to_string(),
        client_id: String::new(),
        machine_id: String::new(),
        scope: "project".to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn default_cursor(resolved: &ResolvedProject) -> RemoteCursor {
    RemoteCursor {
        schema_version: "1".to_string(),
        cursor_id: format!("cursor_{}", resolved.project_id),
        project_id: resolved.project_id.clone(),
        cursor_state: "initialized".to_string(),
        last_event_seq: 0,
        ownership_epoch: 0,
        replay_token: format!("replay_{}", resolved.project_id),
        replay_policy: "replay_from_cursor".to_string(),
        updated_at: timestamp_string(),
    }
}

fn default_control_owner(
    resolved: &ResolvedProject,
    binding: &RemoteBinding,
) -> RemoteControlOwner {
    let owner_id = if binding.client_id.is_empty() {
        "none".to_string()
    } else {
        binding.client_id.clone()
    };
    RemoteControlOwner {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        owner_state: if binding.binding_state == "paired" {
            "available".to_string()
        } else {
            "missing".to_string()
        },
        owner_id: owner_id.clone(),
        control_lease: ControlLease {
            schema_version: "1".to_string(),
            lease_id: format!("lease_{}", resolved.project_id),
            project_id: resolved.project_id.clone(),
            owner_id,
            lease_state: "inactive".to_string(),
            terminal_scope: "project".to_string(),
            expires_at: String::new(),
        },
        ownership_epoch: 0,
        updated_at: timestamp_string(),
    }
}

fn control_owner_for(
    resolved: &ResolvedProject,
    owner_id: &str,
    owner_state: &str,
    ownership_epoch: u64,
    expires_at: String,
) -> RemoteControlOwner {
    RemoteControlOwner {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        owner_state: owner_state.to_string(),
        owner_id: owner_id.to_string(),
        control_lease: ControlLease {
            schema_version: "1".to_string(),
            lease_id: format!("lease_{}", resolved.project_id),
            project_id: resolved.project_id.clone(),
            owner_id: owner_id.to_string(),
            lease_state: "active".to_string(),
            terminal_scope: "project".to_string(),
            expires_at,
        },
        ownership_epoch,
        updated_at: timestamp_string(),
    }
}

fn current_ownership_epoch(owner: &RemoteControlOwner) -> u64 {
    owner.ownership_epoch
}

fn default_machine_metadata(machine: &MachineIdentity) -> RemoteMachineMetadata {
    RemoteMachineMetadata {
        schema_version: "1".to_string(),
        machine_id: machine.machine_id.clone(),
        display_name: "local-machine".to_string(),
        last_known_state: if machine.status == "ready" {
            "online".to_string()
        } else {
            "unknown".to_string()
        },
        updated_at: timestamp_string(),
    }
}

fn default_daemon_state(machine: &MachineIdentity) -> RemoteDaemonState {
    RemoteDaemonState {
        schema_version: "1".to_string(),
        machine_id: machine.machine_id.clone(),
        daemon_state: "not_running".to_string(),
        last_shutdown_state: "unknown".to_string(),
        updated_at: timestamp_string(),
    }
}

fn default_capability_matrix() -> RemoteCapabilityMatrix {
    RemoteCapabilityMatrix {
        schema_version: "1".to_string(),
        machine: RemoteCapabilitySet {
            schema_version: "1".to_string(),
            scope: "machine".to_string(),
            capabilities: vec!["pair".to_string(), "status".to_string()],
        },
        project: RemoteCapabilitySet {
            schema_version: "1".to_string(),
            scope: "project".to_string(),
            capabilities: vec![
                "binding".to_string(),
                "projection".to_string(),
                "cursor".to_string(),
            ],
        },
        session: RemoteCapabilitySet {
            schema_version: "1".to_string(),
            scope: "session".to_string(),
            capabilities: vec!["attach_not_graduated".to_string()],
        },
    }
}

fn graduated_capability_matrix() -> RemoteCapabilityMatrix {
    RemoteCapabilityMatrix {
        schema_version: "1".to_string(),
        machine: RemoteCapabilitySet {
            schema_version: "1".to_string(),
            scope: "machine".to_string(),
            capabilities: vec![
                "pair".to_string(),
                "status".to_string(),
                "lease".to_string(),
                "revocation".to_string(),
                "reconnect".to_string(),
                "capability_advertisement".to_string(),
            ],
        },
        project: RemoteCapabilitySet {
            schema_version: "1".to_string(),
            scope: "project".to_string(),
            capabilities: vec![
                "binding".to_string(),
                "projection".to_string(),
                "cursor".to_string(),
                "event_receipts".to_string(),
                "workbench".to_string(),
            ],
        },
        session: RemoteCapabilitySet {
            schema_version: "1".to_string(),
            scope: "session".to_string(),
            capabilities: vec![
                "attach".to_string(),
                "handoff".to_string(),
                "takeover".to_string(),
                "notify".to_string(),
                "permission_response".to_string(),
                "message_steering".to_string(),
            ],
        },
    }
}

fn default_offline_action_policy() -> OfflineActionPolicy {
    OfflineActionPolicy {
        schema_version: "1".to_string(),
        default_policy: "reject_mutations".to_string(),
        allowed_actions: vec!["status".to_string()],
        rejected_actions: vec![
            "attach".to_string(),
            "handoff".to_string(),
            "takeover".to_string(),
        ],
    }
}

fn graduated_offline_action_policy() -> OfflineActionPolicy {
    OfflineActionPolicy {
        schema_version: "1".to_string(),
        default_policy: "reject_unleased_mutations".to_string(),
        allowed_actions: vec![
            "status".to_string(),
            "attach".to_string(),
            "handoff".to_string(),
            "takeover".to_string(),
            "notify".to_string(),
            "permission_response".to_string(),
            "message_steering".to_string(),
        ],
        rejected_actions: Vec::new(),
    }
}

fn default_switch_policy() -> LocalRemoteSwitchPolicy {
    LocalRemoteSwitchPolicy {
        schema_version: "1".to_string(),
        active_plane: "local".to_string(),
        switch_policy: "explicit_control_lease_required".to_string(),
        local_authority: "kernel_state_bundle".to_string(),
    }
}

fn default_feature_advertisement() -> RemoteFeatureAdvertisement {
    RemoteFeatureAdvertisement {
        schema_version: "1".to_string(),
        substrate: "available".to_string(),
        relay_transport: "tailscale_private_overlay_unpaired".to_string(),
        attach: "guarded".to_string(),
        handoff: "not_graduated".to_string(),
        takeover: "not_graduated".to_string(),
        notify: "not_graduated".to_string(),
        permission_response: "guarded".to_string(),
        message_steering: "guarded".to_string(),
        workbench: "available".to_string(),
        revocation: "guarded".to_string(),
    }
}

fn graduated_feature_advertisement() -> RemoteFeatureAdvertisement {
    RemoteFeatureAdvertisement {
        schema_version: "1".to_string(),
        substrate: "available".to_string(),
        relay_transport: "tailscale_private_overlay".to_string(),
        attach: "ready".to_string(),
        handoff: "ready".to_string(),
        takeover: "ready".to_string(),
        notify: "ready".to_string(),
        permission_response: "ready".to_string(),
        message_steering: "ready".to_string(),
        workbench: "ready".to_string(),
        revocation: "ready".to_string(),
    }
}

fn projection_snapshot(
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> RemoteProjectionSnapshot {
    let runtime_state = if active_session_id.is_some() {
        "session_active"
    } else {
        "project_registered"
    };
    let effective = crate::config::effective_config(resolved).ok();
    let model = effective
        .as_ref()
        .and_then(|c| c.effective.default_model.clone());
    let reasoning_effort = effective
        .as_ref()
        .and_then(|c| c.effective.values.get("reasoning_effort"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let permission_mode = effective
        .as_ref()
        .and_then(|c| c.effective.permission_mode.clone());
    let branch = std::process::Command::new("git")
        .current_dir(&resolved.workspace_root)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    RemoteProjectionSnapshot {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        projection_state: "derived".to_string(),
        regeneration_policy: "regenerate_on_mismatch".to_string(),
        runtime_descriptor: SessionRuntimeDescriptor {
            schema_version: "1".to_string(),
            project_id: resolved.project_id.clone(),
            active_session_id: active_session_id.clone(),
            runtime_state: runtime_state.to_string(),
            model,
            reasoning_effort,
            permission_mode,
            branch,
            workspace_root: resolved.workspace_root.display().to_string(),
        },
        envelope_projection: SessionEnvelopeProjection {
            schema_version: "1".to_string(),
            projection_id: format!("projection_{}", resolved.project_id),
            project_id: resolved.project_id.clone(),
            source: "kernel_state_bundle".to_string(),
            active_session_id,
        },
        updated_at: timestamp_string(),
    }
}

fn regenerate_projection(
    data_dir: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<RemoteProjectionSnapshot, RemoteError> {
    let projection = projection_snapshot(resolved, active_session_id);
    write_json(&remote_dir(data_dir).join("projection.json"), &projection)?;
    Ok(projection)
}

fn projection_for_status(
    data_dir: &Path,
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> Result<(RemoteProjectionSnapshot, bool), RemoteError> {
    let current = projection_snapshot(resolved, active_session_id.clone());
    let loaded = load_projection(data_dir)?;
    let needs_regeneration = loaded
        .as_ref()
        .map(|projection| {
            projection.project_id != current.project_id
                || projection.runtime_descriptor.project_id != current.runtime_descriptor.project_id
                || projection.envelope_projection.project_id
                    != current.envelope_projection.project_id
                || projection.runtime_descriptor.active_session_id
                    != current.runtime_descriptor.active_session_id
                || projection.envelope_projection.active_session_id
                    != current.envelope_projection.active_session_id
                || projection.runtime_descriptor.runtime_state
                    != current.runtime_descriptor.runtime_state
                || projection.runtime_descriptor.model != current.runtime_descriptor.model
                || projection.runtime_descriptor.reasoning_effort
                    != current.runtime_descriptor.reasoning_effort
                || projection.runtime_descriptor.permission_mode
                    != current.runtime_descriptor.permission_mode
                || projection.runtime_descriptor.branch != current.runtime_descriptor.branch
                || projection.runtime_descriptor.workspace_root
                    != current.runtime_descriptor.workspace_root
        })
        .unwrap_or(true);
    if needs_regeneration {
        write_json(&remote_dir(data_dir).join("projection.json"), &current)?;
        return Ok((current, true));
    }
    Ok((
        loaded.expect("projection exists when regeneration is not required"),
        false,
    ))
}

fn default_terminal_lease(owner: &RemoteControlOwner) -> RemoteTerminalLease {
    RemoteTerminalLease {
        schema_version: "1".to_string(),
        terminal_lease_id: owner.control_lease.lease_id.clone(),
        lease_state: owner.control_lease.lease_state.clone(),
        policy: "explicit_attach_required".to_string(),
    }
}

fn workbench_projection(
    resolved: &ResolvedProject,
    _cursor: &RemoteCursor,
) -> RemoteWorkbenchProjection {
    let pending_permissions = pending_permissions(&resolved.data_dir);
    let memory_summary = memory::status(&resolved.data_dir, None)
        .map(|status| RemoteWorkbenchCountSummary {
            status: "durable_memory_available".to_string(),
            count: status.durable_count,
        })
        .unwrap_or_else(|err| RemoteWorkbenchCountSummary {
            status: format!("working_memory_degraded:{err}"),
            count: 0,
        });
    let branch_summary = branches::list(&resolved.data_dir)
        .map(|branches| RemoteWorkbenchCountSummary {
            status: "guarded".to_string(),
            count: branches.total_count,
        })
        .unwrap_or_else(|err| RemoteWorkbenchCountSummary {
            status: format!("branch_index_degraded:{err}"),
            count: 0,
        });
    RemoteWorkbenchProjection {
        schema_version: "1".to_string(),
        project_id: resolved.project_id.clone(),
        source: "kernel_state_bundle_and_project_indexes".to_string(),
        authoritative: false,
        updated_at: timestamp_string(),
        transport: RemoteWorkbenchTransport {
            mode: "tailscale_private_overlay".to_string(),
            authority: "transport_only".to_string(),
        },
        sections: RemoteWorkbenchSections {
            project_status: "available".to_string(),
            sessions: "available".to_string(),
            permissions: "available".to_string(),
            reviews: "available".to_string(),
            artifacts: "available".to_string(),
            memory: "durable_memory_available".to_string(),
            branches: "guarded".to_string(),
        },
        permissions: RemoteWorkbenchPermissionSummary {
            pending_count: pending_permissions.len(),
            pending: pending_permissions,
        },
        reviews: RemoteWorkbenchCountSummary {
            status: "available".to_string(),
            count: count_child_dirs(&resolved.data_dir.join("reviews")),
        },
        artifacts: RemoteWorkbenchCountSummary {
            status: "available".to_string(),
            count: count_child_dirs(&resolved.data_dir.join("artifacts")),
        },
        memory: memory_summary,
        branches: branch_summary,
    }
}

fn pending_permissions(data_dir: &Path) -> Vec<RemoteWorkbenchPermissionRequest> {
    let events_path = data_dir.join("events").join("events.jsonl");
    let Ok(contents) = fs::read_to_string(events_path) else {
        return Vec::new();
    };
    let mut pending = std::collections::BTreeMap::new();
    let mut resolved = std::collections::BTreeSet::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("event_name").and_then(|v| v.as_str()) != Some("permission") {
            continue;
        }
        let Some(request_id) = value.get("object_id").and_then(|v| v.as_str()) else {
            continue;
        };
        match value.get("phase").and_then(|v| v.as_str()) {
            Some("start") => {
                let payload = value.get("payload").unwrap_or(&serde_json::Value::Null);
                pending.insert(
                    request_id.to_string(),
                    RemoteWorkbenchPermissionRequest {
                        request_id: request_id.to_string(),
                        session_id: value
                            .get("session_id")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string),
                        tool_name: payload
                            .get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        target_path: payload
                            .get("target_path")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string),
                        permission_mode: payload
                            .get("permission_mode")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                    },
                );
            }
            Some("terminal") => {
                resolved.insert(request_id.to_string());
            }
            _ => {}
        }
    }
    for request_id in resolved {
        pending.remove(&request_id);
    }
    pending.into_values().collect()
}

fn count_child_dirs(path: &Path) -> usize {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn lease_is_expired(lease: &ControlLease) -> bool {
    timestamp_is_expired(&lease.expires_at)
}

fn timestamp_is_expired(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .parse::<u128>()
        .map(|expires_at| timestamp_millis() >= expires_at)
        .unwrap_or(false)
}

fn sanitize_identifier(value: &str) -> Result<String, RemoteError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RemoteError::InvalidInput("identifier is empty".to_string()));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(RemoteError::InvalidInput(format!(
            "identifier contains unsupported characters: {trimmed}"
        )));
    }
    Ok(trimmed.to_string())
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn timestamp_string() -> String {
    timestamp_millis().to_string()
}
