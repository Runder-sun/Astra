use crate::support::{init_git_workspace, unique_temp_dir, write_text_file};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn registry_root_env() -> String {
    unique_temp_dir("golden_config_registry")
        .display()
        .to_string()
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("config")
        .join(name)
}

fn output_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid json")
}

#[test]
fn config_precedence_fixture_matches_prompt_resolution_output() {
    let workspace_root = init_git_workspace("golden_config_precedence");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("golden_config_precedence_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    write_text_file(
        global_root.join("settings.json"),
        r#"{
  "default_model": "gpt-5"
}"#,
    );
    write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_model": "gpt-5.4"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("RESEARCH_CLI_MODEL", "gpt-4.1-mini")
        .args(["prompt", "--model", "gpt-5.4-mini", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let actual = output_json(&output);
    let trace = &actual["data"]["provider_trace"];
    let normalized = json!({
        "resolved_model": format!(
            "{}/{}",
            trace["resolved_provider"].as_str().expect("provider"),
            trace["resolved_model"].as_str().expect("model")
        ),
        "sources": {
            "model": trace["sources"]["model"]
        }
    });

    let fixture = fs::read_to_string(fixture_path("config_precedence_model_override.json"))
        .expect("fixture should be readable");
    let expected: Value = serde_json::from_str(&fixture).expect("fixture should parse as json");
    assert_eq!(normalized, expected);
}
