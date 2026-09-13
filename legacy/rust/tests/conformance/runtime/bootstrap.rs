use crate::support::{init_git_workspace, unique_temp_dir};
use std::process::Command;

#[test]
fn bootstrap_run_empty_args_returns_chat_first_tui() {
    let workspace_root = init_git_workspace("bootstrap_empty_args");
    let registry_root = unique_temp_dir("bootstrap_registry");

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
        .output()
        .expect("empty-args bootstrap should execute");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("Astra Code 代码智能体"));
    assert!(stdout.contains("对话优先的代码智能体"));
    assert!(stdout.contains("研究："));
    assert!(stdout.contains("输入：直接输入对话"));
    assert!(stdout.contains("/language"));
    assert!(!stdout.contains("pane conversation [expanded] markdown_message_list"));
    assert!(!stdout.contains("fullscreen_split_pane_with_sidebar"));
}

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}
