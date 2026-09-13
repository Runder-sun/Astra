use crate::events::writer::append_event;
use crate::events::{read_events_from, KernelEventEnvelope};
use crate::mcp;
use crate::plugins;
use crate::projects::current::{
    resolve_current_project, resolve_current_project_with_trace, ProjectResolutionTrace,
    ResolveCurrentProjectError,
};
use crate::projects::registry::ProjectRegistry;
use crate::providers::{
    list_providers, provider_auth_status, selected_provider, ProviderResolutionTrace,
};
use crate::runtime::checkpoint::KernelStateBundle;
use crate::runtime::reducer::{publish_checkpoint, CheckpointPublishPlan};
use crate::session::store::SessionStore;
use crate::session::transcript::TranscriptLine;
use serde::Serialize;
use serde_json::json;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimePreflightReport {
    pub overall_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_trace: Option<ProjectResolutionTrace>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provider_status: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub auth_status: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugin_status: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hook_status: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_status: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub degraded_features: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recovery_hints: Vec<String>,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub overall_status: String,
    pub ready: bool,
    pub preflight: RuntimePreflightReport,
    pub workspace: DoctorCheck,
    pub project: DoctorCheck,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<DoctorCheck>,
    pub config: DoctorCheck,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub binaries: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<DoctorCheck>,
    pub remote: DoctorCheck,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repair_hints: Vec<String>,
}

pub struct DoctorService;

#[derive(Debug, Clone, Serialize)]
pub struct SmokeResult {
    pub overall_status: String,
    pub preflight: RuntimePreflightReport,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_trace: Option<ProviderResolutionTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub source_mutation_free: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mutated_paths: Vec<String>,
}

pub struct SmokeService;

impl DoctorService {
    pub fn run(
        registry: &ProjectRegistry,
        cwd: &Path,
        explicit_project: Option<&str>,
        explicit_cwd: Option<&Path>,
    ) -> DoctorReport {
        let scope_root = explicit_cwd.unwrap_or(cwd);
        let state_home = state_home();
        let plugin_snapshot = plugins::discover(&state_home, scope_root);
        let mcp_snapshot = mcp::discover(&state_home, scope_root);
        let plugin_checks = plugin_checks(&plugin_snapshot);
        let hook_checks = hook_checks(&plugin_snapshot);
        let mcp_checks = mcp_checks(&mcp_snapshot);
        let workspace = workspace_check(scope_root);
        let project_resolution =
            resolve_current_project(registry, explicit_project, explicit_cwd, cwd);
        let project = project_check(project_resolution.as_ref());
        let providers = provider_checks();
        let has_ready_provider = providers.iter().any(|check| check.status == "ready");
        let config = config_check(
            project_resolution
                .as_ref()
                .ok()
                .map(|project| project.data_dir.as_path()),
        );
        let binaries = binary_checks();
        let remote = remote_check();
        let preflight = preflight_report(registry, cwd, explicit_project, explicit_cwd);

        let overall_status = preflight.overall_status.clone();
        let repair_hints = preflight.recovery_hints.clone();

        DoctorReport {
            overall_status,
            ready: workspace.status == "ready"
                && project.status == "ready"
                && has_ready_provider
                && config.status == "ready"
                && binaries.iter().all(|check| check.status == "ready"),
            preflight,
            workspace,
            project,
            providers,
            config,
            binaries,
            plugins: plugin_checks,
            hooks: hook_checks,
            mcp: mcp_checks,
            remote,
            repair_hints,
        }
    }
}

impl SmokeService {
    pub fn run(
        registry: &ProjectRegistry,
        cwd: &Path,
        explicit_project: Option<&str>,
        explicit_cwd: Option<&Path>,
    ) -> Result<SmokeResult, String> {
        let doctor = DoctorService::run(registry, cwd, explicit_project, explicit_cwd);
        let preflight = doctor.preflight.clone();
        if preflight.overall_status == "blocked" {
            return Ok(SmokeResult {
                overall_status: "blocked".to_string(),
                preflight,
                checks: vec![DoctorCheck {
                    name: "preflight".to_string(),
                    status: "blocked".to_string(),
                    detail: "doctor preflight blocked smoke execution".to_string(),
                }],
                session_id: None,
                turn_id: None,
                event_log_path: None,
                transcript_path: None,
                provider_trace: None,
                source_mutation_free: true,
                mutated_paths: Vec::new(),
            });
        }

        let resolved = resolve_current_project(registry, explicit_project, explicit_cwd, cwd)
            .map_err(|err| err.to_string())?;
        let store = SessionStore::new(resolved.data_dir.clone());
        let session = store
            .create_session(Some("Smoke Session".to_string()))
            .map_err(|err| err.to_string())?;
        let stored = store
            .load_session(&session.session_id)
            .map_err(|err| err.to_string())?;

        store
            .append_line(
                &session.session_id,
                TranscriptLine::Control {
                    event: "smoke_session_open".to_string(),
                },
            )
            .map_err(|err| err.to_string())?;
        store
            .append_line(
                &session.session_id,
                TranscriptLine::ToolResult {
                    tool_name: "smoke".to_string(),
                    output: "session persistence and event log validated".to_string(),
                    call_id: None,
                },
            )
            .map_err(|err| err.to_string())?;

        let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
        let mut next_seq = read_events_from(&event_log_path)
            .map_err(|err| err.to_string())?
            .last()
            .map(|event| event.seq + 1)
            .unwrap_or(1);

        append_event(
            &event_log_path,
            &KernelEventEnvelope {
                event_id: format!("evt_smoke_preflight_{next_seq}"),
                seq: next_seq,
                event_name: "runtime_preflight".to_string(),
                phase: "terminal".to_string(),
                terminal_outcome: Some("succeeded".to_string()),
                object_kind: "runtime".to_string(),
                object_id: "smoke".to_string(),
                session_id: Some(session.session_id.clone()),
                project_id: Some(resolved.project_id.clone()),
                timestamp: timestamp_string(),
                payload: json!({
                    "status": preflight.overall_status
                }),
            },
        )
        .map_err(|err| err.to_string())?;
        next_seq += 1;
        append_event(
            &event_log_path,
            &KernelEventEnvelope {
                event_id: format!("evt_smoke_session_open_{next_seq}"),
                seq: next_seq,
                event_name: "session_open".to_string(),
                phase: "terminal".to_string(),
                terminal_outcome: Some("succeeded".to_string()),
                object_kind: "session".to_string(),
                object_id: session.session_id.clone(),
                session_id: Some(session.session_id.clone()),
                project_id: Some(resolved.project_id.clone()),
                timestamp: timestamp_string(),
                payload: json!({
                    "status": "active",
                    "transcript_path": stored.transcript_path.display().to_string()
                }),
            },
        )
        .map_err(|err| err.to_string())?;

        let checkpoint_path = resolved.data_dir.join("project_state.json");
        let committed = if checkpoint_path.exists() {
            KernelStateBundle::load_from(&checkpoint_path).map_err(|err| err.to_string())?
        } else {
            KernelStateBundle::default()
        };
        publish_checkpoint(
            &checkpoint_path,
            &event_log_path,
            CheckpointPublishPlan {
                base_seq_cursor: committed.seq_cursor,
                base_checkpoint_epoch: committed.checkpoint_epoch,
                touched_families: vec!["session".to_string()],
            },
        )
        .map_err(|err| err.to_string())?;

        registry
            .touch_project(&resolved.project_id, Some(session.session_id.clone()))
            .map_err(|err| err.to_string())?;

        let mutated_paths = vec![
            resolved
                .data_dir
                .join("sessions")
                .join(&session.session_id)
                .join("session.json")
                .display()
                .to_string(),
            resolved
                .data_dir
                .join("sessions")
                .join(&session.session_id)
                .join("lineage.json")
                .display()
                .to_string(),
            stored.transcript_path.display().to_string(),
            event_log_path.display().to_string(),
            checkpoint_path.display().to_string(),
        ];
        let canonical_data_dir = resolved
            .data_dir
            .canonicalize()
            .unwrap_or_else(|_| resolved.data_dir.clone());
        let source_mutation_free = mutated_paths.iter().all(|path| {
            PathBuf::from(path)
                .canonicalize()
                .map(|candidate| candidate.starts_with(&canonical_data_dir))
                .unwrap_or(false)
        });

        let checks = vec![
            DoctorCheck {
                name: "session_persistence".to_string(),
                status: "ready".to_string(),
                detail: session.session_id.clone(),
            },
            DoctorCheck {
                name: "event_log".to_string(),
                status: "ready".to_string(),
                detail: event_log_path.display().to_string(),
            },
            DoctorCheck {
                name: "checkpoint_reducer".to_string(),
                status: "ready".to_string(),
                detail: checkpoint_path.display().to_string(),
            },
        ];

        Ok(SmokeResult {
            overall_status: if source_mutation_free {
                "ready".to_string()
            } else {
                "blocked".to_string()
            },
            preflight,
            checks,
            provider_trace: Some(select_provider_trace()),
            session_id: Some(session.session_id),
            turn_id: None,
            event_log_path: Some(event_log_path.display().to_string()),
            transcript_path: Some(stored.transcript_path.display().to_string()),
            source_mutation_free,
            mutated_paths,
        })
    }
}

fn preflight_report(
    registry: &ProjectRegistry,
    cwd: &Path,
    explicit_project: Option<&str>,
    explicit_cwd: Option<&Path>,
) -> RuntimePreflightReport {
    let scope_root = explicit_cwd.unwrap_or(cwd);
    let project_trace =
        match resolve_current_project_with_trace(registry, explicit_project, explicit_cwd, cwd) {
            Ok(resolved) => resolved.trace,
            Err(err) => err.trace,
        };
    let provider_status = provider_checks();
    let auth_status = provider_auth_checks();
    let state_home = state_home();
    let plugin_snapshot = plugins::discover(&state_home, scope_root);
    let mcp_snapshot = mcp::discover(&state_home, scope_root);
    let plugin_status = plugin_checks(&plugin_snapshot);
    let hook_status = hook_checks(&plugin_snapshot);
    let mcp_status = mcp_checks(&mcp_snapshot);
    let ready = project_trace.resolution_status == "resolved"
        && provider_status.iter().any(|check| check.status == "ready")
        && workspace_check(scope_root).status == "ready";
    let overall_status = if !ready { "blocked" } else { "ready" }.to_string();

    let mut degraded_features = Vec::new();
    if !provider_status.iter().all(|check| check.status == "ready") {
        degraded_features.push("provider_matrix_partial".to_string());
    }
    if plugin_status.iter().any(|check| check.status != "ready") {
        degraded_features.push("plugin_registry_partial".to_string());
    }
    if hook_status.iter().any(|check| check.status != "ready") {
        degraded_features.push("hook_registry_partial".to_string());
    }
    if mcp_status.iter().any(|check| check.status != "ready") {
        degraded_features.push("mcp_registry_partial".to_string());
    }

    let mut recovery_hints = Vec::new();
    if project_trace.resolution_status != "resolved" {
        recovery_hints.push(
            "Register the project with `research-cli projects init --json` or pass --project."
                .to_string(),
        );
    }
    if !provider_status.iter().any(|check| check.status == "ready") {
        recovery_hints.push(
            "Set one provider credential such as OPENAI_API_KEY, SJTU_GLM_API_KEY, or ANTHROPIC_API_KEY.".to_string(),
        );
    }
    if plugin_status.iter().any(|check| check.status != "ready") {
        recovery_hints.push(
            "Inspect `research-cli plugins validate --json` to repair degraded plugin manifests."
                .to_string(),
        );
    }
    if mcp_status.iter().any(|check| check.status != "ready") {
        recovery_hints.push(
            "Inspect `research-cli mcp test --json` and `research-cli mcp refresh --json`."
                .to_string(),
        );
    }

    RuntimePreflightReport {
        overall_status,
        project_id: project_trace.resolved_project_id.clone(),
        workspace_root: project_trace
            .resolved_project
            .as_ref()
            .map(|entry| entry.workspace_root.display().to_string())
            .or_else(|| Some(scope_root.display().to_string())),
        project_trace: Some(project_trace),
        provider_status,
        auth_status,
        plugin_status,
        hook_status,
        mcp_status,
        degraded_features,
        recovery_hints,
        ready,
    }
}

fn provider_auth_checks() -> Vec<DoctorCheck> {
    provider_auth_status()
        .providers
        .into_iter()
        .map(|entry| DoctorCheck {
            name: format!("auth:{}", entry.provider_id),
            status: entry.status,
            detail: entry
                .hint
                .unwrap_or_else(|| format!("base_url={}", entry.base_url)),
        })
        .collect()
}

fn workspace_check(cwd: &Path) -> DoctorCheck {
    if let Some(workspace_root) = find_workspace_root(cwd) {
        DoctorCheck {
            name: "workspace".to_string(),
            status: "ready".to_string(),
            detail: workspace_root.display().to_string(),
        }
    } else {
        DoctorCheck {
            name: "workspace".to_string(),
            status: "blocked".to_string(),
            detail: "no workspace markers (.git or .pmcli) found from current directory"
                .to_string(),
        }
    }
}

fn project_check(
    result: Result<&crate::projects::current::ResolvedProject, &ResolveCurrentProjectError>,
) -> DoctorCheck {
    match result {
        Ok(project) => DoctorCheck {
            name: "project".to_string(),
            status: "ready".to_string(),
            detail: format!(
                "{} at {}",
                project.project_id,
                project.workspace_root.display()
            ),
        },
        Err(err) => DoctorCheck {
            name: "project".to_string(),
            status: "blocked".to_string(),
            detail: err.to_string(),
        },
    }
}

fn provider_checks() -> Vec<DoctorCheck> {
    list_providers()
        .providers
        .into_iter()
        .map(|provider| DoctorCheck {
            name: format!("provider:{}", provider.provider_id),
            status: if provider.auth_status == "configured" {
                "ready".to_string()
            } else {
                "degraded".to_string()
            },
            detail: format!(
                "catalog={}, base_url_source={}",
                provider.catalog_source, provider.base_url_source
            ),
        })
        .collect()
}

fn config_check(data_dir: Option<&Path>) -> DoctorCheck {
    if let Some(data_dir) = data_dir {
        DoctorCheck {
            name: "config".to_string(),
            status: "ready".to_string(),
            detail: data_dir.display().to_string(),
        }
    } else {
        DoctorCheck {
            name: "config".to_string(),
            status: "degraded".to_string(),
            detail: ".pmcli has not been bootstrapped yet".to_string(),
        }
    }
}

fn binary_checks() -> Vec<DoctorCheck> {
    ["git", "rustc"]
        .into_iter()
        .map(|binary| DoctorCheck {
            name: format!("binary:{binary}"),
            status: if find_on_path(binary) {
                "ready"
            } else {
                "degraded"
            }
            .to_string(),
            detail: if find_on_path(binary) {
                "available on PATH".to_string()
            } else {
                "missing on PATH".to_string()
            },
        })
        .collect()
}

fn remote_check() -> DoctorCheck {
    DoctorCheck {
        name: "remote".to_string(),
        status: "ready".to_string(),
        detail: "M5 remote plane is graduated as a Tailscale/private-overlay daemon PWA backed by canonical .pmcli state".to_string(),
    }
}

fn select_provider_trace() -> ProviderResolutionTrace {
    selected_provider()
}

fn plugin_checks(snapshot: &plugins::PluginRegistrySnapshot) -> Vec<DoctorCheck> {
    snapshot
        .plugins
        .iter()
        .map(|plugin| DoctorCheck {
            name: format!("plugin:{}", plugin.entry.plugin_id),
            status: if plugin.entry.degraded {
                "degraded".to_string()
            } else {
                "ready".to_string()
            },
            detail: plugin.entry.manifest_path.clone(),
        })
        .collect()
}

fn hook_checks(snapshot: &plugins::PluginRegistrySnapshot) -> Vec<DoctorCheck> {
    snapshot
        .plugins
        .iter()
        .flat_map(|plugin| plugin.hooks.iter())
        .map(|hook| DoctorCheck {
            name: format!("hook:{}", hook.hook_id),
            status: if Path::new(&hook.source_path).exists() {
                "ready".to_string()
            } else {
                "degraded".to_string()
            },
            detail: hook.source_path.clone(),
        })
        .collect()
}

fn mcp_checks(snapshot: &mcp::MCPSnapshot) -> Vec<DoctorCheck> {
    snapshot
        .servers
        .iter()
        .map(|server| DoctorCheck {
            name: format!("mcp:{}", server.entry.server_id),
            status: server.entry.status.clone(),
            detail: server.entry.manifest_path.clone(),
        })
        .collect()
}

fn find_workspace_root(cwd: &Path) -> Option<PathBuf> {
    let mut cursor = cwd.canonicalize().ok()?;
    loop {
        if cursor.join(".git").exists() || cursor.join(".pmcli").exists() {
            return Some(cursor);
        }
        cursor = cursor.parent()?.to_path_buf();
    }
}

fn find_on_path(binary: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).exists()))
        .unwrap_or(false)
}

fn state_home() -> PathBuf {
    if let Ok(path) = env::var("RESEARCH_CLI_STATE_HOME") {
        return PathBuf::from(path);
    }

    if let Ok(path) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(path).join("research-cli");
    }

    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("research-cli")
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}
