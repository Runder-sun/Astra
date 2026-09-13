use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct MCPServerEntry {
    pub server_id: String,
    pub transport: String,
    pub tool_count: usize,
    pub auth_state: String,
    pub status: String,
    pub source: String,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MCPListResult {
    pub search_paths: Vec<String>,
    pub servers: Vec<MCPServerEntry>,
    pub total_count: usize,
    pub degraded_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MCPInspectionResult {
    pub server_id: String,
    pub transport: String,
    pub tool_surface: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub auth_source: String,
    pub timeout_ms: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_known_failure: String,
    pub manifest_path: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MCPTestRecord {
    pub server_id: String,
    pub status: String,
    pub handshake_status: String,
    pub tool_surface_ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MCPTestResult {
    pub scope: String,
    pub overall_status: String,
    pub tested_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<MCPTestRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MCPRefreshResult {
    pub refreshed_at: String,
    pub search_paths: Vec<String>,
    pub servers: Vec<MCPServerEntry>,
    pub total_count: usize,
    pub degraded_count: usize,
}

#[derive(Debug, Clone)]
pub struct MCPSnapshot {
    pub search_paths: Vec<PathBuf>,
    pub servers: Vec<DiscoveredMCPServer>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredMCPServer {
    pub entry: MCPServerEntry,
    pub tools: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub auth_source: String,
    pub timeout_ms: u64,
    pub last_known_failure: String,
}

#[derive(Debug, Deserialize)]
struct MCPServerManifest {
    id: String,
    #[serde(default = "default_transport")]
    transport: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    tool_count: usize,
    #[serde(default)]
    auth_state: String,
    #[serde(default)]
    auth_source: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    disabled_tools: Vec<String>,
    #[serde(default)]
    degraded_reason: String,
}

#[derive(Debug)]
pub enum MCPRegistryError {
    UnknownServer(String),
}

impl std::fmt::Display for MCPRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownServer(server_id) => write!(f, "unknown MCP server: {server_id}"),
        }
    }
}

impl std::error::Error for MCPRegistryError {}

pub fn discover(state_home: &Path, cwd: &Path) -> MCPSnapshot {
    let search_paths = mcp_search_paths(state_home, cwd);
    let mut discovered = BTreeMap::<String, DiscoveredMCPServer>::new();

    for root in &search_paths {
        let Ok(children) = fs::read_dir(root) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            if !child_path.is_file()
                || child_path.extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            let Ok(contents) = fs::read_to_string(&child_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<MCPServerManifest>(&contents) else {
                continue;
            };
            if manifest.id.trim().is_empty() || discovered.contains_key(&manifest.id) {
                continue;
            }
            let tool_count = if manifest.tool_count == 0 {
                manifest.tools.len()
            } else {
                manifest.tool_count
            };
            let auth_state = if manifest.auth_state.trim().is_empty() {
                "unknown".to_string()
            } else {
                manifest.auth_state.clone()
            };
            let degraded = !manifest.degraded_reason.trim().is_empty()
                || manifest.command.trim().is_empty()
                || tool_count == 0;
            let status = if degraded {
                "degraded".to_string()
            } else {
                "ready".to_string()
            };

            discovered.insert(
                manifest.id.clone(),
                DiscoveredMCPServer {
                    entry: MCPServerEntry {
                        server_id: manifest.id.clone(),
                        transport: manifest.transport.clone(),
                        tool_count,
                        auth_state,
                        status,
                        source: root.display().to_string(),
                        manifest_path: child_path.display().to_string(),
                    },
                    tools: manifest.tools.clone(),
                    disabled_tools: manifest.disabled_tools.clone(),
                    auth_source: if manifest.auth_source.trim().is_empty() {
                        "unknown".to_string()
                    } else {
                        manifest.auth_source.clone()
                    },
                    timeout_ms: manifest.timeout_ms,
                    last_known_failure: manifest.degraded_reason.clone(),
                },
            );
        }
    }

    let mut servers = discovered.into_values().collect::<Vec<_>>();
    servers.sort_by(|left, right| left.entry.server_id.cmp(&right.entry.server_id));

    MCPSnapshot {
        search_paths,
        servers,
    }
}

pub fn list(snapshot: &MCPSnapshot) -> MCPListResult {
    MCPListResult {
        search_paths: snapshot
            .search_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        servers: snapshot
            .servers
            .iter()
            .map(|server| server.entry.clone())
            .collect(),
        total_count: snapshot.servers.len(),
        degraded_count: snapshot
            .servers
            .iter()
            .filter(|server| server.entry.status != "ready")
            .count(),
    }
}

pub fn inspect(
    snapshot: &MCPSnapshot,
    server_id: &str,
) -> Result<MCPInspectionResult, MCPRegistryError> {
    let server = snapshot
        .servers
        .iter()
        .find(|server| server.entry.server_id == server_id)
        .ok_or_else(|| MCPRegistryError::UnknownServer(server_id.to_string()))?;

    Ok(MCPInspectionResult {
        server_id: server.entry.server_id.clone(),
        transport: server.entry.transport.clone(),
        tool_surface: server.tools.clone(),
        disabled_tools: server.disabled_tools.clone(),
        auth_source: server.auth_source.clone(),
        timeout_ms: server.timeout_ms,
        last_known_failure: server.last_known_failure.clone(),
        manifest_path: server.entry.manifest_path.clone(),
        source: server.entry.source.clone(),
    })
}

pub fn test(
    snapshot: &MCPSnapshot,
    server_id: Option<&str>,
) -> Result<MCPTestResult, MCPRegistryError> {
    let selected = match server_id {
        Some(server_id) => vec![snapshot
            .servers
            .iter()
            .find(|server| server.entry.server_id == server_id)
            .cloned()
            .ok_or_else(|| MCPRegistryError::UnknownServer(server_id.to_string()))?],
        None => snapshot.servers.clone(),
    };

    let servers = selected
        .into_iter()
        .map(|server| {
            let ready = server.entry.status == "ready";
            let mut hints = Vec::new();
            if server.tools.is_empty() {
                hints.push("tool surface is empty".to_string());
            }
            if server.auth_source == "unknown" {
                hints.push("auth source is not declared".to_string());
            }
            MCPTestRecord {
                server_id: server.entry.server_id.clone(),
                status: if ready { "ready" } else { "degraded" }.to_string(),
                handshake_status: if ready { "ready" } else { "degraded" }.to_string(),
                tool_surface_ok: !server.tools.is_empty(),
                hints,
            }
        })
        .collect::<Vec<_>>();

    Ok(MCPTestResult {
        scope: server_id.unwrap_or("all").to_string(),
        overall_status: if servers.iter().all(|server| server.status == "ready") {
            "ready".to_string()
        } else {
            "degraded".to_string()
        },
        tested_count: servers.len(),
        servers,
    })
}

pub fn refresh(snapshot: &MCPSnapshot) -> MCPRefreshResult {
    MCPRefreshResult {
        refreshed_at: timestamp_string(),
        search_paths: snapshot
            .search_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        servers: snapshot
            .servers
            .iter()
            .map(|server| server.entry.clone())
            .collect(),
        total_count: snapshot.servers.len(),
        degraded_count: snapshot
            .servers
            .iter()
            .filter(|server| server.entry.status != "ready")
            .count(),
    }
}

pub fn search_paths(state_home: &Path, cwd: &Path) -> Vec<String> {
    mcp_search_paths(state_home, cwd)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn mcp_search_paths(state_home: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        cwd.join(".pmcli").join("mcp").join("servers"),
        cwd.join(".agents").join("mcp").join("servers"),
        cwd.join(".codex").join("mcp").join("servers"),
        state_home.join("mcp").join("servers"),
    ];

    if let Ok(home) = env::var("HOME") {
        paths.push(
            PathBuf::from(home)
                .join(".codex")
                .join("mcp")
                .join("servers"),
        );
    }

    dedupe_paths(paths)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if deduped.iter().any(|existing| existing == &path) {
            continue;
        }
        deduped.push(path);
    }
    deduped
}

fn default_transport() -> String {
    "stdio".to_string()
}

fn default_timeout_ms() -> u64 {
    2_500
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}
