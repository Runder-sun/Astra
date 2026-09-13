use crate::support::{init_git_workspace, unique_temp_dir};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn registry_root_env() -> String {
    unique_temp_dir("golden_provider_registry")
        .display()
        .to_string()
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("provider")
        .join(name)
}

fn output_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid json")
}

fn normalize_trace(trace: &Value) -> Value {
    json!({
        "requested_model": trace["requested_model"],
        "routed_by_prefix": trace["routed_by_prefix"],
        "resolved_provider": trace["resolved_provider"],
        "resolved_model": trace["resolved_model"],
        "resolution_reason": trace["resolution_reason"],
        "degraded": trace["degraded"],
        "sources": trace["sources"],
        "provider_id": trace["provider_id"],
        "auth_status": trace["auth_status"],
        "auth_source": trace["auth_source"],
        "auth_env_var": trace["auth_env_var"],
        "auth_shape": trace["auth_shape"],
        "base_url": trace["base_url"],
        "base_url_source": trace["base_url_source"],
        "catalog_source": trace["catalog_source"],
        "supported_models": trace["supported_models"],
        "chat_completion_streaming": trace["chat_completion_streaming"],
        "chat_completion_streaming_source": trace["chat_completion_streaming_source"]
    })
}

#[test]
fn provider_resolution_fixture_matches_environment_selected_prompt_trace() {
    let workspace_root = init_git_workspace("golden_provider_resolution");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", "http://example.test/v1")
        .env("RESEARCH_CLI_MODEL", "gpt-4.1-mini")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let actual = output_json(&output);
    let trace = normalize_trace(&actual["data"]["provider_trace"]);

    let fixture = fs::read_to_string(fixture_path("provider_resolution_trace.json"))
        .expect("fixture should be readable");
    let expected: Value = serde_json::from_str(&fixture).expect("fixture should parse as json");
    assert_eq!(trace, expected);
}
