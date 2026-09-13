use super::{RemoteError, RemoteStatusReport};
use crate::artifacts;
use crate::events::writer::{append_event_unlocked, lock_event_log};
use crate::events::{read_events_from, KernelEventEnvelope};
use crate::host_surface;
use crate::projects::current::{resolve_current_project, ResolvedProject};
use crate::projects::registry::{stable_project_id, ProjectRegistry};
use crate::remote;
use crate::routines;
use crate::runtime::cancel::active_runtime_turn_registry;
use crate::runtime::checkpoint::KernelStateBundle;
use crate::runtime::reducer::{publish_checkpoint, CheckpointPublishPlan, ReducerError};
use crate::runtime::run_remote_prompt_turn_streaming;
use crate::session::store::SessionStore;
use crate::session::transcript::TranscriptLine;
use crate::skills;
use crate::workspace::resolve::resolve_workspace_from;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static EVENT_SEQ_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedEventSeq>>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct CachedEventSeq {
    seq: u64,
    file_len: u64,
}

const INDEX_HTML: &str = include_str!("../assets/mobile/index.html");
const APP_JS: &str = include_str!("../assets/mobile/app.js");
const STYLES_CSS: &str = include_str!("../assets/mobile/styles.css");
const MANIFEST: &str = include_str!("../assets/mobile/manifest.webmanifest");
const SERVICE_WORKER: &str = include_str!("../assets/mobile/sw.js");
const APP_ICON_SVG: &str = include_str!("../assets/mobile/icons/icon.svg");
const FAVICON_SVG: &str = include_str!("../assets/mobile/favicon.ico");

#[derive(Debug, Clone)]
pub struct DaemonContext {
    pub state_home: PathBuf,
    pub default_cwd: PathBuf,
    pub control_token: Option<String>,
    terminal_ws_tickets: Arc<Mutex<HashMap<String, TerminalWsTicket>>>,
}

impl DaemonContext {
    pub fn new(state_home: PathBuf, default_cwd: PathBuf) -> Self {
        Self {
            state_home,
            default_cwd,
            control_token: None,
            terminal_ws_tickets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_control_token(mut self, token: impl Into<String>) -> Self {
        self.control_token = Some(token.into());
        self
    }
}

#[derive(Debug, Clone)]
struct TerminalWsTicket {
    cwd: String,
    expires_at: Instant,
    consumed: bool,
}

#[derive(Debug, Clone)]
pub struct DaemonRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DaemonResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonServerInfo {
    pub schema_version: String,
    pub host: String,
    pub port: u16,
    pub bind_addr: String,
    pub app_url: String,
    pub default_cwd: String,
}

#[derive(Debug)]
pub enum DaemonError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Envelope { status: u16, envelope: Value },
    Internal(String),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "remote daemon IO failed: {err}"),
            Self::Json(err) => write!(f, "remote daemon JSON failed: {err}"),
            Self::Envelope { envelope, .. } => write!(f, "remote daemon rejected: {envelope}"),
            Self::Internal(message) => write!(f, "remote daemon failed: {message}"),
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<std::io::Error> for DaemonError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for DaemonError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn serve(
    context: DaemonContext,
    host: &str,
    port: u16,
    announce_json: bool,
) -> Result<(), DaemonError> {
    let listener = TcpListener::bind((host, port))?;
    let addr = listener.local_addr()?;
    announce(&context, host, addr, announce_json)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let peer = stream
                    .peer_addr()
                    .map(|addr| addr.to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                let context = context.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_stream(&context, stream) {
                        eprintln!("remote daemon request failed from {peer}: {err}");
                    }
                });
            }
            Err(err) => eprintln!("remote daemon accept failed: {err}"),
        }
    }
    Ok(())
}

pub fn handle_request(
    context: &DaemonContext,
    request: DaemonRequest,
) -> Result<DaemonResponse, DaemonError> {
    let method = request.method.to_ascii_uppercase();
    let (path, query) = split_path_query(&request.path);

    if method == "OPTIONS" {
        return Ok(empty_response(204, "application/json; charset=utf-8"));
    }
    if path.starts_with("/api/") && path != "/api/health" && path != "/api/terminal/ws" {
        if let Some(response) = authorize_api_request(context, &request) {
            return Ok(response);
        }
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => Ok(text_response(
            200,
            "text/html; charset=utf-8",
            INDEX_HTML.to_string(),
        )),
        ("GET", "/app.js") => Ok(text_response(
            200,
            "application/javascript; charset=utf-8",
            APP_JS.to_string(),
        )),
        ("GET", "/styles.css") => Ok(text_response(
            200,
            "text/css; charset=utf-8",
            STYLES_CSS.to_string(),
        )),
        ("GET", "/manifest.webmanifest") => Ok(text_response(
            200,
            "application/manifest+json; charset=utf-8",
            MANIFEST.to_string(),
        )),
        ("GET", "/sw.js") => Ok(text_response(
            200,
            "application/javascript; charset=utf-8",
            SERVICE_WORKER.to_string(),
        )),
        ("GET", "/icons/icon.svg") => Ok(text_response(
            200,
            "image/svg+xml",
            APP_ICON_SVG.to_string(),
        )),
        ("GET", "/favicon.ico") => Ok(text_response(200, "image/svg+xml", FAVICON_SVG.to_string())),
        ("GET", "/api/health") => json_response(
            200,
            &json!({
                "ok": true,
                "command": "remote daemon health",
                "data": {
                    "schema_version": "1",
                    "daemon_state": "running",
                    "default_cwd": context.default_cwd.display().to_string()
                }
            }),
        ),
        ("GET", "/api/status") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let active_session_id = active_session_id(context, &resolved)?;
            let report = remote::status(&context.state_home, &resolved, active_session_id)
                .map_err(remote_internal)?;
            json_success("remote status", &resolved, None, report)
        }
        ("GET", "/api/host-surface") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let active_session_id = active_session_id(context, &resolved)?;
            let report = host_surface::status(&context.state_home, &resolved, active_session_id)
                .map_err(host_surface_internal)?;
            json_success("host surface status", &resolved, None, report)
        }
        ("GET", "/api/tui/actions") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let active_session_id = active_session_id(context, &resolved)?;
            let actions =
                host_surface::tui_actions(&context.state_home, &resolved, active_session_id)
                    .map_err(host_surface_internal)?;
            json_success("tui actions", &resolved, None, actions)
        }
        ("POST", "/api/tui/action") => {
            let input: TuiActionRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            let action_id = required(input.action_id.clone(), "action_id")?;
            execute_tui_action(context, &resolved, &action_id, input)
        }
        ("GET", "/api/skills") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let snapshot = skills::discover(&context.state_home, &resolved.workspace_root);
            if query.get("kind").map(String::as_str) == Some("inspect") {
                let skill_id = required(query.get("skill_id").cloned(), "skill_id")?;
                let inspected = skills::inspect(&snapshot, &skill_id)
                    .map_err(|err| DaemonError::Internal(format!("skill inspect failed: {err}")))?;
                return json_success(
                    "remote skills inspect",
                    &resolved,
                    None,
                    json!({
                        "schema_version": "remote_skill_inspect.v1",
                        "skill_id": inspected.skill_id,
                        "description": inspected.description,
                        "manifest_path": inspected.manifest_path,
                        "source": inspected.source,
                        "enabled": inspected.enabled,
                        "degraded": inspected.degraded,
                        "disabled_reason": inspected.disabled_reason,
                        "stage_compatibility": inspected.stage_compatibility,
                        "dependency_health": inspected.dependency_health,
                        "availability": inspected.availability
                    }),
                );
            }
            let list = skills::list(&snapshot);
            json_success(
                "remote skills list",
                &resolved,
                None,
                json!({
                    "schema_version": "remote_skill_list.v1",
                    "search_paths": list.search_paths,
                    "skills": list.skills,
                    "total_count": list.total_count,
                    "degraded_count": list.degraded_count
                }),
            )
        }
        ("GET", "/api/artifacts") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            ensure_remote_ready(context, &resolved, "artifact_index")?;
            let list = artifacts::list(&resolved.data_dir, None, None, true);
            json_success(
                "remote artifacts",
                &resolved,
                None,
                json!({
                    "schema_version": "remote_artifact_index.v1",
                    "viewer_contract": "artifact_code_diff_pane",
                    "authority": "artifact_registry_read_only",
                    "list": list
                }),
            )
        }
        ("GET", "/api/artifact/inspect") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let target = query.get("target").cloned();
            let resolved = resolve_project(context, cwd.as_deref())?;
            ensure_remote_ready(context, &resolved, "artifact_view")?;
            let target = required(target, "target")?;
            let inspection =
                artifacts::inspect(&resolved.data_dir, &resolved.workspace_root, &target)
                    .map_err(|err| DaemonError::Internal(err.to_string()))?;
            let preview = artifact_preview(&inspection.canonical_path)?;
            json_success(
                "remote artifact inspect",
                &resolved,
                None,
                json!({
                    "schema_version": "remote_artifact_view.v1",
                    "viewer_contract": "artifact_code_diff_pane",
                    "authority": "artifact_registry_read_only",
                    "inspection": inspection,
                    "preview": preview
                }),
            )
        }
        ("GET", "/api/results") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            ensure_remote_ready(context, &resolved, "result_panel")?;
            let panel = result_panel_payload(&resolved)?;
            json_success("remote results", &resolved, None, panel)
        }
        ("GET", "/api/sessions") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let store = SessionStore::new(resolved.data_dir.clone());
            let sessions = store
                .list_sessions()
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            json_success(
                "remote sessions",
                &resolved,
                None,
                json!({
                    "schema_version": "1",
                    "sessions": sessions
                }),
            )
        }
        ("GET", "/api/session/transcript") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let requested_session_id = query.get("session_id").cloned();
            let resolved = resolve_project(context, cwd.as_deref())?;
            ensure_remote_ready(context, &resolved, "session_transcript")?;
            let session_id = resolve_session_id(
                context,
                &resolved,
                requested_session_id,
                "session_transcript",
            )?;
            ensure_session_exists(&resolved, &session_id)?;
            let lines = SessionStore::new(resolved.data_dir.clone())
                .read_transcript(&session_id)
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            json_success(
                "remote session transcript",
                &resolved,
                Some(session_id.clone()),
                json!({
                    "schema_version": "remote_session_transcript.v1",
                    "surface_contract": "conversation_primary_surface",
                    "terminal_lane": "terminal_compatibility_lane",
                    "session_id": session_id,
                    "line_count": lines.len(),
                    "lines": lines
                }),
            )
        }
        ("GET", "/api/events") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let after_seq = query
                .get("after_seq")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let wait_ms = query
                .get("wait_ms")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
                .min(30_000);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let body = render_event_stream(&resolved, after_seq, wait_ms)?;
            Ok(text_response(200, "text/event-stream; charset=utf-8", body))
        }
        ("POST", "/api/pair") => {
            let input: PairRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            let active_session_id = active_session_id(context, &resolved)?;
            let report = remote::pair_daemon(
                &context.state_home,
                &resolved,
                required(input.client_id, "client_id")?.as_str(),
                required(input.ticket_id, "ticket_id")?.as_str(),
                input.lease_expires_at.as_deref(),
                input.ticket_expires_at.as_deref(),
                active_session_id,
            )
            .map_err(|err| remote_to_daemon_error("remote pair", &resolved, err))?;
            mark_daemon_running(context, &report)?;
            json_success("remote pair", &resolved, None, report)
        }
        ("POST", "/api/quick-pair") => {
            let input: QuickPairRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            let active_session_id = active_session_id(context, &resolved)?;
            let client_id = required(input.client_id, "client_id")?;
            let auto_ticket_id = format!("auto-{}", client_id);
            let report = remote::pair_daemon_automatic(
                &context.state_home,
                &resolved,
                &client_id,
                &auto_ticket_id,
                None,
                None,
                active_session_id,
            )
            .map_err(|err| remote_to_daemon_error("remote quick pair", &resolved, err))?;
            mark_daemon_running(context, &report)?;
            json_success("remote quick pair", &resolved, None, report)
        }
        ("POST", "/api/reconnect") => {
            let input: ReconnectRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "reconnect")?;
            let active_session_id = active_session_id(context, &resolved)?;
            let report = remote::status(&context.state_home, &resolved, active_session_id)
                .map_err(remote_internal)?;
            let client_id = required(input.client_id, "client_id")?;
            if report.binding.client_id != client_id {
                return Err(remote_to_daemon_error(
                    "remote reconnect",
                    &resolved,
                    RemoteError::ActionRejected(remote::RemoteActionRejection {
                        schema_version: "1".to_string(),
                        action: "reconnect".to_string(),
                        rejection_code: "remote_client_mismatch".to_string(),
                        reason: format!(
                            "remote reconnect client mismatch: expected {}, got {client_id}",
                            report.binding.client_id
                        ),
                        retryable: false,
                        required_state: "current_paired_client".to_string(),
                        current_state: "different_client".to_string(),
                        next_steps: vec![
                            "Reconnect with the currently paired client id or pair this device again."
                                .to_string(),
                        ],
                    }),
                ));
            }
            let after_seq = input.after_seq.unwrap_or(0);
            let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
            let replay_events = read_events_from(&event_log_path)
                .map_err(|err| DaemonError::Internal(err.to_string()))?
                .into_iter()
                .filter(|event| event.seq > after_seq)
                .collect::<Vec<_>>();
            let replay = json!({
                "schema_version": "remote_reconnect_replay.v1",
                "after_seq": after_seq,
                "event_count": replay_events.len(),
                "events": replay_events,
                "replay_policy": report.cursor.replay_policy.clone()
            });
            append_remote_event(
                &resolved,
                "remote_reconnect",
                "remote_client",
                &client_id,
                report
                    .projection
                    .runtime_descriptor
                    .active_session_id
                    .as_deref(),
                json!({
                    "schema_version": "remote_reconnect.v1",
                    "client_id": client_id.clone(),
                    "after_seq": after_seq,
                    "replayed_event_count": replay["event_count"],
                    "cursor_id": report.cursor.cursor_id.clone(),
                    "reconnect_state": "accepted"
                }),
            )?;
            publish_project_checkpoint_for_daemon(&resolved, &["remote"])?;
            json_success(
                "remote reconnect",
                &resolved,
                report
                    .projection
                    .runtime_descriptor
                    .active_session_id
                    .clone(),
                json!({
                    "schema_version": "remote_reconnect_result.v1",
                    "client_id": client_id,
                    "status": report.clone(),
                    "projection": report.projection.clone(),
                    "replay": replay
                }),
            )
        }
        ("GET", "/api/workbench") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            let active_session_id = active_session_id(context, &resolved)?;
            let report = remote::status(&context.state_home, &resolved, active_session_id)
                .map_err(remote_internal)?;
            json_success("remote workbench", &resolved, None, report.workbench)
        }
        ("POST", "/api/attach") => {
            let input: AttachRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "attach")?;
            let session_id = resolve_session_id(context, &resolved, input.session_id, "attach")?;
            ensure_session_exists(&resolved, &session_id)?;
            let execute = input.execute.unwrap_or(false);
            let result = remote::attach(
                &context.state_home,
                &resolved,
                &session_id,
                input.strategy.as_deref(),
                execute,
            )
            .map_err(|err| remote_to_daemon_error("remote attach", &resolved, err))?;
            if execute {
                append_remote_event(
                    &resolved,
                    "remote_attach",
                    "session",
                    &session_id,
                    Some(&session_id),
                    serde_json::to_value(&result)?,
                )?;
            }
            json_success("remote attach", &resolved, Some(session_id), result)
        }
        ("POST", "/api/handoff") => {
            let input: HandoffRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "handoff")?;
            let session_id = resolve_session_id(context, &resolved, input.session_id, "handoff")?;
            let target_client = required(input.target_client, "target_client")?;
            ensure_session_exists(&resolved, &session_id)?;
            let execute = input.execute.unwrap_or(false);
            let result = remote::handoff(
                &context.state_home,
                &resolved,
                &session_id,
                &target_client,
                execute,
            )
            .map_err(|err| remote_to_daemon_error("remote handoff", &resolved, err))?;
            if execute {
                append_remote_event(
                    &resolved,
                    "remote_handoff",
                    "session",
                    &session_id,
                    Some(&session_id),
                    serde_json::to_value(&result)?,
                )?;
            }
            json_success("remote handoff", &resolved, Some(session_id), result)
        }
        ("POST", "/api/takeover") => {
            let input: TakeoverRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "takeover")?;
            let session_id = resolve_session_id(context, &resolved, input.session_id, "takeover")?;
            let client_id = required(input.client_id, "client_id")?;
            ensure_session_exists(&resolved, &session_id)?;
            let result = remote::takeover(&context.state_home, &resolved, &session_id, &client_id)
                .map_err(|err| remote_to_daemon_error("remote takeover", &resolved, err))?;
            append_remote_event(
                &resolved,
                "remote_takeover",
                "session",
                &session_id,
                Some(&session_id),
                serde_json::to_value(&result)?,
            )?;
            json_success("remote takeover", &resolved, Some(session_id), result)
        }
        ("POST", "/api/notify") => {
            let input: NotifyRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "notify")?;
            let kind = required(input.kind, "kind")?;
            let message = required(input.message, "message")?;
            let result = remote::notify(&context.state_home, &resolved, &kind, &message)
                .map_err(|err| remote_to_daemon_error("remote notify", &resolved, err))?;
            let active = active_session_id(context, &resolved)?;
            append_remote_event(
                &resolved,
                "remote_notify",
                "project",
                &resolved.project_id,
                active.as_deref(),
                serde_json::to_value(&result)?,
            )?;
            json_success("remote notify", &resolved, None, result)
        }
        ("POST", "/api/message") => {
            let input: MessageRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "message")?;
            let session_id = resolve_session_id(context, &resolved, input.session_id, "message")?;
            ensure_session_exists(&resolved, &session_id)?;
            let message = required(input.message, "message")?;

            // Implicit takeover: if client_id is provided and differs from current owner, claim ownership
            if let Some(ref client_id) = input.client_id {
                let current_owner = remote::load_control_owner(&resolved.data_dir)
                    .ok()
                    .flatten();
                let needs_takeover = current_owner
                    .map(|o| o.owner_id != *client_id)
                    .unwrap_or(true);
                if needs_takeover {
                    if let Ok(result) =
                        remote::takeover(&context.state_home, &resolved, &session_id, client_id)
                    {
                        let _ = append_remote_event(
                            &resolved,
                            "ownership_change",
                            "session",
                            &session_id,
                            Some(&session_id),
                            json!({
                                "action": "implicit_takeover",
                                "previous_owner": result.previous_owner.owner_id,
                                "new_owner": result.new_owner.owner_id,
                                "epoch": result.ownership_epoch,
                            }),
                        );
                    }
                }
            }
            let registry = ProjectRegistry::new(context.state_home.join("registry"));
            let mut accumulated_content = String::new();
            let mut delta_index = 0_usize;
            let mut delta_error = None;
            let turn_result = run_remote_prompt_turn_streaming(
                &registry,
                &resolved,
                &session_id,
                &message,
                |delta| {
                    if delta_error.is_some() {
                        return;
                    }
                    delta_index += 1;
                    accumulated_content.push_str(delta);
                    let payload = json!({
                        "schema_version": "remote_message_delta.v1",
                        "action": "message",
                        "message_state": "streaming_by_kernel",
                        "session_id": session_id,
                        "message": message,
                        "delta": delta,
                        "delta_index": delta_index,
                        "accumulated_content": accumulated_content,
                        "created_at": timestamp_string()
                    });
                    if let Err(err) = append_remote_event(
                        &resolved,
                        "remote_message_delta",
                        "session",
                        &session_id,
                        Some(&session_id),
                        payload,
                    ) {
                        delta_error = Some(err.to_string());
                    }
                },
            )
            .map_err(DaemonError::Internal)?;
            if let Some(err) = delta_error {
                return Err(DaemonError::Internal(err));
            }
            let payload = json!({
                "schema_version": "1",
                "action": "message",
                "message_state": "completed_by_kernel",
                "session_id": session_id,
                "message": message,
                "turn_result": turn_result,
                "created_at": timestamp_string()
            });
            append_remote_event(
                &resolved,
                "remote_message",
                "session",
                &session_id,
                Some(&session_id),
                payload.clone(),
            )?;
            publish_project_checkpoint_for_daemon(&resolved, &["remote"])?;
            json_success("remote message", &resolved, Some(session_id), payload)
        }
        ("POST", "/api/session/attachments") => {
            let input: AttachmentUploadRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "session_attachments")?;
            let session_id =
                resolve_session_id(context, &resolved, input.session_id, "session_attachments")?;
            ensure_session_exists(&resolved, &session_id)?;
            let payload =
                persist_conversation_attachments(&resolved, &session_id, input.attachments)?;
            append_remote_event(
                &resolved,
                "remote_attachment",
                "session",
                &session_id,
                Some(&session_id),
                payload.clone(),
            )?;
            SessionStore::new(resolved.data_dir.clone())
                .append_line(
                    &session_id,
                    TranscriptLine::Control {
                        event: format!(
                            "remote_attachment:{}",
                            payload["attachments"].as_array().map(Vec::len).unwrap_or(0)
                        ),
                    },
                )
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            publish_project_checkpoint_for_daemon(&resolved, &["remote", "attachment"])?;
            json_success(
                "remote session attachments",
                &resolved,
                Some(session_id),
                payload,
            )
        }
        ("POST", "/api/session/resume") => {
            let input: SessionResumeRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "session_resume")?;
            let session_id = required(input.session_id, "session_id")?;
            ensure_session_exists(&resolved, &session_id)?;
            ProjectRegistry::new(context.state_home.join("registry"))
                .touch_project(&resolved.project_id, Some(session_id.clone()))
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            let payload = json!({
                "schema_version": "1",
                "action": "session_resume",
                "source": "remote",
                "project_id": resolved.project_id,
                "session_id": session_id
            });
            append_remote_event(
                &resolved,
                "session_resume",
                "command",
                "remote session resume",
                Some(&session_id),
                payload.clone(),
            )?;
            publish_project_checkpoint_for_daemon(&resolved, &["session", "remote"])?;
            json_success(
                "remote session resume",
                &resolved,
                Some(session_id),
                payload,
            )
        }
        ("POST", "/api/session/create") => {
            let input: SessionCreateRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "session_create")?;
            let mode =
                normalize_permission_mode(required(input.permission_mode, "permission_mode")?)?;
            let title = input
                .title
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let store = SessionStore::new(resolved.data_dir.clone());
            let session = store
                .create_session(title)
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            let session_id = session.session_id.clone();
            let stored = store
                .load_session(&session_id)
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            let permission_payload =
                set_remote_permission_mode(&resolved, &mode, "session_create_permission_mode")?;
            ProjectRegistry::new(context.state_home.join("registry"))
                .touch_project(&resolved.project_id, Some(session_id.clone()))
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            append_remote_event(
                &resolved,
                "session_open",
                "session",
                &session_id,
                Some(&session_id),
                json!({
                    "status": "active",
                    "source": "remote",
                    "transcript_path": stored.transcript_path.display().to_string(),
                    "permission_mode": mode.as_str()
                }),
            )?;
            publish_project_checkpoint_for_daemon(
                &resolved,
                &["session", "permission_mode", "config", "remote"],
            )?;
            json_success(
                "remote session create",
                &resolved,
                Some(session_id),
                json!({
                    "schema_version": "1",
                    "action": "session_create",
                    "source": "remote",
                    "session": session,
                    "permission_mode": mode,
                    "permission": permission_payload
                }),
            )
        }
        ("PATCH", "/api/session/permission-mode") | ("PATCH", "/api/sessions/permission-mode") => {
            let input: PermissionModeRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "permission_mode")?;
            let mode =
                normalize_permission_mode(required(input.permission_mode, "permission_mode")?)?;
            let payload = set_remote_permission_mode(&resolved, &mode, "permission_mode")?;
            publish_project_checkpoint_for_daemon(
                &resolved,
                &["permission_mode", "config", "remote"],
            )?;
            json_success("remote permission mode", &resolved, None, payload)
        }
        ("POST", "/api/permission") => {
            let input: PermissionRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "permission_response")?;
            let request_id = required(input.request_id, "request_id")?;
            let decision = required(input.decision, "decision")?;
            let result = match resolve_permission_from_remote(&resolved, &request_id, &decision) {
                Ok(result) => result,
                Err(err) => return error_response(err),
            };
            append_remote_event(
                &resolved,
                "remote_permission_response",
                "permission",
                &request_id,
                result.get("session_id").and_then(Value::as_str),
                result.clone(),
            )?;
            publish_project_checkpoint_for_daemon(&resolved, &["permission", "remote"])?;
            json_success("remote permission", &resolved, None, result)
        }
        ("POST", "/api/terminal/attach") => {
            let input: TerminalRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "terminal_attach")?;
            let active_session_id = active_session_id(context, &resolved)?;
            let result =
                host_surface::terminal_attach(&context.state_home, &resolved, active_session_id)
                    .map_err(host_surface_internal)?;
            append_remote_event(
                &resolved,
                "remote_terminal_attach",
                "terminal",
                &result.terminal.terminal_id,
                result.terminal.active_session_id.as_deref(),
                serde_json::to_value(&result)?,
            )?;
            publish_project_checkpoint_for_daemon(&resolved, &["remote", "terminal"])?;
            json_success("remote terminal attach", &resolved, None, result)
        }
        ("GET", "/api/terminal/replay") => {
            let query = parse_query(&query);
            let cwd = query.get("cwd").map(PathBuf::from);
            let resolved = resolve_project(context, cwd.as_deref())?;
            ensure_remote_ready(context, &resolved, "terminal_replay")?;
            let active_session_id = active_session_id(context, &resolved)?;
            let result =
                host_surface::terminal_replay(&context.state_home, &resolved, active_session_id)
                    .map_err(host_surface_internal)?;
            json_success("remote terminal replay", &resolved, None, result)
        }
        ("POST", "/api/terminal/ws-ticket") => {
            let input: TerminalRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            ensure_remote_ready(context, &resolved, "terminal_websocket_ticket")?;
            let _terminal = attached_terminal_projection(&resolved)?;
            let ticket = issue_terminal_ws_ticket(context, &resolved)?;
            json_success("remote terminal websocket ticket", &resolved, None, ticket)
        }
        ("GET", "/api/terminal/ws") => {
            terminal_websocket_route(context, &request, &query).or_else(error_response)
        }
        ("POST", "/api/terminal/input") => {
            let input: TerminalInputRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            let payload = record_terminal_input(context, &resolved, &input.data)?;
            json_success("remote terminal input", &resolved, None, payload)
        }
        ("POST", "/api/terminal/resize") => {
            let input: TerminalResizeRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            let payload = record_terminal_resize(context, &resolved, input.cols, input.rows)?;
            json_success("remote terminal resize", &resolved, None, payload)
        }
        ("POST", "/api/terminal/signal") => {
            let input: TerminalSignalRequest = parse_body(&request.body)?;
            let resolved = resolve_project(context, input.cwd.as_deref().map(Path::new))?;
            let signal = required(input.signal, "signal")?;
            let payload = record_terminal_signal(context, &resolved, &signal)?;
            json_success("remote terminal signal", &resolved, None, payload)
        }
        _ => json_response(
            404,
            &json!({
                "ok": false,
                "command": "remote daemon",
                "error": {
                    "code": "not_found",
                    "message": format!("unknown daemon route: {} {}", method, path),
                    "hint": "Use / for the mobile app or /api/health for daemon health.",
                    "retryable": false
                }
            }),
        ),
    }
}

fn announce(
    context: &DaemonContext,
    host: &str,
    addr: SocketAddr,
    announce_json: bool,
) -> Result<(), DaemonError> {
    let app_host = if host == "0.0.0.0" { "127.0.0.1" } else { host };
    let info = DaemonServerInfo {
        schema_version: "1".to_string(),
        host: host.to_string(),
        port: addr.port(),
        bind_addr: addr.to_string(),
        app_url: format!("http://{app_host}:{}/", addr.port()),
        default_cwd: context.default_cwd.display().to_string(),
    };
    if announce_json {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "ok": true,
                "command": "remote daemon",
                "data": info
            }))?
        );
    } else {
        eprintln!("research-cli remote daemon listening at {}", info.app_url);
    }
    Ok(())
}

fn handle_stream(context: &DaemonContext, stream: TcpStream) -> Result<(), DaemonError> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let peer_addr = stream.peer_addr().ok();
    let request = read_http_request(stream.try_clone()?)?;
    eprintln!("remote daemon request: {} {}", request.method, request.path);
    if is_terminal_websocket_request(&request) {
        if !peer_addr
            .map(terminal_peer_allowed_for_private_overlay)
            .unwrap_or(false)
        {
            write_http_response(
                stream,
                json_response(
                    403,
                    &json!({
                        "ok": false,
                        "command": "remote terminal websocket",
                        "error": {
                            "code": "terminal_private_overlay_required",
                            "message": "terminal websocket requires loopback or Tailscale private overlay peer",
                            "hint": "Connect through Tailscale or local loopback; do not expose the terminal websocket on a public interface.",
                            "retryable": false
                        }
                    }),
                )?,
            )?;
            return Ok(());
        }
        return handle_terminal_websocket(context, request, stream);
    }
    let response = match handle_request(context, request) {
        Ok(response) => response,
        Err(err) => error_response(err)?,
    };
    write_http_response(stream, response)?;
    Ok(())
}

fn read_http_request(stream: TcpStream) -> Result<DaemonRequest, DaemonError> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| DaemonError::Internal("missing HTTP method".to_string()))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| DaemonError::Internal("missing HTTP path".to_string()))?
        .to_string();

    let mut content_length = 0usize;
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse::<usize>().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(DaemonRequest {
        method,
        path,
        headers,
        body,
    })
}

fn write_http_response(mut stream: TcpStream, response: DaemonResponse) -> Result<(), DaemonError> {
    let reason = match response.status {
        101 => "Switching Protocols",
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        426 => "Upgrade Required",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );
    for (name, value) in response.headers {
        headers.push_str(&format!("{name}: {value}\r\n"));
    }
    if response.status != 101 {
        headers.push_str("Connection: close\r\n");
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes())?;
    stream.write_all(&response.body)?;
    Ok(())
}

fn resolve_project(
    context: &DaemonContext,
    explicit_cwd: Option<&Path>,
) -> Result<ResolvedProject, DaemonError> {
    let registry = ProjectRegistry::new(context.state_home.join("registry"));
    match resolve_current_project(&registry, None, explicit_cwd, &context.default_cwd) {
        Ok(resolved) => Ok(resolved),
        Err(err) if explicit_cwd.is_none() => bootstrap_default_project(context, &registry)
            .map_err(|bootstrap_err| {
                DaemonError::Internal(format!(
                    "{err}; default daemon workspace bootstrap also failed: {bootstrap_err}"
                ))
            }),
        Err(err) => Err(DaemonError::Internal(err.to_string())),
    }
}

fn bootstrap_default_project(
    context: &DaemonContext,
    registry: &ProjectRegistry,
) -> Result<ResolvedProject, DaemonError> {
    let binding = resolve_workspace_from(&context.default_cwd)
        .map_err(|err| DaemonError::Internal(err.to_string()))?;
    let project_id = stable_project_id(&binding.workspace_root)
        .map_err(|err| DaemonError::Internal(err.to_string()))?;
    let entry = registry
        .entry_for_workspace(project_id, binding.workspace_root.clone())
        .map_err(|err| DaemonError::Internal(err.to_string()))?;
    registry
        .register(entry.clone())
        .map_err(|err| DaemonError::Internal(err.to_string()))?;
    registry
        .set_current_project(entry.to_pointer())
        .map_err(|err| DaemonError::Internal(err.to_string()))?;
    Ok(ResolvedProject {
        project_id: entry.project_id,
        workspace_root: entry.workspace_root,
        workspace_hash: entry.workspace_hash,
        data_dir: entry.data_dir,
        resolution_source: "remote_daemon_default_cwd_bootstrap".to_string(),
    })
}

fn active_session_id(
    context: &DaemonContext,
    resolved: &ResolvedProject,
) -> Result<Option<String>, DaemonError> {
    let registry = ProjectRegistry::new(context.state_home.join("registry"));
    registry
        .get_by_project_id(&resolved.project_id)
        .map(|entry| entry.and_then(|entry| entry.active_session_id))
        .map_err(|err| DaemonError::Internal(err.to_string()))
}

fn resolve_session_id(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    requested: Option<String>,
    action: &str,
) -> Result<String, DaemonError> {
    if let Some(session_id) = requested.filter(|value| !value.trim().is_empty()) {
        return Ok(session_id);
    }
    active_session_id(context, resolved)?.ok_or_else(|| {
        DaemonError::Internal(format!(
            "missing session_id for remote {action}; pass session_id in the request body"
        ))
    })
}

fn ensure_session_exists(resolved: &ResolvedProject, session_id: &str) -> Result<(), DaemonError> {
    let store = SessionStore::new(resolved.data_dir.clone());
    store
        .load_session(session_id)
        .map(|_| ())
        .map_err(|err| DaemonError::Internal(err.to_string()))
}

fn normalize_permission_mode(raw: String) -> Result<String, DaemonError> {
    let trimmed = raw.trim();
    crate::permissions::PermissionMode::parse(trimmed)
        .map(|mode| mode.as_str().to_string())
        .ok_or_else(|| {
            DaemonError::Internal(
                "permission_mode must be one of: read-only, workspace-write, danger-full-access"
                    .to_string(),
            )
        })
}

fn set_remote_permission_mode(
    resolved: &ResolvedProject,
    mode: &str,
    action: &str,
) -> Result<Value, DaemonError> {
    let result = crate::config::config_set(
        resolved,
        "permission_mode",
        mode,
        crate::config::ConfigScope::Private,
    )
    .map_err(|err| DaemonError::Internal(err.to_string()))?;
    let payload = json!({
        "schema_version": "1",
        "action": action,
        "source": "remote",
        "permission_mode": mode,
        "scope": "private",
        "result": result
    });
    append_remote_event(
        resolved,
        "permission_mode",
        "permission_mode",
        mode,
        None,
        payload.clone(),
    )?;
    Ok(payload)
}

fn ensure_remote_ready(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    action: &str,
) -> Result<(), DaemonError> {
    let report = remote::status(
        context.state_home.as_path(),
        resolved,
        active_session_id(context, resolved)?,
    )
    .map_err(remote_internal)?;
    if report.remote_ready {
        return Ok(());
    }
    let rejection = match report.revocation_reason.as_deref() {
        Some("lease_expired") => remote::lease_expired_rejection(action),
        Some("revoked") => remote::binding_revoked_rejection(action),
        _ => remote::attach_rejection(action),
    };
    Err(remote_to_daemon_error(
        &format!("remote {action}"),
        resolved,
        RemoteError::ActionRejected(rejection),
    ))
}

fn append_remote_event(
    resolved: &ResolvedProject,
    event_name: &str,
    object_kind: &str,
    object_id: &str,
    session_id: Option<&str>,
    payload: Value,
) -> Result<(), DaemonError> {
    append_remote_event_with_phase(
        resolved,
        event_name,
        "terminal",
        Some("succeeded"),
        object_kind,
        object_id,
        session_id,
        payload,
    )
}

fn append_remote_event_with_phase(
    resolved: &ResolvedProject,
    event_name: &str,
    phase: &str,
    terminal_outcome: Option<&str>,
    object_kind: &str,
    object_id: &str,
    session_id: Option<&str>,
    payload: Value,
) -> Result<(), DaemonError> {
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let _guard =
        lock_event_log(&event_log_path).map_err(|err| DaemonError::Internal(err.to_string()))?;
    let next_seq = next_remote_event_seq(&event_log_path)?;
    append_event_unlocked(
        &event_log_path,
        &KernelEventEnvelope {
            event_id: format!("evt_{event_name}_{next_seq}"),
            seq: next_seq,
            event_name: event_name.to_string(),
            phase: phase.to_string(),
            terminal_outcome: terminal_outcome.map(ToString::to_string),
            object_kind: object_kind.to_string(),
            object_id: object_id.to_string(),
            session_id: session_id.map(ToString::to_string),
            project_id: Some(resolved.project_id.clone()),
            timestamp: timestamp_string(),
            payload,
        },
    )
    .map_err(|err| DaemonError::Internal(err.to_string()))?;
    update_remote_event_seq_cache_len(&event_log_path, next_seq)?;
    Ok(())
}

fn next_remote_event_seq(event_log_path: &Path) -> Result<u64, DaemonError> {
    let file_len = fs::metadata(event_log_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut cache = EVENT_SEQ_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| {
            DaemonError::Internal("remote event sequence cache lock poisoned".to_string())
        })?;
    let last_seq = match cache.get(event_log_path) {
        Some(cached) if cached.file_len == file_len => cached.seq,
        None => read_events_from(event_log_path)
            .map_err(|err| DaemonError::Internal(err.to_string()))?
            .last()
            .map(|event| event.seq)
            .unwrap_or(0),
        Some(_) => read_events_from(event_log_path)
            .map_err(|err| DaemonError::Internal(err.to_string()))?
            .last()
            .map(|event| event.seq)
            .unwrap_or(0),
    };
    let next_seq = last_seq + 1;
    cache.insert(
        event_log_path.to_path_buf(),
        CachedEventSeq {
            seq: next_seq,
            file_len,
        },
    );
    Ok(next_seq)
}

fn update_remote_event_seq_cache_len(event_log_path: &Path, seq: u64) -> Result<(), DaemonError> {
    let file_len = fs::metadata(event_log_path)
        .map_err(|err| DaemonError::Internal(err.to_string()))?
        .len();
    EVENT_SEQ_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| {
            DaemonError::Internal("remote event sequence cache lock poisoned".to_string())
        })?
        .insert(
            event_log_path.to_path_buf(),
            CachedEventSeq { seq, file_len },
        );
    Ok(())
}

fn render_event_stream(
    resolved: &ResolvedProject,
    after_seq: u64,
    wait_ms: u64,
) -> Result<String, DaemonError> {
    let started_at = Instant::now();
    loop {
        let (stream, has_events) = render_event_stream_once(resolved, after_seq, wait_ms)?;
        if has_events || wait_ms == 0 || started_at.elapsed() >= Duration::from_millis(wait_ms) {
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn render_event_stream_once(
    resolved: &ResolvedProject,
    after_seq: u64,
    wait_ms: u64,
) -> Result<(String, bool), DaemonError> {
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    let events =
        read_events_from(&event_log_path).map_err(|err| DaemonError::Internal(err.to_string()))?;
    let mut stream = String::new();
    let mut has_events = false;
    for event in events.into_iter().filter(|event| event.seq > after_seq) {
        has_events = true;
        stream.push_str("event: remote-event\n");
        stream.push_str(&format!("id: {}\n", event.seq));
        stream.push_str("data: ");
        stream.push_str(&serde_json::to_string(&event)?);
        stream.push_str("\n\n");
    }
    if stream.is_empty() {
        stream.push_str("event: remote-heartbeat\n");
        stream.push_str("data: ");
        stream.push_str(&serde_json::to_string(&json!({
            "ok": true,
            "after_seq": after_seq,
            "wait_ms": wait_ms
        }))?);
        stream.push_str("\n\n");
    }
    Ok((stream, has_events))
}

fn artifact_preview(path: &str) -> Result<Value, DaemonError> {
    const MAX_PREVIEW_BYTES: usize = 64 * 1024;
    let path = PathBuf::from(path);
    if !path.exists() {
        return Ok(json!({
            "schema_version": "artifact_preview.v1",
            "preview_kind": "missing",
            "syntax": "plain_text",
            "path": path.display().to_string(),
            "content": "",
            "truncated": false,
            "size_bytes": null
        }));
    }
    if path.is_dir() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
        entries.sort();
        return Ok(json!({
            "schema_version": "artifact_preview.v1",
            "preview_kind": "directory",
            "syntax": "plain_text",
            "path": path.display().to_string(),
            "content": entries.join("\n"),
            "truncated": false,
            "size_bytes": null
        }));
    }
    let bytes = fs::read(&path)?;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    let visible = if truncated {
        &bytes[..MAX_PREVIEW_BYTES]
    } else {
        bytes.as_slice()
    };
    let content = String::from_utf8_lossy(visible).to_string();
    Ok(json!({
        "schema_version": "artifact_preview.v1",
        "preview_kind": "file",
        "syntax": syntax_for_artifact(&path, &content),
        "path": path.display().to_string(),
        "content": content,
        "truncated": truncated,
        "size_bytes": bytes.len()
    }))
}

fn result_panel_payload(resolved: &ResolvedProject) -> Result<Value, DaemonError> {
    let targets = [
        ("reports", resolved.workspace_root.join("reports")),
        (".pmcli/reports", resolved.data_dir.join("reports")),
        (".pmcli/branches", resolved.data_dir.join("branches")),
        (
            ".pmcli/research/results",
            resolved.data_dir.join("research").join("results"),
        ),
    ];
    let targets = targets
        .into_iter()
        .map(|(target, path)| {
            let preview = artifact_preview(&path.display().to_string())?;
            Ok(json!({
                "target": target,
                "path": path.display().to_string(),
                "status": if path.exists() { "available" } else { "missing" },
                "entry_count": count_result_entries(&path),
                "preview": preview
            }))
        })
        .collect::<Result<Vec<_>, DaemonError>>()?;
    Ok(json!({
        "schema_version": "remote_result_panel.v1",
        "result_contract": "test_result_panel",
        "authority": "artifact_registry_read_only",
        "source": "governed_artifact_and_workspace_report_paths",
        "targets": targets
    }))
}

fn count_result_entries(path: &Path) -> usize {
    if path.is_file() {
        return 1;
    }
    fs::read_dir(path)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

fn persist_conversation_attachments(
    resolved: &ResolvedProject,
    session_id: &str,
    attachments: Vec<AttachmentUploadItem>,
) -> Result<Value, DaemonError> {
    const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
    if attachments.is_empty() {
        return Err(DaemonError::Internal(
            "session attachments require at least one attachment".to_string(),
        ));
    }
    let target_dir = resolved
        .data_dir
        .join("remote")
        .join("attachments")
        .join(safe_path_component(session_id));
    fs::create_dir_all(&target_dir)?;
    let mut saved = Vec::new();
    for (index, attachment) in attachments.into_iter().enumerate() {
        let name = required(Some(attachment.name), "name")?;
        let mime_type = required(Some(attachment.mime_type), "mime_type")?;
        if !mime_type.starts_with("image/") {
            return Err(DaemonError::Internal(format!(
                "unsupported attachment mime type: {mime_type}"
            )));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(attachment.data_base64.as_bytes())
            .map_err(|err| DaemonError::Internal(format!("attachment base64 invalid: {err}")))?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(DaemonError::Internal(format!(
                "attachment exceeds {MAX_ATTACHMENT_BYTES} bytes: {name}"
            )));
        }
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        let digest = format!("{:x}", hasher.finalize());
        let filename = format!(
            "{}_{index}_{}_{}",
            timestamp_string(),
            &digest[..12],
            safe_path_component(&name)
        );
        let path = target_dir.join(filename);
        fs::write(&path, &bytes)?;
        saved.push(json!({
            "attachment_id": format!("att_{}", &digest[..16]),
            "name": name,
            "mime_type": mime_type,
            "size_bytes": bytes.len(),
            "artifact_ref": path.display().to_string(),
            "sha1": digest,
            "status": "stored"
        }));
    }
    Ok(json!({
        "schema_version": "remote_session_attachments.v1",
        "attachment_contract": "conversation_image_attachment",
        "authority": "remote_lease_and_project_artifact_store",
        "session_id": session_id,
        "attachments": saved,
        "created_at": timestamp_string()
    }))
}

fn safe_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim_matches('_');
    if sanitized.is_empty() {
        "attachment.bin".to_string()
    } else {
        sanitized.to_string()
    }
}

fn syntax_for_artifact(path: &Path, content: &str) -> &'static str {
    if path.extension().and_then(|value| value.to_str()) == Some("diff")
        || content.starts_with("diff --git ")
        || content.starts_with("--- ")
    {
        "unified_diff"
    } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
        "json"
    } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
        "markdown"
    } else {
        "plain_text"
    }
}

fn mark_daemon_running(
    context: &DaemonContext,
    report: &RemoteStatusReport,
) -> Result<(), DaemonError> {
    let path = context.state_home.join("remote").join("daemon_state.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "1",
            "machine_id": report.machine_identity.machine_id,
            "daemon_state": "running",
            "last_shutdown_state": "unknown",
            "updated_at": timestamp_string()
        }))?,
    )?;
    Ok(())
}

fn resolve_permission_from_remote(
    resolved: &ResolvedProject,
    request_id: &str,
    decision: &str,
) -> Result<Value, DaemonError> {
    let decision = match decision {
        "approve" | "approved" => "approved",
        "deny" | "denied" => "denied",
        other => {
            return Err(DaemonError::Internal(format!(
                "unsupported remote permission decision: {other}"
            )))
        }
    };
    let events = read_events_from(&resolved.data_dir.join("events").join("events.jsonl"))
        .map_err(|err| DaemonError::Internal(err.to_string()))?;
    if events.iter().any(|event| {
        event.event_name == "permission"
            && event.phase == "terminal"
            && event.object_id == request_id
    }) {
        return Err(DaemonError::Internal(format!(
            "permission request is already resolved: {request_id}"
        )));
    }
    let request = events
        .iter()
        .find(|event| {
            event.event_name == "permission"
                && event.phase == "start"
                && event.object_id == request_id
        })
        .ok_or_else(|| {
            DaemonError::Internal(format!("unknown permission request: {request_id}"))
        })?;
    if permission_request_is_expired(&request.payload) {
        let payload = permission_decision_payload(request_id, "expired", &request.payload, false);
        append_remote_event(
            resolved,
            "permission",
            "permission",
            request_id,
            request.session_id.as_deref(),
            payload.clone(),
        )?;
        return Err(DaemonError::Envelope {
            status: 400,
            envelope: json!({
                "ok": false,
                "command": "remote permission",
                "project_id": resolved.project_id,
                "session_id": request.session_id,
                "error": {
                    "code": "permission_request_expired",
                    "message": format!("permission request expired: {request_id}"),
                    "hint": "Re-run the action to create a fresh permission request.",
                    "retryable": false
                },
                "data": payload
            }),
        });
    }
    let approved = decision == "approved";
    let payload = permission_decision_payload(request_id, decision, &request.payload, approved);
    append_remote_event(
        resolved,
        "permission",
        "permission",
        request_id,
        request.session_id.as_deref(),
        payload.clone(),
    )?;
    Ok(payload)
}

fn permission_request_is_expired(request: &Value) -> bool {
    request
        .get("expires_at_ms")
        .and_then(Value::as_u64)
        .map(|expires_at| timestamp_millis() >= u128::from(expires_at))
        .unwrap_or(false)
}

fn permission_decision_payload(
    request_id: &str,
    decision: &str,
    request: &Value,
    approved: bool,
) -> Value {
    let tool_name = request.get("tool_name").and_then(Value::as_str);
    let permission_mode = request.get("permission_mode").and_then(Value::as_str);
    json!({
        "request_id": request_id,
        "decision": decision,
        "status": "resolved",
        "session_id": request.get("session_id").and_then(Value::as_str),
        "turn_id": request.get("turn_id").and_then(Value::as_str),
        "tool_name": tool_name,
        "permission_mode": permission_mode,
        "resolved_at": timestamp_string(),
        "source": "remote",
        "trace": {
            "request_id": request_id,
            "tool_name": tool_name.unwrap_or("unknown"),
            "action": "permission_decision",
            "requested_path": request.get("target_path").and_then(Value::as_str),
            "required_mode": "workspace-write",
            "current_mode": permission_mode.unwrap_or("read-only"),
            "allowlist_match": false,
            "workspace_boundary_ok": true,
            "branch_boundary_ok": true,
            "destructive": false,
            "approved": approved,
            "reason": match decision {
                "approved" => "remote operator approved pending permission",
                "denied" => "remote operator denied pending permission",
                "expired" => "permission request expired before remote resolution",
                _ => "remote operator resolved pending permission"
            }
        }
    })
}

fn publish_project_checkpoint_for_daemon(
    resolved: &ResolvedProject,
    touched_families: &[&str],
) -> Result<(), DaemonError> {
    let checkpoint_path = resolved.data_dir.join("project_state.json");
    let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
    for attempt in 0..8 {
        let committed = if checkpoint_path.exists() {
            KernelStateBundle::load_from(&checkpoint_path)
                .map_err(|err| DaemonError::Internal(err.to_string()))?
        } else {
            KernelStateBundle::default()
        };
        let outcome = publish_checkpoint(
            &checkpoint_path,
            &event_log_path,
            CheckpointPublishPlan {
                base_seq_cursor: committed.seq_cursor,
                base_checkpoint_epoch: committed.checkpoint_epoch,
                touched_families: touched_families
                    .iter()
                    .map(|family| (*family).to_string())
                    .collect(),
            },
        );
        match outcome {
            Ok(_) => return Ok(()),
            Err(ReducerError::WriteLaneBusy(_)) if attempt < 7 => {
                thread::sleep(Duration::from_millis(8 * (attempt + 1)));
            }
            Err(err) => return Err(DaemonError::Internal(err.to_string())),
        }
    }
    Err(DaemonError::Internal(
        "checkpoint write lane stayed busy after remote retry budget".to_string(),
    ))
}

fn execute_tui_action(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    action_id: &str,
    input: TuiActionRequest,
) -> Result<DaemonResponse, DaemonError> {
    match action_id {
        "terminal_attach" => {
            ensure_remote_ready(context, resolved, "terminal_attach")?;
            let result = host_surface::terminal_attach(
                &context.state_home,
                resolved,
                active_session_id(context, resolved)?,
            )
            .map_err(host_surface_internal)?;
            append_remote_event(
                resolved,
                "remote_terminal_attach",
                "terminal",
                &result.terminal.terminal_id,
                result.terminal.active_session_id.as_deref(),
                serde_json::to_value(&result)?,
            )?;
            publish_project_checkpoint_for_daemon(resolved, &["remote", "terminal"])?;
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "result": result
                }),
            )
        }
        "terminal_replay" => {
            ensure_remote_ready(context, resolved, "terminal_replay")?;
            let result = host_surface::terminal_replay(
                &context.state_home,
                resolved,
                active_session_id(context, resolved)?,
            )
            .map_err(host_surface_internal)?;
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "result": result
                }),
            )
        }
        "switch_session" => {
            if let Some(session_id) = input.session_id {
                ensure_remote_ready(context, resolved, "session_resume")?;
                ensure_session_exists(resolved, &session_id)?;
                ProjectRegistry::new(context.state_home.join("registry"))
                    .touch_project(&resolved.project_id, Some(session_id.clone()))
                    .map_err(|err| DaemonError::Internal(err.to_string()))?;
                publish_project_checkpoint_for_daemon(resolved, &["session", "remote"])?;
                return json_success(
                    "remote tui action",
                    resolved,
                    Some(session_id.clone()),
                    json!({
                        "schema_version": "remote_tui_action_result.v1",
                        "action_id": action_id,
                        "session_id": session_id
                    }),
                );
            }
            let sessions = SessionStore::new(resolved.data_dir.clone())
                .list_sessions()
                .map_err(|err| DaemonError::Internal(err.to_string()))?;
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "sessions": sessions
                }),
            )
        }
        "inspect_permissions"
        | "inspect_memory"
        | "open_research"
        | "open_research_board"
        | "inspect_recovery_governance" => {
            let report = host_surface::status(
                &context.state_home,
                resolved,
                active_session_id(context, resolved)?,
            )
            .map_err(host_surface_internal)?;
            let result = match action_id {
                "inspect_permissions" => json!(report.projection.permissions),
                "inspect_memory" => json!(report.projection.memory),
                "open_research" => json!(report.projection.research),
                "open_research_board" => json!(report.projection.research.board),
                "inspect_recovery_governance" => {
                    json!(report.projection.research.recovery_governance)
                }
                _ => unreachable!(),
            };
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "result": result
                }),
            )
        }
        "retry_routine_trigger" => {
            let trigger_id = required(input.trigger_id, "trigger_id")?;
            let retry = routines::retry_trigger(
                &resolved.data_dir,
                &resolved.workspace_root,
                routines::RoutineRetryRequest {
                    trigger_id: trigger_id.clone(),
                    source: "remote-mobile-retry".to_string(),
                    stale_after_sec: 900,
                    max_retry_attempts: routines::DEFAULT_MAX_RETRY_ATTEMPTS,
                    retry_backoff_ms: routines::DEFAULT_RETRY_BACKOFF_MS,
                },
            )
            .map_err(|err| DaemonError::Internal(err.to_string()))?;
            append_remote_event(
                resolved,
                "routine_retry",
                "routine_trigger",
                &trigger_id,
                active_session_id(context, resolved)?.as_deref(),
                serde_json::to_value(&retry)?,
            )?;
            publish_project_checkpoint_for_daemon(resolved, &["routines", "projectops"])?;
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "control_event_kind": "routine_retry",
                    "contract": "routine_retry_budget_and_backoff",
                    "result": retry
                }),
            )
        }
        "decide_research_stage" => {
            let report = host_surface::status(
                &context.state_home,
                resolved,
                active_session_id(context, resolved)?,
            )
            .map_err(host_surface_internal)?;
            let Some(stage_execution_id) = input.stage_execution_id.clone() else {
                return json_success(
                    "remote tui action",
                    resolved,
                    None,
                    json!({
                        "schema_version": "remote_tui_action_result.v1",
                        "action_id": action_id,
                        "result": report.projection.research.stage_decision_policy
                    }),
                );
            };
            let Some(thread_id) = input
                .thread_id
                .clone()
                .or_else(|| report.projection.research.active_thread_id.clone())
            else {
                return json_success(
                    "remote tui action",
                    resolved,
                    None,
                    json!({
                        "schema_version": "remote_tui_action_result.v1",
                        "action_id": action_id,
                        "result": report.projection.research.stage_decision_policy
                    }),
                );
            };
            let operation = input
                .operation
                .clone()
                .unwrap_or_else(|| "advance".to_string());
            let decision = input
                .decision
                .clone()
                .unwrap_or_else(|| "approve".to_string());
            let result = crate::research::decide(
                &resolved.data_dir,
                &resolved.workspace_root,
                &resolved.project_id,
                crate::research::ResearchDecisionRequest {
                    thread_id,
                    operation,
                    decision,
                    reason: input.message.clone().unwrap_or_else(|| {
                        "operator handled projected research stage decision".to_string()
                    }),
                    evidence_refs: vec![stage_execution_id],
                },
            )
            .map_err(|err| DaemonError::Internal(err.to_string()))?;
            append_remote_event(
                resolved,
                "research_stage_decision",
                "research_decision",
                &result.decision_id,
                active_session_id(context, resolved)?.as_deref(),
                serde_json::to_value(&result)?,
            )?;
            publish_project_checkpoint_for_daemon(resolved, &["research"])?;
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "control_event_kind": "research_stage_decision",
                    "contract": "research_decide_existing_stage_gate",
                    "result": result
                }),
            )
        }
        "advance_research_loop" => {
            let routine_run_due = routines::run_due(
                &resolved.data_dir,
                &resolved.workspace_root,
                routines::RoutineRunDueRequest {
                    trigger_kind: None,
                    limit: None,
                    stale_after_sec: 900,
                    max_retry_attempts: routines::DEFAULT_MAX_RETRY_ATTEMPTS,
                    retry_backoff_ms: routines::DEFAULT_RETRY_BACKOFF_MS,
                },
            )
            .map_err(|err| DaemonError::Internal(err.to_string()))?;
            append_remote_event(
                resolved,
                "routine_run_due",
                "routine_run_due",
                "remote_goal_tick",
                active_session_id(context, resolved)?.as_deref(),
                serde_json::to_value(&routine_run_due)?,
            )?;
            let goal_advance = crate::goals::advance(
                &resolved.data_dir,
                &resolved.workspace_root,
                &resolved.project_id,
            )
            .map_err(|err| DaemonError::Internal(err.to_string()))?;
            let recovery_governance = routine_run_due.recovery_governance.clone();
            let loop_closure = crate::goals::loop_closure_projection(
                &goal_advance.task_pool,
                Some(&recovery_governance),
                goal_advance.dispatches.len(),
                goal_advance.acceptance.is_some(),
            );
            let session_id = active_session_id(context, resolved)?;
            append_remote_event(
                resolved,
                "goal_advance",
                "goal_run",
                &goal_advance.run.run_id,
                session_id.as_deref(),
                serde_json::to_value(&goal_advance)?,
            )?;
            for dispatch in &goal_advance.dispatches {
                append_remote_event(
                    resolved,
                    "goal_task_dispatch",
                    "agent_task_packet",
                    &dispatch.agent_id,
                    session_id.as_deref(),
                    json!({
                        "schema_version": "goal_task_dispatch_event.v1",
                        "project_id": goal_advance.project_id,
                        "run_id": goal_advance.run.run_id,
                        "dispatch": dispatch
                    }),
                )?;
            }
            if let Some(acceptance) = goal_advance.acceptance.as_ref() {
                append_remote_event(
                    resolved,
                    "goal_task_acceptance",
                    "agent_output_manifest",
                    &acceptance.agent_id,
                    session_id.as_deref(),
                    json!({
                        "schema_version": "goal_task_acceptance_event.v1",
                        "project_id": goal_advance.project_id,
                        "run_id": goal_advance.run.run_id,
                        "acceptance": acceptance
                    }),
                )?;
            }
            if let Some(stage_decision) = goal_advance.stage_decision.as_ref() {
                append_remote_event(
                    resolved,
                    "goal_research_stage_decision",
                    "research_decision",
                    &stage_decision.decision_id,
                    session_id.as_deref(),
                    json!({
                        "schema_version": "goal_research_stage_decision_event.v1",
                        "project_id": goal_advance.project_id,
                        "run_id": goal_advance.run.run_id,
                        "stage_decision": stage_decision
                    }),
                )?;
            }
            publish_project_checkpoint_for_daemon(
                resolved,
                &[
                    "routines",
                    "mission_frame",
                    "orchestration",
                    "agents",
                    "projectops",
                    "research",
                ],
            )?;
            json_success(
                "remote tui action",
                resolved,
                session_id,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "control_event_kind": "goal_tick",
                    "contract": "goal_tick_existing_authorities",
                    "result": {
                        "schema_version": "goal_tick_result.v1",
                        "status": if routine_run_due.failed_count == 0 { "advanced" } else { "advanced_with_routine_failures" },
                        "project_id": resolved.project_id,
                        "triggered_count": routine_run_due.triggered_count,
                        "dispatch_count": goal_advance.dispatches.len(),
                        "accepted": goal_advance.acceptance.is_some(),
                        "next_recommended_action": goal_advance.next_recommended_action,
                        "recovery_governance": recovery_governance,
                        "loop_closure": loop_closure,
                        "routine_run_due": routine_run_due,
                        "goal_advance": goal_advance
                    }
                }),
            )
        }
        "interrupt_turn" => {
            ensure_remote_ready(context, resolved, "notify")?;
            let active_session = active_session_id(context, resolved)?;
            if let Some(session_id) = active_session.as_deref() {
                if let Some(cancellation) = active_runtime_turn_registry()
                    .interrupt_session_turn(&resolved.project_id, session_id)
                {
                    append_remote_event_with_phase(
                        resolved,
                        "remote_interrupt_requested",
                        "control",
                        None,
                        "control",
                        "runtime_cancellation_requested",
                        Some(session_id),
                        json!({
                            "schema_version": "remote_interrupt_requested.v1",
                            "action_id": action_id,
                            "control_event_kind": "runtime_cancellation_requested",
                            "contract": "direct_runtime_cancellation",
                            "project_id": cancellation.project_id,
                            "session_id": cancellation.session_id,
                            "turn_id": cancellation.turn_id,
                            "cancelled": cancellation.cancelled,
                        }),
                    )?;
                    return json_success(
                        "remote tui action",
                        resolved,
                        Some(session_id.to_string()),
                        json!({
                            "schema_version": "remote_tui_action_result.v1",
                            "action_id": action_id,
                            "control_event_kind": "runtime_cancellation_requested",
                            "contract": "direct_runtime_cancellation",
                            "result": {
                                "schema_version": "runtime_cancellation_result.v1",
                                "project_id": cancellation.project_id,
                                "session_id": cancellation.session_id,
                                "turn_id": cancellation.turn_id,
                                "cancelled": cancellation.cancelled
                            }
                        }),
                    );
                }
            }
            let result = remote::notify(
                &context.state_home,
                resolved,
                "interrupt_requested",
                input
                    .message
                    .as_deref()
                    .unwrap_or("interrupt requested from product command"),
            )
            .map_err(|err| remote_to_daemon_error("remote tui action", resolved, err))?;
            append_remote_event_with_phase(
                resolved,
                "remote_interrupt_requested",
                "control",
                None,
                "control",
                "interrupt_requested",
                active_session_id(context, resolved)?.as_deref(),
                json!({
                    "schema_version": "remote_interrupt_requested.v1",
                    "action_id": action_id,
                    "control_event_kind": "interrupt_requested",
                    "contract": "notification_backed_interrupt_request",
                    "notification": result.notification,
                    "receipts": result.receipts,
                }),
            )?;
            json_success(
                "remote tui action",
                resolved,
                None,
                json!({
                    "schema_version": "remote_tui_action_result.v1",
                    "action_id": action_id,
                    "control_event_kind": "interrupt_requested",
                    "contract": "notification_backed_interrupt_request",
                    "result": result
                }),
            )
        }
        other => Err(DaemonError::Envelope {
            status: 400,
            envelope: json!({
                "ok": false,
                "command": "remote tui action",
                "project_id": resolved.project_id,
                "error": {
                    "code": "unsupported_tui_action",
                    "message": format!("unsupported tui action id: {other}"),
                    "hint": "Use /api/tui/actions to inspect supported action ids and gates.",
                    "retryable": false
                }
            }),
        }),
    }
}

fn terminal_websocket_route(
    context: &DaemonContext,
    request: &DaemonRequest,
    query: &str,
) -> Result<DaemonResponse, DaemonError> {
    let query = parse_query(query);
    let cwd = query.get("cwd").map(PathBuf::from);
    let ticket = query.get("ticket").cloned();
    let resolved = resolve_project(context, cwd.as_deref())?;
    ensure_remote_ready(context, &resolved, "terminal_websocket")?;
    let _terminal = attached_terminal_projection(&resolved)?;
    let Some(ticket) = ticket else {
        return Err(terminal_ticket_error(
            "terminal websocket requires a one-time ticket",
        ));
    };
    validate_terminal_ws_ticket(context, &ticket, &resolved)?;
    if !is_websocket_upgrade(request) {
        return json_response_with_headers(
            426,
            &json!({
                "ok": false,
                "command": "remote terminal websocket",
                "project_id": resolved.project_id,
                "error": {
                    "code": "terminal_websocket_upgrade_required",
                    "message": "terminal bridge requires an HTTP Upgrade websocket request",
                    "hint": "Use ws://<tailscale-ip>:<port>/api/terminal/ws from the mobile terminal lane.",
                    "retryable": false
                }
            }),
            vec![("Upgrade".to_string(), "websocket".to_string())],
        );
    }
    let accept = websocket_accept_key(request)?;
    consume_terminal_ws_ticket(context, &ticket, &resolved)?;
    Ok(DaemonResponse {
        status: 101,
        content_type: "application/octet-stream",
        headers: vec![
            ("Upgrade".to_string(), "websocket".to_string()),
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Sec-WebSocket-Accept".to_string(), accept),
        ],
        body: Vec::new(),
    })
}

fn issue_terminal_ws_ticket(
    context: &DaemonContext,
    resolved: &ResolvedProject,
) -> Result<Value, DaemonError> {
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| DaemonError::Internal(err.to_string()))?
        .as_nanos();
    let mut hasher = Sha1::new();
    hasher.update(resolved.project_id.as_bytes());
    hasher.update(resolved.workspace_root.as_os_str().as_bytes());
    hasher.update(issued_at.to_string().as_bytes());
    if let Some(token) = context.control_token.as_deref() {
        hasher.update(token.as_bytes());
    }
    let ticket = format!("twst_{:x}", hasher.finalize());
    let ttl = Duration::from_secs(60);
    let now = Instant::now();
    let mut tickets = context.terminal_ws_tickets.lock().map_err(|_| {
        DaemonError::Internal("terminal websocket ticket lock poisoned".to_string())
    })?;
    tickets.retain(|_, record| !record.consumed && now <= record.expires_at);
    tickets.insert(
        ticket.clone(),
        TerminalWsTicket {
            cwd: resolved.workspace_root.display().to_string(),
            expires_at: now + ttl,
            consumed: false,
        },
    );
    Ok(json!({
        "schema_version": "remote_terminal_ws_ticket.v1",
        "ticket": ticket,
        "expires_in_ms": ttl.as_millis(),
        "auth_model": "one_time_ticket"
    }))
}

fn validate_terminal_ws_ticket(
    context: &DaemonContext,
    ticket: &str,
    resolved: &ResolvedProject,
) -> Result<(), DaemonError> {
    let tickets = context.terminal_ws_tickets.lock().map_err(|_| {
        DaemonError::Internal("terminal websocket ticket lock poisoned".to_string())
    })?;
    let Some(record) = tickets.get(ticket) else {
        return Err(terminal_ticket_error(
            "terminal websocket ticket is missing or unknown",
        ));
    };
    if record.consumed {
        return Err(terminal_ticket_error(
            "terminal websocket ticket has already been used",
        ));
    }
    if Instant::now() > record.expires_at {
        return Err(terminal_ticket_error(
            "terminal websocket ticket has expired",
        ));
    }
    if record.cwd != resolved.workspace_root.display().to_string() {
        return Err(terminal_ticket_error(
            "terminal websocket ticket cwd does not match",
        ));
    }
    Ok(())
}

fn consume_terminal_ws_ticket(
    context: &DaemonContext,
    ticket: &str,
    resolved: &ResolvedProject,
) -> Result<(), DaemonError> {
    let mut tickets = context.terminal_ws_tickets.lock().map_err(|_| {
        DaemonError::Internal("terminal websocket ticket lock poisoned".to_string())
    })?;
    let Some(record) = tickets.remove(ticket) else {
        return Err(terminal_ticket_error(
            "terminal websocket ticket is missing or unknown",
        ));
    };
    if record.consumed {
        return Err(terminal_ticket_error(
            "terminal websocket ticket has already been used",
        ));
    }
    if Instant::now() > record.expires_at {
        return Err(terminal_ticket_error(
            "terminal websocket ticket has expired",
        ));
    }
    if record.cwd != resolved.workspace_root.display().to_string() {
        return Err(terminal_ticket_error(
            "terminal websocket ticket cwd does not match",
        ));
    }
    Ok(())
}

fn terminal_ticket_error(message: &str) -> DaemonError {
    DaemonError::Envelope {
        status: 401,
        envelope: json!({
            "ok": false,
            "command": "remote terminal websocket",
            "error": {
                "code": "terminal_websocket_ticket_required",
                "message": message,
                "hint": "Create a one-time websocket ticket with POST /api/terminal/ws-ticket before connecting.",
                "retryable": true
            }
        }),
    }
}

#[derive(Debug, Clone)]
struct PtySession {
    master_fd: i32,
    child_pid: libc::pid_t,
    terminal_id: String,
    active_session_id: Option<String>,
}

fn start_terminal_pty(resolved: &ResolvedProject) -> Result<PtySession, DaemonError> {
    let terminal = attached_terminal_projection(resolved)?;
    let cwd = CString::new(resolved.workspace_root.as_os_str().as_bytes())
        .map_err(|err| DaemonError::Internal(format!("workspace path contains NUL: {err}")))?;
    let exe_path = std::env::current_exe()
        .map_err(DaemonError::Io)
        .and_then(|path| {
            CString::new(path.as_os_str().as_bytes()).map_err(|err| {
                DaemonError::Internal(format!("current executable path contains NUL: {err}"))
            })
        })?;
    let arg0 = CString::new("research-cli").expect("literal cli arg has no nul");
    let arg1 = CString::new("tui").expect("literal cli arg has no nul");
    let arg2 = CString::new("launch").expect("literal cli arg has no nul");
    let arg3 = CString::new("--fullscreen").expect("literal cli arg has no nul");
    let term_name = CString::new("TERM").expect("literal env name has no nul");
    let term_value = CString::new("xterm-256color").expect("literal env value has no nul");
    let render_child_name =
        CString::new("RESEARCH_CLI_TERMINAL_RENDER_CHILD").expect("literal env name has no nul");
    let render_child_value = CString::new("1").expect("literal env value has no nul");
    let mut master_fd = -1;
    let winsize = libc::winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pid = unsafe {
        libc::forkpty(
            &mut master_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &winsize,
        )
    };
    if pid < 0 {
        return Err(DaemonError::Io(std::io::Error::last_os_error()));
    }
    if pid == 0 {
        unsafe {
            libc::chdir(cwd.as_ptr());
            libc::setenv(term_name.as_ptr(), term_value.as_ptr(), 1);
            libc::setenv(render_child_name.as_ptr(), render_child_value.as_ptr(), 1);
            libc::execl(
                exe_path.as_ptr(),
                arg0.as_ptr(),
                arg1.as_ptr(),
                arg2.as_ptr(),
                arg3.as_ptr(),
                std::ptr::null::<libc::c_char>(),
            );
            libc::_exit(127);
        }
    }
    Ok(PtySession {
        master_fd,
        child_pid: pid,
        terminal_id: terminal.terminal_id,
        active_session_id: terminal.active_session_id,
    })
}

fn spawn_pty_output_reader(
    writer: Arc<Mutex<TcpStream>>,
    resolved: ResolvedProject,
    pty: PtySession,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk_index = 0_u64;
        loop {
            let mut buffer = [0_u8; 4096];
            let n = unsafe {
                libc::read(
                    pty.master_fd,
                    buffer.as_mut_ptr().cast::<libc::c_void>(),
                    buffer.len(),
                )
            };
            if n > 0 {
                chunk_index += 1;
                let bytes = &buffer[..n as usize];
                let payload = json!({
                    "schema_version": "remote_terminal_pty_output.v1",
                    "terminal_id": pty.terminal_id,
                    "active_session_id": pty.active_session_id,
                    "transport": "tailscale_private_overlay",
                    "byte_bridge_state": "host_pty_byte_stream",
                    "encoding": "base64",
                    "chunk_index": chunk_index,
                    "bytes": bytes.len(),
                    "chunk_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "text": String::from_utf8_lossy(bytes),
                    "created_at": timestamp_string()
                });
                let frame = json!({
                    "ok": true,
                    "type": "pty_output",
                    "data": payload
                });
                if let Err(err) = append_remote_event(
                    &resolved,
                    "remote_terminal_output",
                    "terminal",
                    pty.terminal_id.as_str(),
                    pty.active_session_id.as_deref(),
                    payload.clone(),
                )
                .and_then(|_| {
                    publish_project_checkpoint_for_daemon(&resolved, &["remote", "terminal"])
                }) {
                    let frame = json!({
                        "ok": true,
                        "type": "pty_output_persistence_degraded",
                        "data": payload,
                        "warning": {
                            "code": "terminal_output_persistence_failed",
                            "message": err.to_string()
                        }
                    });
                    let _ = write_websocket_json(&writer, &frame);
                }
                if write_websocket_json(&writer, &frame).is_err() {
                    break;
                }
            } else if n == 0 {
                break;
            } else {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
        }
    })
}

fn write_websocket_json(writer: &Arc<Mutex<TcpStream>>, value: &Value) -> Result<(), DaemonError> {
    let mut stream = writer
        .lock()
        .map_err(|err| DaemonError::Internal(format!("websocket writer lock poisoned: {err}")))?;
    write_websocket_text_frame(&mut stream, &serde_json::to_string(value)?)
}

fn write_pty_bytes(fd: i32, bytes: &[u8]) -> Result<(), DaemonError> {
    let mut written = 0;
    while written < bytes.len() {
        let n = unsafe {
            libc::write(
                fd,
                bytes[written..].as_ptr().cast::<libc::c_void>(),
                bytes.len() - written,
            )
        };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(DaemonError::Io(err));
        }
        written += n as usize;
    }
    Ok(())
}

fn resize_pty(fd: i32, cols: u16, rows: u16) -> Result<(), DaemonError> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) };
    if rc != 0 {
        return Err(DaemonError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn signal_pty(pty: &PtySession, signal: &str) -> Result<(), DaemonError> {
    match signal.trim() {
        "interrupt" => write_pty_bytes(pty.master_fd, b"\x03"),
        "eof" => write_pty_bytes(pty.master_fd, b"\x04"),
        "terminate" => {
            unsafe {
                libc::kill(pty.child_pid, libc::SIGHUP);
            }
            Ok(())
        }
        other => Err(DaemonError::Internal(format!(
            "unsupported terminal signal: {other}"
        ))),
    }
}

fn cleanup_pty(pty: &PtySession) {
    unsafe {
        libc::kill(pty.child_pid, libc::SIGHUP);
        libc::close(pty.master_fd);
        let mut status = 0;
        libc::waitpid(pty.child_pid, &mut status, 0);
    }
}

fn handle_terminal_websocket(
    context: &DaemonContext,
    request: DaemonRequest,
    mut stream: TcpStream,
) -> Result<(), DaemonError> {
    let response =
        match terminal_websocket_route(context, &request, &split_path_query(&request.path).1) {
            Ok(response) => response,
            Err(err) => error_response(err)?,
        };
    if response.status != 101 {
        write_http_response(stream, response)?;
        return Ok(());
    }
    write_http_response(stream.try_clone()?, response)?;

    let query = parse_query(&split_path_query(&request.path).1);
    let cwd = query.get("cwd").map(PathBuf::from);
    let resolved = resolve_project(context, cwd.as_deref())?;
    let pty = start_terminal_pty(&resolved)?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let output_reader = spawn_pty_output_reader(writer.clone(), resolved.clone(), pty.clone());
    stream.set_read_timeout(None)?;
    loop {
        match read_websocket_text_frame(&mut stream) {
            Ok(Some(message)) => {
                let ack =
                    match handle_terminal_websocket_message(context, &resolved, &message, &pty) {
                        Ok(ack) => ack,
                        Err(err) => json!({
                            "ok": false,
                            "type": "error",
                            "error": {
                                "code": "terminal_websocket_message_rejected",
                                "message": err.to_string()
                            }
                        }),
                    };
                write_websocket_json(&writer, &ack)?;
            }
            Ok(None) => break,
            Err(err) => {
                let ack = json!({
                    "ok": false,
                    "type": "error",
                    "error": {
                        "code": "terminal_websocket_frame_invalid",
                        "message": err.to_string()
                    }
                });
                let _ = write_websocket_json(&writer, &ack);
                break;
            }
        }
    }
    cleanup_pty(&pty);
    let _ = output_reader.join();
    Ok(())
}

fn handle_terminal_websocket_message(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    message: &str,
    pty: &PtySession,
) -> Result<Value, DaemonError> {
    let value: Value = serde_json::from_str(message)?;
    let message_type = value.get("type").and_then(Value::as_str).ok_or_else(|| {
        DaemonError::Internal("missing terminal websocket message type".to_string())
    })?;
    let payload =
        match message_type {
            "input" => {
                let data = value.get("data").and_then(Value::as_str).ok_or_else(|| {
                    DaemonError::Internal("missing terminal input data".to_string())
                })?;
                let payload = record_terminal_input_with_bridge_state(
                    context,
                    resolved,
                    data,
                    "accepted_for_host_pty",
                    "host_pty_byte_stream",
                )?;
                write_pty_bytes(pty.master_fd, data.as_bytes())?;
                payload
            }
            "resize" => {
                let cols = value.get("cols").and_then(Value::as_u64).ok_or_else(|| {
                    DaemonError::Internal("missing terminal resize cols".to_string())
                })?;
                let rows = value.get("rows").and_then(Value::as_u64).ok_or_else(|| {
                    DaemonError::Internal("missing terminal resize rows".to_string())
                })?;
                if cols > u16::MAX as u64 || rows > u16::MAX as u64 {
                    return Err(DaemonError::Internal(format!(
                        "terminal resize out of range: {cols}x{rows}"
                    )));
                }
                let payload = record_terminal_resize_with_bridge_state(
                    context,
                    resolved,
                    cols as u16,
                    rows as u16,
                    "accepted_for_host_pty",
                    "host_pty_byte_stream",
                )?;
                resize_pty(pty.master_fd, cols as u16, rows as u16)?;
                payload
            }
            "signal" => {
                let signal = value
                    .get("signal")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DaemonError::Internal("missing terminal signal".to_string()))?;
                let payload = record_terminal_signal_with_bridge_state(
                    context,
                    resolved,
                    signal,
                    "accepted_for_host_pty",
                    "host_pty_byte_stream",
                )?;
                signal_pty(pty, signal)?;
                payload
            }
            other => {
                return Err(DaemonError::Internal(format!(
                    "unsupported terminal websocket message type: {other}"
                )));
            }
        };
    Ok(json!({
        "ok": true,
        "type": format!("{message_type}_ack"),
        "data": payload
    }))
}

fn record_terminal_input(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    data: &str,
) -> Result<Value, DaemonError> {
    record_terminal_input_with_bridge_state(
        context,
        resolved,
        data,
        "queued_for_governed_bridge",
        "contract_ready_not_host_pty",
    )
}

fn record_terminal_input_with_bridge_state(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    data: &str,
    bridge_state: &str,
    byte_bridge_state: &str,
) -> Result<Value, DaemonError> {
    ensure_remote_ready(context, resolved, "terminal_input")?;
    if data.is_empty() {
        return Err(DaemonError::Internal(
            "missing terminal input data".to_string(),
        ));
    }
    let terminal = attached_terminal_projection(resolved)?;
    let payload = json!({
        "schema_version": "remote_terminal_input.v1",
        "terminal_id": terminal.terminal_id,
        "active_session_id": terminal.active_session_id,
        "transport": "tailscale_private_overlay",
        "bridge_state": bridge_state,
        "byte_bridge_state": byte_bridge_state,
        "data": data,
        "bytes": data.len(),
        "created_at": timestamp_string()
    });
    append_remote_event(
        resolved,
        "remote_terminal_input",
        "terminal",
        terminal.terminal_id.as_str(),
        terminal.active_session_id.as_deref(),
        payload.clone(),
    )?;
    publish_project_checkpoint_for_daemon(resolved, &["remote", "terminal"])?;
    Ok(payload)
}

fn record_terminal_resize(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    cols: u16,
    rows: u16,
) -> Result<Value, DaemonError> {
    record_terminal_resize_with_bridge_state(
        context,
        resolved,
        cols,
        rows,
        "queued_for_governed_bridge",
        "contract_ready_not_host_pty",
    )
}

fn record_terminal_resize_with_bridge_state(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    cols: u16,
    rows: u16,
    bridge_state: &str,
    byte_bridge_state: &str,
) -> Result<Value, DaemonError> {
    ensure_remote_ready(context, resolved, "terminal_resize")?;
    if !(20..=300).contains(&cols) || !(5..=120).contains(&rows) {
        return Err(DaemonError::Internal(format!(
            "terminal resize out of range: {cols}x{rows}"
        )));
    }
    let terminal = attached_terminal_projection(resolved)?;
    let payload = json!({
        "schema_version": "remote_terminal_resize.v1",
        "terminal_id": terminal.terminal_id,
        "active_session_id": terminal.active_session_id,
        "transport": "tailscale_private_overlay",
        "bridge_state": bridge_state,
        "byte_bridge_state": byte_bridge_state,
        "cols": cols,
        "rows": rows,
        "created_at": timestamp_string()
    });
    append_remote_event(
        resolved,
        "remote_terminal_resize",
        "terminal",
        terminal.terminal_id.as_str(),
        terminal.active_session_id.as_deref(),
        payload.clone(),
    )?;
    publish_project_checkpoint_for_daemon(resolved, &["remote", "terminal"])?;
    Ok(payload)
}

fn record_terminal_signal(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    signal: &str,
) -> Result<Value, DaemonError> {
    record_terminal_signal_with_bridge_state(
        context,
        resolved,
        signal,
        "queued_for_governed_bridge",
        "contract_ready_not_host_pty",
    )
}

fn record_terminal_signal_with_bridge_state(
    context: &DaemonContext,
    resolved: &ResolvedProject,
    signal: &str,
    bridge_state: &str,
    byte_bridge_state: &str,
) -> Result<Value, DaemonError> {
    ensure_remote_ready(context, resolved, "terminal_signal")?;
    let signal = signal.trim();
    if !matches!(signal, "interrupt" | "terminate" | "eof") {
        return Err(DaemonError::Internal(format!(
            "unsupported terminal signal: {signal}"
        )));
    }
    let terminal = attached_terminal_projection(resolved)?;
    let payload = json!({
        "schema_version": "remote_terminal_signal.v1",
        "terminal_id": terminal.terminal_id,
        "active_session_id": terminal.active_session_id,
        "transport": "tailscale_private_overlay",
        "bridge_state": bridge_state,
        "byte_bridge_state": byte_bridge_state,
        "signal": signal,
        "created_at": timestamp_string()
    });
    append_remote_event(
        resolved,
        "remote_terminal_signal",
        "terminal",
        terminal.terminal_id.as_str(),
        terminal.active_session_id.as_deref(),
        payload.clone(),
    )?;
    publish_project_checkpoint_for_daemon(resolved, &["remote", "terminal"])?;
    Ok(payload)
}

fn is_terminal_websocket_request(request: &DaemonRequest) -> bool {
    let method = request.method.to_ascii_uppercase();
    let (path, _) = split_path_query(&request.path);
    method == "GET" && path == "/api/terminal/ws" && is_websocket_upgrade(request)
}

fn terminal_peer_allowed_for_private_overlay(peer: SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V4(ip) => ip.is_loopback() || is_tailscale_overlay_ipv4(ip),
        IpAddr::V6(ip) => ip.is_loopback() || is_tailscale_overlay_ipv6(ip),
    }
}

fn is_tailscale_overlay_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_tailscale_overlay_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
}

fn is_websocket_upgrade(request: &DaemonRequest) -> bool {
    let upgrade = header_value(request, "upgrade")
        .map(|value| value.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    let connection = header_value(request, "connection")
        .map(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
        })
        .unwrap_or(false);
    upgrade && connection && header_value(request, "sec-websocket-key").is_some()
}

fn header_value<'a>(request: &'a DaemonRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn attached_terminal_projection(
    resolved: &ResolvedProject,
) -> Result<host_surface::GovernedTerminalProjection, DaemonError> {
    let path = resolved
        .data_dir
        .join("remote")
        .join("terminal_projection.json");
    if !path.exists() {
        return Err(DaemonError::Internal(
            "terminal bridge requires /api/terminal/attach before input, resize, signal, or websocket control"
                .to_string(),
        ));
    }
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(DaemonError::Json)
}

fn websocket_accept_key(request: &DaemonRequest) -> Result<String, DaemonError> {
    let key = header_value(request, "sec-websocket-key")
        .ok_or_else(|| DaemonError::Internal("missing Sec-WebSocket-Key".to_string()))?;
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    Ok(base64::engine::general_purpose::STANDARD.encode(hasher.finalize()))
}

fn read_websocket_text_frame(stream: &mut TcpStream) -> Result<Option<String>, DaemonError> {
    let mut header = [0u8; 2];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
            ) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(DaemonError::Io(err)),
    }
    let opcode = header[0] & 0x0f;
    if opcode == 0x8 {
        return Ok(None);
    }
    if opcode != 0x1 {
        return Err(DaemonError::Internal(format!(
            "unsupported websocket opcode: {opcode}"
        )));
    }
    let masked = header[1] & 0x80 != 0;
    let mut len = (header[1] & 0x7f) as u64;
    if len == 126 {
        let mut extended = [0u8; 2];
        stream.read_exact(&mut extended)?;
        len = u16::from_be_bytes(extended) as u64;
    } else if len == 127 {
        let mut extended = [0u8; 8];
        stream.read_exact(&mut extended)?;
        len = u64::from_be_bytes(extended);
    }
    if len > 64 * 1024 {
        return Err(DaemonError::Internal(format!(
            "terminal websocket frame too large: {len}"
        )));
    }
    let mut mask = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        stream.read_exact(&mut payload)?;
    }
    if masked {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }
    String::from_utf8(payload)
        .map(Some)
        .map_err(|err| DaemonError::Internal(err.to_string()))
}

fn write_websocket_text_frame(stream: &mut TcpStream, text: &str) -> Result<(), DaemonError> {
    let bytes = text.as_bytes();
    let mut header = vec![0x81];
    if bytes.len() < 126 {
        header.push(bytes.len() as u8);
    } else if bytes.len() <= u16::MAX as usize {
        header.push(126);
        header.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    }
    stream.write_all(&header)?;
    stream.write_all(bytes)?;
    Ok(())
}

fn json_success<T: Serialize>(
    command: &str,
    resolved: &ResolvedProject,
    session_id: Option<String>,
    data: T,
) -> Result<DaemonResponse, DaemonError> {
    json_response(
        200,
        &json!({
            "ok": true,
            "command": command,
            "project_id": resolved.project_id,
            "session_id": session_id,
            "data": data
        }),
    )
}

fn authorize_api_request(
    context: &DaemonContext,
    request: &DaemonRequest,
) -> Option<DaemonResponse> {
    let Some(expected) = context.control_token.as_deref().map(str::trim) else {
        return json_response(
            401,
            &json!({
                "ok": false,
                "command": "remote daemon",
                "error": {
                    "code": "remote_daemon_control_token_required",
                    "message": "remote daemon API requires a configured control token",
                    "hint": "Restart the daemon with --control-token or RESEARCH_CLI_REMOTE_DAEMON_TOKEN.",
                    "retryable": false
                }
            }),
        )
        .ok();
    };
    if expected.is_empty() {
        return json_response(
            401,
            &json!({
                "ok": false,
                "command": "remote daemon",
                "error": {
                    "code": "remote_daemon_control_token_required",
                    "message": "remote daemon API requires a non-empty control token",
                    "hint": "Restart the daemon with a non-empty --control-token.",
                    "retryable": false
                }
            }),
        )
        .ok();
    }
    let provided = request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.trim())
        .and_then(|value| value.strip_prefix("Bearer ").or(Some(value)))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_default();
    if provided == expected {
        return None;
    }
    json_response(
        401,
        &json!({
            "ok": false,
            "command": "remote daemon",
            "error": {
                "code": "remote_daemon_unauthorized",
                "message": "remote daemon control token is required",
                "hint": "Pass the daemon control token in the Authorization header.",
                "retryable": false
            }
        }),
    )
    .ok()
}

fn error_response(err: DaemonError) -> Result<DaemonResponse, DaemonError> {
    let (status, code, message) = match err {
        DaemonError::Envelope { status, envelope } => return json_response(status, &envelope),
        DaemonError::Internal(message) => (400, "daemon_request_invalid", message),
        DaemonError::Io(err) => (500, "daemon_io_failed", err.to_string()),
        DaemonError::Json(err) => (400, "daemon_json_invalid", err.to_string()),
    };
    json_response(
        status,
        &json!({
            "ok": false,
            "command": "remote daemon",
            "error": {
                "code": code,
                "message": message,
                "hint": "Check the mobile request payload and project binding.",
                "retryable": false
            }
        }),
    )
}

fn remote_to_daemon_error(
    command: &str,
    resolved: &ResolvedProject,
    err: RemoteError,
) -> DaemonError {
    match err {
        RemoteError::ActionRejected(rejection) => DaemonError::Envelope {
            status: 409,
            envelope: json!({
                "ok": false,
                "command": command,
                "project_id": resolved.project_id,
                "error": {
                    "code": "remote_action_rejected",
                    "message": rejection.reason,
                    "hint": rejection.next_steps.first().cloned(),
                    "retryable": rejection.retryable
                },
                "data": rejection
            }),
        },
        other => DaemonError::Internal(other.to_string()),
    }
}

fn remote_internal(err: RemoteError) -> DaemonError {
    DaemonError::Internal(err.to_string())
}

fn host_surface_internal(err: host_surface::HostSurfaceError) -> DaemonError {
    DaemonError::Internal(err.to_string())
}

fn parse_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, DaemonError> {
    Ok(serde_json::from_slice(body)?)
}

fn required(value: Option<String>, field: &str) -> Result<String, DaemonError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| DaemonError::Internal(format!("missing required field: {field}")))
}

fn json_response(status: u16, value: &Value) -> Result<DaemonResponse, DaemonError> {
    json_response_with_headers(status, value, Vec::new())
}

fn json_response_with_headers(
    status: u16,
    value: &Value,
    headers: Vec<(String, String)>,
) -> Result<DaemonResponse, DaemonError> {
    Ok(DaemonResponse {
        status,
        content_type: "application/json; charset=utf-8",
        headers,
        body: serde_json::to_vec(value)?,
    })
}

fn text_response(status: u16, content_type: &'static str, text: String) -> DaemonResponse {
    DaemonResponse {
        status,
        content_type,
        headers: Vec::new(),
        body: text.into_bytes(),
    }
}

fn empty_response(status: u16, content_type: &'static str) -> DaemonResponse {
    DaemonResponse {
        status,
        content_type,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn split_path_query(path: &str) -> (String, String) {
    match path.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (path.to_string(), String::new()),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}

fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = value.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let hi = chars.next()?;
            let lo = chars.next()?;
            let hex = [hi, lo];
            let hex = std::str::from_utf8(&hex).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
        } else if byte == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn timestamp_string() -> String {
    timestamp_millis().to_string()
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct PairRequest {
    cwd: Option<String>,
    client_id: Option<String>,
    ticket_id: Option<String>,
    lease_expires_at: Option<String>,
    ticket_expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuickPairRequest {
    cwd: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReconnectRequest {
    cwd: Option<String>,
    client_id: Option<String>,
    after_seq: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TuiActionRequest {
    cwd: Option<String>,
    action_id: Option<String>,
    session_id: Option<String>,
    message: Option<String>,
    trigger_id: Option<String>,
    thread_id: Option<String>,
    stage_execution_id: Option<String>,
    operation: Option<String>,
    decision: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttachRequest {
    cwd: Option<String>,
    session_id: Option<String>,
    strategy: Option<String>,
    execute: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HandoffRequest {
    cwd: Option<String>,
    session_id: Option<String>,
    target_client: Option<String>,
    execute: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TakeoverRequest {
    cwd: Option<String>,
    session_id: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotifyRequest {
    cwd: Option<String>,
    kind: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageRequest {
    cwd: Option<String>,
    session_id: Option<String>,
    message: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttachmentUploadRequest {
    cwd: Option<String>,
    session_id: Option<String>,
    attachments: Vec<AttachmentUploadItem>,
}

#[derive(Debug, Deserialize)]
struct AttachmentUploadItem {
    name: String,
    mime_type: String,
    data_base64: String,
}

#[derive(Debug, Deserialize)]
struct SessionResumeRequest {
    cwd: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionCreateRequest {
    cwd: Option<String>,
    title: Option<String>,
    permission_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PermissionRequest {
    cwd: Option<String>,
    request_id: Option<String>,
    decision: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PermissionModeRequest {
    cwd: Option<String>,
    permission_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TerminalRequest {
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TerminalInputRequest {
    cwd: Option<String>,
    data: String,
}

#[derive(Debug, Deserialize)]
struct TerminalResizeRequest {
    cwd: Option<String>,
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
struct TerminalSignalRequest {
    cwd: Option<String>,
    signal: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_resolved_project(cwd: PathBuf) -> ResolvedProject {
        ResolvedProject {
            project_id: "proj_test".to_string(),
            workspace_root: cwd.clone(),
            workspace_hash: "hash_test".to_string(),
            data_dir: cwd.join(".pmcli"),
            resolution_source: "unit_test".to_string(),
        }
    }

    #[test]
    fn terminal_websocket_ticket_is_removed_after_one_consume() {
        let cwd = std::env::temp_dir().join(format!(
            "research_cli_ticket_consume_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let context =
            DaemonContext::new(cwd.join("state"), cwd.clone()).with_control_token("secret");
        let resolved = test_resolved_project(cwd);
        let issued = issue_terminal_ws_ticket(&context, &resolved).expect("ticket should issue");
        let ticket = issued["ticket"]
            .as_str()
            .expect("ticket should be present")
            .to_string();

        consume_terminal_ws_ticket(&context, &ticket, &resolved)
            .expect("first consume should pass");

        assert!(
            !context
                .terminal_ws_tickets
                .lock()
                .expect("ticket lock should not be poisoned")
                .contains_key(&ticket),
            "consumed websocket tickets should not remain reusable or leak in memory"
        );
        assert!(
            consume_terminal_ws_ticket(&context, &ticket, &resolved).is_err(),
            "second consume should fail after the ticket is removed"
        );
    }

    #[test]
    fn terminal_private_overlay_peer_filter_allows_only_loopback_and_tailscale_peers() {
        assert!(terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8787,
        )));
        assert!(terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            8787,
        )));
        assert!(terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(100, 127, 255, 254)),
            8787,
        )));
        assert!(terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e0, 0, 0, 0, 0, 1)),
            8787,
        )));
        assert!(!terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8)),
            8787,
        )));
        assert!(!terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(100, 128, 0, 1)),
            8787,
        )));
        assert!(!terminal_peer_allowed_for_private_overlay(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e1, 0, 0, 0, 0, 1)),
            8787,
        )));
    }
}
