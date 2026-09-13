use crate::support::{init_git_workspace, unique_temp_dir};
use jsonschema::JSONSchema;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn schema_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(name)
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

fn output_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid json")
}

fn write_session_jsonl(workspace_root: &std::path::Path, name: &str, lines: &[Value]) -> PathBuf {
    let path = workspace_root.join(name);
    let mut file = fs::File::create(&path).expect("session fixture should create");
    for line in lines {
        writeln!(file, "{line}").expect("session line should write");
    }
    path
}

fn write_promotion_candidate(
    workspace_root: &std::path::Path,
    candidate_id: &str,
    session_id: &str,
    summary_ref: &str,
    source_artifact_path: &std::path::Path,
    support_refs: &[&str],
) {
    let queue_dir = workspace_root
        .join(".pmcli")
        .join("memory")
        .join("promotion_queue");
    fs::create_dir_all(&queue_dir).expect("promotion queue should exist");
    let candidate_path = queue_dir.join(format!("{candidate_id}.json"));
    fs::write(
        candidate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "projectops.progress_digest_candidate.v1",
            "conformance_line": "M7.digest_candidate",
            "canonical_path": "manual_candidate.json",
            "retention_policy": "test_candidate",
            "atomic_write_policy": "atomic_json_write",
            "candidate_id": candidate_id,
            "tick_id": format!("tick_{candidate_id}"),
            "project_id": "test_project",
            "session_id": session_id,
            "summary_ref": summary_ref,
            "source_artifact_path": source_artifact_path.display().to_string(),
            "status": "pending_review",
            "support_refs": support_refs,
            "confidence": "test",
            "degraded_reasons": [],
            "promotes_to_durable_memory": false,
            "created_at": "1"
        }))
        .expect("candidate should serialize"),
    )
    .expect("candidate should write");
}

fn init_test_project(label: &str) -> (PathBuf, String) {
    let workspace_root = init_git_workspace(label);
    let registry_root = unique_temp_dir(&format!("{label}_registry"))
        .display()
        .to_string();
    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());
    (workspace_root, registry_root)
}

fn run_json_command(workspace_root: &std::path::Path, registry_root: &str, args: &[&str]) -> Value {
    let output = Command::new(cargo_bin())
        .current_dir(workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", registry_root)
        .args(args)
        .output()
        .expect("command should execute");
    assert!(output.status.success(), "{output:?}");
    output_json(&output)
}

fn run_json_command_expect_failure(
    workspace_root: &std::path::Path,
    registry_root: &str,
    args: &[&str],
) -> Value {
    let output = Command::new(cargo_bin())
        .current_dir(workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", registry_root)
        .args(args)
        .output()
        .expect("command should execute");
    assert!(!output.status.success(), "{output:?}");
    output_json(&output)
}

fn evolution_candidate_path(workspace_root: &Path, candidate_id: &str) -> PathBuf {
    workspace_root
        .join(".pmcli")
        .join("skills")
        .join("evolution")
        .join("candidates")
        .join(format!("{candidate_id}.json"))
}

fn rewrite_evolution_candidate(
    workspace_root: &Path,
    candidate_id: &str,
    mut rewrite: impl FnMut(&mut Value),
) {
    let path = evolution_candidate_path(workspace_root, candidate_id);
    let mut candidate: Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("candidate should exist"))
            .expect("candidate should parse");
    rewrite(&mut candidate);
    fs::write(
        &path,
        serde_json::to_string_pretty(&candidate).expect("candidate should serialize"),
    )
    .expect("candidate should rewrite");
}

fn create_m16_skill_install_candidate(
    workspace_root: &Path,
    registry_root: &str,
    label: &str,
) -> (String, String) {
    let artifact_path = workspace_root
        .join("docs")
        .join(format!("{label}-evidence.md"));
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        format!("Install evidence for {label}: verified skills stay governed.\n"),
    )
    .expect("artifact should write");

    for (suffix, summary) in [
        (
            "a",
            "local skill install requires verified candidate approval",
        ),
        ("b", "local skill install writes normal skill manifest path"),
    ] {
        write_promotion_candidate(
            workspace_root,
            &format!("{label}_{suffix}"),
            label,
            summary,
            &artifact_path,
            &[&format!("docs/{label}-evidence.md"), "segment:m16-install"],
        );
        let promoted = run_json_command(
            workspace_root,
            registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                &format!("{label}_{suffix}"),
                "--trust",
                "trusted",
                "--json",
            ],
        );
        assert_eq!(promoted["data"]["record"]["injectable"], true);
    }

    let clusters = run_json_command(
        workspace_root,
        registry_root,
        &[
            "skills",
            "evolve",
            "cluster",
            "--min-members",
            "2",
            "--json",
        ],
    );
    let cluster_id = clusters["data"]["clusters"][0]["cluster_id"]
        .as_str()
        .expect("cluster id")
        .to_string();
    let crystallized = run_json_command(
        workspace_root,
        registry_root,
        &["skills", "evolve", "crystallize", &cluster_id, "--json"],
    );
    let candidate_id = crystallized["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();
    let skill_id = crystallized["data"]["candidate"]["skill_id"]
        .as_str()
        .expect("skill id")
        .to_string();
    (candidate_id, skill_id)
}

#[test]
fn m15_reingest_supersedes_changed_segments_and_invalidates_old_memory() {
    let (workspace_root, registry_root) = init_test_project("m15_reingest_supersession");
    let artifact_path = workspace_root.join("docs").join("m15-reingest.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact has parent"))
        .expect("artifact parent should create");
    fs::write(&artifact_path, "M15 reingest artifact\n").expect("artifact should write");

    let _session_path = write_session_jsonl(
        &workspace_root,
        "m15-reingest.jsonl",
        &[
            serde_json::json!({"role": "user", "content": "# task: stable task"}),
            serde_json::json!({"role": "assistant", "content": "Kept docs/m15-reingest.md unchanged"}),
            serde_json::json!({"role": "user", "content": "# task: evolving task"}),
            serde_json::json!({"role": "assistant", "content": "Initial result in docs/m15-reingest.md"}),
        ],
    );

    let ingest_json = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "ingest",
            "m15-reingest.jsonl",
            "--source-kind",
            "cli_session",
            "--json",
        ],
    );
    let ingest_id = ingest_json["data"]["record"]["ingest_id"]
        .as_str()
        .expect("ingest id should exist")
        .to_string();
    let first_segment_json = run_json_command(
        &workspace_root,
        &registry_root,
        &["memory", "trajectory", "segment", &ingest_id, "--json"],
    );
    let stable_segment_id = first_segment_json["data"]["segments"][0]["segment_id"]
        .as_str()
        .expect("stable segment id should exist")
        .to_string();
    let old_evolving_segment_id = first_segment_json["data"]["segments"][1]["segment_id"]
        .as_str()
        .expect("evolving segment id should exist")
        .to_string();
    run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "label",
            &old_evolving_segment_id,
            "--json",
        ],
    );
    let old_extract_json = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "extract",
            &old_evolving_segment_id,
            "--json",
        ],
    );
    let old_candidate_id = old_extract_json["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id should exist")
        .to_string();
    let old_promote_json = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "promote",
            "--candidate-id",
            &old_candidate_id,
            "--trust",
            "trusted",
            "--json",
        ],
    );
    let old_memory_id = old_promote_json["data"]["record"]["record_id"]
        .as_str()
        .expect("memory id should exist")
        .to_string();

    write_session_jsonl(
        &workspace_root,
        "m15-reingest.jsonl",
        &[
            serde_json::json!({"role": "user", "content": "# task: stable task"}),
            serde_json::json!({"role": "assistant", "content": "Kept docs/m15-reingest.md unchanged"}),
            serde_json::json!({"role": "user", "content": "# task: evolving task"}),
            serde_json::json!({"role": "assistant", "content": "Updated result in docs/m15-reingest.md with better evidence"}),
        ],
    );

    let reingest_json = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "ingest",
            "m15-reingest.jsonl",
            "--source-kind",
            "cli_session",
            "--json",
        ],
    );
    let reingest_id = reingest_json["data"]["record"]["ingest_id"]
        .as_str()
        .expect("reingest id should exist")
        .to_string();
    let resegment_json = run_json_command(
        &workspace_root,
        &registry_root,
        &["memory", "trajectory", "segment", &reingest_id, "--json"],
    );
    let resegments = resegment_json["data"]["segments"]
        .as_array()
        .expect("segments should be array");
    assert_eq!(resegments.len(), 2);
    assert_eq!(resegments[0]["segment_id"], stable_segment_id);
    let new_evolving_segment_id = resegments[1]["segment_id"]
        .as_str()
        .expect("new evolving segment id should exist")
        .to_string();
    assert_ne!(new_evolving_segment_id, old_evolving_segment_id);
    assert_eq!(
        resegment_json["data"]["superseded_segment_ids"][0],
        old_evolving_segment_id
    );
    assert_eq!(
        resegments[1]["supersedes"][0], old_evolving_segment_id,
        "changed span should point back to the old segment"
    );

    let old_segment_explain = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "explain",
            &old_evolving_segment_id,
            "--json",
        ],
    );
    assert_eq!(old_segment_explain["data"]["status"], "superseded");
    assert_eq!(
        old_segment_explain["data"]["superseded_by"],
        new_evolving_segment_id
    );

    let old_memory_explain = run_json_command(
        &workspace_root,
        &registry_root,
        &["memory", "explain", "--record-id", &old_memory_id, "--json"],
    );
    assert_eq!(old_memory_explain["data"]["status"], "invalidated");
    assert_eq!(
        old_memory_explain["data"]["injection_decision"],
        "explain_visible_only"
    );
    assert!(old_memory_explain["data"]["invalidation_reason"]
        .as_str()
        .unwrap_or_default()
        .contains("trajectory_segment_superseded"));

    assert_eq!(
        first_segment_json["data"]["segments"][0]["fingerprint"], resegments[0]["fingerprint"],
        "unchanged segment fingerprint should be reused"
    );
}

#[test]
fn m15_adoption_rejects_unknown_memory_records() {
    let (workspace_root, registry_root) = init_test_project("m15_adoption_unknown_memory");

    let adoption_json = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "adoption",
            "record",
            "mem_missing",
            "--event",
            "retrieved",
            "--json",
        ],
    );

    assert_eq!(adoption_json["error"]["code"], "invariant_violation");
    assert!(adoption_json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("unknown durable memory record"));
}

#[test]
fn m15_trajectory_candidates_require_governance_fields_before_promotion() {
    let (workspace_root, registry_root) = init_test_project("m15_candidate_governance");
    let artifact_path = workspace_root.join("docs").join("m15-governance.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact has parent"))
        .expect("artifact parent should create");
    fs::write(&artifact_path, "governance fixture\n").expect("artifact should write");

    let queue_dir = workspace_root
        .join(".pmcli")
        .join("memory")
        .join("promotion_queue");
    fs::create_dir_all(&queue_dir).expect("promotion queue should exist");
    fs::write(
        queue_dir.join("bad_traj_candidate.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "trajectory.memory_candidate.v1",
            "conformance_line": "M15.segment_memory_candidate",
            "canonical_path": ".pmcli/memory/promotion_queue/bad_traj_candidate.json",
            "retention_policy": "reviewable_until_promoted_or_invalidated",
            "atomic_write_policy": "atomic_json_write",
            "candidate_id": "bad_traj_candidate",
            "segment_id": "traj_seg_missing_fields",
            "ingest_id": "traj_ingest_missing_fields",
            "project_id": "test_project",
            "title": "bad trajectory candidate",
            "summary": "bad trajectory candidate",
            "body": "assistant: learned from docs/m15-governance.md",
            "memory_kind": "trajectory_segment",
            "support_refs": ["docs/m15-governance.md"],
            "source_artifacts": ["docs/m15-governance.md"],
            "source_artifact_path": artifact_path.display().to_string(),
            "promotion_status": "queued",
            "created_at": "1",
            "updated_at": "1",
            "session_id": "session_1",
            "summary_ref": "bad trajectory candidate",
            "status": "pending_review",
            "confidence": "deterministic_quality_gate",
            "degraded_reasons": [],
            "promotes_to_durable_memory": false
        }))
        .expect("candidate should serialize"),
    )
    .expect("candidate should write");

    let promote_json = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "promote",
            "--candidate-id",
            "bad_traj_candidate",
            "--trust",
            "trusted",
            "--json",
        ],
    );

    assert!(promote_json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("trajectory promotion requires segment_tag and quality_id"));
}

#[test]
fn m15_quality_gate_blocks_private_and_unsupported_segments() {
    let (workspace_root, registry_root) = init_test_project("m15_quality_negative_gates");
    fs::create_dir_all(workspace_root.join("docs")).expect("docs should create");
    fs::write(
        workspace_root.join("docs").join("m15-private.md"),
        "private fixture\n",
    )
    .expect("fixture should write");
    let _session_path = write_session_jsonl(
        &workspace_root,
        "m15-negative.jsonl",
        &[
            serde_json::json!({"role": "user", "content": "# task: unsupported"}),
            serde_json::json!({"role": "assistant", "content": "Completed but cited no source artifact"}),
            serde_json::json!({"role": "user", "content": "# task: private"}),
            serde_json::json!({"role": "assistant", "content": "Wrote docs/m15-private.md with API_KEY=sk-TESTSECRET"}),
        ],
    );

    let ingest_json = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "ingest",
            "m15-negative.jsonl",
            "--source-kind",
            "cli_session",
            "--json",
        ],
    );
    let ingest_id = ingest_json["data"]["record"]["ingest_id"]
        .as_str()
        .expect("ingest id should exist")
        .to_string();
    let segment_json = run_json_command(
        &workspace_root,
        &registry_root,
        &["memory", "trajectory", "segment", &ingest_id, "--json"],
    );
    let unsupported_segment_id = segment_json["data"]["segments"][0]["segment_id"]
        .as_str()
        .expect("segment id should exist")
        .to_string();
    let private_segment_id = segment_json["data"]["segments"][1]["segment_id"]
        .as_str()
        .expect("segment id should exist")
        .to_string();

    let unsupported_label = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "label",
            &unsupported_segment_id,
            "--json",
        ],
    );
    assert_eq!(
        unsupported_label["data"]["quality"]["memory_eligible"],
        false
    );
    assert_eq!(
        unsupported_label["data"]["quality"]["failure_reason"],
        "missing_support_refs"
    );
    let unsupported_extract = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "extract",
            &unsupported_segment_id,
            "--json",
        ],
    );
    assert!(unsupported_extract["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("missing_support_refs"));

    let private_label = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "label",
            &private_segment_id,
            "--json",
        ],
    );
    assert_eq!(private_label["data"]["quality"]["memory_eligible"], false);
    assert_eq!(
        private_label["data"]["quality"]["failure_reason"],
        "privacy_risk"
    );
    let private_extract = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "trajectory",
            "extract",
            &private_segment_id,
            "--json",
        ],
    );
    assert!(private_extract["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("privacy_risk"));
}

#[test]
fn m15_trajectory_ingest_segments_extracts_and_tiers_memory_candidates() {
    let workspace_root = init_git_workspace("m15_trajectory_collective_memory");
    let registry_root = unique_temp_dir("m15_trajectory_collective_memory_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let artifact_path = workspace_root.join("docs").join("m15-fixture.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact has parent"))
        .expect("artifact parent should create");
    fs::write(&artifact_path, "M15 fixture artifact\n").expect("artifact should write");

    let _session_path = write_session_jsonl(
        &workspace_root,
        "m15-session.jsonl",
        &[
            serde_json::json!({"role": "user", "content": "# task: build docs"}),
            serde_json::json!({"role": "assistant", "content": "Created docs/m15-fixture.md and support ref docs/m15-fixture.md"}),
            serde_json::json!({"role": "user", "content": "# task: run tests"}),
            serde_json::json!({"role": "assistant", "content": "cargo test passed with support ref docs/m15-fixture.md"}),
        ],
    );

    let ingest_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "trajectory",
            "ingest",
            "m15-session.jsonl",
            "--source-kind",
            "cli_session",
            "--json",
        ])
        .output()
        .expect("trajectory ingest should execute");
    assert!(ingest_output.status.success(), "{ingest_output:?}");
    let ingest_json = output_json(&ingest_output);
    let ingest_id = ingest_json["data"]["record"]["ingest_id"]
        .as_str()
        .expect("ingest id should exist")
        .to_string();

    let segment_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "trajectory", "segment", &ingest_id, "--json"])
        .output()
        .expect("trajectory segment should execute");
    assert!(segment_output.status.success(), "{segment_output:?}");
    let segment_json = output_json(&segment_output);
    let segments = segment_json["data"]["segments"]
        .as_array()
        .expect("segments should be array");
    assert_eq!(segments.len(), 2);
    let first_segment_id = segments[0]["segment_id"]
        .as_str()
        .expect("segment id should exist")
        .to_string();

    let label_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "trajectory", "label", &first_segment_id, "--json"])
        .output()
        .expect("trajectory label should execute");
    assert!(label_output.status.success(), "{label_output:?}");
    let label_json = output_json(&label_output);
    assert_eq!(label_json["data"]["quality"]["memory_eligible"], true);
    assert!(
        !label_json["data"]["quality"]
            .as_object()
            .expect("quality should be an object")
            .keys()
            .any(|key| key.contains("export_eligible")),
        "M15 quality output must not expose a training-export authority field"
    );

    let extract_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "trajectory",
            "extract",
            &first_segment_id,
            "--json",
        ])
        .output()
        .expect("trajectory extract should execute");
    assert!(extract_output.status.success(), "{extract_output:?}");
    let extract_json = output_json(&extract_output);
    let candidate_id = extract_json["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id should exist")
        .to_string();
    assert_eq!(
        extract_json["data"]["candidate"]["segment_tag"],
        format!("segment:{}", &first_segment_id[..8])
    );

    let promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            &candidate_id,
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("memory promote should execute");
    assert!(promote_output.status.success(), "{promote_output:?}");
    let promoted_json = output_json(&promote_output);
    let memory_id = promoted_json["data"]["record"]["record_id"]
        .as_str()
        .expect("memory id should exist")
        .to_string();
    assert!(promoted_json["data"]["record"]["body"]
        .as_str()
        .expect("promoted body should be string")
        .contains("Created docs/m15-fixture.md"));

    for event in ["retrieved", "inspected", "cited_in_output"] {
        let adoption_output = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args([
                "memory", "adoption", "record", &memory_id, "--event", event, "--json",
            ])
            .output()
            .expect("adoption event should execute");
        assert!(adoption_output.status.success(), "{adoption_output:?}");
    }

    let tier_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "tier", "rebalance", "--json"])
        .output()
        .expect("tier rebalance should execute");
    assert!(tier_output.status.success(), "{tier_output:?}");

    let tier_explain_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "tier", "explain", &memory_id, "--json"])
        .output()
        .expect("tier explain should execute");
    assert!(
        tier_explain_output.status.success(),
        "{tier_explain_output:?}"
    );
    let tier_json = output_json(&tier_explain_output);
    assert_eq!(tier_json["data"]["explanation"]["tier"], "HOT");
    assert!(tier_json["data"]["explanation"]["governance_limits"]
        .as_array()
        .expect("governance limits should be array")
        .iter()
        .any(|value| value
            .as_str()
            .unwrap_or_default()
            .contains("tier_does_not_override")));
}

#[test]
fn m15_trajectory_ingest_rejects_workspace_external_sources() {
    let (workspace_root, registry_root) = init_test_project("m15_trajectory_external_source");
    let outside_dir = unique_temp_dir("m15_trajectory_outside_source");
    let outside_path = outside_dir.join("outside-session.jsonl");
    fs::write(
        &outside_path,
        format!(
            "{}\n",
            serde_json::json!({"role": "assistant", "content": "External trajectory should not enter project memory."})
        ),
    )
    .expect("outside source should write");

    let blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "trajectory",
            "ingest",
            outside_path.to_str().expect("outside path should be utf8"),
            "--source-kind",
            "cli_session",
            "--json",
        ])
        .output()
        .expect("external trajectory ingest should execute");
    assert_eq!(blocked.status.code(), Some(2));
    let blocked_json = output_json(&blocked);
    assert_eq!(blocked_json["error"]["code"], "usage_invalid");
    assert!(blocked_json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("workspace-relative"));
}

#[test]
fn working_memory_append_status_bounds_and_pins_records() {
    let workspace_root = init_git_workspace("working_memory_bounds");
    let registry_root = unique_temp_dir("working_memory_bounds_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let append_pinned = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "append",
            "--content",
            "pinned architectural decision",
            "--kind",
            "decision",
            "--pin",
            "--limit",
            "2",
            "--json",
        ])
        .output()
        .expect("pinned append should execute");
    assert!(append_pinned.status.success());

    let append_second = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "append",
            "--content",
            "old unpinned observation",
            "--kind",
            "observation",
            "--limit",
            "2",
            "--json",
        ])
        .output()
        .expect("second append should execute");
    assert!(append_second.status.success());

    let append_third = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "append",
            "--content",
            "new unpinned observation",
            "--kind",
            "observation",
            "--limit",
            "2",
            "--json",
        ])
        .output()
        .expect("third append should execute");
    assert!(append_third.status.success());

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "status", "--limit", "2", "--json"])
        .output()
        .expect("memory status should execute");
    assert!(status_output.status.success());

    let status_json = output_json(&status_output);
    let report = &status_json["data"];
    let status_schema = compiled_schema("memory_status_report.schema.json");
    if let Err(errors) = status_schema.validate(report) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("memory status should validate: {rendered:?}");
    }

    assert_eq!(report["limit"], 2);
    assert_eq!(report["active_count"], 2);
    assert_eq!(report["pinned_count"], 1);
    assert_eq!(report["evicted_count"], 1);
    assert_eq!(report["promotion_queue_count"], 0);

    let records = report["active_records"]
        .as_array()
        .expect("active records should be an array");
    assert_eq!(records.len(), 2);
    assert!(records.iter().any(
        |record| record["content"] == "pinned architectural decision" && record["pinned"] == true
    ));
    assert!(
        records
            .iter()
            .any(|record| record["content"] == "new unpinned observation"
                && record["pinned"] == false)
    );
    assert!(!records
        .iter()
        .any(|record| record["content"] == "old unpinned observation"));

    let invalid_limit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "append",
            "--content",
            "invalid limit",
            "--limit",
            "0",
            "--json",
        ])
        .output()
        .expect("invalid append should execute");
    assert!(!invalid_limit_output.status.success());
    let invalid_json = output_json(&invalid_limit_output);
    assert_eq!(invalid_json["error"]["code"], "usage_invalid");
}

#[test]
fn compact_stages_reviewable_digest_candidate_and_projectops_tick() {
    let workspace_root = init_git_workspace("working_memory_digest");
    let registry_root = unique_temp_dir("working_memory_digest_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Digest Source", "--json"])
        .output()
        .expect("session create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should exist")
        .to_string();

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let mut transcript = fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "user",
            "content": "complete M7 digest staging"
        })
    )
    .expect("user line should append");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "assistant",
            "content": "staged digest candidate without memory promotion"
        })
    )
    .expect("assistant line should append");

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let compact_json = output_json(&compact_output);
    let data = &compact_json["data"];
    let digest_candidate_id = data["digest_candidate_id"]
        .as_str()
        .expect("digest candidate id should be reported");
    let projectops_tick_id = data["projectops_tick_id"]
        .as_str()
        .expect("projectops tick id should be reported");
    assert_eq!(data["silent_summary_staged"], true);
    assert_eq!(data["durable_memory_promoted"], false);
    assert_eq!(data["digest_candidate_status"], "pending_review");

    let digest_path = workspace_root
        .join(".pmcli")
        .join("memory")
        .join("promotion_queue")
        .join(format!("{digest_candidate_id}.json"));
    let digest: Value = serde_json::from_str(
        &fs::read_to_string(&digest_path).expect("digest candidate should persist"),
    )
    .expect("digest candidate should parse");
    let digest_schema = compiled_schema("progress_digest_candidate.schema.json");
    if let Err(errors) = digest_schema.validate(&digest) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("digest candidate should validate: {rendered:?}");
    }
    assert_eq!(digest["candidate_id"], digest_candidate_id);
    assert_eq!(digest["session_id"], session_id);
    assert_eq!(digest["status"], "pending_review");
    assert_eq!(digest["promotes_to_durable_memory"], false);
    assert!(digest["support_refs"]
        .as_array()
        .expect("support refs should exist")
        .iter()
        .any(|value| value.as_str().unwrap_or_default().contains("summaries/")));

    let tick_path = workspace_root
        .join(".pmcli")
        .join("projectops")
        .join("ticks")
        .join(format!("{projectops_tick_id}.json"));
    let tick: Value =
        serde_json::from_str(&fs::read_to_string(&tick_path).expect("tick should persist"))
            .expect("tick should parse");
    let tick_schema = compiled_schema("projectops_tick.schema.json");
    if let Err(errors) = tick_schema.validate(&tick) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("projectops tick should validate: {rendered:?}");
    }
    assert_eq!(tick["tick_id"], projectops_tick_id);
    assert_eq!(tick["digest_candidate_id"], digest_candidate_id);
    assert_eq!(tick["trigger"], "session_compacted");

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "status", "--json"])
        .output()
        .expect("memory status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(status_json["data"]["promotion_queue_count"], 1);
}

#[test]
fn rapid_digest_staging_keeps_distinct_candidate_and_tick_files() {
    let workspace_root = init_git_workspace("working_memory_digest_ids");
    let registry_root = unique_temp_dir("working_memory_digest_ids_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Digest IDs", "--json"])
        .output()
        .expect("session create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should exist")
        .to_string();

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let mut transcript = fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "user",
            "content": "first digest id source"
        })
    )
    .expect("first line should append");

    let first_compact = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("first compact should execute");
    assert!(first_compact.status.success());
    let first_json = output_json(&first_compact);

    let second_compact = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("second compact should execute");
    assert!(second_compact.status.success());
    let second_json = output_json(&second_compact);

    assert_ne!(
        first_json["data"]["digest_candidate_id"],
        second_json["data"]["digest_candidate_id"]
    );
    assert_ne!(
        first_json["data"]["projectops_tick_id"],
        second_json["data"]["projectops_tick_id"]
    );
}

#[test]
fn durable_memory_query_uses_hybrid_lanes_for_identifier_and_provenance_recall() {
    let workspace_root = init_git_workspace("durable_memory_hybrid_retrieval");
    let registry_root = unique_temp_dir("durable_memory_hybrid_retrieval_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let noisy_source = workspace_root.join("notes").join("noise.md");
    fs::create_dir_all(noisy_source.parent().expect("noise parent should exist"))
        .expect("noise parent should be created");
    fs::write(
        &noisy_source,
        "docs mobile linking docs mobile linking docs mobile linking",
    )
    .expect("noisy source should write");
    write_promotion_candidate(
        &workspace_root,
        "dig_noisy_lexical",
        "sess_noise",
        "noise_summary",
        &noisy_source,
        &["support:noise"],
    );

    let canonical_source = workspace_root
        .join("docs")
        .join("mobile")
        .join("linking.md");
    fs::create_dir_all(
        canonical_source
            .parent()
            .expect("canonical parent should exist"),
    )
    .expect("canonical parent should be created");
    fs::write(
        &canonical_source,
        "Canonical mobile control decision: use the private overlay route for phone links.",
    )
    .expect("canonical source should write");
    write_promotion_candidate(
        &workspace_root,
        "dig_canonical_source_path",
        "sess_canonical",
        "canonical_summary",
        &canonical_source,
        &["implementation:evidence"],
    );

    let noisy_promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            "dig_noisy_lexical",
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("noisy memory promote should execute");
    assert!(noisy_promote.status.success());

    let canonical_promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            "dig_canonical_source_path",
            "--trust",
            "supported",
            "--json",
        ])
        .output()
        .expect("canonical memory promote should execute");
    assert!(canonical_promote.status.success());
    let canonical_id = output_json(&canonical_promote)["data"]["record"]["record_id"]
        .as_str()
        .expect("canonical record id should exist")
        .to_string();

    let query_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "query",
            "docs/mobile/linking.md",
            "--limit",
            "2",
            "--json",
        ])
        .output()
        .expect("memory query should execute");
    assert!(query_output.status.success());
    let query_json = output_json(&query_output);
    assert_eq!(
        query_json["data"]["route"],
        "hybrid_vector_temporal_project_durable_memory"
    );

    let matched = query_json["data"]["matched_records"]
        .as_array()
        .expect("matched records should exist");
    assert_eq!(matched[0]["record"]["record_id"], canonical_id);
    let matched_via = matched[0]["matched_via"]
        .as_array()
        .expect("matched_via should exist");
    assert!(matched_via.iter().any(|value| value == "exact_identifier"));
    assert!(matched_via.iter().any(|value| value == "source_artifacts"));
}

#[test]
fn durable_memory_query_uses_vector_lane_for_semantic_alias_recall() {
    let workspace_root = init_git_workspace("durable_memory_vector_retrieval");
    let registry_root = unique_temp_dir("durable_memory_vector_retrieval_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let canonical_source = workspace_root.join("docs").join("remote.md");
    fs::create_dir_all(
        canonical_source
            .parent()
            .expect("source parent should exist"),
    )
    .expect("source parent should be created");
    fs::write(
        &canonical_source,
        "Mobile private overlay route keeps the app connected across networks.",
    )
    .expect("canonical source should write");
    write_promotion_candidate(
        &workspace_root,
        "dig_vector_mobile_overlay",
        "sess_vector",
        "vector_summary",
        &canonical_source,
        &["decision:remote-control"],
    );

    let promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            "dig_vector_mobile_overlay",
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("memory promote should execute");
    assert!(promote_output.status.success());
    let record_id = output_json(&promote_output)["data"]["record"]["record_id"]
        .as_str()
        .expect("record id should exist")
        .to_string();

    let query_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "query", "phone tunnel", "--limit", "3", "--json"])
        .output()
        .expect("memory query should execute");
    assert!(query_output.status.success());
    let query_json = output_json(&query_output);
    assert_eq!(
        query_json["data"]["route"],
        "hybrid_vector_temporal_project_durable_memory"
    );
    let matched = query_json["data"]["matched_records"]
        .as_array()
        .expect("matched records should exist");
    assert_eq!(matched[0]["record"]["record_id"], record_id);
    assert!(matched[0]["matched_via"]
        .as_array()
        .expect("matched_via should exist")
        .iter()
        .any(|value| value == "vector_embedding"));
}

#[test]
fn durable_memory_query_uses_temporal_graph_to_surface_current_successor() {
    let workspace_root = init_git_workspace("durable_memory_temporal_graph");
    let registry_root = unique_temp_dir("durable_memory_temporal_graph_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let old_source = workspace_root.join("docs").join("old-decision.md");
    let new_source = workspace_root.join("docs").join("new-decision.md");
    fs::create_dir_all(old_source.parent().expect("source parent should exist"))
        .expect("source parent should be created");
    fs::write(
        &old_source,
        "Legacy cobalt setup was used for mobile access.",
    )
    .expect("old source should write");
    fs::write(
        &new_source,
        "Current private overlay route is the canonical mobile access decision.",
    )
    .expect("new source should write");
    write_promotion_candidate(
        &workspace_root,
        "dig_old_cobalt",
        "sess_temporal",
        "old_summary",
        &old_source,
        &["decision:mobile-access"],
    );
    write_promotion_candidate(
        &workspace_root,
        "dig_new_overlay",
        "sess_temporal",
        "new_summary",
        &new_source,
        &["decision:mobile-access"],
    );

    let old_promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            "dig_old_cobalt",
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("old memory promote should execute");
    assert!(old_promote.status.success());
    let old_id = output_json(&old_promote)["data"]["record"]["record_id"]
        .as_str()
        .expect("old record id should exist")
        .to_string();

    let new_promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            "dig_new_overlay",
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("new memory promote should execute");
    assert!(new_promote.status.success());
    let new_id = output_json(&new_promote)["data"]["record"]["record_id"]
        .as_str()
        .expect("new record id should exist")
        .to_string();

    let invalidate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "invalidate",
            "--record-id",
            &old_id,
            "--status",
            "superseded",
            "--reason",
            "private overlay route replaced cobalt setup",
            "--json",
        ])
        .output()
        .expect("old memory supersede should execute");
    assert!(invalidate_output.status.success());

    let query_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "query",
            "legacy cobalt setup",
            "--limit",
            "3",
            "--json",
        ])
        .output()
        .expect("memory query should execute");
    assert!(query_output.status.success());
    let query_json = output_json(&query_output);
    let matched = query_json["data"]["matched_records"]
        .as_array()
        .expect("matched records should exist");
    assert_eq!(matched[0]["record"]["record_id"], new_id);
    assert_eq!(matched[0]["auto_inject"], true);
    assert!(matched[0]["matched_via"]
        .as_array()
        .expect("matched_via should exist")
        .iter()
        .any(|value| value == "temporal_graph"));
    assert!(!matched
        .iter()
        .any(|entry| entry["record"]["record_id"] == old_id));
}

#[test]
fn durable_memory_promote_query_explain_and_invalidate_are_governed() {
    let workspace_root = init_git_workspace("durable_memory_lifecycle");
    let registry_root = unique_temp_dir("durable_memory_lifecycle_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Durable Source", "--json"])
        .output()
        .expect("session create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should exist")
        .to_string();

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let mut transcript = fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "assistant",
            "content": "M8 durable memory must preserve support refs and block invalidated auto injection"
        })
    )
    .expect("transcript line should append");

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let compact_json = output_json(&compact_output);
    let candidate_id = compact_json["data"]["digest_candidate_id"]
        .as_str()
        .expect("digest candidate id should exist")
        .to_string();

    let promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            &candidate_id,
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("memory promote should execute");
    assert!(promote_output.status.success());
    let promote_json = output_json(&promote_output);
    let record = &promote_json["data"]["record"];
    let record_schema = compiled_schema("memory_record.schema.json");
    if let Err(errors) = record_schema.validate(record) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("memory record should validate: {rendered:?}");
    }
    let record_id = record["record_id"]
        .as_str()
        .expect("record id should exist")
        .to_string();
    assert_eq!(record["source_candidate_id"], candidate_id);
    assert_eq!(record["status"], "trusted");
    assert_eq!(record["injectable"], true);

    let query_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "query",
            "durable support refs",
            "--limit",
            "5",
            "--json",
        ])
        .output()
        .expect("memory query should execute");
    assert!(query_output.status.success());
    let query_json = output_json(&query_output);
    let query_schema = compiled_schema("memory_query_result.schema.json");
    if let Err(errors) = query_schema.validate(&query_json["data"]) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("memory query should validate: {rendered:?}");
    }
    assert_eq!(query_json["data"]["auto_inject_count"], 1);
    assert!(query_json["data"]["matched_records"]
        .as_array()
        .expect("matched records should exist")
        .iter()
        .any(|entry| entry["record"]["record_id"] == record_id
            && entry["auto_inject"] == true
            && entry["provenance"]["source_candidate_id"] == candidate_id));

    let explain_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "explain", "--record-id", &record_id, "--json"])
        .output()
        .expect("memory explain should execute");
    assert!(explain_output.status.success());
    let explain_json = output_json(&explain_output);
    let explain_schema = compiled_schema("memory_explain_record.schema.json");
    if let Err(errors) = explain_schema.validate(&explain_json["data"]) {
        let rendered = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("memory explain should validate: {rendered:?}");
    }
    assert_eq!(explain_json["data"]["record_id"], record_id);
    assert_eq!(
        explain_json["data"]["injection_decision"],
        "auto_inject_allowed"
    );

    let invalidate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "invalidate",
            "--record-id",
            &record_id,
            "--reason",
            "test invalidation",
            "--json",
        ])
        .output()
        .expect("memory invalidate should execute");
    assert!(invalidate_output.status.success());

    let query_after_invalidate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "query",
            "durable support refs",
            "--include-inactive",
            "--json",
        ])
        .output()
        .expect("memory query after invalidate should execute");
    assert!(query_after_invalidate.status.success());
    let query_after_json = output_json(&query_after_invalidate);
    assert_eq!(query_after_json["data"]["auto_inject_count"], 0);
    assert!(query_after_json["data"]["matched_records"]
        .as_array()
        .expect("matched records should exist")
        .iter()
        .any(|entry| entry["record"]["record_id"] == record_id
            && entry["auto_inject"] == false
            && entry["record"]["status"] == "invalidated"));

    let explain_after_invalidate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "explain", "--record-id", &record_id, "--json"])
        .output()
        .expect("memory explain after invalidate should execute");
    assert!(explain_after_invalidate.status.success());
    let explain_after_json = output_json(&explain_after_invalidate);
    assert_eq!(
        explain_after_json["data"]["injection_decision"],
        "explain_visible_only"
    );

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "status", "--json"])
        .output()
        .expect("memory status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(status_json["data"]["durable_count"], 1);
    assert_eq!(status_json["data"]["durable_injectable_count"], 0);
    assert_eq!(status_json["data"]["durable_status"], "available");
}

#[test]
fn durable_memory_superseded_records_stop_auto_injecting_but_remain_explainable() {
    let workspace_root = init_git_workspace("durable_memory_superseded");
    let registry_root = unique_temp_dir("durable_memory_superseded_registry")
        .display()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("project init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "sessions",
            "create",
            "--title",
            "Supersede Source",
            "--json",
        ])
        .output()
        .expect("session create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should exist")
        .to_string();

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let mut transcript = fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "assistant",
            "content": "superseded memory should never auto inject"
        })
    )
    .expect("transcript line should append");

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let candidate_id = output_json(&compact_output)["data"]["digest_candidate_id"]
        .as_str()
        .expect("digest candidate id should exist")
        .to_string();

    let promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "promote",
            "--candidate-id",
            &candidate_id,
            "--trust",
            "trusted",
            "--json",
        ])
        .output()
        .expect("memory promote should execute");
    assert!(promote_output.status.success());
    let record_id = output_json(&promote_output)["data"]["record"]["record_id"]
        .as_str()
        .expect("record id should exist")
        .to_string();

    let supersede_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "invalidate",
            "--record-id",
            &record_id,
            "--status",
            "superseded",
            "--reason",
            "newer canonical decision replaced it",
            "--json",
        ])
        .output()
        .expect("memory supersede should execute");
    assert!(supersede_output.status.success());
    assert_eq!(
        output_json(&supersede_output)["data"]["record"]["status"],
        "superseded"
    );

    let active_query = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "query", "superseded memory", "--json"])
        .output()
        .expect("active query should execute");
    assert!(active_query.status.success());
    assert_eq!(output_json(&active_query)["data"]["auto_inject_count"], 0);

    let explain_query = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "memory",
            "query",
            "superseded memory",
            "--include-inactive",
            "--json",
        ])
        .output()
        .expect("explain query should execute");
    assert!(explain_query.status.success());
    let explain_query_json = output_json(&explain_query);
    assert!(explain_query_json["data"]["matched_records"]
        .as_array()
        .expect("matched records should exist")
        .iter()
        .any(|entry| entry["record"]["record_id"] == record_id
            && entry["record"]["status"] == "superseded"
            && entry["auto_inject"] == false));

    let explain_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "explain", "--record-id", &record_id, "--json"])
        .output()
        .expect("memory explain should execute");
    assert!(explain_output.status.success());
    let explain_json = output_json(&explain_output);
    assert_eq!(explain_json["data"]["status"], "superseded");
    assert_eq!(
        explain_json["data"]["injection_decision"],
        "explain_visible_only"
    );
}

#[test]
fn m16_skill_evolution_crystallizes_verified_candidates_through_m11_envelopes() {
    let (workspace_root, registry_root) = init_test_project("m16_skill_evolution");
    let artifact_path = workspace_root.join("docs").join("m16-skill-evidence.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        "Skill evidence: always inspect canonical docs before publishing.\n",
    )
    .expect("artifact should write");

    for (candidate_id, summary) in [
        (
            "m16_candidate_a",
            "skill evolution workflow requires canonical docs inspection",
        ),
        (
            "m16_candidate_b",
            "skill evolution workflow validates support refs before publishing",
        ),
    ] {
        write_promotion_candidate(
            &workspace_root,
            candidate_id,
            "m16_session",
            summary,
            &artifact_path,
            &["docs/m16-skill-evidence.md", "segment:m16"],
        );
        let promote = run_json_command(
            &workspace_root,
            &registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                candidate_id,
                "--trust",
                "trusted",
                "--json",
            ],
        );
        assert_eq!(promote["data"]["record"]["injectable"], true);
    }

    let clusters = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "cluster",
            "--min-members",
            "2",
            "--json",
        ],
    );
    let cluster = clusters["data"]["clusters"]
        .as_array()
        .expect("clusters should be array")
        .first()
        .expect("at least one cluster should exist")
        .clone();
    assert_eq!(cluster["schema_version"], "skill.knowledge_cluster.v1");
    assert_eq!(cluster["cluster_status"], "ready_for_crystallization");
    assert_eq!(cluster["memory_count"], 2);
    let cluster_id = cluster["cluster_id"]
        .as_str()
        .expect("cluster id")
        .to_string();

    let crystallized = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "crystallize", &cluster_id, "--json"],
    );
    let candidate_id = crystallized["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();
    assert_eq!(
        crystallized["data"]["candidate"]["schema_version"],
        "skill.evolution_candidate.v1"
    );
    assert!(crystallized["data"]["candidate"]["skill_artifact_path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("SKILL.md"));

    let verified = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "verify", &candidate_id, "--json"],
    );
    assert_eq!(verified["data"]["verification"]["decision"], "verified");
    assert!(
        verified["data"]["verification"]["grounded_evidence_ratio"]
            .as_f64()
            .expect("grounded ratio should be numeric")
            >= 0.8
    );
    assert!(
        verified["data"]["structure_score"]["overall_score"]
            .as_f64()
            .expect("structure score should be numeric")
            > 0.0
    );

    let submitted = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "submit", &candidate_id, "--json"],
    );
    assert_eq!(submitted["data"]["status"], "candidate_recorded");
    assert_eq!(
        submitted["data"]["envelope"]["skill_id"],
        "skills.evolve.crystallized"
    );
    assert_eq!(
        submitted["data"]["envelope"]["visibility"],
        "review_candidate"
    );
}

#[test]
fn m18_feedback_collects_governed_signals_and_calibrates_advisory_projection() {
    let (workspace_root, registry_root) = init_test_project("m18_feedback_loop");
    let artifact_path = workspace_root.join("docs").join("m18-feedback-evidence.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        "Feedback evidence: retrieval adoption should tune recommendations, not publish truth.\n",
    )
    .expect("artifact should write");

    for candidate_id in ["m18_candidate_a", "m18_candidate_b"] {
        write_promotion_candidate(
            &workspace_root,
            candidate_id,
            "m18_session",
            "feedback loop memory supports retrieval calibration",
            &artifact_path,
            &["docs/m18-feedback-evidence.md", "segment:m18"],
        );
        let promote = run_json_command(
            &workspace_root,
            &registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                candidate_id,
                "--trust",
                "trusted",
                "--json",
            ],
        );
        let memory_id = promote["data"]["record"]["record_id"]
            .as_str()
            .expect("memory id")
            .to_string();
        for event in ["retrieved", "inspected"] {
            let adoption = run_json_command(
                &workspace_root,
                &registry_root,
                &[
                    "memory", "adoption", "record", &memory_id, "--event", event, "--json",
                ],
            );
            assert_eq!(adoption["data"]["event"]["event_kind"], event);
        }
    }

    let clusters = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "cluster",
            "--min-members",
            "2",
            "--json",
        ],
    );
    let cluster_id = clusters["data"]["clusters"][0]["cluster_id"]
        .as_str()
        .expect("cluster id")
        .to_string();
    let crystallized = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "crystallize", &cluster_id, "--json"],
    );
    let candidate_id = crystallized["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();
    let verified = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "verify", &candidate_id, "--json"],
    );
    assert_eq!(verified["data"]["verification"]["decision"], "verified");

    let collected = run_json_command(
        &workspace_root,
        &registry_root,
        &["feedback", "collect", "--json"],
    );
    let signals = collected["data"]["signals"]
        .as_array()
        .expect("signals should be array");
    assert!(signals
        .iter()
        .any(|signal| signal["signal_kind"] == "memory_adoption"));
    assert!(signals
        .iter()
        .any(|signal| signal["signal_kind"] == "skill_verification"));

    let calibrated = run_json_command(
        &workspace_root,
        &registry_root,
        &["feedback", "calibrate", "--json"],
    );
    assert_eq!(
        calibrated["data"]["calibration"]["authority_boundary"],
        "advisory_only"
    );
    assert!(calibrated["data"]["calibration"]["routing_adjustments"]
        .as_array()
        .expect("routing adjustments should be array")
        .iter()
        .any(|adjustment| adjustment["target_lane"] == "memory_retrieval"));

    let status = run_json_command(
        &workspace_root,
        &registry_root,
        &["feedback", "status", "--json"],
    );
    assert_eq!(
        status["data"]["latest_calibration"]["authority_boundary"],
        "advisory_only"
    );

    let query = run_json_command(
        &workspace_root,
        &registry_root,
        &["memory", "query", "retrieval calibration", "--json"],
    );
    assert!(query["data"]["route"]
        .as_str()
        .expect("memory query route should be a string")
        .contains("feedback_calibrated"));
    assert!(query["data"]["matched_records"]
        .as_array()
        .expect("matched records should be array")
        .iter()
        .any(|entry| entry["matched_via"]
            .as_array()
            .expect("matched via should be array")
            .iter()
            .any(|value| value == "feedback_calibration")
            && entry["feedback_adjustments"]
                .as_array()
                .expect("feedback adjustments should be array")
                .iter()
                .any(|value| value
                    .as_str()
                    .unwrap_or_default()
                    .contains("memory_adoption"))));
}

#[test]
fn m18_negative_feedback_downranks_memory_query_results() {
    let (workspace_root, registry_root) = init_test_project("m18_negative_feedback_rank");
    let artifact_path = workspace_root.join("docs").join("m18-negative-feedback.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        "Negative feedback evidence: rejected memories should lose retrieval priority.\n",
    )
    .expect("artifact should write");

    let mut memory_ids = BTreeMap::<String, String>::new();
    for (candidate_id, summary) in [
        (
            "m18_negative_priority",
            "retrieval calibration priority target obsolete workflow",
        ),
        (
            "m18_negative_baseline",
            "retrieval calibration baseline current workflow",
        ),
    ] {
        write_promotion_candidate(
            &workspace_root,
            candidate_id,
            "m18_negative_session",
            summary,
            &artifact_path,
            &["docs/m18-negative-feedback.md", "segment:m18-negative"],
        );
        let promoted = run_json_command(
            &workspace_root,
            &registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                candidate_id,
                "--trust",
                "trusted",
                "--json",
            ],
        );
        memory_ids.insert(
            candidate_id.to_string(),
            promoted["data"]["record"]["record_id"]
                .as_str()
                .expect("memory id should exist")
                .to_string(),
        );
    }
    let priority_id = memory_ids
        .get("m18_negative_priority")
        .expect("priority memory id")
        .clone();
    let baseline_id = memory_ids
        .get("m18_negative_baseline")
        .expect("baseline memory id")
        .clone();

    let before = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "query",
            "retrieval calibration priority",
            "--json",
        ],
    );
    assert_eq!(
        before["data"]["matched_records"][0]["record"]["record_id"],
        priority_id
    );

    let adoption = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "adoption",
            "record",
            &priority_id,
            "--event",
            "rejected",
            "--json",
        ],
    );
    assert_eq!(adoption["data"]["event"]["weight"], -3.0);
    let calibrated = run_json_command(
        &workspace_root,
        &registry_root,
        &["feedback", "calibrate", "--json"],
    );
    assert_eq!(
        calibrated["data"]["calibration"]["authority_boundary"],
        "advisory_only"
    );

    let after = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "memory",
            "query",
            "retrieval calibration priority",
            "--json",
        ],
    );
    assert_eq!(
        after["data"]["matched_records"][0]["record"]["record_id"],
        baseline_id
    );
    assert!(after["data"]["matched_records"]
        .as_array()
        .expect("matched records should be array")
        .iter()
        .any(|entry| entry["record"]["record_id"] == priority_id
            && entry["feedback_adjustments"]
                .as_array()
                .expect("feedback adjustments should be array")
                .iter()
                .any(|value| value
                    .as_str()
                    .unwrap_or_default()
                    .contains("negative_weight"))));
}

#[test]
fn m16_verified_evolved_skills_are_projected_into_existing_prompt_context() {
    let (workspace_root, registry_root) = init_test_project("m16_prompt_projection");
    let artifact_path = workspace_root.join("docs").join("m16-context-evidence.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        "Context evidence: verified evolved skills should be recommended in prompt context.\n",
    )
    .expect("artifact should write");

    for (candidate_id, summary) in [
        (
            "m16_context_candidate_a",
            "verified evolved skills improve prompt context with canonical workflow",
        ),
        (
            "m16_context_candidate_b",
            "verified evolved skills stay governed while entering prompt context",
        ),
    ] {
        write_promotion_candidate(
            &workspace_root,
            candidate_id,
            "m16_context_session",
            summary,
            &artifact_path,
            &["docs/m16-context-evidence.md", "segment:m16-context"],
        );
        let promoted = run_json_command(
            &workspace_root,
            &registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                candidate_id,
                "--trust",
                "trusted",
                "--json",
            ],
        );
        assert_eq!(promoted["data"]["record"]["injectable"], true);
    }

    let clusters = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "cluster",
            "--min-members",
            "2",
            "--json",
        ],
    );
    let cluster_id = clusters["data"]["clusters"][0]["cluster_id"]
        .as_str()
        .expect("cluster id")
        .to_string();
    let crystallized = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "crystallize", &cluster_id, "--json"],
    );
    let candidate_id = crystallized["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();
    let verified = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "verify", &candidate_id, "--json"],
    );
    assert_eq!(verified["data"]["verification"]["decision"], "verified");

    let prompt = run_json_command(
        &workspace_root,
        &registry_root,
        &["prompt", "status", "--json"],
    );
    let recommendations = prompt["data"]["skill_outputs"]["evolved_recommendations"]
        .as_array()
        .expect("evolved recommendations should be array");
    assert!(recommendations.iter().any(|entry| {
        entry["candidate_id"] == candidate_id
            && entry["verification_status"] == "verified"
            && entry["publication_status"] == "not_submitted"
    }));
    assert!(prompt["data"]["skill_outputs"]["projection_policy"]
        .as_str()
        .expect("projection policy should be string")
        .contains("verified_evolution_recommendations"));
}

#[test]
fn m16_cluster_enhancement_discovers_distinct_memory_workflow_topics() {
    let (workspace_root, registry_root) = init_test_project("m16_cluster_enhancement");
    let artifact_path = workspace_root.join("docs").join("m16-cluster-evidence.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        "Cluster evidence: retrieval calibration and canonical doc publication are separate workflows.\n",
    )
    .expect("artifact should write");

    for (candidate_id, summary, support_ref) in [
        (
            "m16_cluster_retrieval_a",
            "retrieval calibration feedback ranking improves memory recall lane",
            "segment:retrieval-a",
        ),
        (
            "m16_cluster_retrieval_b",
            "calibration retrieval feedback boosts memory adoption ranking",
            "segment:retrieval-b",
        ),
        (
            "m16_cluster_doc_a",
            "canonical doc publication validates docframe lifecycle before release",
            "segment:doc-a",
        ),
        (
            "m16_cluster_doc_b",
            "publication canonical doc routes skill output through review gate",
            "segment:doc-b",
        ),
    ] {
        write_promotion_candidate(
            &workspace_root,
            candidate_id,
            "m16_cluster_session",
            summary,
            &artifact_path,
            &["docs/m16-cluster-evidence.md", support_ref],
        );
        let promoted = run_json_command(
            &workspace_root,
            &registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                candidate_id,
                "--trust",
                "trusted",
                "--json",
            ],
        );
        assert_eq!(promoted["data"]["record"]["injectable"], true);
    }

    let clusters = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "cluster",
            "--min-members",
            "2",
            "--json",
        ],
    );
    let cluster_records = clusters["data"]["clusters"]
        .as_array()
        .expect("clusters should be array");
    assert!(
        cluster_records.len() >= 2,
        "{}",
        serde_json::to_string_pretty(&clusters).unwrap()
    );
    assert!(cluster_records.iter().any(|cluster| {
        cluster["memory_count"] == 2
            && cluster["cluster_topic"]
                .as_str()
                .unwrap_or_default()
                .contains("retrieval")
    }));
    assert!(cluster_records.iter().any(|cluster| {
        cluster["memory_count"] == 2
            && cluster["cluster_topic"]
                .as_str()
                .unwrap_or_default()
                .contains("canonical")
    }));
}

#[test]
fn m16_verified_evolved_skill_install_enters_existing_skill_registry() {
    let (workspace_root, registry_root) = init_test_project("m16_skill_install");
    let artifact_path = workspace_root.join("docs").join("m16-install-evidence.md");
    fs::create_dir_all(artifact_path.parent().expect("artifact should have parent"))
        .expect("artifact dir should create");
    fs::write(
        &artifact_path,
        "Install evidence: verified evolved skill should become a normal local skill.\n",
    )
    .expect("artifact should write");

    for (candidate_id, summary) in [
        (
            "m16_install_candidate_a",
            "local skill install requires verified candidate approval",
        ),
        (
            "m16_install_candidate_b",
            "local skill install writes normal skill manifest path",
        ),
    ] {
        write_promotion_candidate(
            &workspace_root,
            candidate_id,
            "m16_install_session",
            summary,
            &artifact_path,
            &["docs/m16-install-evidence.md", "segment:m16-install"],
        );
        let promoted = run_json_command(
            &workspace_root,
            &registry_root,
            &[
                "memory",
                "promote",
                "--candidate-id",
                candidate_id,
                "--trust",
                "trusted",
                "--json",
            ],
        );
        assert_eq!(promoted["data"]["record"]["injectable"], true);
    }

    let clusters = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "cluster",
            "--min-members",
            "2",
            "--json",
        ],
    );
    let cluster_id = clusters["data"]["clusters"][0]["cluster_id"]
        .as_str()
        .expect("cluster id")
        .to_string();
    let crystallized = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "crystallize", &cluster_id, "--json"],
    );
    let candidate_id = crystallized["data"]["candidate"]["candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();
    let skill_id = crystallized["data"]["candidate"]["skill_id"]
        .as_str()
        .expect("skill id")
        .to_string();
    let blocked = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "install", &candidate_id, "--json"],
    );
    assert_eq!(blocked["error"]["code"], "publication_gate_required");

    let verified = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "verify", &candidate_id, "--json"],
    );
    assert_eq!(verified["data"]["verification"]["decision"], "verified");
    let installed = run_json_command(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "install",
            &candidate_id,
            "--approve-human-gate",
            "--json",
        ],
    );
    assert_eq!(installed["data"]["install_state"], "installed");
    assert_eq!(installed["data"]["skill_id"], skill_id);
    assert!(workspace_root
        .join(".codex")
        .join("skills")
        .join(&skill_id)
        .join("SKILL.md")
        .exists());

    let listed = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "list", "--json"],
    );
    assert!(listed["data"]["skills"]
        .as_array()
        .expect("skills should be array")
        .iter()
        .any(|entry| entry["skill_id"] == skill_id));
    let inspected = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "inspect", &skill_id, "--json"],
    );
    assert_eq!(inspected["data"]["skill_id"], skill_id);
    assert!(inspected["data"]["manifest_path"]
        .as_str()
        .unwrap_or_default()
        .contains(".codex/skills"));
}

#[test]
fn m16_skill_install_rejects_unverified_candidate_even_with_human_gate() {
    let (workspace_root, registry_root) = init_test_project("m16_skill_install_unverified");
    let (candidate_id, _skill_id) = create_m16_skill_install_candidate(
        &workspace_root,
        &registry_root,
        "m16_unverified_candidate",
    );

    let blocked = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "install",
            &candidate_id,
            "--approve-human-gate",
            "--json",
        ],
    );
    assert_eq!(blocked["error"]["code"], "publication_gate_required");
    assert!(blocked["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("not verified"));
}

#[test]
fn m16_skill_install_rejects_artifacts_outside_workspace() {
    let (workspace_root, registry_root) = init_test_project("m16_skill_install_artifact_path");
    let (candidate_id, _skill_id) = create_m16_skill_install_candidate(
        &workspace_root,
        &registry_root,
        "m16_outside_artifact_candidate",
    );
    let verified = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "verify", &candidate_id, "--json"],
    );
    assert_eq!(verified["data"]["verification"]["decision"], "verified");

    let absolute_outside_path = workspace_root
        .parent()
        .expect("workspace should have parent")
        .join("m16-outside-skill.md");
    fs::write(&absolute_outside_path, "# outside\n").expect("outside artifact should write");
    rewrite_evolution_candidate(&workspace_root, &candidate_id, |candidate| {
        candidate["skill_artifact_path"] =
            Value::String(absolute_outside_path.display().to_string());
    });
    let absolute_blocked = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "install",
            &candidate_id,
            "--approve-human-gate",
            "--json",
        ],
    );
    assert_eq!(absolute_blocked["error"]["code"], "usage_invalid");
    assert!(absolute_blocked["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("workspace-relative"));

    rewrite_evolution_candidate(&workspace_root, &candidate_id, |candidate| {
        candidate["skill_artifact_path"] = Value::String("../m16-outside-skill.md".to_string());
    });
    let traversal_blocked = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "install",
            &candidate_id,
            "--approve-human-gate",
            "--json",
        ],
    );
    assert_eq!(traversal_blocked["error"]["code"], "usage_invalid");
    assert!(traversal_blocked["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("workspace-relative"));
}

#[test]
fn m16_skill_install_rejects_existing_manifest_overwrite() {
    let (workspace_root, registry_root) = init_test_project("m16_skill_install_no_overwrite");
    let (candidate_id, skill_id) = create_m16_skill_install_candidate(
        &workspace_root,
        &registry_root,
        "m16_no_overwrite_candidate",
    );
    let verified = run_json_command(
        &workspace_root,
        &registry_root,
        &["skills", "evolve", "verify", &candidate_id, "--json"],
    );
    assert_eq!(verified["data"]["verification"]["decision"], "verified");

    let manifest_path = workspace_root
        .join(".codex")
        .join("skills")
        .join(&skill_id)
        .join("SKILL.md");
    fs::create_dir_all(manifest_path.parent().expect("manifest should have parent"))
        .expect("skill dir should create");
    fs::write(&manifest_path, "# original local skill\n").expect("existing manifest should write");

    let blocked = run_json_command_expect_failure(
        &workspace_root,
        &registry_root,
        &[
            "skills",
            "evolve",
            "install",
            &candidate_id,
            "--approve-human-gate",
            "--json",
        ],
    );
    assert_eq!(blocked["error"]["code"], "usage_invalid");
    assert!(blocked["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("already exists"));
    assert_eq!(
        fs::read_to_string(&manifest_path).expect("manifest should remain"),
        "# original local skill\n"
    );
}
