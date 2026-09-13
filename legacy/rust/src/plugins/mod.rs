use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct PluginRegistryEntry {
    pub plugin_id: String,
    pub manifest_path: String,
    pub source: String,
    pub enabled: bool,
    pub hook_count: usize,
    pub tool_count: usize,
    pub degraded: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub disabled_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginListResult {
    pub search_paths: Vec<String>,
    pub plugins: Vec<PluginRegistryEntry>,
    pub total_count: usize,
    pub degraded_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInspectionResult {
    pub plugin_id: String,
    pub manifest_origin: String,
    pub manifest_path: String,
    pub enabled: bool,
    pub hook_surface: Vec<String>,
    pub tool_additions: Vec<String>,
    pub config_source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_known_degraded_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginValidationIssue {
    pub plugin_id: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginValidationResult {
    pub scope: String,
    pub overall_status: String,
    pub validated_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub degraded_plugin_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<PluginValidationIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookRegistryEntry {
    pub hook_id: String,
    pub owner_plugin_id: String,
    pub trigger: String,
    pub source_path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookListResult {
    pub hooks: Vec<HookRegistryEntry>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookInspectionResult {
    pub hook_id: String,
    pub owner_plugin_id: String,
    pub trigger: String,
    pub source_path: String,
    pub timeout_policy: String,
    pub failure_policy: String,
    pub last_health_record: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookTestRecord {
    pub hook_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookTestResult {
    pub scope: String,
    pub overall_status: String,
    pub tested_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookTestRecord>,
}

#[derive(Debug, Clone)]
pub struct PluginRegistrySnapshot {
    pub search_paths: Vec<PathBuf>,
    pub plugins: Vec<DiscoveredPlugin>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub entry: PluginRegistryEntry,
    pub hooks: Vec<HookRegistryEntry>,
    pub tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PluginManifest {
    #[serde(default)]
    id: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    hooks: BTreeMap<String, Vec<String>>,
}

#[derive(Debug)]
pub enum PluginRegistryError {
    UnknownPlugin(String),
    UnknownHook(String),
}

impl std::fmt::Display for PluginRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPlugin(plugin_id) => write!(f, "unknown plugin: {plugin_id}"),
            Self::UnknownHook(hook_id) => write!(f, "unknown hook: {hook_id}"),
        }
    }
}

impl std::error::Error for PluginRegistryError {}

pub fn discover(state_home: &Path, cwd: &Path) -> PluginRegistrySnapshot {
    let search_paths = plugin_search_paths(state_home, cwd);
    let mut discovered = BTreeMap::<String, DiscoveredPlugin>::new();

    for root in &search_paths {
        let Ok(children) = fs::read_dir(root) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            if !child_path.is_dir() {
                continue;
            }
            let Some(manifest_path) = plugin_manifest_path(&child_path) else {
                continue;
            };
            let manifest_contents = match fs::read_to_string(&manifest_path) {
                Ok(contents) => contents,
                Err(_) => continue,
            };
            let manifest: PluginManifest = match serde_json::from_str(&manifest_contents) {
                Ok(manifest) => manifest,
                Err(_) => continue,
            };

            let plugin_id = if manifest.id.trim().is_empty() {
                child.file_name().to_string_lossy().to_string()
            } else {
                manifest.id.trim().to_string()
            };
            if plugin_id.is_empty() || discovered.contains_key(&plugin_id) {
                continue;
            }

            let hooks = build_hooks(&plugin_id, &child_path, &manifest.hooks);
            let degraded = hooks
                .iter()
                .any(|hook| !Path::new(&hook.source_path).exists());
            let disabled_reason = if degraded {
                "one or more hook paths are missing".to_string()
            } else {
                String::new()
            };

            discovered.insert(
                plugin_id.clone(),
                DiscoveredPlugin {
                    entry: PluginRegistryEntry {
                        plugin_id: plugin_id.clone(),
                        manifest_path: manifest_path.display().to_string(),
                        source: root.display().to_string(),
                        enabled: manifest.enabled,
                        hook_count: hooks.len(),
                        tool_count: manifest.tools.len(),
                        degraded,
                        disabled_reason,
                    },
                    hooks,
                    tools: manifest.tools,
                },
            );
        }
    }

    let mut plugins = discovered.into_values().collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.entry.plugin_id.cmp(&right.entry.plugin_id));

    PluginRegistrySnapshot {
        search_paths,
        plugins,
    }
}

pub fn list(snapshot: &PluginRegistrySnapshot) -> PluginListResult {
    PluginListResult {
        search_paths: snapshot
            .search_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        plugins: snapshot
            .plugins
            .iter()
            .map(|plugin| plugin.entry.clone())
            .collect(),
        total_count: snapshot.plugins.len(),
        degraded_count: snapshot
            .plugins
            .iter()
            .filter(|plugin| plugin.entry.degraded)
            .count(),
    }
}

pub fn inspect(
    snapshot: &PluginRegistrySnapshot,
    plugin_id: &str,
) -> Result<PluginInspectionResult, PluginRegistryError> {
    let plugin = snapshot
        .plugins
        .iter()
        .find(|plugin| plugin.entry.plugin_id == plugin_id)
        .ok_or_else(|| PluginRegistryError::UnknownPlugin(plugin_id.to_string()))?;

    Ok(PluginInspectionResult {
        plugin_id: plugin.entry.plugin_id.clone(),
        manifest_origin: plugin.entry.source.clone(),
        manifest_path: plugin.entry.manifest_path.clone(),
        enabled: plugin.entry.enabled,
        hook_surface: plugin
            .hooks
            .iter()
            .map(|hook| hook.hook_id.clone())
            .collect(),
        tool_additions: plugin.tools.clone(),
        config_source: plugin.entry.source.clone(),
        last_known_degraded_reason: plugin.entry.disabled_reason.clone(),
    })
}

pub fn validate(
    snapshot: &PluginRegistrySnapshot,
    plugin_id: Option<&str>,
) -> Result<PluginValidationResult, PluginRegistryError> {
    let selected = match plugin_id {
        Some(plugin_id) => vec![snapshot
            .plugins
            .iter()
            .find(|plugin| plugin.entry.plugin_id == plugin_id)
            .cloned()
            .ok_or_else(|| PluginRegistryError::UnknownPlugin(plugin_id.to_string()))?],
        None => snapshot.plugins.clone(),
    };

    let mut degraded_plugin_ids = Vec::new();
    let mut issues = Vec::new();
    for plugin in &selected {
        if plugin.entry.degraded {
            degraded_plugin_ids.push(plugin.entry.plugin_id.clone());
            issues.push(PluginValidationIssue {
                plugin_id: plugin.entry.plugin_id.clone(),
                severity: "degraded".to_string(),
                message: plugin.entry.disabled_reason.clone(),
                path: plugin.entry.manifest_path.clone(),
            });
        }
        for hook in &plugin.hooks {
            if !Path::new(&hook.source_path).exists() {
                if !degraded_plugin_ids
                    .iter()
                    .any(|plugin_id| plugin_id == &plugin.entry.plugin_id)
                {
                    degraded_plugin_ids.push(plugin.entry.plugin_id.clone());
                }
                issues.push(PluginValidationIssue {
                    plugin_id: plugin.entry.plugin_id.clone(),
                    severity: "degraded".to_string(),
                    message: format!("missing hook for trigger {}", hook.trigger),
                    path: hook.source_path.clone(),
                });
            }
        }
    }

    Ok(PluginValidationResult {
        scope: plugin_id.unwrap_or("all").to_string(),
        overall_status: if degraded_plugin_ids.is_empty() {
            "ready".to_string()
        } else {
            "degraded".to_string()
        },
        validated_count: selected.len(),
        degraded_plugin_ids,
        issues,
    })
}

pub fn list_hooks(snapshot: &PluginRegistrySnapshot) -> HookListResult {
    let hooks = snapshot
        .plugins
        .iter()
        .flat_map(|plugin| plugin.hooks.clone())
        .collect::<Vec<_>>();
    HookListResult {
        total_count: hooks.len(),
        hooks,
    }
}

pub fn inspect_hook(
    snapshot: &PluginRegistrySnapshot,
    hook_id: &str,
) -> Result<HookInspectionResult, PluginRegistryError> {
    let hook = snapshot
        .plugins
        .iter()
        .flat_map(|plugin| plugin.hooks.iter())
        .find(|hook| hook.hook_id == hook_id)
        .ok_or_else(|| PluginRegistryError::UnknownHook(hook_id.to_string()))?;

    Ok(HookInspectionResult {
        hook_id: hook.hook_id.clone(),
        owner_plugin_id: hook.owner_plugin_id.clone(),
        trigger: hook.trigger.clone(),
        source_path: hook.source_path.clone(),
        timeout_policy: "default".to_string(),
        failure_policy: "warn".to_string(),
        last_health_record: if Path::new(&hook.source_path).exists() {
            "ready".to_string()
        } else {
            "missing".to_string()
        },
    })
}

pub fn test_hooks(
    snapshot: &PluginRegistrySnapshot,
    hook_id: Option<&str>,
) -> Result<HookTestResult, PluginRegistryError> {
    let hooks = match hook_id {
        Some(hook_id) => vec![snapshot
            .plugins
            .iter()
            .flat_map(|plugin| plugin.hooks.iter())
            .find(|hook| hook.hook_id == hook_id)
            .cloned()
            .ok_or_else(|| PluginRegistryError::UnknownHook(hook_id.to_string()))?],
        None => snapshot
            .plugins
            .iter()
            .flat_map(|plugin| plugin.hooks.iter().cloned())
            .collect(),
    };

    let hook_records = hooks
        .into_iter()
        .map(|hook| HookTestRecord {
            hook_id: hook.hook_id,
            status: if Path::new(&hook.source_path).exists() {
                "ready".to_string()
            } else {
                "degraded".to_string()
            },
            detail: hook.source_path,
        })
        .collect::<Vec<_>>();

    Ok(HookTestResult {
        scope: hook_id.unwrap_or("all").to_string(),
        overall_status: if hook_records.iter().all(|record| record.status == "ready") {
            "ready".to_string()
        } else {
            "degraded".to_string()
        },
        tested_count: hook_records.len(),
        hooks: hook_records,
    })
}

pub fn plugin_search_paths_for_display(state_home: &Path, cwd: &Path) -> Vec<String> {
    plugin_search_paths(state_home, cwd)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn plugin_search_paths(state_home: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        cwd.join(".pmcli").join("plugins"),
        cwd.join(".agents").join("plugins"),
        cwd.join(".codex").join("plugins"),
        state_home.join("plugins"),
    ];

    if let Ok(home) = env::var("HOME") {
        paths.push(PathBuf::from(home).join(".codex").join("plugins"));
    }

    dedupe_paths(paths)
}

fn plugin_manifest_path(root: &Path) -> Option<PathBuf> {
    [
        root.join(".codex-plugin").join("plugin.json"),
        root.join(".claude-plugin").join("plugin.json"),
        root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

fn build_hooks(
    plugin_id: &str,
    plugin_root: &Path,
    hooks: &BTreeMap<String, Vec<String>>,
) -> Vec<HookRegistryEntry> {
    let mut entries = Vec::new();
    for (trigger, commands) in hooks {
        for (index, command) in commands.iter().enumerate() {
            entries.push(HookRegistryEntry {
                hook_id: format!("{plugin_id}:{trigger}:{index}"),
                owner_plugin_id: plugin_id.to_string(),
                trigger: trigger.to_string(),
                source_path: plugin_root.join(command).display().to_string(),
                enabled: true,
            });
        }
    }
    entries.sort_by(|left, right| left.hook_id.cmp(&right.hook_id));
    entries
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

fn default_enabled() -> bool {
    true
}
