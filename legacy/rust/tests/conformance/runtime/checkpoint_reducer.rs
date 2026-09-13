use crate::support::unique_temp_dir;
use research_cli::events::writer::append_event;
use research_cli::events::KernelEventEnvelope;
use research_cli::runtime::checkpoint::{
    KernelStateBundle, PermissionRuntimeState, ProjectState, SessionRuntimeState, TurnRuntimeState,
};
use research_cli::runtime::reducer::{publish_checkpoint, CheckpointPublishPlan};
use serde_json::json;

#[test]
fn stale_publish_reloads_latest_bundle_and_replays_unapplied_events() {
    let temp_dir = unique_temp_dir("checkpoint_reducer");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    let initial_bundle = KernelStateBundle {
        project_state: ProjectState {
            project_id: "project_bootstrap".to_string(),
            workspace_root: temp_dir.display().to_string(),
            workspace_hash: "workspace_hash".to_string(),
            protocol_version: "v1alpha1".to_string(),
            active_session_id: Some("session_a".to_string()),
            current_permission_mode: Some("default".to_string()),
            mission_frame_ref: None,
        },
        session_state: SessionRuntimeState {
            session_id: Some("session_a".to_string()),
            status: "idle".to_string(),
            transcript_path: Some(".pmcli/sessions/session_a/transcript.jsonl".to_string()),
            summary_ref: None,
        },
        turn_state: TurnRuntimeState {
            turn_id: None,
            status: "idle".to_string(),
            active_provider_route: None,
            pending_permission_id: None,
        },
        permission_state: PermissionRuntimeState {
            status: "idle".to_string(),
            pending_request_count: 0,
        },
        seq_cursor: 1,
        checkpoint_epoch: 1,
        ..KernelStateBundle::default()
    };
    initial_bundle
        .save_to(&checkpoint_path)
        .expect("initial bundle should save");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_session_open".to_string(),
            seq: 2,
            event_name: "session_open".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "session".to_string(),
            object_id: "session_b".to_string(),
            session_id: Some("session_b".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000002".to_string(),
            payload: json!({
                "status": "active",
                "transcript_path": ".pmcli/sessions/session_b/transcript.jsonl"
            }),
        },
    )
    .expect("session open event should append");

    let first_publish = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 1,
            base_checkpoint_epoch: 1,
            touched_families: vec!["session".to_string()],
        },
    )
    .expect("fresh publish should succeed");

    assert!(!first_publish.stale_base_detected);
    assert_eq!(first_publish.bundle.seq_cursor, 2);
    assert_eq!(first_publish.bundle.checkpoint_epoch, 2);
    assert_eq!(
        first_publish
            .bundle
            .project_state
            .active_session_id
            .as_deref(),
        Some("session_b")
    );

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_permission_pending".to_string(),
            seq: 3,
            event_name: "permission_request_pending".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "permission".to_string(),
            object_id: "perm_001".to_string(),
            session_id: Some("session_b".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000003".to_string(),
            payload: json!({
                "request_id": "perm_001"
            }),
        },
    )
    .expect("permission event should append");

    let stale_publish = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 1,
            base_checkpoint_epoch: 1,
            touched_families: vec!["permission".to_string()],
        },
    )
    .expect("stale publish should replay from latest bundle");

    assert!(stale_publish.stale_base_detected);
    assert_eq!(stale_publish.bundle.seq_cursor, 3);
    assert_eq!(stale_publish.bundle.checkpoint_epoch, 3);
    assert_eq!(
        stale_publish
            .bundle
            .project_state
            .active_session_id
            .as_deref(),
        Some("session_b")
    );
    assert_eq!(
        stale_publish.bundle.permission_state.pending_request_count,
        1
    );
}

#[test]
fn duplicate_event_sequence_is_rejected() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_duplicate_seq");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_turn_started".to_string(),
            seq: 1,
            event_name: "turn_started".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "turn".to_string(),
            object_id: "turn_001".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000001".to_string(),
            payload: json!({
                "turn_id": "turn_001"
            }),
        },
    )
    .expect("first event should append");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_turn_completed".to_string(),
            seq: 1,
            event_name: "turn_completed".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "turn".to_string(),
            object_id: "turn_001".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000001".to_string(),
            payload: json!({}),
        },
    )
    .expect("duplicate sequence event should append");

    let err = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec!["turn".to_string()],
        },
    )
    .expect_err("duplicate sequence should fail");

    assert!(
        err.to_string().contains("duplicate event sequence"),
        "unexpected error: {err}"
    );
}

#[test]
fn out_of_order_event_sequence_is_rejected() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_out_of_order");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_turn_completed".to_string(),
            seq: 2,
            event_name: "turn_completed".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "turn".to_string(),
            object_id: "turn_001".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000002".to_string(),
            payload: json!({}),
        },
    )
    .expect("first event should append");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_turn_started".to_string(),
            seq: 1,
            event_name: "turn_started".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "turn".to_string(),
            object_id: "turn_001".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000001".to_string(),
            payload: json!({
                "turn_id": "turn_001"
            }),
        },
    )
    .expect("second event should append");

    let err = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec!["turn".to_string()],
        },
    )
    .expect_err("out of order sequence should fail");

    assert!(
        err.to_string().contains("out-of-order event sequence"),
        "unexpected error: {err}"
    );
}

#[test]
fn inspection_and_compaction_events_do_not_break_reducer_replay() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_inspection_compaction");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_session_open".to_string(),
            seq: 1,
            event_name: "session_open".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "session".to_string(),
            object_id: "session_a".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000001".to_string(),
            payload: json!({
                "status": "active",
                "transcript_path": ".pmcli/sessions/session_a/transcript.jsonl"
            }),
        },
    )
    .expect("session open event should append");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_inspection".to_string(),
            seq: 2,
            event_name: "inspection".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "session".to_string(),
            object_id: "session_a".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000002".to_string(),
            payload: json!({
                "session_id": "session_a"
            }),
        },
    )
    .expect("inspection event should append");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_session_compaction".to_string(),
            seq: 3,
            event_name: "session_compaction".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "session".to_string(),
            object_id: "session_a".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000003".to_string(),
            payload: json!({
                "session_id": "session_a",
                "summary_ref": "summary_session_a_1777000000003"
            }),
        },
    )
    .expect("compaction event should append");

    let publish = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec!["session".to_string()],
        },
    )
    .expect("inspection and compaction events should replay cleanly");

    assert_eq!(publish.bundle.seq_cursor, 3);
    assert_eq!(
        publish.bundle.project_state.active_session_id.as_deref(),
        Some("session_a")
    );
    assert_eq!(
        publish.bundle.session_state.summary_ref.as_deref(),
        Some("summary_session_a_1777000000003")
    );
}

#[test]
fn session_resume_event_updates_active_session_during_replay() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_session_resume");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_session_open".to_string(),
            seq: 1,
            event_name: "session_open".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "session".to_string(),
            object_id: "session_a".to_string(),
            session_id: Some("session_a".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000010".to_string(),
            payload: json!({
                "status": "active",
                "transcript_path": ".pmcli/sessions/session_a/transcript.jsonl"
            }),
        },
    )
    .expect("session open event should append");

    append_event(
        &events_path,
        &KernelEventEnvelope {
            event_id: "evt_session_resume".to_string(),
            seq: 2,
            event_name: "session_resume".to_string(),
            phase: "terminal".to_string(),
            terminal_outcome: Some("succeeded".to_string()),
            object_kind: "command".to_string(),
            object_id: "resume".to_string(),
            session_id: Some("session_b".to_string()),
            project_id: Some("project_bootstrap".to_string()),
            timestamp: "1777000000011".to_string(),
            payload: json!({
                "session_id": "session_b",
                "project_id": "project_bootstrap"
            }),
        },
    )
    .expect("session resume event should append");

    let publish = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec!["session".to_string()],
        },
    )
    .expect("session resume should replay cleanly");

    assert_eq!(publish.bundle.seq_cursor, 2);
    assert_eq!(
        publish.bundle.project_state.active_session_id.as_deref(),
        Some("session_b")
    );
    assert_eq!(
        publish.bundle.session_state.session_id.as_deref(),
        Some("session_b")
    );
}

#[test]
fn remote_control_events_do_not_break_reducer_replay() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_remote_control");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    for (seq, event_name) in [
        (1, "remote_attach"),
        (2, "remote_handoff"),
        (3, "remote_takeover"),
        (4, "remote_notify"),
        (5, "remote_message"),
        (6, "remote_permission_response"),
        (7, "remote_lease_revoked"),
        (8, "remote_binding_revoked"),
        (9, "remote_reconnect"),
    ] {
        append_event(
            &events_path,
            &KernelEventEnvelope {
                event_id: format!("evt_{event_name}"),
                seq,
                event_name: event_name.to_string(),
                phase: "terminal".to_string(),
                terminal_outcome: Some("succeeded".to_string()),
                object_kind: "remote_control".to_string(),
                object_id: "phone-alpha".to_string(),
                session_id: Some("session_a".to_string()),
                project_id: Some("project_bootstrap".to_string()),
                timestamp: format!("177700000000{seq}"),
                payload: json!({
                    "client_id": "phone-alpha",
                    "session_id": "session_a"
                }),
            },
        )
        .expect("remote control event should append");
    }

    let publish = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec!["remote".to_string()],
        },
    )
    .expect("remote control events should replay cleanly");

    assert_eq!(publish.bundle.seq_cursor, 9);
    assert_eq!(publish.applied_event_count, 9);
}

#[test]
fn canonical_advanced_event_stems_do_not_break_reducer_replay() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_advanced_stems");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");

    let initial_bundle = KernelStateBundle {
        agent_state: research_cli::runtime::checkpoint::AgentRuntimeState {
            status: "running".to_string(),
            active_agent_ids: vec!["agent_a".to_string()],
            pending_count: 1,
        },
        branch_state: research_cli::runtime::checkpoint::BranchRuntimeState {
            status: "active".to_string(),
            active_batch_ids: vec!["batch_a".to_string()],
            promotable_branch_ids: vec!["branch_a".to_string()],
        },
        seq_cursor: 0,
        checkpoint_epoch: 0,
        ..KernelStateBundle::default()
    };
    initial_bundle
        .save_to(&checkpoint_path)
        .expect("initial bundle should save");

    for (seq, event_name, object_id, payload) in [
        (
            1,
            "tool",
            "tool_read_file",
            json!({"tool_name": "read_file"}),
        ),
        (
            2,
            "memory_query",
            "memory_query_001",
            json!({"status": "idle", "working_set_count": 2, "trusted_count": 1}),
        ),
        (
            3,
            "memory_record",
            "memory_record_001",
            json!({"status": "recorded", "working_set_count": 3}),
        ),
        (
            4,
            "memory_promotion",
            "memory_promotion_001",
            json!({"status": "promoted", "trusted_count": 2, "pending_promotion_count": 0}),
        ),
        (
            5,
            "memory_invalidation",
            "memory_invalidation_001",
            json!({"status": "invalidated", "working_set_count": 2}),
        ),
        (
            6,
            "projectops_tick",
            "tick_001",
            json!({"tick_id": "tick_001", "status": "ticked", "wake_queue_count": 1}),
        ),
        (
            7,
            "digest_promotion",
            "digest_001",
            json!({"digest_ref": "digest_001", "pending_digest_count": 0}),
        ),
        (
            8,
            "digest_rejection",
            "digest_002",
            json!({"pending_digest_count": 0}),
        ),
        (
            9,
            "experiment_supervision",
            "run_001",
            json!({"status": "supervising", "pending_research_runs": 1}),
        ),
        (
            10,
            "wake_event",
            "wake_001",
            json!({"status": "wake_queued", "wake_queue_count": 2}),
        ),
        (
            11,
            "research_stage",
            "stage_001",
            json!({"status": "research_updated", "pending_research_runs": 1}),
        ),
        (
            12,
            "research_repair",
            "repair_001",
            json!({"status": "research_repair", "pending_research_runs": 1}),
        ),
        (
            13,
            "research_pivot",
            "pivot_001",
            json!({"status": "research_pivot", "pending_research_runs": 1}),
        ),
        (
            14,
            "research_refine",
            "refine_001",
            json!({"status": "research_refine", "pending_research_runs": 1}),
        ),
        (15, "agent_stop", "agent_a", json!({"agent_id": "agent_a"})),
        (
            16,
            "branch_cancel",
            "batch_a",
            json!({"batch_id": "batch_a"}),
        ),
        (
            17,
            "branch_promote",
            "branch_a",
            json!({"branch_id": "branch_a", "status": "promoted"}),
        ),
        (
            18,
            "review_open",
            "review_a",
            json!({"review_id": "review_a"}),
        ),
        (
            19,
            "review_retry",
            "review_b",
            json!({"review_id": "review_b"}),
        ),
        (20, "session", "session_a", json!({"status": "idle"})),
        (21, "repl", "repl_a", json!({"status": "idle"})),
        (
            22,
            "research_stage_decision",
            "decision_direct_001",
            json!({"schema_version": "research_decision_record.v1", "status": "research_stage_decision", "pending_research_runs": 1}),
        ),
        (
            23,
            "goal_research_stage_decision",
            "decision_001",
            json!({"schema_version": "goal_research_stage_decision_event.v1", "status": "auto_approved_low_risk_advance"}),
        ),
    ] {
        append_event(
            &events_path,
            &KernelEventEnvelope {
                event_id: format!("evt_{event_name}_{seq}"),
                seq,
                event_name: event_name.to_string(),
                phase: "terminal".to_string(),
                terminal_outcome: Some("succeeded".to_string()),
                object_kind: event_name.to_string(),
                object_id: object_id.to_string(),
                session_id: Some("session_a".to_string()),
                project_id: Some("project_bootstrap".to_string()),
                timestamp: format!("17770000000{seq:02}"),
                payload,
            },
        )
        .expect("advanced canonical event should append");
    }

    let publish = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec![
                "tools".to_string(),
                "memory".to_string(),
                "projectops".to_string(),
                "research".to_string(),
                "agents".to_string(),
                "branches".to_string(),
                "reviews".to_string(),
            ],
        },
    )
    .expect("canonical advanced events should replay cleanly");

    assert_eq!(publish.bundle.seq_cursor, 23);
    assert_eq!(publish.applied_event_count, 23);
    assert_eq!(publish.bundle.memory_state.status, "invalidated");
    assert_eq!(publish.bundle.memory_state.working_set_count, 2);
    assert_eq!(publish.bundle.memory_state.trusted_count, 2);
    assert_eq!(publish.bundle.projectops_state.pending_research_runs, 1);
    assert_eq!(publish.bundle.projectops_state.wake_queue_count, 2);
    assert!(publish.bundle.agent_state.active_agent_ids.is_empty());
    assert_eq!(publish.bundle.agent_state.status, "idle");
    assert!(publish.bundle.branch_state.active_batch_ids.is_empty());
    assert!(publish.bundle.branch_state.promotable_branch_ids.is_empty());
    assert_eq!(publish.bundle.branch_state.status, "promoted");
    assert_eq!(
        publish.bundle.review_state.open_review_ids,
        vec!["review_a".to_string(), "review_b".to_string()]
    );
}

#[test]
fn write_lane_lock_must_be_acquired_before_publication() {
    let temp_dir = unique_temp_dir("checkpoint_reducer_lock");
    let checkpoint_path = temp_dir.join(".pmcli").join("project_state.json");
    let events_path = temp_dir.join(".pmcli").join("events").join("events.jsonl");
    let lock_path = temp_dir.join(".pmcli").join("project_state.lock");

    std::fs::create_dir_all(lock_path.parent().expect("lock parent should exist"))
        .expect("lock dir should exist");
    std::fs::write(&lock_path, "busy").expect("lock file should exist");

    let err = publish_checkpoint(
        &checkpoint_path,
        &events_path,
        CheckpointPublishPlan {
            base_seq_cursor: 0,
            base_checkpoint_epoch: 0,
            touched_families: vec!["session".to_string()],
        },
    )
    .expect_err("existing write lane lock should fail");

    assert!(
        err.to_string().contains("checkpoint write lane is busy"),
        "unexpected error: {err}"
    );
}
