use crate::support::init_git_workspace;
use research_cli::orchestration::{
    load_active_run, reconcile_run_from_markdown, render_run_markdown, save_run, OrchestrationRun,
    OrchestrationStep, OrchestrationStepStatus, RunControlCommand,
};

#[test]
fn orchestration_markdown_round_trip_preserves_checklist_and_gates() {
    let mut run = OrchestrationRun::new(
        "run_001",
        "Implement first agent loop substrate",
        "suggestion_first",
    );
    run.steps = vec![
        OrchestrationStep::new(
            "step_1",
            "Resolve ContextPack and Soul",
            OrchestrationStepStatus::Done,
        )
        .with_worker("main-agent")
        .with_artifact("src/session/context_pack.rs"),
        OrchestrationStep::new(
            "step_2",
            "Project orchestration markdown",
            OrchestrationStepStatus::Running,
        )
        .with_gate("permission: workspace-write")
        .with_continuation("wait for targeted tests"),
        OrchestrationStep::new(
            "step_3",
            "Expose progress projection",
            OrchestrationStepStatus::Pending,
        ),
    ];

    let markdown = render_run_markdown(&run);
    assert!(
        markdown.contains("- [x] <!-- step:step_1 status:done --> Resolve ContextPack and Soul")
    );
    assert!(markdown.contains("gate: permission: workspace-write"));
    assert!(markdown.contains("continuation: wait for targeted tests"));

    let reconciled =
        reconcile_run_from_markdown(&run, &markdown).expect("markdown should reconcile");
    assert_eq!(reconciled.steps.len(), 3);
    assert_eq!(reconciled.steps[0].status, OrchestrationStepStatus::Done);
    assert_eq!(
        reconciled.steps[1].continuation_points,
        vec!["wait for targeted tests".to_string()]
    );
}

#[test]
fn orchestration_progress_projection_uses_checklist_completion_by_default() {
    let mut run = OrchestrationRun::new("run_002", "Runtime substrate", "approved");
    run.steps = vec![
        OrchestrationStep::new("a", "done", OrchestrationStepStatus::Done),
        OrchestrationStep::new("b", "running", OrchestrationStepStatus::Running),
        OrchestrationStep::new("c", "blocked", OrchestrationStepStatus::Blocked),
        OrchestrationStep::new("d", "pending", OrchestrationStepStatus::Pending),
    ];

    let projection = run.progress_projection();

    assert_eq!(projection.done_steps, 1);
    assert_eq!(projection.total_steps, 4);
    assert_eq!(projection.percent, 25);
    assert_eq!(projection.current_step_id.as_deref(), Some("b"));
    assert_eq!(projection.blocked_steps, 1);
    assert!(projection.compact_line.contains("1/4"));
}

#[test]
fn orchestration_runtime_api_persists_active_run_and_control_commands() {
    let workspace_root = init_git_workspace("orchestration_active_run");
    let data_dir = workspace_root.join(".pmcli");
    let mut run = OrchestrationRun::new("run_003", "Recover interrupted substrate run", "approved");
    run.status = "running".to_string();
    run.steps = vec![OrchestrationStep::new(
        "resume",
        "Load active run",
        OrchestrationStepStatus::Running,
    )];
    run.control_commands
        .push(RunControlCommand::new("pause", "user requested pause"));

    let path = save_run(&data_dir, &run).expect("run should save");
    assert!(path.ends_with("orchestration.json"));
    assert!(path.parent().expect("run dir").ends_with("run_003"));
    assert!(path
        .parent()
        .expect("run dir")
        .join("orchestration.md")
        .exists());

    let loaded = load_active_run(&data_dir)
        .expect("active run lookup should succeed")
        .expect("active run should exist");

    assert_eq!(loaded.run_id, "run_003");
    assert_eq!(loaded.control_commands[0].command, "pause");
    assert_eq!(
        loaded.progress_projection().current_step_id.as_deref(),
        Some("resume")
    );
}

#[test]
fn orchestration_load_accepts_active_step_status_as_running_alias() {
    let workspace_root = init_git_workspace("orchestration_active_status_alias");
    let data_dir = workspace_root.join(".pmcli");
    let mut run = OrchestrationRun::new(
        "run_active_alias",
        "Recover a long-running autonomous research job",
        "full_auto",
    );
    run.status = "running".to_string();
    run.steps = vec![OrchestrationStep::new(
        "goal_acceptance",
        "Review agent outputs and decide continue, repair, review, or complete",
        OrchestrationStepStatus::Running,
    )];

    let path = save_run(&data_dir, &run).expect("run should save");
    let mut raw = std::fs::read_to_string(&path).expect("run json should be readable");
    raw = raw.replace("\"status\": \"running\"", "\"status\": \"active\"");
    std::fs::write(&path, raw).expect("run json should be patched with legacy active status");

    let loaded = load_active_run(&data_dir)
        .expect("active alias should deserialize")
        .expect("active run should exist");

    assert_eq!(
        loaded.steps[0].status,
        OrchestrationStepStatus::Running,
        "legacy/board `active` step status should be normalized to running"
    );
    assert_eq!(
        loaded.progress_projection().current_step_id.as_deref(),
        Some("goal_acceptance")
    );
}
