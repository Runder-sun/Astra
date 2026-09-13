use research_cli::runtime::checkpoint::{
    KernelStateBundle, ProjectState, SessionRuntimeState, TurnRuntimeState,
};
use std::fs;
use std::path::PathBuf;
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
fn kernel_state_bundle_round_trip_persists_and_reloads() {
    let temp_dir = unique_temp_dir("bundle_round_trip");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let bundle = KernelStateBundle {
        project_state: ProjectState {
            project_id: "project_bootstrap".to_string(),
            workspace_root: temp_dir.display().to_string(),
            workspace_hash: "workspace_hash".to_string(),
            protocol_version: "v1alpha1".to_string(),
            active_session_id: Some("session_bootstrap".to_string()),
            current_permission_mode: Some("default".to_string()),
            mission_frame_ref: Some(".pmcli/project_goals/mission_frame.json".to_string()),
        },
        session_state: SessionRuntimeState {
            session_id: Some("session_bootstrap".to_string()),
            status: "idle".to_string(),
            transcript_path: Some(".pmcli/sessions/session_bootstrap/transcript.jsonl".to_string()),
            summary_ref: None,
        },
        turn_state: TurnRuntimeState {
            turn_id: None,
            status: "idle".to_string(),
            active_provider_route: None,
            pending_permission_id: None,
        },
        seq_cursor: 1,
        checkpoint_epoch: 1,
        ..KernelStateBundle::default()
    };

    bundle
        .save_to(&checkpoint_path)
        .expect("bundle save should succeed");

    let loaded =
        KernelStateBundle::load_from(&checkpoint_path).expect("bundle reload should succeed");

    assert_eq!(loaded, bundle);
}
