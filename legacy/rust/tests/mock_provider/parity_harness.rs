use crate::support::{unique_temp_dir, write_text_file};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn mock_provider_parity_harness_executes_selected_batch8_checks_sequentially() {
    let harness_dir = unique_temp_dir("mock_provider_harness");
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
        .arg(repo_root().join("scripts/run_mock_parity_harness.sh"))
        .env("CARGO_BIN", &fake_cargo)
        .env("MOCK_CARGO_LOG", &log_path)
        .env("MOCK_PARITY_MANIFEST", &manifest_path)
        .output()
        .expect("mock parity harness should execute");

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

    let expected_tests = [
        "prompt_uses_config_and_env_precedence_for_default_model_then_cli_overrides",
        "providers_test_reports_live_lane_readiness_machine_readably",
        "providers_test_missing_credentials_returns_blocked_typed_failure",
        "config_effective_applies_environment_overrides_machine_readably",
        "config_get_reports_environment_override_source_machine_readably",
        "config_sources_reports_cross_gap_override_without_middle_layer",
        "providers_refresh_catalog_file_source_reports_not_graduated_typed_failure",
        "prompt_with_session_appends_to_existing_session_transcript",
        "resume_returns_selected_session_with_recap_and_lineage",
        "sessions_create_list_and_resume_latest_are_project_scoped",
    ];

    assert_eq!(
        invocations.len(),
        expected_tests.len(),
        "expected one cargo invocation per parity check, got {:?}",
        invocations
    );

    for (invocation, expected_test) in invocations.iter().zip(expected_tests) {
        assert_eq!(
            invocation.as_str(),
            format!("test --test conformance {expected_test} -- --nocapture")
        );
    }

    let manifest_contents = fs::read_to_string(&manifest_path).expect("manifest should be written");
    let manifest: Value =
        serde_json::from_str(&manifest_contents).expect("manifest should parse as json");
    assert_eq!(manifest["harness"], "mock_provider_parity");
    assert_eq!(manifest["status"], "passed");
    let scenarios = manifest["scenarios"].as_array().expect("scenarios array");
    assert_eq!(scenarios.len(), expected_tests.len());
    assert!(scenarios
        .iter()
        .any(|scenario| scenario["category"] == "provider_auth"));
    assert!(scenarios
        .iter()
        .any(|scenario| scenario["category"] == "session_routing"));
}
