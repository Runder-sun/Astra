use crate::support::{init_git_workspace, unique_temp_dir, write_text_file};
use jsonschema::JSONSchema;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn schema_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(name)
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("provider")
        .join(name)
}

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn registry_root_env(label: &str) -> String {
    unique_temp_dir(label).display().to_string()
}

fn output_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid json")
}

fn read_json_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("jsonl file should read")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("jsonl line should parse"))
        .collect()
}

fn write_skill_manifest(root: &Path, skill_id: &str) {
    write_text_file(
        root.join(skill_id).join("SKILL.md"),
        &format!("# {skill_id}\n\nschema test fixture\n"),
    );
}

fn write_plugin_manifest(root: &Path, plugin_id: &str) {
    write_text_file(
        root.join(plugin_id)
            .join(".codex-plugin")
            .join("plugin.json"),
        &json!({
            "id": plugin_id,
            "enabled": true,
            "tools": ["fixture_tool"],
            "hooks": {
                "pre_tool_use": ["hooks/pre.sh"]
            }
        })
        .to_string(),
    );
    write_text_file(
        root.join(plugin_id).join("hooks").join("pre.sh"),
        "#!/usr/bin/env bash\nprintf 'pre'\n",
    );
}

fn write_mcp_manifest(root: &Path, server_id: &str) {
    write_text_file(
        root.join(format!("{server_id}.json")),
        &json!({
            "id": server_id,
            "transport": "stdio",
            "command": "uvx demo-mcp",
            "tools": ["search", "fetch"],
            "tool_count": 2,
            "auth_state": "configured",
            "auth_source": "env",
            "timeout_ms": 2500
        })
        .to_string(),
    );
}

fn compiled_schema(name: &str) -> JSONSchema {
    let contents = fs::read_to_string(schema_path(name)).expect("schema should exist");
    let mut schema: Value = serde_json::from_str(&contents).expect("schema should parse");
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "$id".to_string(),
            Value::String(format!("https://research-cli.local/schemas/{name}")),
        );
    }
    JSONSchema::compile(&schema).expect("schema should compile")
}

fn assert_schema_validates(validator: &JSONSchema, instance: &Value, label: &str) {
    if let Err(errors) = validator.validate(instance) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("{label} should validate against schema: {rendered:?}");
    }
}

#[test]
fn mission_frame_schema_accepts_legacy_frames_without_automation_mode() {
    let mission_frame_schema = compiled_schema("mission_frame.schema.json");
    let legacy_frame = json!({
        "frame_id": "mission_frame",
        "version": 1,
        "project_id": "project",
        "project_max_goal": "Keep the research loop coherent.",
        "milestone_goal": "Preserve legacy mission frame validation.",
        "current_implementation_goal": "Accept frames created before automation mode.",
        "priority_rule": "project_max_goal > milestone_goal > current_implementation_goal",
        "non_goals": [],
        "success_criteria": [],
        "evidence_refs": [],
        "risk_notes": [],
        "updated_at": "1"
    });

    assert_schema_validates(&mission_frame_schema, &legacy_frame, "legacy mission frame");

    let mission_frame_status_schema = compiled_schema("mission_frame_status.schema.json");
    let legacy_status = json!({
        "status": "active",
        "project_id": "project",
        "mission_frame_ref": ".pmcli/project_goals/mission_frame.json",
        "mission_frame": legacy_frame,
        "projection": {
            "mission_frame_ref": ".pmcli/project_goals/mission_frame.json",
            "project_max_goal": "Keep the research loop coherent.",
            "milestone_goal": "Preserve legacy mission frame validation.",
            "current_implementation_goal": "Accept frames created before automation mode.",
            "priority_rule": "project_max_goal > milestone_goal > current_implementation_goal",
            "automation_mode": "human_in_the_loop"
        },
        "task_pool": {
            "schema_version": "goal_task_pool_snapshot.v1",
            "authority_model": "derived_from_existing_goal_orchestration_research_authorities",
            "source": "mission_frame_orchestration_research_agents_reviews_projectops_policy",
            "project_id": "project",
            "automation_mode": "human_in_the_loop",
            "target_run": null,
            "summary": {
                "total": 0,
                "ready_to_run": 0,
                "running": 0,
                "needs_approval": 0,
                "needs_review": 0,
                "blocked": 0
            },
            "entries": [],
            "next_recommended_action": "discuss_or_refine_goal"
        },
        "warnings": []
    });

    assert_schema_validates(
        &mission_frame_status_schema,
        &legacy_status,
        "legacy mission frame status",
    );
}

#[test]
fn provider_resolution_trace_schema_rejects_unknown_fields_and_locks_sources_shape() {
    let contents = fs::read_to_string(schema_path("provider_resolution_trace.schema.json"))
        .expect("provider schema should exist");
    let mut schema: Value = serde_json::from_str(&contents).expect("provider schema should parse");
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "$id".to_string(),
            Value::String(
                "https://research-cli.local/schemas/provider_resolution_trace.schema.json"
                    .to_string(),
            ),
        );
    }

    assert_eq!(
        schema["additionalProperties"],
        Value::Bool(false),
        "provider trace schema must reject unknown top-level fields"
    );
    assert_eq!(
        schema["properties"]["sources"]["additionalProperties"],
        Value::Bool(false),
        "provider trace sources must reject unknown keys"
    );
    let source_properties = schema["properties"]["sources"]["properties"]
        .as_object()
        .expect("sources.properties should be an object");
    assert!(source_properties.contains_key("model"));
    assert!(source_properties.contains_key("provider"));

    let validator = JSONSchema::compile(&schema).expect("provider schema should compile");
    let invalid_fixture =
        fs::read_to_string(fixture_path("provider_resolution_trace.invalid.json"))
            .expect("invalid fixture should be readable");
    let invalid_instance: Value =
        serde_json::from_str(&invalid_fixture).expect("invalid fixture should parse");
    assert!(
        validator.validate(&invalid_instance).is_err(),
        "invalid provider trace fixture should be rejected by schema"
    );
}

#[test]
fn doc_context_result_schemas_enforce_strict_docframe_items() {
    for schema_name in [
        "turn_result.schema.json",
        "resume_result.schema.json",
        "compact_result.schema.json",
    ] {
        let contents = fs::read_to_string(schema_path(schema_name)).expect("schema should exist");
        let schema: Value = serde_json::from_str(&contents).expect("schema should parse");
        let doc_context = schema["properties"]["doc_context"].clone();
        let strict_shape = if schema_name == "compact_result.schema.json" {
            doc_context["anyOf"][0]["properties"]["doc_frames"]["items"].clone()
        } else {
            doc_context["properties"]["doc_frames"]["items"].clone()
        };
        let required = strict_shape["required"].as_array().unwrap_or_else(|| {
            panic!("{schema_name} doc_frames items must declare required fields")
        });
        assert!(
            required.iter().any(|value| value == "doc_id"),
            "{schema_name} must enforce DocFrame doc_id"
        );
        assert_eq!(
            strict_shape["additionalProperties"],
            Value::Bool(false),
            "{schema_name} must reject unknown DocFrame fields"
        );
        let properties = if schema_name == "compact_result.schema.json" {
            &doc_context["anyOf"][0]["properties"]
        } else {
            &doc_context["properties"]
        };
        assert!(
            properties.get("injected_count").is_some(),
            "{schema_name} must expose injected_count"
        );
        assert!(
            properties.get("omitted_active_count").is_some(),
            "{schema_name} must expose omitted_active_count"
        );
    }
}

#[test]
fn batch10_enforcement_entrypoints_exist_and_reference_conformance_jobs() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let conformance_runner = repo_root.join("tests/conformance/run_conformance.sh");
    let golden_runner = repo_root.join("tests/golden/run_golden.sh");
    let ci_workflow = repo_root.join(".github/workflows/pr-fast.yml");

    assert!(conformance_runner.exists(), "missing conformance runner");
    assert!(golden_runner.exists(), "missing golden runner");
    assert!(ci_workflow.exists(), "missing CI workflow");

    let conformance_contents =
        fs::read_to_string(&conformance_runner).expect("conformance runner should be readable");
    assert!(conformance_contents.contains("test --test conformance"));
    assert!(conformance_contents.contains("run_mock_parity_harness.sh"));

    let golden_contents =
        fs::read_to_string(&golden_runner).expect("golden runner should be readable");
    assert!(golden_contents.contains("test --test conformance golden_"));

    let workflow_contents = fs::read_to_string(&ci_workflow).expect("workflow should be readable");
    assert!(workflow_contents.contains("tests/conformance/run_conformance.sh"));
    assert!(workflow_contents.contains("tests/golden/run_golden.sh"));
}

#[test]
fn batch10_foundation_surface_schemas_exist_and_compile() {
    for schema_name in [
        "command_success.schema.json",
        "command_failure.schema.json",
        "project_state.schema.json",
        "event.schema.json",
        "help_surface_report.schema.json",
        "palette_surface_report.schema.json",
        "interactive_launch_result.schema.json",
        "turn_result.schema.json",
        "resume_result.schema.json",
        "profile_resolution_trace.schema.json",
        "interrupt_result.schema.json",
        "retry_result.schema.json",
        "undo_refusal.schema.json",
        "policy_refusal_result.schema.json",
        "followup_required_result.schema.json",
        "session.schema.json",
        "session_identity.schema.json",
        "session_lineage.schema.json",
        "session_title_record.schema.json",
        "resume_recap.schema.json",
        "session_browse_result.schema.json",
        "resume_ambiguity_result.schema.json",
        "session_search_result.schema.json",
        "session_export_result.schema.json",
        "session_stats.schema.json",
        "session_summary.schema.json",
        "compact_result.schema.json",
        "session_inspection.schema.json",
        "project_inspection_core.schema.json",
        "project_inspection.schema.json",
        "goal_alignment_trace.schema.json",
        "project_registry_entry.schema.json",
        "current_project_pointer.schema.json",
        "project_resolution_trace.schema.json",
        "project_registry_list.schema.json",
        "project_status.schema.json",
        "git_state_snapshot.schema.json",
        "canonical_surface_manifest.schema.json",
        "canonical_lineage_manifest.schema.json",
        "code_owner_manifest.schema.json",
        "canonicality_audit_report.schema.json",
        "runtime_preflight_report.schema.json",
        "doctor_report.schema.json",
        "smoke_result.schema.json",
        "feature_gate_result.schema.json",
        "provider_status.schema.json",
        "provider_catalog_state.schema.json",
        "provider_resolution.schema.json",
        "provider_status_list.schema.json",
        "model_catalog_list.schema.json",
        "model_current_result.schema.json",
        "effective_config_report.schema.json",
        "config_value_result.schema.json",
        "config_source_report.schema.json",
        "provider_catalog_refresh_result.schema.json",
        "prune_result.schema.json",
        "permission_pending_list.schema.json",
        "permission_request.schema.json",
        "permission_decision_trace.schema.json",
        "permission_history_result.schema.json",
        "runtime_feature_status.schema.json",
        "conformance_result.schema.json",
        "setup_status_report.schema.json",
        "migrate_check_report.schema.json",
        "repair_hints_report.schema.json",
        "install_routes_report.schema.json",
        "skill_registry_entry.schema.json",
        "skill_list_result.schema.json",
        "skill_inspect_result.schema.json",
        "skill_paths_result.schema.json",
        "skill_validation_result.schema.json",
        "skill_availability_record.schema.json",
        "skill_output_artifact.schema.json",
        "skill_output_envelope.schema.json",
        "skill_output_list_result.schema.json",
        "skill_output_inspection_result.schema.json",
        "skill_output_submit_result.schema.json",
        "skill_run_result.schema.json",
        "skill_output_publish_result.schema.json",
        "skill_publication_gate.schema.json",
        "skill_output_context_projection.schema.json",
        "skill_docframe_publication.schema.json",
        "plugin_list_result.schema.json",
        "plugin_inspection_result.schema.json",
        "plugin_validation_result.schema.json",
        "hook_list_result.schema.json",
        "hook_inspection_result.schema.json",
        "hook_test_result.schema.json",
        "mcp_list_result.schema.json",
        "mcp_server_record.schema.json",
        "mcp_inspection_result.schema.json",
        "mcp_test_result.schema.json",
        "mcp_refresh_result.schema.json",
        "plugin_record.schema.json",
        "artifact_family.schema.json",
        "hook_record.schema.json",
        "artifact_list_result.schema.json",
        "artifact_inspection_result.schema.json",
        "artifact_promotion_candidate.schema.json",
        "artifact_promotion_queue_result.schema.json",
        "artifact_promotion_result.schema.json",
        "repo_cleanup_proposal.schema.json",
        "review_packet.schema.json",
        "review_trace.schema.json",
        "review_list_result.schema.json",
        "review_inspection_result.schema.json",
        "review_open_result.schema.json",
        "doc_frame.schema.json",
        "doc_index.schema.json",
        "mission_frame.schema.json",
        "mission_frame_status.schema.json",
        "working_memory_record.schema.json",
        "memory_record.schema.json",
        "memory_query_result.schema.json",
        "memory_explain_record.schema.json",
        "memory_status_report.schema.json",
        "trajectory_ingest_record.schema.json",
        "task_segment_record.schema.json",
        "segment_quality_record.schema.json",
        "segment_memory_candidate.schema.json",
        "memory_adoption_event.schema.json",
        "memory_tier_explanation.schema.json",
        "knowledge_cluster_record.schema.json",
        "skill_evolution_candidate.schema.json",
        "skill_provenance_verification.schema.json",
        "skill_structure_score.schema.json",
        "skill_install_record.schema.json",
        "agent_feedback_signal.schema.json",
        "feedback_calibration_record.schema.json",
        "progress_digest_candidate.schema.json",
        "projectops_tick.schema.json",
        "machine_identity.schema.json",
        "remote_client_identity.schema.json",
        "pair_ticket.schema.json",
        "remote_lease.schema.json",
        "remote_binding.schema.json",
        "remote_cursor.schema.json",
        "control_lease.schema.json",
        "remote_control_owner.schema.json",
        "remote_machine_metadata.schema.json",
        "remote_daemon_state.schema.json",
        "remote_capability_set.schema.json",
        "remote_capability_matrix.schema.json",
        "offline_action_policy.schema.json",
        "local_remote_switch_policy.schema.json",
        "remote_feature_advertisement.schema.json",
        "session_runtime_descriptor.schema.json",
        "session_envelope_projection_v1.schema.json",
        "remote_projection_snapshot.schema.json",
        "remote_terminal_lease.schema.json",
        "remote_status_report.schema.json",
        "remote_action_rejection.schema.json",
        "host_surface_projection.schema.json",
        "host_surface_action.schema.json",
        "tui_launch_result.schema.json",
        "tailscale_status_report.schema.json",
        "remote_terminal_projection.schema.json",
        "task_packet.schema.json",
        "agent_runtime_identity.schema.json",
        "agent_runtime_record.schema.json",
        "agent_trace.schema.json",
        "agent_output_manifest.schema.json",
        "agent_workspace_binding.schema.json",
        "agent_worktree_artifact_candidate_manifest.schema.json",
        "main_agent_worker_artifact_decision.schema.json",
        "search_batch_record.schema.json",
        "branch_run_record.schema.json",
        "branch_list_result.schema.json",
        "branch_inspection_result.schema.json",
        "branch_mutation_result.schema.json",
        "evaluation_packet.schema.json",
        "debate_trace.schema.json",
        "verifier_criterion.schema.json",
        "verifier_pair_score.schema.json",
        "verifier_tournament.schema.json",
        "promotion_decision.schema.json",
        "branch_archive_result.schema.json",
        "research_thread.schema.json",
        "deliberation_span.schema.json",
        "research_stage_execution.schema.json",
        "stage_execution_map.schema.json",
        "research_status_report.schema.json",
        "research_thread_list_result.schema.json",
        "research_thread_inspection.schema.json",
        "research_stage_inspection.schema.json",
        "research_decision_record.schema.json",
        "research_record_result.schema.json",
        "research_turn_classification.schema.json",
        "change_envelope.schema.json",
        "debate_packet.schema.json",
        "research_debate_trace.schema.json",
        "research_skill_contract.schema.json",
        "research_role_template.schema.json",
        "tool_selection_trace.schema.json",
        "stage_reflection_packet.schema.json",
        "hitl_pending_gate.schema.json",
        "tool_error_recovery.schema.json",
        "research_ux_middleware_result.schema.json",
        "skill_manifest.schema.json",
        "skill_output_artifact.schema.json",
        "skill_output_envelope.schema.json",
        "skill_output_list_result.schema.json",
        "skill_output_inspection_result.schema.json",
        "skill_output_submit_result.schema.json",
        "skill_run_result.schema.json",
        "skill_output_publish_result.schema.json",
        "skill_publication_gate.schema.json",
        "skill_output_context_projection.schema.json",
        "skill_docframe_publication.schema.json",
    ] {
        let _ = compiled_schema(schema_name);
    }
}

#[test]
fn main_agent_worker_artifact_decision_schema_requires_explicit_candidate_decision() {
    let schema = compiled_schema("main_agent_worker_artifact_decision.schema.json");
    let valid = json!({
        "schema_version": "main_agent_worker_artifact_decision.v1",
        "decision_id": "worker_artifact_decision_project_1",
        "decision_ref": "main_agent_worker_artifact_decision::worker_artifact_decision_project_1",
        "agent_id": "agent_worker_1",
        "task_id": "main_agent_task/implementation_worker",
        "candidate_refs": [
            "agent_worktree_artifact_candidate_manifest::agent_worker_1/worktree_artifact_candidates.json"
        ],
        "decision": "accept",
        "rationale": "The worker candidate is ready for strict review before canonical adoption.",
        "adoption_scope": "review-gated implementation candidate",
        "canonical_target_refs": ["src/benchmark/harness.rs"],
        "review_required": true,
        "cleanup_required": false,
        "readiness_refs": ["review_packet::implementation"],
        "project_id": "project",
        "job_id": "job",
        "stage_id": "implement-solution",
        "stage_execution_id": "stage_implement_1",
        "created_at": "1",
        "published_by": "main_agent",
        "authority_note": "candidate-only worker artifacts are not canonical until reviewed"
    });
    assert_schema_validates(&schema, &valid, "valid worker artifact decision");

    let invalid = json!({
        "schema_version": "main_agent_worker_artifact_decision.v1",
        "decision_id": "worker_artifact_decision_project_1",
        "decision_ref": "main_agent_worker_artifact_decision::worker_artifact_decision_project_1",
        "agent_id": "agent_worker_1",
        "task_id": "main_agent_task/implementation_worker",
        "candidate_refs": [],
        "decision": "accept",
        "rationale": "No candidates should fail.",
        "canonical_target_refs": [],
        "review_required": true,
        "cleanup_required": false,
        "readiness_refs": [],
        "project_id": "project",
        "created_at": "1",
        "published_by": "main_agent",
        "authority_note": "candidate-only worker artifacts are not canonical until reviewed"
    });
    assert!(
        schema.validate(&invalid).is_err(),
        "worker artifact decision must reject empty candidate refs"
    );
}

#[test]
fn task_packet_schema_accepts_stage_contract_and_collaboration_protocol() {
    let schema = compiled_schema("task_packet.schema.json");
    let valid = json!({
        "schema_version": "v1alpha1",
        "task_packet_id": "task_1",
        "agent_id": "agent_1",
        "runner_kind": "provider",
        "intent": "dispatch stage task",
        "role_profile": "literature_researcher",
        "retention_policy": "ephemeral",
        "run_class": "bounded",
        "io_mode": "request_response",
        "resume_policy": "fresh_thread",
        "replay_seed_ref": "",
        "budget": {
            "max_turns": 1,
            "max_runtime_ms": 120000
        },
        "scope": {
            "workspace_root": "/tmp/astra",
            "allowed_paths": ["."]
        },
        "write_authority": "workspace_write",
        "success_criteria": ["evidence returned"],
        "output_manifest_required": true,
        "review_gate_required": true,
        "stage_task_contract": {
            "schema_version": "agent_stage_task_contract.v1",
            "task_id": "main_agent_task/literature/paper_search",
            "stage_execution_id": "stage_literature_1",
            "stage_id": "literature",
            "task_type": "paper search",
            "worker_role": "literature_researcher",
            "objective": "Find closest prior work.",
            "input_artifact_refs": ["prompt_sha256:abc"],
            "required_output_artifact_type": "literature_matrix",
            "required_output_fields": ["source refs"],
            "acceptance_checks": ["closest-family coverage"],
            "failure_signals": ["invented citations"],
            "depends_on_task_ids": [],
            "priority": 1,
            "review_findings_refs": []
        },
        "collaboration_protocol": {
            "schema_version": "agent_collaboration_protocol.v1",
            "protocol_id": "astra_main_runtime_worker_v1",
            "main_agent_authority": "owns research strategy",
            "runtime_authority": "persists and enforces only",
            "worker_authority": "executes only the assigned TaskPacket",
            "required_worker_capabilities": ["source-grounded evidence"],
            "required_worker_output_sections": ["evidence produced"],
            "forbidden_worker_actions": ["publish board tasks"],
            "artifact_flow": "candidate evidence then main-agent decision",
            "adoption_rule": "candidate-only until accepted",
            "review_rule": "strict review required",
            "cleanup_rule": "main-agent route decision decides cleanup"
        },
        "message": "Produce evidence.",
        "created_at": "1"
    });
    assert_schema_validates(&schema, &valid, "task packet with collaboration protocol");

    let mut invalid = valid.clone();
    invalid["collaboration_protocol"]["forbidden_worker_actions"] = json!([]);
    assert!(
        schema.validate(&invalid).is_err(),
        "collaboration_protocol must require forbidden worker actions"
    );
}

#[test]
fn m15_trajectory_collective_memory_schemas_lock_governed_learning_contracts() {
    let ingest_schema = compiled_schema("trajectory_ingest_record.schema.json");
    assert_schema_validates(
        &ingest_schema,
        &json!({
            "schema_version": "trajectory.ingest_record.v1",
            "conformance_line": "M15.trajectory_ingest",
            "canonical_path": ".pmcli/trajectory/ingests/ing_1.json",
            "retention_policy": "project_local_until_explicit_cleanup",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "ingest_id": "ing_1",
            "project_id": "project_1",
            "source_kind": "cli_session",
            "source_agent_id": "local",
            "source_session_id": "session_1",
            "source_path": "fixtures/session.jsonl",
            "source_digest": "abc123",
            "normalized_format": "jsonl.v1",
            "ingest_status": "pending_segmentation",
            "segment_count": 0,
            "degraded_reasons": [],
            "created_at": "1",
            "updated_at": "1"
        }),
        "trajectory ingest record",
    );

    let segment_schema = compiled_schema("task_segment_record.schema.json");
    assert_schema_validates(
        &segment_schema,
        &json!({
            "schema_version": "trajectory.task_segment.v1",
            "conformance_line": "M15.task_segment",
            "canonical_path": ".pmcli/trajectory/segments/seg_1.json",
            "retention_policy": "project_local_until_source_segment_superseded",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "segment_id": "seg_1",
            "ingest_id": "ing_1",
            "project_id": "project_1",
            "segment_index": 0,
            "start_line": 1,
            "end_line": 2,
            "message_count": 2,
            "fingerprint": "0123456789abcdef",
            "topic": "build docs",
            "summary": "build docs",
            "segmentation_method": "manual_fixture",
            "segmentation_model": "",
            "segmentation_prompt_digest": "",
            "quality_status": "pending",
            "memory_extraction_status": "not_started",
            "segment_tag": "segment:seg_1",
            "status": "active",
            "supersedes": [],
            "superseded_by": "",
            "created_at": "1",
            "updated_at": "1"
        }),
        "task segment record",
    );

    let quality_schema = compiled_schema("segment_quality_record.schema.json");
    assert_schema_validates(
        &quality_schema,
        &json!({
            "schema_version": "trajectory.segment_quality.v1",
            "conformance_line": "M15.segment_quality",
            "canonical_path": ".pmcli/trajectory/quality/q_1.json",
            "retention_policy": "append_only_quality_evidence",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "quality_id": "q_1",
            "segment_id": "seg_1",
            "provider": "deterministic",
            "model": "rules.v1",
            "criteria": {
                "task_completion": 1.0,
                "evidence_density": 1.0,
                "artifact_linkage": 1.0,
                "contradiction_risk": 0.0,
                "privacy_risk": 0.0,
                "reuse_value": 0.8
            },
            "overall_score": 0.9,
            "memory_eligible": true,
            "failure_reason": "",
            "evidence_refs": ["docs/example.md"],
            "created_at": "1"
        }),
        "segment quality record",
    );

    let candidate_schema = compiled_schema("segment_memory_candidate.schema.json");
    assert_schema_validates(
        &candidate_schema,
        &json!({
            "schema_version": "trajectory.memory_candidate.v1",
            "conformance_line": "M15.segment_memory_candidate",
            "canonical_path": ".pmcli/memory/promotion_queue/cand_1.json",
            "retention_policy": "reviewable_until_promoted_or_invalidated",
            "atomic_write_policy": "atomic_json_write",
            "candidate_id": "cand_1",
            "segment_id": "seg_1",
            "ingest_id": "ing_1",
            "project_id": "project_1",
            "title": "build docs",
            "summary": "build docs",
            "body": "Created docs/example.md",
            "memory_kind": "trajectory_segment",
            "support_refs": ["segment:seg_1", "docs/example.md"],
            "source_artifacts": ["docs/example.md"],
            "source_artifact_path": "docs/example.md",
            "segment_tag": "segment:seg_1",
            "quality_id": "q_1",
            "promotion_status": "queued",
            "created_at": "1",
            "updated_at": "1"
        }),
        "segment memory candidate",
    );

    let adoption_schema = compiled_schema("memory_adoption_event.schema.json");
    assert_schema_validates(
        &adoption_schema,
        &json!({
            "schema_version": "memory.adoption_event.v1",
            "conformance_line": "M15.memory_adoption",
            "canonical_path": ".pmcli/memory/adoption_events.jsonl",
            "retention_policy": "append_only_adoption_evidence",
            "atomic_write_policy": "append_jsonl",
            "event_id": "adopt_1",
            "memory_record_id": "mem_1",
            "segment_id": "seg_1",
            "event_kind": "inspected",
            "weight": 2.0,
            "surface": "cli",
            "actor": "operator",
            "support_ref": "memory explain",
            "created_at": "1"
        }),
        "memory adoption event",
    );

    let tier_schema = compiled_schema("memory_tier_explanation.schema.json");
    assert_schema_validates(
        &tier_schema,
        &json!({
            "schema_version": "memory.tier_explanation.v1",
            "conformance_line": "M15.memory_tier",
            "canonical_path": ".pmcli/memory/tiers/mem_1.json",
            "retention_policy": "derived_rebuildable_projection",
            "atomic_write_policy": "atomic_json_write",
            "memory_record_id": "mem_1",
            "tier": "HOT",
            "adoption_score": 3.0,
            "event_counts": {"inspected": 1},
            "last_positive_event_at": "1",
            "negative_event_count": 0,
            "ranking_reason": "positive adoption events",
            "governance_limits": [
                "tier_does_not_override_invalidated_or_superseded_memory"
            ],
            "created_at": "1",
            "updated_at": "1"
        }),
        "memory tier explanation",
    );
}

#[test]
fn m16_m18_evolution_and_feedback_schemas_lock_governed_learning_boundaries() {
    let cluster_schema = compiled_schema("knowledge_cluster_record.schema.json");
    assert_schema_validates(
        &cluster_schema,
        &json!({
            "schema_version": "skill.knowledge_cluster.v1",
            "conformance_line": "M16.knowledge_cluster",
            "canonical_path": ".pmcli/skills/evolution/clusters/cluster_1.json",
            "retention_policy": "project_local_until_cluster_superseded",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "cluster_id": "cluster_1",
            "project_id": "project_1",
            "cluster_topic": "canonical docs",
            "centroid_method": "deterministic_token_overlap",
            "similarity_threshold": 0.75,
            "crystallization_threshold": 2,
            "memory_ids": ["mem_1", "mem_2"],
            "memory_count": 2,
            "support_refs": ["docs/a.md"],
            "cluster_status": "ready_for_crystallization",
            "supersedes": [],
            "created_at": "1",
            "updated_at": "1"
        }),
        "knowledge cluster record",
    );

    let candidate_schema = compiled_schema("skill_evolution_candidate.schema.json");
    assert_schema_validates(
        &candidate_schema,
        &json!({
            "schema_version": "skill.evolution_candidate.v1",
            "conformance_line": "M16.skill_evolution_candidate",
            "canonical_path": ".pmcli/skills/evolution/candidates/cand_1.json",
            "retention_policy": "project_local_until_candidate_superseded_or_rejected",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "candidate_id": "cand_1",
            "cluster_id": "cluster_1",
            "project_id": "project_1",
            "skill_id": "evolved.canonical-docs",
            "title": "Canonical Docs Workflow",
            "category": "research_workflow",
            "trigger": "Use when canonical docs are required",
            "skill_artifact_path": "docs/skill_candidates/cand_1/SKILL.md",
            "source_memory_ids": ["mem_1", "mem_2"],
            "support_refs": ["docs/a.md"],
            "verification_status": "pending",
            "publication_status": "not_submitted",
            "created_at": "1",
            "updated_at": "1"
        }),
        "skill evolution candidate",
    );

    let verification_schema = compiled_schema("skill_provenance_verification.schema.json");
    assert_schema_validates(
        &verification_schema,
        &json!({
            "schema_version": "skill.provenance_verification.v1",
            "conformance_line": "M16.skill_provenance_verification",
            "canonical_path": ".pmcli/skills/evolution/verifications/ver_1.json",
            "retention_policy": "append_only_skill_evolution_evidence",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "verification_id": "ver_1",
            "candidate_id": "cand_1",
            "grounded_claim_count": 2,
            "hallucinated_claim_count": 0,
            "contradicted_claim_count": 0,
            "grounded_evidence_ratio": 1.0,
            "decision": "verified",
            "evidence_refs": ["docs/a.md"],
            "created_at": "1"
        }),
        "skill provenance verification",
    );

    let structure_schema = compiled_schema("skill_structure_score.schema.json");
    assert_schema_validates(
        &structure_schema,
        &json!({
            "schema_version": "skill.structure_score.v1",
            "conformance_line": "M16.skill_structure_score",
            "canonical_path": ".pmcli/skills/evolution/structure_scores/score_1.json",
            "retention_policy": "append_only_skill_evolution_evidence",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "score_id": "score_1",
            "candidate_id": "cand_1",
            "workflow_clarity": 1.0,
            "specificity_and_reusability": 1.0,
            "preserves_existing_value": 1.0,
            "overall_score": 1.0,
            "upgrade_decision": "upgrade_allowed",
            "created_at": "1"
        }),
        "skill structure score",
    );

    let install_schema = compiled_schema("skill_install_record.schema.json");
    assert_schema_validates(
        &install_schema,
        &json!({
            "schema_version": "skill.install_record.v1",
            "conformance_line": "M16.skill_install",
            "canonical_path": ".pmcli/skills/evolution/installs/install_1.json",
            "retention_policy": "project_local_until_skill_uninstalled_or_superseded",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "install_id": "install_1",
            "candidate_id": "cand_1",
            "skill_id": "evolved.canonical-docs",
            "install_state": "installed",
            "install_path": ".codex/skills/evolved.canonical-docs",
            "manifest_path": ".codex/skills/evolved.canonical-docs/SKILL.md",
            "human_gate_approved": true,
            "support_refs": ["docs/a.md"],
            "created_at": "1"
        }),
        "skill install record",
    );

    let signal_schema = compiled_schema("agent_feedback_signal.schema.json");
    assert_schema_validates(
        &signal_schema,
        &json!({
            "schema_version": "feedback.agent_signal.v1",
            "conformance_line": "M18.feedback_signal",
            "canonical_path": ".pmcli/feedback/signals/signal_1.json",
            "retention_policy": "append_only_feedback_evidence",
            "atomic_write_policy": "append_log_plus_atomic_record_snapshot",
            "signal_id": "signal_1",
            "signal_kind": "skill_verification",
            "source_id": "ver_1",
            "target_lane": "skill_recommendation",
            "polarity": "positive",
            "weight": 1.0,
            "authority_boundary": "advisory_only",
            "evidence_refs": ["docs/a.md"],
            "created_at": "1"
        }),
        "agent feedback signal",
    );

    let calibration_schema = compiled_schema("feedback_calibration_record.schema.json");
    assert_schema_validates(
        &calibration_schema,
        &json!({
            "schema_version": "feedback.calibration_record.v1",
            "conformance_line": "M18.feedback_calibration",
            "canonical_path": ".pmcli/feedback/calibration/latest.json",
            "retention_policy": "project_local_until_next_calibration",
            "atomic_write_policy": "atomic_latest_projection_plus_append_log",
            "calibration_id": "cal_1",
            "authority_boundary": "advisory_only",
            "signal_count": 2,
            "positive_signal_count": 2,
            "negative_signal_count": 0,
            "routing_adjustments": [{
                "target_lane": "memory_retrieval",
                "adjustment_kind": "boost_positive_adoption_explainably",
                "weight_delta": 0.1,
                "reason": "memory adoption signals indicate useful retrieval",
                "evidence_refs": ["memory explain"]
            }],
            "skill_recommendation_hints": ["recommend verified evolved skills"],
            "verifier_calibration_hints": [],
            "governance_limits": ["feedback_cannot_publish_project_truth"],
            "created_at": "1"
        }),
        "feedback calibration record",
    );
}

#[test]
fn m11_skill_publication_schemas_lock_single_public_latest_contracts() {
    for schema_name in [
        "skill_output_artifact.schema.json",
        "skill_output_envelope.schema.json",
        "skill_output_list_result.schema.json",
        "skill_output_inspection_result.schema.json",
        "skill_output_submit_result.schema.json",
        "skill_run_result.schema.json",
        "skill_output_publish_result.schema.json",
        "skill_publication_gate.schema.json",
        "skill_output_context_projection.schema.json",
        "skill_docframe_publication.schema.json",
    ] {
        let contents = fs::read_to_string(schema_path(schema_name)).expect("schema should exist");
        let schema: Value = serde_json::from_str(&contents).expect("schema should parse");
        assert_eq!(
            schema["additionalProperties"],
            Value::Bool(false),
            "{schema_name} must reject unknown top-level fields"
        );
    }

    let envelope_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("skill_output_envelope.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        envelope_schema["properties"]["visibility"]["enum"],
        json!(["review_candidate", "canonical_public", "superseded"])
    );
    assert!(envelope_schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "canonicality_policy"));
    let gate_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("skill_publication_gate.schema.json")).unwrap(),
    )
    .unwrap();
    assert!(gate_schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "human_gate_approved"));

    let contract_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("research_skill_contract.schema.json")).unwrap(),
    )
    .unwrap();
    assert!(contract_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("allowed_output_kinds"));
    assert!(contract_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("doc_frame_required"));
    assert_eq!(
        contract_schema["properties"]["canonicality_policy"]["enum"],
        json!([
            "private_only",
            "candidate_only",
            "requires_review",
            "winner_only_public_surface"
        ])
    );
    assert!(contract_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("human_gate_required"));
    assert!(contract_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("allowed_write_scopes"));
    assert!(contract_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("runner_adapter"));

    let run_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("skill_run_result.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run_schema["properties"]["runner_adapter"]["enum"],
        json!(["local-command"])
    );
    assert!(run_schema["properties"]["side_effect_audit"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "mutated_paths"));
    let docframe_publication_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("skill_docframe_publication.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        docframe_publication_schema["properties"]["publication_state"]["enum"],
        json!(["candidate", "public_latest", "superseded"])
    );

    let manifest_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("skill_manifest.schema.json")).unwrap(),
    )
    .unwrap();
    assert!(manifest_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("contract_path"));
    assert!(manifest_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("runner_adapter"));
    assert!(manifest_schema["properties"]
        .as_object()
        .unwrap()
        .contains_key("output_kinds"));
}

#[test]
fn m10_research_runtime_schemas_lock_non_linear_deliberation_contracts() {
    for schema_name in [
        "research_thread.schema.json",
        "deliberation_span.schema.json",
        "research_stage_execution.schema.json",
        "stage_execution_map.schema.json",
        "research_status_report.schema.json",
        "research_thread_list_result.schema.json",
        "research_thread_inspection.schema.json",
        "research_stage_inspection.schema.json",
        "research_decision_record.schema.json",
        "research_record_result.schema.json",
        "research_turn_classification.schema.json",
        "change_envelope.schema.json",
        "debate_packet.schema.json",
        "research_debate_trace.schema.json",
        "research_skill_contract.schema.json",
        "skill_manifest.schema.json",
    ] {
        let contents = fs::read_to_string(schema_path(schema_name)).expect("schema should exist");
        let schema: Value = serde_json::from_str(&contents).expect("schema should parse");
        assert_eq!(
            schema["additionalProperties"],
            Value::Bool(false),
            "{schema_name} must reject unknown top-level fields"
        );
    }

    let stage_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("research_stage_execution.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        stage_schema["properties"]["operation"]["enum"],
        json!([
            "advance",
            "retry",
            "repair",
            "pivot",
            "fork",
            "supersede",
            "abandon",
            "human_override"
        ])
    );
    assert_eq!(stage_schema["properties"]["stage_id"]["minLength"], 1);
    assert!(stage_schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "change_envelope_ref"));
    assert_eq!(
        stage_schema["properties"]["status"]["enum"],
        json!([
            "active",
            "completed",
            "awaiting_gate",
            "blocked",
            "abandoned",
            "waiting_for_m9_promotion",
            "canonicality_refresh_pending"
        ])
    );

    let map_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("stage_execution_map.schema.json")).unwrap(),
    )
    .unwrap();
    let map_required = map_schema["properties"]["entries"]["items"]["required"]
        .as_array()
        .expect("stage map entries required array");
    for required in [
        "stage_id",
        "stage_class",
        "allowed_operations",
        "default_next_stage_id",
        "requires_change_envelope",
        "requires_human_gate",
    ] {
        assert!(
            map_required.iter().any(|value| value == required),
            "stage execution map must require {required}"
        );
    }

    let span_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("deliberation_span.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        span_schema["properties"]["mode"]["enum"],
        json!([
            "exploring",
            "comparing",
            "reviewing",
            "debugging",
            "interpreting_results",
            "drafting",
            "awaiting_human_gate",
            "ready_to_record",
            "ready_to_execute"
        ])
    );

    let classification_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("research_turn_classification.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        classification_schema["properties"]["candidate_operation"]["enum"],
        json!([
            "advance",
            "retry",
            "repair",
            "pivot",
            "fork",
            "supersede",
            "abandon",
            "human_override",
            null
        ])
    );
}

#[test]
fn m9_branch_schemas_lock_public_contracts() {
    for schema_name in [
        "search_batch_record.schema.json",
        "branch_run_record.schema.json",
        "branch_list_result.schema.json",
        "branch_inspection_result.schema.json",
        "branch_mutation_result.schema.json",
        "evaluation_packet.schema.json",
        "debate_trace.schema.json",
        "verifier_criterion.schema.json",
        "verifier_pair_score.schema.json",
        "verifier_tournament.schema.json",
        "promotion_decision.schema.json",
        "branch_archive_result.schema.json",
    ] {
        let contents = fs::read_to_string(schema_path(schema_name)).expect("schema should exist");
        let schema: Value = serde_json::from_str(&contents).expect("schema should parse");
        assert_eq!(
            schema["additionalProperties"],
            Value::Bool(false),
            "{schema_name} must reject unknown top-level fields"
        );
    }

    let search_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("search_batch_record.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        search_schema["properties"]["strategy"]["enum"],
        json!(["beam", "mcts", "evolutionary", "manual"])
    );
    assert_eq!(search_schema["properties"]["max_branches"]["minimum"], 1);

    let evaluation_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("evaluation_packet.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        evaluation_schema["properties"]["canonicality_gate"]["$ref"],
        "#/$defs/evaluation_outcome"
    );
    assert_eq!(evaluation_schema["properties"]["risk_score"]["minimum"], 0);
    assert_eq!(evaluation_schema["properties"]["risk_score"]["maximum"], 1);

    let mutation_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("branch_mutation_result.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        mutation_schema["properties"]["mutation_source"]["enum"],
        json!(["llm_command_adapter"])
    );

    let promotion_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("promotion_decision.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        promotion_schema["properties"]["verifier_gate_status"]["enum"],
        json!(["not_required", "passed"])
    );
    assert_eq!(
        promotion_schema["properties"]["merge_status"]["enum"],
        json!(["merged", "no_public_diff", "director_decision_only"])
    );

    let verifier_schema: Value = serde_json::from_str(
        &fs::read_to_string(schema_path("verifier_tournament.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        verifier_schema["properties"]["scope"]["enum"],
        json!(["branch_promotion", "research_decision", "claim_validation"])
    );
    assert_eq!(
        verifier_schema["properties"]["pair_scores"]["items"]["$ref"],
        "#/$defs/verifier_pair_score"
    );
    assert_eq!(
        verifier_schema["properties"]["advisory_until_gates_pass"]["const"],
        true
    );
    assert_schema_validates(
        &compiled_schema("verifier_tournament.schema.json"),
        &json!({
            "schema_version": "verifier_tournament.v1",
            "conformance_line": "M12.verifier_tournament",
            "tournament_id": "verifier_fixture",
            "scope": "branch_promotion",
            "batch_id": "batch_fixture",
            "candidate_refs": ["branch_a", "branch_b"],
            "criteria": [{
                "criterion_id": "deterministic_eval",
                "name": "Deterministic Eval",
                "description": "Prefer candidates with passing deterministic evaluation.",
                "evidence_requirements": ["evaluation_packet"],
                "ground_truth_note": "Tests and gates outrank narration."
            }],
            "repetitions": 1,
            "pair_scores": [{
                "schema_version": "verifier_pair_score.v1",
                "conformance_line": "M12.verifier_tournament",
                "pair_score_id": "score_fixture",
                "batch_id": "batch_fixture",
                "candidate_a": "branch_a",
                "candidate_b": "branch_b",
                "criterion_id": "deterministic_eval",
                "repetition_index": 0,
                "score_a": 0.9,
                "score_b": 0.1,
                "score_delta": 0.8,
                "winner_ref": "branch_a",
                "provider": "local",
                "model": "research-cli-deterministic-verifier-v1",
                "scoring_mode": "deterministic_local_evidence",
                "evidence_digest": "digest_fixture",
                "raw_trace_ref": "evaluation:eval_fixture"
            }],
            "winner_ref": "branch_a",
            "recommendation": "recommend_candidate_a",
            "deterministic_gate_result_ref": "eval_fixture",
            "canonicality_gate_result_ref": "eval_fixture",
            "merge_gate_result_ref": null,
            "advisory_until_gates_pass": true,
            "created_at": "1777299242"
        }),
        "verifier tournament fixture",
    );
}

#[test]
fn remote_status_and_pair_payloads_validate_against_remote_schemas() {
    let workspace_root = init_git_workspace("remote_schema_validation");
    let registry_root = registry_root_env("remote_schema_registry");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "status", "--json"])
        .output()
        .expect("remote status should execute");
    assert!(
        status_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status_json = output_json(&status_output);
    let remote_status_schema = compiled_schema("remote_status_report.schema.json");
    assert_schema_validates(
        &remote_status_schema,
        &status_json["data"],
        "unpaired remote status payload",
    );

    let pair_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "pair",
            "--client",
            "mobile-alpha",
            "--ticket",
            "pair_ticket_fixture",
            "--json",
        ])
        .output()
        .expect("remote pair should execute");
    assert!(
        pair_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pair_output.stderr)
    );
    let pair_json = output_json(&pair_output);
    assert_schema_validates(
        &remote_status_schema,
        &pair_json["data"],
        "paired remote status payload",
    );

    let machine_identity = serde_json::from_str::<Value>(
        &fs::read_to_string(Path::new(&registry_root).join("remote/machine_identity.json"))
            .expect("machine identity should persist"),
    )
    .expect("machine identity should parse");
    let client_identity = serde_json::from_str::<Value>(
        &fs::read_to_string(Path::new(&registry_root).join("remote/clients/mobile-alpha.json"))
            .expect("client identity should persist"),
    )
    .expect("client identity should parse");
    let pair_ticket = serde_json::from_str::<Value>(
        &fs::read_to_string(
            Path::new(&registry_root).join("remote/pair_tickets/pair_ticket_fixture.json"),
        )
        .expect("pair ticket should persist"),
    )
    .expect("pair ticket should parse");
    let machine_metadata = serde_json::from_str::<Value>(
        &fs::read_to_string(Path::new(&registry_root).join("remote/machine_metadata.json"))
            .expect("machine metadata should persist"),
    )
    .expect("machine metadata should parse");
    let daemon_state = serde_json::from_str::<Value>(
        &fs::read_to_string(Path::new(&registry_root).join("remote/daemon_state.json"))
            .expect("daemon state should persist"),
    )
    .expect("daemon state should parse");
    let binding = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(".pmcli/remote/binding.json"))
            .expect("binding should persist"),
    )
    .expect("binding should parse");
    let cursor = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(".pmcli/remote/cursor.json"))
            .expect("cursor should persist"),
    )
    .expect("cursor should parse");
    let projection = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(".pmcli/remote/projection.json"))
            .expect("projection should persist"),
    )
    .expect("projection should parse");
    let control_owner = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(".pmcli/remote/control_owner.json"))
            .expect("control owner should persist"),
    )
    .expect("control owner should parse");

    assert_schema_validates(
        &compiled_schema("machine_identity.schema.json"),
        &machine_identity,
        "machine identity record",
    );
    assert_schema_validates(
        &compiled_schema("remote_client_identity.schema.json"),
        &client_identity,
        "remote client identity record",
    );
    assert_schema_validates(
        &compiled_schema("pair_ticket.schema.json"),
        &pair_ticket,
        "pair ticket record",
    );
    assert_schema_validates(
        &compiled_schema("remote_machine_metadata.schema.json"),
        &machine_metadata,
        "remote machine metadata record",
    );
    assert_schema_validates(
        &compiled_schema("remote_daemon_state.schema.json"),
        &daemon_state,
        "remote daemon state record",
    );
    assert_schema_validates(
        &compiled_schema("remote_binding.schema.json"),
        &binding,
        "remote binding record",
    );
    assert_schema_validates(
        &compiled_schema("remote_cursor.schema.json"),
        &cursor,
        "remote cursor record",
    );
    assert_schema_validates(
        &compiled_schema("remote_projection_snapshot.schema.json"),
        &projection,
        "remote projection record",
    );
    assert_schema_validates(
        &compiled_schema("remote_control_owner.schema.json"),
        &control_owner,
        "remote control owner record",
    );
}

#[test]
fn remote_action_rejections_validate_against_schema() {
    let workspace_root = init_git_workspace("remote_rejection_schema_validation");
    let registry_root = registry_root_env("remote_rejection_schema_registry");

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
        .args(["remote", "attach", "--json"])
        .output()
        .expect("remote attach should execute");
    assert_eq!(output.status.code(), Some(12));
    let json = output_json(&output);
    assert_schema_validates(
        &compiled_schema("remote_action_rejection.schema.json"),
        &json["data"],
        "remote attach unpaired rejection",
    );
}

#[test]
fn host_surface_and_terminal_payloads_validate_against_m14_schemas() {
    let workspace_root = init_git_workspace("host_surface_schema_validation");
    let registry_root = registry_root_env("host_surface_schema_registry");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let pair_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "pair",
            "--client",
            "mobile-alpha",
            "--ticket",
            "pair-ticket-host-surface-schema",
            "--json",
        ])
        .status()
        .expect("remote pair should execute");
    assert!(pair_status.success());

    let host_surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        host_surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&host_surface_output.stderr)
    );
    let host_surface_json = output_json(&host_surface_output);
    assert_schema_validates(
        &compiled_schema("host_surface_projection.schema.json"),
        &host_surface_json["data"]["projection"],
        "host surface projection",
    );

    let tui_launch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "launch", "--json"])
        .output()
        .expect("tui launch should execute");
    assert!(tui_launch_output.status.success());
    let tui_launch_json = output_json(&tui_launch_output);
    assert_schema_validates(
        &compiled_schema("tui_launch_result.schema.json"),
        &tui_launch_json["data"],
        "tui launch result",
    );
    let fullscreen_tui_launch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "launch", "--fullscreen", "--json"])
        .output()
        .expect("fullscreen tui launch should execute");
    assert!(fullscreen_tui_launch_output.status.success());
    let fullscreen_tui_launch_json = output_json(&fullscreen_tui_launch_output);
    assert_schema_validates(
        &compiled_schema("tui_launch_result.schema.json"),
        &fullscreen_tui_launch_json["data"],
        "fullscreen tui launch result",
    );

    let action_schema = compiled_schema("host_surface_action.schema.json");
    for action in host_surface_json["data"]["projection"]["actions"]
        .as_array()
        .expect("actions array")
    {
        assert_schema_validates(&action_schema, action, "host surface action");
    }

    let tailscale_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_TAILSCALE_IP", "100.101.102.103")
        .args(["remote", "tailscale", "status", "--json"])
        .output()
        .expect("tailscale status should execute");
    assert!(tailscale_output.status.success());
    let tailscale_json = output_json(&tailscale_output);
    assert_schema_validates(
        &compiled_schema("tailscale_status_report.schema.json"),
        &tailscale_json["data"],
        "tailscale status",
    );

    let terminal_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "terminal", "attach", "--json"])
        .output()
        .expect("terminal attach should execute");
    assert!(terminal_output.status.success());
    let terminal_json = output_json(&terminal_output);
    assert_schema_validates(
        &compiled_schema("remote_terminal_projection.schema.json"),
        &terminal_json["data"]["terminal"],
        "remote terminal projection",
    );
}

#[test]
fn batch10_foundation_surface_payloads_validate_against_frozen_schemas() {
    let workspace_root = init_git_workspace("batch10_surface_payloads");
    let registry_root = registry_root_env("batch10_surface_registry");
    let blocked_registry_root = registry_root_env("batch10_blocked_registry");
    write_text_file(workspace_root.join("README.md"), "schema review target\n");
    write_skill_manifest(&workspace_root.join(".codex/skills"), "schema_skill");
    write_text_file(
        workspace_root
            .join(".codex/skills")
            .join("schema_skill")
            .join("research_skill_contract.json"),
        &json!({
            "schema_version": "research_skill_contract.v1",
            "conformance_line": "M11.skill_publication",
            "skill_id": "schema_skill",
            "compatible_stage_classes": ["paper"],
            "required_outputs": ["doc_frame", "report"],
            "allowed_output_kinds": ["report"],
            "doc_frame_required": true,
            "canonicality_policy": "requires_review",
            "human_gate_required": true,
            "runner_adapter": "local-command",
            "allowed_write_scopes": ["docs"]
        })
        .to_string(),
    );
    write_plugin_manifest(&workspace_root.join(".agents/plugins"), "schema_plugin");
    write_mcp_manifest(&workspace_root.join(".pmcli/mcp/servers"), "schema_mcp");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let init_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init should execute");
    assert!(init_output.status.success());
    let init_json = output_json(&init_output);

    let command_success_schema = compiled_schema("command_success.schema.json");
    assert_schema_validates(
        &command_success_schema,
        &init_json,
        "projects init envelope",
    );
    let project_status_schema = compiled_schema("project_status.schema.json");
    assert_schema_validates(
        &project_status_schema,
        &init_json["data"],
        "projects init project status",
    );

    let help_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("batch10_help_surface"))
        .args(["help", "--json"])
        .output()
        .expect("help should execute");
    assert!(help_output.status.success());
    let help_json = output_json(&help_output);
    assert_schema_validates(&command_success_schema, &help_json, "help envelope");
    let help_schema = compiled_schema("help_surface_report.schema.json");
    assert_schema_validates(&help_schema, &help_json["data"], "help surface report");

    let palette_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("batch10_palette_surface"))
        .args(["palette", "--json"])
        .output()
        .expect("palette should execute");
    assert!(palette_output.status.success());
    let palette_json = output_json(&palette_output);
    assert_schema_validates(&command_success_schema, &palette_json, "palette envelope");
    let palette_schema = compiled_schema("palette_surface_report.schema.json");
    assert_schema_validates(
        &palette_schema,
        &palette_json["data"],
        "palette surface report",
    );

    let current_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "current", "--json"])
        .output()
        .expect("projects current should execute");
    assert!(current_output.status.success());
    let current_json = output_json(&current_output);
    assert_schema_validates(
        &command_success_schema,
        &current_json,
        "projects current envelope",
    );
    let project_trace_schema = compiled_schema("project_resolution_trace.schema.json");
    assert_schema_validates(
        &project_trace_schema,
        &current_json["data"],
        "project resolution trace",
    );

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "list", "--json"])
        .output()
        .expect("projects list should execute");
    assert!(list_output.status.success());
    let list_json = output_json(&list_output);
    assert_schema_validates(
        &command_success_schema,
        &list_json,
        "projects list envelope",
    );
    let project_list_schema = compiled_schema("project_registry_list.schema.json");
    assert_schema_validates(
        &project_list_schema,
        &list_json["data"],
        "project registry list",
    );

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "status", "--json"])
        .output()
        .expect("projects status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_schema_validates(
        &command_success_schema,
        &status_json,
        "projects status envelope",
    );
    assert_schema_validates(
        &project_status_schema,
        &status_json["data"],
        "projects status project status",
    );

    let project_inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["inspect", "--project", "--json"])
        .output()
        .expect("inspect project should execute");
    assert!(project_inspect_output.status.success());
    let project_inspect_json = output_json(&project_inspect_output);
    assert_schema_validates(
        &command_success_schema,
        &project_inspect_json,
        "project inspection envelope",
    );
    let project_inspection_schema = compiled_schema("project_inspection.schema.json");
    assert_schema_validates(
        &project_inspection_schema,
        &project_inspect_json["data"],
        "project inspection result",
    );

    let audit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "audit", "--canonical", "--json"])
        .output()
        .expect("projects audit should execute");
    assert!(audit_output.status.success());
    let audit_json = output_json(&audit_output);
    assert_schema_validates(
        &command_success_schema,
        &audit_json,
        "projects audit envelope",
    );
    let audit_schema = compiled_schema("canonicality_audit_report.schema.json");
    assert_schema_validates(
        &audit_schema,
        &audit_json["data"],
        "canonicality audit report",
    );

    let memory_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "status", "--json"])
        .output()
        .expect("memory status should execute");
    assert!(memory_status_output.status.success());
    let memory_status_json = output_json(&memory_status_output);
    assert_schema_validates(
        &command_success_schema,
        &memory_status_json,
        "memory status envelope",
    );
    let memory_status_schema = compiled_schema("memory_status_report.schema.json");
    assert_schema_validates(
        &memory_status_schema,
        &memory_status_json["data"],
        "memory status result",
    );

    let research_record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "design",
            "--title",
            "schema-backed research runtime",
            "--stage",
            "refine",
            "--decision",
            "Track deliberation as runtime state.",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(research_record_output.status.success());
    let research_record_json = output_json(&research_record_output);
    assert_schema_validates(
        &command_success_schema,
        &research_record_json,
        "research record envelope",
    );
    let research_record_schema = compiled_schema("research_record_result.schema.json");
    assert_schema_validates(
        &research_record_schema,
        &research_record_json["data"],
        "research record result",
    );
    let thread_id = research_record_json["data"]["thread"]["thread_id"]
        .as_str()
        .expect("thread id");
    let stage_id = research_record_json["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage id");

    let research_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute");
    assert!(research_status_output.status.success());
    let research_status_json = output_json(&research_status_output);
    let research_status_schema = compiled_schema("research_status_report.schema.json");
    assert_schema_validates(
        &research_status_schema,
        &research_status_json["data"],
        "research status result",
    );

    let middleware_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "middleware",
            "plan",
            "--role",
            "planner",
            "--thread",
            thread_id,
            "--stage-execution",
            stage_id,
            "--intent",
            "Reflect on a failed result before approving a heavy benchmark rerun.",
            "--candidate-tool",
            "think_tool",
            "--candidate-tool",
            "mcp.search.paper",
            "--candidate-tool",
            "search_backup",
            "--candidate-tool",
            "shell",
            "--tool-error",
            "shell:timeout",
            "--json",
        ])
        .output()
        .expect("research middleware plan should execute");
    assert!(middleware_output.status.success());
    let middleware_json = output_json(&middleware_output);
    assert_schema_validates(
        &command_success_schema,
        &middleware_json,
        "research middleware envelope",
    );
    let middleware_schema = compiled_schema("research_ux_middleware_result.schema.json");
    assert_schema_validates(
        &middleware_schema,
        &middleware_json["data"],
        "research middleware result",
    );
    assert_schema_validates(
        &compiled_schema("research_role_template.schema.json"),
        &middleware_json["data"]["role_template"],
        "research middleware role template",
    );
    assert_schema_validates(
        &compiled_schema("tool_selection_trace.schema.json"),
        &middleware_json["data"]["tool_selection_trace"],
        "research middleware tool selection trace",
    );
    assert_schema_validates(
        &compiled_schema("stage_reflection_packet.schema.json"),
        &middleware_json["data"]["stage_reflection_packet"],
        "research middleware stage reflection packet",
    );
    assert_schema_validates(
        &compiled_schema("hitl_pending_gate.schema.json"),
        &middleware_json["data"]["hitl_pending_gate"],
        "research middleware hitl gate",
    );
    assert_schema_validates(
        &compiled_schema("tool_error_recovery.schema.json"),
        &middleware_json["data"]["tool_error_recovery"],
        "research middleware tool error recovery",
    );
    assert!(
        middleware_json["data"]["tool_selection_trace"]["selected_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "mcp.search.paper")
    );
    assert!(
        middleware_json["data"]["tool_selection_trace"]["filtered_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "search_backup")
    );

    let research_decide_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "decide",
            "--thread",
            thread_id,
            "--operation",
            "fork",
            "--decision",
            "approve",
            "--json",
        ])
        .output()
        .expect("research decide should execute");
    assert!(research_decide_output.status.success());
    let research_decide_json = output_json(&research_decide_output);
    let research_decision_schema = compiled_schema("research_decision_record.schema.json");
    assert_schema_validates(
        &research_decision_schema,
        &research_decide_json["data"],
        "research decision record",
    );
    let fork_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research fork status should execute");
    assert!(fork_status_output.status.success());
    let fork_status_json = output_json(&fork_status_output);
    assert_schema_validates(
        &compiled_schema("research_stage_execution.schema.json"),
        &fork_status_json["data"]["active_stage_execution"],
        "fork research stage execution",
    );
    let fork_artifacts = fork_status_json["data"]["active_stage_execution"]["output_artifacts"]
        .as_array()
        .expect("fork output artifacts");
    let debate_packet_ref = fork_artifacts
        .iter()
        .find_map(|value| {
            value
                .as_str()
                .filter(|path| path.contains("debate_packets"))
        })
        .expect("debate packet ref should exist");
    let debate_trace_ref = fork_artifacts
        .iter()
        .find_map(|value| value.as_str().filter(|path| path.contains("debate_traces")))
        .expect("debate trace ref should exist");
    let debate_packet = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(debate_packet_ref))
            .expect("debate packet should persist"),
    )
    .expect("debate packet should parse");
    assert_schema_validates(
        &compiled_schema("debate_packet.schema.json"),
        &debate_packet,
        "research debate packet",
    );
    let debate_trace = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(debate_trace_ref))
            .expect("debate trace should persist"),
    )
    .expect("debate trace should parse");
    assert_schema_validates(
        &compiled_schema("research_debate_trace.schema.json"),
        &debate_trace,
        "research debate trace",
    );

    let research_stage_map_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "stage", "map", "--json"])
        .output()
        .expect("research stage map should execute");
    assert!(research_stage_map_output.status.success());
    let research_stage_map_json = output_json(&research_stage_map_output);
    assert_schema_validates(
        &compiled_schema("stage_execution_map.schema.json"),
        &research_stage_map_json["data"],
        "research stage execution map",
    );

    let research_supersede_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "decide",
            "--thread",
            thread_id,
            "--operation",
            "supersede",
            "--decision",
            "approve",
            "--evidence-ref",
            "docs/deep_study/33-advanced-systems-implementation-plan.md",
            "--json",
        ])
        .output()
        .expect("research supersede should execute");
    assert!(research_supersede_output.status.success());
    let supersede_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research supersede status should execute");
    assert!(supersede_status_output.status.success());
    let supersede_status_json = output_json(&supersede_status_output);
    assert_schema_validates(
        &compiled_schema("research_stage_execution.schema.json"),
        &supersede_status_json["data"]["active_stage_execution"],
        "supersede research stage execution",
    );
    let change_envelope_ref = supersede_status_json["data"]["active_stage_execution"]
        ["change_envelope_ref"]
        .as_str()
        .expect("change envelope ref should exist");
    let change_envelope = serde_json::from_str::<Value>(
        &fs::read_to_string(workspace_root.join(change_envelope_ref))
            .expect("change envelope should persist"),
    )
    .expect("change envelope should parse");
    assert_schema_validates(
        &compiled_schema("change_envelope.schema.json"),
        &change_envelope,
        "research change envelope",
    );

    let research_classify_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "classify",
            "--thread",
            thread_id,
            "--text",
            "The result is not enough, maybe we need to fork competing repair plans.",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("research classify should execute");
    assert!(research_classify_output.status.success());
    let research_classify_json = output_json(&research_classify_output);
    let research_classify_schema = compiled_schema("research_turn_classification.schema.json");
    assert_schema_validates(
        &research_classify_schema,
        &research_classify_json["data"],
        "research turn classification",
    );

    let research_thread_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "thread", "inspect", thread_id, "--json"])
        .output()
        .expect("research thread inspect should execute");
    assert!(research_thread_output.status.success());
    let research_thread_json = output_json(&research_thread_output);
    let research_thread_schema = compiled_schema("research_thread_inspection.schema.json");
    assert_schema_validates(
        &research_thread_schema,
        &research_thread_json["data"],
        "research thread inspection",
    );

    let research_stage_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "stage", "inspect", stage_id, "--json"])
        .output()
        .expect("research stage inspect should execute");
    assert!(research_stage_output.status.success());
    let research_stage_json = output_json(&research_stage_output);
    let research_stage_schema = compiled_schema("research_stage_inspection.schema.json");
    assert_schema_validates(
        &research_stage_schema,
        &research_stage_json["data"],
        "research stage inspection",
    );

    let prune_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "prune", "--json"])
        .output()
        .expect("projects prune should execute");
    assert!(prune_output.status.success());
    let prune_json = output_json(&prune_output);
    assert_schema_validates(&command_success_schema, &prune_json, "prune envelope");
    let prune_schema = compiled_schema("prune_result.schema.json");
    assert_schema_validates(&prune_schema, &prune_json["data"], "prune result");

    let permission_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", "http://example.test/v1")
        .args(["prompt", "implement a small change", "--json"])
        .output()
        .expect("permission-producing prompt should execute");
    assert!(!permission_output.status.success());

    let pending_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "pending", "--json"])
        .output()
        .expect("permissions pending should execute");
    assert!(pending_output.status.success());
    let pending_json = output_json(&pending_output);
    assert_schema_validates(
        &command_success_schema,
        &pending_json,
        "permission pending envelope",
    );
    let pending_schema = compiled_schema("permission_pending_list.schema.json");
    assert_schema_validates(
        &pending_schema,
        &pending_json["data"],
        "permission pending list",
    );
    let request_id = pending_json["data"]["requests"][0]["request_id"]
        .as_str()
        .expect("pending request id should be present")
        .to_string();

    let approve_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "approve", &request_id, "--json"])
        .output()
        .expect("permissions approve should execute");
    assert!(approve_output.status.success());

    let history_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "history", "--json"])
        .output()
        .expect("permissions history should execute");
    assert!(history_output.status.success());
    let history_json = output_json(&history_output);
    assert_schema_validates(
        &command_success_schema,
        &history_json,
        "permission history envelope",
    );
    let history_schema = compiled_schema("permission_history_result.schema.json");
    assert_schema_validates(
        &history_schema,
        &history_json["data"],
        "permission history result",
    );
    assert_eq!(history_json["data"]["total_count"], 1);
    assert_eq!(history_json["data"]["decisions"][0]["decision"], "approved");

    let conformance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFORMANCE_SELFTEST", "1")
        .args(["conformance", "--json"])
        .output()
        .expect("conformance should execute");
    let conformance_json = output_json(&conformance_output);
    assert_schema_validates(
        &command_success_schema,
        &conformance_json,
        "conformance envelope",
    );
    let conformance_schema = compiled_schema("conformance_result.schema.json");
    let conformance_payload = conformance_json
        .get("data")
        .unwrap_or_else(|| panic!("conformance output should include data: {conformance_json}"));
    assert_schema_validates(
        &conformance_schema,
        conformance_payload,
        "conformance result",
    );

    let setup_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["setup", "status", "--json"])
        .output()
        .expect("setup status should execute");
    assert!(setup_output.status.success());
    let setup_json = output_json(&setup_output);
    assert_schema_validates(
        &command_success_schema,
        &setup_json,
        "setup status envelope",
    );
    let setup_schema = compiled_schema("setup_status_report.schema.json");
    assert_schema_validates(&setup_schema, &setup_json["data"], "setup status report");

    let routes_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["setup", "install-routes", "--json"])
        .output()
        .expect("setup install-routes should execute");
    assert!(routes_output.status.success());
    let routes_json = output_json(&routes_output);
    assert_schema_validates(
        &command_success_schema,
        &routes_json,
        "setup install-routes envelope",
    );
    let routes_schema = compiled_schema("install_routes_report.schema.json");
    assert_schema_validates(
        &routes_schema,
        &routes_json["data"],
        "setup install-routes report",
    );

    let skills_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "list", "--json"])
        .output()
        .expect("skills list should execute");
    assert!(skills_output.status.success());
    let skills_json = output_json(&skills_output);
    assert_schema_validates(
        &command_success_schema,
        &skills_json,
        "skills list envelope",
    );
    let skills_schema = compiled_schema("skill_list_result.schema.json");
    assert_schema_validates(&skills_schema, &skills_json["data"], "skills list result");
    assert_schema_validates(
        &compiled_schema("skill_registry_entry.schema.json"),
        &skills_json["data"]["skills"][0],
        "skill registry entry",
    );

    let skill_id = skills_json["data"]["skills"][0]["skill_id"]
        .as_str()
        .expect("skill id should exist");

    let skills_inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "inspect", skill_id, "--json"])
        .output()
        .expect("skills inspect should execute");
    assert!(skills_inspect_output.status.success());
    let skills_inspect_json = output_json(&skills_inspect_output);
    assert_schema_validates(
        &command_success_schema,
        &skills_inspect_json,
        "skills inspect envelope",
    );
    assert_schema_validates(
        &compiled_schema("skill_inspect_result.schema.json"),
        &skills_inspect_json["data"],
        "skills inspect result",
    );
    assert_schema_validates(
        &compiled_schema("skill_availability_record.schema.json"),
        &skills_inspect_json["data"]["availability"],
        "skill availability record",
    );

    let skills_paths_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "paths", "--json"])
        .output()
        .expect("skills paths should execute");
    assert!(skills_paths_output.status.success());
    let skills_paths_json = output_json(&skills_paths_output);
    assert_schema_validates(
        &compiled_schema("skill_paths_result.schema.json"),
        &skills_paths_json["data"],
        "skills paths result",
    );

    let skills_validate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "validate", skill_id, "--json"])
        .output()
        .expect("skills validate should execute");
    assert!(skills_validate_output.status.success());
    let skills_validate_json = output_json(&skills_validate_output);
    assert_schema_validates(
        &compiled_schema("skill_validation_result.schema.json"),
        &skills_validate_json["data"],
        "skills validate result",
    );
    assert_schema_validates(
        &compiled_schema("skill_availability_record.schema.json"),
        &skills_validate_json["data"]["availability_records"][0],
        "skill validation availability record",
    );

    write_text_file(
        workspace_root.join("docs").join("skill-candidate.md"),
        r#"---
doc_frame:
  doc_id: docs.schema_skill_candidate
  schema_version: "1"
  source_path: docs/skill-candidate.md
  title: Schema Skill Candidate
  doc_type: report
  lifecycle: active
  scope: project
  summary: Candidate generated by schema skill.
  generated_by: skill
  updated_at: 2026-04-27
---

# Schema Skill Candidate
"#,
    );
    let skill_outputs_list = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "outputs", "list", "--json"])
        .output()
        .expect("skills outputs list should execute");
    assert!(skill_outputs_list.status.success());
    let skill_outputs_list_json = output_json(&skill_outputs_list);
    assert_schema_validates(
        &compiled_schema("skill_output_list_result.schema.json"),
        &skill_outputs_list_json["data"],
        "empty skill output list result",
    );

    let skill_output_submit = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "submit",
            "--skill",
            skill_id,
            "--kind",
            "report",
            "--artifact",
            "docs/skill-candidate.md",
            "--family",
            "skill_reports",
            "--doc-frame",
            "docs/skill-candidate.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("skills output submit should execute");
    assert!(skill_output_submit.status.success());
    let skill_output_submit_json = output_json(&skill_output_submit);
    assert_schema_validates(
        &compiled_schema("skill_output_submit_result.schema.json"),
        &skill_output_submit_json["data"],
        "skill output submit result",
    );
    assert_schema_validates(
        &compiled_schema("skill_output_envelope.schema.json"),
        &skill_output_submit_json["data"]["envelope"],
        "skill output envelope",
    );

    let skill_run_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            skill_id,
            "--adapter",
            "local-command",
            "--command",
            "mkdir -p docs && cp docs/skill-candidate.md docs/skill-run-candidate.md",
            "--kind",
            "report",
            "--artifact",
            "docs/skill-run-candidate.md",
            "--family",
            "skill_run_reports",
            "--doc-frame",
            "docs/skill-run-candidate.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("skills run should execute");
    assert!(skill_run_output.status.success());
    let skill_run_json = output_json(&skill_run_output);
    assert_schema_validates(
        &compiled_schema("skill_run_result.schema.json"),
        &skill_run_json["data"],
        "skill run result",
    );

    let skill_output_envelope_id = skill_output_submit_json["data"]["envelope"]["envelope_id"]
        .as_str()
        .expect("skill output envelope id");

    let skill_output_inspect = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "inspect",
            skill_output_envelope_id,
            "--json",
        ])
        .output()
        .expect("skills output inspect should execute");
    assert!(skill_output_inspect.status.success());
    let skill_output_inspect_json = output_json(&skill_output_inspect);
    assert_schema_validates(
        &compiled_schema("skill_output_inspection_result.schema.json"),
        &skill_output_inspect_json["data"],
        "skill output inspection result",
    );

    let skill_output_publish = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "publish",
            "--execute",
            skill_output_envelope_id,
            "--approve-human-gate",
            "--json",
        ])
        .output()
        .expect("skills publish should execute");
    assert!(skill_output_publish.status.success());
    let skill_output_publish_json = output_json(&skill_output_publish);
    assert_schema_validates(
        &compiled_schema("skill_output_publish_result.schema.json"),
        &skill_output_publish_json["data"],
        "skill output publish result",
    );
    assert_schema_validates(
        &compiled_schema("skill_publication_gate.schema.json"),
        &skill_output_publish_json["data"]["publication_gate"],
        "skill publication gate",
    );

    let plugins_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["plugins", "list", "--json"])
        .output()
        .expect("plugins list should execute");
    assert!(plugins_output.status.success());
    let plugins_json = output_json(&plugins_output);
    assert_schema_validates(
        &command_success_schema,
        &plugins_json,
        "plugins list envelope",
    );
    let plugins_schema = compiled_schema("plugin_list_result.schema.json");
    assert_schema_validates(
        &plugins_schema,
        &plugins_json["data"],
        "plugins list result",
    );

    let hooks_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["hooks", "list", "--json"])
        .output()
        .expect("hooks list should execute");
    assert!(hooks_output.status.success());
    let hooks_json = output_json(&hooks_output);
    assert_schema_validates(&command_success_schema, &hooks_json, "hooks list envelope");
    let hooks_schema = compiled_schema("hook_list_result.schema.json");
    assert_schema_validates(&hooks_schema, &hooks_json["data"], "hooks list result");

    let mcp_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["mcp", "list", "--json"])
        .output()
        .expect("mcp list should execute");
    assert!(mcp_output.status.success());
    let mcp_json = output_json(&mcp_output);
    assert_schema_validates(&command_success_schema, &mcp_json, "mcp list envelope");
    let mcp_schema = compiled_schema("mcp_list_result.schema.json");
    assert_schema_validates(&mcp_schema, &mcp_json["data"], "mcp list result");

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "schema fixture", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", "latest", "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    write_text_file(workspace_root.join(".pmcli/archive/schema.tmp"), "stale");

    let artifacts_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "list", "--include-archive", "--json"])
        .output()
        .expect("artifacts list should execute");
    assert!(artifacts_output.status.success());
    let artifacts_json = output_json(&artifacts_output);
    assert_schema_validates(
        &command_success_schema,
        &artifacts_json,
        "artifacts list envelope",
    );
    let artifacts_schema = compiled_schema("artifact_list_result.schema.json");
    assert_schema_validates(
        &artifacts_schema,
        &artifacts_json["data"],
        "artifacts list result",
    );

    let cleanup_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--json"])
        .output()
        .expect("repo cleanup-plan should execute");
    assert!(cleanup_output.status.success());
    let cleanup_json = output_json(&cleanup_output);
    assert_schema_validates(
        &command_success_schema,
        &cleanup_json,
        "cleanup plan envelope",
    );
    let cleanup_schema = compiled_schema("repo_cleanup_proposal.schema.json");
    assert_schema_validates(
        &cleanup_schema,
        &cleanup_json["data"],
        "cleanup proposal result",
    );

    let review_open_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Schema fixture review",
            "--target",
            "README.md",
            "--reviewer-role",
            "schema_auditor",
            "--review-model",
            "gpt-5.4",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(review_open_output.status.success());
    let review_open_json = output_json(&review_open_output);
    assert_schema_validates(
        &command_success_schema,
        &review_open_json,
        "review open envelope",
    );
    let review_open_schema = compiled_schema("review_open_result.schema.json");
    assert_schema_validates(
        &review_open_schema,
        &review_open_json["data"],
        "review open result",
    );
    let review_packet_schema = compiled_schema("review_packet.schema.json");
    assert_schema_validates(
        &review_packet_schema,
        &review_open_json["data"]["packet"],
        "review packet",
    );
    let review_trace_schema = compiled_schema("review_trace.schema.json");
    assert_schema_validates(
        &review_trace_schema,
        &review_open_json["data"]["trace"],
        "review trace",
    );
    let review_id = review_open_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("review id should be present")
        .to_string();

    let review_list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "list", "--json"])
        .output()
        .expect("reviews list should execute");
    assert!(review_list_output.status.success());
    let review_list_json = output_json(&review_list_output);
    assert_schema_validates(
        &command_success_schema,
        &review_list_json,
        "reviews list envelope",
    );
    let review_list_schema = compiled_schema("review_list_result.schema.json");
    assert_schema_validates(
        &review_list_schema,
        &review_list_json["data"],
        "reviews list result",
    );

    let review_inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "inspect", review_id.as_str(), "--json"])
        .output()
        .expect("reviews inspect should execute");
    assert!(review_inspect_output.status.success());
    let review_inspect_json = output_json(&review_inspect_output);
    assert_schema_validates(
        &command_success_schema,
        &review_inspect_json,
        "reviews inspect envelope",
    );
    let review_inspection_schema = compiled_schema("review_inspection_result.schema.json");
    assert_schema_validates(
        &review_inspection_schema,
        &review_inspect_json["data"],
        "reviews inspection result",
    );

    write_text_file(
        workspace_root.join("docs").join("schema-doc.md"),
        r#"---
doc_frame:
  id: docs.schema_doc
  title: Schema Doc
  doc_type: plan
  lifecycle: active
  scope: project
  summary: Schema-backed DocFrame fixture.
  evidence_refs:
    - docs/schema-doc.md
  generated_by: test
  updated_at: 2026-04-25
---

# Schema Doc
"#,
    );
    let docs_index_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["docs", "index", "--json"])
        .output()
        .expect("docs index should execute");
    assert!(docs_index_output.status.success());
    let docs_index_json = output_json(&docs_index_output);
    assert_schema_validates(
        &command_success_schema,
        &docs_index_json,
        "docs index envelope",
    );
    let doc_index_schema = compiled_schema("doc_index.schema.json");
    assert_schema_validates(
        &doc_index_schema,
        &docs_index_json["data"],
        "doc index result",
    );
    let doc_frame_schema = compiled_schema("doc_frame.schema.json");
    assert_schema_validates(
        &doc_frame_schema,
        &docs_index_json["data"]["doc_frames"][0],
        "doc frame result",
    );

    let goals_set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep project-level implementation aligned with the reference-superiority plan.",
            "--milestone-goal",
            "Graduate MissionFrame schema and context injection.",
            "--current-implementation-goal",
            "Expose MissionFrame status and projections through CLI payloads.",
            "--success-criterion",
            "MissionFrame validates against schema and appears in prompt/resume/compact payloads.",
            "--evidence-ref",
            "docs/deep_study/48-mission-frame-context-and-compact-policy.md",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(goals_set_output.status.success());
    let goals_set_json = output_json(&goals_set_output);
    assert_schema_validates(
        &command_success_schema,
        &goals_set_json,
        "goals set envelope",
    );
    let mission_frame_schema = compiled_schema("mission_frame.schema.json");
    assert_schema_validates(
        &mission_frame_schema,
        &goals_set_json["data"]["mission_frame"],
        "mission frame",
    );
    let mission_frame_status_schema = compiled_schema("mission_frame_status.schema.json");
    assert_schema_validates(
        &mission_frame_status_schema,
        &goals_set_json["data"],
        "mission frame status",
    );

    let goals_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "status", "--json"])
        .output()
        .expect("goals status should execute");
    assert!(goals_status_output.status.success());
    let goals_status_json = output_json(&goals_status_output);
    assert_schema_validates(
        &command_success_schema,
        &goals_status_json,
        "goals status envelope",
    );
    assert_schema_validates(
        &mission_frame_status_schema,
        &goals_status_json["data"],
        "goals status result",
    );

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", "latest", "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let compact_json = output_json(&compact_output);
    assert_schema_validates(&command_success_schema, &compact_json, "compact envelope");
    let compact_schema = compiled_schema("compact_result.schema.json");
    assert_schema_validates(&compact_schema, &compact_json["data"], "compact result");
    let command_failure_schema = compiled_schema("command_failure.schema.json");

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(&events_path);
    let goal_alignment_trace = events
        .iter()
        .rev()
        .find(|event| event["event_name"] == "goal_alignment_trace")
        .expect("goal alignment trace event should exist");
    let goal_alignment_schema = compiled_schema("goal_alignment_trace.schema.json");
    assert_schema_validates(
        &goal_alignment_schema,
        &goal_alignment_trace["payload"],
        "goal alignment trace payload",
    );

    let doctor_blocked_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("batch10_doctor_blocked"))
        .env("RESEARCH_CLI_STATE_HOME", &blocked_registry_root)
        .args(["doctor", "--json"])
        .output()
        .expect("doctor blocked should execute");
    assert!(!doctor_blocked_output.status.success());
    let doctor_blocked_json = output_json(&doctor_blocked_output);
    assert_schema_validates(
        &command_failure_schema,
        &doctor_blocked_json,
        "doctor blocked envelope",
    );
    let doctor_schema = compiled_schema("doctor_report.schema.json");
    assert_schema_validates(
        &doctor_schema,
        &doctor_blocked_json["data"],
        "doctor blocked report",
    );
    let preflight_schema = compiled_schema("runtime_preflight_report.schema.json");
    assert_schema_validates(
        &preflight_schema,
        &doctor_blocked_json["data"]["preflight"],
        "doctor blocked preflight",
    );

    let doctor_ready_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["doctor", "--json"])
        .output()
        .expect("doctor ready should execute");
    assert!(doctor_ready_output.status.success());
    let doctor_ready_json = output_json(&doctor_ready_output);
    assert_schema_validates(
        &command_success_schema,
        &doctor_ready_json,
        "doctor ready envelope",
    );
    assert_schema_validates(
        &doctor_schema,
        &doctor_ready_json["data"],
        "doctor ready report",
    );
    assert_schema_validates(
        &preflight_schema,
        &doctor_ready_json["data"]["preflight"],
        "doctor ready preflight",
    );

    let smoke_blocked_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("batch10_smoke_blocked"))
        .env("RESEARCH_CLI_STATE_HOME", &blocked_registry_root)
        .args(["smoke", "--json"])
        .output()
        .expect("smoke blocked should execute");
    assert!(!smoke_blocked_output.status.success());
    let smoke_blocked_json = output_json(&smoke_blocked_output);
    assert_schema_validates(
        &command_failure_schema,
        &smoke_blocked_json,
        "smoke blocked envelope",
    );
    let smoke_schema = compiled_schema("smoke_result.schema.json");
    assert_schema_validates(
        &smoke_schema,
        &smoke_blocked_json["data"],
        "smoke blocked result",
    );

    let smoke_ready_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["smoke", "--json"])
        .output()
        .expect("smoke ready should execute");
    assert!(smoke_ready_output.status.success());
    let smoke_ready_json = output_json(&smoke_ready_output);
    assert_schema_validates(
        &command_success_schema,
        &smoke_ready_json,
        "smoke ready envelope",
    );
    assert_schema_validates(
        &smoke_schema,
        &smoke_ready_json["data"],
        "smoke ready result",
    );
}

#[test]
fn event_schema_rejects_legacy_event_type_and_requires_terminal_outcome() {
    let schema = compiled_schema("event.schema.json");

    let valid_terminal_event = json!({
        "event_id": "evt_turn_1",
        "seq": 1,
        "event_name": "turn",
        "phase": "terminal",
        "terminal_outcome": "succeeded",
        "object_kind": "turn",
        "object_id": "turn_1",
        "session_id": "sess_1",
        "project_id": "proj_1",
        "timestamp": "1777000000000",
        "payload": {}
    });
    assert_schema_validates(&schema, &valid_terminal_event, "valid terminal event");

    let legacy_event_type = json!({
        "event_id": "evt_turn_1",
        "seq": 1,
        "event_type": "turn_completed",
        "event_name": "turn",
        "phase": "terminal",
        "terminal_outcome": "succeeded",
        "object_kind": "turn",
        "object_id": "turn_1",
        "timestamp": "1777000000000",
        "payload": {}
    });
    assert!(
        schema.validate(&legacy_event_type).is_err(),
        "legacy event_type must be rejected"
    );

    let missing_outcome = json!({
        "event_id": "evt_turn_1",
        "seq": 1,
        "event_name": "turn",
        "phase": "terminal",
        "object_kind": "turn",
        "object_id": "turn_1",
        "timestamp": "1777000000000",
        "payload": {}
    });
    assert!(
        schema.validate(&missing_outcome).is_err(),
        "terminal events must require terminal_outcome"
    );
}

#[test]
fn batch8_operator_payloads_validate_against_frozen_schemas() {
    let workspace_root = init_git_workspace("batch8_schema_payloads");
    let registry_root = registry_root_env("batch8_schema_registry");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let provider_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", "http://example.test/v1")
        .env("RESEARCH_CLI_MODEL", "gpt-4.1-mini")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(provider_output.status.success());
    let provider_json = output_json(&provider_output);
    let provider_schema = compiled_schema("provider_resolution_trace.schema.json");
    assert_schema_validates(
        &provider_schema,
        &provider_json["data"]["provider_trace"],
        "provider trace",
    );

    let usage_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["usage", "--json"])
        .output()
        .expect("usage should execute");
    assert!(usage_output.status.success());
    let usage_json = output_json(&usage_output);
    let usage_schema = compiled_schema("usage_summary.schema.json");
    assert_schema_validates(&usage_schema, &usage_json["data"], "usage summary");

    let stats_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["stats", "--json"])
        .output()
        .expect("stats should execute");
    assert!(stats_output.status.success());
    let stats_json = output_json(&stats_output);
    let stats_schema = compiled_schema("stats_summary.schema.json");
    assert_schema_validates(&stats_schema, &stats_json["data"], "stats summary");

    let catalog_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["providers", "refresh-catalog", "--json"])
        .output()
        .expect("providers refresh-catalog should execute");
    assert!(catalog_output.status.success());
    let catalog_json = output_json(&catalog_output);
    let catalog_schema = compiled_schema("provider_catalog_refresh_result.schema.json");
    assert_schema_validates(
        &catalog_schema,
        &catalog_json["data"],
        "provider catalog refresh result",
    );
}

#[test]
fn intentionally_broken_provider_trace_fixture_fails_schema_validation() {
    let schema = compiled_schema("provider_resolution_trace.schema.json");
    let invalid_fixture =
        fs::read_to_string(fixture_path("provider_resolution_trace.invalid.json"))
            .expect("invalid fixture should be readable");
    let invalid_instance: Value =
        serde_json::from_str(&invalid_fixture).expect("invalid fixture should parse");

    assert!(
        schema.validate(&invalid_instance).is_err(),
        "broken provider trace fixture must fail schema validation"
    );
}
