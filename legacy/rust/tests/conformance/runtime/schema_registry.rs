use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn schema_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(name)
}

#[test]
fn batch8_schema_files_exist_and_parse() {
    for schema in [
        "event.schema.json",
        "provider_resolution_trace.schema.json",
        "usage_summary.schema.json",
        "stats_summary.schema.json",
        "setup_status_report.schema.json",
        "migrate_check_report.schema.json",
        "repair_hints_report.schema.json",
        "install_routes_report.schema.json",
        "skill_list_result.schema.json",
        "skill_inspect_result.schema.json",
        "skill_paths_result.schema.json",
        "skill_validation_result.schema.json",
        "plugin_list_result.schema.json",
        "plugin_inspection_result.schema.json",
        "plugin_validation_result.schema.json",
        "hook_list_result.schema.json",
        "hook_inspection_result.schema.json",
        "hook_test_result.schema.json",
        "mcp_list_result.schema.json",
        "mcp_inspection_result.schema.json",
        "mcp_test_result.schema.json",
        "mcp_refresh_result.schema.json",
        "artifact_family.schema.json",
        "artifact_list_result.schema.json",
        "artifact_inspection_result.schema.json",
        "artifact_promotion_candidate.schema.json",
        "artifact_promotion_queue_result.schema.json",
        "artifact_promotion_result.schema.json",
        "repo_cleanup_proposal.schema.json",
        "canonical_lineage_manifest.schema.json",
        "review_packet.schema.json",
        "review_trace.schema.json",
        "review_list_result.schema.json",
        "review_inspection_result.schema.json",
        "review_open_result.schema.json",
        "working_memory_record.schema.json",
        "memory_record.schema.json",
        "memory_query_result.schema.json",
        "memory_explain_record.schema.json",
        "memory_status_report.schema.json",
        "progress_digest_candidate.schema.json",
        "projectops_tick.schema.json",
        "doc_context_projection.schema.json",
        "experiment_supervisor_lease.schema.json",
        "wake_event.schema.json",
        "projectops_status_report.schema.json",
        "host_surface_projection.schema.json",
        "host_surface_action.schema.json",
        "tui_launch_result.schema.json",
        "tailscale_status_report.schema.json",
        "remote_terminal_projection.schema.json",
    ] {
        let contents = fs::read_to_string(schema_path(schema)).expect("schema should exist");
        let json: Value = serde_json::from_str(&contents).expect("schema should parse as json");
        assert!(
            json["title"].is_string(),
            "schema title missing for {schema}"
        );
        assert_eq!(json["type"], "object", "schema type mismatch for {schema}");
    }
}
