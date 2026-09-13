use crate::support::init_git_workspace;
use research_cli::events::read_events_from;
use research_cli::events::writer::append_event;
use research_cli::events::KernelEventEnvelope;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    let dir = std::env::temp_dir().join(format!("research_cli_{label}_{nonce}"));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

#[test]
fn append_event_writes_canonical_jsonl_line() {
    let temp_dir = unique_temp_dir("event_log");
    let log_path = temp_dir.join(".pmcli").join("events.jsonl");
    let event = KernelEventEnvelope {
        event_id: "evt_bootstrap".to_string(),
        seq: 1,
        event_name: "runtime_preflight".to_string(),
        phase: "start".to_string(),
        terminal_outcome: None,
        object_kind: "runtime".to_string(),
        object_id: "doctor".to_string(),
        session_id: Some("session_bootstrap".to_string()),
        project_id: Some("project_bootstrap".to_string()),
        timestamp: "1777000000000".to_string(),
        payload: json!({
            "status": "ready"
        }),
    };

    append_event(&log_path, &event).expect("event append should succeed");

    let contents = fs::read_to_string(&log_path).expect("event log should exist");
    let stored: KernelEventEnvelope =
        serde_json::from_str(contents.trim()).expect("event line should deserialize");

    assert_eq!(stored, event);
}

#[test]
fn control_phase_event_is_valid_without_terminal_outcome() {
    let event = KernelEventEnvelope {
        event_id: "evt_control_interrupt".to_string(),
        seq: 1,
        event_name: "remote_interrupt_requested".to_string(),
        phase: "control".to_string(),
        terminal_outcome: None,
        object_kind: "control".to_string(),
        object_id: "interrupt_requested".to_string(),
        session_id: Some("session_control".to_string()),
        project_id: Some("project_control".to_string()),
        timestamp: "1777000000001".to_string(),
        payload: json!({
            "control_event_kind": "interrupt_requested"
        }),
    };

    event.validate().expect("control phase should be valid");
}

#[test]
fn concurrent_cli_event_writes_keep_jsonl_and_seq_canonical() {
    let workspace_root = init_git_workspace("concurrent_cli_event_writes");
    let state_home = unique_temp_dir("concurrent_cli_event_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let session_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "sessions",
            "create",
            "--title",
            "Concurrent Event Test",
            "--json",
        ])
        .status()
        .expect("sessions create should execute");
    assert!(session_status.success());

    let mut children = Vec::new();
    for _ in 0..8 {
        for args in [
            ["inspect", "--json"].as_slice(),
            ["providers", "auth-status", "--json"].as_slice(),
        ] {
            let child = Command::new(cargo_bin())
                .current_dir(&workspace_root)
                .env("RESEARCH_CLI_STATE_HOME", &state_home)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("concurrent command should spawn");
            children.push(child);
        }
    }

    for child in children {
        let output = child
            .wait_with_output()
            .expect("concurrent command should finish");
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let event_log_path = workspace_root
        .join(".pmcli")
        .join("events")
        .join("events.jsonl");
    let events = read_events_from(&event_log_path).expect("event log should stay parseable");
    assert!(
        events.len() >= 34,
        "expected initialization plus concurrent audit events, got {}",
        events.len()
    );

    let mut seen = HashSet::new();
    for (index, event) in events.iter().enumerate() {
        assert_eq!(
            event.seq,
            index as u64 + 1,
            "event seq should match append order"
        );
        assert!(
            seen.insert(event.seq),
            "event seq should not be duplicated: {}",
            event.seq
        );
        assert!(
            event.event_id.ends_with(&format!("_{}", event.seq)),
            "event id should include locked canonical seq: {}",
            event.event_id
        );
    }
}

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}
