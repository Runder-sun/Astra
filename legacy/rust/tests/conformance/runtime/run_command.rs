use crate::support::init_git_workspace;
use research_cli::orchestration::{
    apply_run_control, build_recovery_decision_context, load_active_run, save_run,
    OrchestrationRun, OrchestrationStep, OrchestrationStepStatus, RunControlAction,
};
use research_cli::providers::ProviderResolutionTrace;
use research_cli::session::store::SessionStore;
use research_cli::session::transcript::TranscriptLine;
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn run_control_updates_active_run_status_and_records_command() {
    let workspace_root = init_git_workspace("run_control_update");
    let data_dir = workspace_root.join(".pmcli");
    let mut run = OrchestrationRun::new("run_ctl_001", "Ship runtime loop", "approved");
    run.status = "running".to_string();
    run.steps = vec![
        OrchestrationStep::new("s1", "first", OrchestrationStepStatus::Done),
        OrchestrationStep::new("s2", "second", OrchestrationStepStatus::Running),
    ];
    save_run(&data_dir, &run).expect("run should save");

    let result = apply_run_control(&data_dir, RunControlAction::Pause, None, "operator")
        .expect("pause should apply");

    assert_eq!(result.command.command, "pause");
    assert_eq!(result.run.status, "paused");
    assert_eq!(result.progress.percent, 50);
    assert_eq!(result.recovery_context.recommended_actions[0], "continue");

    let loaded = load_active_run(&data_dir)
        .expect("active run should load")
        .expect("active run should exist");
    assert_eq!(loaded.status, "paused");
    assert_eq!(loaded.control_commands.len(), 1);
}

#[test]
fn run_control_retry_and_skip_reject_unknown_or_unmatched_steps_without_recording_command() {
    let workspace_root = init_git_workspace("run_control_invalid_step");
    let data_dir = workspace_root.join(".pmcli");
    let mut run = OrchestrationRun::new("run_ctl_invalid", "Reject invalid controls", "approved");
    run.status = "running".to_string();
    run.steps = vec![
        OrchestrationStep::new("failed", "failed step", OrchestrationStepStatus::Failed),
        OrchestrationStep::new("done", "done step", OrchestrationStepStatus::Done),
    ];
    save_run(&data_dir, &run).expect("run should save");

    let unknown = apply_run_control(
        &data_dir,
        RunControlAction::Retry,
        Some("missing"),
        "operator",
    )
    .expect_err("unknown explicit retry step should fail");
    assert!(unknown.to_string().contains("unknown orchestration step"));

    let unmatched = apply_run_control(&data_dir, RunControlAction::Skip, Some("done"), "operator")
        .expect_err("explicit skip step in unsupported status should fail");
    assert!(unmatched
        .to_string()
        .contains("cannot skip orchestration step"));

    let loaded = load_active_run(&data_dir)
        .expect("active run should load")
        .expect("active run should exist");
    assert!(loaded.control_commands.is_empty());
    assert_eq!(loaded.steps[0].status, OrchestrationStepStatus::Failed);
    assert_eq!(loaded.steps[1].status, OrchestrationStepStatus::Done);

    let no_pending = apply_run_control(&data_dir, RunControlAction::Skip, None, "operator")
        .expect_err("implicit skip with no pending or blocked step should fail");
    assert!(no_pending
        .to_string()
        .contains("no orchestration step matching skip"));
}

#[test]
fn recovery_decision_context_includes_snapshot_progress_controls_repo_and_session() {
    let workspace_root = init_git_workspace("run_recovery_context");
    let data_dir = workspace_root.join(".pmcli");
    let store = SessionStore::new(data_dir.clone());
    let session = store
        .create_session(Some("Recovery".to_string()))
        .expect("session should create");
    store
        .append_line(
            &session.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "continue the run".to_string(),
            },
        )
        .expect("line should append");
    let mut run = OrchestrationRun::new("run_ctx_001", "Recover safely", "approved");
    run.status = "blocked".to_string();
    run.steps = vec![OrchestrationStep::new(
        "blocked",
        "needs input",
        OrchestrationStepStatus::Blocked,
    )];
    save_run(&data_dir, &run).expect("run should save");
    apply_run_control(
        &data_dir,
        RunControlAction::Replan,
        Some("blocked"),
        "operator",
    )
    .expect("control should apply");

    let context = build_recovery_decision_context(
        &data_dir,
        &workspace_root,
        Some(session.session_id.as_str()),
    )
    .expect("recovery context should build");

    assert_eq!(
        context.active_run.as_ref().expect("run").run_id,
        "run_ctx_001"
    );
    assert_eq!(
        context.progress.as_ref().expect("progress").blocked_steps,
        1
    );
    assert!(context
        .control_commands
        .iter()
        .any(|command| command.command == "replan"));
    assert!(context
        .repo_summary
        .iter()
        .any(|line| line.contains("Workspace root")));
    assert_eq!(
        context
            .session_summary
            .as_ref()
            .expect("session")
            .session_id,
        session.session_id
    );
    assert!(context.recommended_actions.contains(&"replan".to_string()));
}

#[test]
fn compact_session_summary_preserves_active_orchestration_state() {
    let workspace_root = init_git_workspace("compact_orchestration_state");
    let data_dir = workspace_root.join(".pmcli");
    let store = SessionStore::new(data_dir.clone());
    let session = store
        .create_session(Some("Compact".to_string()))
        .expect("session should create");
    store
        .append_line(
            &session.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "work on orchestration".to_string(),
            },
        )
        .expect("message should append");
    let mut run = OrchestrationRun::new("run_compact_001", "Keep plan alive", "approved");
    run.status = "running".to_string();
    run.steps = vec![OrchestrationStep::new(
        "active",
        "active checklist item",
        OrchestrationStepStatus::Running,
    )];
    save_run(&data_dir, &run).expect("run should save");

    let compacted = store
        .compact_session(
            &session.session_id,
            "project_compact",
            None,
            None,
            None,
            None,
        )
        .expect("compact should succeed");
    let summary_path = data_dir
        .join("sessions")
        .join(&session.session_id)
        .join("summaries")
        .join(format!("{}.md", compacted.summary_ref));
    let summary = std::fs::read_to_string(summary_path).expect("summary should be readable");

    assert!(summary.contains("## Active Orchestration"));
    assert!(summary.contains("run_compact_001"));
    assert!(summary.contains("active checklist item"));
}

#[test]
fn compact_session_llm_for_project_short_transcript_uses_real_project_id_for_digest() {
    let workspace_root = init_git_workspace("compact_llm_project_id");
    let data_dir = workspace_root.join(".pmcli");
    let store = SessionStore::new(data_dir.clone());
    let session = store
        .create_session(Some("Compact LLM".to_string()))
        .expect("session should create");
    store
        .append_line(
            &session.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "short transcript should use project id".to_string(),
            },
        )
        .expect("message should append");

    let compacted = store
        .compact_session_llm_for_project(
            &session.session_id,
            "project_real",
            &provider_trace_fixture(),
        )
        .expect("llm compaction fallback should succeed");

    let digest_path = data_dir
        .join("memory")
        .join("promotion_queue")
        .join(format!("{}.json", compacted.digest_candidate_id));
    let digest: Value = serde_json::from_str(
        &std::fs::read_to_string(digest_path).expect("digest should be readable"),
    )
    .expect("digest should parse");

    assert_eq!(digest["project_id"], "project_real");
    assert_ne!(digest["project_id"], "");
}

fn provider_trace_fixture() -> ProviderResolutionTrace {
    ProviderResolutionTrace {
        requested_model: "gpt-fixture".to_string(),
        requested_provider: "fixture".to_string(),
        model_alias_applied: String::new(),
        routed_by_prefix: false,
        resolved_provider: "fixture".to_string(),
        resolved_model: "gpt-fixture".to_string(),
        resolution_reason: "test".to_string(),
        degraded: false,
        warnings: Vec::new(),
        sources: BTreeMap::new(),
        provider_id: "fixture".to_string(),
        auth_status: "configured".to_string(),
        auth_source: "test".to_string(),
        auth_env_var: "FIXTURE_API_KEY".to_string(),
        auth_shape: "env".to_string(),
        auth_value: Some("fixture-key".to_string()),
        base_url: "https://fixture.invalid".to_string(),
        base_url_source: "test".to_string(),
        catalog_source: "test".to_string(),
        supported_models: vec!["gpt-fixture".to_string()],
        api_surface: "chat_completions".to_string(),
        api_surface_source: "default".to_string(),
        responses_tool_schema: "responses".to_string(),
        responses_tool_schema_source: "default".to_string(),
        chat_completion_streaming: true,
        chat_completion_streaming_source: "default".to_string(),
        degraded_features: Vec::new(),
    }
}
