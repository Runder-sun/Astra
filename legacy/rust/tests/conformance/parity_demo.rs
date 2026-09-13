use crate::support::{unique_temp_dir, write_text_file};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn parity_demo_harness_executes_expected_scenarios() {
    let harness_dir = unique_temp_dir("parity_demo_harness");
    let log_path = harness_dir.join("cargo_invocations.log");
    let manifest_path = harness_dir.join("manifest.json");
    let fake_cargo = harness_dir.join("fake_cargo.sh");

    write_text_file(
        &fake_cargo,
        r#"#!/usr/bin/env bash
set -euo pipefail
: "${MOCK_CARGO_LOG:?missing MOCK_CARGO_LOG}"
printf '%s\n' "$*" >> "$MOCK_CARGO_LOG"
"#,
    );

    let permissions = fs::metadata(&fake_cargo)
        .expect("fake cargo should exist")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = permissions;
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_cargo, permissions).expect("fake cargo should be executable");
    }

    let output = Command::new("bash")
        .current_dir(repo_root())
        .arg(repo_root().join("scripts/run_parity_demo_suite.sh"))
        .env("CARGO_BIN", &fake_cargo)
        .env("MOCK_CARGO_LOG", &log_path)
        .env("PARITY_DEMO_MANIFEST", &manifest_path)
        .output()
        .expect("parity demo harness should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let invocations = fs::read_to_string(&log_path)
        .expect("fake cargo log should exist")
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    let expected_invocations = [
        "test --test conformance remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly -- --nocapture",
        "test --test conformance remote_daemon_executes_projected_tui_action_ids -- --nocapture",
        "test --test conformance transcript_round_trip_preserves_tool_turn_structure -- --nocapture",
        "test --lib tui::tests:: -- --nocapture",
        "test --test conformance remote_mobile_web_app_assets_expose_theme_language_and_session_controls -- --nocapture",
        "test --test conformance remote_mobile_runtime_bootstrap_probes_before_requiring_control_token -- --nocapture",
        "test --test conformance remote_daemon_reconnect_replays_cursor_and_rejects_expired_lease -- --nocapture",
        "test --test conformance remote_daemon_terminal_bridge_records_governed_input_resize_and_signal -- --nocapture",
        "test --test conformance remote_daemon_exposes_workbench_message_and_permission_response -- --nocapture",
        "test --test conformance remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly -- --nocapture",
        "test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture",
        "test --test conformance remote_daemon_exposes_host_surface_and_governed_terminal_projection -- --nocapture",
        "test --test conformance remote_daemon_events_streams_canonical_events_after_cursor -- --nocapture",
        "test --test conformance remote_daemon_exposes_artifact_code_viewer_projection -- --nocapture",
        "test --test conformance remote_daemon_exposes_result_panel_projection_without_new_truth -- --nocapture",
        "test --test conformance operator_cli_surfaces::routines_create_trigger_and_project_background_agent -- --nocapture",
        "test --test conformance operator_cli_surfaces::goal_research_loop_demo_closes_success_and_repair_paths -- --nocapture",
    ];

    assert_eq!(
        invocations.len(),
        expected_invocations.len(),
        "expected one cargo invocation per parity demo, got {:?}",
        invocations
    );

    for (invocation, expected_invocation) in invocations.iter().zip(expected_invocations) {
        assert_eq!(invocation.as_str(), expected_invocation);
    }

    let manifest_contents = fs::read_to_string(&manifest_path).expect("manifest should be written");
    let manifest: Value =
        serde_json::from_str(&manifest_contents).expect("manifest should parse as json");
    assert_eq!(manifest["harness"], "parity_demo_suite");
    assert_eq!(manifest["schema_version"], "1");
    assert_eq!(manifest["status"], "passed");
    assert_eq!(manifest["summary"]["passed"], 6);
    assert_eq!(manifest["summary"]["failed"], 0);
    assert_eq!(manifest["summary"]["gap"], 0);

    let demos = manifest["demos"].as_array().expect("demos array");
    let demo_ids = demos
        .iter()
        .map(|demo| demo["demo_id"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        demo_ids,
        vec![
            "runtime_turn_demo".to_string(),
            "tui_inline_repl_demo".to_string(),
            "remote_mobile_handoff_demo".to_string(),
            "host_contract_demo".to_string(),
            "routine_background_demo".to_string(),
            "goal_research_loop_demo".to_string(),
        ]
    );
    assert_eq!(demos[4]["status"], "passed");
    assert_eq!(demos[4]["required_for_parity"], true);
    assert_eq!(demos[4]["gap"], Value::Null);
    assert_eq!(demos[5]["status"], "passed");
    assert_eq!(demos[5]["required_for_parity"], true);
    assert_eq!(demos[5]["gap"], Value::Null);
    assert_eq!(
        demos[5]["evidence"],
        json!([
            "goal_watch_install",
            "watch_tick",
            "goal_task_dispatch",
            "worker_acceptance",
            "stage_decision",
            "repair_review",
            "followup_dispatch",
            "research_board_projection",
            "event_log"
        ])
    );
    assert_eq!(
        demos[0]["test_names"]
            .as_array()
            .expect("runtime test names")
            .len(),
        3
    );
    assert_eq!(
        demos[2]["test_names"]
            .as_array()
            .expect("mobile test names")
            .len(),
        6
    );
    assert_eq!(
        demos[3]["test_names"]
            .as_array()
            .expect("host test names")
            .len(),
        5
    );
}

#[test]
fn parity_demo_harness_writes_failed_manifest_when_a_required_demo_fails() {
    let harness_dir = unique_temp_dir("parity_demo_harness_failure");
    let log_path = harness_dir.join("cargo_invocations.log");
    let manifest_path = harness_dir.join("manifest.json");
    let fake_cargo = harness_dir.join("fake_cargo.sh");

    write_text_file(
        &fake_cargo,
        r#"#!/usr/bin/env bash
set -euo pipefail
: "${MOCK_CARGO_LOG:?missing MOCK_CARGO_LOG}"
printf '%s\n' "$*" >> "$MOCK_CARGO_LOG"
if [[ "$*" == *"remote_daemon_executes_projected_tui_action_ids"* ]]; then
  exit 17
fi
"#,
    );

    let permissions = fs::metadata(&fake_cargo)
        .expect("fake cargo should exist")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = permissions;
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_cargo, permissions).expect("fake cargo should be executable");
    }

    let output = Command::new("bash")
        .current_dir(repo_root())
        .arg(repo_root().join("scripts/run_parity_demo_suite.sh"))
        .env("CARGO_BIN", &fake_cargo)
        .env("MOCK_CARGO_LOG", &log_path)
        .env("PARITY_DEMO_MANIFEST", &manifest_path)
        .output()
        .expect("parity demo harness should execute");

    assert!(
        !output.status.success(),
        "harness should return non-zero when a required demo fails"
    );

    let manifest_contents =
        fs::read_to_string(&manifest_path).expect("failed parity manifest should still be written");
    let manifest: Value =
        serde_json::from_str(&manifest_contents).expect("manifest should parse as json");
    assert_eq!(manifest["harness"], "parity_demo_suite");
    assert_eq!(manifest["status"], "failed");
    assert_eq!(manifest["summary"]["passed"], 5);
    assert_eq!(manifest["summary"]["failed"], 1);
    assert_eq!(manifest["summary"]["gap"], 0);

    let demos = manifest["demos"].as_array().expect("demos array");
    assert_eq!(demos[0]["demo_id"], "runtime_turn_demo");
    assert_eq!(demos[0]["status"], "failed");
    assert_eq!(demos[4]["demo_id"], "routine_background_demo");
    assert_eq!(demos[4]["status"], "passed");
    assert_eq!(demos[5]["demo_id"], "goal_research_loop_demo");
    assert_eq!(demos[5]["status"], "passed");
}
