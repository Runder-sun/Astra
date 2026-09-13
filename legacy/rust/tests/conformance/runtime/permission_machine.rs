use research_cli::permissions::{PermissionMode, PermissionPolicy, ToolClassification, ToolSpec};
use research_cli::tools::{
    agent_team_worker_tool_definitions, builtin_registry, builtin_tool_definitions_for_mode,
    format_tool_result_for_llm, is_read_only_shell_command, LocalToolExecutor,
    LocalToolExecutorContext, ToolCall, ToolResult,
};
use std::fs;

#[test]
fn permission_policy_gates_tools_by_mode_and_workspace_boundary() {
    let root = crate::support::unique_temp_dir("permission_policy");
    fs::write(root.join("README.md"), "fixture").expect("workspace fixture should write");

    let read_tool = ToolSpec::new("read_file", ToolClassification::ReadOnly);
    let write_tool = ToolSpec::new("write_file", ToolClassification::Mutating);
    let destructive_tool = ToolSpec::new("delete_file", ToolClassification::Destructive);

    let read_only = PermissionPolicy::new(PermissionMode::ReadOnly, &root);
    assert!(read_only.evaluate(&read_tool, Some("README.md")).allowed);
    let read_only_write = read_only.evaluate(&write_tool, Some("README.md"));
    assert!(!read_only_write.allowed);
    assert!(read_only_write.requires_approval);

    let workspace_write = PermissionPolicy::new(PermissionMode::WorkspaceWrite, &root);
    assert!(
        workspace_write
            .evaluate(&write_tool, Some("README.md"))
            .allowed
    );
    let destructive = workspace_write.evaluate(&destructive_tool, Some("README.md"));
    assert!(!destructive.allowed);
    assert!(destructive.requires_approval);

    let escaped = workspace_write.evaluate(&write_tool, Some("../outside.txt"));
    assert!(!escaped.allowed);
    assert!(!escaped.requires_approval);
    assert_eq!(escaped.reason_code, "workspace_scope_violation");

    let danger = PermissionPolicy::new(PermissionMode::DangerFullAccess, &root);
    assert!(
        danger
            .evaluate(&destructive_tool, Some("../outside.txt"))
            .allowed
    );
    assert!(
        danger
            .evaluate(&destructive_tool, Some("ls papers/ 2>/dev/null || true"))
            .allowed
    );
    assert!(
        !danger
            .evaluate(&destructive_tool, Some("cat /dev/sda"))
            .allowed
    );
}

#[test]
fn builtin_tool_registry_classifies_read_write_and_destructive_tools() {
    let registry = builtin_registry();

    let read_file = registry.get("read_file").expect("read_file should exist");
    assert_eq!(read_file.classification, ToolClassification::ReadOnly);

    let fetch = registry.get("fetch").expect("fetch should exist");
    assert_eq!(fetch.classification, ToolClassification::ReadOnly);

    let write_file = registry.get("write_file").expect("write_file should exist");
    assert_eq!(write_file.classification, ToolClassification::Mutating);

    let delete_file = registry
        .get("delete_file")
        .expect("delete_file should exist");
    assert_eq!(delete_file.classification, ToolClassification::Destructive);

    let shell = registry.get("shell").expect("shell should exist");
    assert_eq!(shell.classification, ToolClassification::Destructive);
}

#[test]
fn worker_tool_surface_excludes_main_agent_authority_tools() {
    let worker_tools = agent_team_worker_tool_definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();

    assert!(worker_tools.iter().any(|tool| tool == "read_file"));
    assert!(worker_tools.iter().any(|tool| tool == "write_file"));
    assert!(worker_tools.iter().any(|tool| tool == "worker_shell"));
    for forbidden in [
        "shell",
        "publish_board_tasks",
        "update_board_task",
        "merge_board_tasks",
        "record_obligation_decision",
        "record_worker_artifact_decision",
        "request_review_rerun",
        "request_route_change",
        "request_cleanup_plan",
    ] {
        assert!(
            !worker_tools.iter().any(|tool| tool == forbidden),
            "agent-team worker tool surface must not include {forbidden}"
        );
    }

    let main_agent_workspace_tools = builtin_tool_definitions_for_mode("workspace-write")
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    assert!(main_agent_workspace_tools
        .iter()
        .any(|tool| tool == "record_worker_artifact_decision"));
    assert!(!main_agent_workspace_tools
        .iter()
        .any(|tool| tool == "shell"));
}

#[test]
fn shell_read_only_classifier_blocks_obvious_mutations() {
    assert!(is_read_only_shell_command("cat README.md"));
    assert!(is_read_only_shell_command("git status --short"));
    assert!(!is_read_only_shell_command("touch created.txt"));
    assert!(!is_read_only_shell_command("cat README.md > copied.txt"));
    assert!(!is_read_only_shell_command("sed -i s/a/b/ README.md"));
    assert!(!is_read_only_shell_command(
        "grep fixture README.md | tee out.txt"
    ));
}

#[test]
fn local_tool_executor_reads_and_writes_workspace_relative_files() {
    let root = crate::support::unique_temp_dir("local_tool_executor");
    fs::write(root.join("README.md"), "fixture").expect("workspace fixture should write");
    let executor = LocalToolExecutor::new(&root);

    let mut read_call = ToolCall::new("read_file");
    read_call.target_path = Some("README.md".to_string());
    let read_result = executor
        .execute(&read_call)
        .expect("read_file should execute");
    assert_eq!(read_result.tool_name, "read_file");
    assert_eq!(read_result.status, "succeeded");
    assert_eq!(read_result.output.as_deref(), Some("fixture"));
    assert_eq!(
        read_result
            .structured
            .as_ref()
            .and_then(|value| value.get("path"))
            .and_then(|value| value.as_str()),
        Some("README.md")
    );

    let mut write_call = ToolCall::new("write_file");
    write_call.target_path = Some("notes/out.txt".to_string());
    write_call.content = Some("created".to_string());
    let write_result = executor
        .execute(&write_call)
        .expect("write_file should execute");
    assert_eq!(write_result.status, "succeeded");
    assert_eq!(
        fs::read_to_string(root.join("notes/out.txt")).expect("written file should exist"),
        "created"
    );
}

#[test]
fn local_tool_executor_runs_registered_file_tools() {
    let root = crate::support::unique_temp_dir("local_tool_executor_file_tools");
    fs::create_dir_all(root.join("src")).expect("src directory should write");
    fs::write(root.join("README.md"), "hello fixture\n").expect("readme should write");
    fs::write(root.join("src/lib.rs"), "fn fixture() {}\n").expect("source should write");
    fs::write(root.join("large.bin"), [0, 159, 146, 150]).expect("binary fixture should write");
    let executor = LocalToolExecutor::new(&root);

    let mut list_call = ToolCall::new("list_files");
    list_call.target_path = Some(".".to_string());
    let list_result = executor
        .execute(&list_call)
        .expect("list_files should execute");
    let files = list_result
        .structured
        .as_ref()
        .and_then(|value| value.get("files"))
        .and_then(|value| value.as_array())
        .expect("list_files should return files");
    assert!(files.iter().any(|file| file == "README.md"));
    assert!(files.iter().any(|file| file == "src/lib.rs"));

    let mut search_call = ToolCall::new("search");
    search_call.target_path = Some(".".to_string());
    search_call.content = Some("fixture".to_string());
    let search_result = executor
        .execute(&search_call)
        .expect("search should execute");
    let matches = search_result
        .structured
        .as_ref()
        .and_then(|value| value.get("matches"))
        .and_then(|value| value.as_array())
        .expect("search should return matches");
    assert_eq!(matches.len(), 2);
    assert!(matches
        .iter()
        .any(|item| item["path"] == "README.md" && item["line"] == 1));

    let mut patch_call = ToolCall::new("apply_patch");
    patch_call.target_path = Some("README.md".to_string());
    patch_call.content = Some("fixture=>patched".to_string());
    let patch_result = executor
        .execute(&patch_call)
        .expect("apply_patch should execute");
    assert_eq!(patch_result.status, "succeeded");
    assert_eq!(
        fs::read_to_string(root.join("README.md")).expect("patched file should read"),
        "hello patched\n"
    );

    let mut delete_call = ToolCall::new("delete_file");
    delete_call.target_path = Some("large.bin".to_string());
    let delete_result = executor
        .execute(&delete_call)
        .expect("delete_file should execute");
    assert_eq!(delete_result.status, "succeeded");
    assert!(!root.join("large.bin").exists());
}

#[test]
fn local_tool_executor_publishes_main_agent_board_tasks_into_active_run() {
    let root = crate::support::unique_temp_dir("local_tool_executor_main_agent_board");
    let data_dir = root.join(".pmcli");
    fs::create_dir_all(&data_dir).expect("data dir should write");
    let mut run = research_cli::orchestration::OrchestrationRun::new(
        "run_main_agent_board",
        "structured board task publishing",
        "approved",
    );
    run.steps = vec![research_cli::orchestration::OrchestrationStep::new(
        "goal_task_pool",
        "Goal task pool",
        research_cli::orchestration::OrchestrationStepStatus::Running,
    )];
    research_cli::orchestration::save_run(&data_dir, &run).expect("run should save");
    let executor = LocalToolExecutor::with_autonomous_research_context(
        &root,
        LocalToolExecutorContext {
            data_dir: data_dir.clone(),
            project_id: "project_tool_board".to_string(),
            job_id: Some("job_tool_board".to_string()),
            stage_id: Some("literature".to_string()),
            stage_execution_id: Some("stage_literature_1".to_string()),
            stage_artifact_type: Some("literature_matrix".to_string()),
        },
    );
    let mut call = ToolCall::new("publish_board_tasks");
    call.arguments = serde_json::json!({
        "tasks": [{
            "task_id": "main_agent_task/tool_publish",
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "task_type": "paper search",
            "worker_role": "literature_research_worker",
            "objective": "Find closest prior benchmark papers.",
            "required_output_artifact_type": "literature_evidence_packet",
            "required_output_fields": ["paper refs"],
            "acceptance_checks": ["closest-family coverage"],
            "failure_signals": ["generic unsourced prose"],
            "evidence_standard": "accepted stage-local worker evidence with source-grounded closest-family coverage",
            "priority": 1
        }]
    });

    let result = executor
        .execute(&call)
        .expect("publish_board_tasks should execute");

    assert_eq!(result.status, "succeeded");
    assert!(data_dir
        .join("main-agent-board")
        .join("tasks")
        .join("main_agent_task_tool_publish.json")
        .exists());
    let loaded = research_cli::orchestration::load_active_run(&data_dir)
        .expect("run should load")
        .expect("active run should exist");
    let pool_step = loaded
        .steps
        .iter()
        .find(|step| step.step_id == "goal_task_pool")
        .expect("goal task pool step should exist");
    assert!(pool_step
        .artifacts
        .iter()
        .any(|artifact| artifact == "main_agent_board_task::main_agent_task/tool_publish"));
}

#[test]
fn local_tool_executor_updates_merges_and_records_main_agent_control_requests() {
    let root = crate::support::unique_temp_dir("local_tool_executor_main_agent_control");
    let data_dir = root.join(".pmcli");
    fs::create_dir_all(&data_dir).expect("data dir should write");
    let mut run = research_cli::orchestration::OrchestrationRun::new(
        "run_main_agent_control",
        "structured main agent control",
        "approved",
    );
    run.automation_mode = research_cli::goals::GoalAutomationMode::FullAuto;
    run.steps = vec![research_cli::orchestration::OrchestrationStep::new(
        "goal_task_pool",
        "Goal task pool",
        research_cli::orchestration::OrchestrationStepStatus::Running,
    )];
    research_cli::orchestration::save_run(&data_dir, &run).expect("run should save");
    let executor = LocalToolExecutor::with_autonomous_research_context(
        &root,
        LocalToolExecutorContext {
            data_dir: data_dir.clone(),
            project_id: "project_tool_control".to_string(),
            job_id: Some("job_tool_control".to_string()),
            stage_id: Some("literature".to_string()),
            stage_execution_id: Some("stage_literature_1".to_string()),
            stage_artifact_type: Some("literature_matrix".to_string()),
        },
    );

    let mut publish = ToolCall::new("publish_board_tasks");
    publish.arguments = serde_json::json!({
        "tasks": [
            {
                "task_id": "main_agent_task/control_target",
                "stage_id": "literature",
                "stage_execution_id": "stage_literature_1",
                "task_type": "paper search",
                "worker_role": "literature_research_worker",
                "objective": "Find closest benchmark papers.",
                "required_output_artifact_type": "literature_evidence_packet",
                "required_output_fields": ["paper refs"],
                "acceptance_checks": ["closest-family coverage"],
                "failure_signals": ["generic unsourced prose"],
                "evidence_standard": "accepted stage-local worker evidence with benchmark paper refs",
                "priority": 1
            },
            {
                "task_id": "main_agent_task/control_duplicate",
                "stage_id": "literature",
                "stage_execution_id": "stage_literature_1",
                "task_type": "paper search duplicate",
                "worker_role": "literature_research_worker",
                "objective": "Duplicate paper search.",
                "required_output_artifact_type": "literature_evidence_packet",
                "required_output_fields": ["paper refs"],
                "acceptance_checks": ["closest-family coverage"],
                "failure_signals": ["generic unsourced prose"],
                "evidence_standard": "accepted stage-local worker evidence with benchmark paper refs",
                "priority": 2
            }
        ]
    });
    executor
        .execute(&publish)
        .expect("publish_board_tasks should execute");

    let mut update = ToolCall::new("update_board_task");
    update.arguments = serde_json::json!({
        "task_id": "main_agent_task/control_target",
        "patch": {
            "objective": "Find closest benchmark papers and extract evaluation protocol gaps.",
            "acceptance_checks": ["closest-family coverage", "evaluation protocol gap extracted"],
            "priority": 0
        },
        "rationale": "Review requires stronger benchmark grounding."
    });
    executor
        .execute(&update)
        .expect("update_board_task should execute");

    let updated_path = data_dir
        .join("main-agent-board")
        .join("tasks")
        .join("main_agent_task_control_target.json");
    let updated: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(updated_path).expect("updated task should read"))
            .expect("updated task should parse");
    assert_eq!(updated["priority"], 0);
    assert!(updated["objective"]
        .as_str()
        .unwrap_or_default()
        .contains("evaluation protocol gaps"));

    let mut merge = ToolCall::new("merge_board_tasks");
    merge.arguments = serde_json::json!({
        "source_ids": ["main_agent_task/control_duplicate"],
        "target_id": "main_agent_task/control_target",
        "rationale": "Both tasks ask for the same literature search."
    });
    executor
        .execute(&merge)
        .expect("merge_board_tasks should execute");

    let duplicate_path = data_dir
        .join("main-agent-board")
        .join("tasks")
        .join("main_agent_task_control_duplicate.json");
    let duplicate: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(duplicate_path).expect("merged source task should read"),
    )
    .expect("merged source task should parse");
    assert_eq!(duplicate["status"], "merged");
    assert_eq!(duplicate["merged_into"], "main_agent_task/control_target");

    let mut review = ToolCall::new("request_review_rerun");
    review.arguments = serde_json::json!({
        "stage_id": "literature",
        "stage_execution_id": "stage_literature_1",
        "readiness_refs": ["main_agent_board_task::main_agent_task/control_target"],
        "rationale": "Canonical literature task has been repaired and is ready for strict review."
    });
    executor
        .execute(&review)
        .expect("request_review_rerun should execute");

    let mut route = ToolCall::new("request_route_change");
    route.arguments = serde_json::json!({
        "operation": "pivot",
        "target_stage_id": "literature",
        "rationale": "The current benchmark framing changed after closest-family evidence.",
        "cleanup_required": true,
        "invalidated_refs": ["research/old-plan.md"],
        "readiness_refs": ["main_agent_board_task::main_agent_task/control_target"]
    });
    executor
        .execute(&route)
        .expect("request_route_change should execute");

    let mut cleanup = ToolCall::new("request_cleanup_plan");
    cleanup.arguments = serde_json::json!({
        "reason": "Pivot invalidated old benchmark notes.",
        "invalidated_refs": ["research/old-plan.md"],
        "cleanup_scope": "research artifacts only"
    });
    executor
        .execute(&cleanup)
        .expect("request_cleanup_plan should execute");

    assert!(data_dir
        .join("main-agent-board")
        .join("review_rerun_requests")
        .exists());
    assert!(data_dir
        .join("main-agent-board")
        .join("route_change_requests")
        .exists());
    assert!(data_dir
        .join("main-agent-board")
        .join("cleanup_plan_requests")
        .exists());

    let status = research_cli::goals::status(&data_dir, &root, "project_tool_control")
        .expect("goal status should build");
    let main_agent_entries = status
        .task_pool
        .entries
        .iter()
        .filter(|entry| entry.source.source_kind == "main_agent_published_task")
        .collect::<Vec<_>>();
    assert_eq!(main_agent_entries.len(), 1);
    assert_eq!(
        main_agent_entries[0].entry_id,
        "main_agent_task/control_target"
    );
    let active_run = research_cli::orchestration::load_active_run(&data_dir)
        .expect("active run should load")
        .expect("active run should exist");
    let pool_step = active_run
        .steps
        .iter()
        .find(|step| step.step_id == "goal_task_pool")
        .expect("goal task pool step should exist");
    assert!(pool_step
        .artifacts
        .iter()
        .any(|artifact| artifact.starts_with("main_agent_review_rerun_request::")));
    assert!(pool_step
        .artifacts
        .iter()
        .any(|artifact| artifact.starts_with("main_agent_route_change_request::")));
    assert!(pool_step
        .artifacts
        .iter()
        .any(|artifact| artifact.starts_with("main_agent_cleanup_plan_request::")));
}

#[test]
fn local_tool_executor_records_worker_artifact_decisions_without_applying_candidates() {
    let root = crate::support::unique_temp_dir("local_tool_executor_worker_artifact_decision");
    let data_dir = root.join(".pmcli");
    fs::create_dir_all(&data_dir).expect("data dir should write");
    let mut run = research_cli::orchestration::OrchestrationRun::new(
        "run_worker_artifact_decision",
        "structured worker artifact decision",
        "approved",
    );
    run.steps = vec![research_cli::orchestration::OrchestrationStep::new(
        "goal_task_pool",
        "Goal task pool",
        research_cli::orchestration::OrchestrationStepStatus::Running,
    )];
    research_cli::orchestration::save_run(&data_dir, &run).expect("run should save");
    let executor = LocalToolExecutor::with_autonomous_research_context(
        &root,
        LocalToolExecutorContext {
            data_dir: data_dir.clone(),
            project_id: "project_tool_worker_artifact_decision".to_string(),
            job_id: Some("job_tool_worker_artifact_decision".to_string()),
            stage_id: Some("implementation".to_string()),
            stage_execution_id: Some("stage_implementation_1".to_string()),
            stage_artifact_type: Some("implementation_report".to_string()),
        },
    );
    let mut call = ToolCall::new("record_worker_artifact_decision");
    call.arguments = serde_json::json!({
        "agent_id": "agent_worker_candidate",
        "task_id": "main_agent_task/implementation_worker",
        "candidate_refs": [
            "agent_worktree_artifact_candidate_manifest::agent_worker_candidate/worktree_artifact_candidates.json",
            "agent_worktree_patch::agent_worker_candidate/worktree_diff.patch"
        ],
        "decision": "accept",
        "rationale": "The isolated worker patch implements the requested benchmark harness and must proceed to strict review before canonical adoption.",
        "adoption_scope": "review-gated benchmark harness candidate",
        "canonical_target_refs": ["src/benchmark/harness.rs"],
        "review_required": true,
        "cleanup_required": false,
        "readiness_refs": ["review_packet::implementation_strict_review"]
    });

    let result = executor
        .execute(&call)
        .expect("worker artifact decision should execute");

    assert_eq!(result.status, "succeeded");
    let decision_ref = result
        .structured
        .as_ref()
        .and_then(|value| value.get("decision_ref"))
        .and_then(|value| value.as_str())
        .expect("decision ref should be returned");
    assert!(decision_ref.starts_with("main_agent_worker_artifact_decision::"));
    assert!(result
        .structured
        .as_ref()
        .and_then(|value| value.get("accepted_worker_evidence_projection_refs"))
        .and_then(|value| value.as_array())
        .is_some());
    let decision_dir = data_dir
        .join("main-agent-board")
        .join("worker-artifact-decisions");
    let decision_files = fs::read_dir(&decision_dir)
        .expect("decision dir should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("decision dir should list");
    assert_eq!(decision_files.len(), 1);
    let decision: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(decision_files[0].path()).expect("decision should read"),
    )
    .expect("decision should parse");
    assert_eq!(
        decision["schema_version"],
        "main_agent_worker_artifact_decision.v1"
    );
    assert_eq!(decision["decision"], "accept");
    assert_eq!(decision["review_required"], true);
    assert!(decision["authority_note"]
        .as_str()
        .unwrap_or_default()
        .contains("not canonical"));
    assert!(
        !root.join("src/benchmark/harness.rs").exists(),
        "recording a main-agent decision must not apply candidate patches"
    );

    let active_run = research_cli::orchestration::load_active_run(&data_dir)
        .expect("active run should load")
        .expect("active run should exist");
    let pool_step = active_run
        .steps
        .iter()
        .find(|step| step.step_id == "goal_task_pool")
        .expect("goal task pool step should exist");
    assert!(pool_step
        .artifacts
        .iter()
        .any(|artifact| artifact.starts_with("main_agent_worker_artifact_decision::")));
}

#[test]
fn local_tool_executor_rejects_hollow_main_agent_board_tasks() {
    let root = crate::support::unique_temp_dir("local_tool_executor_hollow_board_task");
    let data_dir = root.join(".pmcli");
    fs::create_dir_all(&data_dir).expect("data dir should write");
    let executor = LocalToolExecutor::with_autonomous_research_context(
        &root,
        LocalToolExecutorContext {
            data_dir: data_dir.clone(),
            project_id: "project_tool_hollow".to_string(),
            job_id: Some("job_tool_hollow".to_string()),
            stage_id: Some("literature".to_string()),
            stage_execution_id: Some("stage_literature_1".to_string()),
            stage_artifact_type: Some("literature_matrix".to_string()),
        },
    );
    let mut call = ToolCall::new("publish_board_tasks");
    call.arguments = serde_json::json!({
        "tasks": [{
            "task_id": "main_agent_task/hollow",
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "task_type": "paper search",
            "worker_role": "literature_research_worker",
            "objective": "Find papers.",
            "required_output_artifact_type": "literature_evidence_packet"
        }]
    });

    let error = executor
        .execute(&call)
        .expect_err("hollow board task must be rejected");

    assert_eq!(error.reason_code(), "invalid_argument");
    assert!(error.to_string().contains("required_output_fields"));
    assert!(!data_dir
        .join("main-agent-board")
        .join("tasks")
        .join("main_agent_task_hollow.json")
        .exists());
}

#[test]
fn local_tool_executor_times_out_shell_commands() {
    let root = crate::support::unique_temp_dir("local_tool_executor_shell_timeout");
    let executor = LocalToolExecutor::new(&root);
    let mut shell_call = ToolCall::new("shell");
    shell_call
        .parameters
        .insert("command".to_string(), "sleep 2".to_string());
    shell_call
        .parameters
        .insert("timeout_ms".to_string(), "1000".to_string());

    let result = executor
        .execute(&shell_call)
        .expect("timed-out shell should return a tool result");

    assert_eq!(result.status, "timeout");
    assert!(result
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("timed out"));
    let structured = result
        .structured
        .as_ref()
        .expect("timeout should expose structured payload");
    assert_eq!(structured["timed_out"], true);
    assert_eq!(structured["timeout_ms"], 1000);
    assert!(format_tool_result_for_llm(&result).contains("timed out"));
}

#[test]
fn tool_result_format_prefers_error_when_output_is_empty() {
    let result = ToolResult {
        tool_name: "shell".to_string(),
        status: "timeout".to_string(),
        exit_code: None,
        output: Some(String::new()),
        structured: None,
        error_message: Some("shell command timed out after 1000 ms".to_string()),
    };

    assert_eq!(
        format_tool_result_for_llm(&result),
        "Error: shell command timed out after 1000 ms"
    );
}

#[test]
fn local_tool_executor_fetches_http_url_as_read_only_web_tool() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let root = crate::support::unique_temp_dir("local_tool_executor_fetch");
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener
        .local_addr()
        .expect("test server should expose addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test server should accept");
        let mut buffer = [0u8; 1024];
        let _ = stream.read(&mut buffer).expect("request should read");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\n\r\nhello fetch\n",
            )
            .expect("response should write");
    });

    let executor = LocalToolExecutor::new(&root);
    let mut fetch_call = ToolCall::new("fetch");
    fetch_call
        .parameters
        .insert("url".to_string(), format!("http://{addr}/fixture"));
    fetch_call
        .parameters
        .insert("allow_private_network".to_string(), "true".to_string());
    let fetch_result = executor.execute(&fetch_call).expect("fetch should execute");
    handle.join().expect("server thread should join");

    assert_eq!(fetch_result.tool_name, "fetch");
    assert_eq!(fetch_result.status, "succeeded");
    assert_eq!(fetch_result.output.as_deref(), Some("hello fetch\n"));
    let structured = fetch_result
        .structured
        .as_ref()
        .expect("fetch should return structured payload");
    assert_eq!(structured["status_code"], 200);
    assert_eq!(structured["url"], format!("http://{addr}/fixture"));
    assert_eq!(structured["bytes"], 12);
}

#[test]
fn local_tool_executor_fetch_decodes_chunked_and_reports_redirect_chain() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let root = crate::support::unique_temp_dir("local_tool_executor_fetch_redirect");
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener
        .local_addr()
        .expect("test server should expose addr");
    let handle = thread::spawn(move || {
        for response in [
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{addr}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n6\r\nhello \r\n5\r\nworld\r\n0\r\n\r\n".to_string(),
        ] {
            let (mut stream, _) = listener.accept().expect("test server should accept");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer).expect("request should read");
            stream
                .write_all(response.as_bytes())
                .expect("response should write");
        }
    });

    let executor = LocalToolExecutor::new(&root);
    let mut fetch_call = ToolCall::new("fetch");
    fetch_call
        .parameters
        .insert("url".to_string(), format!("http://{addr}/start"));
    fetch_call
        .parameters
        .insert("allow_private_network".to_string(), "true".to_string());
    let fetch_result = executor.execute(&fetch_call).expect("fetch should execute");
    handle.join().expect("server thread should join");

    assert_eq!(fetch_result.status, "succeeded");
    assert_eq!(fetch_result.output.as_deref(), Some("hello world"));
    let structured = fetch_result
        .structured
        .as_ref()
        .expect("fetch should return structured payload");
    assert_eq!(structured["status_code"], 200);
    assert_eq!(structured["redirect_count"], 1);
    assert_eq!(
        structured["redirect_chain"][0],
        format!("http://{addr}/final")
    );
    assert_eq!(structured["bytes"], 11);
}

#[test]
fn local_tool_executor_rejects_binary_and_oversized_text_reads() {
    let root = crate::support::unique_temp_dir("local_tool_executor_file_safety");
    fs::write(root.join("binary.dat"), [0, 1, 2, 3]).expect("binary file should write");
    fs::write(root.join("large.txt"), "x".repeat(10 * 1024 * 1024 + 1))
        .expect("large file should write");
    let executor = LocalToolExecutor::new(&root);

    let mut binary_call = ToolCall::new("read_file");
    binary_call.target_path = Some("binary.dat".to_string());
    let binary_err = executor
        .execute(&binary_call)
        .expect_err("binary read should fail");
    assert_eq!(binary_err.reason_code(), "invalid_argument");
    assert!(binary_err.to_string().contains("binary"));

    let mut large_call = ToolCall::new("read_file");
    large_call.target_path = Some("large.txt".to_string());
    let large_err = executor
        .execute(&large_call)
        .expect_err("oversized read should fail");
    assert_eq!(large_err.reason_code(), "invalid_argument");
    assert!(large_err.to_string().contains("too large"));
}

#[cfg(unix)]
#[test]
fn local_tool_executor_rejects_symlink_workspace_escape() {
    let root = crate::support::unique_temp_dir("local_tool_symlink_escape");
    let outside = crate::support::unique_temp_dir("local_tool_symlink_escape_outside");
    std::os::unix::fs::symlink(&outside, root.join("link_out"))
        .expect("symlink fixture should be created");
    let executor = LocalToolExecutor::new(&root);

    let mut write_call = ToolCall::new("write_file");
    write_call.target_path = Some("link_out/escaped.txt".to_string());
    write_call.content = Some("escape".to_string());

    let err = executor
        .execute(&write_call)
        .expect_err("symlink escape should be rejected");
    assert!(
        err.to_string().contains("workspace boundary"),
        "unexpected error: {err}"
    );
    assert!(!outside.join("escaped.txt").exists());
}
