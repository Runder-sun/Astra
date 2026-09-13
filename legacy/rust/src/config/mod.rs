use crate::projects::current::ResolvedProject;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveConfigReport {
    pub scope: String,
    pub effective: EffectiveConfig,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct EffectiveConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_failover: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_profiles: BTreeMap<String, ProviderProfileConfig>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub values: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSourceReport {
    pub sources: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overridden_key: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigValueResult {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderProfileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing, default)]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_env_var: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_completion_streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses_tool_schema: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    Global,
    Project,
    Private,
}

impl ConfigScope {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "global" => Some(Self::Global),
            "project" => Some(Self::Project),
            "private" => Some(Self::Private),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Private => "private",
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    KeyNotFound(String),
    InvalidConfigShape(PathBuf),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "config io failed: {err}"),
            Self::Parse(err) => write!(f, "config parse failed: {err}"),
            Self::KeyNotFound(key) => write!(f, "config key not found: {key}"),
            Self::InvalidConfigShape(path) => write!(
                f,
                "config file must contain a JSON object: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        Self::Parse(value)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ConfigFile {
    default_provider: Option<String>,
    default_model: Option<String>,
    provider_failover: Option<Vec<String>>,
    permission_mode: Option<String>,
    #[serde(default)]
    provider_profiles: BTreeMap<String, ProviderProfileConfig>,
    #[serde(default)]
    values: Map<String, Value>,
    #[serde(flatten, default)]
    extra: Map<String, Value>,
}

#[derive(Debug, Clone)]
struct LoadedConfig {
    effective: EffectiveConfig,
    sources: ConfigSourceReport,
    layers: Vec<ConfigLayer>,
}

#[derive(Debug, Clone)]
struct ConfigLayer {
    scope: &'static str,
    path: Option<PathBuf>,
    config: ConfigFile,
}

pub fn effective_config(resolved: &ResolvedProject) -> Result<EffectiveConfigReport, ConfigError> {
    let loaded = load_config_layers(resolved)?;
    Ok(EffectiveConfigReport {
        scope: "project".to_string(),
        effective: loaded.effective,
        project_id: resolved.project_id.clone(),
    })
}

pub fn config_sources(resolved: &ResolvedProject) -> Result<ConfigSourceReport, ConfigError> {
    let loaded = load_config_layers(resolved)?;
    Ok(loaded.sources)
}

pub fn config_get(resolved: &ResolvedProject, key: &str) -> Result<ConfigValueResult, ConfigError> {
    let loaded = load_config_layers(resolved)?;
    for layer in loaded.layers.iter().rev() {
        if let Some(value) = lookup_key(&layer.config, key) {
            return Ok(ConfigValueResult {
                key: key.to_string(),
                value: Some(value),
                value_source: Some(layer.scope.to_string()),
                resolved_scope: Some("project".to_string()),
                config_path: layer.path.as_ref().map(|path| path.display().to_string()),
                project_id: Some(resolved.project_id.clone()),
                workspace_root: Some(resolved.workspace_root.display().to_string()),
                resolution_source: Some(resolved.resolution_source.clone()),
            });
        }
    }

    Err(ConfigError::KeyNotFound(key.to_string()))
}

pub fn effective_value_source(
    resolved: &ResolvedProject,
    key: &str,
) -> Result<Option<String>, ConfigError> {
    let loaded = load_config_layers(resolved)?;
    for layer in loaded.layers.iter().rev() {
        if lookup_key(&layer.config, key).is_some() {
            return Ok(Some(layer.scope.to_string()));
        }
    }
    Ok(None)
}

pub fn config_set(
    resolved: &ResolvedProject,
    key: &str,
    raw_value: &str,
    scope: ConfigScope,
) -> Result<ConfigValueResult, ConfigError> {
    let path = config_path_for_scope(resolved, scope);
    let mut root = read_json_object(&path)?;
    let value = parse_config_value(raw_value);
    insert_key(&mut root, key, value.clone());
    write_json_object(&path, &root)?;

    Ok(ConfigValueResult {
        key: key.to_string(),
        value: Some(value),
        value_source: Some(scope_label(scope).to_string()),
        resolved_scope: Some("project".to_string()),
        config_path: Some(path.display().to_string()),
        project_id: Some(resolved.project_id.clone()),
        workspace_root: Some(resolved.workspace_root.display().to_string()),
        resolution_source: Some(resolved.resolution_source.clone()),
    })
}

pub fn global_provider_profiles() -> Result<BTreeMap<String, ProviderProfileConfig>, ConfigError> {
    let path = config_home_dir()?.join("settings.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents = fs::read_to_string(&path)?;
    if contents.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    let config: ConfigFile = serde_json::from_str(&contents)?;
    Ok(config.provider_profiles)
}

pub fn global_provider_profile(
    profile_name: &str,
) -> Result<Option<ProviderProfileConfig>, ConfigError> {
    Ok(provider_profile_for_provider(&global_provider_profiles()?, profile_name).cloned())
}

pub fn effective_provider_profile(
    resolved: &ResolvedProject,
    provider_id: &str,
) -> Result<Option<ProviderProfileConfig>, ConfigError> {
    let loaded = load_config_layers(resolved)?;
    Ok(provider_profile_for_provider(&loaded.effective.provider_profiles, provider_id).cloned())
}

pub fn provider_profile_for_provider<'a>(
    profiles: &'a BTreeMap<String, ProviderProfileConfig>,
    provider_id: &str,
) -> Option<&'a ProviderProfileConfig> {
    profiles.get(provider_id)
}

fn load_config_layers(resolved: &ResolvedProject) -> Result<LoadedConfig, ConfigError> {
    let global_path = config_home_dir()?.join("settings.json");
    let project_path = resolved.data_dir.join("settings.json");
    let private_path = resolved.data_dir.join("settings.local.json");

    let mut layers = Vec::new();
    if let Some(config) = read_optional_config(&global_path)? {
        layers.push(ConfigLayer {
            scope: "global",
            path: Some(global_path.clone()),
            config,
        });
    }
    if let Some(config) = read_optional_config(&project_path)? {
        layers.push(ConfigLayer {
            scope: "project",
            path: Some(project_path.clone()),
            config,
        });
    }
    if let Some(config) = read_optional_config(&private_path)? {
        layers.push(ConfigLayer {
            scope: "private",
            path: Some(private_path.clone()),
            config,
        });
    }
    if let Some(config) = environment_config() {
        layers.push(ConfigLayer {
            scope: "environment",
            path: None,
            config,
        });
    }

    let mut effective = EffectiveConfig::default();
    let mut overridden = Vec::new();
    let mut seen = ConfigFile::default();
    for layer in &layers {
        apply_config(&layer.config, &mut effective, &seen, &mut overridden);
        remember_seen_config(&layer.config, &mut seen);
    }

    let mut sources = BTreeMap::new();
    for layer in &layers {
        let source = layer
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "process_environment".to_string());
        sources.insert(layer.scope.to_string(), source);
    }

    Ok(LoadedConfig {
        effective,
        sources: ConfigSourceReport {
            sources,
            overridden_key: overridden,
        },
        layers,
    })
}

fn environment_config() -> Option<ConfigFile> {
    let default_provider = env_override("RESEARCH_CLI_PROVIDER");
    let default_model = env_override("RESEARCH_CLI_MODEL");
    let provider_failover = env_list_override("RESEARCH_CLI_PROVIDER_FAILOVER");
    let permission_mode = env_override("RESEARCH_CLI_PERMISSION_MODE");

    if default_provider.is_none()
        && default_model.is_none()
        && provider_failover.is_none()
        && permission_mode.is_none()
    {
        return None;
    }

    Some(ConfigFile {
        default_provider,
        default_model,
        provider_failover,
        permission_mode,
        provider_profiles: BTreeMap::new(),
        values: Map::new(),
        extra: Map::new(),
    })
}

pub fn env_override(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_list_override(name: &str) -> Option<Vec<String>> {
    env::var(name).ok().map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    })
}

fn read_optional_config(path: &Path) -> Result<Option<ConfigFile>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&contents)?))
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, ConfigError> {
    if !path.exists() {
        return Ok(Map::new());
    }

    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(&contents)?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| ConfigError::InvalidConfigShape(path.to_path_buf()))
}

fn write_json_object(path: &Path, root: &Map<String, Value>) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(root)?)?;
    Ok(())
}

fn apply_config(
    config: &ConfigFile,
    effective: &mut EffectiveConfig,
    previous: &ConfigFile,
    overridden: &mut Vec<String>,
) {
    merge_field(
        &mut effective.default_provider,
        config.default_provider.clone(),
        previous.default_provider.clone(),
        "default_provider",
        overridden,
    );
    merge_field(
        &mut effective.default_model,
        config.default_model.clone(),
        previous.default_model.clone(),
        "default_model",
        overridden,
    );
    merge_vec_field(
        &mut effective.provider_failover,
        config.provider_failover.as_ref(),
        previous.provider_failover.as_ref(),
        "provider_failover",
        overridden,
    );
    merge_field(
        &mut effective.permission_mode,
        config.permission_mode.clone(),
        previous.permission_mode.clone(),
        "permission_mode",
        overridden,
    );

    for (profile_name, profile) in &config.provider_profiles {
        if previous.provider_profiles.contains_key(profile_name)
            && !overridden
                .iter()
                .any(|item| item == &format!("provider_profiles.{profile_name}"))
        {
            overridden.push(format!("provider_profiles.{profile_name}"));
        }
        effective
            .provider_profiles
            .insert(profile_name.clone(), profile.clone());
    }

    for (key, value) in config.values.iter().chain(config.extra.iter()) {
        let prior = previous.values.get(key).or_else(|| previous.extra.get(key));
        if prior.is_some()
            && !overridden
                .iter()
                .any(|item| item == &format!("values.{key}"))
        {
            overridden.push(format!("values.{key}"));
        }
        effective.values.insert(key.clone(), value.clone());
    }
}

fn remember_seen_config(config: &ConfigFile, seen: &mut ConfigFile) {
    if config.default_provider.is_some() {
        seen.default_provider = config.default_provider.clone();
    }
    if config.default_model.is_some() {
        seen.default_model = config.default_model.clone();
    }
    if config.provider_failover.is_some() {
        seen.provider_failover = config.provider_failover.clone();
    }
    if config.permission_mode.is_some() {
        seen.permission_mode = config.permission_mode.clone();
    }
    for (profile_name, profile) in &config.provider_profiles {
        seen.provider_profiles
            .insert(profile_name.clone(), profile.clone());
    }
    for (key, value) in config.values.iter().chain(config.extra.iter()) {
        seen.values.insert(key.clone(), value.clone());
    }
}

fn merge_field(
    slot: &mut Option<String>,
    candidate: Option<String>,
    previous: Option<String>,
    field_name: &str,
    overridden: &mut Vec<String>,
) {
    if candidate.is_none() {
        return;
    }

    if previous.is_some() && !overridden.iter().any(|item| item == field_name) {
        overridden.push(field_name.to_string());
    }
    *slot = candidate;
}

fn merge_vec_field(
    slot: &mut Vec<String>,
    candidate: Option<&Vec<String>>,
    previous: Option<&Vec<String>>,
    field_name: &str,
    overridden: &mut Vec<String>,
) {
    let Some(candidate) = candidate else {
        return;
    };

    if previous.is_some() && !overridden.iter().any(|item| item == field_name) {
        overridden.push(field_name.to_string());
    }
    *slot = candidate.to_vec();
}

fn lookup_key(config: &ConfigFile, key: &str) -> Option<Value> {
    match key {
        "default_provider" => config.default_provider.clone().map(Value::String),
        "default_model" => config.default_model.clone().map(Value::String),
        "provider_failover" => config
            .provider_failover
            .as_ref()
            .map(|providers| Value::Array(providers.iter().cloned().map(Value::String).collect())),
        "permission_mode" => config.permission_mode.clone().map(Value::String),
        "provider_profiles" => serde_json::to_value(&config.provider_profiles).ok(),
        _ => config
            .values
            .get(key)
            .cloned()
            .or_else(|| config.extra.get(key).cloned()),
    }
}

fn insert_key(root: &mut Map<String, Value>, key: &str, value: Value) {
    root.insert(key.to_string(), value);
}

fn parse_config_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn config_path_for_scope(resolved: &ResolvedProject, scope: ConfigScope) -> PathBuf {
    match scope {
        ConfigScope::Global => config_home_dir()
            .expect("config home should resolve")
            .join("settings.json"),
        ConfigScope::Project => resolved.data_dir.join("settings.json"),
        ConfigScope::Private => resolved.data_dir.join("settings.local.json"),
    }
}

fn scope_label(scope: ConfigScope) -> &'static str {
    match scope {
        ConfigScope::Global => "global",
        ConfigScope::Project => "project",
        ConfigScope::Private => "private",
    }
}

fn config_home_dir() -> Result<PathBuf, ConfigError> {
    if let Ok(path) = std::env::var("RESEARCH_CLI_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }

    if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("research-cli"));
    }

    let home = std::env::var("HOME").map_err(|err| {
        ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("HOME is not set: {err}"),
        ))
    })?;
    Ok(PathBuf::from(home).join(".config").join("research-cli"))
}
