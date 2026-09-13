use crate::config::{self, ProviderProfileConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Context injection types (Phase 1)
// ---------------------------------------------------------------------------

/// A single message in the OpenAI-compatible chat messages array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call entry within an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatToolCall {
    pub id: String,
    pub r#type: String,
    pub function: ChatToolFunction,
}

/// The function name + arguments within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatToolFunction {
    pub name: String,
    pub arguments: String,
}

/// Per-model context window configuration for token budgeting.
pub struct ModelWindowConfig {
    pub context_window: usize,
    pub output_reservation: usize,
    pub system_budget: usize,
    pub history_soft_limit: f64,
}

/// Stable system prompt prefix (cache-friendly).
const SYSTEM_STABLE: &str = "You are Research CLI, a concise coding and research assistant.\n\
    You help users write, debug, and reason about code.\n\
    Answer directly and concisely. Prefer code over explanation.\n\
    When using tools, be precise with file paths and arguments.";

/// Dynamic boundary marker between stable and per-turn system content.
const SYSTEM_DYNAMIC_BOUNDARY: &str = "-- dynamic context --";

/// Return the model window config for the given model name prefix.
pub fn model_window_config(model: &str) -> ModelWindowConfig {
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("gpt-5") {
        return ModelWindowConfig {
            context_window: 200_000,
            output_reservation: 4_096,
            system_budget: 10_000,
            history_soft_limit: 0.80,
        };
    }
    if lower.starts_with("gpt-4o") {
        return ModelWindowConfig {
            context_window: 128_000,
            output_reservation: 4_096,
            system_budget: 10_000,
            history_soft_limit: 0.80,
        };
    }
    if lower.starts_with("claude-sonnet") || lower.starts_with("claude-opus") {
        return ModelWindowConfig {
            context_window: 200_000,
            output_reservation: 8_192,
            system_budget: 12_000,
            history_soft_limit: 0.80,
        };
    }
    // Default fallback for unknown models.
    ModelWindowConfig {
        context_window: 32_000,
        output_reservation: 4_096,
        system_budget: 8_000,
        history_soft_limit: 0.80,
    }
}

/// Rough token estimation (conservative char/3 approximation).
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(3)
}

/// Build the system messages (stable prefix + dynamic boundary + environment).
pub fn build_system_messages() -> Vec<ChatMessage> {
    let stable = ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_STABLE.to_string(),
        tool_calls: None,
        tool_call_id: None,
    };
    let mut dynamic_parts = vec![SYSTEM_DYNAMIC_BOUNDARY.to_string()];
    if let Ok(cwd) = env::var("PWD").or_else(|_| env::var("HOME")) {
        dynamic_parts.push(format!("Working directory: {cwd}"));
    }
    if let Ok(branch) = get_git_branch() {
        dynamic_parts.push(format!("Git branch: {branch}"));
    }
    let dynamic = ChatMessage {
        role: "system".to_string(),
        content: dynamic_parts.join("\n"),
        tool_calls: None,
        tool_call_id: None,
    };
    vec![stable, dynamic]
}

fn get_git_branch() -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            return Ok(branch);
        }
    }
    Err("not in git repo".to_string())
}

// End context injection types

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResolutionTrace {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub requested_model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub requested_provider: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model_alias_applied: String,
    pub routed_by_prefix: bool,
    pub resolved_provider: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resolved_model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resolution_reason: String,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, String>,

    // Legacy compatibility fields kept until provider inspect fixtures migrate.
    pub provider_id: String,
    pub auth_status: String,
    pub auth_source: String,
    pub auth_env_var: String,
    pub auth_shape: String,
    #[serde(skip_serializing, default)]
    pub auth_value: Option<String>,
    pub base_url: String,
    pub base_url_source: String,
    pub catalog_source: String,
    pub supported_models: Vec<String>,
    pub api_surface: String,
    pub api_surface_source: String,
    pub responses_tool_schema: String,
    pub responses_tool_schema_source: String,
    pub chat_completion_streaming: bool,
    pub chat_completion_streaming_source: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub degraded_features: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub provider_id: String,
    pub auth_status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub degraded_features: Vec<String>,
    pub supported_models: Vec<String>,
    pub catalog_source: String,
    pub base_url_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatusList {
    pub providers: Vec<ProviderStatus>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderCatalogState {
    pub provider_id: String,
    pub catalog_source: String,
    pub catalog_version: String,
    pub embedded: bool,
    pub refreshable: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_refreshed_at: String,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAuthStatusEntry {
    pub provider_id: String,
    pub ready: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub auth_source: String,
    pub auth_shape: String,
    pub base_url: String,
    pub base_url_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAuthStatusResult {
    pub providers: Vec<ProviderAuthStatusEntry>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTransportStatus {
    pub lane: String,
    pub endpoint: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub provider_id: String,
    pub overall_status: String,
    pub auth_status: String,
    pub live_ready: bool,
    pub transport: ProviderTransportStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderCatalogRefreshResult {
    pub requested_source: String,
    pub applied_source: String,
    pub catalog_version: String,
    pub total_count: usize,
    pub providers: Vec<ProviderCatalogState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCatalogEntry {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alias: String,
    pub canonical_model_id: String,
    pub provider_id: String,
    pub degraded: bool,
    pub default_selected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCatalogList {
    pub models: Vec<ModelCatalogEntry>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub current_model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub current_provider: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub degraded_providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCurrentResult {
    pub provider_id: String,
    pub model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resolved_scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub config_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderPromptCompletion {
    pub content: String,
    pub execution_mode: String,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTimeoutPolicy {
    Interactive,
    Blocking,
    TaskBudget(Duration),
}

impl ProviderTimeoutPolicy {
    fn global_timeout(self) -> Option<Duration> {
        match self {
            Self::Interactive => Some(Duration::from_secs(180)),
            Self::Blocking => None,
            Self::TaskBudget(limit) => Some(limit),
        }
    }

    fn recv_response_timeout(self) -> Option<Duration> {
        match self {
            Self::Interactive => Some(Duration::from_secs(120)),
            Self::Blocking => None,
            Self::TaskBudget(limit) => Some(limit),
        }
    }

    fn recv_body_timeout(self) -> Option<Duration> {
        match self {
            Self::Interactive => Some(Duration::from_secs(120)),
            Self::Blocking => None,
            Self::TaskBudget(limit) => Some(limit),
        }
    }
}

#[derive(Debug)]
struct OpenAiChatCompletionBody {
    content: String,
    tool_calls: Vec<ChatToolCall>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderCatalogEntry {
    pub provider_id: &'static str,
    pub auth_env_var: &'static str,
    pub base_url_env_var: &'static str,
    pub default_base_url: &'static str,
    pub supported_models: &'static [&'static str],
}

#[derive(Debug)]
pub enum ProviderInspectError {
    UnknownProvider(String),
}

impl fmt::Display for ProviderInspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProvider(provider_id) => write!(f, "unknown provider: {provider_id}"),
        }
    }
}

impl std::error::Error for ProviderInspectError {}

#[derive(Debug)]
pub enum ProviderResolveError {
    UnknownProvider(String),
    UnsupportedModel(String),
    IncompatibleProviderModel { provider_id: String, model: String },
}

impl fmt::Display for ProviderResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProvider(provider_id) => write!(f, "unknown provider: {provider_id}"),
            Self::UnsupportedModel(model) => write!(f, "unsupported model: {model}"),
            Self::IncompatibleProviderModel { provider_id, model } => {
                write!(f, "provider {provider_id} does not support model {model}")
            }
        }
    }
}

impl std::error::Error for ProviderResolveError {}

#[derive(Debug)]
pub enum ProviderTestError {
    UnknownProvider(String),
    AuthMissing {
        provider_id: String,
        env_var: String,
    },
    TransportBlocked {
        provider_id: String,
        transport: ProviderTransportStatus,
        message: String,
    },
}

impl fmt::Display for ProviderTestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProvider(provider_id) => write!(f, "unknown provider: {provider_id}"),
            Self::AuthMissing {
                provider_id,
                env_var,
            } => write!(
                f,
                "provider {provider_id} is missing required credential: {env_var}"
            ),
            Self::TransportBlocked {
                provider_id,
                transport: _,
                message,
            } => write!(
                f,
                "provider {provider_id} transport check failed: {message}"
            ),
        }
    }
}

impl std::error::Error for ProviderTestError {}

#[derive(Debug)]
pub enum ProviderExecutionError {
    UnsupportedProvider(String),
    AuthMissing {
        provider_id: String,
        env_var: String,
    },
    Transport(String),
    HttpStatus {
        status: u16,
        body: String,
    },
    LiveProviderRequired {
        provider_id: String,
        model: String,
        hint: String,
    },
    Parse(String),
    EmptyResponse,
    EmptyResponseWithDiagnostics {
        summary: String,
    },
    Cancelled {
        partial_content: String,
    },
}

impl fmt::Display for ProviderExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProvider(provider_id) => {
                write!(
                    f,
                    "provider {provider_id} does not support prompt execution yet"
                )
            }
            Self::AuthMissing {
                provider_id,
                env_var,
            } => write!(
                f,
                "provider {provider_id} is missing required credential: {env_var}"
            ),
            Self::Transport(message) => write!(f, "provider request failed: {message}"),
            Self::HttpStatus { status, body } => {
                write!(f, "provider returned HTTP {status}: {body}")
            }
            Self::LiveProviderRequired {
                provider_id,
                model,
                hint,
            } => write!(
                f,
                "provider {provider_id} model {model} is not configured for live execution: {hint}"
            ),
            Self::Parse(message) => write!(f, "provider response parse failed: {message}"),
            Self::EmptyResponse => write!(f, "provider response did not contain assistant text"),
            Self::EmptyResponseWithDiagnostics { summary } => write!(
                f,
                "provider response did not contain assistant text: {summary}"
            ),
            Self::Cancelled { .. } => write!(f, "provider request was cancelled"),
        }
    }
}

impl std::error::Error for ProviderExecutionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderFailureDisposition {
    RetryWithBackoff,
    OperatorGate,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderFailurePolicy {
    pub category: String,
    pub disposition: ProviderFailureDisposition,
    pub retryable: bool,
    pub operator_gate_required: bool,
    pub base_backoff_ms: u64,
    pub summary: String,
}

pub const PROVIDER_FAILURE_BACKOFF_CAP_MS: u64 = 30 * 60 * 1_000;
pub const PROVIDER_FAILURE_QUOTA_EXHAUSTED_BACKOFF_MS: u64 = 6 * 60 * 60 * 1_000;

pub fn classify_provider_execution_error(error: &ProviderExecutionError) -> ProviderFailurePolicy {
    match error {
        ProviderExecutionError::UnsupportedProvider(provider_id) => ProviderFailurePolicy {
            category: "unsupported_provider".to_string(),
            disposition: ProviderFailureDisposition::OperatorGate,
            retryable: false,
            operator_gate_required: true,
            base_backoff_ms: 0,
            summary: format!("provider {provider_id} is unsupported for prompt execution"),
        },
        ProviderExecutionError::AuthMissing {
            provider_id,
            env_var,
        } => ProviderFailurePolicy {
            category: "auth_missing".to_string(),
            disposition: ProviderFailureDisposition::OperatorGate,
            retryable: false,
            operator_gate_required: true,
            base_backoff_ms: 0,
            summary: format!("provider {provider_id} is missing required credential: {env_var}"),
        },
        ProviderExecutionError::Transport(message) => {
            let lowered = message.to_ascii_lowercase();
            let retryable = provider_transport_error_is_retryable(&lowered);
            ProviderFailurePolicy {
                category: if retryable {
                    "transport_retryable".to_string()
                } else {
                    "transport_error".to_string()
                },
                disposition: if retryable {
                    ProviderFailureDisposition::RetryWithBackoff
                } else {
                    ProviderFailureDisposition::OperatorGate
                },
                retryable,
                operator_gate_required: !retryable,
                base_backoff_ms: if retryable { 60_000 } else { 0 },
                summary: format!("provider request failed: {message}"),
            }
        }
        ProviderExecutionError::HttpStatus { status, body } => {
            let lowered_body = body.to_ascii_lowercase();
            let auth_invalid =
                provider_http_status_body_is_auth_invalid(*status, body, &lowered_body);
            let quota_exhausted = !auth_invalid
                && *status != 429
                && provider_http_status_body_is_quota_exhausted(body, &lowered_body);
            let provider_downstream_retryable = !auth_invalid
                && provider_http_status_body_is_retryable(*status, body, &lowered_body);
            let retryable = !auth_invalid
                && (quota_exhausted
                    || *status == 429
                    || (500..600).contains(status)
                    || provider_downstream_retryable);
            ProviderFailurePolicy {
                category: if auth_invalid {
                    "auth_invalid".to_string()
                } else if quota_exhausted {
                    "quota_exhausted".to_string()
                } else if *status == 429 {
                    "rate_limit".to_string()
                } else if retryable {
                    if provider_downstream_retryable && !(500..600).contains(status) {
                        "provider_downstream_error".to_string()
                    } else {
                        "http_5xx".to_string()
                    }
                } else {
                    "http_status".to_string()
                },
                disposition: if retryable {
                    ProviderFailureDisposition::RetryWithBackoff
                } else {
                    ProviderFailureDisposition::OperatorGate
                },
                retryable,
                operator_gate_required: !retryable,
                base_backoff_ms: if quota_exhausted {
                    PROVIDER_FAILURE_QUOTA_EXHAUSTED_BACKOFF_MS
                } else if *status == 429 {
                    5 * 60_000
                } else if retryable {
                    60_000
                } else {
                    0
                },
                summary: format!("provider returned HTTP {status}: {body}"),
            }
        }
        ProviderExecutionError::LiveProviderRequired {
            provider_id,
            model,
            hint,
        } => ProviderFailurePolicy {
            category: "live_provider_required".to_string(),
            disposition: ProviderFailureDisposition::OperatorGate,
            retryable: false,
            operator_gate_required: true,
            base_backoff_ms: 0,
            summary: format!(
                "provider {provider_id} model {model} is not configured for live execution: {hint}"
            ),
        },
        ProviderExecutionError::Parse(message) => ProviderFailurePolicy {
            category: "parse_error".to_string(),
            disposition: ProviderFailureDisposition::RetryWithBackoff,
            retryable: true,
            operator_gate_required: false,
            base_backoff_ms: 30_000,
            summary: format!("provider response parse failed: {message}"),
        },
        ProviderExecutionError::EmptyResponse => ProviderFailurePolicy {
            category: "empty_response".to_string(),
            disposition: ProviderFailureDisposition::RetryWithBackoff,
            retryable: true,
            operator_gate_required: false,
            base_backoff_ms: 30_000,
            summary: "provider response did not contain assistant text".to_string(),
        },
        ProviderExecutionError::EmptyResponseWithDiagnostics { summary } => ProviderFailurePolicy {
            category: "empty_response".to_string(),
            disposition: ProviderFailureDisposition::RetryWithBackoff,
            retryable: true,
            operator_gate_required: false,
            base_backoff_ms: 30_000,
            summary: format!("provider response did not contain assistant text: {summary}"),
        },
        ProviderExecutionError::Cancelled { .. } => ProviderFailurePolicy {
            category: "cancelled".to_string(),
            disposition: ProviderFailureDisposition::Cancel,
            retryable: false,
            operator_gate_required: false,
            base_backoff_ms: 0,
            summary: "provider request was cancelled".to_string(),
        },
    }
}

fn provider_http_status_body_is_quota_exhausted(body: &str, lowered_body: &str) -> bool {
    lowered_body.contains("quota")
        || lowered_body.contains("insufficient_quota")
        || lowered_body.contains("usage limit")
        || lowered_body.contains("usage_limit")
        || lowered_body.contains("monthly limit")
        || lowered_body.contains("weekly limit")
        || lowered_body.contains("credit exhausted")
        || body.contains("使用上限")
        || body.contains("每周")
        || body.contains("每月")
}

fn provider_http_status_body_is_auth_invalid(status: u16, body: &str, lowered_body: &str) -> bool {
    if !matches!(status, 400 | 401 | 403) {
        return false;
    }
    lowered_body.contains("coding_plan_app_key_not_exist")
        || lowered_body.contains("app_key_not_exist")
        || lowered_body.contains("apikey_not_exist")
        || lowered_body.contains("api_key_not_exist")
        || lowered_body.contains("invalid_api_key")
        || lowered_body.contains("invalid api key")
        || lowered_body.contains("invalid app key")
        || lowered_body.contains("invalid token")
        || lowered_body.contains("invalid_token")
        || lowered_body.contains("expired api key")
        || lowered_body.contains("expired token")
        || body.contains("AppKey不存在")
        || body.contains("AppKey不存在或已失效")
        || body.contains("密钥不存在")
        || body.contains("密钥已失效")
        || body.contains("凭证无效")
        || body.contains("令牌无效")
}

fn provider_http_status_body_is_retryable(status: u16, body: &str, lowered_body: &str) -> bool {
    if status == 429 || (500..600).contains(&status) {
        return true;
    }
    lowered_body.contains("down_stream_model_error")
        || lowered_body.contains("downstream_model_error")
        || lowered_body.contains("downstream model")
        || lowered_body.contains("downstream")
        || lowered_body.contains("upstream")
        || lowered_body.contains("network error")
        || lowered_body.contains("temporarily unavailable")
        || lowered_body.contains("service unavailable")
        || lowered_body.contains("timeout")
        || lowered_body.contains("timed out")
        || body.contains("网络错误")
        || body.contains("网络异常")
        || body.contains("服务暂不可用")
        || body.contains("下游")
}

fn provider_transport_error_is_retryable(lowered_message: &str) -> bool {
    lowered_message.contains("timeout")
        || lowered_message.contains("timed out")
        || lowered_message.contains("connection refused")
        || lowered_message.contains("connection reset")
        || lowered_message.contains("connection closed")
        || lowered_message.contains("peer closed connection")
        || lowered_message.contains("peer disconnected")
        || lowered_message.contains("temporarily unavailable")
        || lowered_message.contains("temporary failure in name resolution")
        || lowered_message.contains("failed to lookup address information")
        || lowered_message.contains("lookup address")
        || lowered_message.contains("name resolution")
        || lowered_message.contains("network")
        || lowered_message.contains("dns")
        || lowered_message.contains("tls")
        || lowered_message.contains("close_notify")
        || lowered_message.contains("unexpected-eof")
        || lowered_message.contains("unexpected eof")
}

pub fn provider_failure_backoff_ms(
    policy: &ProviderFailurePolicy,
    consecutive_failures: usize,
) -> u64 {
    if !policy.retryable {
        return 0;
    }
    if policy.category == "quota_exhausted" {
        return policy.base_backoff_ms;
    }
    let failures = consecutive_failures.max(1);
    let shift = failures.saturating_sub(1).min(5) as u32;
    let multiplier = 1u64 << shift;
    policy
        .base_backoff_ms
        .saturating_mul(multiplier)
        .min(PROVIDER_FAILURE_BACKOFF_CAP_MS)
}

#[derive(Debug)]
pub enum ProviderCatalogError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    Invalid(String),
}

impl fmt::Display for ProviderCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Parse(err) => write!(f, "{err}"),
            Self::Invalid(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for ProviderCatalogError {}

impl From<std::io::Error> for ProviderCatalogError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ProviderCatalogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Parse(value)
    }
}

const PROVIDER_CATALOG: &[ProviderCatalogEntry] = &[
    ProviderCatalogEntry {
        provider_id: "openai",
        auth_env_var: "OPENAI_API_KEY",
        base_url_env_var: "OPENAI_BASE_URL",
        default_base_url: "https://api.openai.com/v1",
        supported_models: &[
            "gpt-5.4",
            "gpt-5.5",
            "gpt-5.4-mini",
            "gpt-5",
            "gpt-5-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
        ],
    },
    ProviderCatalogEntry {
        provider_id: "anthropic",
        auth_env_var: "ANTHROPIC_API_KEY",
        base_url_env_var: "ANTHROPIC_BASE_URL",
        default_base_url: "https://api.anthropic.com",
        supported_models: &[
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251213",
        ],
    },
];

const MODEL_ALIASES: &[(&str, &str)] = &[
    ("smart", "gpt-5.4"),
    ("fast", "gpt-5.4-mini"),
    ("cheap", "gpt-4.1-mini"),
];

pub fn inspect_provider(provider: &str) -> Result<ProviderResolutionTrace, ProviderInspectError> {
    let route = resolve_provider_route(provider)
        .map_err(|_| ProviderInspectError::UnknownProvider(provider.to_string()))?;
    Ok(build_provider_trace(
        route.entry,
        route.route_provider_id,
        route.profile.as_ref(),
        provider.to_string(),
        String::new(),
        String::new(),
        route
            .snapshot
            .default_model
            .clone()
            .or_else(|| {
                route
                    .entry
                    .supported_models
                    .first()
                    .map(|model| (*model).to_string())
            })
            .unwrap_or_default(),
        "explicit_provider".to_string(),
        false,
        Some("cli_flag"),
        None,
    ))
}

pub fn list_providers() -> ProviderStatusList {
    let mut providers = PROVIDER_CATALOG
        .iter()
        .map(|entry| {
            let snapshot = provider_snapshot(entry, None);
            ProviderStatus {
                provider_id: entry.provider_id.to_string(),
                auth_status: snapshot.auth_status,
                degraded_features: Vec::new(),
                supported_models: entry
                    .supported_models
                    .iter()
                    .map(|model| (*model).to_string())
                    .collect(),
                catalog_source: "embedded".to_string(),
                base_url_source: snapshot.base_url_source,
            }
        })
        .collect::<Vec<_>>();
    for (profile_name, profile) in config::global_provider_profiles().unwrap_or_default() {
        let adapter_id = profile.provider_id.as_deref().unwrap_or(&profile_name);
        let Some(entry) = provider_catalog_entry(adapter_id) else {
            continue;
        };
        let snapshot = provider_snapshot(entry, Some(&profile));
        providers.push(ProviderStatus {
            provider_id: profile_name,
            auth_status: snapshot.auth_status,
            degraded_features: Vec::new(),
            supported_models: snapshot.supported_models,
            catalog_source: snapshot.catalog_source,
            base_url_source: snapshot.base_url_source,
        });
    }

    ProviderStatusList {
        total_count: providers.len(),
        providers,
    }
}

pub fn provider_auth_status() -> ProviderAuthStatusResult {
    let mut providers = PROVIDER_CATALOG
        .iter()
        .map(|entry| {
            let snapshot = provider_snapshot(entry, None);
            ProviderAuthStatusEntry {
                provider_id: entry.provider_id.to_string(),
                ready: snapshot.auth_status == "configured",
                status: if snapshot.auth_status == "configured" {
                    "ready".to_string()
                } else {
                    "degraded".to_string()
                },
                hint: if snapshot.auth_status == "configured" {
                    None
                } else {
                    Some(format!("Set {}", snapshot.auth_env_var))
                },
                auth_source: snapshot.auth_source,
                auth_shape: snapshot.auth_shape,
                base_url: snapshot.base_url,
                base_url_source: snapshot.base_url_source,
            }
        })
        .collect::<Vec<_>>();
    for (profile_name, profile) in config::global_provider_profiles().unwrap_or_default() {
        let adapter_id = profile.provider_id.as_deref().unwrap_or(&profile_name);
        let Some(entry) = provider_catalog_entry(adapter_id) else {
            continue;
        };
        let snapshot = provider_snapshot(entry, Some(&profile));
        providers.push(ProviderAuthStatusEntry {
            provider_id: profile_name,
            ready: snapshot.auth_status == "configured",
            status: if snapshot.auth_status == "configured" {
                "ready".to_string()
            } else {
                "degraded".to_string()
            },
            hint: if snapshot.auth_status == "configured" {
                None
            } else {
                Some(format!("Set {}", snapshot.auth_env_var))
            },
            auth_source: snapshot.auth_source,
            auth_shape: snapshot.auth_shape,
            base_url: snapshot.base_url,
            base_url_source: snapshot.base_url_source,
        });
    }

    ProviderAuthStatusResult {
        total_count: providers.len(),
        providers,
    }
}

pub fn selected_provider() -> ProviderResolutionTrace {
    resolve_provider_trace(None, None, None, None)
        .expect("embedded provider catalog should resolve a default")
}

pub fn resolve_provider_trace(
    explicit_provider: Option<&str>,
    explicit_model: Option<&str>,
    explicit_provider_source: Option<&str>,
    explicit_model_source: Option<&str>,
) -> Result<ProviderResolutionTrace, ProviderResolveError> {
    resolve_provider_trace_with_profile_map(
        explicit_provider,
        explicit_model,
        explicit_provider_source,
        explicit_model_source,
        None,
    )
}

pub fn resolve_provider_trace_with_profiles(
    explicit_provider: Option<&str>,
    explicit_model: Option<&str>,
    explicit_provider_source: Option<&str>,
    explicit_model_source: Option<&str>,
    profiles: &BTreeMap<String, ProviderProfileConfig>,
) -> Result<ProviderResolutionTrace, ProviderResolveError> {
    resolve_provider_trace_with_profile_map(
        explicit_provider,
        explicit_model,
        explicit_provider_source,
        explicit_model_source,
        Some(profiles),
    )
}

fn resolve_provider_trace_with_profile_map(
    explicit_provider: Option<&str>,
    explicit_model: Option<&str>,
    explicit_provider_source: Option<&str>,
    explicit_model_source: Option<&str>,
    profiles: Option<&BTreeMap<String, ProviderProfileConfig>>,
) -> Result<ProviderResolutionTrace, ProviderResolveError> {
    let requested_provider = explicit_provider.unwrap_or_default().to_string();
    let requested_model = explicit_model.unwrap_or_default().to_string();
    let (aliased_model, model_alias_applied) =
        apply_model_alias(explicit_model.unwrap_or_default());
    let (prefixed_provider, normalized_model, routed_by_prefix) =
        parse_prefixed_model_with_profiles(&aliased_model, profiles);
    let alias_applied = !model_alias_applied.is_empty();

    let route_provider_id = if let Some(provider) = explicit_provider {
        provider.to_string()
    } else if let Some(provider) = prefixed_provider {
        provider
    } else if let Some(model) = explicit_model {
        provider_for_model_with_profiles(&aliased_model, profiles)
            .ok_or_else(|| ProviderResolveError::UnsupportedModel(model.to_string()))?
            .to_string()
    } else if has_env("OPENAI_API_KEY") {
        "openai".to_string()
    } else if has_env("ANTHROPIC_API_KEY") {
        "anthropic".to_string()
    } else {
        "openai".to_string()
    };

    let route = resolve_provider_route_with_profile_map(&route_provider_id, profiles)?;
    let entry = route.entry;
    let snapshot = route.snapshot.clone();

    let resolved_model = if explicit_model.is_some() {
        normalized_model.clone()
    } else {
        snapshot.default_model.clone().unwrap_or_default()
    };

    let custom_openai_compatible_model = explicit_model.is_some()
        && !supports_model(&snapshot.supported_models, &resolved_model)
        && allows_openai_compatible_custom_models(entry, &snapshot);

    if explicit_model.is_some()
        && !supports_model(&snapshot.supported_models, &resolved_model)
        && !custom_openai_compatible_model
    {
        return Err(ProviderResolveError::IncompatibleProviderModel {
            provider_id: route.route_provider_id.clone(),
            model: resolved_model,
        });
    }

    let resolution_reason = if custom_openai_compatible_model {
        "openai_compatible_custom_model"
    } else if explicit_provider.is_some() && explicit_model.is_some() {
        "explicit_provider_and_model"
    } else if explicit_provider.is_some() {
        "explicit_provider_default_model"
    } else if alias_applied {
        "model_alias_catalog_match"
    } else if routed_by_prefix {
        "model_prefix"
    } else if explicit_model.is_some() {
        "model_catalog_match"
    } else if has_env("OPENAI_API_KEY") || has_env("ANTHROPIC_API_KEY") {
        "ambient_credentials"
    } else {
        "default_provider"
    };

    Ok(build_provider_trace(
        entry,
        route.route_provider_id,
        route.profile.as_ref(),
        requested_provider,
        requested_model,
        model_alias_applied,
        resolved_model,
        resolution_reason.to_string(),
        routed_by_prefix,
        explicit_provider_source,
        explicit_model_source,
    ))
}

pub fn provider_catalog_entry(provider: &str) -> Option<&'static ProviderCatalogEntry> {
    PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.provider_id == provider)
}

pub fn provider_catalog_states() -> Vec<ProviderCatalogState> {
    let refreshed_at = timestamp_string();
    PROVIDER_CATALOG
        .iter()
        .map(|entry| ProviderCatalogState {
            provider_id: entry.provider_id.to_string(),
            catalog_source: "embedded".to_string(),
            catalog_version: "embedded-v1".to_string(),
            embedded: true,
            refreshable: true,
            last_refreshed_at: refreshed_at.clone(),
            model_count: entry.supported_models.len(),
        })
        .collect()
}

pub fn list_models(
    current_provider: Option<&str>,
    current_model: Option<&str>,
) -> ModelCatalogList {
    let current_provider = current_provider.unwrap_or_default().to_string();
    let current_model = current_model.unwrap_or_default().to_string();
    let degraded_providers = PROVIDER_CATALOG
        .iter()
        .filter(|entry| provider_snapshot(entry, None).auth_status != "configured")
        .map(|entry| entry.provider_id.to_string())
        .collect::<Vec<_>>();

    let mut models = MODEL_ALIASES
        .iter()
        .filter_map(|(alias, canonical)| {
            provider_for_model(canonical).map(|provider_id| {
                let route = resolve_provider_route(&provider_id)
                    .expect("catalog or profile should contain alias target provider");
                let entry = route.entry;
                let snapshot = provider_snapshot(entry, route.profile.as_ref());
                ModelCatalogEntry {
                    alias: (*alias).to_string(),
                    canonical_model_id: (*canonical).to_string(),
                    provider_id: provider_id.to_string(),
                    degraded: snapshot.auth_status != "configured",
                    default_selected: false,
                }
            })
        })
        .collect::<Vec<_>>();

    for entry in PROVIDER_CATALOG {
        let degraded = provider_snapshot(entry, None).auth_status != "configured";
        for model in entry.supported_models {
            models.push(ModelCatalogEntry {
                alias: String::new(),
                canonical_model_id: (*model).to_string(),
                provider_id: entry.provider_id.to_string(),
                degraded,
                default_selected: current_provider == entry.provider_id && current_model == *model,
            });
        }
    }

    ModelCatalogList {
        models,
        current_model,
        current_provider,
        degraded_providers,
    }
}

pub fn refresh_catalog(source: &str) -> ProviderCatalogRefreshResult {
    let providers = provider_catalog_states();
    ProviderCatalogRefreshResult {
        requested_source: source.to_string(),
        applied_source: if source == "embedded" {
            "embedded".to_string()
        } else {
            "unsupported".to_string()
        },
        catalog_version: "embedded-v1".to_string(),
        total_count: providers.len(),
        providers,
    }
}

pub fn refresh_catalog_from_file<P>(
    path: P,
) -> Result<ProviderCatalogRefreshResult, ProviderCatalogError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let raw = fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("file")
        .to_string();
    let provider_values = value
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            ProviderCatalogError::Invalid(
                "provider catalog file requires providers array".to_string(),
            )
        })?;
    let refreshed_at = timestamp_string();
    let mut providers = Vec::new();
    for provider in provider_values {
        let provider_id = provider
            .get("provider_id")
            .or_else(|| provider.get("id"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                ProviderCatalogError::Invalid(
                    "provider catalog entries require provider_id".to_string(),
                )
            })?;
        let models = provider
            .get("models")
            .or_else(|| provider.get("supported_models"))
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                ProviderCatalogError::Invalid(format!(
                    "provider catalog entry {provider_id} requires models array"
                ))
            })?;
        let model_count = models
            .iter()
            .filter(|model| model.as_str().is_some_and(|raw| !raw.trim().is_empty()))
            .count();
        providers.push(ProviderCatalogState {
            provider_id: provider_id.to_string(),
            catalog_source: "file".to_string(),
            catalog_version: version.clone(),
            embedded: false,
            refreshable: true,
            last_refreshed_at: refreshed_at.clone(),
            model_count,
        });
    }
    Ok(ProviderCatalogRefreshResult {
        requested_source: path.display().to_string(),
        applied_source: "file".to_string(),
        catalog_version: version,
        total_count: providers.len(),
        providers,
    })
}

pub fn test_provider(provider: Option<&str>) -> Result<ProviderTestResult, ProviderTestError> {
    let trace =
        resolve_provider_trace(provider, None, Some("cli_flag"), None).map_err(
            |err| match err {
                ProviderResolveError::UnknownProvider(provider_id) => {
                    ProviderTestError::UnknownProvider(provider_id)
                }
                ProviderResolveError::UnsupportedModel(model) => {
                    ProviderTestError::UnknownProvider(model)
                }
                ProviderResolveError::IncompatibleProviderModel { provider_id, .. } => {
                    ProviderTestError::UnknownProvider(provider_id)
                }
            },
        )?;
    let entry = provider_catalog_entry(&trace.provider_id)
        .ok_or_else(|| ProviderTestError::UnknownProvider(trace.provider_id.clone()))?;
    if trace.auth_status != "configured" {
        return Err(ProviderTestError::AuthMissing {
            provider_id: trace.resolved_provider.clone(),
            env_var: trace.auth_env_var,
        });
    }

    let transport = transport_probe(
        entry,
        &trace.base_url,
        &trace.auth_env_var,
        trace.auth_value.as_deref(),
    )
    .map_err(|failure| ProviderTestError::TransportBlocked {
        provider_id: trace.resolved_provider.clone(),
        transport: failure.transport,
        message: failure.message,
    })?;

    let live_ready = transport.status == "ready";
    let overall_status = if live_ready { "ready" } else { "degraded" };
    let mut hints = Vec::new();
    if !live_ready {
        hints.push(
            "HTTPS endpoints currently validate socket reachability only; use a local http:// base URL override to exercise the full request path deterministically."
                .to_string(),
        );
    }

    Ok(ProviderTestResult {
        provider_id: trace.resolved_provider,
        overall_status: overall_status.to_string(),
        auth_status: trace.auth_status,
        live_ready,
        transport,
        hints,
    })
}

pub fn complete_prompt(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
) -> Result<ProviderPromptCompletion, ProviderExecutionError> {
    complete_prompt_with_reasoning(trace, messages, None)
}

pub fn complete_prompt_with_reasoning(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
) -> Result<ProviderPromptCompletion, ProviderExecutionError> {
    complete_prompt_streaming_with_reasoning(trace, messages, reasoning_effort, |_| {})
}

pub fn complete_prompt_streaming<F>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    on_delta: F,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
{
    complete_prompt_streaming_with_reasoning(trace, messages, None, on_delta)
}

pub fn complete_prompt_streaming_with_reasoning<F>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    mut on_delta: F,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
{
    complete_prompt_streaming_with_cancel_and_reasoning(
        trace,
        messages,
        reasoning_effort,
        &mut on_delta,
        || false,
    )
}

pub fn complete_prompt_streaming_with_cancel<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    on_delta: F,
    is_cancelled: C,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    complete_prompt_streaming_with_cancel_and_reasoning(
        trace,
        messages,
        None,
        on_delta,
        is_cancelled,
    )
}

pub fn complete_prompt_streaming_with_cancel_and_reasoning<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    mut on_delta: F,
    is_cancelled: C,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    if !should_execute_live_prompt(trace) {
        let last_user_content = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let content = format!(
            "Completed prompt via {}: {}",
            trace.provider_id, last_user_content
        );
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: String::new(),
            });
        }
        on_delta(&content);
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        return Ok(ProviderPromptCompletion {
            content,
            execution_mode: "scaffold".to_string(),
            endpoint: String::new(),
            tool_calls: None,
            finish_reason: None,
        });
    }

    match trace.provider_id.as_str() {
        "openai" => complete_openai_chat_streaming_with_cancel_and_reasoning(
            trace,
            messages,
            reasoning_effort,
            on_delta,
            is_cancelled,
            ProviderTimeoutPolicy::Interactive,
        ),
        other => Err(ProviderExecutionError::UnsupportedProvider(
            other.to_string(),
        )),
    }
}

pub fn complete_prompt_streaming_live<F>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    on_delta: F,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
{
    complete_prompt_streaming_live_with_cancel_and_reasoning(
        trace,
        messages,
        None,
        on_delta,
        || false,
    )
}

pub fn complete_prompt_streaming_live_with_cancel<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    on_delta: F,
    is_cancelled: C,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    complete_prompt_streaming_live_with_cancel_and_reasoning(
        trace,
        messages,
        None,
        on_delta,
        is_cancelled,
    )
}

pub fn complete_prompt_streaming_live_with_cancel_and_reasoning<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    on_delta: F,
    is_cancelled: C,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    require_live_prompt(trace)?;
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    match trace.provider_id.as_str() {
        "openai" => complete_openai_chat_streaming_with_cancel_and_reasoning(
            trace,
            messages,
            reasoning_effort,
            on_delta,
            is_cancelled,
            ProviderTimeoutPolicy::Interactive,
        ),
        other => Err(ProviderExecutionError::UnsupportedProvider(
            other.to_string(),
        )),
    }
}

pub fn complete_prompt_streaming_live_blocking_with_cancel<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    on_delta: F,
    is_cancelled: C,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    complete_prompt_streaming_live_with_cancel_reasoning_and_timeout_policy(
        trace,
        messages,
        None,
        on_delta,
        is_cancelled,
        ProviderTimeoutPolicy::Blocking,
    )
}

pub fn complete_prompt_streaming_live_with_cancel_reasoning_and_timeout_policy<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    on_delta: F,
    is_cancelled: C,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    require_live_prompt(trace)?;
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    match trace.provider_id.as_str() {
        "openai" => complete_openai_chat_streaming_with_cancel_and_reasoning(
            trace,
            messages,
            reasoning_effort,
            on_delta,
            is_cancelled,
            timeout_policy,
        ),
        other => Err(ProviderExecutionError::UnsupportedProvider(
            other.to_string(),
        )),
    }
}

fn require_live_prompt(trace: &ProviderResolutionTrace) -> Result<(), ProviderExecutionError> {
    if trace.auth_status != "configured" {
        return Err(ProviderExecutionError::AuthMissing {
            provider_id: trace.provider_id.clone(),
            env_var: trace.auth_env_var.clone(),
        });
    }
    if trace.provider_id != "openai" {
        return Err(ProviderExecutionError::UnsupportedProvider(
            trace.provider_id.clone(),
        ));
    }
    if env::var("RESEARCH_CLI_LIVE_PROVIDER").ok().as_deref() == Some("1") {
        return Ok(());
    }
    let auth_value = provider_auth_value(trace).unwrap_or_default();
    if auth_value == "sk-test" && !is_loopback_base_url(&trace.base_url) {
        return Err(ProviderExecutionError::LiveProviderRequired {
            provider_id: trace.provider_id.clone(),
            model: trace.resolved_model.clone(),
            hint: format!(
                "{} is set to test credentials. Configure a real key or use a loopback OPENAI_BASE_URL for tests.",
                trace.auth_env_var
            ),
        });
    }
    Ok(())
}

pub(crate) fn should_execute_live_prompt(trace: &ProviderResolutionTrace) -> bool {
    if trace.auth_status != "configured" || trace.provider_id != "openai" {
        return false;
    }
    if env::var("RESEARCH_CLI_LIVE_PROVIDER").ok().as_deref() == Some("1") {
        return true;
    }
    let auth_value = provider_auth_value(trace).unwrap_or_default();
    if auth_value == "sk-test" && !is_loopback_base_url(&trace.base_url) {
        return false;
    }
    is_loopback_base_url(&trace.base_url)
        || trace.base_url_source == "config"
        || auth_value.starts_with("sk-")
        || trace.resolution_reason == "openai_compatible_custom_model"
}

fn require_live_tool_prompt(trace: &ProviderResolutionTrace) -> Result<(), ProviderExecutionError> {
    if should_execute_live_prompt(trace) {
        return Ok(());
    }
    if trace.auth_status != "configured" {
        return Err(ProviderExecutionError::AuthMissing {
            provider_id: trace.provider_id.clone(),
            env_var: trace.auth_env_var.clone(),
        });
    }
    Err(ProviderExecutionError::LiveProviderRequired {
        provider_id: trace.provider_id.clone(),
        model: trace.resolved_model.clone(),
        hint: "tool-capable agent loops require a live provider route; scaffold prompt completion is not valid for autonomous agents".to_string(),
    })
}

fn provider_auth_value(trace: &ProviderResolutionTrace) -> Option<String> {
    trace
        .auth_value
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            env::var(&trace.auth_env_var)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn is_loopback_base_url(base_url: &str) -> bool {
    base_url.starts_with("http://127.0.0.1:")
        || base_url.starts_with("http://localhost:")
        || base_url.starts_with("http://[::1]:")
}

/// Like `complete_prompt_streaming_with_cancel_and_reasoning` but also sends
/// tool definitions to the LLM API so it can invoke them.
pub fn complete_prompt_streaming_with_tools<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    tools: Option<serde_json::Value>,
    mut on_delta: F,
    is_cancelled: C,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    if tools.is_some() {
        require_live_tool_prompt(trace)?;
    } else if !should_execute_live_prompt(trace) {
        let last_user_content = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let content = format!(
            "Completed prompt via {}: {}",
            trace.provider_id, last_user_content
        );
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: String::new(),
            });
        }
        on_delta(&content);
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        return Ok(ProviderPromptCompletion {
            content,
            execution_mode: "scaffold".to_string(),
            endpoint: String::new(),
            tool_calls: None,
            finish_reason: None,
        });
    }
    match trace.provider_id.as_str() {
        "openai" => complete_openai_chat_streaming_internal(
            trace,
            messages,
            reasoning_effort,
            tools.as_ref(),
            on_delta,
            is_cancelled,
            ProviderTimeoutPolicy::Interactive,
        ),
        other => Err(ProviderExecutionError::UnsupportedProvider(
            other.to_string(),
        )),
    }
}

pub fn complete_prompt_streaming_with_tools_and_timeout_policy<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    tools: Option<serde_json::Value>,
    on_delta: F,
    is_cancelled: C,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    if tools.is_some() {
        require_live_tool_prompt(trace)?;
    } else if !should_execute_live_prompt(trace) {
        return complete_prompt_streaming_with_tools(
            trace,
            messages,
            reasoning_effort,
            tools,
            on_delta,
            is_cancelled,
        );
    }
    match trace.provider_id.as_str() {
        "openai" => complete_openai_chat_streaming_internal(
            trace,
            messages,
            reasoning_effort,
            tools.as_ref(),
            on_delta,
            is_cancelled,
            timeout_policy,
        ),
        other => Err(ProviderExecutionError::UnsupportedProvider(
            other.to_string(),
        )),
    }
}

fn complete_openai_chat_streaming_with_cancel_and_reasoning<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    on_delta: F,
    is_cancelled: C,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    complete_openai_chat_streaming_internal(
        trace,
        messages,
        reasoning_effort,
        None,
        on_delta,
        is_cancelled,
        timeout_policy,
    )
}

fn complete_openai_chat_streaming_internal<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    tools: Option<&serde_json::Value>,
    mut on_delta: F,
    is_cancelled: C,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if trace.api_surface == "responses" {
        return complete_openai_responses_internal(
            trace,
            messages,
            reasoning_effort,
            tools,
            on_delta,
            is_cancelled,
            timeout_policy,
        );
    }
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    let auth_value =
        provider_auth_value(trace).ok_or_else(|| ProviderExecutionError::AuthMissing {
            provider_id: trace.provider_id.clone(),
            env_var: trace.auth_env_var.clone(),
        })?;

    let endpoint = openai_chat_completions_endpoint(&trace.base_url);
    let messages_json: serde_json::Value = serde_json::to_value(messages)
        .map_err(|err| ProviderExecutionError::Parse(err.to_string()))?;
    let mut payload = serde_json::json!({
        "model": trace.resolved_model,
        "messages": messages_json,
        "temperature": 0.2,
        "stream": trace.chat_completion_streaming
    });
    if let Some(effort) =
        reasoning_effort.filter(|value| matches!(*value, "low" | "medium" | "high"))
    {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "reasoning_effort".to_string(),
                serde_json::Value::String(effort.to_string()),
            );
        }
    }
    if let Some(tool_defs) = tools {
        if let Some(object) = payload.as_object_mut() {
            object.insert("tools".to_string(), tool_defs.clone());
        }
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(timeout_policy.global_timeout())
        .timeout_recv_response(timeout_policy.recv_response_timeout())
        .timeout_recv_body(timeout_policy.recv_body_timeout())
        .http_status_as_error(false)
        .user_agent("research-cli/0.1")
        .build()
        .into();
    let mut response = agent
        .post(&endpoint)
        .header("Authorization", &format!("Bearer {auth_value}"))
        .header("Content-Type", "application/json")
        .header(
            "Accept",
            if trace.chat_completion_streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .send(payload.to_string())
        .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let body = response
            .body_mut()
            .with_config()
            .limit(1_048_576)
            .read_to_string()
            .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
        return Err(ProviderExecutionError::HttpStatus { status, body });
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !trace.chat_completion_streaming || !content_type.contains("text/event-stream") {
        let body = response
            .body_mut()
            .with_config()
            .limit(1_048_576)
            .read_to_string()
            .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
        let completion = parse_openai_chat_completion_body(&body)?;
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: String::new(),
            });
        }
        let content = completion.content;
        on_delta(&content);
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        return Ok(ProviderPromptCompletion {
            content,
            execution_mode: "openai_chat_completions".to_string(),
            endpoint,
            tool_calls: if completion.tool_calls.is_empty() {
                None
            } else {
                Some(completion.tool_calls)
            },
            finish_reason: completion.finish_reason,
        });
    }
    let reader = response.body_mut().with_config().limit(8_388_608).reader();
    let result =
        parse_openai_chat_stream_with_cancel(reader, |delta| on_delta(delta), is_cancelled)?;
    if result.content.trim().is_empty() && result.tool_calls.is_empty() {
        return Err(ProviderExecutionError::EmptyResponse);
    }

    Ok(ProviderPromptCompletion {
        content: result.content,
        execution_mode: "openai_chat_completions_stream".to_string(),
        endpoint,
        tool_calls: if result.tool_calls.is_empty() {
            None
        } else {
            Some(result.tool_calls)
        },
        finish_reason: result.finish_reason,
    })
}

fn complete_openai_responses_internal<F, C>(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],
    reasoning_effort: Option<&str>,
    tools: Option<&serde_json::Value>,
    mut on_delta: F,
    is_cancelled: C,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<ProviderPromptCompletion, ProviderExecutionError>
where
    F: FnMut(&str),
    C: Fn() -> bool,
{
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    let auth_value =
        provider_auth_value(trace).ok_or_else(|| ProviderExecutionError::AuthMissing {
            provider_id: trace.provider_id.clone(),
            env_var: trace.auth_env_var.clone(),
        })?;
    let endpoint = openai_responses_endpoint(&trace.base_url);
    let (instructions, input) = openai_responses_prompt_from_chat_messages(messages);
    let mut payload = serde_json::json!({
        "model": trace.resolved_model,
        "input": input,
        "temperature": 0.2,
        "store": false,
        "stream": trace.chat_completion_streaming
    });
    if let Some(instructions) = instructions.filter(|value| !value.trim().is_empty()) {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "instructions".to_string(),
                serde_json::Value::String(instructions),
            );
        }
    }
    if let Some(effort) =
        reasoning_effort.filter(|value| matches!(*value, "low" | "medium" | "high"))
    {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "reasoning".to_string(),
                serde_json::json!({ "effort": effort }),
            );
        }
    }
    if let Some(tool_defs) = tools {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "tools".to_string(),
                openai_responses_tools_from_chat_tools(trace, tool_defs),
            );
        }
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(timeout_policy.global_timeout())
        .timeout_recv_response(timeout_policy.recv_response_timeout())
        .timeout_recv_body(timeout_policy.recv_body_timeout())
        .http_status_as_error(false)
        .user_agent("research-cli/0.1")
        .build()
        .into();
    let mut response = agent
        .post(&endpoint)
        .header("Authorization", &format!("Bearer {auth_value}"))
        .header("Content-Type", "application/json")
        .header(
            "Accept",
            if trace.chat_completion_streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .send(payload.to_string())
        .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let body = response
            .body_mut()
            .with_config()
            .limit(1_048_576)
            .read_to_string()
            .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
        return Err(ProviderExecutionError::HttpStatus { status, body });
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if trace.chat_completion_streaming && content_type.contains("text/event-stream") {
        let reader = response.body_mut().with_config().limit(8_388_608).reader();
        let result = parse_openai_responses_stream_with_cancel(
            reader,
            |delta| on_delta(delta),
            is_cancelled,
        )?;
        if result.content.trim().is_empty() && result.tool_calls.is_empty() {
            return Err(ProviderExecutionError::EmptyResponseWithDiagnostics {
                summary: "responses stream completed without output_text or tool calls".to_string(),
            });
        }
        return Ok(ProviderPromptCompletion {
            content: result.content,
            execution_mode: "openai_responses_stream".to_string(),
            endpoint,
            tool_calls: if result.tool_calls.is_empty() {
                None
            } else {
                Some(result.tool_calls)
            },
            finish_reason: result.finish_reason,
        });
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(8_388_608)
        .read_to_string()
        .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
    let completion = parse_openai_responses_body(&body)?;
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: String::new(),
        });
    }
    if !completion.content.is_empty() {
        on_delta(&completion.content);
    }
    if is_cancelled() {
        return Err(ProviderExecutionError::Cancelled {
            partial_content: completion.content,
        });
    }
    Ok(ProviderPromptCompletion {
        content: completion.content,
        execution_mode: "openai_responses".to_string(),
        endpoint,
        tool_calls: if completion.tool_calls.is_empty() {
            None
        } else {
            Some(completion.tool_calls)
        },
        finish_reason: completion.finish_reason,
    })
}

fn openai_chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn openai_responses_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/responses") {
        trimmed.to_string()
    } else if let Some(prefix) = trimmed.strip_suffix("/chat/completions") {
        format!("{prefix}/responses")
    } else {
        format!("{trimmed}/responses")
    }
}

fn openai_responses_prompt_from_chat_messages(
    messages: &[ChatMessage],
) -> (Option<String>, String) {
    let mut instructions = Vec::new();
    let mut input_parts = Vec::new();
    for message in messages {
        match message.role.as_str() {
            "system" | "developer" => instructions.push(message.content.clone()),
            "assistant" => {
                if !message.content.trim().is_empty() {
                    input_parts.push(format!("[assistant]\n{}", message.content));
                }
                if let Some(tool_calls) = message.tool_calls.as_ref() {
                    for call in tool_calls {
                        input_parts.push(format!(
                            "[assistant tool call]\ncall_id: {}\nname: {}\narguments: {}",
                            call.id, call.function.name, call.function.arguments
                        ));
                    }
                }
            }
            "tool" => input_parts.push(format!(
                "[tool result]\ncall_id: {}\n{}",
                message.tool_call_id.as_deref().unwrap_or("unknown"),
                message.content
            )),
            role => input_parts.push(format!("[{role}]\n{}", message.content)),
        }
    }
    if input_parts.is_empty() {
        input_parts.push("[user]\n".to_string());
    }
    (
        if instructions.is_empty() {
            None
        } else {
            Some(instructions.join("\n\n"))
        },
        input_parts.join("\n\n"),
    )
}

fn openai_responses_tools_from_chat_tools(
    trace: &ProviderResolutionTrace,
    tools: &serde_json::Value,
) -> serde_json::Value {
    if trace.responses_tool_schema == "chat_completions" {
        return tools.clone();
    }
    let Some(items) = tools.as_array() else {
        return tools.clone();
    };
    serde_json::Value::Array(
        items
            .iter()
            .map(|item| {
                if item.get("type").and_then(serde_json::Value::as_str) != Some("function") {
                    return item.clone();
                }
                let Some(function) = item.get("function").and_then(serde_json::Value::as_object)
                else {
                    return item.clone();
                };
                let mut object = serde_json::Map::new();
                object.insert(
                    "type".to_string(),
                    serde_json::Value::String("function".to_string()),
                );
                for key in ["name", "description", "parameters", "strict"] {
                    if let Some(value) = function.get(key).or_else(|| item.get(key)) {
                        object.insert(key.to_string(), value.clone());
                    }
                }
                if !object.contains_key("strict") {
                    object.insert("strict".to_string(), serde_json::Value::Bool(true));
                }
                serde_json::Value::Object(object)
            })
            .collect(),
    )
}

fn parse_openai_responses_body(
    body: &str,
) -> Result<OpenAiChatCompletionBody, ProviderExecutionError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|err| ProviderExecutionError::Parse(err.to_string()))?;
    let mut content_parts = Vec::new();
    collect_openai_response_text_value(value.get("output_text"), &mut content_parts);
    if matches!(value.get("text"), Some(serde_json::Value::String(_))) {
        collect_openai_response_text_value(value.get("text"), &mut content_parts);
    }
    collect_openai_response_text_value(value.get("content"), &mut content_parts);
    let mut tool_calls = Vec::new();
    if let Some(output) = value.get("output").and_then(serde_json::Value::as_array) {
        for item in output {
            match item.get("type").and_then(serde_json::Value::as_str) {
                Some("message") => {
                    collect_openai_response_text_value(item.get("content"), &mut content_parts);
                    collect_openai_response_text_value(item.get("text"), &mut content_parts);
                }
                Some("function_call") => {
                    let name = item
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if name.trim().is_empty() {
                        continue;
                    }
                    tool_calls.push(ChatToolCall {
                        id: item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        r#type: "function".to_string(),
                        function: ChatToolFunction {
                            name: name.to_string(),
                            arguments: item
                                .get("arguments")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        },
                    });
                }
                Some("custom_tool_call") => {
                    let name = item
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if name.trim().is_empty() {
                        continue;
                    }
                    tool_calls.push(ChatToolCall {
                        id: item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        r#type: "function".to_string(),
                        function: ChatToolFunction {
                            name: name.to_string(),
                            arguments: openai_response_arguments_as_string(item.get("input")),
                        },
                    });
                }
                Some("reasoning") => {
                    collect_openai_response_text_value(item.get("summary"), &mut content_parts);
                }
                _ => {}
            }
        }
    }
    let content = content_parts.join("\n").trim().to_string();
    let finish_reason = if tool_calls.is_empty() {
        value
            .get("status")
            .and_then(serde_json::Value::as_str)
            .filter(|status| *status == "completed")
            .map(|_| "stop".to_string())
    } else {
        Some("tool_calls".to_string())
    };
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(ProviderExecutionError::EmptyResponseWithDiagnostics {
            summary: summarize_openai_responses_body_shape(&value),
        });
    }
    Ok(OpenAiChatCompletionBody {
        content,
        tool_calls,
        finish_reason,
    })
}

fn parse_openai_responses_stream_with_cancel<R, F, C>(
    reader: R,
    mut on_delta: F,
    is_cancelled: C,
) -> Result<StreamParseResult, ProviderExecutionError>
where
    R: Read,
    F: FnMut(&str),
    C: Fn() -> bool,
{
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut finish_reason: Option<String> = None;
    let mut completed = false;
    let mut saw_text_delta = false;
    let mut buffered = BufReader::new(reader);
    let mut line = String::new();
    loop {
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        line.clear();
        let bytes = buffered
            .read_line(&mut line)
            .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        if bytes == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        if data == "[DONE]" {
            break;
        }
        if data.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| ProviderExecutionError::Parse(err.to_string()))?;
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = value.get("delta").and_then(serde_json::Value::as_str) {
                    if !delta.is_empty() {
                        saw_text_delta = true;
                        content.push_str(delta);
                        on_delta(delta);
                    }
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = value.get("item") {
                    let parsed = parse_openai_responses_body(
                        &serde_json::json!({
                            "status": "completed",
                            "output": [item.clone()]
                        })
                        .to_string(),
                    );
                    match parsed {
                        Ok(completion) => {
                            if !saw_text_delta && !completion.content.trim().is_empty() {
                                content.push_str(&completion.content);
                                on_delta(&completion.content);
                            }
                            tool_calls.extend(completion.tool_calls);
                        }
                        Err(ProviderExecutionError::EmptyResponse)
                        | Err(ProviderExecutionError::EmptyResponseWithDiagnostics { .. }) => {}
                        Err(err) => return Err(err),
                    }
                }
            }
            Some("response.completed") => {
                completed = true;
                finish_reason = Some("stop".to_string());
                if let Some(response) = value.get("response") {
                    if content.trim().is_empty() && tool_calls.is_empty() {
                        match parse_openai_responses_body(&response.to_string()) {
                            Ok(completion) => {
                                if !completion.content.trim().is_empty() {
                                    content.push_str(&completion.content);
                                }
                                tool_calls.extend(completion.tool_calls);
                                if completion.finish_reason.is_some() {
                                    finish_reason = completion.finish_reason;
                                }
                            }
                            Err(ProviderExecutionError::EmptyResponse)
                            | Err(ProviderExecutionError::EmptyResponseWithDiagnostics {
                                ..
                            }) => {}
                            Err(err) => return Err(err),
                        }
                    }
                }
                break;
            }
            Some("response.failed") => {
                let summary = value
                    .get("response")
                    .and_then(|response| response.get("error"))
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| value.to_string());
                return Err(ProviderExecutionError::HttpStatus {
                    status: 500,
                    body: summary,
                });
            }
            Some("response.incomplete") => {
                let reason = value
                    .get("response")
                    .and_then(|response| response.get("incomplete_details"))
                    .and_then(|details| details.get("reason"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                return Err(ProviderExecutionError::Parse(format!(
                    "responses stream incomplete: {reason}"
                )));
            }
            _ => {}
        }
    }
    if !completed && finish_reason.is_none() {
        finish_reason = if tool_calls.is_empty() {
            None
        } else {
            Some("tool_calls".to_string())
        };
    }
    Ok(StreamParseResult {
        content,
        tool_calls,
        finish_reason,
    })
}

fn collect_openai_response_text_value(value: Option<&serde_json::Value>, parts: &mut Vec<String>) {
    let Some(value) = value else {
        return;
    };
    match value {
        serde_json::Value::String(text) => push_non_empty_openai_response_text(parts, text),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_openai_response_text_value(Some(item), parts);
            }
        }
        serde_json::Value::Object(object) => {
            if let Some(value) = object
                .get("text")
                .or_else(|| object.get("output_text"))
                .or_else(|| object.get("value"))
                .or_else(|| object.get("json"))
                .or_else(|| object.get("content"))
            {
                collect_openai_response_text_value(Some(value), parts);
                return;
            }
            if matches!(
                object.get("type").and_then(serde_json::Value::as_str),
                Some("refusal")
            ) {
                collect_openai_response_text_value(object.get("refusal"), parts);
                return;
            }
            if object.contains_key("properties") || object.contains_key("schema") {
                return;
            }
            if object
                .keys()
                .any(|key| matches!(key.as_str(), "type" | "annotations" | "logprobs"))
            {
                return;
            }
            if let Ok(text) = serde_json::to_string(value) {
                push_non_empty_openai_response_text(parts, &text);
            }
        }
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            push_non_empty_openai_response_text(parts, &value.to_string());
        }
        serde_json::Value::Null => {}
    }
}

fn push_non_empty_openai_response_text(parts: &mut Vec<String>, text: &str) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
}

fn openai_response_arguments_as_string(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.to_string(),
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
        None => String::new(),
    }
}

fn summarize_openai_responses_body_shape(value: &serde_json::Value) -> String {
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let output_len = value
        .get("output")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let top_keys = value
        .as_object()
        .map(|object| {
            object
                .keys()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let item_summaries = value
        .get("output")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(8)
                .enumerate()
                .map(|(index, item)| {
                    let item_type = item
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown");
                    let content_shape = item
                        .get("content")
                        .map(summarize_openai_response_value_shape)
                        .unwrap_or_else(|| "no_content".to_string());
                    format!("{index}:{item_type}:{content_shape}")
                })
                .collect::<Vec<_>>()
                .join(";")
        })
        .unwrap_or_default();
    let mut summary = format!("status={status}; output_len={output_len}; top_keys={top_keys}");
    if !item_summaries.is_empty() {
        summary.push_str("; output_items=");
        summary.push_str(&item_summaries);
    }
    let mut truncated = summary.chars().take(1200).collect::<String>();
    if truncated.len() < summary.len() {
        truncated.push_str("...");
    }
    truncated
}

fn summarize_openai_response_value_shape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(_) => "bool".to_string(),
        serde_json::Value::Number(_) => "number".to_string(),
        serde_json::Value::String(text) => format!("string_len={}", text.len()),
        serde_json::Value::Array(items) => {
            let types = items
                .iter()
                .take(6)
                .map(|item| {
                    item.get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(match item {
                            serde_json::Value::Object(_) => "object",
                            serde_json::Value::Array(_) => "array",
                            serde_json::Value::String(_) => "string",
                            serde_json::Value::Number(_) => "number",
                            serde_json::Value::Bool(_) => "bool",
                            serde_json::Value::Null => "null",
                        })
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("array_len={},types=[{}]", items.len(), types)
        }
        serde_json::Value::Object(object) => {
            let keys = object.keys().take(8).cloned().collect::<Vec<_>>().join(",");
            format!("object_keys=[{keys}]")
        }
    }
}

fn parse_openai_chat_completion_body(
    body: &str,
) -> Result<OpenAiChatCompletionBody, ProviderExecutionError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|err| ProviderExecutionError::Parse(err.to_string()))?;
    let choice = value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
        .cloned();
    let content = choice
        .as_ref()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("output_text").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let tool_calls = choice
        .as_ref()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(serde_json::Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .map(|call| ChatToolCall {
                    id: call
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    r#type: call
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("function")
                        .to_string(),
                    function: ChatToolFunction {
                        name: call
                            .get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        arguments: call
                            .get("function")
                            .and_then(|function| function.get("arguments"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    },
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let finish_reason = choice
        .as_ref()
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(serde_json::Value::as_str)
        .filter(|reason| !reason.is_empty() && *reason != "null")
        .map(ToString::to_string)
        .or_else(|| {
            if tool_calls.is_empty() {
                None
            } else {
                Some("tool_calls".to_string())
            }
        });
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(ProviderExecutionError::EmptyResponse);
    }
    Ok(OpenAiChatCompletionBody {
        content,
        tool_calls,
        finish_reason,
    })
}

#[cfg(test)]
fn parse_openai_chat_stream<R, F>(
    reader: R,
    on_delta: F,
) -> Result<StreamParseResult, ProviderExecutionError>
where
    R: Read,
    F: FnMut(&str),
{
    parse_openai_chat_stream_with_cancel(reader, on_delta, || false)
}

/// Result of parsing an OpenAI streaming chat completion.
#[derive(Debug, PartialEq)]
pub struct StreamParseResult {
    pub content: String,
    pub tool_calls: Vec<ChatToolCall>,
    pub finish_reason: Option<String>,
}

struct PartialToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

fn parse_openai_chat_stream_with_cancel<R, F, C>(
    reader: R,
    mut on_delta: F,
    is_cancelled: C,
) -> Result<StreamParseResult, ProviderExecutionError>
where
    R: Read,
    F: FnMut(&str),
    C: Fn() -> bool,
{
    let mut content = String::new();
    let mut tool_calls_map: std::collections::BTreeMap<usize, PartialToolCall> =
        std::collections::BTreeMap::new();
    let mut finish_reason: Option<String> = None;
    let mut buffered = BufReader::new(reader);
    let mut line = String::new();
    loop {
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        line.clear();
        let bytes = buffered
            .read_line(&mut line)
            .map_err(|err| ProviderExecutionError::Transport(err.to_string()))?;
        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
        if bytes == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        if data == "[DONE]" {
            break;
        }
        if data.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| ProviderExecutionError::Parse(err.to_string()))?;

        // Extract text content delta
        if let Some(delta_text) = value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(serde_json::Value::as_str)
        {
            if !delta_text.is_empty() {
                content.push_str(delta_text);
                on_delta(delta_text);
            }
        }

        // Extract tool_calls deltas
        if let Some(tc_array) = value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("tool_calls"))
            .and_then(serde_json::Value::as_array)
        {
            for tc in tc_array {
                let index = tc
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize;
                let entry = tool_calls_map.entry(index).or_insert(PartialToolCall {
                    id: None,
                    name: None,
                    arguments: String::new(),
                });
                if let Some(id) = tc.get("id").and_then(serde_json::Value::as_str) {
                    entry.id = Some(id.to_string());
                }
                if let Some(name) = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(serde_json::Value::as_str)
                {
                    if !name.trim().is_empty() {
                        entry.name = Some(name.to_string());
                    }
                }
                if let Some(args) = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(serde_json::Value::as_str)
                {
                    entry.arguments.push_str(args);
                }
            }
        }

        // Extract finish_reason
        if let Some(reason) = value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(serde_json::Value::as_str)
        {
            if !reason.is_empty() && reason != "null" {
                finish_reason = Some(reason.to_string());
            }
        }

        if is_cancelled() {
            return Err(ProviderExecutionError::Cancelled {
                partial_content: content,
            });
        }
    }

    let tool_calls: Vec<ChatToolCall> = tool_calls_map
        .into_iter()
        .filter(|(_, partial)| {
            partial
                .name
                .as_deref()
                .map(|name| !name.trim().is_empty())
                .unwrap_or(false)
        })
        .map(|(_, partial)| ChatToolCall {
            id: partial.id.unwrap_or_default(),
            r#type: "function".to_string(),
            function: ChatToolFunction {
                name: partial.name.unwrap_or_default(),
                arguments: partial.arguments,
            },
        })
        .collect();

    Ok(StreamParseResult {
        content,
        tool_calls,
        finish_reason,
    })
}

fn has_env(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
struct ProviderRoute {
    route_provider_id: String,
    entry: &'static ProviderCatalogEntry,
    profile: Option<ProviderProfileConfig>,
    snapshot: ProviderSnapshot,
}

fn resolve_provider_route(provider: &str) -> Result<ProviderRoute, ProviderResolveError> {
    resolve_provider_route_with_profile_map(provider, None)
}

fn resolve_provider_route_with_profile_map(
    provider: &str,
    profiles: Option<&BTreeMap<String, ProviderProfileConfig>>,
) -> Result<ProviderRoute, ProviderResolveError> {
    let profile = if let Some(profiles) = profiles {
        config::provider_profile_for_provider(profiles, provider).cloned()
    } else {
        config::global_provider_profile(provider).ok().flatten()
    };
    if let Some(profile) = profile {
        let adapter_id = profile.provider_id.as_deref().unwrap_or(provider);
        let entry = provider_catalog_entry(adapter_id)
            .ok_or_else(|| ProviderResolveError::UnknownProvider(provider.to_string()))?;
        let snapshot = provider_snapshot(entry, Some(&profile));
        return Ok(ProviderRoute {
            route_provider_id: provider.to_string(),
            entry,
            profile: Some(profile),
            snapshot,
        });
    }
    if let Some(entry) = provider_catalog_entry(provider) {
        let snapshot = provider_snapshot(entry, None);
        return Ok(ProviderRoute {
            route_provider_id: entry.provider_id.to_string(),
            entry,
            profile: None,
            snapshot,
        });
    }
    Err(ProviderResolveError::UnknownProvider(provider.to_string()))
}

fn effective_supported_models(
    entry: &ProviderCatalogEntry,
    profile: Option<&ProviderProfileConfig>,
) -> Vec<String> {
    match profile {
        Some(profile) => {
            let mut models = profile.supported_models.clone();
            if models.is_empty() {
                if let Some(default_model) = profile.default_model.as_ref() {
                    models.push(default_model.clone());
                }
            }
            models
        }
        None => entry
            .supported_models
            .iter()
            .map(|model| (*model).to_string())
            .collect::<Vec<_>>(),
    }
}

fn supports_model(models: &[String], model: &str) -> bool {
    models.iter().any(|candidate| candidate == model)
}

fn allows_openai_compatible_custom_models(
    entry: &ProviderCatalogEntry,
    snapshot: &ProviderSnapshot,
) -> bool {
    entry.provider_id == "openai"
        && snapshot.base_url_source != "default"
        && env::var("RESEARCH_CLI_ALLOW_CUSTOM_MODEL").ok().as_deref() != Some("0")
}

fn provider_for_model(model: &str) -> Option<String> {
    provider_for_model_with_profiles(model, None)
}

fn provider_for_model_with_profiles(
    model: &str,
    profiles: Option<&BTreeMap<String, ProviderProfileConfig>>,
) -> Option<String> {
    let (_, normalized, _) = parse_prefixed_model_with_profiles(model, profiles);
    for entry in PROVIDER_CATALOG {
        let snapshot = provider_snapshot(entry, None);
        if supports_model(&snapshot.supported_models, &normalized) {
            return Some(entry.provider_id.to_string());
        }
    }
    let owned_profiles;
    let profile_map = if let Some(profiles) = profiles {
        profiles
    } else {
        owned_profiles = config::global_provider_profiles().unwrap_or_default();
        &owned_profiles
    };
    for (profile_name, profile) in profile_map {
        let adapter_id = profile.provider_id.as_deref().unwrap_or(profile_name);
        let Some(entry) = provider_catalog_entry(adapter_id) else {
            continue;
        };
        let snapshot = provider_snapshot(entry, Some(profile));
        if supports_model(&snapshot.supported_models, &normalized) {
            return Some(profile_name.to_string());
        }
    }
    None
}

fn parse_prefixed_model_with_profiles(
    raw: &str,
    profiles: Option<&BTreeMap<String, ProviderProfileConfig>>,
) -> (Option<String>, String, bool) {
    if let Some((provider, model)) = raw.split_once('/') {
        if !model.trim().is_empty()
            && resolve_provider_route_with_profile_map(provider, profiles).is_ok()
        {
            return (Some(provider.to_string()), model.to_string(), true);
        }
    }
    (None, raw.to_string(), false)
}

fn apply_model_alias(raw: &str) -> (String, String) {
    if raw.trim().is_empty() {
        return (String::new(), String::new());
    }

    MODEL_ALIASES
        .iter()
        .find(|(alias, _)| *alias == raw)
        .map(|(_, canonical)| ((*canonical).to_string(), (*canonical).to_string()))
        .unwrap_or_else(|| (raw.to_string(), String::new()))
}

fn auth_shape(value: &str) -> &'static str {
    if value.starts_with("sk-") {
        "api_key"
    } else if value.starts_with("anthropic-") {
        "bearer_token"
    } else {
        "opaque"
    }
}

#[derive(Debug, Clone)]
struct ProviderSnapshot {
    auth_env_var: String,
    auth_source: String,
    auth_status: String,
    auth_shape: String,
    auth_value: Option<String>,
    base_url: String,
    base_url_source: String,
    catalog_source: String,
    supported_models: Vec<String>,
    default_model: Option<String>,
    api_surface: String,
    api_surface_source: String,
    responses_tool_schema: String,
    responses_tool_schema_source: String,
    chat_completion_streaming: bool,
    chat_completion_streaming_source: String,
}

fn provider_snapshot(
    entry: &ProviderCatalogEntry,
    profile: Option<&ProviderProfileConfig>,
) -> ProviderSnapshot {
    let auth_env_var = profile
        .and_then(|profile| profile.auth_env_var.as_ref())
        .cloned()
        .unwrap_or_else(|| entry.auth_env_var.to_string());
    let config_auth_value = profile
        .and_then(|profile| profile.api_key.as_ref())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let env_auth_value = env::var(&auth_env_var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let auth_value = config_auth_value.clone().or(env_auth_value);
    let auth_status = if auth_value
        .as_ref()
        .map(|value| !value.is_empty())
        .unwrap_or(false)
    {
        "configured"
    } else {
        "missing"
    };

    let auth_shape = auth_value
        .as_deref()
        .map(auth_shape)
        .unwrap_or("absent")
        .to_string();

    let configured_base_url = profile
        .and_then(|profile| profile.base_url.clone())
        .or_else(|| {
            env::var(entry.base_url_env_var)
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    let base_url_source = if configured_base_url.is_some() {
        if profile
            .and_then(|profile| profile.base_url.as_ref())
            .is_some()
        {
            "config"
        } else {
            "environment"
        }
    } else {
        "default"
    };
    let supported_models = effective_supported_models(entry, profile);
    let default_model = profile
        .and_then(|profile| profile.default_model.clone())
        .or_else(|| {
            entry
                .supported_models
                .first()
                .map(|model| (*model).to_string())
        });
    let chat_completion_streaming = profile
        .and_then(|profile| profile.chat_completion_streaming)
        .unwrap_or(true);
    let chat_completion_streaming_source = if profile
        .and_then(|profile| profile.chat_completion_streaming)
        .is_some()
    {
        "config"
    } else {
        "default"
    };
    let api_surface = profile
        .and_then(|profile| profile.api_surface.as_deref())
        .map(normalize_provider_api_surface)
        .unwrap_or_else(|| "chat_completions".to_string());
    let api_surface_source = if profile
        .and_then(|profile| profile.api_surface.as_ref())
        .is_some()
    {
        "config"
    } else {
        "default"
    };
    let responses_tool_schema = profile
        .and_then(|profile| profile.responses_tool_schema.as_deref())
        .map(normalize_openai_responses_tool_schema)
        .unwrap_or_else(|| "responses".to_string());
    let responses_tool_schema_source = if profile
        .and_then(|profile| profile.responses_tool_schema.as_ref())
        .is_some()
    {
        "config"
    } else {
        "default"
    };

    ProviderSnapshot {
        auth_env_var,
        auth_source: if config_auth_value.is_some()
            || profile
                .and_then(|profile| profile.auth_env_var.as_ref())
                .is_some()
        {
            "config".to_string()
        } else {
            "environment".to_string()
        },
        auth_status: auth_status.to_string(),
        auth_shape,
        auth_value,
        base_url: configured_base_url.unwrap_or_else(|| entry.default_base_url.to_string()),
        base_url_source: base_url_source.to_string(),
        catalog_source: if profile.is_some() {
            "config".to_string()
        } else {
            "embedded".to_string()
        },
        supported_models,
        default_model,
        api_surface,
        api_surface_source: api_surface_source.to_string(),
        responses_tool_schema,
        responses_tool_schema_source: responses_tool_schema_source.to_string(),
        chat_completion_streaming,
        chat_completion_streaming_source: chat_completion_streaming_source.to_string(),
    }
}

fn normalize_provider_api_surface(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "responses" | "response" | "openai_responses" | "openai-responses" => {
            "responses".to_string()
        }
        "chat" | "chat_completions" | "chat-completions" | "chat/completions" | "" => {
            "chat_completions".to_string()
        }
        other => other.to_string(),
    }
}

fn normalize_openai_responses_tool_schema(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "chat" | "chat_completion" | "chat-completion" | "chat_completions"
        | "chat-completions" | "chat/completions" | "legacy_chat" | "legacy-chat" => {
            "chat_completions".to_string()
        }
        "responses" | "response" | "openai_responses" | "openai-responses" | "" => {
            "responses".to_string()
        }
        other => other.to_string(),
    }
}

fn build_provider_trace(
    entry: &ProviderCatalogEntry,
    route_provider_id: String,
    profile: Option<&ProviderProfileConfig>,
    requested_provider: String,
    requested_model: String,
    model_alias_applied: String,
    resolved_model: String,
    resolution_reason: String,
    routed_by_prefix: bool,
    explicit_provider_source: Option<&str>,
    explicit_model_source: Option<&str>,
) -> ProviderResolutionTrace {
    let snapshot = provider_snapshot(entry, profile);
    let degraded = snapshot.auth_status != "configured";
    let warnings = if degraded {
        vec![format!(
            "Set {} to enable live provider execution.",
            snapshot.auth_env_var
        )]
    } else {
        Vec::new()
    };
    let mut sources = BTreeMap::new();
    if !requested_provider.is_empty() {
        sources.insert(
            "provider".to_string(),
            explicit_provider_source
                .unwrap_or("explicit_request")
                .to_string(),
        );
    } else if matches!(
        resolution_reason.as_str(),
        "model_alias_catalog_match"
            | "model_prefix"
            | "model_catalog_match"
            | "ambient_credentials"
            | "default_provider"
            | "openai_compatible_custom_model"
    ) {
        sources.insert("provider".to_string(), resolution_reason.clone());
    }
    if !requested_model.is_empty() {
        sources.insert(
            "model".to_string(),
            explicit_model_source
                .unwrap_or("explicit_request")
                .to_string(),
        );
    } else if !resolved_model.is_empty()
        && matches!(
            resolution_reason.as_str(),
            "explicit_provider_default_model" | "ambient_credentials" | "default_provider"
        )
    {
        let model_source = if snapshot.default_model.as_deref() == Some(&resolved_model)
            && snapshot.catalog_source == "config"
        {
            "provider_profile_default"
        } else {
            "provider_default"
        };
        sources.insert("model".to_string(), model_source.to_string());
    }

    ProviderResolutionTrace {
        requested_model,
        requested_provider,
        model_alias_applied,
        routed_by_prefix,
        resolved_provider: route_provider_id.clone(),
        resolved_model,
        resolution_reason,
        degraded,
        warnings: warnings.clone(),
        sources,
        provider_id: entry.provider_id.to_string(),
        auth_status: snapshot.auth_status.clone(),
        auth_source: snapshot.auth_source.clone(),
        auth_env_var: snapshot.auth_env_var.clone(),
        auth_shape: snapshot.auth_shape,
        auth_value: snapshot.auth_value,
        base_url: snapshot.base_url,
        base_url_source: snapshot.base_url_source,
        catalog_source: snapshot.catalog_source,
        supported_models: snapshot.supported_models,
        api_surface: snapshot.api_surface,
        api_surface_source: snapshot.api_surface_source,
        responses_tool_schema: snapshot.responses_tool_schema,
        responses_tool_schema_source: snapshot.responses_tool_schema_source,
        chat_completion_streaming: snapshot.chat_completion_streaming,
        chat_completion_streaming_source: snapshot.chat_completion_streaming_source,
        degraded_features: Vec::new(),
    }
}

fn transport_probe(
    entry: &ProviderCatalogEntry,
    base_url: &str,
    auth_env_var: &str,
    configured_auth_value: Option<&str>,
) -> Result<ProviderTransportStatus, ProviderTransportProbeError> {
    let (scheme, host, port, path) =
        parse_http_endpoint(base_url).map_err(|message| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "endpoint_parse".to_string(),
                endpoint: base_url.to_string(),
                status: "blocked".to_string(),
            },
            message,
        })?;
    let endpoint = format!("{host}:{port}");
    if scheme == "https" {
        connect_stream(&host, port).map_err(|message| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "tcp_connect".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message,
        })?;
        return Ok(ProviderTransportStatus {
            lane: "tcp_connect".to_string(),
            endpoint,
            status: "reachable".to_string(),
        });
    }
    let mut stream =
        connect_stream(&host, port).map_err(|message| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message,
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|err| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message: err.to_string(),
        })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|err| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message: err.to_string(),
        })?;

    let auth_value = configured_auth_value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            env::var(auth_env_var)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message: format!("missing required credential in {auth_env_var}"),
        })?;
    let path = provider_probe_path(entry, &path);
    let request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\n{}\r\nConnection: close\r\n\r\n",
        provider_probe_method(entry),
        path,
        host,
        provider_auth_headers(entry, &auth_value)
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message: err.to_string(),
        })?;

    let mut buffer = [0_u8; 512];
    let bytes_read = stream
        .read(&mut buffer)
        .map_err(|err| ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message: err.to_string(),
        })?;
    let response = String::from_utf8_lossy(&buffer[..bytes_read]);
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Err(ProviderTransportProbeError {
            transport: ProviderTransportStatus {
                lane: "http_request".to_string(),
                endpoint: endpoint.clone(),
                status: "blocked".to_string(),
            },
            message: format!("provider probe returned unexpected response: {response}"),
        });
    }

    Ok(ProviderTransportStatus {
        lane: "http_request".to_string(),
        endpoint,
        status: "ready".to_string(),
    })
}

fn connect_stream(host: &str, port: u16) -> Result<TcpStream, String> {
    let address = format!("{host}:{port}");
    let mut last_error = None;
    for resolved in address
        .to_socket_addrs()
        .map_err(|err| format!("failed to resolve {address}: {err}"))?
    {
        match TcpStream::connect_timeout(&resolved, Duration::from_secs(2)) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Err(last_error.unwrap_or_else(|| format!("no usable socket address for {address}")))
}

fn provider_probe_method(_entry: &ProviderCatalogEntry) -> &'static str {
    "GET"
}

fn provider_probe_path(entry: &ProviderCatalogEntry, base_path: &str) -> String {
    let trimmed = base_path.trim_end_matches('/');
    match entry.provider_id {
        "anthropic" if trimmed.is_empty() || trimmed == "/" => "/v1/models".to_string(),
        _ if trimmed.is_empty() || trimmed == "/" => "/models".to_string(),
        _ => format!("{trimmed}/models"),
    }
}

fn provider_auth_headers(entry: &ProviderCatalogEntry, auth_value: &str) -> String {
    match entry.provider_id {
        "anthropic" => format!("x-api-key: {auth_value}\r\nanthropic-version: 2023-06-01"),
        _ => format!("Authorization: Bearer {auth_value}"),
    }
}

fn parse_http_endpoint(base_url: &str) -> Result<(String, String, u16, String), String> {
    let trimmed = base_url.trim();
    let (scheme, no_scheme) = if let Some(value) = trimmed.strip_prefix("http://") {
        ("http".to_string(), value)
    } else if let Some(value) = trimmed.strip_prefix("https://") {
        ("https".to_string(), value)
    } else {
        return Err(format!("unsupported provider base url: {base_url}"));
    };
    let (authority, remainder) = no_scheme.split_once('/').unwrap_or((no_scheme, ""));
    let (host, port) = if let Some((host, port)) = authority.split_once(':') {
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("invalid provider port in base url: {base_url}"))?;
        (host.to_string(), port)
    } else {
        let default_port = if scheme == "https" { 443 } else { 80 };
        (authority.to_string(), default_port)
    };
    let path = if remainder.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", remainder)
    };
    Ok((scheme, host, port, path))
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}
#[derive(Debug)]
struct ProviderTransportProbeError {
    transport: ProviderTransportStatus,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_profile_supported_models_are_authoritative_for_profile_routing() {
        let entry = provider_catalog_entry("openai").expect("openai catalog entry should exist");
        let profile = ProviderProfileConfig {
            provider_id: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:65535/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            auth_env_var: Some("OPENAI_API_KEY".to_string()),
            default_model: Some("gpt-5.4-mini".to_string()),
            supported_models: vec!["gpt-5.4-mini".to_string()],
            chat_completion_streaming: Some(true),
            api_surface: Some("chat_completions".to_string()),
            responses_tool_schema: Some("responses".to_string()),
        };

        let models = effective_supported_models(entry, Some(&profile));
        assert_eq!(models, vec!["gpt-5.4-mini".to_string()]);
    }

    #[test]
    fn provider_profile_default_model_only_acts_as_fallback_when_supported_models_missing() {
        let entry = provider_catalog_entry("openai").expect("openai catalog entry should exist");
        let profile = ProviderProfileConfig {
            provider_id: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:65535/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            auth_env_var: Some("OPENAI_API_KEY".to_string()),
            default_model: Some("gpt-5.4-mini".to_string()),
            supported_models: Vec::new(),
            chat_completion_streaming: Some(true),
            api_surface: Some("chat_completions".to_string()),
            responses_tool_schema: Some("responses".to_string()),
        };

        let models = effective_supported_models(entry, Some(&profile));
        assert_eq!(models, vec!["gpt-5.4-mini".to_string()]);
    }

    #[test]
    fn provider_profile_cannot_implicitly_claim_base_catalog_models() {
        let entry = provider_catalog_entry("openai").expect("openai catalog entry should exist");
        let profile = ProviderProfileConfig {
            provider_id: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:65535/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            auth_env_var: Some("OPENAI_API_KEY".to_string()),
            default_model: Some("gpt-5.4-mini".to_string()),
            supported_models: vec!["gpt-5.4-mini".to_string()],
            chat_completion_streaming: Some(true),
            api_surface: Some("chat_completions".to_string()),
            responses_tool_schema: Some("responses".to_string()),
        };
        let snapshot = provider_snapshot(entry, Some(&profile));
        assert_eq!(snapshot.supported_models, vec!["gpt-5.4-mini".to_string()]);
        assert!(!supports_model(&snapshot.supported_models, "gpt-5.4"));
    }

    fn provider_trace_fixture() -> ProviderResolutionTrace {
        ProviderResolutionTrace {
            requested_model: "gpt-5.4".to_string(),
            requested_provider: "openai".to_string(),
            model_alias_applied: String::new(),
            routed_by_prefix: false,
            resolved_provider: "openai".to_string(),
            resolved_model: "gpt-5.4".to_string(),
            resolution_reason: "test".to_string(),
            degraded: false,
            warnings: Vec::new(),
            sources: BTreeMap::new(),
            provider_id: "openai".to_string(),
            auth_status: "configured".to_string(),
            auth_source: "test".to_string(),
            auth_env_var: "OPENAI_API_KEY".to_string(),
            auth_shape: "api_key".to_string(),
            auth_value: Some("sk-test".to_string()),
            base_url: "https://api.openai.com/v1".to_string(),
            base_url_source: "test".to_string(),
            catalog_source: "test".to_string(),
            supported_models: vec!["gpt-5.4".to_string()],
            api_surface: "responses".to_string(),
            api_surface_source: "config".to_string(),
            responses_tool_schema: "responses".to_string(),
            responses_tool_schema_source: "default".to_string(),
            chat_completion_streaming: true,
            chat_completion_streaming_source: "default".to_string(),
            degraded_features: Vec::new(),
        }
    }

    #[test]
    fn openai_chat_stream_parser_extracts_token_deltas() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut deltas = Vec::new();

        let content = parse_openai_chat_stream(stream.as_bytes(), |delta| {
            deltas.push(delta.to_string());
        })
        .expect("stream should parse");

        assert_eq!(deltas, vec!["hel", "lo"]);
        assert_eq!(content.content, "hello");
        assert!(content.tool_calls.is_empty());
        assert_eq!(content.finish_reason, None);
    }

    #[test]
    fn openai_chat_stream_parser_returns_cancelled_without_later_deltas() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut deltas = Vec::new();
        let emitted = std::cell::Cell::new(0usize);

        let err = parse_openai_chat_stream_with_cancel(
            stream.as_bytes(),
            |delta| {
                deltas.push(delta.to_string());
                emitted.set(emitted.get() + 1);
            },
            || emitted.get() > 0,
        )
        .expect_err("cancelled stream should return a cancellation error");

        match err {
            ProviderExecutionError::Cancelled { partial_content } => {
                assert_eq!(partial_content, "hel");
            }
            other => panic!("expected cancellation, got {other}"),
        }
        assert_eq!(deltas, vec!["hel"]);
    }

    #[test]
    fn openai_chat_stream_parser_keeps_parse_errors_distinct_from_cancelled() {
        let stream = "data: {not-json}\n\n";

        let err = parse_openai_chat_stream_with_cancel(stream.as_bytes(), |_| {}, || false)
            .expect_err("invalid stream should fail parsing");

        match err {
            ProviderExecutionError::Parse(_) => {}
            other => panic!("expected parse error, got {other}"),
        }
    }

    #[test]
    fn openai_chat_stream_parser_preserves_tool_name_when_later_deltas_are_empty() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"echo\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"\",\"arguments\":\"{\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"\",\"arguments\":\"text\\\":\\\"hello\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let parsed = parse_openai_chat_stream(stream.as_bytes(), |_| {})
            .expect("streaming tool call should parse");

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.name, "echo");
        assert_eq!(
            parsed.tool_calls[0].function.arguments,
            "{\"text\":\"hello\"}"
        );
        assert_eq!(parsed.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn openai_chat_completions_endpoint_accepts_prejoined_path() {
        assert_eq!(
            openai_chat_completions_endpoint("https://models.sjtu.edu.cn/api/v1/chat/completions"),
            "https://models.sjtu.edu.cn/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_endpoint("https://models.sjtu.edu.cn/api/v1/"),
            "https://models.sjtu.edu.cn/api/v1/chat/completions"
        );
    }

    #[test]
    fn openai_responses_parser_extracts_text_and_function_calls() {
        let body = r#"{
  "status": "completed",
  "output": [
    {"type": "message", "content": [{"type": "output_text", "text": "done"}]},
    {"type": "function_call", "call_id": "call_1", "name": "echo", "arguments": "{\"text\":\"hello\"}"}
  ]
}"#;

        let parsed = parse_openai_responses_body(body).expect("responses body should parse");

        assert_eq!(parsed.content, "done");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_1");
        assert_eq!(parsed.tool_calls[0].function.name, "echo");
        assert_eq!(
            parsed.tool_calls[0].function.arguments,
            "{\"text\":\"hello\"}"
        );
        assert_eq!(parsed.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn openai_responses_parser_extracts_json_content_parts() {
        let body = r#"{
  "status": "completed",
  "output": [
    {
      "type": "message",
      "content": [
        {
          "type": "output_text",
          "json": {"verdict": "fail", "required_repairs": ["add evidence"]}
        }
      ]
    }
  ]
}"#;

        let parsed = parse_openai_responses_body(body)
            .expect("responses body with structured json content should parse");

        assert!(parsed.content.contains("\"verdict\":\"fail\""));
        assert!(parsed.content.contains("\"required_repairs\""));
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_responses_parser_extracts_message_content_string() {
        let body = r#"{
  "status": "completed",
  "output": [
    {"type": "message", "content": "{\"verdict\":\"pass\"}"}
  ]
}"#;

        let parsed =
            parse_openai_responses_body(body).expect("string content should parse as text");

        assert_eq!(parsed.content, "{\"verdict\":\"pass\"}");
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_responses_parser_extracts_custom_tool_call_input() {
        let body = r#"{
  "status": "completed",
  "output": [
    {
      "type": "custom_tool_call",
      "call_id": "call_custom_1",
      "name": "freeform_patch",
      "input": {"patch": "*** Begin Patch"}
    }
  ]
}"#;

        let parsed =
            parse_openai_responses_body(body).expect("custom tool call should parse as tool call");

        assert_eq!(parsed.content, "");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_custom_1");
        assert_eq!(parsed.tool_calls[0].function.name, "freeform_patch");
        assert!(parsed.tool_calls[0]
            .function
            .arguments
            .contains("\"patch\":\"*** Begin Patch\""));
        assert_eq!(parsed.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn openai_responses_parser_reports_empty_shape_diagnostics() {
        let body = r#"{
  "status": "completed",
  "output": [
    {"type": "web_search_call", "status": "completed"},
    {"type": "reasoning", "summary": []}
  ],
  "text": {"format": {"type": "json_schema"}}
}"#;

        let err = parse_openai_responses_body(body)
            .expect_err("non-text response should return diagnostics");

        match err {
            ProviderExecutionError::EmptyResponseWithDiagnostics { summary } => {
                assert!(summary.contains("status=completed"));
                assert!(summary.contains("output_len=2"));
                assert!(summary.contains("web_search_call"));
                assert!(summary.contains("reasoning"));
            }
            other => panic!("expected diagnostics, got {other}"),
        }
    }

    #[test]
    fn openai_responses_stream_parser_extracts_output_text_deltas() {
        let stream = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\n"
        );
        let mut deltas = Vec::new();

        let parsed = parse_openai_responses_stream_with_cancel(
            stream.as_bytes(),
            |delta| deltas.push(delta.to_string()),
            || false,
        )
        .expect("responses stream should parse");

        assert_eq!(deltas, vec!["hel", "lo"]);
        assert_eq!(parsed.content, "hello");
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_responses_stream_parser_extracts_done_item_without_deltas() {
        let stream = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"done-only\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\n"
        );

        let parsed = parse_openai_responses_stream_with_cancel(stream.as_bytes(), |_| {}, || false)
            .expect("done-only responses stream should parse");

        assert_eq!(parsed.content, "done-only");
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_responses_stream_parser_extracts_function_call_done_item() {
        let stream = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"echo\",\"arguments\":\"{\\\"text\\\":\\\"hi\\\"}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\n"
        );

        let parsed = parse_openai_responses_stream_with_cancel(stream.as_bytes(), |_| {}, || false)
            .expect("tool-call responses stream should parse");

        assert_eq!(parsed.content, "");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.name, "echo");
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_responses_endpoint_rewrites_chat_completions_path() {
        assert_eq!(
            openai_responses_endpoint("https://models.sjtu.edu.cn/api/v1/chat/completions"),
            "https://models.sjtu.edu.cn/api/v1/responses"
        );
        assert_eq!(
            openai_responses_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn openai_responses_tool_schema_can_keep_chat_compatible_shape() {
        let tools = serde_json::json!([
            {
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read a file",
                    "parameters": {
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }
                }
            }
        ]);
        let mut trace = provider_trace_fixture();

        trace.responses_tool_schema = "responses".to_string();
        let responses_tools = openai_responses_tools_from_chat_tools(&trace, &tools);
        assert_eq!(
            responses_tools[0]["name"],
            serde_json::Value::String("read_file".to_string())
        );
        assert!(responses_tools[0].get("function").is_none());

        trace.responses_tool_schema = "chat_completions".to_string();
        let chat_compatible_tools = openai_responses_tools_from_chat_tools(&trace, &tools);
        assert_eq!(chat_compatible_tools, tools);
    }

    #[test]
    fn provider_failure_policy_classifies_rate_limit_as_retryable_backoff() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::HttpStatus {
            status: 429,
            body: "rate limit exceeded".to_string(),
        });

        assert_eq!(policy.category, "rate_limit");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert!(!policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 5 * 60_000);
        assert_eq!(provider_failure_backoff_ms(&policy, 2), 10 * 60_000);
    }

    #[test]
    fn provider_failure_policy_classifies_downstream_model_error_as_retryable() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::HttpStatus {
            status: 400,
            body: r#"{"code":500006,"detail":"网络错误，请联系客服","message":"DOWN_STREAM_MODEL_ERROR","error":{"code":"500006","message":"网络错误，请联系客服","type":"DOWN_STREAM_MODEL_ERROR"}}"#.to_string(),
        });

        assert_eq!(policy.category, "provider_downstream_error");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert!(!policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 60_000);
    }

    #[test]
    fn provider_failure_policy_classifies_quota_exhausted_as_long_backoff() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::HttpStatus {
            status: 400,
            body: r#"{"code":500006,"detail":"您已达到每周/每月使用上限","message":"DOWN_STREAM_MODEL_ERROR","error":{"code":"500006","message":"您已达到每周/每月使用上限","type":"DOWN_STREAM_MODEL_ERROR"}}"#.to_string(),
        });

        assert_eq!(policy.category, "quota_exhausted");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert!(!policy.operator_gate_required);
        assert_eq!(
            provider_failure_backoff_ms(&policy, 1),
            PROVIDER_FAILURE_QUOTA_EXHAUSTED_BACKOFF_MS
        );
        assert_eq!(
            provider_failure_backoff_ms(&policy, 8),
            PROVIDER_FAILURE_QUOTA_EXHAUSTED_BACKOFF_MS
        );
    }

    #[test]
    fn provider_failure_policy_classifies_invalid_app_key_as_operator_gate() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::HttpStatus {
            status: 400,
            body: r#"{"code":610003,"detail":"当前套餐AppKey不存在或已失效","message":"CODING_PLAN_APP_KEY_NOT_EXIST","error":{"code":"610003","message":"当前套餐AppKey不存在或已失效","type":"CODING_PLAN_APP_KEY_NOT_EXIST"}}"#.to_string(),
        });

        assert_eq!(policy.category, "auth_invalid");
        assert_eq!(policy.disposition, ProviderFailureDisposition::OperatorGate);
        assert!(!policy.retryable);
        assert!(policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 0);
    }

    #[test]
    fn provider_failure_policy_classifies_transport_timeout_as_retryable() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::Transport(
            "request timed out while reading response".to_string(),
        ));

        assert_eq!(policy.category, "transport_retryable");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 60_000);
    }

    #[test]
    fn provider_failure_policy_classifies_tls_eof_as_retryable() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::Transport(
            "io: peer closed connection without sending TLS close_notify: connection closed before message completed; unexpected-eof".to_string(),
        ));

        assert_eq!(policy.category, "transport_retryable");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert!(!policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 60_000);
    }

    #[test]
    fn provider_failure_policy_classifies_peer_disconnected_as_retryable() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::Transport(
            "io: Peer disconnected".to_string(),
        ));

        assert_eq!(policy.category, "transport_retryable");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert!(!policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 60_000);
    }

    #[test]
    fn provider_failure_policy_classifies_temporary_dns_resolution_as_retryable() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::Transport(
            "io: failed to lookup address information: Temporary failure in name resolution"
                .to_string(),
        ));

        assert_eq!(policy.category, "transport_retryable");
        assert_eq!(
            policy.disposition,
            ProviderFailureDisposition::RetryWithBackoff
        );
        assert!(policy.retryable);
        assert!(!policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 60_000);
    }

    #[test]
    fn provider_failure_policy_classifies_auth_missing_as_operator_gate() {
        let policy = classify_provider_execution_error(&ProviderExecutionError::AuthMissing {
            provider_id: "glm".to_string(),
            env_var: "SJTU_GLM_API_KEY".to_string(),
        });

        assert_eq!(policy.category, "auth_missing");
        assert_eq!(policy.disposition, ProviderFailureDisposition::OperatorGate);
        assert!(!policy.retryable);
        assert!(policy.operator_gate_required);
        assert_eq!(provider_failure_backoff_ms(&policy, 1), 0);
    }

    #[test]
    fn live_prompt_completion_refuses_scaffold_when_provider_is_not_configured() {
        let trace = ProviderResolutionTrace {
            requested_model: String::new(),
            requested_provider: String::new(),
            model_alias_applied: String::new(),
            routed_by_prefix: false,
            resolved_provider: "openai".to_string(),
            resolved_model: "gpt-5.4".to_string(),
            resolution_reason: "default_provider".to_string(),
            degraded: true,
            warnings: vec!["Set OPENAI_API_KEY to enable live provider execution.".to_string()],
            sources: BTreeMap::new(),
            provider_id: "openai".to_string(),
            auth_status: "missing".to_string(),
            auth_source: "environment".to_string(),
            auth_env_var: "OPENAI_API_KEY".to_string(),
            auth_shape: "missing".to_string(),
            auth_value: None,
            base_url: "https://api.openai.com/v1".to_string(),
            base_url_source: "default".to_string(),
            catalog_source: "embedded".to_string(),
            supported_models: vec!["gpt-5.4".to_string()],
            api_surface: "chat_completions".to_string(),
            api_surface_source: "default".to_string(),
            responses_tool_schema: "responses".to_string(),
            responses_tool_schema_source: "default".to_string(),
            chat_completion_streaming: true,
            chat_completion_streaming_source: "default".to_string(),
            degraded_features: Vec::new(),
        };

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];

        let err = complete_prompt_streaming_live(&trace, &messages, |_| {})
            .expect_err("strict live prompt should not return scaffold output");

        match err {
            ProviderExecutionError::AuthMissing { provider_id, .. } => {
                assert_eq!(provider_id, "openai");
            }
            other => panic!("expected missing auth, got {other}"),
        }
    }

    #[test]
    fn tool_prompt_completion_refuses_scaffold_when_live_route_is_not_available() {
        let trace = ProviderResolutionTrace {
            requested_model: "gpt-5.4".to_string(),
            requested_provider: "openai".to_string(),
            model_alias_applied: String::new(),
            routed_by_prefix: false,
            resolved_provider: "openai".to_string(),
            resolved_model: "gpt-5.4".to_string(),
            resolution_reason: "explicit_provider_and_model".to_string(),
            degraded: false,
            warnings: Vec::new(),
            sources: BTreeMap::new(),
            provider_id: "openai".to_string(),
            auth_status: "configured".to_string(),
            auth_source: "environment".to_string(),
            auth_env_var: "OPENAI_API_KEY".to_string(),
            auth_shape: "api_key".to_string(),
            auth_value: Some("sk-test".to_string()),
            base_url: "https://api.openai.com/v1".to_string(),
            base_url_source: "default".to_string(),
            catalog_source: "embedded".to_string(),
            supported_models: vec!["gpt-5.4".to_string()],
            api_surface: "chat_completions".to_string(),
            api_surface_source: "default".to_string(),
            responses_tool_schema: "responses".to_string(),
            responses_tool_schema_source: "default".to_string(),
            chat_completion_streaming: true,
            chat_completion_streaming_source: "default".to_string(),
            degraded_features: Vec::new(),
        };
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "use a tool".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];
        let tools = serde_json::json!([
            {
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "read a file",
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }
                }
            }
        ]);

        let err = complete_prompt_streaming_with_tools(
            &trace,
            &messages,
            None,
            Some(tools),
            |_| {},
            || false,
        )
        .expect_err("tool-capable agent loops must not return scaffold prompt output");

        match err {
            ProviderExecutionError::LiveProviderRequired { provider_id, .. } => {
                assert_eq!(provider_id, "openai");
            }
            other => panic!("expected live provider requirement, got {other}"),
        }
    }
}
