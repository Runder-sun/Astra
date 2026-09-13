use crate::support::{init_git_workspace, unique_temp_dir};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn registry_root_env() -> String {
    unique_temp_dir("golden_accounting_registry")
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

#[test]
fn accounting_fixtures_match_empty_project_usage_and_runtime_stats() {
    let workspace_root = init_git_workspace("golden_accounting");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let usage_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["usage", "--json"])
        .output()
        .expect("usage should execute");
    assert!(usage_output.status.success());
    let usage_actual = output_json(&usage_output);
    let usage_fixture = fs::read_to_string(fixture_path("usage_summary_estimated.json"))
        .expect("usage fixture should be readable");
    let usage_expected: Value =
        serde_json::from_str(&usage_fixture).expect("usage fixture should parse as json");
    assert_eq!(usage_actual["data"], usage_expected);

    let stats_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["stats", "--json"])
        .output()
        .expect("stats should execute");
    assert!(stats_output.status.success());
    let stats_actual = output_json(&stats_output);
    let stats_fixture = fs::read_to_string(fixture_path("stats_summary_runtime.json"))
        .expect("stats fixture should be readable");
    let stats_expected: Value =
        serde_json::from_str(&stats_fixture).expect("stats fixture should parse as json");
    assert_eq!(stats_actual["data"], stats_expected);
}
