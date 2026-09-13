use crate::support::{init_git_workspace, unique_temp_dir, write_text_file};
use research_cli::goals::GoalAutomationMode;
use research_cli::orchestration::{
    save_run, OrchestrationRun, OrchestrationStep, OrchestrationStepStatus,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn astra_bin() -> String {
    std::env::var("CARGO_BIN_EXE_astra").expect("cargo should expose built astra binary path")
}

fn registry_root_env() -> String {
    unique_temp_dir("registry_env").display().to_string()
}

fn output_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid json")
}

fn run_cli_json(workspace_root: &Path, registry_root: &str, args: &[&str]) -> Value {
    let output = Command::new(cargo_bin())
        .current_dir(workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", registry_root)
        .args(args)
        .output()
        .expect("research-cli command should execute");
    assert!(
        output.status.success(),
        "command {:?} stdout: {} stderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output_json(&output)
}

fn run_astra_research_tick_json_with_config(
    workspace_root: &Path,
    registry_root: &str,
    config_root: Option<&str>,
    job_id: &str,
) -> Value {
    let mut command = Command::new(astra_bin());
    command
        .current_dir(workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", registry_root);
    if let Some(config_root) = config_root {
        command
            .env("RESEARCH_CLI_CONFIG_HOME", config_root)
            .env("OPENAI_API_KEY", "sk-test");
    }
    let tick = command
        .args(["research", "jobs", "tick", job_id, "--json"])
        .output()
        .expect("research jobs tick should execute");
    assert!(
        tick.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&tick.stderr),
        String::from_utf8_lossy(&tick.stdout)
    );
    output_json(&tick)
}

fn spawn_mock_openai_provider_server(
    response_body: String,
) -> (
    u16,
    std::sync::mpsc::Sender<()>,
    thread::JoinHandle<Vec<String>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut requests = Vec::new();
        loop {
            if stop_rx.try_recv().is_ok() {
                return requests;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    requests.push(String::from_utf8_lossy(&request).to_string());
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return requests;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });
    (port, stop_tx, server)
}

fn write_mock_openai_provider_settings(config_root: &str, port: u16) {
    write_text_file(
        Path::new(config_root).join("settings.json"),
        &format!(
            r#"{{
  "provider_profiles": {{
    "openai": {{
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:{port}/v1",
      "auth_env_var": "OPENAI_API_KEY",
      "default_model": "gpt-4.1-mini",
      "supported_models": ["gpt-4.1-mini"],
      "chat_completion_streaming": false
    }}
  }},
  "default_provider": "openai",
  "default_model": "gpt-4.1-mini"
}}"#
        ),
    );
}

fn start_mock_openai_provider(
    config_root: &str,
    response_body: impl Into<String>,
) -> (std::sync::mpsc::Sender<()>, thread::JoinHandle<Vec<String>>) {
    let (port, stop_tx, server) = spawn_mock_openai_provider_server(response_body.into());
    write_mock_openai_provider_settings(config_root, port);
    (stop_tx, server)
}

fn mock_openai_assistant_response_body(content: &str) -> String {
    json!({
        "id": "chatcmpl-test",
        "status": "completed",
        "output_text": content,
        "output": [{
            "type": "message",
            "content": [{
                "type": "output_text",
                "text": content
            }]
        }],
        "choices": [{
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": content
            }
        }],
        "usage": {
            "prompt_tokens": 3,
            "completion_tokens": 4,
            "total_tokens": 7
        }
    })
    .to_string()
}

fn run_astra_research_tick_until_review(
    workspace_root: &Path,
    registry_root: &str,
    job_id: &str,
    max_ticks: usize,
) -> Value {
    run_astra_research_tick_until_review_with_config(
        workspace_root,
        registry_root,
        None,
        job_id,
        max_ticks,
    )
}

fn run_astra_research_tick_until_review_with_config(
    workspace_root: &Path,
    registry_root: &str,
    config_root: Option<&str>,
    job_id: &str,
    max_ticks: usize,
) -> Value {
    let mut last_tick = None;
    for _ in 0..max_ticks {
        let current = run_astra_research_tick_json_with_config(
            workspace_root,
            registry_root,
            config_root,
            job_id,
        );
        if !current["data"]["review"].is_null() {
            return current;
        }
        last_tick = Some(current);
    }
    panic!(
        "stage review did not run within {max_ticks} tick(s); last tick: {}",
        last_tick.unwrap_or(Value::Null)
    );
}

fn read_json_lines(path: &str) -> Vec<Value> {
    std::fs::read_to_string(path)
        .expect("jsonl path should be readable")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("line should contain valid json"))
        .collect()
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf8")
}

fn write_skill_manifest(root: &Path, skill_id: &str) {
    write_text_file(
        root.join(skill_id).join("SKILL.md"),
        &format!("# {skill_id}\n\nUse this skill for deterministic {skill_id} coverage.\n"),
    );
}

fn write_plugin_manifest(root: &Path, plugin_id: &str) {
    write_text_file(
        root.join(plugin_id)
            .join(".codex-plugin")
            .join("plugin.json"),
        &json!({
            "id": plugin_id,
            "name": format!("{plugin_id} plugin"),
            "version": "0.1.0",
            "enabled": true,
            "tools": ["example_tool"],
            "hooks": {
                "pre_tool_use": ["hooks/pre.sh"],
                "post_tool_use": ["hooks/post.sh"]
            }
        })
        .to_string(),
    );
    write_text_file(
        root.join(plugin_id).join("hooks").join("pre.sh"),
        "#!/usr/bin/env bash\nprintf 'pre hook'\n",
    );
    write_text_file(
        root.join(plugin_id).join("hooks").join("post.sh"),
        "#!/usr/bin/env bash\nprintf 'post hook'\n",
    );
}

fn write_mcp_manifest(root: &Path, server_id: &str) {
    write_text_file(
        root.join(format!("{server_id}.json")),
        &json!({
            "id": server_id,
            "transport": "stdio",
            "command": "uvx demo-mcp",
            "tool_count": 3,
            "auth_state": "configured",
            "auth_source": "env",
            "timeout_ms": 2500,
            "tools": ["search", "fetch", "summarize"]
        })
        .to_string(),
    );
}

fn write_active_orchestration_run(workspace_root: &Path, run_id: &str) {
    write_text_file(
        workspace_root
            .join(".pmcli")
            .join("orchestrations")
            .join(format!("{run_id}.json")),
        &json!({
            "schema_version": "orchestration.run.v1",
            "run_id": run_id,
            "objective": "Operator visible orchestration",
            "status": "running",
            "start_policy": "approved",
            "automation_mode": "high_autonomy",
            "steps": [
                {
                    "step_id": "s1",
                    "title": "done",
                    "status": "done",
                    "worker": null,
                    "gates": [],
                    "artifacts": [],
                    "continuation_points": []
                },
                {
                    "step_id": "s2",
                    "title": "running",
                    "status": "running",
                    "worker": null,
                    "gates": [],
                    "artifacts": [],
                    "continuation_points": []
                }
            ],
            "control_commands": [],
            "created_at": "1",
            "updated_at": "1"
        })
        .to_string(),
    );
    write_text_file(
        workspace_root
            .join(".pmcli")
            .join("orchestrations")
            .join("active_run"),
        run_id,
    );
}

fn sha256_hex_for_fixture(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn result_to_claim_evidence_binding_ledger_fixture() -> &'static str {
    "## Evidence Binding Ledger\n\n\
| unit_id | unit_kind | statement_or_target | support_status | accepted_evidence_refs | main_agent_decision_refs | canonical_artifact_refs | support_scope | limitations_or_missing_risks |\n\
| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n\
| rtc-map | claim | Bounded claim must be traced to accepted result-to-claim evidence. | partial | accepted_worker_evidence_task:task_result_to_claim_evidence_to_claim_mapping | main_agent_worker_artifact_decision::fixture::task_result_to_claim_evidence_to_claim_mapping | none | Supports claim mapping within fixture scope only. | Upstream paper, experiment, implementation, and literature artifacts may still narrow the claim. |\n\
| rtc-removal | claim deletion or narrowing decision | Unsupported claim text must be narrowed before promotion. | supported | accepted_worker_evidence_task:task_result_to_claim_unsupported_claim_removal | main_agent_worker_artifact_decision::fixture::task_result_to_claim_unsupported_claim_removal | none | Supports deletion or narrowing decision for unsupported claim fragments. | Does not prove the final paper claim. |\n\
| rtc-limits | limitation | Limitations and missing evidence risks remain explicit. | supported | accepted_worker_evidence_task:task_result_to_claim_limitation_extraction | main_agent_worker_artifact_decision::fixture::task_result_to_claim_limitation_extraction | none | Supports limitation language for the result-to-claim stage. | Stronger wording requires accepted upstream artifacts. |"
}

fn write_stage_artifact_adoption_receipt_fixture(
    workspace_root: &Path,
    job_id: &str,
    stage_execution_id: &str,
    stage_id: &str,
    source_agent_id: &str,
    source_task_id: &str,
    source_task_type: &str,
    target_artifact_path: &str,
) {
    let stage_content = std::fs::read_to_string(workspace_root.join(target_artifact_path))
        .expect("fixture stage artifact should exist before writing adoption receipt");
    let content_sha256 = sha256_hex_for_fixture(stage_content.as_bytes());
    let adoption_ref = format!(
        "main_agent_stage_artifact_adoption::fixture::{}",
        source_task_id.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_")
    );
    let receipt_file = format!(
        "{}.json",
        adoption_ref.replace(|ch: char| !ch.is_ascii_alphanumeric(), "_")
    );
    let source_artifact_name = target_artifact_path
        .rsplit('/')
        .next()
        .unwrap_or("stage_artifact.md");
    write_text_file(
        workspace_root
            .join(".pmcli")
            .join("main-agent-board")
            .join("stage-artifact-adoptions")
            .join(format!("{receipt_file}")),
        &json!({
            "adoption_ref": adoption_ref,
            "stage_id": stage_id,
            "stage_execution_id": stage_execution_id,
            "source_agent_id": source_agent_id,
            "source_task_id": source_task_id,
            "source_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
            "source_artifact_path": source_artifact_name,
            "target_artifact_path": target_artifact_path,
            "rationale": "fixture main-agent adopt_stage_artifact action",
            "evidence_refs": ["research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md"],
            "replacement_of_artifact_ids": [],
            "cleanup_required": false,
            "request_review_rerun": true,
            "created_at": "1"
        })
        .to_string(),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join(stage_execution_id)
            .join("stage_artifact_adoptions")
            .join(receipt_file),
        &json!({
            "schema_version": "autonomous_research.stage_artifact_adoption_manifest.v1",
            "adoption_ref": adoption_ref,
            "job_id": job_id,
            "stage_id": stage_id,
            "stage_execution_id": stage_execution_id,
            "source_agent_id": source_agent_id,
            "source_task_id": source_task_id,
            "source_task_type": source_task_type,
            "source_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
            "resolved_source_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
            "source_artifact_path": source_artifact_name,
            "target_artifact_path": target_artifact_path,
            "rationale": "fixture main-agent adopt_stage_artifact receipt",
            "cleanup_required": false,
            "request_review_rerun": true,
            "accepted_worker_output_manifest_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
            "accepted_worker_evidence_refs": ["research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md"],
            "content_sha256": content_sha256,
            "review_body_sha256": content_sha256,
            "created_at": "1",
            "authority_boundary": "main_agent_decides_source_and_target; runtime_validates_provenance_and_reviewability"
        })
        .to_string(),
    );
}

fn write_literature_review_ready_fixture(
    workspace_root: &Path,
    job_id: &str,
    project_id: &str,
    stage_execution_id: &str,
) {
    let current_evidence_set_id = format!("{job_id}::task_literature_stage_artifact_synthesis");
    let stage_dir = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence");
    write_text_file(
        stage_dir.join("index.json"),
        &json!({
            "schema_version": "autonomous_research.accepted_worker_evidence_index.v1",
            "job_id": job_id,
            "project_id": project_id,
            "stage_execution_id": stage_execution_id,
            "stage_id": "literature",
            "generated_at": "1",
            "entries": [{
                "agent_id": "agent_literature_synthesizer",
                "task_id": "task_literature_stage_artifact_synthesis",
                "task_type": "stage artifact synthesis",
                "worker_role": "research_synthesizer",
                "required_output_artifact_type": "literature_matrix",
                "output_manifest_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
                "task_packet_ref": "research/stages/fixture/accepted_worker_evidence/task_packet.json",
                "evidence_refs": ["research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md"],
                "matched_required_fields": ["research question", "citation ledger", "source entries", "claim support boundary"],
                "matched_acceptance_checks": ["citation ledger rows include source ids, canonical titles, refs, verification status, metadata confidence, and claim support boundaries"],
                "matched_quality_signals": ["provider_tool_completed"],
                "quality_profile": {
                    "schema_version": "goal_stage_task_quality_profile.v1",
                    "score": 100,
                    "level": "strong",
                    "strengths": ["fixture accepted worker evidence"],
                    "risks": []
                },
                "semantic_review": {
                    "schema_version": "goal_stage_task_semantic_review_result.v1",
                    "verdict": "pass",
                    "score": 92,
                    "reviewer_role": "literature_semantic_reviewer",
                    "review_model": "fixture",
                    "execution_mode": "fixture",
                    "review_packet_ref": "research/stages/fixture/accepted_worker_evidence/review_packet.json",
                    "review_trace_ref": "research/stages/fixture/accepted_worker_evidence/review_trace.json",
                    "findings": ["fixture accepted worker evidence"],
                    "cleanup_required": false
                },
                "main_agent_acceptance": {
                    "schema_version": "goal_stage_task_acceptance_result.v1",
                    "verdict": "accepted",
                    "score": 96,
                    "reviewer_role": "main_agent_worker_artifact_acceptance",
                    "review_model": "astra-main-agent",
                    "execution_mode": "main_agent_authority_projection",
                    "review_packet_ref": "main_agent_worker_artifact_decision::fixture",
                    "review_trace_ref": "main_agent_worker_artifact_decision::fixture",
                    "findings": ["main agent accepted the synthesis evidence"],
                    "cleanup_required": false
                },
                "acceptance_authority": "main_agent_worker_artifact_decision",
                "main_agent_decision_ref": "main_agent_worker_artifact_decision::task_literature_stage_artifact_synthesis",
                "review_required": true,
                "active_status": "active",
                "current_evidence_set_id": current_evidence_set_id,
                "superseded_by_task_id": null,
                "replacement_of_task_ids": [],
                "decision_reason": null,
                "created_at": "1"
            }]
        })
        .to_string(),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("literature")
            .join("literature_matrix.md"),
        &[
            "# Literature Matrix",
            "",
            "- stage_id: `literature`",
            "- artifact_type: `literature_matrix`",
            "",
            "## Research question",
            "How should a benchmark measure LLM decision quality when useful evidence, irrelevant distractors, mutually inconsistent claims, and adversarial instructions are all present in one chaotic context?",
            "",
            "## Canonical citation ledger",
            "",
            "| Source id | Canonical title | Verification status | Venue/year verification status | Metadata confidence | Accessibility status | Allowed claim boundary |",
            "|---|---|---|---|---|---|---|",
            "| S01 | Lost in the Middle: How Language Models Use Long Contexts | verified | TACL 2024 verified | high | accessible via accepted evidence | Supports position sensitivity in long contexts, not optimal decision quality under mixed chaos. |",
            "| S02 | Large Language Models Can Be Easily Distracted by Irrelevant Context | verified | arXiv 2023 verified | high | accessible via accepted evidence | Supports distraction by irrelevant context, not full conflict resolution. |",
            "| S03 | Universal and Transferable Adversarial Attacks on Aligned Language Models | verified | arXiv 2023 verified | high | accessible via accepted evidence | Supports adversarial context vulnerability, not benchmark scoring. |",
            "| S04 | DecodingTrust: A Comprehensive Assessment of Trustworthiness in GPT Models | verified | arXiv 2023 verified | high | accessible via accepted evidence | Supports broad trustworthiness testing, not a unified chaotic decision benchmark. |",
            "",
            "## Grounded literature matrix rows",
            "",
            "| Matrix row | Method family | Representative evidence rows | What the literature actually measures | Main limitation relative to chaotic-context decision benchmark | Allowed synthesis claim |",
            "|---|---|---|---|---|---|",
            "| R1 | Long-context confusion and distraction | S01, S02 | Whether models can use relevant information when position or distractors make the context hard | It does not combine contradiction, source trust, adversarial instructions, and abstention scoring | Existing literature motivates context-chaos stress tests but does not close the optimal-decision benchmark gap. |",
            "| R2 | Adversarial context attacks and defenses | S03 | Attack or defense success under manipulated prompts | It scores attack success rather than the best decision under mixed reliable and unreliable evidence | Adversarial evidence must be part of the benchmark, but the benchmark must score decision optimality. |",
            "| R3 | Broad trustworthiness and benchmark frameworks | S04 | Multi-axis benchmark reporting and risk categories | It is too broad to isolate chaotic-context decision quality | The reporting framework can be reused, while the task primitive must be new. |",
            "",
            "## Row-level claim-to-evidence mapping",
            "The accepted evidence supports only bounded claims. Provisional, quarantined, or title-mismatched rows are not used as positive support. Missing-source risks are surfaced explicitly so later stages cannot overclaim novelty or completeness.",
            "",
            "## Closest-family ranking",
            "The nearest predecessor families are long-context confusion, irrelevant-context distraction, adversarial context manipulation, and broad trustworthiness benchmarks. The relation to current objective is direct but incomplete because none provides a controlled oracle for optimal decision, abstention, trust ranking, and conflict resolution in one super-chaotic context.",
        ].join("\n"),
    );
    write_stage_artifact_adoption_receipt_fixture(
        workspace_root,
        job_id,
        stage_execution_id,
        "literature",
        "agent_literature_synthesizer",
        "task_literature_stage_artifact_synthesis",
        "stage artifact synthesis",
        &format!("research/stages/{job_id}/literature/literature_matrix.md"),
    );
}

fn write_stage_accepted_worker_evidence_fixture(
    workspace_root: &Path,
    job_id: &str,
    project_id: &str,
    stage_execution_id: &str,
    stage_id: &str,
    artifact_type: &str,
    task_id: &str,
    task_type: &str,
    worker_role: &str,
    required_output_artifact_type: &str,
    output_manifest_ref: &str,
    task_packet_ref: &str,
    evidence_ref: &str,
    matched_required_fields: &[&str],
    matched_acceptance_checks: &[&str],
    quality_level: &str,
    quality_score: u64,
    semantic_review_role: &str,
    semantic_review_verdict: &str,
    semantic_review_score: u64,
    main_agent_accepted: bool,
) {
    let stage_dir = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence");
    let file_key = task_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let evidence_set_id = format!("{stage_execution_id}::{task_id}");
    let main_agent_decision_ref = if main_agent_accepted {
        Some(format!(
            "main_agent_worker_artifact_decision::fixture::{task_id}"
        ))
    } else {
        None
    };
    let acceptance_authority = if main_agent_accepted {
        Some("main_agent_worker_artifact_decision")
    } else {
        Some("goal_acceptance")
    };
    let entry = json!({
        "agent_id": format!("agent_{}_{}", stage_id, task_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")),
        "task_id": task_id,
        "task_type": task_type,
        "worker_role": worker_role,
        "required_output_artifact_type": required_output_artifact_type,
        "output_manifest_ref": output_manifest_ref,
        "task_packet_ref": task_packet_ref,
        "evidence_refs": [evidence_ref],
        "matched_required_fields": matched_required_fields,
        "matched_acceptance_checks": matched_acceptance_checks,
        "matched_quality_signals": ["provider_tool_completed"],
        "quality_profile": {
            "schema_version": "goal_stage_task_quality_profile.v1",
            "score": quality_score,
            "level": quality_level,
            "strengths": ["fixture accepted worker evidence"],
            "risks": []
        },
        "semantic_review": {
            "schema_version": "goal_stage_task_semantic_review_result.v1",
            "verdict": semantic_review_verdict,
            "score": semantic_review_score,
            "reviewer_role": semantic_review_role,
            "review_model": "fixture",
            "execution_mode": "local_acceptance_preflight",
            "review_packet_ref": format!(
                "research/stages/{job_id}/{stage_id}/acceptance_diagnostic_{file_key}.json"
            ),
            "review_trace_ref": format!(
                "research/stages/{job_id}/{stage_id}/acceptance_diagnostic_{file_key}.json"
            ),
            "findings": ["fixture accepted worker evidence"],
            "cleanup_required": false
        },
        "main_agent_acceptance": if main_agent_accepted {
            json!({
                "schema_version": "goal_stage_task_semantic_review_result.v1",
                "verdict": semantic_review_verdict,
                "score": semantic_review_score,
                "reviewer_role": "main_agent_worker_artifact_acceptance",
                "review_model": "astra-main-agent",
                "execution_mode": "main_agent_authority_projection",
                "review_packet_ref": format!("main_agent_worker_artifact_decision::fixture::{task_id}"),
                "review_trace_ref": format!("main_agent_worker_artifact_decision::fixture::{task_id}"),
                "findings": ["fixture main agent acceptance"],
                "cleanup_required": false
            })
        } else {
            Value::Null
        },
        "acceptance_authority": acceptance_authority,
        "main_agent_decision_ref": main_agent_decision_ref,
        "review_required": true,
        "active_status": "active",
        "current_evidence_set_id": evidence_set_id,
        "superseded_by_task_id": null,
        "replacement_of_task_ids": [],
        "decision_reason": null,
        "created_at": "1"
    });
    let index_path = stage_dir.join("index.json");
    let mut entries = std::fs::read_to_string(&index_path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|value| {
            value
                .get("entries")
                .and_then(|entries| entries.as_array())
                .cloned()
        })
        .unwrap_or_default();
    entries.retain(|existing| {
        existing.get("task_id").and_then(|value| value.as_str()) != Some(task_id)
    });
    entries.push(entry);
    write_text_file(
        index_path,
        &json!({
            "schema_version": "autonomous_research.accepted_worker_evidence_index.v1",
            "job_id": job_id,
            "project_id": project_id,
            "stage_execution_id": stage_execution_id,
            "stage_id": stage_id,
            "generated_at": "1",
            "entries": entries
        })
        .to_string(),
    );
    write_text_file(
        stage_dir.join(format!("{file_key}_{artifact_type}.md")),
        &format!(
            "# {artifact_type}\n\naccepted worker evidence for {stage_id}\n\n{task_type}\n{evidence_ref}\nprovider_tool_completed\n"
        ),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join(stage_id)
            .join(format!("review_packet_{file_key}.json")),
        &json!({
            "schema_version": "review_packet.v1",
            "reviewer_role": semantic_review_role,
            "objective": format!("Review accepted worker evidence for {stage_id} / {task_type}."),
            "target_paths": [task_packet_ref, output_manifest_ref, evidence_ref]
        })
        .to_string(),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join(stage_id)
            .join(format!("review_trace_{file_key}.json")),
        &json!({
            "schema_version": "review_trace.v1",
            "review_id": format!("review_{task_id}"),
            "trace_id": format!("trace_{task_id}"),
            "model": "fixture",
            "response_text": format!(
                "verdict: {semantic_review_verdict}\nscore: {semantic_review_score}\n\n- fixture accepted worker evidence\n"
            ),
            "verdict": semantic_review_verdict,
            "timestamp": "1"
        })
        .to_string(),
    );
}

fn write_main_agent_stage_evidence_plan_fixture(
    workspace_root: &Path,
    job_id: &str,
    stage_execution_id: &str,
    stage_id: &str,
    task_types: &[&str],
) {
    let path = workspace_root
        .join(".pmcli")
        .join("main-agent-board")
        .join("stage-evidence-plans")
        .join(format!(
            "latest_{}.json",
            stage_execution_id.replace(['/', '\\'], "_")
        ));
    let evidence_requirements = task_types
        .iter()
        .map(|task_type| {
            let (worker_role, objective, required_output_artifact_type, required_output_fields, acceptance_checks, failure_signals, evidence_standard) =
                match (*task_type, stage_id) {
                    ("acceptance standard setting", _) => (
                        "research_synthesizer",
                        "Draft a concrete stage evidence plan that can be adopted by the main agent.",
                        "claim_table",
                        vec![
                            "task_type",
                            "worker_role",
                            "objective",
                            "required_output_artifact_type",
                            "required_output_fields",
                            "acceptance_checks",
                            "failure_signals",
                            "evidence_standard",
                        ],
                        vec!["fixture stage evidence plan"],
                        vec!["missing evidence refs"],
                        "The adopted plan must enumerate the exact accepted evidence required for stage closure.",
                    ),
                    ("stage artifact synthesis", "result-to-claim") => (
                        "research_synthesizer",
                        "Synthesize a claim table from accepted worker evidence and upstream research artifacts.",
                        "claim_table",
                        vec![
                            "evidence refs",
                            "limitations",
                            "next action",
                            "claim support boundary",
                        ],
                        vec!["fixture stage evidence plan"],
                        vec!["missing evidence refs"],
                        "A claim table must cite accepted worker evidence and reject unsupported claims.",
                    ),
                    ("stage artifact synthesis", "research-review") => (
                        "research_synthesizer",
                        "Synthesize a hard review packet from accepted worker evidence and upstream paper artifacts.",
                        "hard_review_packet",
                        vec![
                            "PDF and TeX refs",
                            "claim table ref",
                            "literature matrix ref",
                            "experiment report ref",
                            "implementation manifest ref",
                            "reproducibility audit",
                            "novelty audit",
                            "verdict, score, findings, repair routes",
                        ],
                        vec!["fixture stage evidence plan"],
                        vec!["missing evidence refs"],
                        "A hard review packet must verify the complete upstream paper and stage bundle.",
                    ),
                    _ => (
                        "goal_worker",
                        "Produce explicit, auditable stage-local evidence accepted by the main agent.",
                        "stage_artifact",
                        vec![
                            "task_type",
                            "worker_role",
                            "objective",
                            "required_output_artifact_type",
                            "required_output_fields",
                            "acceptance_checks",
                            "failure_signals",
                            "evidence_standard",
                        ],
                        vec!["fixture stage evidence plan"],
                        vec!["missing evidence refs"],
                        "Accepted worker evidence must be specific enough for the main agent to gate stage coverage.",
                    ),
                };
            json!({
                "task_type": task_type,
                "worker_role": worker_role,
                "objective": objective,
                "required_output_artifact_type": required_output_artifact_type,
                "required_output_fields": required_output_fields,
                "acceptance_checks": acceptance_checks,
                "failure_signals": failure_signals,
                "evidence_standard": evidence_standard,
            })
        })
        .collect::<Vec<_>>();
    write_text_file(
        path,
        &json!({
            "schema_version": "main_agent_stage_evidence_plan.v1",
            "plan_id": format!("fixture_stage_evidence_plan_{stage_execution_id}"),
            "plan_ref": format!("main_agent_stage_evidence_plan::fixture_stage_evidence_plan_{stage_execution_id}"),
            "stage_id": stage_id,
            "stage_execution_id": stage_execution_id,
            "rationale": "fixture main-agent adopted evidence plan",
            "plan_source_refs": ["accepted_worker_evidence:acceptance standard setting"],
            "evidence_requirements": evidence_requirements,
            "worker_task_requirements": evidence_requirements,
            "project_id": "fixture",
            "job_id": job_id,
            "created_at": "1"
        })
        .to_string(),
    );
}

fn write_result_to_claim_review_ready_fixture(
    workspace_root: &Path,
    job_id: &str,
    project_id: &str,
    stage_execution_id: &str,
) {
    write_main_agent_stage_evidence_plan_fixture(
        workspace_root,
        job_id,
        stage_execution_id,
        "result-to-claim",
        &[
            "acceptance standard setting",
            "evidence-to-claim mapping",
            "unsupported-claim removal",
            "limitation extraction",
        ],
    );
    let current_evidence_set_id =
        format!("{job_id}::task_result_to_claim_stage_artifact_synthesis");
    let stage_dir = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence");
    write_text_file(
        stage_dir.join("index.json"),
        &json!({
            "schema_version": "autonomous_research.accepted_worker_evidence_index.v1",
            "job_id": job_id,
            "project_id": project_id,
            "stage_execution_id": stage_execution_id,
            "stage_id": "result-to-claim",
            "generated_at": "1",
            "entries": [{
                "agent_id": "agent_result_to_claim_synthesizer",
                "task_id": "task_result_to_claim_stage_artifact_synthesis",
                "task_type": "stage artifact synthesis",
                "worker_role": "research_synthesizer",
                "required_output_artifact_type": "claim_table",
                "output_manifest_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
                "task_packet_ref": "research/stages/fixture/accepted_worker_evidence/task_packet.json",
                "evidence_refs": ["research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md"],
                "matched_required_fields": ["evidence refs", "limitations", "next action"],
                "matched_acceptance_checks": ["fixture accepted worker evidence"],
                "matched_quality_signals": ["provider_tool_completed"],
                "quality_profile": {
                    "schema_version": "goal_stage_task_quality_profile.v1",
                    "score": 100,
                    "level": "strong",
                    "strengths": ["fixture accepted worker evidence"],
                    "risks": []
                },
                "semantic_review": {
                    "schema_version": "goal_stage_task_semantic_review_result.v1",
                    "verdict": "pass",
                    "score": 92,
                    "reviewer_role": "claim_auditor",
                    "review_model": "fixture",
                    "execution_mode": "fixture",
                    "review_packet_ref": "research/stages/fixture/accepted_worker_evidence/review_packet.json",
                    "review_trace_ref": "research/stages/fixture/accepted_worker_evidence/review_trace.json",
                    "findings": ["fixture accepted worker evidence"],
                    "cleanup_required": false
                },
                "main_agent_acceptance": {
                    "schema_version": "goal_stage_task_semantic_review_result.v1",
                    "verdict": "pass",
                    "score": 92,
                    "reviewer_role": "main_agent_worker_artifact_acceptance",
                    "review_model": "astra-main-agent",
                    "execution_mode": "main_agent_authority_projection",
                    "review_packet_ref": "main_agent_worker_artifact_decision::fixture",
                    "review_trace_ref": "main_agent_worker_artifact_decision::fixture",
                    "findings": ["fixture main agent acceptance"],
                    "cleanup_required": false
                },
                "acceptance_authority": "main_agent_worker_artifact_decision",
                "main_agent_decision_ref": "main_agent_worker_artifact_decision::task_result_to_claim_stage_artifact_synthesis",
                "review_required": true,
                "active_status": "active",
                "current_evidence_set_id": current_evidence_set_id,
                "superseded_by_task_id": null,
                "replacement_of_task_ids": [],
                "decision_reason": null,
                "created_at": "1"
            }]
        })
        .to_string(),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("result-to-claim")
            .join("claim_table.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: {}\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/result-to-claim/claim_table.md\n\
               title: Fixture Claim Table\n\
               doc_type: report\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Fixture claim table for result-to-claim review.\n\
               key_claims:\n\
                 - Claim table is evidence-grounded.\n\
               decisions:\n\
                 - Use fixture claim table for review gate tests.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\n\
               next_actions:\n\
                 - Run the result-to-claim review gate.\n\
               non_goals:\n\
               - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Claim Table\n\nstage: result-to-claim\nartifact_type: claim_table\nclaim: Astra can enforce stage-gated autonomous research while preserving stage boundaries, explicit evidence routing, and reviewable provenance.\n\n## Evidence refs\n\n- research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\n- research/stages/{job_id}/result-to-claim/stage_plan.md\n- research/stages/{job_id}/result-to-claim/stage_acceptance_rubric.md\n- research/stages/{job_id}/literature/literature_matrix.md\n\n## Support level\n\npartial, because the claim is only supported for the current fixture scope and still depends on upstream literature, experiment, implementation, and report artifacts.\n\n## Scope and assumptions\n\nThis fixture is limited to the current job and the accepted worker evidence bundle. It assumes the main agent will narrow unsupported claims instead of promoting them into the canonical claim table.\n\n## Limitations\n\nThe artifact does not prove the whole research program. It only shows how accepted evidence can be mapped to a bounded claim, and it leaves room for missing upstream artifacts, unverified source rows, and repair routing.\n\n## Required extra evidence if partial\n\nliterature matrix, experiment plan, implementation manifest, experiment report.\n\n## Deletion or narrowing decision\n\nNarrow claim until all upstream evidence is accepted and the claim can be traced to concrete evidence refs without ambiguity.\n\n{evidence_binding_ledger}\n\n## Risk\n\nunsupported claim stronger than evidence.\n",
            "research.stages.arj_fixture.result.to.claim.claim.table",
            evidence_binding_ledger = result_to_claim_evidence_binding_ledger_fixture()
        ),
    );
    write_stage_artifact_adoption_receipt_fixture(
        workspace_root,
        job_id,
        stage_execution_id,
        "result-to-claim",
        "agent_result_to_claim_synthesizer",
        "task_result_to_claim_stage_artifact_synthesis",
        "stage artifact synthesis",
        &format!("research/stages/{job_id}/result-to-claim/claim_table.md"),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("result-to-claim")
            .join("stage_plan.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: {}\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/result-to-claim/stage_plan.md\n\
               title: Fixture Result To Claim Stage Plan\n\
               doc_type: plan\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Result-to-claim stage plan for review gate tests.\n\
               key_claims:\n\
                 - Result-to-claim requires accepted claim table evidence.\n\
               decisions:\n\
                 - Use accepted worker evidence and upstream research artifacts.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\n\
               next_actions:\n\
                 - Run the result-to-claim review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Stage Plan\n\n## Stage Identity\n\n- stage_id: `result-to-claim`\n- stage_class: `result_to_claim`\n- artifact_type: `claim_table`\n- artifact_path: `research/stages/arj_fixture/result-to-claim/claim_table.md`\n\n## Main Agent Responsibility\n\nThe main agent must use accepted worker evidence and upstream research artifacts to synthesize `claim_table`.\n",
            "research.stages.arj_fixture.result.to.claim.stage.plan"
        ),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("result-to-claim")
            .join("stage_acceptance_rubric.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: {}\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/result-to-claim/stage_acceptance_rubric.md\n\
               title: Fixture Result To Claim Rubric\n\
               doc_type: rubric\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Result-to-claim acceptance rubric for review gate tests.\n\
               key_claims:\n\
                 - Claim table must cite accepted worker evidence.\n\
               decisions:\n\
                 - Reject unsupported claims.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\n\
               next_actions:\n\
                 - Run the result-to-claim review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Stage Acceptance Rubric\n\n## Expert-Level Acceptance Target\n\n- claim table cites accepted worker evidence\n- unsupported claim is narrowed or rejected\n- limitations are explicit\n- upstream artifacts are auditable\n",
            "research.stages.arj_fixture.result.to.claim.stage.acceptance.rubric"
        ),
    );
}

fn write_research_review_ready_fixture(
    workspace_root: &Path,
    job_id: &str,
    project_id: &str,
    stage_execution_id: &str,
) {
    write_main_agent_stage_evidence_plan_fixture(
        workspace_root,
        job_id,
        stage_execution_id,
        "research-review",
        &[
            "acceptance standard setting",
            "adversarial review",
            "reproducibility review",
            "claim-evidence audit",
        ],
    );
    let current_evidence_set_id =
        format!("{job_id}::task_research_review_stage_artifact_synthesis");
    let stage_dir = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence");
    write_text_file(
        stage_dir.join("index.json"),
        &json!({
            "schema_version": "autonomous_research.accepted_worker_evidence_index.v1",
            "job_id": job_id,
            "project_id": project_id,
            "stage_execution_id": stage_execution_id,
            "stage_id": "research-review",
            "generated_at": "1",
            "entries": [{
                "agent_id": "agent_research_review_synthesizer",
                "task_id": "task_research_review_stage_artifact_synthesis",
                "task_type": "stage artifact synthesis",
                "worker_role": "research_synthesizer",
                "required_output_artifact_type": "hard_review_packet",
                "output_manifest_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
                "task_packet_ref": "research/stages/fixture/accepted_worker_evidence/task_packet.json",
                "evidence_refs": ["research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md"],
                "matched_required_fields": ["PDF and TeX refs", "claim table ref", "literature matrix ref", "experiment report ref", "implementation manifest ref", "reproducibility audit", "novelty audit", "verdict, score, findings, repair routes"],
                "matched_acceptance_checks": ["fixture accepted worker evidence"],
                "matched_quality_signals": ["provider_tool_completed"],
                "quality_profile": {
                    "schema_version": "goal_stage_task_quality_profile.v1",
                    "score": 100,
                    "level": "strong",
                    "strengths": ["fixture accepted worker evidence"],
                    "risks": []
                },
                "semantic_review": {
                    "schema_version": "goal_stage_task_semantic_review_result.v1",
                    "verdict": "pass",
                    "score": 92,
                    "reviewer_role": "hard_reviewer",
                    "review_model": "fixture",
                    "execution_mode": "fixture",
                    "review_packet_ref": "research/stages/fixture/accepted_worker_evidence/review_packet.json",
                    "review_trace_ref": "research/stages/fixture/accepted_worker_evidence/review_trace.json",
                    "findings": ["fixture accepted worker evidence"],
                    "cleanup_required": false
                },
                "main_agent_acceptance": {
                    "schema_version": "goal_stage_task_semantic_review_result.v1",
                    "verdict": "pass",
                    "score": 92,
                    "reviewer_role": "main_agent_worker_artifact_acceptance",
                    "review_model": "astra-main-agent",
                    "execution_mode": "main_agent_authority_projection",
                    "review_packet_ref": "main_agent_worker_artifact_decision::fixture",
                    "review_trace_ref": "main_agent_worker_artifact_decision::fixture",
                    "findings": ["fixture main agent acceptance"],
                    "cleanup_required": false
                },
                "acceptance_authority": "main_agent_worker_artifact_decision",
                "main_agent_decision_ref": "main_agent_worker_artifact_decision::task_research_review_stage_artifact_synthesis",
                "review_required": true,
                "active_status": "active",
                "current_evidence_set_id": current_evidence_set_id,
                "superseded_by_task_id": null,
                "replacement_of_task_ids": [],
                "decision_reason": null,
                "created_at": "1"
            }]
        })
        .to_string(),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("research-review")
            .join("hard_review_packet.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: {}\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/research-review/hard_review_packet.md\n\
               title: Fixture Hard Review Packet\n\
               doc_type: report\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Fixture hard review packet for research-review gate tests.\n\
               key_claims:\n\
                 - Hard review packet inspects PDF, TeX, claims, and upstream evidence.\n\
               decisions:\n\
                 - Require all upstream artifacts before pass.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - papers/{job_id}/main.tex\n\
                 - papers/{job_id}/compiled_pdf_bundle.md\n\
                 - research/stages/{job_id}/result-to-claim/claim_table.md\n\
               next_actions:\n\
                 - Run the research-review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# hard_review_packet\n\nPDF and TeX refs\n\n- papers/{job_id}/main.tex\n- papers/{job_id}/compiled_pdf_bundle.md\n\nclaim table ref\n\n- research/stages/{job_id}/result-to-claim/claim_table.md\n\nliterature matrix ref\n\n- research/stages/{job_id}/literature/literature_matrix.md\n\nexperiment report ref\n\n- research/stages/{job_id}/monitor/experiment_report.md\n\nimplementation manifest ref\n\n- research/stages/{job_id}/implement-solution/implementation_manifest.md\n\nreproducibility audit\n\nThe final bundle must be auditable, and the hard review must fail if any cited artifact is missing, stale, or contradictory. The reviewer should inspect the paper source, compiled PDF bundle, claim table, experiment report, and implementation manifest before pass.\n\nnovelty audit\n\nThe review should confirm that the final paper does not overclaim novelty, does not ignore accepted evidence boundaries, and does not hide missing upstream artifacts behind a polished summary.\n\nverdict, score, findings, repair routes\n\nThis fixture deliberately keeps the final bundle incomplete so the review gate can return a fail verdict, a concrete score, and explicit repair routes. compiled PDF missing.\n",
            "research.stages.arj_fixture.research.review.hard.review.packet"
        ),
    );
    write_stage_artifact_adoption_receipt_fixture(
        workspace_root,
        job_id,
        stage_execution_id,
        "research-review",
        "agent_research_review_synthesizer",
        "task_research_review_stage_artifact_synthesis",
        "stage artifact synthesis",
        &format!("research/stages/{job_id}/research-review/hard_review_packet.md"),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("novelty")
            .join("novelty_report.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: research.stages.{job_id}.novelty.novelty.report\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/novelty/novelty_report.md\n\
               title: Fixture Novelty Report\n\
               doc_type: report\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Fixture novelty report for research-review gate tests.\n\
               key_claims:\n\
                 - Novelty report is evidence-grounded.\n\
               decisions:\n\
                 - Use fixture novelty report for review tests.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\n\
               next_actions:\n\
                 - Run the research-review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Novelty Report\n\nstage: novelty\nartifact_type: novelty_report\naccepted worker evidence: fixture novelty report\n",
        ),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("research-review")
            .join("stage_plan.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: {}\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/research-review/stage_plan.md\n\
               title: Fixture Research Review Stage Plan\n\
               doc_type: plan\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Research review stage plan for gate tests.\n\
               key_claims:\n\
                 - Research review requires the full paper bundle and upstream evidence.\n\
               decisions:\n\
                 - Review the final bundle only after upstream artifacts exist.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - papers/{job_id}/main.tex\n\
                 - papers/{job_id}/compiled_pdf_bundle.md\n\
                 - research/stages/{job_id}/result-to-claim/claim_table.md\n\
               next_actions:\n\
                 - Run the research-review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Stage Plan\n\n## Stage Identity\n\n- stage_id: `research-review`\n- stage_class: `publish`\n- artifact_type: `hard_review_packet`\n- artifact_path: `research/stages/arj_fixture/research-review/hard_review_packet.md`\n\n## Upstream Evidence\n\n- papers/{job_id}/main.tex\n- papers/{job_id}/compiled_pdf_bundle.md\n- research/stages/{job_id}/result-to-claim/claim_table.md\n- research/stages/{job_id}/literature/literature_matrix.md\n- research/stages/{job_id}/monitor/experiment_report.md\n- research/stages/{job_id}/implement-solution/implementation_manifest.md\n\n## Main Agent Responsibility\n\nThe main agent must use accepted worker evidence and upstream paper evidence to synthesize `hard_review_packet`.\n",
            "research.stages.arj_fixture.research.review.stage.plan"
        ),
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("research-review")
            .join("stage_acceptance_rubric.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: {}\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/research-review/stage_acceptance_rubric.md\n\
               title: Fixture Research Review Rubric\n\
               doc_type: rubric\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Research review rubric for gate tests.\n\
               key_claims:\n\
                 - Review pass requires the full paper bundle and citations.\n\
               decisions:\n\
                 - Fail missing PDF or TeX.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - papers/{job_id}/main.tex\n\
                 - papers/{job_id}/compiled_pdf_bundle.md\n\
               next_actions:\n\
                 - Run the research-review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Stage Acceptance Rubric\n\n## Expert-Level Acceptance Target\n\n- PDF and TeX are both present\n- claim table, literature matrix, experiment report, and implementation manifest are cited\n- reproducibility and novelty audits are explicit\n- verdict, score, findings, and repair routes are present\n",
            "research.stages.arj_fixture.research.review.stage.acceptance.rubric"
        ),
    );
    write_text_file(
        workspace_root.join("papers").join(job_id).join("main.tex"),
        "\\documentclass{article}\n\\begin{document}\nFixture research review paper.\n\\end{document}\n",
    );
    write_text_file(
        workspace_root
            .join("papers")
            .join(job_id)
            .join("paper_tex_bundle.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: research.stages.{job_id}.paper.write.paper.tex.bundle\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: papers/{job_id}/paper_tex_bundle.md\n\
               title: Fixture Paper TeX Bundle\n\
               doc_type: report\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Fixture TeX bundle for research-review gate tests.\n\
               key_claims:\n\
                 - TeX source exists for the final paper bundle.\n\
               decisions:\n\
                 - Use fixture TeX bundle for review tests.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - papers/{job_id}/main.tex\n\
               next_actions:\n\
                 - Run the research-review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Paper TeX Bundle\n\nstage: paper-write\nartifact_type: paper_tex_bundle\naccepted worker evidence: fixture TeX source\n",
        ),
    );
    write_text_file(
        workspace_root
            .join("papers")
            .join(job_id)
            .join("compiled_pdf_bundle.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: research.stages.{job_id}.paper.compile.compiled.pdf.bundle\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: papers/{job_id}/compiled_pdf_bundle.md\n\
               title: Fixture Compiled PDF Bundle\n\
               doc_type: report\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Fixture compiled PDF bundle for research-review gate tests.\n\
               key_claims:\n\
                 - PDF bundle exists for the final paper.\n\
               decisions:\n\
                 - Use fixture PDF bundle for review tests.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - papers/{job_id}/build/main.pdf\n\
               next_actions:\n\
                 - Run the research-review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# compiled_pdf_bundle\n\nPDF path: papers/{job_id}/build/main.pdf\n\nBuild log: successful\n\nPDF validation result: pass\n"
        ),
    );
    write_text_file(
        workspace_root
            .join("papers")
            .join(job_id)
            .join("build")
            .join("main.pdf"),
        "%PDF-1.4\n%fixture\n",
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("literature")
            .join("literature_matrix.md"),
        "# Literature Matrix\n\nstage: literature\nartifact_type: literature_matrix\naccepted worker evidence: fixture literature matrix\n",
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("monitor")
            .join("experiment_report.md"),
        "# Experiment Report\n\nstage: monitor\nartifact_type: experiment_report\naccepted worker evidence: fixture experiment report\n",
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("implement-solution")
            .join("implementation_manifest.md"),
        "# Implementation Manifest\n\nstage: implement-solution\nartifact_type: implementation_manifest\naccepted worker evidence: fixture implementation manifest\n",
    );
    write_text_file(
        workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join("result-to-claim")
            .join("claim_table.md"),
        &format!(
            "---\n\
             doc_frame:\n\
               doc_id: research.stages.{job_id}.result.to.claim.claim.table\n\
               schema_version: autonomous_research_doc_frame.v1\n\
               source_path: research/stages/{job_id}/result-to-claim/claim_table.md\n\
               title: Fixture Claim Table\n\
               doc_type: report\n\
               lifecycle: active\n\
               scope: project\n\
               summary: Fixture claim table for result-to-claim review.\n\
               key_claims:\n\
                 - Claim table is evidence-grounded.\n\
               decisions:\n\
                 - Use fixture claim table for review gate tests.\n\
               interfaces:\n\
                 - autonomous_research_stage_contract\n\
               evidence_refs:\n\
                 - research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\n\
               next_actions:\n\
                 - Run the result-to-claim review gate.\n\
               non_goals:\n\
                 - Do not treat this fixture as production state.\n\
               generated_by: astra_autonomous_research_runtime\n\
               updated_at: 1\n\
             ---\n\
             \n# Claim Table\n\nstage: result-to-claim\nartifact_type: claim_table\nclaim: Astra can enforce stage-gated autonomous research.\naccepted worker evidence: research/stages/{job_id}/{stage_execution_id}/accepted_worker_evidence/index.json\nlimitations: unsupported claim stronger than evidence.\nscope and assumptions: fixture scope only.\nrequired extra evidence if partial: literature matrix, experiment plan, implementation manifest, experiment report.\ndeletion/narrowing decision: narrow claim until all upstream evidence is accepted.\n\n{evidence_binding_ledger}\n\nrisk: unsupported claim stronger than evidence.\n",
            evidence_binding_ledger = result_to_claim_evidence_binding_ledger_fixture()
        ),
    );
}

#[test]
fn run_cli_status_and_pause_operate_on_active_orchestration_run() {
    let workspace_root = init_git_workspace("run_cli_status_pause");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    write_active_orchestration_run(&workspace_root, "run_cli_001");

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["run", "status", "--json"])
        .output()
        .expect("run status should execute");
    assert!(
        status_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status_json = output_json(&status_output);
    assert_eq!(status_json["command"], "run status");
    assert_eq!(status_json["data"]["active_run"]["run_id"], "run_cli_001");
    assert_eq!(status_json["data"]["progress"]["percent"], 50);

    let pause_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["run", "pause", "--json"])
        .output()
        .expect("run pause should execute");
    assert!(
        pause_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pause_output.stderr)
    );
    let pause_json = output_json(&pause_output);
    assert_eq!(pause_json["command"], "run pause");
    assert_eq!(pause_json["data"]["run"]["status"], "paused");
    assert_eq!(pause_json["data"]["command"]["command"], "pause");
    assert_eq!(
        pause_json["data"]["recovery_context"]["recommended_actions"][0],
        "continue"
    );
}

fn init_real_git_workspace(label: &str) -> std::path::PathBuf {
    let root = unique_temp_dir(label);
    let init = Command::new("git")
        .current_dir(&root)
        .args(["init", "-q"])
        .status()
        .expect("git init should execute");
    assert!(init.success());
    let config_name = Command::new("git")
        .current_dir(&root)
        .args(["config", "user.name", "Research CLI Test"])
        .status()
        .expect("git config user.name should execute");
    assert!(config_name.success());
    let config_email = Command::new("git")
        .current_dir(&root)
        .args(["config", "user.email", "research-cli@example.test"])
        .status()
        .expect("git config user.email should execute");
    assert!(config_email.success());
    write_text_file(root.join("README.md"), "# test workspace\n");
    let add = Command::new("git")
        .current_dir(&root)
        .args(["add", "README.md"])
        .status()
        .expect("git add should execute");
    assert!(add.success());
    let commit = Command::new("git")
        .current_dir(&root)
        .args(["commit", "-q", "-m", "initial"])
        .status()
        .expect("git commit should execute");
    assert!(commit.success());
    root
}

fn read_active_research_status(workspace_root: &Path, registry_root: &str) -> Value {
    run_cli_json(
        workspace_root,
        registry_root,
        &["research", "status", "--json"],
    )
}

fn start_research_run_json(
    workspace_root: &Path,
    registry_root: &str,
    prompt: &str,
    mode_args: &[&str],
) -> Value {
    let mut args = vec!["research", "run", "--prompt", prompt];
    args.extend_from_slice(mode_args);
    args.extend_from_slice(&[
        "--background",
        "--max-runtime-ms",
        "1000",
        "--max-ticks",
        "1",
        "--interval-ms",
        "1",
        "--json",
    ]);
    let output = Command::new(astra_bin())
        .current_dir(workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", registry_root)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .env_remove("RESEARCH_CLI_LIVE_PROVIDER")
        .args(args)
        .output()
        .expect("astra research run should execute");
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    output_json(&output)
}

#[test]
fn astra_research_run_accepts_three_automation_modes_and_starts_literature_stage() {
    let cases = [
        (
            "research_run_mode_human",
            &["--automation-mode", "human_in_the_loop"][..],
            "human_in_the_loop",
        ),
        (
            "research_run_mode_high",
            &["--high-autonomy"][..],
            "high_autonomy",
        ),
        (
            "research_run_mode_full",
            &["--automation-mode", "full_auto"][..],
            "full_auto",
        ),
    ];

    for (label, mode_args, expected_mode) in cases {
        let workspace_root = init_real_git_workspace(label);
        let registry_root = registry_root_env();
        let prompt = format!("Study staged autonomous research entry mode {expected_mode}");
        let json = start_research_run_json(&workspace_root, &registry_root, &prompt, mode_args);
        assert_eq!(json["command"], "research run");
        assert_eq!(json["data"]["background"], true);
        assert_eq!(json["data"]["job"]["automation_mode"], expected_mode);
        assert_eq!(
            json["data"]["job"]["stage_task_semantic_review_mode"],
            "provider"
        );
        assert_eq!(json["data"]["job"]["status"], "running");

        let status = read_active_research_status(&workspace_root, &registry_root);
        assert_eq!(
            status["data"]["active_stage_execution"]["stage_id"], "literature",
            "all research prompts enter literature as the first real stage"
        );
        assert_eq!(
            status["data"]["active_stage_execution"]["stage_class"],
            "survey"
        );
    }
}

#[test]
fn astra_research_run_full_auto_uses_durable_job_supervisor_without_smoke_pipeline() {
    let workspace_root = init_real_git_workspace("astra_research_run_full_auto");
    let registry_root = registry_root_env();
    let prompt = "Find a lightweight benchmark idea for autonomous research CLI evaluation";

    let output = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .env_remove("RESEARCH_CLI_LIVE_PROVIDER")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--full-auto",
            "--max-runtime-ms",
            "1",
            "--max-ticks",
            "1",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run should execute");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["command"], "research run");
    assert!(json["project_id"].is_string());
    assert_eq!(
        json["data"]["schema_version"],
        "autonomous_research_job_run_result.v1"
    );
    assert_eq!(json["data"]["background"], false);
    assert!(json["data"]["job_id"]
        .as_str()
        .expect("job id")
        .starts_with("arj_"));
    assert!(json["data"]["job_state_path"].is_string());
    assert_eq!(
        json["data"]["job"]["schema_version"],
        "autonomous_research_job.v1"
    );
    assert_eq!(json["data"]["job"]["prompt"], prompt);
    assert_eq!(json["data"]["job"]["automation_mode"], "full_auto");
    assert_eq!(
        json["data"]["job"]["stage_task_semantic_review_mode"],
        "provider"
    );
    assert_eq!(
        read_active_research_status(&workspace_root, &registry_root)["data"]
            ["active_stage_execution"]["stage_id"],
        "literature"
    );
    assert_eq!(json["data"]["job"]["max_runtime_ms"], 1);
    assert_eq!(json["data"]["job"]["max_ticks"], 1);
    assert!(json["data"]["job"]["artifact_refs"]
        .as_array()
        .expect("job artifact refs")
        .iter()
        .any(|entry| entry
            .as_str()
            .unwrap_or_default()
            .contains("auto_research_job:")));
    assert!(json["data"].get("pipeline_stage_count").is_none());
    assert!(json["data"].get("pipeline_manifest_path").is_none());
    assert!(json["data"].get("pipeline_stages").is_none());
    assert!(json["data"].get("live_synthesis").is_none());
    assert!(!workspace_root.join("research/pipeline").exists());
    assert!(!workspace_root.join("experiments/astra_pipeline").exists());

    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    assert!(events.contains("\"event_name\":\"autonomous_research_job\""));
    assert!(!events.contains("\"event_name\":\"autonomous_research_run\""));
    assert!(!events.contains("\"event_name\":\"autonomous_research_pipeline_stage\""));

    let git_status = Command::new("git")
        .current_dir(&workspace_root)
        .args(["status", "--porcelain"])
        .output()
        .expect("git status should execute");
    assert!(git_status.status.success());
    let status_text = String::from_utf8(git_status.stdout).expect("git status should be utf8");
    assert!(
        status_text.contains("?? .pmcli/"),
        "Astra runtime state should remain project-local, status: {status_text}"
    );
}

#[test]
fn astra_research_run_local_stage_task_semantic_review_is_explicit_opt_in() {
    let workspace_root = init_real_git_workspace("astra_research_run_local_semantic_review");
    let registry_root = registry_root_env();
    let prompt = "Study explicit offline stage-task semantic review diagnostics";

    let json = start_research_run_json(
        &workspace_root,
        &registry_root,
        prompt,
        &[
            "--automation-mode",
            "full_auto",
            "--stage-task-semantic-review",
            "local",
        ],
    );

    assert_eq!(
        json["data"]["job"]["stage_task_semantic_review_mode"],
        "local"
    );
    let policy_path = workspace_root
        .join(".pmcli")
        .join("goals")
        .join("stage_task_semantic_review_policy.json");
    let policy: Value = serde_json::from_str(
        &std::fs::read_to_string(policy_path).expect("semantic review policy should read"),
    )
    .expect("semantic review policy should be json");
    assert_eq!(policy["mode"], "local");
}

#[test]
fn astra_research_run_persists_provider_stage_task_semantic_review_policy() {
    let workspace_root = init_real_git_workspace("astra_research_run_provider_semantic_review");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("astra_research_run_provider_semantic_review_global");
    let prompt = "Study provider-backed stage-task semantic review policy persistence";
    write_text_file(
        global_root.join("settings.json"),
        r#"{
  "provider_profiles": {
    "sjtu-glm": {
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:65535/v1",
      "auth_env_var": "SJTU_GLM_API_KEY",
      "default_model": "glm-5.1",
      "supported_models": ["glm-5.1"]
    }
  },
  "default_provider": "sjtu-glm",
  "default_model": "glm-5.1"
}"#,
    );

    let output = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("SJTU_GLM_API_KEY", "sk-test")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("RESEARCH_CLI_LIVE_PROVIDER")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--full-auto",
            "--stage-task-semantic-review",
            "provider",
            "--max-runtime-ms",
            "1",
            "--max-ticks",
            "1",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run should execute");
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let json = output_json(&output);
    let job_id = json["data"]["job_id"].as_str().expect("job id");
    assert_eq!(
        json["data"]["job"]["stage_task_semantic_review_mode"],
        "provider"
    );

    let job_state_path = json["data"]["job_state_path"]
        .as_str()
        .expect("job state path");
    let job_state: Value = serde_json::from_str(
        &std::fs::read_to_string(job_state_path).expect("job state should read"),
    )
    .expect("job state should be json");
    assert_eq!(job_state["stage_task_semantic_review_mode"], "provider");

    let policy_path = workspace_root
        .join(".pmcli")
        .join("goals")
        .join("stage_task_semantic_review_policy.json");
    let policy: Value = serde_json::from_str(
        &std::fs::read_to_string(policy_path).expect("semantic review policy should read"),
    )
    .expect("semantic review policy should be json");
    assert_eq!(
        policy["schema_version"],
        "goal_stage_task_semantic_review_policy.v1"
    );
    assert_eq!(policy["mode"], "provider");
    assert_eq!(policy["provider"], "sjtu-glm");
    assert_eq!(policy["model"], "glm-5.1");
    assert!(
        matches!(
            policy["source"].as_str(),
            Some("autonomous_research_job") | Some("autonomous_research_job_tick")
        ),
        "policy source should come from job creation or tick resume: {policy}"
    );
    assert_eq!(policy["job_id"], job_id);
}

#[test]
fn astra_research_run_autoregisters_unregistered_git_cwd_before_current_pointer() {
    let old_workspace = init_real_git_workspace("astra_old_current_pointer_workspace");
    let new_workspace = init_real_git_workspace("astra_new_unregistered_research_workspace");
    let registry_root = registry_root_env();
    let prompt = "Autonomously research from a newly opened git workspace";

    let old_init = Command::new(astra_bin())
        .current_dir(&old_workspace)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("old project init should execute");
    assert!(
        old_init.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&old_init.stderr),
        String::from_utf8_lossy(&old_init.stdout)
    );

    let start = Command::new(astra_bin())
        .current_dir(&new_workspace)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .env_remove("RESEARCH_CLI_LIVE_PROVIDER")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--full-auto",
            "--background",
            "--max-runtime-ms",
            "1",
            "--max-ticks",
            "1",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("research run should execute in new workspace");
    assert!(
        start.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&start.stderr),
        String::from_utf8_lossy(&start.stdout)
    );
    let json = output_json(&start);
    let job_id = json["data"]["job_id"].as_str().expect("job id");
    let job_state_path = json["data"]["job"]["job_state_path"]
        .as_str()
        .expect("job state path");
    assert!(
        job_state_path.starts_with(&format!("{registry_root}/projects/")),
        "job state should live under global state root, got {job_state_path}"
    );
    assert!(
        job_state_path.ends_with(&format!("/research/jobs/{job_id}/job.json")),
        "job state path should keep canonical research job layout, got {job_state_path}"
    );
    assert!(
        !new_workspace.join(".pmcli").exists(),
        "new workspace should not receive runtime state"
    );
    assert!(
        !old_workspace
            .join(".pmcli")
            .join("research")
            .join("jobs")
            .exists(),
        "old current pointer workspace must not receive research runtime state"
    );

    let _ = Command::new(astra_bin())
        .current_dir(&new_workspace)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "jobs", "stop", job_id, "--json"])
        .output();
}

#[test]
fn astra_research_run_creates_durable_job_and_tick_repairs_after_review_fail() {
    let workspace_root = init_real_git_workspace("astra_research_job_supervisor");
    let registry_root = registry_root_env();
    let config_root = unique_temp_dir("astra_research_job_supervisor_config")
        .display()
        .to_string();
    let prompt = "Autonomously research and write a paper about a tiny CLI research benchmark";

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut requests = Vec::new();
        loop {
            if stop_rx.try_recv().is_ok() {
                return requests;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"placeholder literature draft"}}],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    requests.push(String::from_utf8_lossy(&request).to_string());
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return requests;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    write_text_file(
        std::path::Path::new(&config_root).join("settings.json"),
        &format!(
            r#"{{
  "provider_profiles": {{
    "openai": {{
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:{port}/v1",
      "auth_env_var": "OPENAI_API_KEY",
      "default_model": "gpt-4.1-mini",
      "supported_models": ["gpt-4.1-mini"],
      "chat_completion_streaming": false
    }}
  }},
  "default_provider": "openai",
  "default_model": "gpt-4.1-mini"
}}"#
        ),
    );

    let start = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--full-auto",
            "--max-runtime-ms",
            "1000",
            "--max-ticks",
            "3",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run job should execute");
    assert!(
        start.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&start.stderr),
        String::from_utf8_lossy(&start.stdout)
    );
    let start_json = output_json(&start);
    assert_eq!(start_json["command"], "research run");
    assert_eq!(
        start_json["data"]["schema_version"],
        "autonomous_research_job_run_result.v1"
    );
    let job = &start_json["data"]["job"];
    let job_id = job["job_id"].as_str().expect("job id");
    let project_id = start_json["project_id"]
        .as_str()
        .expect("project id")
        .to_string();
    let stage_execution_id = job["stage_execution_id"]
        .as_str()
        .expect("stage execution id");
    write_literature_review_ready_fixture(&workspace_root, job_id, &project_id, stage_execution_id);
    assert!(Path::new(job["job_state_path"].as_str().expect("job state")).exists());
    assert_ne!(job["status"], "completed");
    let active_run_ref =
        std::fs::read_to_string(workspace_root.join(".pmcli/orchestrations/active_run"))
            .expect("auto research should persist an active goal run pointer");
    let active_run = std::fs::read_to_string(
        workspace_root
            .join(".pmcli/orchestrations")
            .join(active_run_ref.trim())
            .join("orchestration.json"),
    )
    .expect("auto research active goal run should be readable");
    assert!(
        active_run.contains(job_id),
        "fresh auto research goal run should be bound to current job {job_id}: {active_run}"
    );

    let status = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .args(["research", "jobs", "status", "--json"])
        .output()
        .expect("research jobs status should execute");
    assert!(status.status.success());
    let status_json = output_json(&status);
    assert_eq!(
        status_json["data"]["schema_version"],
        "autonomous_research_job_list.v1"
    );
    assert!(status_json["data"]["jobs"]
        .as_array()
        .expect("jobs")
        .iter()
        .any(|job| job["job_id"] == job_id));

    let mut tick_json = Value::Null;
    for _ in 0..8 {
        tick_json = run_astra_research_tick_json_with_config(
            &workspace_root,
            &registry_root,
            Some(&config_root),
            job_id,
        );
    }
    assert_eq!(
        tick_json["data"]["schema_version"],
        "autonomous_research_job_tick_result.v1"
    );
    assert_eq!(tick_json["data"]["review_passed"], false);
    assert_eq!(tick_json["data"]["job"]["last_review"]["verdict"], "fail");
    assert!(matches!(
        tick_json["data"]["job"]["phase"].as_str(),
        Some(
            "review_failed_waiting_for_main_agent_decision"
                | "stage_review_failed_waiting_for_repair_gate"
                | "blocked_by_open_obligations"
        )
    ));
    assert!(matches!(
        tick_json["data"]["job"]["stop_reason"].as_str(),
        Some(
            "review_failure_requires_main_agent_route_decision"
                | "stage_review_failed_repair_requires_approval"
                | "active_blocking_obligations_require_main_agent_action"
        )
    ));
    assert_ne!(tick_json["data"]["job"]["status"], "completed");

    let tail = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .args([
            "research", "jobs", "tail", job_id, "--limit", "80", "--json",
        ])
        .output()
        .expect("research jobs tail should execute");
    assert!(tail.status.success());
    let tail_json = output_json(&tail);
    let events = tail_json["data"]["events"].as_array().expect("tail events");
    assert!(events
        .iter()
        .any(|event| event["event"] == "review_gate_completed"));
    assert!(events
        .iter()
        .any(|event| event["event"] == "goal_tick_completed"));
    assert!(events
        .iter()
        .any(|event| event["event"] == "review_gate_started"));
    assert!(events.iter().any(|event| {
        event["event"] == "review_gate_started"
            && event["payload"]["stage_contract"]["stage_id"] == "literature"
            && event["payload"]["stage_contract"]["artifact_type"] == "literature_matrix"
    }));
    assert!(events.iter().any(|event| {
        event["event"] == "repair_obligation_recorded_waiting_for_main_agent_decision"
            || event["event"] == "main_agent_round_completed"
            || event["event"] == "tick_started"
    }));
    let status_after_failed_review = read_active_research_status(&workspace_root, &registry_root);
    assert_eq!(
        status_after_failed_review["data"]["active_stage_execution"]["stage_id"], "literature",
        "failed literature review must not advance to novelty"
    );

    let repair_tick = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["research", "jobs", "tick", job_id, "--json"])
        .output()
        .expect("repair research jobs tick should execute");
    assert!(
        repair_tick.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&repair_tick.stderr),
        String::from_utf8_lossy(&repair_tick.stdout)
    );
    let repair_tail = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args([
            "research", "jobs", "tail", job_id, "--limit", "40", "--json",
        ])
        .output()
        .expect("repair research jobs tail should execute");
    assert!(repair_tail.status.success());
    let repair_tail_json = output_json(&repair_tail);
    let repair_events = repair_tail_json["data"]["events"]
        .as_array()
        .expect("repair tail events");
    assert!(
        repair_events.iter().any(|event| {
            event["event"] == "main_agent_round_started"
                && event["payload"]["reason"] == "failed_review_repair"
        }),
        "failed review must force the next tick back into the main agent round: {repair_events:?}"
    );

    stop_tx.send(()).expect("mock provider server should stop");
    let requests = server.join().expect("mock provider server should join");
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n")),
        "mock provider should be exercised by the autonomous research loop: {requests:?}"
    );
    assert!(
        requests
            .iter()
            .any(|request| request.contains(r#""stream":false"#)),
        "mock provider should run through the configured non-streaming profile: {requests:?}"
    );
}

#[test]
fn astra_research_stage_protocol_projects_stage_local_board_tasks() {
    let workspace_root = init_real_git_workspace("astra_stage_local_board_tasks");
    let registry_root = registry_root_env();
    let prompt = "Study whether stage-local autonomous research tasks are visible on the board";

    let start = start_research_run_json(
        &workspace_root,
        &registry_root,
        prompt,
        &["--automation-mode", "full_auto"],
    );
    assert_eq!(start["data"]["job"]["automation_mode"], "full_auto");
    assert_eq!(
        start["data"]["job"]["stage_task_semantic_review_mode"],
        "provider"
    );

    let goals = run_cli_json(
        &workspace_root,
        &registry_root,
        &["goals", "status", "--json"],
    );
    let entries = goals["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries");
    let standard_setting = entries
        .iter()
        .find(|entry| {
            entry["source"]["source_kind"] == "research_stage_task_candidate"
                && entry["stage_task"]["stage_id"] == "literature"
                && entry["stage_task"]["task_type"] == "acceptance standard setting"
        })
        .expect("literature acceptance standard setter candidate should be projected");
    assert_eq!(standard_setting["bucket_id"], "needs_review");
    assert_eq!(
        standard_setting["status"],
        "candidate_requires_main_agent_publication"
    );
    assert_eq!(
        standard_setting["stage_task"]["worker_role"],
        "stage_standard_setter"
    );
    assert!(standard_setting["stage_task"]["objective"]
        .as_str()
        .expect("standard setting objective")
        .contains("top-tier acceptance rubric"));
    let paper_search = entries
        .iter()
        .find(|entry| {
            entry["source"]["source_kind"] == "research_stage_task_candidate"
                && entry["stage_task"]["stage_id"] == "literature"
                && entry["stage_task"]["task_type"] == "paper search"
        })
        .expect("literature paper search candidate should be projected");
    assert_eq!(paper_search["bucket_id"], "needs_review");
    assert!(paper_search["action_policy"].is_null());
    assert_eq!(
        paper_search["stage_task"]["required_output_artifact_type"],
        "literature_matrix"
    );
    assert!(paper_search["stage_task"]["required_output_fields"]
        .as_array()
        .expect("required output fields")
        .iter()
        .any(|field| field == "source entries"));
    assert!(paper_search["stage_task"]["acceptance_checks"]
        .as_array()
        .expect("acceptance checks")
        .iter()
        .any(|check| check
            .as_str()
            .unwrap_or_default()
            .contains("closest method families")));
    let dispatch_policy = entries
        .iter()
        .find(|entry| {
            entry["source"]["source_kind"] == "goal_run_automation_policy"
                && entry["title"] == "dispatch_agent_task"
        })
        .expect("dispatch policy should remain visible");
    assert_eq!(dispatch_policy["bucket_id"], "needs_review");
    assert_eq!(dispatch_policy["action_policy"], "requires_main_agent_task");

    let advance = run_cli_json(
        &workspace_root,
        &registry_root,
        &["goals", "advance", "--json"],
    );
    assert!(
        advance["data"]["dispatch"].is_null(),
        "runtime stage candidates must not be dispatched before main-agent publication: {advance:?}"
    );
    assert!(advance["data"]["task_pool"]["entries"]
        .as_array()
        .expect("advance task pool entries")
        .iter()
        .any(
            |entry| entry["source"]["source_kind"] == "research_stage_task_candidate"
                && entry["bucket_id"] == "needs_review"
        ));
}

#[test]
fn astra_research_failed_review_records_obligation_without_runtime_repair_route() {
    let workspace_root = init_real_git_workspace("astra_repair_obligation_no_runtime_route");
    let registry_root = registry_root_env();
    let config_root = unique_temp_dir("astra_repair_obligation_no_runtime_route_config")
        .display()
        .to_string();
    let (stop_tx, server) = start_mock_openai_provider(
        &config_root,
        mock_openai_assistant_response_body(
            r##"# Claim Table

stage: result-to-claim
artifact_type: claim_table
claim: Astra can enforce stage-gated autonomous research by keeping supported claims bounded to accepted worker evidence and by refusing to promote unsupported claims into the canonical research record.
evidence refs: research/stages/arj_fixture/result-to-claim/accepted_worker_evidence/index.json; research/stages/arj_fixture/result-to-claim/stage_plan.md; research/stages/arj_fixture/result-to-claim/stage_acceptance_rubric.md; research/stages/arj_fixture/literature/literature_matrix.md; research/stages/arj_fixture/monitor/experiment_report.md
support level: partial, because the present claim is only justified for the fixture scope and still depends on upstream literature, experiment, implementation, and report evidence.
scope and assumptions: fixture scope only, with accepted worker evidence treated as the only admissible support for the current claim and with the main agent expected to narrow any unsupported statement.
limitations: this artifact does not prove the whole research program; it only shows how one bounded claim can be traced to accepted evidence and where the evidence stops.
required extra evidence if partial: literature matrix, experiment plan, implementation manifest, experiment report.
deletion/narrowing decision: narrow the claim until all upstream evidence is accepted, every evidence ref is auditable, and the unsupported portion is removed from the canonical claim table.
risk: unsupported claim stronger than evidence and missing upstream artifacts may still block review even when the local claim text looks complete.
This fixture claim table intentionally states the decision boundary in plain language so the runtime can treat it as reviewable stage content rather than diagnostic filler. It mentions accepted evidence, upstream support, explicit limitations, and the action needed to repair the claim before promotion. The wording is intentionally long enough to satisfy the reviewability gate and to keep the artifact from being mistaken for a prompt echo or a short summary. The main-agent trace is preserved as evidence, but the stage artifact itself remains the canonical object under review.
"##,
        ),
    );
    let prompt = "Autonomously research a result-to-claim repair route";

    let start = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--review-model",
            "gpt-4.1-mini",
            "--full-auto",
            "--max-runtime-ms",
            "1",
            "--max-ticks",
            "1",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run should execute");
    assert!(start.status.success());
    let start_json = output_json(&start);
    let job_id = start_json["data"]["job_id"].as_str().expect("job id");
    let project_id = start_json["project_id"]
        .as_str()
        .expect("project id")
        .to_string();
    let job_state_path = start_json["data"]["job_state_path"]
        .as_str()
        .expect("job state path");
    let record = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "research",
            "record",
            "--kind",
            "auto_research_test",
            "--title",
            "Result to claim repair stage",
            "--stage",
            "result-to-claim",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "experiments/result-table.csv",
            "--json",
        ],
    );
    let stage_execution_id = record["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage execution id")
        .to_string();
    let mut job_state: Value = serde_json::from_str(
        &std::fs::read_to_string(job_state_path).expect("job state should read"),
    )
    .expect("job state should parse");
    job_state["stage_execution_id"] = json!(stage_execution_id);
    job_state["review_rounds_completed"] = json!(0);
    job_state["max_review_rounds"] = json!(10);
    std::fs::write(
        job_state_path,
        serde_json::to_string_pretty(&job_state).expect("job state should serialize"),
    )
    .expect("job state should write");

    let stage_path = workspace_root.join(format!(
        "research/stages/{job_id}/result-to-claim/claim_table.md"
    ));
    write_result_to_claim_review_ready_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "result-to-claim",
        "claim_table",
        "task_result_to_claim_acceptance_standard_setting",
        "acceptance standard setting",
        "research_synthesizer",
        "claim_table",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &[
            "task_type",
            "worker_role",
            "objective",
            "required_output_artifact_type",
            "required_output_fields",
        ],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "claim_auditor",
        "pass",
        92,
        true,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "result-to-claim",
        "claim_table",
        "task_result_to_claim_evidence_to_claim_mapping",
        "evidence-to-claim mapping",
        "claim_auditor",
        "claim_table",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &["evidence refs", "claim support boundary", "next action"],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "claim_auditor",
        "pass",
        92,
        true,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "result-to-claim",
        "claim_table",
        "task_result_to_claim_unsupported_claim_removal",
        "unsupported-claim removal",
        "claim_auditor",
        "claim_table",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &["unsupported claim", "repair route", "decision reason"],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "claim_auditor",
        "pass",
        92,
        true,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "result-to-claim",
        "claim_table",
        "task_result_to_claim_limitation_extraction",
        "limitation extraction",
        "claim_auditor",
        "claim_table",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &["limitations", "scope and assumptions", "risk"],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "claim_auditor",
        "pass",
        92,
        true,
    );

    let tick_json = run_astra_research_tick_until_review_with_config(
        &workspace_root,
        &registry_root,
        Some(&config_root),
        job_id,
        8,
    );
    assert_eq!(tick_json["data"]["review_passed"], false);

    let tail = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .args([
            "research", "jobs", "tail", job_id, "--limit", "40", "--json",
        ])
        .output()
        .expect("research jobs tail should execute");
    assert!(tail.status.success());
    let tail_json = output_json(&tail);
    let events = tail_json["data"]["events"].as_array().expect("events");
    let repair_event = events
        .iter()
        .rev()
        .find(|event| {
            event["event"] == "repair_obligation_recorded_waiting_for_main_agent_decision"
                && event["payload"]["stage_id"] == "result-to-claim"
        })
        .expect("result-to-claim repair obligation should be recorded");
    assert_eq!(
        repair_event["payload"]["runtime_hint"]["failure_class"],
        "unsupported_claim"
    );
    assert_eq!(repair_event["payload"]["stage_id"], "result-to-claim");
    assert_eq!(
        repair_event["payload"]["runtime_hint"]["cleanup_required"],
        false
    );
    assert_eq!(
        repair_event["payload"]["runtime_hint"]["suggested_operation"], "repair",
        "runtime may expose an untrusted hint but must not apply a pivot or repair route"
    );
    assert!(repair_event["payload"].get("repair_plan_ref").is_none());
    assert!(repair_event["payload"].get("repair_plan_doc_ref").is_none());
    let review_summary_ref = tick_json["data"]["job"]["last_review"]["review_summary_path"]
        .as_str()
        .expect("review summary ref");
    let review_summary = std::fs::read_to_string(workspace_root.join(review_summary_ref))
        .expect("review summary should read");
    assert!(review_summary.contains("doc_frame:"));
    assert!(review_summary
        .contains("Every blocking finding below must become a board-visible repair task"));
    let rubric_ref = tick_json["data"]["job"]["last_review"]["rubric_path"]
        .as_str()
        .expect("rubric ref");
    let rubric =
        std::fs::read_to_string(workspace_root.join(rubric_ref)).expect("rubric should read");
    assert!(rubric.contains("doc_frame:"));
    assert!(rubric.contains("Expert-Level Acceptance Target"));
    let stage_doc = std::fs::read_to_string(&stage_path).expect("stage doc should read");
    assert!(stage_doc.contains("doc_frame:"));
    let job_artifact_refs = tick_json["data"]["job"]["artifact_refs"]
        .as_array()
        .expect("artifact refs");
    assert!(job_artifact_refs.iter().any(|value| value == rubric_ref));
    assert!(job_artifact_refs
        .iter()
        .any(|value| value == review_summary_ref));
    assert!(
        !events
            .iter()
            .any(|event| event["event"] == "cleanup_plan_created"),
        "runtime hints must not trigger cleanup before a main-agent route decision"
    );

    let goals_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .args(["goals", "status", "--json"])
        .output()
        .expect("goals status should execute");
    assert!(goals_output.status.success());
    let goals = output_json(&goals_output);
    let entries = goals["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries");
    assert!(
        !entries
            .iter()
            .any(|entry| entry["source"]["source_kind"] == "research_stage_repair_candidate"),
        "failed review must not create repair board tasks until the main agent publishes them"
    );

    stop_tx.send(()).expect("mock provider server should stop");
    let requests = server.join().expect("mock provider server should join");
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n")),
        "mock provider should be exercised by the autonomous research loop: {requests:?}"
    );
}

#[test]
fn astra_research_final_review_rejects_markdown_only_or_missing_pdf() {
    let workspace_root = init_real_git_workspace("astra_final_review_requires_tex_pdf");
    let registry_root = registry_root_env();
    let config_root = unique_temp_dir("astra_final_review_requires_tex_pdf_config")
        .display()
        .to_string();
    let (stop_tx, server) = start_mock_openai_provider(
        &config_root,
        mock_openai_assistant_response_body(
            r##"# hard_review_packet

PDF and TeX refs: papers/{job_id}/main.tex and papers/{job_id}/compiled_pdf_bundle.md must both be present, and the compiled bundle must prove that the PDF exists rather than merely claiming it.
claim table ref: research/stages/{job_id}/result-to-claim/claim_table.md must show a bounded claim, explicit evidence refs, support level, limitations, and narrowing decision.
literature matrix ref: research/stages/{job_id}/literature/literature_matrix.md must provide grounded source coverage and a clear claim boundary.
experiment report ref: research/stages/{job_id}/monitor/experiment_report.md must expose the reported metrics, anomaly handling, and rerun decisions.
implementation manifest ref: research/stages/{job_id}/implement-solution/implementation_manifest.md must tie the code entry points and tests to the paper claim.
reproducibility audit: the reviewer checks command strings, configs, logs, and artifact refs so the bundle is auditable end to end, not just narratively plausible.
novelty audit: the review must confirm the paper does not overclaim novelty, does not skip accepted evidence boundaries, and does not hide unresolved upstream gaps behind polished prose.
verdict, score, findings, repair routes: this fixture should fail with a concrete score and explicit repair routes when the compiled PDF is missing, the TeX source is incomplete, or the claim bundle is not fully auditable. The content is intentionally long, direct, and evidence-oriented so it is treated as a reviewable hard review packet rather than a diagnostic note.
"##,
        ),
    );
    let prompt = "Autonomously produce a final paper with TeX and PDF";

    let start = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--full-auto",
            "--max-runtime-ms",
            "1",
            "--max-ticks",
            "1",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run should execute");
    assert!(start.status.success());
    let start_json = output_json(&start);
    let job_id = start_json["data"]["job_id"].as_str().expect("job id");
    let project_id = start_json["project_id"]
        .as_str()
        .expect("project id")
        .to_string();
    let job_state_path = start_json["data"]["job_state_path"]
        .as_str()
        .expect("job state path");
    let record = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "research",
            "record",
            "--kind",
            "auto_research_test",
            "--title",
            "Final research review stage",
            "--stage",
            "research-review",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "papers/final-review.md",
            "--json",
        ],
    );
    let stage_execution_id = record["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage execution id")
        .to_string();
    let mut job_state: Value = serde_json::from_str(
        &std::fs::read_to_string(job_state_path).expect("job state should read"),
    )
    .expect("job state should parse");
    job_state["stage_execution_id"] = json!(stage_execution_id);
    job_state["review_rounds_completed"] = json!(0);
    job_state["max_review_rounds"] = json!(10);
    job_state["artifact_refs"] = json!([format!(
        "research/stages/{job_id}/research-review/hard_review_packet.md"
    )]);
    std::fs::write(
        job_state_path,
        serde_json::to_string_pretty(&job_state).expect("job state should serialize"),
    )
    .expect("job state should write");

    write_research_review_ready_fixture(&workspace_root, job_id, &project_id, &stage_execution_id);
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "research-review",
        "hard_review_packet",
        "task_research_review_acceptance_standard_setting",
        "acceptance standard setting",
        "research_synthesizer",
        "hard_review_packet",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &[
            "task_type",
            "worker_role",
            "objective",
            "required_output_artifact_type",
            "required_output_fields",
        ],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "hard_reviewer",
        "pass",
        92,
        true,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "research-review",
        "hard_review_packet",
        "task_research_review_adversarial_review",
        "adversarial review",
        "hard_reviewer",
        "hard_review_packet",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &["adversarial claims", "failure modes", "repair routes"],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "hard_reviewer",
        "pass",
        92,
        true,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "research-review",
        "hard_review_packet",
        "task_research_review_reproducibility_review",
        "reproducibility review",
        "hard_reviewer",
        "hard_review_packet",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &["command", "config", "log"],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "hard_reviewer",
        "pass",
        92,
        true,
    );
    write_stage_accepted_worker_evidence_fixture(
        &workspace_root,
        job_id,
        &project_id,
        &stage_execution_id,
        "research-review",
        "hard_review_packet",
        "task_research_review_claim_evidence_audit",
        "claim-evidence audit",
        "hard_reviewer",
        "hard_review_packet",
        "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md",
        &[
            "claim table",
            "literature matrix",
            "experiment report",
            "implementation manifest",
        ],
        &["fixture accepted worker evidence"],
        "strong",
        95,
        "hard_reviewer",
        "pass",
        92,
        true,
    );
    let research_review_synthesis_index_path = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(&stage_execution_id)
        .join("accepted_worker_evidence")
        .join("index.json");
    let mut research_review_synthesis_index: Value = serde_json::from_str(
        &std::fs::read_to_string(&research_review_synthesis_index_path)
            .expect("research-review evidence index should read"),
    )
    .expect("research-review evidence index should parse");
    let mut research_review_entries = research_review_synthesis_index["entries"]
        .as_array()
        .cloned()
        .expect("research-review evidence entries");
    research_review_entries.push(json!({
        "agent_id": "agent_research_review_synthesizer",
        "task_id": "task_research_review_stage_artifact_synthesis",
        "task_type": "stage artifact synthesis",
        "worker_role": "research_synthesizer",
        "required_output_artifact_type": "hard_review_packet",
        "output_manifest_ref": "research/stages/fixture/accepted_worker_evidence/output_manifest.json",
        "task_packet_ref": "research/stages/fixture/accepted_worker_evidence/task_packet.json",
        "evidence_refs": ["research/stages/fixture/accepted_worker_evidence/provider_worker_evidence.md"],
        "matched_required_fields": [
            "PDF and TeX refs",
            "claim table ref",
            "literature matrix ref",
            "experiment report ref",
            "implementation manifest ref",
            "reproducibility audit",
            "novelty audit",
            "verdict, score, findings, repair routes"
        ],
        "matched_acceptance_checks": ["fixture accepted worker evidence"],
        "matched_quality_signals": ["provider_tool_completed"],
        "quality_profile": {
            "schema_version": "goal_stage_task_quality_profile.v1",
            "score": 100,
            "level": "strong",
            "strengths": ["fixture accepted worker evidence"],
            "risks": []
        },
        "semantic_review": {
            "schema_version": "goal_stage_task_semantic_review_result.v1",
            "verdict": "pass",
            "score": 92,
            "reviewer_role": "hard_reviewer",
            "review_model": "fixture",
            "execution_mode": "fixture",
            "review_packet_ref": "research/stages/fixture/accepted_worker_evidence/review_packet.json",
            "review_trace_ref": "research/stages/fixture/accepted_worker_evidence/review_trace.json",
            "findings": ["fixture accepted worker evidence"],
            "cleanup_required": false
        },
        "main_agent_acceptance": {
            "schema_version": "goal_stage_task_semantic_review_result.v1",
            "verdict": "pass",
            "score": 92,
            "reviewer_role": "main_agent_worker_artifact_acceptance",
            "review_model": "astra-main-agent",
            "execution_mode": "main_agent_authority_projection",
            "review_packet_ref": "main_agent_worker_artifact_decision::fixture",
            "review_trace_ref": "main_agent_worker_artifact_decision::fixture",
            "findings": ["fixture main agent acceptance"],
            "cleanup_required": false
        },
        "acceptance_authority": "main_agent_worker_artifact_decision",
        "main_agent_decision_ref": "main_agent_worker_artifact_decision::task_research_review_stage_artifact_synthesis",
        "review_required": true,
        "active_status": "active",
        "current_evidence_set_id": format!("{job_id}::task_research_review_stage_artifact_synthesis"),
        "superseded_by_task_id": null,
        "replacement_of_task_ids": [],
        "decision_reason": null,
        "created_at": "1"
    }));
    research_review_synthesis_index["entries"] = json!(research_review_entries);
    write_text_file(
        research_review_synthesis_index_path,
        &serde_json::to_string_pretty(&research_review_synthesis_index)
            .expect("research-review evidence index should serialize"),
    );
    let compiled_pdf_path = workspace_root.join(format!("papers/{job_id}/build/main.pdf"));
    let _ = fs::remove_file(&compiled_pdf_path);
    write_text_file(
        workspace_root.join(format!("papers/{job_id}/compiled_pdf_bundle.md")),
        "# compiled_pdf_bundle\n\nPDF path: papers/job/main.pdf\n\nBuild log: missing\n\nPDF validation result: fail: PDF is missing\n",
    );

    let mut tick_json = Value::Null;
    for _ in 0..40 {
        tick_json = run_astra_research_tick_json_with_config(
            &workspace_root,
            &registry_root,
            Some(&config_root),
            job_id,
        );
    }
    assert_eq!(tick_json["data"]["review_passed"], false);
    assert_eq!(
        tick_json["data"]["job"]["status"], "running",
        "Markdown-only or missing-PDF final packet must not complete the job"
    );
    assert_eq!(
        tick_json["data"]["job"]["phase"], "review_readiness_blocked",
        "missing PDF should block review readiness before live review runs"
    );
    assert_eq!(
        tick_json["data"]["job"]["stop_reason"],
        "review_readiness_blocked_requires_main_agent_action"
    );
    assert!(tick_json["data"]["job"]["last_review"].is_null());
    assert!(tick_json["data"]["job"]["artifact_refs"]
        .as_array()
        .expect("artifact refs")
        .iter()
        .any(|value| value
            .as_str()
            .unwrap_or_default()
            .contains("hard_review_packet.md")));

    stop_tx.send(()).expect("mock provider server should stop");
    let requests = server.join().expect("mock provider server should join");
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n")),
        "mock provider should be exercised by the autonomous research loop: {requests:?}"
    );
}

#[test]
fn agent_list_files_output_is_bounded_for_large_workspace() {
    let workspace_root = init_real_git_workspace("astra_agent_tool_list_files_bound");
    let registry_root = registry_root_env();
    run_cli_json(
        &workspace_root,
        &registry_root,
        &["projects", "init", "--json"],
    );
    fs::create_dir_all(workspace_root.join(".claude").join("ignored"))
        .expect("ignored dir should be created");
    write_text_file(
        workspace_root.join(".claude/ignored/hidden.txt"),
        "hidden\n",
    );
    fs::create_dir_all(workspace_root.join("node_modules").join("ignored"))
        .expect("node_modules dir should be created");
    write_text_file(
        workspace_root.join("node_modules/ignored/package.json"),
        "{}\n",
    );
    for index in 0..350 {
        let path = workspace_root
            .join("many")
            .join(format!("file_{index:03}.txt"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("test dir should be created");
        }
        write_text_file(path, "x\n");
    }

    let output = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tools", "run", "list_files", "--json"])
        .output()
        .expect("list_files should execute");
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let json = output_json(&output);
    assert_eq!(json["command"], "tools run");
    assert_eq!(json["data"]["structured"]["count"], 200);
    assert_eq!(json["data"]["structured"]["truncated"], true);
    let output_text = json["data"]["output"]
        .as_str()
        .expect("list output should be text");
    assert!(!output_text.contains(".claude/ignored"));
    assert!(!output_text.contains("node_modules/ignored"));
}

#[test]
fn astra_research_run_full_auto_does_not_use_legacy_live_synthesis_pipeline() {
    let workspace_root = init_real_git_workspace("astra_research_run_no_live_synthesis_pipeline");
    let registry_root = registry_root_env();
    let prompt = "Assess a benchmark idea for autonomous research CLI evaluation";

    let output = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", "http://127.0.0.1:9/v1")
        .env("RESEARCH_CLI_PROVIDER", "openai")
        .env("RESEARCH_CLI_MODEL", "glm-5.1")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--full-auto",
            "--max-runtime-ms",
            "1",
            "--max-ticks",
            "1",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run should execute");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let json = output_json(&output);
    assert_eq!(json["command"], "research run");
    assert_eq!(
        json["data"]["schema_version"],
        "autonomous_research_job_run_result.v1"
    );
    assert_eq!(json["data"]["job"]["review_model"], Value::Null);
    assert_eq!(json["data"]["job"]["automation_mode"], "full_auto");
    assert!(json["data"].get("live_synthesis").is_none());
    assert!(json["data"].get("pipeline_stage_count").is_none());
    assert!(!workspace_root.join("research/pipeline").exists());
    assert!(!workspace_root.join("experiments/astra_pipeline").exists());
}

#[test]
fn astra_research_run_human_mode_failed_stage_review_waits_for_repair_gate() {
    let workspace_root = init_real_git_workspace("astra_research_run_human_repair_gate");
    let registry_root = registry_root_env();
    let config_root = unique_temp_dir("astra_research_run_human_repair_gate_config")
        .display()
        .to_string();
    let (stop_tx, server) = start_mock_openai_provider(
        &config_root,
        mock_openai_assistant_response_body(
            r##"# hard_review_packet

PDF and TeX refs

claim table ref

literature matrix ref

experiment report ref

implementation manifest ref

reproducibility audit

novelty audit

verdict, score, findings, repair routes

compiled PDF missing
"##,
        ),
    );
    let prompt = "Human gated autonomous research should not repair without approval";

    let output = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("ASTRA_STAGE_TASK_SEMANTIC_REVIEW")
        .args([
            "research",
            "run",
            "--prompt",
            prompt,
            "--automation-mode",
            "human_in_the_loop",
            "--max-runtime-ms",
            "1000",
            "--max-ticks",
            "3",
            "--interval-ms",
            "1",
            "--json",
        ])
        .output()
        .expect("astra research run should execute");
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let run_json = output_json(&output);
    let job_id = run_json["data"]["job_id"].as_str().expect("job id");
    assert!(run_json["data"]["job"]["last_review"].is_null());
    assert!(matches!(
        run_json["data"]["job"]["phase"].as_str(),
        Some("review_readiness_blocked" | "tick_budget_pause" | "time_budget_pause")
    ));
    assert!(matches!(
        run_json["data"]["job"]["stop_reason"].as_str(),
        Some(
            "review_readiness_blocked_requires_main_agent_action"
                | "max_ticks_elapsed_resume_later"
                | "max_runtime_ms_elapsed_resume_later"
        )
    ));
    let status = read_active_research_status(&workspace_root, &registry_root);
    assert_eq!(
        status["data"]["active_stage_execution"]["stage_id"],
        "literature"
    );

    let tail = Command::new(astra_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &config_root)
        .args([
            "research", "jobs", "tail", job_id, "--limit", "20", "--json",
        ])
        .output()
        .expect("research jobs tail should execute");
    assert!(tail.status.success());
    let tail_json = output_json(&tail);
    let events = tail_json["data"]["events"].as_array().expect("tail events");
    assert!(
        !events
            .iter()
            .any(|event| event["event"] == "repair_decision_recorded"),
        "human-in-the-loop failed review should surface the repair gate instead of auto-recording repair"
    );
    assert!(
        !events
            .iter()
            .any(|event| event["event"]
                == "repair_obligation_recorded_waiting_for_main_agent_decision"),
        "human-in-the-loop failed review should not use the full-auto main-agent obligation path"
    );

    stop_tx.send(()).expect("mock provider server should stop");
    let requests = server.join().expect("mock provider server should join");
    if run_json["data"]["job"]["phase"] != "time_budget_pause" {
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n")),
            "mock provider should be exercised by the autonomous research loop: {requests:?}"
        );
    }
}

fn write_explicit_canonical_manifests(workspace_root: &Path) {
    write_explicit_canonical_surface_manifest(workspace_root);
    write_explicit_code_owner_manifest(workspace_root);
    write_explicit_canonical_lineage_manifest(workspace_root);
}

fn write_explicit_canonical_surface_manifest(workspace_root: &Path) {
    write_text_file(
        workspace_root.join(".pmcli/canonical_surface.json"),
        &json!({
            "schema_version": "1",
            "source": "test",
            "active_status_doc": "README.md",
            "active_architecture_doc": "",
            "active_implementation_plan": "",
            "active_proof_packet": "",
            "active_code_roots": ["src"],
            "active_schema_roots": ["schemas"],
            "archive_roots": [".pmcli/archive"],
            "external_surface_files": ["README.md"],
            "superseded_paths": [],
            "forbidden_public_patterns": ["final_final", "latest_new"]
        })
        .to_string(),
    );
}

fn write_explicit_code_owner_manifest(workspace_root: &Path) {
    write_text_file(
        workspace_root.join(".pmcli/code_owner_manifest.json"),
        &json!({
            "schema_version": "1",
            "source": "test",
            "active_runtime_root": "src/runtime",
            "active_command_registry": "src/commands/help.rs",
            "active_tool_registry": "src/tools",
            "active_schema_registry": "schemas",
            "active_provider_layer": "src/providers",
            "active_session_layer": "src/session",
            "legacy_paths": [],
            "forbidden_duplicate_modules": []
        })
        .to_string(),
    );
}

fn write_explicit_canonical_lineage_manifest(workspace_root: &Path) {
    write_text_file(
        workspace_root.join(".pmcli/canonical_lineage.json"),
        &json!({
            "schema_version": "canonical_lineage.v1",
            "source": "test",
            "current_generation": "test-current",
            "policy": "current_nodes_must_not_depend_on_non_current_sources",
            "nodes": [
                {
                    "artifact_id": "doc.status.readme",
                    "family": "doc",
                    "role": "status",
                    "path": "README.md",
                    "status": "current",
                    "generation": "test-current",
                    "derived_from": [],
                    "evidence_refs": ["README.md"]
                },
                {
                    "artifact_id": "code.src.current",
                    "family": "code",
                    "role": "runtime",
                    "path": "src",
                    "status": "current",
                    "generation": "test-current",
                    "derived_from": ["doc.status.readme"],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "schema.current",
                    "family": "schema",
                    "role": "contract",
                    "path": "schemas",
                    "status": "current",
                    "generation": "test-current",
                    "derived_from": ["code.src.current"],
                    "evidence_refs": []
                }
            ]
        })
        .to_string(),
    );
}

#[test]
fn startup_json_returns_interactive_launch_result_without_repl_event() {
    let workspace_root = init_git_workspace("startup_json");
    let registry_root = registry_root_env();

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
        .args(["--json"])
        .output()
        .expect("bare launch json should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "launch");
    assert_eq!(json["data"]["launch_disposition"], "inspect_exit");
    assert_eq!(json["data"]["interactive_eligible"], true);
    assert!(json["data"]["session_id"].is_string());
    assert!(json["data"]["project_id"].is_string());
    assert!(json["data"]["workspace_root"].is_string());
    assert!(json["data"]["preflight"].is_object());
    assert!(json["data"]["project_trace"].is_object());
    assert!(json["data"]["profile_trace"].is_object());

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "runtime_preflight" && event["object_id"] == "launch"
    }));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "session_open"));
    assert!(!events.iter().any(|event| event["event_name"] == "repl"));
}

#[test]
fn bare_text_launch_defaults_to_inline_repl_and_bootstraps_plain_directory() {
    let workspace_root = unique_temp_dir("bare_tui_plain_directory");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .output()
        .expect("bare text launch should execute");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = stdout_text(&output);
    assert!(stdout.contains("Astra Code 代码智能体"));
    assert!(stdout.contains("对话优先的代码智能体"));
    assert!(stdout.contains("研究："));
    assert!(stdout.contains("输入：直接输入对话"));
    assert!(stdout.contains("/language"));
    assert!(!stdout.contains("fullscreen_split_pane_with_sidebar"));
    assert!(workspace_root.join(".git").is_dir());
    assert!(
        !workspace_root.join(".pmcli").exists(),
        "bare launch should not write runtime state into the workspace"
    );
    let head = Command::new("git")
        .current_dir(&workspace_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .expect("git rev-parse should execute");
    assert!(
        head.status.success(),
        "bare launch should create a baseline commit, stderr: {}",
        String::from_utf8_lossy(&head.stderr)
    );

    let current_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "current", "--json"])
        .output()
        .expect("auto-registered project should be current");
    assert!(current_output.status.success());
    let current_json = output_json(&current_output);
    assert_eq!(
        current_json["data"]["resolved_project"]["workspace_root"],
        workspace_root.display().to_string()
    );
    let data_dir = current_json["data"]["resolved_project"]["data_dir"]
        .as_str()
        .expect("resolved data dir");
    assert!(
        data_dir.starts_with(&format!("{registry_root}/projects/")),
        "runtime state should live under global state root, got {data_dir}"
    );
}

#[test]
fn chat_json_uses_same_launch_inspect_lane() {
    let workspace_root = init_git_workspace("chat_json");
    let registry_root = registry_root_env();

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
        .args(["chat", "--json"])
        .output()
        .expect("chat json should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "chat");
    assert_eq!(json["data"]["launch_disposition"], "inspect_exit");
    assert_eq!(json["data"]["interactive_eligible"], true);
    assert!(json["data"]["session_id"].is_string());
    assert!(json["data"]["preflight"].is_object());
}

#[test]
fn help_json_returns_canonical_command_registry() {
    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("help_json_workspace"))
        .args(["help", "--json"])
        .output()
        .expect("help json should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "help");
    assert_eq!(json["data"]["scope"], "cli");
    let commands = json["data"]["commands"].as_array().expect("commands array");
    assert!(commands.iter().any(|entry| entry["name"] == "help"));
    assert!(commands.iter().any(|entry| entry["name"] == "chat"));
    assert!(commands.iter().any(|entry| entry["name"] == "model"));
    assert!(commands.iter().any(|entry| entry["name"] == "prompt"));
    assert!(commands.iter().any(|entry| entry["name"] == "usage"));
    assert!(commands.iter().any(|entry| entry["name"] == "cost"));
    assert!(commands.iter().any(|entry| entry["name"] == "stats"));
    assert!(commands.iter().any(|entry| entry["name"] == "continue"));
    assert!(commands.iter().any(|entry| entry["name"] == "setup"));
    assert!(commands.iter().any(|entry| entry["name"] == "mcp"));
    assert!(commands.iter().any(|entry| entry["name"] == "skills"));
    assert!(commands.iter().any(|entry| entry["name"] == "plugins"));
    assert!(commands.iter().any(|entry| entry["name"] == "hooks"));
    assert!(commands.iter().any(|entry| entry["name"] == "reviews"));
    assert!(commands.iter().any(|entry| entry["name"] == "goals"));
    assert!(commands.iter().any(|entry| entry["name"] == "routines"));
    assert!(commands
        .iter()
        .any(|entry| entry["name"] == "help" && entry["supports_json_help"] == true));
}

#[test]
fn help_json_marks_every_public_command_with_graduation_and_canonical_invocation() {
    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("help_graduation_registry_workspace"))
        .args(["help", "--json"])
        .output()
        .expect("help json should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    let commands = json["data"]["commands"].as_array().expect("commands array");

    let expected_commands = [
        "help",
        "palette",
        "slash",
        "chat",
        "prompt",
        "model",
        "resume",
        "continue",
        "inspect",
        "compact",
        "sessions",
        "projects",
        "permissions",
        "goals",
        "tools",
        "providers",
        "config",
        "usage",
        "cost",
        "stats",
        "doctor",
        "setup",
        "smoke",
        "conformance",
        "mcp",
        "skills",
        "plugins",
        "hooks",
        "memory",
        "feedback",
        "agents",
        "branches",
        "artifacts",
        "repo",
        "projectops",
        "routines",
        "host",
        "tui",
        "remote",
        "research",
        "reviews",
        "docs",
    ];

    assert_eq!(commands.len(), expected_commands.len());
    for expected in expected_commands {
        let command = commands
            .iter()
            .find(|entry| entry["name"] == expected)
            .unwrap_or_else(|| panic!("missing command {expected}"));
        assert!(
            command["canonical_json_invocation"]
                .as_array()
                .is_some_and(|args| !args.is_empty()),
            "{expected} must expose canonical_json_invocation"
        );
        let status = command["graduation_status"]
            .as_str()
            .unwrap_or_else(|| panic!("{expected} must expose graduation_status"));
        assert!(
            matches!(status, "graduated" | "feature_gated"),
            "{expected} has unexpected graduation_status {status}"
        );
    }

    let memory = commands
        .iter()
        .find(|entry| entry["name"] == "memory")
        .expect("memory command should be registered");
    assert_eq!(memory["graduation_status"], "graduated");
    let agents = commands
        .iter()
        .find(|entry| entry["name"] == "agents")
        .expect("agents command should be registered");
    assert_eq!(agents["graduation_status"], "graduated");
    let projectops = commands
        .iter()
        .find(|entry| entry["name"] == "projectops")
        .expect("projectops command should be registered");
    assert_eq!(projectops["graduation_status"], "graduated");
    let routines = commands
        .iter()
        .find(|entry| entry["name"] == "routines")
        .expect("routines command should be registered");
    assert_eq!(routines["graduation_status"], "graduated");
    let docs = commands
        .iter()
        .find(|entry| entry["name"] == "docs")
        .expect("docs command should be registered");
    assert_eq!(docs["graduation_status"], "graduated");
}

#[test]
fn agents_mock_lifecycle_persists_packet_runtime_trace_and_output_manifest() {
    let workspace_root = init_git_workspace("agents_mock_lifecycle");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--mock",
            "--intent",
            "review",
            "--role-profile",
            "reviewer",
            "--message",
            "review the M6 packet contract",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert!(
        start_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    let start_json = output_json(&start_output);
    assert_eq!(start_json["ok"], true);
    assert_eq!(start_json["command"], "agents start");
    assert_eq!(start_json["data"]["status"], "succeeded");
    assert_eq!(start_json["data"]["task_packet"]["intent"], "review");
    assert_eq!(
        start_json["data"]["task_packet"]["role_profile"],
        "reviewer"
    );
    assert_eq!(
        start_json["data"]["task_packet"]["retention_policy"],
        "ephemeral"
    );
    assert_eq!(start_json["data"]["task_packet"]["run_class"], "bounded");
    assert_eq!(
        start_json["data"]["task_packet"]["io_mode"],
        "request_response"
    );
    assert_eq!(
        start_json["data"]["task_packet"]["output_manifest_required"],
        true
    );
    assert_eq!(
        start_json["data"]["runtime_record"]["lifecycle_status"],
        "succeeded"
    );
    assert_eq!(start_json["data"]["output_manifest"]["status"], "complete");

    let agent_id = start_json["data"]["agent_id"]
        .as_str()
        .expect("agent id")
        .to_string();
    let agent_root = workspace_root.join(".pmcli/agents").join(&agent_id);
    assert!(agent_root.join("task_packet.json").exists());
    assert!(agent_root.join("runtime.json").exists());
    assert!(agent_root.join("output_manifest.json").exists());
    assert!(agent_root.join("traces").join("trace.json").exists());

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "list", "--json"])
        .output()
        .expect("agents list should execute");
    assert!(list_output.status.success());
    let list_json = output_json(&list_output);
    assert_eq!(list_json["data"]["total_count"], 1);
    assert_eq!(list_json["data"]["agents"][0]["agent_id"], agent_id);
    assert_eq!(
        list_json["data"]["agents"][0]["lifecycle_status"],
        "succeeded"
    );

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "inspect", &agent_id, "--json"])
        .output()
        .expect("agents inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["data"]["agent_id"], agent_id);
    assert_eq!(inspect_json["data"]["trace"]["final_status"], "succeeded");

    let traces_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "traces", &agent_id, "--json"])
        .output()
        .expect("agents traces should execute");
    assert!(
        traces_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&traces_output.stderr)
    );
    let traces_json = output_json(&traces_output);
    assert_eq!(traces_json["command"], "agents traces");
    assert_eq!(traces_json["data"]["agent_id"], agent_id);
    assert_eq!(traces_json["data"]["trace"]["final_status"], "succeeded");
    assert!(traces_json["data"]["trace_path"]
        .as_str()
        .expect("trace path should be present")
        .ends_with("traces/trace.json"));

    let stop_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "stop", &agent_id, "--json"])
        .output()
        .expect("agents stop should execute");
    assert!(stop_output.status.success());
    let stop_json = output_json(&stop_output);
    assert_eq!(stop_json["data"]["agent_id"], agent_id);
    assert_eq!(stop_json["data"]["idempotent"], true);
    assert_eq!(
        stop_json["data"]["runtime_record"]["lifecycle_status"],
        "succeeded"
    );
}

#[test]
fn agents_local_runner_executes_in_git_worktree_with_directive_status_and_trace() {
    let workspace_root = init_real_git_workspace("agents_local_runner");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "implementation",
            "--role-profile",
            "coder",
            "--message",
            "write the local worker output",
            "--command",
            "printf worker-output > worker.txt",
            "--json",
        ])
        .output()
        .expect("agents local start should execute");
    assert!(
        start_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    let start_json = output_json(&start_output);
    assert_eq!(start_json["ok"], true);
    assert_eq!(start_json["command"], "agents start");
    assert_eq!(start_json["data"]["status"], "succeeded");
    assert_eq!(start_json["data"]["task_packet"]["runner_kind"], "local");
    assert_eq!(
        start_json["data"]["runtime_record"]["lifecycle_status"],
        "succeeded"
    );
    assert_eq!(
        start_json["data"]["workspace_binding"]["mode"],
        "git_worktree"
    );

    let agent_id = start_json["data"]["agent_id"]
        .as_str()
        .expect("agent id")
        .to_string();
    let task_packet_path = start_json["data"]["task_packet_path"]
        .as_str()
        .expect("task packet path");
    assert!(
        task_packet_path.starts_with(&format!("{registry_root}/projects/")),
        "agent state should live under global state root, got {task_packet_path}"
    );
    let agent_root = Path::new(task_packet_path)
        .parent()
        .expect("task packet should be inside agent root")
        .to_path_buf();
    assert!(agent_root.join("DIRECTIVE.md").exists());
    assert!(agent_root.join("STATUS.md").exists());
    assert!(agent_root.join("stdout.txt").exists());
    assert!(agent_root.join("stderr.txt").exists());

    let worktree_path = start_json["data"]["workspace_binding"]["worktree_path"]
        .as_str()
        .expect("worktree path");
    assert!(
        worktree_path.starts_with(&format!("{registry_root}/projects/")),
        "worker worktree should live under global state root, got {worktree_path}"
    );
    assert!(Path::new(worktree_path).join("worker.txt").exists());
    assert!(!workspace_root.join("worker.txt").exists());
    assert!(
        !workspace_root.join(".pmcli").exists(),
        "worker state should not be written into the workspace"
    );

    let directive =
        std::fs::read_to_string(agent_root.join("DIRECTIVE.md")).expect("directive should read");
    assert!(directive.contains("directive_id:"));
    assert!(directive.contains("role_profile: \"coder\""));
    assert!(directive.contains("## Task"));
    assert!(directive.contains("## Astra Collaboration Protocol"));
    assert!(directive.contains("astra_main_runtime_worker_v1"));
    assert!(directive.contains("## Worker Authority Boundary"));
    assert!(directive.contains("Do not publish board tasks"));

    let status = std::fs::read_to_string(agent_root.join("STATUS.md")).expect("status should read");
    assert!(status.contains("status: \"completed\""));
    assert!(status.contains("exit_reason: \"task_complete\""));

    let trace_events = start_json["data"]["trace"]["runtime_events"]
        .as_array()
        .expect("runtime events");
    for expected in [
        "agent_spawn_requested",
        "agent_ready",
        "agent_task_bound",
        "agent_completed",
    ] {
        assert!(
            trace_events.iter().any(|event| event["event"] == expected),
            "missing runtime event {expected}"
        );
    }

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "inspect", &agent_id, "--json"])
        .output()
        .expect("agents inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(
        inspect_json["data"]["workspace_binding"]["mode"],
        "git_worktree"
    );
    assert_eq!(inspect_json["data"]["task_packet"]["runner_kind"], "local");
}

#[test]
fn agents_local_runner_overlays_dirty_source_worktree_for_worker_visibility() {
    let workspace_root = init_real_git_workspace("agents_dirty_source");
    let registry_root = registry_root_env();
    write_text_file(
        workspace_root.join("README.md"),
        "# test workspace\n\nuncommitted change\n",
    );
    write_text_file(
        workspace_root.join("notes").join("todo.txt"),
        "scratch note\n",
    );

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "implementation",
            "--role-profile",
            "coder",
            "--message",
            "dirty files should be visible",
            "--command",
            "grep -q 'uncommitted change' README.md && grep -q 'scratch note' notes/todo.txt",
            "--json",
        ])
        .output()
        .expect("agents local start should execute");
    assert!(
        start_output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&start_output.stderr),
        String::from_utf8_lossy(&start_output.stdout)
    );
    let start_json = output_json(&start_output);
    assert_eq!(
        start_json["data"]["workspace_binding"]["baseline_mode"],
        "dirty_worktree_overlay"
    );
    assert_eq!(
        start_json["data"]["workspace_binding"]["dirty_overlay_ref"]
            .as_str()
            .map(|value| value.contains("source_overlay/manifest.json")),
        Some(true)
    );
    let worktree_path = start_json["data"]["workspace_binding"]["worktree_path"]
        .as_str()
        .expect("worktree path");
    assert!(Path::new(worktree_path).join("README.md").exists());
    assert!(Path::new(worktree_path).join("notes/todo.txt").exists());
    let output_manifest_ref = start_json["data"]["output_manifest_path"]
        .as_str()
        .expect("output manifest path");
    let agent_root = Path::new(output_manifest_ref)
        .parent()
        .expect("agent root should exist");
    assert!(agent_root.join("worker_workspace_baseline.json").exists());
}

#[test]
fn agents_local_runner_rejects_invalid_stale_and_unauthorized_task_packets() {
    let workspace_root = init_real_git_workspace("agents_packet_validation");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let valid_packet = json!({
        "schema_version": "v1alpha1",
        "task_packet_id": "task_external_validation",
        "agent_id": "agent_external_validation",
        "runner_kind": "local",
        "intent": "implementation",
        "role_profile": "coder",
        "retention_policy": "ephemeral",
        "run_class": "bounded",
        "io_mode": "request_response",
        "resume_policy": "fresh_thread",
        "replay_seed_ref": "",
        "budget": { "max_turns": 1, "max_runtime_ms": 30000 },
        "scope": {
            "workspace_root": workspace_root.display().to_string(),
            "allowed_paths": ["."]
        },
        "write_authority": "workspace_write",
        "success_criteria": ["packet validated before execution"],
        "output_manifest_required": true,
        "review_gate_required": true,
        "command": "true",
        "message": "external packet validation",
        "created_at": "1"
    });

    let invalid_packet_path = workspace_root.join(".pmcli/test_packets/invalid_packet.json");
    write_text_file(&invalid_packet_path, "{ not json");
    let invalid_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--task-packet",
            ".pmcli/test_packets/invalid_packet.json",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert_eq!(invalid_output.status.code(), Some(9));
    let invalid_json = output_json(&invalid_output);
    assert_eq!(invalid_json["error"]["code"], "agent_invalid_task_packet");

    let stale_packet_path = workspace_root.join(".pmcli/test_packets/stale_packet.json");
    let mut stale_packet = valid_packet.clone();
    stale_packet["agent_id"] = Value::String("agent_external_validation_stale".to_string());
    stale_packet["task_packet_id"] = Value::String("task_external_validation_stale".to_string());
    stale_packet["scope"]["workspace_root"] = Value::String("/tmp/outside-stale-scope".to_string());
    write_text_file(
        &stale_packet_path,
        &serde_json::to_string_pretty(&stale_packet).expect("packet should serialize"),
    );
    let stale_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--task-packet",
            ".pmcli/test_packets/stale_packet.json",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert_eq!(stale_output.status.code(), Some(9));
    let stale_json = output_json(&stale_output);
    assert_eq!(stale_json["error"]["code"], "agent_stale_scope");
    assert_eq!(stale_json["data"]["failure_code"], "stale_scope");
    let stale_agent_id = stale_packet["agent_id"]
        .as_str()
        .expect("stale agent id should be string");

    let stale_inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "inspect", stale_agent_id, "--json"])
        .output()
        .expect("agents inspect should execute");
    assert!(
        stale_inspect_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stale_inspect_output.stderr)
    );
    let stale_inspect_json = output_json(&stale_inspect_output);
    assert_eq!(
        stale_inspect_json["data"]["runtime_record"]["lifecycle_status"],
        "launch_failed"
    );
    assert_eq!(
        stale_inspect_json["data"]["runtime_record"]["failure_code"],
        "stale_scope"
    );
    assert_eq!(
        stale_inspect_json["data"]["trace"]["final_status"],
        "launch_failed"
    );
    assert_eq!(
        stale_inspect_json["data"]["output_manifest"]["status"],
        "failed"
    );

    let unauthorized_packet_path =
        workspace_root.join(".pmcli/test_packets/unauthorized_packet.json");
    let mut unauthorized_packet = valid_packet;
    unauthorized_packet["agent_id"] =
        Value::String("agent_external_validation_unauthorized".to_string());
    unauthorized_packet["task_packet_id"] =
        Value::String("task_external_validation_unauthorized".to_string());
    unauthorized_packet["write_authority"] = Value::String("none".to_string());
    write_text_file(
        &unauthorized_packet_path,
        &serde_json::to_string_pretty(&unauthorized_packet).expect("packet should serialize"),
    );
    let unauthorized_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--task-packet",
            ".pmcli/test_packets/unauthorized_packet.json",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert_eq!(unauthorized_output.status.code(), Some(9));
    let unauthorized_json = output_json(&unauthorized_output);
    assert_eq!(
        unauthorized_json["error"]["code"],
        "agent_missing_write_authority"
    );
    assert_eq!(
        unauthorized_json["data"]["failure_code"],
        "missing_write_authority"
    );

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "list", "--json"])
        .output()
        .expect("agents list should execute");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_json = output_json(&list_output);
    let agents = list_json["data"]["agents"]
        .as_array()
        .expect("agents list should contain array");
    assert!(agents.iter().any(|agent| {
        agent["agent_id"] == "agent_external_validation_stale"
            && agent["lifecycle_status"] == "launch_failed"
    }));
    assert!(agents.iter().any(|agent| {
        agent["agent_id"] == "agent_external_validation_unauthorized"
            && agent["lifecycle_status"] == "launch_failed"
    }));
}

#[test]
fn agents_provider_runner_accepts_runtime_task_packet_absolute_path() {
    let workspace_root = init_real_git_workspace("agents_provider_runtime_packet_path");
    let registry_root = registry_root_env();
    let init_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["projects", "init", "--json"],
    );
    let data_dir = Path::new(
        init_json["data"]["registry_entry"]["data_dir"]
            .as_str()
            .expect("registry data dir"),
    );

    let runtime_packet_path = data_dir.join("agents/agent_provider_runtime/task_packet.json");
    write_text_file(&runtime_packet_path, "{ not json");
    let runtime_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "provider",
            "--task-packet",
            runtime_packet_path.to_str().expect("runtime packet path"),
            "--provider",
            "openai",
            "--model",
            "gpt-5.4",
            "--json",
        ])
        .output()
        .expect("agents provider start should execute");
    assert_eq!(runtime_output.status.code(), Some(9));
    let runtime_json = output_json(&runtime_output);
    assert_eq!(runtime_json["error"]["code"], "agent_invalid_task_packet");
    assert!(
        runtime_json["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("invalid task packet"),
        "runtime data-dir packet should reach task packet parsing, stdout: {}",
        String::from_utf8_lossy(&runtime_output.stdout)
    );

    let outside_packet_path =
        unique_temp_dir("agents_provider_outside_packet").join("task_packet.json");
    write_text_file(&outside_packet_path, "{ not json");
    let outside_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "provider",
            "--task-packet",
            outside_packet_path.to_str().expect("outside packet path"),
            "--provider",
            "openai",
            "--model",
            "gpt-5.4",
            "--json",
        ])
        .output()
        .expect("agents provider start should execute");
    assert_eq!(outside_output.status.code(), Some(9));
    let outside_json = output_json(&outside_output);
    assert_eq!(outside_json["error"]["code"], "agent_invalid_task_packet");
    assert!(
        outside_json["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("absolute paths must be inside the current project runtime data dir"),
        "outside absolute packet should stay blocked, stdout: {}",
        String::from_utf8_lossy(&outside_output.stdout)
    );
}

#[test]
fn agents_local_runner_persists_failed_command_lifecycle() {
    let workspace_root = init_real_git_workspace("agents_local_runner_failed");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "implementation",
            "--role-profile",
            "coder",
            "--message",
            "fail with persisted lifecycle",
            "--command",
            "printf failure-detail >&2; exit 7",
            "--json",
        ])
        .output()
        .expect("agents local start should execute");
    assert!(
        start_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    let start_json = output_json(&start_output);
    assert_eq!(start_json["data"]["status"], "failed");
    assert_eq!(
        start_json["data"]["runtime_record"]["lifecycle_status"],
        "failed"
    );
    assert_eq!(
        start_json["data"]["runtime_record"]["failure_code"],
        "exit_7"
    );
    assert_eq!(start_json["data"]["output_manifest"]["status"], "failed");
    assert_eq!(start_json["data"]["trace"]["final_status"], "failed");

    let agent_id = start_json["data"]["agent_id"].as_str().expect("agent id");
    let agent_root = workspace_root.join(".pmcli/agents").join(agent_id);
    let status = std::fs::read_to_string(agent_root.join("STATUS.md")).expect("status should read");
    let stderr =
        std::fs::read_to_string(agent_root.join("stderr.txt")).expect("stderr should read");
    assert!(status.contains("status: \"failed\""));
    assert!(status.contains("error_message: \"exit_7\""));
    assert_eq!(stderr, "failure-detail");
}

#[test]
fn agents_local_runner_records_timeout_crash_and_validated_manifest_lifecycle() {
    let workspace_root = init_real_git_workspace("agents_local_runner_timeout_crash");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let timeout_packet = json!({
        "schema_version": "v1alpha1",
        "task_packet_id": "task_timeout",
        "agent_id": "agent_timeout",
        "runner_kind": "local",
        "intent": "implementation",
        "role_profile": "coder",
        "retention_policy": "ephemeral",
        "run_class": "bounded",
        "io_mode": "request_response",
        "resume_policy": "fresh_thread",
        "replay_seed_ref": "",
        "budget": { "max_turns": 1, "max_runtime_ms": 50 },
        "scope": {
            "workspace_root": workspace_root.display().to_string(),
            "allowed_paths": ["."]
        },
        "write_authority": "workspace_write",
        "success_criteria": ["timeout is recorded"],
        "output_manifest_required": true,
        "review_gate_required": true,
        "command": "sleep 2",
        "message": "prove timeout lifecycle",
        "created_at": "1"
    });
    write_text_file(
        workspace_root.join(".pmcli/test_packets/timeout_packet.json"),
        &serde_json::to_string_pretty(&timeout_packet).expect("packet should serialize"),
    );
    let timeout_started = Instant::now();
    let timeout_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--task-packet",
            ".pmcli/test_packets/timeout_packet.json",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    let timeout_elapsed = timeout_started.elapsed();
    assert!(
        timeout_elapsed < Duration::from_secs(2),
        "timeout runner should return before the full sleep duration; elapsed={timeout_elapsed:?}"
    );
    assert!(
        timeout_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&timeout_output.stderr)
    );
    let timeout_json = output_json(&timeout_output);
    assert_eq!(timeout_json["data"]["status"], "timeout");
    assert_eq!(
        timeout_json["data"]["runtime_record"]["lifecycle_status"],
        "timeout"
    );
    assert_eq!(
        timeout_json["data"]["runtime_record"]["failure_code"],
        "timeout"
    );
    assert_eq!(timeout_json["data"]["output_manifest"]["status"], "failed");
    assert_eq!(
        timeout_json["data"]["output_manifest"]["validation_status"],
        "valid"
    );
    assert_eq!(timeout_json["data"]["trace"]["final_status"], "timeout");
    assert!(timeout_json["data"]["trace"]["runtime_events"]
        .as_array()
        .expect("runtime events")
        .iter()
        .any(|event| event["event"] == "agent_timeout"));
    assert!(timeout_json["data"]["trace"]["output_records"]
        .as_array()
        .expect("output records")
        .iter()
        .any(|event| event["event"] == "output_manifest_validated"));

    let crash_packet = json!({
        "schema_version": "v1alpha1",
        "task_packet_id": "task_crash",
        "agent_id": "agent_crash",
        "runner_kind": "local",
        "intent": "implementation",
        "role_profile": "coder",
        "retention_policy": "ephemeral",
        "run_class": "bounded",
        "io_mode": "request_response",
        "resume_policy": "fresh_thread",
        "replay_seed_ref": "",
        "budget": { "max_turns": 1, "max_runtime_ms": 30000 },
        "scope": {
            "workspace_root": workspace_root.display().to_string(),
            "allowed_paths": ["."]
        },
        "write_authority": "workspace_write",
        "success_criteria": ["crash is recorded"],
        "output_manifest_required": true,
        "review_gate_required": true,
        "command": "kill -TERM $$",
        "message": "prove crash lifecycle",
        "created_at": "2"
    });
    write_text_file(
        workspace_root.join(".pmcli/test_packets/crash_packet.json"),
        &serde_json::to_string_pretty(&crash_packet).expect("packet should serialize"),
    );
    let crash_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--task-packet",
            ".pmcli/test_packets/crash_packet.json",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert!(
        crash_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&crash_output.stderr)
    );
    let crash_json = output_json(&crash_output);
    assert_eq!(crash_json["data"]["status"], "failed");
    assert_eq!(
        crash_json["data"]["runtime_record"]["failure_code"],
        "signal_15"
    );
    assert!(crash_json["data"]["trace"]["runtime_events"]
        .as_array()
        .expect("runtime events")
        .iter()
        .any(|event| event["event"] == "agent_crashed"));
    assert_eq!(
        crash_json["data"]["output_manifest"]["validation_status"],
        "valid"
    );
}

#[test]
fn agents_replay_starts_new_local_agent_from_prior_task_packet() {
    let workspace_root = init_real_git_workspace("agents_replay");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "implementation",
            "--role-profile",
            "coder",
            "--message",
            "create replay output",
            "--command",
            "printf replay-output > replay.txt",
            "--json",
        ])
        .output()
        .expect("agents local start should execute");
    assert!(start_output.status.success());
    let start_json = output_json(&start_output);
    let original_agent_id = start_json["data"]["agent_id"]
        .as_str()
        .expect("agent id")
        .to_string();

    let replay_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "replay", &original_agent_id, "--json"])
        .output()
        .expect("agents replay should execute");
    assert!(
        replay_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let replay_json = output_json(&replay_output);
    assert_eq!(replay_json["command"], "agents replay");
    assert_eq!(replay_json["data"]["status"], "succeeded");
    assert_ne!(replay_json["data"]["agent_id"], original_agent_id);
    assert_eq!(
        replay_json["data"]["task_packet"]["resume_policy"],
        "replay"
    );
    assert_eq!(
        replay_json["data"]["task_packet"]["replay_seed_ref"],
        original_agent_id
    );
    assert!(replay_json["data"]["trace"]["runtime_events"]
        .as_array()
        .expect("runtime events")
        .iter()
        .any(|event| event["event"] == "agent_replay_bound"));
    let replay_worktree = replay_json["data"]["workspace_binding"]["worktree_path"]
        .as_str()
        .expect("worktree path");
    assert!(Path::new(replay_worktree).join("replay.txt").exists());
}

#[test]
fn agents_replay_reports_replay_command_on_invalid_source_packet() {
    let workspace_root = init_git_workspace("agents_replay_invalid_source");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--mock",
            "--intent",
            "review",
            "--role-profile",
            "reviewer",
            "--message",
            "mock source cannot be replayed by local replay",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert!(start_output.status.success());
    let start_json = output_json(&start_output);
    let mock_agent_id = start_json["data"]["agent_id"]
        .as_str()
        .expect("agent id")
        .to_string();

    let replay_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "replay", &mock_agent_id, "--json"])
        .output()
        .expect("agents replay should execute");
    assert_eq!(replay_output.status.code(), Some(9));
    let replay_json = output_json(&replay_output);
    assert_eq!(replay_json["command"], "agents replay");
    assert_eq!(replay_json["error"]["code"], "agent_invalid_task_packet");
}

#[test]
fn goals_mission_frame_persists_and_survives_prompt_resume_continue_and_compact() {
    let workspace_root = init_git_workspace("goals_mission_frame");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a schema-first research CLI that exceeds reference projects on implemented operator workflows.",
            "--milestone-goal",
            "Graduate MissionFrame context retention.",
            "--current-implementation-goal",
            "Persist and inject mission frame projections.",
            "--non-goal",
            "Do not graduate full durable memory in this slice.",
            "--success-criterion",
            "prompt, resume, continue, and compact expose the same mission_frame_ref.",
            "--evidence-ref",
            "docs/deep_study/48-mission-frame-context-and-compact-policy.md",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );
    let set_json = output_json(&set_output);
    assert_eq!(set_json["ok"], true);
    assert_eq!(set_json["command"], "goals set");
    assert_eq!(set_json["data"]["status"], "active");
    assert_eq!(set_json["data"]["mission_frame"]["version"], 1);
    let frame_ref = set_json["data"]["mission_frame_ref"]
        .as_str()
        .expect("mission frame ref")
        .to_string();
    assert!(frame_ref.ends_with(".pmcli/project_goals/mission_frame.json"));
    assert!(workspace_root
        .join(".pmcli/project_goals/mission_frame.json")
        .exists());

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "status", "--json"])
        .output()
        .expect("goals status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(status_json["command"], "goals status");
    assert_eq!(status_json["data"]["status"], "active");
    assert_eq!(
        status_json["data"]["projection"]["priority_rule"],
        "project_max_goal > milestone_goal > current_implementation_goal"
    );
    assert_eq!(
        status_json["data"]["task_pool"]["schema_version"],
        "goal_task_pool_snapshot.v1"
    );
    assert_eq!(
        status_json["data"]["task_pool"]["summary"]["total"],
        status_json["data"]["task_pool"]["entries"]
            .as_array()
            .expect("task pool entries")
            .len()
    );
    assert_eq!(
        status_json["data"]["watch_plan"]["schema_version"],
        "goal_watch_plan_projection.v1"
    );
    assert_eq!(
        status_json["data"]["watch_plan"]["recommended_runner"],
        "local_private_schedule"
    );
    assert_eq!(
        status_json["data"]["stage_decision_policy"]["schema_version"],
        "goal_stage_decision_policy_projection.v1"
    );
    assert_eq!(
        status_json["data"]["stage_decision_policy"]["status"],
        "clear"
    );

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(
        prompt_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&prompt_output.stderr)
    );
    let prompt_json = output_json(&prompt_output);
    assert!(prompt_json["data"]["assistant_content"]
        .as_str()
        .expect("prompt turn should expose assistant content")
        .contains("status"));
    assert_eq!(
        prompt_json["data"]["mission_frame"]["mission_frame_ref"],
        frame_ref
    );
    assert_eq!(
        prompt_json["data"]["mission_frame"]["current_implementation_goal"],
        "Persist and inject mission frame projections."
    );
    let session_id = prompt_json["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let resume_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["resume", &session_id, "--json"])
        .output()
        .expect("resume should execute");
    assert!(resume_output.status.success());
    let resume_json = output_json(&resume_output);
    assert_eq!(
        resume_json["data"]["mission_frame"]["mission_frame_ref"],
        frame_ref
    );

    let continue_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["continue", "--json"])
        .output()
        .expect("continue should execute");
    assert!(continue_output.status.success());
    let continue_json = output_json(&continue_output);
    assert_eq!(
        continue_json["data"]["mission_frame"]["mission_frame_ref"],
        frame_ref
    );

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", "latest", "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let compact_json = output_json(&compact_output);
    assert_eq!(compact_json["data"]["mission_frame_ref"], frame_ref);
    assert_eq!(
        compact_json["data"]["mission_frame"]["project_max_goal"],
        "Build a schema-first research CLI that exceeds reference projects on implemented operator workflows."
    );
    let summary_ref = compact_json["data"]["summary_ref"]
        .as_str()
        .expect("summary ref should be present");
    let summary_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("summaries")
        .join(format!("{summary_ref}.md"));
    let summary_contents =
        std::fs::read_to_string(&summary_path).expect("summary artifact should be readable");
    assert!(summary_contents.contains("## MissionFrame Projection"));
    assert!(summary_contents.contains(&format!("- mission_frame_ref: {frame_ref}")));
    assert!(summary_contents.contains(
        "- priority_rule: project_max_goal > milestone_goal > current_implementation_goal"
    ));

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "mission_frame"
            && event["phase"] == "terminal"
            && event["object_id"] == "mission_frame"
    }));
}

#[test]
fn compact_goal_alignment_trace_reports_drift_for_unrelated_summary() {
    let workspace_root = init_git_workspace("goals_alignment_drift");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Deliver autonomous research CLI workflows with durable schema governance.",
            "--milestone-goal",
            "Finish MissionFrame compact drift detection.",
            "--current-implementation-goal",
            "Detect compact summaries that omit alignment terminology.",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_output.status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "drift fixture", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let mut transcript = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "user",
            "content": "Please discuss unrelated vacation packing and coffee brewing."
        })
    )
    .expect("user line should append");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "assistant",
            "content": "Pack socks, check weather, and grind beans before breakfast."
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

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    let trace = events
        .iter()
        .find(|event| {
            event["event_name"] == "goal_alignment_trace"
                && event["object_id"] == session_id
                && event["payload"]["command"] == "compact"
        })
        .expect("goal alignment trace should be emitted");
    assert_eq!(trace["payload"]["alignment_status"], "drift_risk");
    assert_eq!(trace["payload"]["implementation_goal_supported"], false);
    assert!(!trace["payload"]["drift_reasons"]
        .as_array()
        .expect("drift reasons")
        .is_empty());
    assert_eq!(
        trace["payload"]["recommended_next_action"],
        "review_compaction_against_mission_frame"
    );
}

#[test]
fn goals_status_rejects_stale_mission_frame_project_id() {
    let workspace_root = init_git_workspace("goals_stale_project_id");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Project goal",
            "--milestone-goal",
            "Milestone goal",
            "--current-implementation-goal",
            "Implementation goal",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_output.status.success());

    let frame_path = workspace_root.join(".pmcli/project_goals/mission_frame.json");
    let mut frame: Value = serde_json::from_str(
        &std::fs::read_to_string(&frame_path).expect("mission frame should read"),
    )
    .expect("mission frame should parse");
    frame["project_id"] = json!("proj_other_workspace");
    std::fs::write(
        &frame_path,
        serde_json::to_string_pretty(&frame).expect("mission frame should serialize"),
    )
    .expect("mission frame should write");

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "status", "--json"])
        .output()
        .expect("goals status should execute");
    assert_eq!(status_output.status.code(), Some(6));
    let status_json = output_json(&status_output);
    assert_eq!(status_json["ok"], false);
    assert_eq!(status_json["command"], "goals status");
    assert_eq!(status_json["error"]["code"], "mission_frame_invalid");
    assert!(status_json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("does not match resolved project"));
}

#[test]
fn goals_advance_creates_goal_run_and_advances_one_cycle() {
    let workspace_root = init_real_git_workspace("goals_advance_cycle");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a goal-driven research loop",
            "--milestone-goal",
            "Promote a real goal run",
            "--current-implementation-goal",
            "Advance one goal cycle through the runtime",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_output.status.success());

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "Goal cycle research stage",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "docs/spec.md",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );
    let record_json = output_json(&record_output);
    let stage_execution_id = record_json["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage execution id")
        .to_string();

    let advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("goals advance should execute");
    assert!(
        advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&advance_output.stderr)
    );
    let advance_json = output_json(&advance_output);
    assert_eq!(advance_json["command"], "goals advance");
    assert_eq!(
        advance_json["data"]["schema_version"],
        "goal_advance_result.v1"
    );
    assert_eq!(
        advance_json["data"]["task_pool"]["schema_version"],
        "goal_task_pool_snapshot.v1"
    );
    assert!(advance_json["data"]["run"]["run_id"].is_string());
    assert_eq!(advance_json["data"]["run"]["automation_mode"], "full_auto");
    assert_eq!(
        advance_json["data"]["dispatch"]["schema_version"],
        "goal_task_dispatch_result.v1"
    );
    assert_eq!(advance_json["data"]["dispatch"]["status"], "dispatched");
    assert_eq!(
        advance_json["data"]["dispatch"]["source_entry"]["bucket_id"],
        "ready_to_run"
    );
    assert_eq!(
        advance_json["data"]["dispatch"]["source_entry"]["action_policy"],
        "allowed"
    );
    assert!(
        advance_json["data"]["dispatch"]["source_entry"]["action_refs"]
            .as_array()
            .expect("source entry action refs")
            .iter()
            .any(|action| action["action_id"] == "advance_research_loop")
    );
    assert_eq!(
        advance_json["data"]["dispatch"]["task_pool_entry_id"],
        advance_json["data"]["dispatch"]["source_entry"]["entry_id"]
    );
    assert_eq!(
        advance_json["data"]["dispatch"]["concurrency_status"],
        "claim_granted"
    );
    assert_eq!(
        advance_json["data"]["dispatch"]["active_claim_count_before_dispatch"],
        0
    );
    assert_eq!(advance_json["data"]["dispatch"]["max_active_claims"], 3);
    assert!(advance_json["data"]["dispatch"]["claim_ref"]
        .as_str()
        .expect("claim ref")
        .starts_with("goal_task_claim::"));
    let dispatched_agent_id = advance_json["data"]["dispatch"]["agent_id"]
        .as_str()
        .expect("dispatch should include agent id")
        .to_string();
    let dispatched_entry_id = advance_json["data"]["dispatch"]["task_pool_entry_id"]
        .as_str()
        .expect("dispatch should include entry id")
        .to_string();
    let dispatched_stdout = std::fs::read_to_string(
        workspace_root
            .join(".pmcli")
            .join("agents")
            .join(&dispatched_agent_id)
            .join("stdout.txt"),
    )
    .expect("goal worker stdout should be persisted");
    assert!(dispatched_stdout.contains("Astra Goal Worker Evidence Brief"));
    assert!(dispatched_stdout.contains(&dispatched_entry_id));
    assert!(dispatched_stdout.contains("Repository Snapshot"));
    assert!(!dispatched_stdout.contains("accepted goal task entry"));
    assert!(
        advance_json["data"]["run"]["control_commands"]
            .as_array()
            .expect("control commands array")
            .len()
            >= 1
    );
    assert!(
        advance_json["data"]["task_pool"]["entries"]
            .as_array()
            .expect("task pool entries")
            .len()
            >= 1
    );
    assert!(advance_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| {
            entry["source"]["source_kind"] == "agent_task_packet"
                && entry["source"]["source_id"] == dispatched_agent_id
        }));
    assert!(advance_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| {
            entry["source"]["source_kind"] == "goal_task_claim"
                && entry["source"]["source_id"] == dispatched_agent_id
                && entry["bucket_id"] == "running"
        }));

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "list", "--json"])
        .output()
        .expect("agents list should execute");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_json = output_json(&list_output);
    assert_eq!(list_json["data"]["total_count"], 1);
    assert_eq!(
        list_json["data"]["agents"][0]["agent_id"],
        dispatched_agent_id
    );

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_output.stderr)
    );
    let surface_json = output_json(&surface_output);
    assert!(
        surface_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("research board entries")
            .iter()
            .any(|entry| {
                entry["source"]["source_kind"] == "agent_task_packet"
                    && entry["source"]["source_id"] == dispatched_agent_id
            })
    );
    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    assert!(events.contains("\"event_name\":\"goal_advance\""));
    assert!(events.contains("\"event_name\":\"goal_task_dispatch\""));

    let second_advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("second goals advance should execute");
    assert!(
        second_advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second_advance_output.stderr)
    );
    let second_advance_json = output_json(&second_advance_output);
    assert_eq!(
        second_advance_json["data"]["acceptance"]["schema_version"],
        "goal_task_acceptance_result.v1"
    );
    assert_eq!(
        second_advance_json["data"]["acceptance"]["status"],
        "accepted"
    );
    assert_eq!(
        second_advance_json["data"]["acceptance"]["task_pool_entry_id"],
        dispatched_entry_id
    );
    assert_eq!(
        second_advance_json["data"]["acceptance"]["claim_status"],
        "closed_accepted"
    );
    assert_eq!(
        second_advance_json["data"]["acceptance"]["claim_ref"],
        advance_json["data"]["dispatch"]["claim_ref"]
    );
    assert_eq!(
        second_advance_json["data"]["acceptance"]["research_stage_execution_ref"],
        stage_execution_id
    );
    assert_eq!(second_advance_json["data"]["run"]["status"], "accepted");
    assert_eq!(
        second_advance_json["data"]["run"]["steps"]
            .as_array()
            .expect("run steps")
            .iter()
            .filter(|step| step["status"] == "done")
            .count(),
        4
    );
    assert_eq!(
        second_advance_json["data"]["stage_decision"]["schema_version"],
        "goal_research_stage_decision_result.v1"
    );
    assert_eq!(
        second_advance_json["data"]["stage_decision"]["status"],
        "auto_approved_low_risk_advance"
    );
    assert_eq!(
        second_advance_json["data"]["stage_decision"]["stage_execution_id"],
        stage_execution_id.as_str()
    );
    assert_eq!(
        second_advance_json["data"]["stage_decision"]["agent_id"],
        dispatched_agent_id.as_str()
    );
    assert!(second_advance_json["data"]["run"]["steps"]
        .as_array()
        .expect("run steps")
        .iter()
        .flat_map(|step| step["artifacts"].as_array().into_iter().flatten())
        .any(|artifact| {
            artifact
                .as_str()
                .unwrap_or_default()
                .starts_with("goal_research_stage_decision_consumed::")
        }));
    assert!(!second_advance_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| entry["source"]["source_kind"] == "goal_research_stage_decision"));

    let post_surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute after acceptance");
    assert!(
        post_surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&post_surface_output.stderr)
    );
    let post_surface_json = output_json(&post_surface_output);
    assert_eq!(
        post_surface_json["data"]["projection"]["research"]["task_pool_maintenance"]
            ["schema_version"],
        "goal_task_pool_maintenance_projection.v1"
    );
    assert_eq!(
        post_surface_json["data"]["projection"]["research"]["watch_plan"]["schema_version"],
        "goal_watch_plan_projection.v1"
    );
    assert_eq!(
        post_surface_json["data"]["projection"]["research"]["stage_decision_policy"]
            ["schema_version"],
        "goal_stage_decision_policy_projection.v1"
    );
    assert!(
        !post_surface_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("research board entries")
            .iter()
            .any(|entry| entry["source"]["source_kind"] == "goal_research_stage_decision")
    );
    let research_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute after auto stage decision");
    assert!(research_status_output.status.success());
    let research_status_json = output_json(&research_status_output);
    assert_eq!(
        research_status_json["data"]["active_stage_execution"]["operation"],
        "advance"
    );
    assert_ne!(
        research_status_json["data"]["active_stage_execution"]["execution_id"],
        stage_execution_id.as_str()
    );
    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    assert!(events.contains("\"event_name\":\"goal_task_acceptance\""));
    assert!(events.contains("\"event_name\":\"goal_research_stage_decision\""));
}

#[test]
fn goals_advance_high_autonomy_keeps_stage_decision_as_approval_gate() {
    let workspace_root = init_real_git_workspace("goals_stage_decision_high_autonomy");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep high autonomy gated at research stage decisions",
            "--milestone-goal",
            "Dispatch but do not auto-approve stage decisions",
            "--current-implementation-goal",
            "Advance one gated goal cycle",
            "--automation-mode",
            "high_autonomy",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "High autonomy gated stage",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "docs/gated-stage.md",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );
    let record_json = output_json(&record_output);
    let stage_execution_id = record_json["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage execution id")
        .to_string();

    let first_advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("first goals advance should execute");
    assert!(
        first_advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first_advance_output.stderr)
    );
    let first_advance_json = output_json(&first_advance_output);
    assert_eq!(
        first_advance_json["data"]["dispatch"]["schema_version"],
        "goal_task_dispatch_result.v1"
    );

    let agent_id = first_advance_json["data"]["dispatch"]["agent_id"]
        .as_str()
        .expect("dispatch should include agent id")
        .to_string();
    let second_advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("second goals advance should execute");
    assert!(
        second_advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second_advance_output.stderr)
    );
    let second_advance_json = output_json(&second_advance_output);
    assert!(second_advance_json["data"]["acceptance"].is_null());
    assert!(second_advance_json["data"]["stage_decision"].is_null());

    let set_full_auto_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Complete gated stage after operator raises automation",
            "--milestone-goal",
            "Allow full-auto acceptance and low-risk stage continue",
            "--current-implementation-goal",
            "Consume the existing high-autonomy worker output",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set full_auto should execute");
    assert!(
        set_full_auto_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_full_auto_output.stderr)
    );

    let full_auto_advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("full-auto goals advance should execute");
    assert!(
        full_auto_advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&full_auto_advance_output.stderr)
    );
    let full_auto_advance_json = output_json(&full_auto_advance_output);
    assert_eq!(
        full_auto_advance_json["data"]["acceptance"]["agent_id"],
        agent_id.as_str()
    );
    assert_eq!(
        full_auto_advance_json["data"]["stage_decision"]["status"],
        "auto_approved_low_risk_advance"
    );
    assert_eq!(
        full_auto_advance_json["data"]["stage_decision"]["stage_execution_id"],
        stage_execution_id.as_str()
    );
    assert!(!full_auto_advance_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| entry["source"]["source_kind"] == "goal_research_stage_decision"));
}

#[test]
fn goals_advance_full_auto_dispatches_multiple_goal_tasks() {
    let workspace_root = init_real_git_workspace("goals_multi_dispatch_cli");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a multi-agent research loop",
            "--milestone-goal",
            "Dispatch multiple bounded workers",
            "--current-implementation-goal",
            "Claim several independent low-risk tasks",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "Multi-agent pending operations",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "docs/spec.md",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );
    let record_json = output_json(&record_output);
    let span_id = record_json["data"]["deliberation_span"]["span_id"]
        .as_str()
        .expect("span id");
    let span_path = workspace_root
        .join(".pmcli")
        .join("research")
        .join("spans")
        .join(format!("{span_id}.json"));
    let mut span_value: Value =
        serde_json::from_str(&std::fs::read_to_string(&span_path).expect("span should read"))
            .expect("span should parse");
    span_value["pending_operations"] = json!(["run_reference_scan", "run_patch_probe"]);
    std::fs::write(
        &span_path,
        serde_json::to_string_pretty(&span_value).expect("span should serialize"),
    )
    .expect("span should write");

    let advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("goals advance should execute");
    assert!(
        advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&advance_output.stderr)
    );
    let advance_json = output_json(&advance_output);
    let dispatches = advance_json["data"]["dispatches"]
        .as_array()
        .expect("dispatches should be an array");
    assert_eq!(dispatches.len(), 3);
    assert_eq!(
        advance_json["data"]["dispatch"]["agent_id"],
        dispatches[0]["agent_id"]
    );
    assert_eq!(dispatches[0]["active_claim_count_before_dispatch"], 0);
    assert_eq!(dispatches[1]["active_claim_count_before_dispatch"], 1);
    assert_eq!(dispatches[2]["active_claim_count_before_dispatch"], 2);
    assert!(dispatches
        .iter()
        .any(|dispatch| dispatch["task_pool_entry_id"] == "research_operation::0"));
    assert!(dispatches
        .iter()
        .any(|dispatch| dispatch["task_pool_entry_id"] == "research_operation::1"));
    assert!(dispatches
        .iter()
        .all(|dispatch| dispatch["max_active_claims"] == 3));

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "list", "--json"])
        .output()
        .expect("agents list should execute");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_json = output_json(&list_output);
    assert_eq!(list_json["data"]["total_count"], 3);
    let running_claim_count = advance_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .filter(|entry| {
            entry["source"]["source_kind"] == "goal_task_claim" && entry["bucket_id"] == "running"
        })
        .count();
    assert_eq!(running_claim_count, 3);

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_output.stderr)
    );
    let surface_json = output_json(&surface_output);
    assert_eq!(
        surface_json["data"]["projection"]["research"]["recovery_governance_status"],
        "healthy"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["open_recovery_wake_count"],
        0
    );
    let board_running_claim_count = surface_json["data"]["projection"]["research"]["board"]
        ["entries"]
        .as_array()
        .expect("research board entries")
        .iter()
        .filter(|entry| {
            entry["source"]["source_kind"] == "goal_task_claim" && entry["bucket_id"] == "running"
        })
        .count();
    assert_eq!(board_running_claim_count, 3);

    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    assert_eq!(
        events
            .matches("\"event_name\":\"goal_task_dispatch\"")
            .count(),
        3
    );

    let repeat_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("repeat goals advance should execute");
    assert!(
        repeat_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repeat_output.stderr)
    );
    let repeat_json = output_json(&repeat_output);
    assert!(repeat_json["data"]["dispatch"].is_null());
    assert!(repeat_json["data"]["dispatches"]
        .as_array()
        .expect("repeat dispatches array")
        .is_empty());
}

#[test]
fn goals_tick_runs_due_routines_and_advances_goal_loop() {
    let workspace_root = init_real_git_workspace("goals_tick_continuous_loop");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep a continuous research loop moving",
            "--milestone-goal",
            "Run background routines and goal dispatch together",
            "--current-implementation-goal",
            "Tick the research automation loop once",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "Continuous loop pending operation",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "docs/loop.md",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "create",
            "--name",
            "Goal tick routine",
            "--trigger-kind",
            "schedule",
            "--intent",
            "run scheduled background check",
            "--role-profile",
            "goal_background_worker",
            "--message",
            "run the scheduled background check",
            "--command",
            "mkdir -p .pmcli/tick-output && printf tick > .pmcli/tick-output/routine.txt",
            "--delivery-target",
            "research-board",
            "--json",
        ])
        .output()
        .expect("routines create should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let create_json = output_json(&create_output);
    let routine_id = create_json["data"]["routine"]["routine_id"]
        .as_str()
        .expect("routine id should exist")
        .to_string();

    let ingress_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "schedule",
            "--source",
            "goal-tick-test",
            "--dedupe-key",
            "tick-1",
            "--json",
        ])
        .output()
        .expect("routines ingress should execute");
    assert!(
        ingress_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingress_output.stderr)
    );

    let tick_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "tick",
            "--trigger-kind",
            "schedule",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("goals tick should execute");
    assert!(
        tick_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&tick_output.stderr)
    );
    let tick_json = output_json(&tick_output);
    assert_eq!(tick_json["command"], "goals tick");
    assert_eq!(tick_json["data"]["schema_version"], "goal_tick_result.v1");
    assert_eq!(tick_json["data"]["status"], "advanced");
    assert_eq!(tick_json["data"]["triggered_count"], 1);
    assert_eq!(tick_json["data"]["dispatch_count"], 1);
    assert_eq!(tick_json["data"]["routine_run_due"]["selected_count"], 1);
    assert_eq!(
        tick_json["data"]["routine_run_due"]["outcomes"][0]["status"],
        "triggered"
    );
    assert_eq!(
        tick_json["data"]["goal_advance"]["schema_version"],
        "goal_advance_result.v1"
    );
    assert_eq!(
        tick_json["data"]["goal_advance"]["dispatches"]
            .as_array()
            .expect("dispatches should be array")
            .len(),
        1
    );
    assert_eq!(
        tick_json["data"]["recovery_governance"]["schema_version"],
        "routine.recovery_governance_summary.v1"
    );
    assert_eq!(
        tick_json["data"]["recovery_governance"]["status"],
        "healthy"
    );
    assert_eq!(
        tick_json["data"]["recovery_governance"]["failed_trigger_count"],
        0
    );
    assert_eq!(
        tick_json["data"]["loop_closure"]["schema_version"],
        "goal_loop_closure_projection.v1"
    );
    assert_eq!(
        tick_json["data"]["loop_closure"]["automation_mode"],
        "full_auto"
    );
    assert!(
        tick_json["data"]["loop_closure"]["status"] == "waiting"
            || tick_json["data"]["loop_closure"]["status"] == "running"
            || tick_json["data"]["loop_closure"]["status"] == "needs_operator"
            || tick_json["data"]["loop_closure"]["status"] == "blocked",
        "loop_closure: {}",
        tick_json["data"]["loop_closure"]
    );
    assert_eq!(tick_json["data"]["loop_closure"]["dispatch_count"], 1);
    assert_eq!(
        tick_json["data"]["loop_closure"]["recovery_governance_status"],
        "healthy"
    );
    assert!(tick_json["data"]["loop_closure"]["main_agent_instruction"]
        .as_str()
        .expect("loop instruction")
        .contains("Main agent"));
    assert!(tick_json["data"]["goal_advance"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| entry["source"]["source_kind"] == "goal_task_claim"));

    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    assert!(events.contains("\"event_name\":\"routine_run_due\""));
    assert!(events.contains("\"event_name\":\"goal_advance\""));
    assert_eq!(
        events
            .matches("\"event_name\":\"goal_task_dispatch\"")
            .count(),
        1
    );

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_output.stderr)
    );
    let surface_json = output_json(&surface_output);
    assert!(
        surface_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("research board entries")
            .iter()
            .any(|entry| entry["source"]["source_kind"] == "goal_task_claim")
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["loop_closure"]["schema_version"],
        "goal_loop_closure_projection.v1"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["loop_closure"]["automation_mode"],
        "full_auto"
    );
    assert!(
        surface_json["data"]["projection"]["research"]["loop_closure"]["next_recommended_action"]
            .as_str()
            .expect("host loop next action")
            .len()
            > 0
    );
}

#[test]
fn goals_tick_automation_mode_changes_routine_recovery_budget() {
    let workspace_root = init_real_git_workspace("goals_tick_recovery_budget");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_routine = |routine_name: &str, command: &str| {
        let output = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args([
                "routines",
                "create",
                "--name",
                routine_name,
                "--trigger-kind",
                "schedule",
                "--intent",
                "exercise recovery policy",
                "--role-profile",
                "background_worker",
                "--message",
                "exercise recovery policy",
                "--command",
                command,
                "--delivery-target",
                "research-board",
                "--json",
            ])
            .output()
            .expect("routines create should execute");
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output_json(&output)["data"]["routine"]["routine_id"]
            .as_str()
            .expect("routine id should exist")
            .to_string()
    };

    let human_routine_id = create_routine(
        "Human-in-loop recovery routine",
        "printf human-recovery >&2; exit 7",
    );
    let full_auto_routine_id = create_routine(
        "Full auto recovery routine",
        "printf full-auto-recovery >&2; exit 7",
    );

    let set_human_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep a human in the loop",
            "--milestone-goal",
            "Recover only with manual oversight",
            "--current-implementation-goal",
            "Exercise the recovery budget",
            "--automation-mode",
            "human_in_the_loop",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_human_output.status.success());

    let human_ingress_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &human_routine_id,
            "--trigger-kind",
            "schedule",
            "--source",
            "human-mode-test",
            "--dedupe-key",
            "human-mode-1",
            "--json",
        ])
        .output()
        .expect("routines ingress should execute");
    assert!(human_ingress_output.status.success());

    let human_tick_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "tick",
            "--trigger-kind",
            "schedule",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("goals tick should execute");
    assert!(human_tick_output.status.success());
    let human_tick_json = output_json(&human_tick_output);
    let human_trigger =
        &human_tick_json["data"]["routine_run_due"]["outcomes"][0]["trigger_result"];
    assert_eq!(human_trigger["trigger"]["retry_budget_remaining"], 0);
    assert_eq!(
        human_trigger["trigger"]["recovery_decision"],
        "retry_budget_exhausted"
    );

    let set_full_auto_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep a full auto recovery loop",
            "--milestone-goal",
            "Recover with maximum automation",
            "--current-implementation-goal",
            "Exercise the full-auto recovery budget",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_full_auto_output.status.success());

    let full_auto_ingress_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &full_auto_routine_id,
            "--trigger-kind",
            "schedule",
            "--source",
            "full-auto-mode-test",
            "--dedupe-key",
            "full-auto-mode-1",
            "--json",
        ])
        .output()
        .expect("routines ingress should execute");
    assert!(full_auto_ingress_output.status.success());

    let full_auto_tick_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "tick",
            "--trigger-kind",
            "schedule",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("goals tick should execute");
    assert!(full_auto_tick_output.status.success());
    let full_auto_tick_json = output_json(&full_auto_tick_output);
    let full_auto_trigger =
        &full_auto_tick_json["data"]["routine_run_due"]["outcomes"][0]["trigger_result"];
    assert_eq!(full_auto_trigger["trigger"]["retry_budget_remaining"], 4);
    assert_eq!(
        full_auto_trigger["trigger"]["recovery_decision"],
        "retry_available_after_backoff"
    );
}

#[test]
fn goals_watch_once_runs_one_tick_and_reports_last_tick() {
    let workspace_root = init_real_git_workspace("goals_watch_once");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep watch-driven research automation moving",
            "--milestone-goal",
            "Run a local watch tick through the shared loop",
            "--current-implementation-goal",
            "Execute one watch tick",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let watch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "watch",
            "--once",
            "--interval-ms",
            "1",
            "--trigger-kind",
            "schedule",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("goals watch should execute");
    assert!(
        watch_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&watch_output.stderr)
    );
    let watch_json = output_json(&watch_output);
    assert_eq!(watch_json["command"], "goals watch");
    assert_eq!(watch_json["data"]["schema_version"], "goal_watch_result.v1");
    assert_eq!(watch_json["data"]["status"], "completed");
    assert_eq!(watch_json["data"]["ticks_completed"], 1);
    assert_eq!(watch_json["data"]["stop_reason"], "once");
    assert_eq!(watch_json["data"]["interval_ms"], 1);
    assert_eq!(
        watch_json["data"]["watch_state_path"]
            .as_str()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str()),
        Some("goal_watch_state.json")
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["schema_version"],
        "goal_tick_result.v1"
    );
    assert_eq!(watch_json["data"]["last_tick"]["status"], "advanced");
    assert_eq!(
        watch_json["data"]["last_tick"]["goal_advance"]["schema_version"],
        "goal_advance_result.v1"
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["recovery_governance"]["status"],
        "healthy"
    );
    assert!(Path::new(
        watch_json["data"]["watch_lock_path"]
            .as_str()
            .expect("watch lock path should be string")
    )
    .exists());
    let watch_state_path = watch_json["data"]["watch_state_path"]
        .as_str()
        .expect("watch state path should be string");
    assert!(Path::new(watch_state_path).exists());
    let watch_state: Value = serde_json::from_str(
        &std::fs::read_to_string(watch_state_path).expect("watch state should read"),
    )
    .expect("watch state should parse");
    assert_eq!(
        watch_state["schema_version"],
        "goal_watch_health_projection.v1"
    );
    assert_eq!(watch_state["status"], "completed");
    assert_eq!(watch_state["ticks_completed"], 1);
    assert_eq!(watch_state["stop_reason"], "once");
    assert_eq!(
        watch_state["loop_closure"]["schema_version"],
        "goal_loop_closure_projection.v1"
    );

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_output.stderr)
    );
    let surface_json = output_json(&surface_output);
    assert_eq!(
        surface_json["data"]["projection"]["research"]["goal_watch"]["schema_version"],
        "goal_watch_health_projection.v1"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["goal_watch"]["status"],
        "completed"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["task_pool_maintenance"]["schema_version"],
        "goal_task_pool_maintenance_projection.v1"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["watch_plan"]["schema_version"],
        "goal_watch_plan_projection.v1"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["watch_plan"]["install_mode"],
        "dry_run_plan_only"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["stage_decision_policy"]["schema_version"],
        "goal_stage_decision_policy_projection.v1"
    );

    let watch_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "plan", "--json"])
        .output()
        .expect("goals watch plan should execute");
    assert!(
        watch_plan_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&watch_plan_output.stderr)
    );
    let watch_plan_json = output_json(&watch_plan_output);
    assert_eq!(watch_plan_json["command"], "goals watch plan");
    assert_eq!(
        watch_plan_json["data"]["schema_version"],
        "goal_watch_plan_projection.v1"
    );
    assert!(watch_plan_json["data"]["cron_line"]
        .as_str()
        .expect("cron line")
        .contains("goals watch"));

    let install_dry_run_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "install", "--dry-run", "--json"])
        .output()
        .expect("goals watch install dry-run should execute");
    assert!(
        install_dry_run_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install_dry_run_output.stderr)
    );
    let install_dry_run_json = output_json(&install_dry_run_output);
    assert_eq!(install_dry_run_json["command"], "goals watch install");
    assert_eq!(
        install_dry_run_json["data"]["schema_version"],
        "goal_watch_install_result.v1"
    );
    assert_eq!(install_dry_run_json["data"]["status"], "dry_run");
    assert_eq!(install_dry_run_json["data"]["dry_run"], true);
    assert_eq!(
        install_dry_run_json["data"]["files_written"]
            .as_array()
            .expect("files_written array")
            .len(),
        0
    );
    let install_record_path = install_dry_run_json["data"]["record_path"]
        .as_str()
        .expect("install record path");
    assert!(!Path::new(install_record_path).exists());

    let install_status_before_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "status", "--json"])
        .output()
        .expect("goals watch status should execute");
    assert!(
        install_status_before_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install_status_before_output.stderr)
    );
    let install_status_before_json = output_json(&install_status_before_output);
    assert_eq!(install_status_before_json["command"], "goals watch status");
    assert_eq!(
        install_status_before_json["data"]["schema_version"],
        "goal_watch_install_status.v1"
    );
    assert_eq!(
        install_status_before_json["data"]["status"],
        "not_installed"
    );
    assert_eq!(install_status_before_json["data"]["installed"], false);

    let install_write_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "install", "--write-files", "--json"])
        .output()
        .expect("goals watch install write-files should execute");
    assert!(
        install_write_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install_write_output.stderr)
    );
    let install_write_json = output_json(&install_write_output);
    assert_eq!(install_write_json["data"]["status"], "installed");
    assert_eq!(install_write_json["data"]["write_files"], true);
    assert!(Path::new(
        install_write_json["data"]["record_path"]
            .as_str()
            .expect("record path")
    )
    .exists());
    assert!(Path::new(
        install_write_json["data"]["cron_plan_path"]
            .as_str()
            .expect("cron plan path")
    )
    .exists());
    assert!(Path::new(
        install_write_json["data"]["systemd_plan_path"]
            .as_str()
            .expect("systemd plan path")
    )
    .exists());
    assert_eq!(
        install_write_json["data"]["install_record"]["schema_version"],
        "goal_watch_install_record.v1"
    );
    assert!(install_write_json["data"]["install_record"]["command"][0]
        .as_str()
        .expect("executable command")
        .contains("research-cli"));
    assert!(install_write_json["data"]["install_record"]["cron_line"]
        .as_str()
        .expect("cron line")
        .contains("--once"));

    let install_status_after_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "status", "--json"])
        .output()
        .expect("goals watch status after install should execute");
    assert!(
        install_status_after_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install_status_after_output.stderr)
    );
    let install_status_after_json = output_json(&install_status_after_output);
    assert_eq!(install_status_after_json["data"]["status"], "installed");
    assert_eq!(install_status_after_json["data"]["installed"], true);
    assert_eq!(
        install_status_after_json["data"]["install_record"]["install_mode"],
        "project_plan_files_only"
    );

    let surface_after_install_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status after install should execute");
    assert!(
        surface_after_install_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_after_install_output.stderr)
    );
    let surface_after_install_json = output_json(&surface_after_install_output);
    assert_eq!(
        surface_after_install_json["data"]["projection"]["research"]["watch_install_status"]
            ["schema_version"],
        "goal_watch_install_status.v1"
    );
    assert_eq!(
        surface_after_install_json["data"]["projection"]["research"]["watch_install_status"]
            ["status"],
        "installed"
    );

    let uninstall_dry_run_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "uninstall", "--dry-run", "--json"])
        .output()
        .expect("goals watch uninstall dry-run should execute");
    assert!(
        uninstall_dry_run_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&uninstall_dry_run_output.stderr)
    );
    let uninstall_dry_run_json = output_json(&uninstall_dry_run_output);
    assert_eq!(uninstall_dry_run_json["data"]["status"], "dry_run");
    assert!(Path::new(
        uninstall_dry_run_json["data"]["record_path"]
            .as_str()
            .expect("record path")
    )
    .exists());

    let uninstall_write_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "uninstall", "--write-files", "--json"])
        .output()
        .expect("goals watch uninstall write-files should execute");
    assert!(
        uninstall_write_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&uninstall_write_output.stderr)
    );
    let uninstall_write_json = output_json(&uninstall_write_output);
    assert_eq!(uninstall_write_json["data"]["status"], "uninstalled");
    assert!(!Path::new(
        uninstall_write_json["data"]["record_path"]
            .as_str()
            .expect("record path")
    )
    .exists());

    let install_status_final_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "status", "--json"])
        .output()
        .expect("goals watch status after uninstall should execute");
    assert!(
        install_status_final_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&install_status_final_output.stderr)
    );
    let install_status_final_json = output_json(&install_status_final_output);
    assert_eq!(install_status_final_json["data"]["status"], "not_installed");

    let second_watch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "--once", "--interval-ms", "1", "--json"])
        .output()
        .expect("second goals watch should execute");
    assert!(
        second_watch_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second_watch_output.stderr)
    );
    let second_watch_json = output_json(&second_watch_output);
    assert_eq!(second_watch_json["data"]["ticks_completed"], 1);
    assert_eq!(second_watch_json["data"]["stop_reason"], "once");
}

#[test]
fn goals_watch_explicit_system_apply_uses_fake_backends() {
    let workspace_root = init_real_git_workspace("goals_watch_system_apply");
    let registry_root = registry_root_env();
    let fake_root = unique_temp_dir("goals_watch_system_apply_fake");
    let fake_crontab = fake_root.join("user.crontab");
    let fake_systemd_dir = fake_root.join("systemd-user");
    let fake_systemctl_log = fake_root.join("systemctl.log");
    std::fs::write(&fake_crontab, "# existing user crontab\n").expect("fake crontab should write");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep watch-driven research automation installed locally",
            "--milestone-goal",
            "Apply local watch scheduler safely",
            "--current-implementation-goal",
            "Exercise fake scheduler backends",
            "--automation-mode",
            "high_autonomy",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let cron_install_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_GOAL_WATCH_CRONTAB_FILE", &fake_crontab)
        .args(["goals", "watch", "install", "--apply-cron", "--json"])
        .output()
        .expect("goals watch install --apply-cron should execute");
    assert!(
        cron_install_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cron_install_output.stderr)
    );
    let cron_install_json = output_json(&cron_install_output);
    assert_eq!(cron_install_json["data"]["status"], "installed_cron_user");
    assert_eq!(
        cron_install_json["data"]["install_record"]["applied_runner"],
        "cron_user"
    );
    assert_eq!(
        cron_install_json["data"]["system_install"]["runner"],
        "cron_user"
    );
    assert_eq!(
        cron_install_json["data"]["system_actions"][0]["action"],
        "write_fake_crontab"
    );
    let crontab_text =
        std::fs::read_to_string(&fake_crontab).expect("fake crontab should read after install");
    assert!(crontab_text.contains("# existing user crontab"));
    assert!(crontab_text.contains("# BEGIN research-cli-goals-watch-"));
    assert!(crontab_text.contains("goals watch --once"));

    let cron_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "status", "--json"])
        .output()
        .expect("goals watch status after cron apply should execute");
    assert!(
        cron_status_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cron_status_output.stderr)
    );
    let cron_status_json = output_json(&cron_status_output);
    assert_eq!(
        cron_status_json["data"]["system_install"]["runner"],
        "cron_user"
    );
    assert_eq!(
        cron_status_json["data"]["system_install"]["status"],
        "recorded"
    );

    let cron_uninstall_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_GOAL_WATCH_CRONTAB_FILE", &fake_crontab)
        .args(["goals", "watch", "uninstall", "--apply-cron", "--json"])
        .output()
        .expect("goals watch uninstall --apply-cron should execute");
    assert!(
        cron_uninstall_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cron_uninstall_output.stderr)
    );
    let cron_uninstall_json = output_json(&cron_uninstall_output);
    assert_eq!(
        cron_uninstall_json["data"]["status"],
        "uninstalled_cron_user"
    );
    let crontab_text =
        std::fs::read_to_string(&fake_crontab).expect("fake crontab should read after uninstall");
    assert!(crontab_text.contains("# existing user crontab"));
    assert!(!crontab_text.contains("# BEGIN research-cli-goals-watch-"));

    let systemd_install_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env(
            "RESEARCH_CLI_GOAL_WATCH_SYSTEMD_USER_DIR",
            &fake_systemd_dir,
        )
        .env("RESEARCH_CLI_GOAL_WATCH_SYSTEMCTL_LOG", &fake_systemctl_log)
        .args([
            "goals",
            "watch",
            "install",
            "--apply-systemd-user",
            "--json",
        ])
        .output()
        .expect("goals watch install --apply-systemd-user should execute");
    assert!(
        systemd_install_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&systemd_install_output.stderr)
    );
    let systemd_install_json = output_json(&systemd_install_output);
    assert_eq!(
        systemd_install_json["data"]["status"],
        "installed_systemd_user"
    );
    assert_eq!(
        systemd_install_json["data"]["install_record"]["applied_runner"],
        "systemd_user"
    );
    let unit_path = systemd_install_json["data"]["system_install"]["artifact_path"]
        .as_str()
        .expect("systemd artifact path should be string")
        .to_string();
    assert!(Path::new(&unit_path).exists());
    let unit_text = std::fs::read_to_string(&unit_path).expect("unit should read");
    assert!(unit_text.contains("[Service]"));
    assert!(unit_text.contains("goals watch"));
    let systemctl_log =
        std::fs::read_to_string(&fake_systemctl_log).expect("systemctl log should read");
    assert!(systemctl_log.contains("systemctl --user daemon-reload"));
    assert!(systemctl_log.contains("systemctl --user enable --now"));

    let systemd_uninstall_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env(
            "RESEARCH_CLI_GOAL_WATCH_SYSTEMD_USER_DIR",
            &fake_systemd_dir,
        )
        .env("RESEARCH_CLI_GOAL_WATCH_SYSTEMCTL_LOG", &fake_systemctl_log)
        .args([
            "goals",
            "watch",
            "uninstall",
            "--apply-systemd-user",
            "--json",
        ])
        .output()
        .expect("goals watch uninstall --apply-systemd-user should execute");
    assert!(
        systemd_uninstall_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&systemd_uninstall_output.stderr)
    );
    let systemd_uninstall_json = output_json(&systemd_uninstall_output);
    assert_eq!(
        systemd_uninstall_json["data"]["status"],
        "uninstalled_systemd_user"
    );
    assert!(!Path::new(&unit_path).exists());
    let systemctl_log =
        std::fs::read_to_string(&fake_systemctl_log).expect("systemctl log should read");
    assert!(systemctl_log.contains("systemctl --user disable --now"));
}

#[test]
fn goals_watch_stops_when_loop_closure_blocks_continuation() {
    let workspace_root = init_real_git_workspace("goals_watch_loop_closure_stop");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Keep watch-driven research automation moving",
            "--milestone-goal",
            "Stop watch when the loop closure asks for pause",
            "--current-implementation-goal",
            "Close the loop when the task pool is blocked",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "Watch loop closure pending operation",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "docs/watch-loop.md",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "create",
            "--name",
            "Goal watch routine",
            "--trigger-kind",
            "schedule",
            "--intent",
            "run watch loop background check",
            "--role-profile",
            "goal_background_worker",
            "--message",
            "run the watch loop background check",
            "--command",
            "mkdir -p .pmcli/watch-output && printf watch > .pmcli/watch-output/routine.txt",
            "--delivery-target",
            "research-board",
            "--json",
        ])
        .output()
        .expect("routines create should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let create_json = output_json(&create_output);
    let routine_id = create_json["data"]["routine"]["routine_id"]
        .as_str()
        .expect("routine id should exist")
        .to_string();

    let ingress_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "schedule",
            "--source",
            "goal-watch-test",
            "--dedupe-key",
            "watch-1",
            "--json",
        ])
        .output()
        .expect("routines ingress should execute");
    assert!(
        ingress_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingress_output.stderr)
    );

    let watch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "watch",
            "--interval-ms",
            "1",
            "--trigger-kind",
            "schedule",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("goals watch should execute");
    assert!(
        watch_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&watch_output.stderr)
    );
    let watch_json = output_json(&watch_output);
    assert_eq!(watch_json["command"], "goals watch");
    assert_eq!(watch_json["data"]["schema_version"], "goal_watch_result.v1");
    assert_eq!(watch_json["data"]["status"], "stopped");
    assert!(
        watch_json["data"]["ticks_completed"]
            .as_u64()
            .expect("ticks_completed should be numeric")
            >= 1
    );
    assert_eq!(watch_json["data"]["stop_reason"], "agent_team_running");
    assert_eq!(
        watch_json["data"]["last_tick"]["schema_version"],
        "goal_tick_result.v1"
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["loop_closure"]["status"],
        "waiting"
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["loop_closure"]["should_continue"],
        false
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["loop_closure"]["next_recommended_action"],
        "wait_for_agent_task_output"
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["goal_advance"]["task_pool"]["summary"]["running"],
        2
    );
}

#[test]
fn goals_watch_reports_retryable_busy_when_lock_is_held() {
    let workspace_root = init_git_workspace("goals_watch_busy");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let lock_path = workspace_root
        .join(".pmcli")
        .join("goals")
        .join("goals_watch.lock");
    std::fs::create_dir_all(
        lock_path
            .parent()
            .expect("watch lock path should have parent"),
    )
    .expect("watch lock parent should be creatable");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("watch lock file should open");
    let lock_rc = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(lock_rc, 0);

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "watch", "--once", "--json"])
        .output()
        .expect("goals watch should execute");
    assert!(!output.status.success());
    let json = output_json(&output);
    assert_eq!(output.status.code(), Some(11));
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "goals watch");
    assert_eq!(json["error"]["code"], "goal_watch_busy");
    assert_eq!(json["error"]["retryable"], true);
    assert_eq!(json["data"]["lock_path"], lock_path.display().to_string());

    let _ = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN) };
}

#[test]
fn goals_advance_goal_task_claim_blocks_duplicate_dispatch_until_acceptance() {
    let workspace_root = init_git_workspace("goals_claim_blocks_duplicate_cli");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a claim-safe goal loop",
            "--milestone-goal",
            "Prevent duplicate goal task dispatch",
            "--current-implementation-goal",
            "Keep a claimed task from being dispatched twice",
            "--automation-mode",
            "high_autonomy",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let entry_id = "automation_policy::high_autonomy::dispatch_agent_task";
    let run = OrchestrationRun {
        schema_version: "orchestration.run.v1".to_string(),
        run_id: "run_goal_claim_blocks_duplicate_cli".to_string(),
        objective: "Prevent duplicate goal task dispatch".to_string(),
        status: "running".to_string(),
        start_policy: "high_autonomy".to_string(),
        automation_mode: GoalAutomationMode::HighAutonomy,
        steps: vec![
            OrchestrationStep::new(
                "goal_scope",
                "Confirm goal boundary and success criteria",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_task_pool",
                "Derive goal task pool",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_dispatch",
                "Dispatch or request approval for the next eligible agent task",
                OrchestrationStepStatus::Running,
            )
            .with_artifact(format!("goal_task_claim::{entry_id}::agent:agent_existing")),
            OrchestrationStep::new(
                "goal_acceptance",
                "Review agent outputs",
                OrchestrationStepStatus::Pending,
            ),
        ],
        control_commands: Vec::new(),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
    };
    save_run(&workspace_root.join(".pmcli"), &run).expect("run should save");

    let advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("goals advance should execute");
    assert!(
        advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&advance_output.stderr)
    );
    let advance_json = output_json(&advance_output);
    assert!(advance_json["data"]["dispatch"].is_null());
    assert!(advance_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| {
            entry["source"]["source_kind"] == "goal_task_claim"
                && entry["source"]["source_id"] == "agent_existing"
                && entry["bucket_id"] == "running"
        }));
    assert!(advance_json["data"]["run"]["steps"]
        .as_array()
        .expect("run steps")
        .iter()
        .flat_map(|step| { step["continuation_points"].as_array().into_iter().flatten() })
        .any(|point| {
            point
                .as_str()
                .unwrap_or_default()
                .starts_with("dispatch_wait:claim_exists:")
        }));

    let agents_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "list", "--json"])
        .output()
        .expect("agents list should execute");
    assert!(
        agents_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&agents_output.stderr)
    );
    let agents_json = output_json(&agents_output);
    assert_eq!(agents_json["data"]["total_count"], 0);
}

#[test]
fn goals_advance_full_auto_failed_worker_records_acceptance_diagnostic() {
    let workspace_root = init_real_git_workspace("goals_failed_worker_repair_cli");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a goal-driven research loop",
            "--milestone-goal",
            "Repair failed worker outputs",
            "--current-implementation-goal",
            "Record diagnostic for a failed goal worker",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "goal task dispatch: failing repair fixture",
            "--role-profile",
            "goal_worker",
            "--message",
            "produce a failed output manifest",
            "--command",
            "printf broken >&2; exit 7",
            "--json",
        ])
        .output()
        .expect("agents local start should execute");
    assert!(
        start_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    let start_json = output_json(&start_output);
    let agent_id = start_json["data"]["agent_id"]
        .as_str()
        .expect("failed agent id")
        .to_string();
    assert_eq!(
        start_json["data"]["runtime_record"]["lifecycle_status"],
        "failed"
    );

    let run = OrchestrationRun {
        schema_version: "orchestration.run.v1".to_string(),
        run_id: "run_goal_failed_acceptance_cli".to_string(),
        objective: "Record diagnostic for failed goal worker".to_string(),
        status: "running".to_string(),
        start_policy: "full_auto".to_string(),
        automation_mode: GoalAutomationMode::FullAuto,
        steps: vec![
            OrchestrationStep::new(
                "goal_scope",
                "Confirm goal boundary and success criteria",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_task_pool",
                "Derive goal task pool",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_dispatch",
                "Dispatch or request approval for the next eligible agent task",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_acceptance",
                "Review agent outputs",
                OrchestrationStepStatus::Running,
            ),
        ],
        control_commands: Vec::new(),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
    };
    save_run(&workspace_root.join(".pmcli"), &run).expect("run should save");

    let advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("goals advance should execute");
    assert!(
        advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&advance_output.stderr)
    );
    let advance_json = output_json(&advance_output);
    assert_eq!(
        advance_json["data"]["acceptance"]["schema_version"],
        "goal_task_acceptance_result.v1"
    );
    assert_eq!(advance_json["data"]["acceptance"]["status"], "blocked");
    assert_eq!(
        advance_json["data"]["acceptance"]["verdict"],
        "worker_output_repair_required"
    );
    assert_eq!(advance_json["data"]["acceptance"]["agent_id"], agent_id);
    assert_eq!(advance_json["data"]["run"]["status"], "blocked");
    let review_packet_refs_empty = advance_json["data"]["acceptance"]
        .get("review_packet_refs")
        .and_then(Value::as_array)
        .map(|refs| refs.is_empty())
        .unwrap_or(true);
    assert!(review_packet_refs_empty);
    assert_eq!(
        advance_json["data"]["acceptance"]["diagnostic_refs"]
            .as_array()
            .expect("diagnostic refs")
            .len(),
        1
    );

    let reviews_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "list", "--json"])
        .output()
        .expect("reviews list should execute");
    assert!(
        reviews_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&reviews_output.stderr)
    );
    let reviews_json = output_json(&reviews_output);
    assert_eq!(reviews_json["data"]["total_count"], 0);

    let repeat_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("repeat goals advance should execute");
    assert!(
        repeat_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repeat_output.stderr)
    );
    let repeat_json = output_json(&repeat_output);
    assert!(repeat_json["data"]["acceptance"].is_null());

    let reviews_after_repeat = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "list", "--json"])
        .output()
        .expect("reviews list should execute");
    assert!(reviews_after_repeat.status.success());
    let reviews_after_repeat_json = output_json(&reviews_after_repeat);
    assert_eq!(reviews_after_repeat_json["data"]["total_count"], 0);
}

#[test]
fn goals_advance_consumes_resolved_repair_review() {
    let workspace_root = init_real_git_workspace("goals_repair_review_resolution_cli");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a goal-driven research loop",
            "--milestone-goal",
            "Resume after resolved repair review",
            "--current-implementation-goal",
            "Consume repair review resolution and close blocked claim",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    let start_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "goal task dispatch: failing repair fixture",
            "--role-profile",
            "goal_worker",
            "--message",
            "produce a failed output manifest",
            "--command",
            "printf broken >&2; exit 7",
            "--json",
        ])
        .output()
        .expect("agents local start should execute");
    assert!(
        start_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&start_output.stderr)
    );
    let start_json = output_json(&start_output);
    let agent_id = start_json["data"]["agent_id"]
        .as_str()
        .expect("failed agent id")
        .to_string();
    let entry_id = "automation_policy::full_auto::dispatch_agent_task";
    let output_manifest_target = format!(".pmcli/agents/{agent_id}/output_manifest.json");
    let review_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--target",
            &output_manifest_target,
            "--objective",
            "Repair failed goal worker output",
            "--reviewer-role",
            "repair_reviewer",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(
        review_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&review_output.stderr)
    );
    let review_open_json = output_json(&review_output);
    let review_id = review_open_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("review id")
        .to_string();
    let run = OrchestrationRun {
        schema_version: "orchestration.run.v1".to_string(),
        run_id: "run_goal_repair_resolution_cli".to_string(),
        objective: "Consume resolved repair review".to_string(),
        status: "blocked".to_string(),
        start_policy: "full_auto".to_string(),
        automation_mode: GoalAutomationMode::FullAuto,
        steps: vec![
            OrchestrationStep::new(
                "goal_scope",
                "Confirm goal boundary and success criteria",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_task_pool",
                "Derive goal task pool",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_dispatch",
                "Dispatch or request approval for the next eligible agent task",
                OrchestrationStepStatus::Done,
            )
            .with_artifact(format!("goal_task_claim::{entry_id}::agent:{agent_id}")),
            OrchestrationStep::new(
                "goal_acceptance",
                "Review agent outputs",
                OrchestrationStepStatus::Blocked,
            )
            .with_artifact(format!("goal_repair_review::agent:{agent_id}"))
            .with_artifact(format!(
                "goal_repair_review_link::agent:{agent_id}::review:{review_id}"
            ))
            .with_artifact(format!("review_packet:{review_id}"))
            .with_artifact(format!(
                "goal_task_claim_blocked::{entry_id}::agent:{agent_id}::repair_review_opened"
            )),
        ],
        control_commands: Vec::new(),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
    };
    save_run(&workspace_root.join(".pmcli"), &run).expect("run should save");

    let resolve_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "resolve",
            &review_id,
            "--verdict",
            "pass",
            "--response",
            "safe to continue",
            "--json",
        ])
        .output()
        .expect("reviews resolve should execute");
    assert!(
        resolve_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resolve_output.stderr)
    );
    let resolve_json = output_json(&resolve_output);
    assert_eq!(resolve_json["command"], "reviews resolve");
    assert_eq!(resolve_json["data"]["status"], "resolved");
    assert_eq!(resolve_json["data"]["trace"]["verdict"], "pass");

    let consumed_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("goals advance should execute");
    assert!(
        consumed_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&consumed_output.stderr)
    );
    let consumed_json = output_json(&consumed_output);
    assert_eq!(
        consumed_json["data"]["action"],
        "consume_resolved_repair_review"
    );
    assert!(consumed_json["data"]["acceptance"].is_null());
    assert_eq!(consumed_json["data"]["run"]["status"], "running");
    let task_pool_entries = consumed_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries");
    let followup_entry = task_pool_entries
        .iter()
        .find(|entry| entry["source"]["source_kind"] == "goal_repair_followup_task")
        .expect("repair follow-up task should be projected");
    assert_eq!(followup_entry["bucket_id"], "ready_to_run");
    assert_eq!(followup_entry["action_policy"], "allowed");
    assert_eq!(followup_entry["source"]["source_id"], review_id.as_str());
    assert!(!consumed_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| {
            entry["source"]["source_kind"] == "goal_task_claim"
                && entry["source"]["source_id"] == agent_id
        }));
    let closed_ref =
        format!("goal_task_claim_closed::{entry_id}::agent:{agent_id}::repair_review_passed");
    assert!(consumed_json["data"]["run"]["steps"]
        .as_array()
        .expect("run steps")
        .iter()
        .flat_map(|step| step["artifacts"].as_array().into_iter().flatten())
        .any(|artifact| artifact == &Value::String(closed_ref.clone())));
    assert!(consumed_json["data"]["run"]["steps"]
        .as_array()
        .expect("run steps")
        .iter()
        .flat_map(|step| step["artifacts"].as_array().into_iter().flatten())
        .any(|artifact| {
            artifact
                .as_str()
                .unwrap_or_default()
                .starts_with("goal_repair_review_consumed::")
        }));

    let followup_dispatch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("repair follow-up goals advance should execute");
    assert!(
        followup_dispatch_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&followup_dispatch_output.stderr)
    );
    let followup_dispatch_json = output_json(&followup_dispatch_output);
    assert_eq!(
        followup_dispatch_json["data"]["action"],
        "start_pending_step"
    );
    assert_eq!(
        followup_dispatch_json["data"]["dispatch"]["source_entry"]["source"]["source_kind"],
        "goal_repair_followup_task"
    );
    assert_eq!(
        followup_dispatch_json["data"]["dispatch"]["source_entry"]["source"]["source_id"],
        review_id.as_str()
    );
}

#[test]
fn goal_research_loop_demo_closes_success_and_repair_paths() {
    let workspace_root = init_real_git_workspace("goal_research_loop_demo");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let _set_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "goals",
            "set",
            "--project-max-goal",
            "Prove the Hermes research loop product closure",
            "--milestone-goal",
            "Show watch, worker acceptance, stage advance, repair, and board projection",
            "--current-implementation-goal",
            "Run one product demo loop through the local CLI surfaces",
            "--automation-mode",
            "full_auto",
            "--json",
        ],
    );

    let stage_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "Hermes loop demo stage",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--evidence-ref",
            "docs/hermes-loop-demo.md",
            "--json",
        ],
    );
    let stage_execution_id = stage_json["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage execution id")
        .to_string();

    let create_routine_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "routines",
            "create",
            "--name",
            "Hermes loop demo routine",
            "--trigger-kind",
            "schedule",
            "--intent",
            "capture background loop evidence",
            "--role-profile",
            "goal_background_worker",
            "--message",
            "write background loop evidence",
            "--command",
            "mkdir -p .pmcli/hermes-demo && printf background > .pmcli/hermes-demo/routine.txt",
            "--delivery-target",
            "research-board",
            "--json",
        ],
    );
    let routine_id = create_routine_json["data"]["routine"]["routine_id"]
        .as_str()
        .expect("routine id")
        .to_string();

    let _ingress_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "schedule",
            "--source",
            "goal-loop-demo",
            "--dedupe-key",
            "goal-loop-demo-1",
            "--json",
        ],
    );

    let install_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["goals", "watch", "install", "--write-files", "--json"],
    );
    assert_eq!(install_json["data"]["status"], "installed");
    assert_eq!(
        install_json["data"]["install_record"]["install_mode"],
        "project_plan_files_only"
    );

    let watch_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "goals",
            "watch",
            "--once",
            "--interval-ms",
            "1",
            "--trigger-kind",
            "schedule",
            "--stale-after-sec",
            "120",
            "--json",
        ],
    );
    assert_eq!(watch_json["data"]["schema_version"], "goal_watch_result.v1");
    assert_eq!(watch_json["data"]["ticks_completed"], 1);
    assert_eq!(watch_json["data"]["stop_reason"], "once");
    assert_eq!(watch_json["data"]["last_tick"]["triggered_count"], 1);
    assert_eq!(watch_json["data"]["last_tick"]["dispatch_count"], 1);
    assert_eq!(
        watch_json["data"]["last_tick"]["goal_advance"]["dispatch"]["status"],
        "dispatched"
    );
    assert_eq!(
        watch_json["data"]["last_tick"]["loop_closure"]["schema_version"],
        "goal_loop_closure_projection.v1"
    );
    let agent_id = watch_json["data"]["last_tick"]["goal_advance"]["dispatch"]["agent_id"]
        .as_str()
        .expect("dispatched agent id")
        .to_string();

    let acceptance_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["goals", "advance", "--json"],
    );
    assert_eq!(
        acceptance_json["data"]["acceptance"]["schema_version"],
        "goal_task_acceptance_result.v1"
    );
    assert_eq!(acceptance_json["data"]["acceptance"]["status"], "accepted");
    assert_eq!(acceptance_json["data"]["acceptance"]["agent_id"], agent_id);
    assert_eq!(
        acceptance_json["data"]["acceptance"]["research_stage_execution_ref"],
        stage_execution_id.as_str()
    );
    assert_eq!(
        acceptance_json["data"]["stage_decision"]["status"],
        "auto_approved_low_risk_advance"
    );

    let board_after_success = run_cli_json(
        &workspace_root,
        &registry_root,
        &["research", "board", "--json"],
    );
    assert_eq!(
        board_after_success["data"]["schema_version"],
        "research_board_projection.v1"
    );
    assert!(board_after_success["data"]["entries"]
        .as_array()
        .expect("board entries")
        .iter()
        .any(|entry| entry["source"]["source_kind"] == "goal_run_automation_policy"));

    let failed_start_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "agents",
            "start",
            "--runner",
            "local",
            "--intent",
            "goal task dispatch: failing repair fixture",
            "--role-profile",
            "goal_worker",
            "--message",
            "produce a failed output manifest",
            "--command",
            "printf broken >&2; exit 7",
            "--json",
        ],
    );
    let failed_agent_id = failed_start_json["data"]["agent_id"]
        .as_str()
        .expect("failed agent id")
        .to_string();
    assert_eq!(
        failed_start_json["data"]["runtime_record"]["lifecycle_status"],
        "failed"
    );

    let repair_entry_id = "automation_policy::full_auto::dispatch_agent_task";
    let review_target = format!(".pmcli/agents/{failed_agent_id}/output_manifest.json");
    let review_open_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "reviews",
            "open",
            "--target",
            &review_target,
            "--objective",
            "Repair failed worker output inside the loop demo",
            "--reviewer-role",
            "repair_reviewer",
            "--json",
        ],
    );
    let review_id = review_open_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("repair review id")
        .to_string();
    let run = OrchestrationRun {
        schema_version: "orchestration.run.v1".to_string(),
        run_id: "run_goal_research_loop_demo_repair".to_string(),
        objective: "Prove repair governance inside the Hermes loop demo".to_string(),
        status: "blocked".to_string(),
        start_policy: "full_auto".to_string(),
        automation_mode: GoalAutomationMode::FullAuto,
        steps: vec![
            OrchestrationStep::new(
                "goal_scope",
                "Confirm goal boundary and success criteria",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_task_pool",
                "Derive goal task pool",
                OrchestrationStepStatus::Done,
            ),
            OrchestrationStep::new(
                "goal_dispatch",
                "Dispatch or request approval for the next eligible agent task",
                OrchestrationStepStatus::Done,
            )
            .with_artifact(format!(
                "goal_task_claim::{repair_entry_id}::agent:{failed_agent_id}"
            )),
            OrchestrationStep::new(
                "goal_acceptance",
                "Review agent outputs",
                OrchestrationStepStatus::Blocked,
            )
            .with_artifact(format!("goal_repair_review::agent:{failed_agent_id}"))
            .with_artifact(format!(
                "goal_repair_review_link::agent:{failed_agent_id}::review:{review_id}"
            ))
            .with_artifact(format!("review_packet:{review_id}"))
            .with_artifact(format!(
                "goal_task_claim_blocked::{repair_entry_id}::agent:{failed_agent_id}::repair_review_opened"
            )),
        ],
        control_commands: Vec::new(),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
    };
    save_run(&workspace_root.join(".pmcli"), &run).expect("repair demo run should save");

    let surface_blocked_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["host", "surface", "status", "--json"],
    );
    assert_eq!(
        surface_blocked_json["data"]["projection"]["research"]["loop_closure"]["status"],
        "blocked"
    );
    assert_eq!(
        surface_blocked_json["data"]["projection"]["research"]["task_pool_maintenance"]["status"],
        "blocked"
    );
    assert!(
        surface_blocked_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("board entries")
            .iter()
            .any(|entry| {
                entry["source"]["source_kind"] == "goal_task_claim"
                    && entry["source"]["source_id"] == failed_agent_id
                    && entry["bucket_id"] == "blocked"
            })
    );

    let resolve_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "reviews",
            "resolve",
            &review_id,
            "--verdict",
            "pass",
            "--response",
            "repair evidence accepted",
            "--json",
        ],
    );
    assert_eq!(resolve_json["data"]["status"], "resolved");

    let repair_consumed_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["goals", "advance", "--json"],
    );
    assert_eq!(
        repair_consumed_json["data"]["action"],
        "consume_resolved_repair_review"
    );
    assert!(repair_consumed_json["data"]["task_pool"]["entries"]
        .as_array()
        .expect("task pool entries")
        .iter()
        .any(|entry| {
            entry["source"]["source_kind"] == "goal_repair_followup_task"
                && entry["source"]["source_id"] == review_id
                && entry["bucket_id"] == "ready_to_run"
        }));

    let followup_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["goals", "advance", "--json"],
    );
    assert_eq!(
        followup_json["data"]["dispatch"]["source_entry"]["source"]["source_kind"],
        "goal_repair_followup_task"
    );
    assert_eq!(
        followup_json["data"]["dispatch"]["source_entry"]["source"]["source_id"],
        review_id.as_str()
    );

    let final_surface_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["host", "surface", "status", "--json"],
    );
    assert_eq!(
        final_surface_json["data"]["projection"]["research"]["watch_install_status"]
            ["schema_version"],
        "goal_watch_install_status.v1"
    );
    assert_eq!(
        final_surface_json["data"]["projection"]["research"]["goal_watch"]["schema_version"],
        "goal_watch_health_projection.v1"
    );
    assert_eq!(
        final_surface_json["data"]["projection"]["research"]["board_inspector_command"],
        json!(["research", "board", "--json"])
    );

    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    for event_name in [
        "routine_run_due",
        "goal_advance",
        "goal_task_dispatch",
        "goal_task_acceptance",
        "goal_research_stage_decision",
    ] {
        assert!(
            events.contains(&format!("\"event_name\":\"{event_name}\"")),
            "missing event {event_name} in {events}"
        );
    }
}

#[test]
fn goals_advance_high_autonomy_dispatches_but_does_not_auto_accept() {
    let workspace_root = init_git_workspace("goals_high_auto_no_acceptance_cli");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Build a goal-driven research loop",
            "--milestone-goal",
            "Promote a real goal run",
            "--current-implementation-goal",
            "Advance one goal cycle through the runtime",
            "--automation-mode",
            "high_autonomy",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_output.status.success());

    let advance_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["goals", "advance", "--json"])
        .output()
        .expect("goals advance should execute");
    assert!(
        advance_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&advance_output.stderr)
    );
    let advance_json = output_json(&advance_output);
    assert_eq!(advance_json["data"]["dispatch"]["runner_kind"], "mock");
    assert!(advance_json["data"]["acceptance"].is_null());

    let events = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("event log should read");
    assert!(events.contains("\"event_name\":\"goal_task_dispatch\""));
    assert!(!events.contains("\"event_name\":\"goal_task_acceptance\""));
}

#[test]
fn help_and_palette_surfaces_are_project_mutation_free_even_with_registered_scope() {
    let workspace_root = init_git_workspace("help_palette_mutation_free");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let event_log_path = workspace_root.join(".pmcli/events/events.jsonl");
    let baseline_events = read_json_lines(event_log_path.to_str().expect("events path")).len();

    for command in [
        vec!["help", "--json"],
        vec!["palette", "--json"],
        vec!["slash", "help", "--json"],
    ] {
        let output = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args(&command)
            .output()
            .expect("surface command should execute");

        assert!(
            output.status.success(),
            "command {:?} stderr: {}",
            command,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let final_events = read_json_lines(event_log_path.to_str().expect("events path")).len();
    assert_eq!(
        final_events, baseline_events,
        "help/palette surfaces must not append project events"
    );
}

#[test]
fn palette_json_returns_canonical_palette_surface_from_shared_registry() {
    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("palette_json_workspace"))
        .args(["palette", "--json"])
        .output()
        .expect("palette json should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "palette");
    assert_eq!(json["data"]["scope"], "cli");

    let entries = json["data"]["entries"].as_array().expect("palette entries");
    assert!(entries.iter().any(|entry| entry["command"] == "help"));
    assert!(entries.iter().any(|entry| entry["command"] == "chat"));
    assert!(entries.iter().any(|entry| entry["command"] == "model"));
    assert!(entries.iter().any(|entry| entry["command"] == "prompt"));
    assert!(entries.iter().any(|entry| entry["command"] == "usage"));
    assert!(entries.iter().any(|entry| entry["command"] == "cost"));
    assert!(entries.iter().any(|entry| entry["command"] == "stats"));
    assert!(entries.iter().any(|entry| entry["command"] == "setup"));
    assert!(entries.iter().any(|entry| entry["command"] == "mcp"));
    assert!(entries.iter().any(|entry| entry["command"] == "skills"));
    assert!(entries.iter().any(|entry| entry["command"] == "plugins"));
    assert!(entries.iter().any(|entry| entry["command"] == "hooks"));
}

#[test]
fn slash_help_json_matches_help_registry_commands() {
    let slash_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("slash_help_workspace"))
        .args(["slash", "help", "--json"])
        .output()
        .expect("slash help json should execute");

    assert!(
        slash_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&slash_output.stderr)
    );
    let slash_json = output_json(&slash_output);
    assert_eq!(slash_json["ok"], true);
    assert_eq!(slash_json["command"], "slash help");

    let help_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("slash_help_registry_workspace"))
        .args(["help", "--json"])
        .output()
        .expect("help json should execute");
    assert!(help_output.status.success());
    let help_json = output_json(&help_output);

    let slash_commands = slash_json["data"]["commands"]
        .as_array()
        .expect("slash commands");
    let help_commands = help_json["data"]["commands"]
        .as_array()
        .expect("help commands");

    for required in [
        "help", "chat", "model", "prompt", "usage", "cost", "stats", "inspect", "compact",
    ] {
        assert!(
            slash_commands.iter().any(|entry| entry["name"] == required),
            "missing {required} in slash help"
        );
        assert!(
            help_commands.iter().any(|entry| entry["name"] == required),
            "missing {required} in help"
        );
    }
}

#[test]
fn help_flag_text_surface_matches_json_registry_core_commands() {
    let help_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("help_flag_workspace"))
        .args(["--help"])
        .output()
        .expect("help flag should execute");

    assert!(
        help_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&help_output.stderr)
    );
    let help_text = stdout_text(&help_output);
    assert!(help_text.contains("astra"));
    assert!(help_text.contains("help"));
    assert!(help_text.contains("chat"));
    assert!(help_text.contains("prompt"));

    let help_json_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("help_flag_json_workspace"))
        .args(["help", "--json"])
        .output()
        .expect("help json should execute");
    let help_json = output_json(&help_json_output);
    let commands = help_json["data"]["commands"]
        .as_array()
        .expect("commands array");
    for required in ["help", "chat", "model", "prompt", "usage", "cost", "stats"] {
        assert!(
            help_text.contains(required),
            "missing {required} in text help"
        );
        assert!(commands.iter().any(|entry| entry["name"] == required));
    }
}

#[test]
fn version_flag_works_as_global_cli_surface() {
    let text_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("version_flag_workspace"))
        .args(["--version"])
        .output()
        .expect("version flag should execute");
    assert!(
        text_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&text_output.stderr)
    );
    assert_eq!(stdout_text(&text_output).trim(), "astra 0.1.0");

    let json_output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("version_flag_json_workspace"))
        .args(["--version", "--json"])
        .output()
        .expect("version json flag should execute");
    assert!(json_output.status.success());
    let version_json = output_json(&json_output);
    assert_eq!(version_json["ok"], true);
    assert_eq!(version_json["command"], "version");
    assert_eq!(version_json["data"]["name"], "astra");
    assert_eq!(version_json["data"]["version"], "0.1.0");
}

#[test]
fn prompt_json_returns_successful_turn_result_for_minimal_one_shot_lane() {
    let workspace_root = init_git_workspace("prompt_not_graduated");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "prompt");
    assert_eq!(json["data"]["outcome"], "completed");
    assert!(json["data"]["provider_trace"].is_object());
}

#[test]
fn prompt_json_returns_turn_result_with_routing_traces_and_events() {
    let workspace_root = init_git_workspace("prompt_turn_result");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "prompt");
    assert_eq!(json["data"]["outcome"], "completed");
    assert!(json["session_id"].is_string());
    assert!(json["data"]["turn_id"].is_string());
    assert!(json["data"]["project_trace"].is_object());
    assert!(json["data"]["profile_trace"].is_object());
    assert!(json["data"]["provider_trace"].is_object());
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "openai"
    );
    assert_eq!(json["data"]["provider_trace"]["resolved_model"], "gpt-5.4");

    let session_id = json["session_id"].as_str().expect("session id").to_string();
    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let transcript = std::fs::read_to_string(&transcript_path).expect("transcript should exist");
    assert!(transcript.contains("\"role\":\"user\""));
    assert!(transcript.contains("\"content\":\"status\""));
    assert!(transcript.contains("\"role\":\"assistant\""));

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    let turn_id = json["data"]["turn_id"].as_str().expect("turn id");
    assert!(events.iter().any(|event| {
        event["event_name"] == "turn"
            && event["phase"] == "start"
            && event["object_id"] == turn_id
            && event["session_id"] == session_id
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "turn"
            && event["phase"] == "terminal"
            && event["object_id"] == turn_id
            && event["session_id"] == session_id
    }));
}

#[test]
fn model_list_reports_aliases_canonical_ids_and_degraded_providers_machine_readably() {
    let workspace_root = init_git_workspace("model_list");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .args(["model", "list", "--json"])
        .output()
        .expect("model list should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "model list");
    assert_eq!(json["data"]["current_model"], "gpt-5.4");
    assert_eq!(json["data"]["current_provider"], "openai");
    let models = json["data"]["models"].as_array().expect("models array");
    assert!(models.iter().any(|entry| entry["alias"] == "smart"
        && entry["canonical_model_id"] == "gpt-5.4"
        && entry["provider_id"] == "openai"));
    assert!(models
        .iter()
        .any(|entry| entry["canonical_model_id"] == "claude-sonnet-4-6"
            && entry["provider_id"] == "anthropic"
            && entry["degraded"] == true));
    let degraded = json["data"]["degraded_providers"]
        .as_array()
        .expect("degraded providers");
    assert!(degraded.iter().any(|provider| provider == "anthropic"));
}

#[test]
fn model_current_reports_effective_model_provider_and_config_source() {
    let workspace_root = init_git_workspace("model_current");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_model": "gpt-5.4"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["model", "current", "--json"])
        .output()
        .expect("model current should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "model current");
    assert_eq!(json["data"]["provider_id"], "openai");
    assert_eq!(json["data"]["model"], "gpt-4.1-mini");
    assert_eq!(json["data"]["resolved_scope"], "project");
    assert_eq!(json["data"]["config_source"], "private");
}

#[test]
fn model_set_requires_scope_and_writes_default_model_with_config_event() {
    let workspace_root = init_git_workspace("model_set");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("model_set_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let missing_scope = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["model", "set", "gpt-5.4", "--json"])
        .output()
        .expect("model set should execute");
    assert_eq!(missing_scope.status.code(), Some(2));
    let missing_scope_json = output_json(&missing_scope);
    assert_eq!(missing_scope_json["error"]["code"], "usage_invalid");

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("model_set_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "model",
            "set",
            "gpt-4.1-mini",
            "--scope",
            "private",
            "--json",
        ])
        .output()
        .expect("model set should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "model set");
    assert_eq!(json["data"]["key"], "default_model");
    assert_eq!(json["data"]["value"], "gpt-4.1-mini");
    assert_eq!(json["data"]["value_source"], "private");
    assert_eq!(json["data"]["resolved_scope"], "project");

    let private_file = workspace_root.join(".pmcli/settings.local.json");
    assert_eq!(
        json["data"]["config_path"],
        private_file.display().to_string()
    );
    assert_eq!(
        json["data"]["workspace_root"],
        workspace_root.display().to_string()
    );
    assert_eq!(json["data"]["resolution_source"], "explicit_cwd");
    let private_json: Value = serde_json::from_str(
        &std::fs::read_to_string(&private_file).expect("private settings should exist"),
    )
    .expect("private settings should parse");
    assert_eq!(private_json["default_model"], "gpt-4.1-mini");

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "start"
            && event["object_id"] == "model set"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
            && event["object_id"] == "model set"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "config"
            && event["phase"] == "terminal"
            && event["payload"]["key"] == "default_model"
            && event["payload"]["scope"] == "private"
            && event["payload"]["config_path"] == private_file.display().to_string()
    }));
}

#[test]
fn usage_reports_zero_project_summary_with_estimated_accounting() {
    let workspace_root = init_git_workspace("usage_empty_project");
    let registry_root = registry_root_env();

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
        .args(["usage", "--json"])
        .output()
        .expect("usage should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "usage");
    assert_eq!(json["data"]["scope"], "project");
    assert_eq!(json["data"]["input_tokens"], 0);
    assert_eq!(json["data"]["output_tokens"], 0);
    assert_eq!(json["data"]["tool_calls"], 0);
    assert_eq!(json["data"]["estimated_cost"], "0.000000");
    assert_eq!(json["data"]["accounting_quality"], "estimated");
}

#[test]
fn usage_and_cost_report_estimated_project_accounting_after_prompt_turn() {
    let workspace_root = init_git_workspace("usage_after_prompt");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(prompt_output.status.success());

    let usage_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["usage", "--scope", "project", "--json"])
        .output()
        .expect("usage should execute");

    assert!(
        usage_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&usage_output.stderr)
    );
    let usage_json = output_json(&usage_output);
    assert_eq!(usage_json["ok"], true);
    assert_eq!(usage_json["command"], "usage");
    assert_eq!(usage_json["data"]["scope"], "project");
    assert!(
        usage_json["data"]["input_tokens"]
            .as_i64()
            .expect("input tokens should be integer")
            > 0
    );
    assert!(
        usage_json["data"]["output_tokens"]
            .as_i64()
            .expect("output tokens should be integer")
            > 0
    );
    assert_eq!(usage_json["data"]["tool_calls"], 0);
    assert_eq!(usage_json["data"]["accounting_quality"], "estimated");
    assert!(usage_json["data"]["estimated_cost"].is_string());

    let cost_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["cost", "--scope", "project", "--json"])
        .output()
        .expect("cost should execute");

    assert!(
        cost_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cost_output.stderr)
    );
    let cost_json = output_json(&cost_output);
    assert_eq!(cost_json["ok"], true);
    assert_eq!(cost_json["command"], "cost");
    assert_eq!(cost_json["data"]["scope"], "project");
    assert_eq!(cost_json["data"]["accounting_quality"], "estimated");
    assert_eq!(
        cost_json["data"]["estimated_cost"],
        usage_json["data"]["estimated_cost"]
    );
}

#[test]
fn stats_reports_runtime_counts_across_registered_projects() {
    let workspace_root = init_git_workspace("stats_runtime_a");
    let second_root = init_git_workspace("stats_runtime_b");
    let registry_root = registry_root_env();

    for root in [&workspace_root, &second_root] {
        let init_status = Command::new(cargo_bin())
            .current_dir(root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args(["projects", "init", "--json"])
            .status()
            .expect("projects init should execute");
        assert!(init_status.success());

        let create_status = Command::new(cargo_bin())
            .current_dir(root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args(["sessions", "create", "--title", "Tracked", "--json"])
            .status()
            .expect("sessions create should execute");
        assert!(create_status.success());
    }

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["stats", "--json"])
        .output()
        .expect("stats should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "stats");
    assert_eq!(json["data"]["scope"], "runtime");
    assert_eq!(json["data"]["project_count"], 2);
    assert_eq!(json["data"]["session_count"], 2);
    assert_eq!(json["data"]["active_session_count"], 2);
}

#[test]
fn prompt_model_flag_returns_canonical_provider_resolution_trace() {
    let workspace_root = init_git_workspace("prompt_model_flag");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .args(["prompt", "--model", "gpt-5.4-mini", "status", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "prompt");
    assert_eq!(
        json["data"]["provider_trace"]["requested_model"],
        "gpt-5.4-mini"
    );
    assert_eq!(
        json["data"]["provider_trace"]["resolved_model"],
        "gpt-5.4-mini"
    );
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "openai"
    );
}

#[test]
fn prompt_executes_openai_compatible_chat_completion_and_persists_assistant_message() {
    let workspace_root = init_git_workspace("prompt_live_openai_compatible");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"message":{"role":"assistant","content":"mock gpt55 answer from provider"}}],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .args([
            "prompt",
            "--model",
            "gpt-5.5",
            "say live provider smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
        "prompt should call OpenAI-compatible chat completions, got: {request}"
    );
    assert!(request
        .to_lowercase()
        .contains("authorization: bearer sk-test\r\n"));
    assert!(request.contains(r#""model":"gpt-5.5""#));
    assert!(request.contains(r#""stream":true"#));
    assert!(request.contains("say live provider smoke"));

    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["provider_trace"]["resolved_model"], "gpt-5.5");
    let session_id = json["session_id"].as_str().expect("session id");
    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(session_id)
        .join("transcript.jsonl");
    let transcript = std::fs::read_to_string(&transcript_path).expect("transcript should exist");
    assert!(
        transcript.contains("mock gpt55 answer from provider"),
        "transcript should contain provider assistant output: {transcript}"
    );
}

#[test]
fn prompt_allows_custom_model_for_openai_compatible_base_url() {
    let workspace_root = init_git_workspace("prompt_openai_compatible_custom_model");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"message":{"role":"assistant","content":"custom glm answer"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .args([
            "prompt",
            "--provider",
            "openai",
            "--model",
            "glm-5.1",
            "say custom provider smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    assert!(request.contains(r#""model":"glm-5.1""#));

    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["assistant_content"], "custom glm answer");
    assert_eq!(json["data"]["provider_trace"]["resolved_model"], "glm-5.1");
    assert_eq!(
        json["data"]["provider_trace"]["resolution_reason"],
        "openai_compatible_custom_model"
    );
}

#[test]
fn prompt_streams_openai_compatible_chat_completion_deltas_and_persists_final_message() {
    let workspace_root = init_git_workspace("prompt_live_openai_streaming");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = concat!(
                        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"streamed \"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"provider answer\"}}]}\n\n",
                        "data: [DONE]\n\n"
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .args([
            "prompt",
            "--model",
            "gpt-5.5",
            "say streaming provider smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(request
        .to_lowercase()
        .contains("accept: text/event-stream\r\n"));
    assert!(request.contains(r#""stream":true"#));

    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["assistant_content"],
        "streamed provider answer"
    );
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "openai"
    );
    let session_id = json["session_id"].as_str().expect("session id");
    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(session_id)
        .join("transcript.jsonl");
    let transcript = std::fs::read_to_string(&transcript_path).expect("transcript should exist");
    assert!(
        transcript.contains("streamed provider answer"),
        "transcript should contain streamed assistant output: {transcript}"
    );
}

#[test]
fn prompt_uses_configured_reasoning_effort_in_openai_payload_and_event() {
    let workspace_root = init_git_workspace("prompt_reasoning_effort");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let config_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "config",
            "set",
            "reasoning_effort",
            "high",
            "--scope",
            "project",
            "--json",
        ])
        .status()
        .expect("config set should execute");
    assert!(config_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"message":{"role":"assistant","content":"reasoning aware answer"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .args([
            "prompt",
            "--model",
            "gpt-5.5",
            "say reasoning payload smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(request.contains(r#""reasoning_effort":"high""#));

    let json = output_json(&output);
    let turn_id = json["data"]["turn_id"].as_str().expect("turn id");
    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "provider_resolution"
            && event["object_id"] == turn_id
            && event["payload"]["reasoning_effort"] == "high"
            && event["payload"]["reasoning_effort_source"] == "project"
    }));
}

#[test]
fn prompt_model_alias_resolves_before_provider_inference() {
    let workspace_root = init_git_workspace("prompt_model_alias");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .args(["prompt", "--model", "smart", "ping", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "prompt");
    assert_eq!(json["data"]["provider_trace"]["requested_model"], "smart");
    assert_eq!(
        json["data"]["provider_trace"]["model_alias_applied"],
        "gpt-5.4"
    );
    assert_eq!(json["data"]["provider_trace"]["resolved_model"], "gpt-5.4");
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "openai"
    );
    assert_eq!(
        json["data"]["provider_trace"]["resolution_reason"],
        "model_alias_catalog_match"
    );
}

#[test]
fn prompt_uses_config_and_env_precedence_for_default_model_then_cli_overrides() {
    let workspace_root = init_git_workspace("prompt_config_precedence");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("prompt_config_precedence_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        global_root.join("settings.json"),
        r#"{
  "default_model": "gpt-5"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_model": "gpt-5.4"
}"#,
    );

    let env_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .env("RESEARCH_CLI_MODEL", "gpt-4.1-mini")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        env_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&env_output.stderr)
    );
    let env_json = output_json(&env_output);
    assert_eq!(env_json["ok"], true);
    assert_eq!(
        env_json["data"]["provider_trace"]["resolved_model"],
        "gpt-4.1-mini"
    );
    assert_eq!(
        env_json["data"]["provider_trace"]["sources"]["model"],
        "environment"
    );
    assert_eq!(
        env_json["data"]["provider_trace"]["resolution_reason"],
        "model_catalog_match"
    );

    let cli_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .env("RESEARCH_CLI_MODEL", "gpt-4.1-mini")
        .args(["prompt", "--model", "gpt-5.4-mini", "status", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        cli_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cli_output.stderr)
    );
    let cli_json = output_json(&cli_output);
    assert_eq!(cli_json["ok"], true);
    assert_eq!(
        cli_json["data"]["provider_trace"]["resolved_model"],
        "gpt-5.4-mini"
    );
    assert_eq!(
        cli_json["data"]["provider_trace"]["sources"]["model"],
        "cli_flag"
    );
}

#[test]
fn prompt_uses_private_config_default_model_when_env_and_cli_are_absent() {
    let workspace_root = init_git_workspace("prompt_private_default_model");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_model": "gpt-5.4"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["provider_trace"]["resolved_model"],
        "gpt-4.1-mini"
    );
    assert_eq!(
        json["data"]["provider_trace"]["resolution_reason"],
        "model_catalog_match"
    );
}

#[test]
fn prompt_explicit_provider_rejects_incompatible_model_without_ambient_fallback() {
    let workspace_root = init_git_workspace("prompt_provider_model_conflict");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .args([
            "prompt",
            "--provider",
            "anthropic",
            "--model",
            "gpt-5.4",
            "ping",
            "--json",
        ])
        .output()
        .expect("prompt should execute");

    assert_eq!(output.status.code(), Some(3));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "prompt");
    assert_eq!(json["error"]["code"], "provider_model_incompatible");
    assert_eq!(json["data"]["requested_provider"], "anthropic");
    assert_eq!(json["data"]["resolved_provider"], "anthropic");
    assert_eq!(json["data"]["requested_model"], "gpt-5.4");
    assert_eq!(
        json["data"]["resolution_reason"],
        "explicit_provider_incompatible_model"
    );
}

#[test]
fn prompt_rejects_custom_openai_model_without_base_url_override() {
    let workspace_root = init_git_workspace("prompt_custom_model_default_openai_blocked");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("OPENAI_BASE_URL")
        .args([
            "prompt",
            "--provider",
            "openai",
            "--model",
            "glm-5.1",
            "ping",
            "--json",
        ])
        .output()
        .expect("prompt should execute");

    assert_eq!(output.status.code(), Some(3));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "prompt");
    assert_eq!(json["error"]["code"], "provider_model_incompatible");
    assert_eq!(json["data"]["requested_provider"], "openai");
    assert_eq!(json["data"]["requested_model"], "glm-5.1");
}

#[test]
fn prompt_uses_config_provider_profile_for_openai_compatible_custom_model() {
    let workspace_root = init_git_workspace("prompt_provider_profile_custom_model");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("prompt_provider_profile_custom_model_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"message":{"role":"assistant","content":"profile glm answer"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let settings = format!(
        r#"{{
  "provider_profiles": {{
    "sjtu-glm": {{
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:{port}/v1",
      "auth_env_var": "SJTU_GLM_API_KEY",
      "default_model": "glm-5.1",
      "supported_models": ["glm-5.1"]
    }}
  }}
}}"#
    );
    write_text_file(global_root.join("settings.json"), &settings);

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("SJTU_GLM_API_KEY", "sk-test")
        .args([
            "prompt",
            "--provider",
            "sjtu-glm",
            "--model",
            "glm-5.1",
            "say provider profile smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    assert!(request.contains(r#""model":"glm-5.1""#));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer sk-test\r\n"));

    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["assistant_content"], "profile glm answer");
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "sjtu-glm"
    );
    assert_eq!(json["data"]["provider_trace"]["provider_id"], "openai");
    assert_eq!(json["data"]["provider_trace"]["resolved_model"], "glm-5.1");
    assert_eq!(
        json["data"]["provider_trace"]["auth_env_var"],
        "SJTU_GLM_API_KEY"
    );
    assert_eq!(
        json["data"]["provider_trace"]["base_url"],
        format!("http://127.0.0.1:{port}/v1")
    );
    assert_eq!(json["data"]["provider_trace"]["base_url_source"], "config");

    let inspect_openai = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("OPENAI_BASE_URL")
        .args(["providers", "inspect", "openai", "--json"])
        .output()
        .expect("providers inspect should execute");
    assert!(
        inspect_openai.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&inspect_openai.stderr),
        String::from_utf8_lossy(&inspect_openai.stdout)
    );
    let inspect_json = output_json(&inspect_openai);
    assert_eq!(inspect_json["data"]["resolved_provider"], "openai");
    assert_eq!(inspect_json["data"]["provider_id"], "openai");
    assert_eq!(inspect_json["data"]["auth_env_var"], "OPENAI_API_KEY");
    assert_eq!(inspect_json["data"]["base_url_source"], "default");
    assert_eq!(inspect_json["data"]["catalog_source"], "embedded");
}

#[test]
fn prompt_explicit_provider_uses_profile_default_model_without_global_model_leak() {
    let workspace_root = init_git_workspace("prompt_profile_default_model_binding");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("prompt_profile_default_model_binding_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"message":{"role":"assistant","content":"ctyun profile answer"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let settings = format!(
        r#"{{
  "provider_profiles": {{
    "sjtu-glm": {{
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:65535/v1",
      "auth_env_var": "SJTU_GLM_API_KEY",
      "default_model": "glm-5.1",
      "supported_models": ["glm-5.1"]
    }},
    "ctyun-glm": {{
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:{port}/v1",
      "auth_env_var": "CTYUN_GLM_API_KEY",
      "default_model": "GLM-5.1",
      "supported_models": ["GLM-5.1"],
      "chat_completion_streaming": false
    }}
  }},
  "default_provider": "sjtu-glm",
  "default_model": "glm-5.1"
}}"#
    );
    write_text_file(global_root.join("settings.json"), &settings);

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("SJTU_GLM_API_KEY", "sk-test")
        .env("CTYUN_GLM_API_KEY", "sk-test-ctyun")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .args([
            "prompt",
            "--provider",
            "ctyun-glm",
            "say provider profile default model smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(request.contains(r#""model":"GLM-5.1""#));
    assert!(!request.contains(r#""model":"glm-5.1""#));

    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["assistant_content"], "ctyun profile answer");
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "ctyun-glm"
    );
    assert_eq!(json["data"]["provider_trace"]["resolved_model"], "GLM-5.1");
    assert_eq!(
        json["data"]["provider_trace"]["sources"]["model"],
        "provider_profile_default"
    );
}

#[test]
fn model_current_native_openai_does_not_inherit_openai_compatible_profile_defaults() {
    let workspace_root = init_git_workspace("native_openai_not_profile_alias");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("native_openai_not_profile_alias_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let settings = r#"{
  "provider_profiles": {
    "ctyun-glm": {
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:65535/v1",
      "auth_env_var": "CTYUN_GLM_API_KEY",
      "default_model": "GLM-5.1",
      "supported_models": ["GLM-5.1"],
      "chat_completion_streaming": false
    }
  },
  "default_provider": "openai",
  "default_model": "gpt-5.4"
}"#;
    write_text_file(global_root.join("settings.json"), settings);

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("OPENAI_API_KEY", "sk-openai")
        .env("CTYUN_GLM_API_KEY", "sk-ctyun")
        .args(["model", "current", "--json"])
        .output()
        .expect("model current should execute");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["provider_id"], "openai");
    assert_eq!(json["data"]["model"], "gpt-5.4");
    assert_ne!(json["data"]["model"], "GLM-5.1");
}

#[test]
fn prompt_provider_profile_can_disable_chat_completion_streaming() {
    let workspace_root = init_git_workspace("prompt_provider_profile_non_streaming");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("prompt_provider_profile_non_streaming_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let read = stream.read(&mut buffer).expect("request should read");
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_text = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = header_text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let body = r#"{"id":"chatcmpl-test","choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"non stream profile answer"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("response should write");
                    return String::from_utf8_lossy(&request).to_string();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return String::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("server accept failed: {err}"),
            }
        }
    });

    let settings = format!(
        r#"{{
  "provider_profiles": {{
    "sjtu-glm": {{
      "provider_id": "openai",
      "base_url": "http://127.0.0.1:{port}/v1",
      "auth_env_var": "SJTU_GLM_API_KEY",
      "default_model": "glm-5.1",
      "supported_models": ["glm-5.1"],
      "chat_completion_streaming": false
    }}
  }}
}}"#
    );
    write_text_file(global_root.join("settings.json"), &settings);

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("SJTU_GLM_API_KEY", "sk-test")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .args([
            "prompt",
            "--provider",
            "sjtu-glm",
            "--model",
            "glm-5.1",
            "say non streaming provider profile smoke",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    assert!(request
        .to_ascii_lowercase()
        .contains("accept: application/json\r\n"));
    assert!(request.contains(r#""stream":false"#));
    assert!(request.contains(r#""model":"glm-5.1""#));

    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["assistant_content"],
        "non stream profile answer"
    );
    assert_eq!(
        json["data"]["provider_trace"]["resolved_provider"],
        "sjtu-glm"
    );
    assert_eq!(
        json["data"]["provider_trace"]["chat_completion_streaming"],
        false
    );
    assert_eq!(
        json["data"]["provider_trace"]["chat_completion_streaming_source"],
        "config"
    );
}

#[test]
fn prompt_with_session_appends_to_existing_session_transcript() {
    let workspace_root = init_git_workspace("prompt_existing_session");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Prompt Target", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args([
            "prompt",
            "--session",
            &session_id,
            "continue work",
            "--json",
        ])
        .output()
        .expect("prompt with session should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["session_id"], session_id);
    assert_eq!(json["data"]["session_id"], session_id);

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let transcript = std::fs::read_to_string(&transcript_path).expect("transcript should exist");
    assert!(transcript.contains("continue work"));
}

#[test]
fn global_flags_are_accepted_without_breaking_command_parsing() {
    let workspace_root = init_git_workspace("global_flags");
    let registry_root = registry_root_env();

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
        .args([
            "--quiet",
            "--no-color",
            "--trace",
            "projects",
            "current",
            "--json",
        ])
        .output()
        .expect("projects current with global flags should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "projects current");
    assert_eq!(json["data"]["resolution_status"], "resolved");
}

#[test]
fn repeated_profile_flags_fail_fast_with_typed_policy_refusal() {
    let workspace_root = init_git_workspace("profile_conflict");
    let registry_root = registry_root_env();

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
        .args([
            "--profile",
            "daily-focus",
            "--profile",
            "deep-research",
            "--json",
        ])
        .output()
        .expect("startup should execute");

    assert_eq!(output.status.code(), Some(7));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "launch");
    assert_eq!(json["error"]["code"], "profile_conflict");
    assert_eq!(json["data"]["policy_kind"], "startup_profile");
    assert_eq!(json["data"]["policy_source"], "cli_flag");
    assert_eq!(json["data"]["refusal_reason"], "conflicting_profile_intent");

    let sessions_root = workspace_root.join(".pmcli").join("sessions");
    assert!(!sessions_root.exists());
}

#[test]
fn bare_text_mode_returns_operator_readable_fullscreen_tui() {
    let workspace_root = init_git_workspace("bare_text_launch");
    let registry_root = registry_root_env();

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
        .output()
        .expect("bare text launch should execute");

    assert!(output.status.success());
    let stdout = stdout_text(&output);
    assert!(stdout.contains("Astra Code 代码智能体"));
    assert!(stdout.contains("对话优先的代码智能体"));
    assert!(stdout.contains("研究："));
    assert!(stdout.contains("输入：直接输入对话"));
    assert!(stdout.contains("/research"));
    assert!(stdout.contains("技能："));
    assert!(stdout.contains("$skill-name"));
    assert!(stdout.contains("/language"));
    assert!(!stdout.contains("pane conversation [expanded] markdown_message_list"));
    assert!(!stdout.contains("fullscreen_split_pane_with_sidebar"));
}

#[test]
fn projects_init_and_current_return_machine_readable_json() {
    let workspace_root = init_git_workspace("cli_projects_init");
    let registry_root = registry_root_env();

    let init_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init should execute");
    assert!(
        init_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init_output.stderr)
    );

    let init_json: Value = serde_json::from_slice(&init_output.stdout).expect("json");
    assert_eq!(init_json["ok"], true);
    assert_eq!(init_json["command"], "projects init");
    assert_eq!(
        init_json["data"]["workspace_root"],
        workspace_root.display().to_string()
    );

    let current_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "current", "--json"])
        .output()
        .expect("projects current should execute");
    assert!(
        current_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&current_output.stderr)
    );

    let current_json: Value = serde_json::from_slice(&current_output.stdout).expect("json");
    assert_eq!(current_json["ok"], true);
    assert_eq!(current_json["command"], "projects current");
    assert_eq!(
        current_json["data"]["resolution_source"],
        "workspace_detection"
    );
    assert_eq!(current_json["data"]["resolution_status"], "resolved");
    assert_eq!(
        current_json["data"]["resolved_project_id"],
        init_json["data"]["project_id"]
    );
    assert_eq!(
        current_json["data"]["resolved_project"]["project_id"],
        init_json["data"]["project_id"]
    );

    let data_dir = init_json["data"]["registry_entry"]["data_dir"]
        .as_str()
        .expect("registry data dir");
    assert!(
        data_dir.starts_with(&format!("{registry_root}/projects/")),
        "project runtime state should live under global state root, got {data_dir}"
    );
    assert!(
        !workspace_root.join(".pmcli").exists(),
        "projects init should not write runtime state into the workspace"
    );

    let events_path = Path::new(data_dir).join("events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "start"
            && event["object_id"] == "projects init"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
            && event["object_id"] == "projects init"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "start"
            && event["object_id"] == "projects current"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
            && event["object_id"] == "projects current"
    }));
}

#[test]
fn projects_init_bootstraps_plain_directory_without_git() {
    let workspace_root = unique_temp_dir("cli_projects_init_plain_directory");
    let registry_root = registry_root_env();

    let init_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init should execute in plain directory");
    assert!(
        init_output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&init_output.stderr),
        String::from_utf8_lossy(&init_output.stdout)
    );

    let init_json = output_json(&init_output);
    assert_eq!(init_json["ok"], true);
    assert_eq!(
        init_json["data"]["workspace_root"],
        workspace_root.display().to_string()
    );
    assert!(workspace_root.join(".git").is_dir());
    assert!(
        !workspace_root.join(".pmcli").exists(),
        "projects init should keep runtime state out of the workspace"
    );
    let head = Command::new("git")
        .current_dir(&workspace_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .expect("git rev-parse should execute");
    assert!(
        head.status.success(),
        "projects init should create a baseline commit, stderr: {}",
        String::from_utf8_lossy(&head.stderr)
    );
    let data_dir = init_json["data"]["registry_entry"]["data_dir"]
        .as_str()
        .expect("registry data dir");
    assert!(
        data_dir.starts_with(&format!("{registry_root}/projects/")),
        "project runtime state should live under global state root, got {data_dir}"
    );

    let current_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "current", "--json"])
        .output()
        .expect("projects current should execute after plain init");
    assert!(current_output.status.success());
    let current_json = output_json(&current_output);
    assert_eq!(current_json["data"]["resolution_status"], "resolved");
    assert_eq!(
        current_json["data"]["resolved_project"]["workspace_root"],
        workspace_root.display().to_string()
    );
}

#[test]
fn remote_status_and_pair_persist_machine_and_project_substrate() {
    let workspace_root = init_git_workspace("remote_substrate");
    let registry_root = registry_root_env();

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
    assert_eq!(status_json["command"], "remote status");
    assert_eq!(status_json["data"]["remote_ready"], false);
    assert_eq!(status_json["data"]["machine_identity"]["status"], "missing");
    assert_eq!(status_json["data"]["binding"]["status"], "missing");
    assert_eq!(
        status_json["data"]["feature_advertisement"]["substrate"],
        "available"
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
    assert_eq!(pair_json["command"], "remote pair");
    assert_eq!(pair_json["data"]["remote_ready"], true);
    assert_eq!(pair_json["data"]["machine_identity"]["status"], "ready");
    assert_eq!(
        pair_json["data"]["client_identity"]["client_id"],
        "mobile-alpha"
    );
    assert_eq!(pair_json["data"]["binding"]["binding_state"], "paired");
    assert_eq!(
        pair_json["data"]["control_owner"]["owner_state"],
        "available"
    );
    assert_eq!(pair_json["data"]["cursor"]["cursor_state"], "initialized");
    assert_eq!(
        pair_json["data"]["projection"]["regeneration_policy"],
        "regenerate_on_mismatch"
    );

    let machine_identity_path = Path::new(&registry_root)
        .join("remote")
        .join("machine_identity.json");
    let client_identity_path = Path::new(&registry_root)
        .join("remote")
        .join("clients")
        .join("mobile-alpha.json");
    let pair_ticket_path = Path::new(&registry_root)
        .join("remote")
        .join("pair_tickets")
        .join("pair_ticket_fixture.json");
    let machine_metadata_path = Path::new(&registry_root)
        .join("remote")
        .join("machine_metadata.json");
    let daemon_state_path = Path::new(&registry_root)
        .join("remote")
        .join("daemon_state.json");
    assert!(machine_identity_path.exists());
    assert!(client_identity_path.exists());
    assert!(pair_ticket_path.exists());
    assert!(machine_metadata_path.exists());
    assert!(daemon_state_path.exists());
    assert!(workspace_root.join(".pmcli/remote/binding.json").exists());
    assert!(workspace_root.join(".pmcli/remote/cursor.json").exists());
    assert!(workspace_root
        .join(".pmcli/remote/projection.json")
        .exists());
    assert!(workspace_root
        .join(".pmcli/remote/control_owner.json")
        .exists());
    assert!(!workspace_root
        .join(".pmcli/remote/machine_metadata.json")
        .exists());
    assert!(!workspace_root
        .join(".pmcli/remote/daemon_state.json")
        .exists());

    let persisted_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "status", "--json"])
        .output()
        .expect("remote status should execute after pair");
    assert!(persisted_status_output.status.success());
    let persisted_status_json = output_json(&persisted_status_output);
    assert_eq!(persisted_status_json["data"]["remote_ready"], true);
    assert_eq!(
        persisted_status_json["data"]["binding"]["client_id"],
        "mobile-alpha"
    );
    assert_eq!(
        persisted_status_json["data"]["daemon_state"]["last_shutdown_state"],
        "unknown"
    );
}

#[test]
fn remote_attach_without_pairing_returns_typed_rejection() {
    let workspace_root = init_git_workspace("remote_attach_rejection");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let attach_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "attach", "--json"])
        .output()
        .expect("remote attach should execute");
    assert_eq!(attach_output.status.code(), Some(12));
    let attach_json = output_json(&attach_output);
    assert_eq!(attach_json["command"], "remote attach");
    assert_eq!(attach_json["error"]["code"], "remote_action_rejected");
    assert_eq!(attach_json["data"]["action"], "attach");
    assert_eq!(attach_json["data"]["rejection_code"], "remote_not_paired");
    assert_eq!(attach_json["data"]["retryable"], false);
}

#[test]
fn remote_mutations_without_pairing_return_typed_rejections_before_usage() {
    let workspace_root = init_git_workspace("remote_mutation_rejections");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    for command in ["handoff", "takeover", "notify"] {
        let output = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args(["remote", command, "--json"])
            .output()
            .expect("remote mutation should execute");
        assert_eq!(output.status.code(), Some(12));
        let json = output_json(&output);
        assert_eq!(json["command"], format!("remote {command}"));
        assert_eq!(json["error"]["code"], "remote_action_rejected");
        assert_eq!(json["data"]["action"], command);
        assert_eq!(json["data"]["rejection_code"], "remote_not_paired");
    }
}

#[test]
fn remote_graduated_control_commands_advertise_ready_capabilities() {
    let workspace_root = init_git_workspace("remote_graduated_advertisements");
    let registry_root = registry_root_env();

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
            "pair_ticket_fixture",
            "--json",
        ])
        .status()
        .expect("remote pair should execute");
    assert!(pair_status.success());

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "status", "--json"])
        .output()
        .expect("remote status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    for command in ["attach", "handoff", "takeover", "notify"] {
        assert_eq!(
            status_json["data"]["feature_advertisement"][command],
            "ready"
        );
        let session_capabilities = status_json["data"]["capability_matrix"]["session"]
            ["capabilities"]
            .as_array()
            .expect("session capabilities should be array");
        assert!(
            session_capabilities
                .iter()
                .any(|value| value.as_str() == Some(command)),
            "missing capability {command}: {session_capabilities:?}"
        );
    }
}

#[test]
fn remote_pair_graduates_lease_capabilities_and_feature_advertisements() {
    let workspace_root = init_git_workspace("remote_pair_graduates");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

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
    assert_eq!(pair_json["data"]["lease"]["lease_state"], "active");
    assert_eq!(
        pair_json["data"]["feature_advertisement"]["attach"],
        "ready"
    );
    assert_eq!(
        pair_json["data"]["feature_advertisement"]["handoff"],
        "ready"
    );
    assert_eq!(
        pair_json["data"]["feature_advertisement"]["takeover"],
        "ready"
    );
    assert_eq!(
        pair_json["data"]["feature_advertisement"]["notify"],
        "ready"
    );
    let session_capabilities = pair_json["data"]["capability_matrix"]["session"]["capabilities"]
        .as_array()
        .expect("session capabilities should be array");
    for expected in ["attach", "handoff", "takeover", "notify"] {
        assert!(
            session_capabilities
                .iter()
                .any(|value| value.as_str() == Some(expected)),
            "missing capability {expected}: {session_capabilities:?}"
        );
    }

    assert!(Path::new(&registry_root)
        .join("remote")
        .join("lease.json")
        .exists());
    assert!(Path::new(&registry_root)
        .join("remote")
        .join("capabilities.json")
        .exists());
}

#[test]
fn remote_control_executes_attach_handoff_takeover_notify_and_regenerates_projection() {
    let workspace_root = init_git_workspace("remote_control_full");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let session_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Remote target", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(session_output.status.success());
    let session_json = output_json(&session_output);
    let session_id = session_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();

    let pair_status = Command::new(cargo_bin())
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
        .status()
        .expect("remote pair should execute");
    assert!(pair_status.success());

    let attach_inspect = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "attach", "--session", &session_id, "--json"])
        .output()
        .expect("remote attach inspect should execute");
    assert!(attach_inspect.status.success());
    let attach_inspect_json = output_json(&attach_inspect);
    assert_eq!(attach_inspect_json["command"], "remote attach");
    assert_eq!(attach_inspect_json["data"]["execution_mode"], "inspect");
    assert_eq!(attach_inspect_json["data"]["eligibility"]["eligible"], true);
    assert_eq!(
        attach_inspect_json["data"]["eligibility"]["strategy"],
        "terminal_host"
    );

    let attach_execute = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "attach",
            "--session",
            &session_id,
            "--strategy",
            "terminal_host",
            "--execute",
            "--json",
        ])
        .output()
        .expect("remote attach execute should execute");
    assert!(
        attach_execute.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&attach_execute.stderr)
    );
    let attach_execute_json = output_json(&attach_execute);
    assert_eq!(attach_execute_json["data"]["execution_mode"], "execute");
    assert_eq!(
        attach_execute_json["data"]["control_owner"]["owner_id"],
        "mobile-alpha"
    );
    assert_eq!(
        attach_execute_json["data"]["control_owner"]["control_lease"]["lease_state"],
        "active"
    );
    assert_eq!(attach_execute_json["data"]["ownership_epoch"], 1);

    let handoff_execute = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "handoff",
            "--session",
            &session_id,
            "--target-client",
            "tablet-beta",
            "--execute",
            "--json",
        ])
        .output()
        .expect("remote handoff execute should execute");
    assert!(handoff_execute.status.success());
    let handoff_json = output_json(&handoff_execute);
    assert_eq!(handoff_json["data"]["execution_mode"], "execute");
    assert_eq!(
        handoff_json["data"]["control_owner"]["owner_id"],
        "tablet-beta"
    );
    assert_eq!(handoff_json["data"]["ownership_epoch"], 2);

    let takeover_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "takeover",
            "--session",
            &session_id,
            "--client",
            "mobile-alpha",
            "--json",
        ])
        .output()
        .expect("remote takeover should execute");
    assert!(takeover_output.status.success());
    let takeover_json = output_json(&takeover_output);
    assert_eq!(
        takeover_json["data"]["previous_owner"]["owner_id"],
        "tablet-beta"
    );
    assert_eq!(
        takeover_json["data"]["new_owner"]["owner_id"],
        "mobile-alpha"
    );
    assert_eq!(takeover_json["data"]["ownership_epoch"], 3);

    let notify_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "notify",
            "--kind",
            "task_finished",
            "--message",
            "done",
            "--json",
        ])
        .output()
        .expect("remote notify should execute");
    assert!(notify_output.status.success());
    let notify_json = output_json(&notify_output);
    assert_eq!(notify_json["data"]["notification"]["kind"], "task_finished");
    assert_eq!(
        notify_json["data"]["receipts"][0]["delivery_state"],
        "queued"
    );

    write_text_file(
        workspace_root.join(".pmcli/remote/projection.json"),
        &json!({
            "schema_version": "1",
            "project_id": "wrong_project",
            "projection_state": "stale",
            "regeneration_policy": "regenerate_on_mismatch",
            "runtime_descriptor": {
                "schema_version": "1",
                "project_id": "wrong_project",
                "active_session_id": "wrong_session",
                "runtime_state": "stale"
            },
            "envelope_projection": {
                "schema_version": "1",
                "projection_id": "stale_projection",
                "project_id": "wrong_project",
                "source": "stale_cache",
                "active_session_id": "wrong_session"
            },
            "updated_at": "0"
        })
        .to_string(),
    );

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "status", "--json"])
        .output()
        .expect("remote status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(
        status_json["data"]["projection"]["runtime_descriptor"]["active_session_id"],
        session_id
    );
    assert_eq!(status_json["data"]["projection_regenerated"], true);
    assert_eq!(
        status_json["data"]["control_owner"]["owner_id"],
        "mobile-alpha"
    );

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("event path should be utf8"),
    );
    for expected in [
        "remote_attach",
        "remote_handoff",
        "remote_takeover",
        "remote_notify",
    ] {
        assert!(
            events.iter().any(|event| event["event_name"] == expected),
            "missing event {expected}: {events:?}"
        );
    }
}

#[test]
fn remote_pair_rejects_ticket_replay_and_revocation_blocks_control() {
    let workspace_root = init_git_workspace("remote_replay_revocation");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let first_pair = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "pair",
            "--client",
            "mobile-alpha",
            "--ticket",
            "pair-ticket-once",
            "--json",
        ])
        .output()
        .expect("first remote pair should execute");
    assert!(first_pair.status.success());

    let replay_pair = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "pair",
            "--client",
            "mobile-alpha",
            "--ticket",
            "pair-ticket-once",
            "--json",
        ])
        .output()
        .expect("replayed remote pair should execute");
    assert_eq!(replay_pair.status.code(), Some(12));
    let replay_json = output_json(&replay_pair);
    assert_eq!(
        replay_json["data"]["rejection_code"],
        "remote_pair_ticket_replayed"
    );

    let revoke = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "revoke", "--client", "mobile-alpha", "--json"])
        .output()
        .expect("remote revoke should execute");
    assert!(revoke.status.success());
    let revoke_json = output_json(&revoke);
    assert_eq!(revoke_json["command"], "remote revoke");
    assert_eq!(revoke_json["data"]["binding"]["binding_state"], "revoked");

    let attach = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "attach", "--json"])
        .output()
        .expect("remote attach after revoke should execute");
    assert_eq!(attach.status.code(), Some(12));
    let attach_json = output_json(&attach);
    assert_eq!(
        attach_json["data"]["rejection_code"],
        "remote_binding_revoked"
    );
}

#[test]
fn remote_expired_lease_degrades_to_not_ready_and_blocks_mutations() {
    let workspace_root = init_git_workspace("remote_expired_lease");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let pair = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "pair",
            "--client",
            "mobile-alpha",
            "--ticket",
            "pair-ticket-expiring",
            "--lease-expires-at",
            "1",
            "--json",
        ])
        .output()
        .expect("remote pair should execute");
    assert!(pair.status.success());

    let status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "status", "--json"])
        .output()
        .expect("remote status should execute");
    assert!(status.status.success());
    let status_json = output_json(&status);
    assert_eq!(status_json["data"]["remote_ready"], false);
    assert_eq!(status_json["data"]["lease"]["lease_state"], "expired");
    assert_eq!(status_json["data"]["revocation_reason"], "lease_expired");

    let notify = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "notify",
            "--kind",
            "task_finished",
            "--message",
            "done",
            "--json",
        ])
        .output()
        .expect("remote notify should execute");
    assert_eq!(notify.status.code(), Some(12));
    let notify_json = output_json(&notify);
    assert_eq!(
        notify_json["data"]["rejection_code"],
        "remote_lease_expired"
    );
}

#[test]
fn remote_pair_rejects_expired_pair_ticket_before_binding() {
    let workspace_root = init_git_workspace("remote_expired_pair_ticket");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let pair = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "remote",
            "pair",
            "--client",
            "mobile-alpha",
            "--ticket",
            "pair-ticket-expired-before-use",
            "--ticket-expires-at",
            "1",
            "--json",
        ])
        .output()
        .expect("remote pair should execute");
    assert_eq!(pair.status.code(), Some(12));
    let pair_json = output_json(&pair);
    assert_eq!(
        pair_json["data"]["rejection_code"],
        "remote_pair_ticket_expired"
    );
}

#[test]
fn host_surface_status_and_tui_projections_share_remote_workbench_contract() {
    let workspace_root = init_git_workspace("host_surface_tui_projection");
    let registry_root = registry_root_env();

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
            "pair-ticket-host-surface",
            "--json",
        ])
        .status()
        .expect("remote pair should execute");
    assert!(pair_status.success());
    write_active_orchestration_run(&workspace_root, "run_host_surface_001");

    let agent_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "agents",
            "start",
            "--mock",
            "--intent",
            "host surface board agent handoff",
            "--role-profile",
            "implementation",
            "--message",
            "produce an implementation handoff for the goal board",
            "--json",
        ])
        .output()
        .expect("agents start should execute");
    assert!(
        agent_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&agent_output.stderr)
    );
    write_text_file(
        workspace_root.join("docs").join("review-host-surface.md"),
        "# Host Surface Board Evidence\n\nReview target for board projection contract.\n",
    );

    let review_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--target",
            "docs/review-host-surface.md",
            "--objective",
            "Review host surface board handoff evidence",
            "--reviewer-role",
            "independent_reviewer",
            "--verdict",
            "pending",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(
        review_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&review_output.stderr)
    );

    let lease_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "lease",
            "acquire",
            "--run-id",
            "run_host_surface_001",
            "--owner-agent",
            "agent_host_surface",
            "--stale-after-sec",
            "60",
            "--json",
        ])
        .output()
        .expect("projectops lease acquire should execute");
    assert!(
        lease_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&lease_output.stderr)
    );
    let lease_json = output_json(&lease_output);
    let lease_id = lease_json["data"]["lease_id"]
        .as_str()
        .expect("lease id should exist")
        .to_string();
    let wake_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "emit",
            "--run-id",
            "run_host_surface_001",
            "--lease-id",
            &lease_id,
            "--owner-agent",
            "agent_host_surface",
            "--kind",
            "attention",
            "--urgency",
            "high",
            "--summary",
            "host surface board needs operator decision",
            "--requires-main-system",
            "--json",
        ])
        .output()
        .expect("projectops wake emit should execute");
    assert!(
        wake_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&wake_output.stderr)
    );

    let data_dir = workspace_root.join(".pmcli");
    let mut run = OrchestrationRun::new(
        "run_host_surface_goal_policy",
        "Project high-autonomy research board policy",
        "approved",
    );
    run.status = "running".to_string();
    run.automation_mode = GoalAutomationMode::HighAutonomy;
    run.steps = vec![OrchestrationStep::new(
        "dispatch",
        "Dispatch research worker",
        OrchestrationStepStatus::Running,
    )];
    save_run(&data_dir, &run).expect("active orchestration run should save");

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_output.stderr)
    );
    let surface_json = output_json(&surface_output);
    assert_eq!(surface_json["command"], "host surface status");
    assert_eq!(
        surface_json["data"]["projection"]["project_id"],
        surface_json["project_id"]
    );
    assert_eq!(
        surface_json["data"]["projection"]["authority_model"],
        "kernel_authoritative_projection"
    );
    assert_eq!(
        surface_json["data"]["projection"]["surfaces"]["local_tui"]["readiness"],
        "projection_ready"
    );
    assert_eq!(
        surface_json["data"]["projection"]["surfaces"]["mobile"]["transport"],
        "tailscale_private_overlay"
    );
    assert_eq!(
        surface_json["data"]["projection"]["workbench"]["source"],
        "kernel_state_bundle_and_project_indexes"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["board"]["schema_version"],
        "research_board_projection.v1"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["board"]["authority_model"],
        "derived_from_existing_research_authorities"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["board"]["automation_mode"],
        "high_autonomy"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["board_inspector_ready"],
        true
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["board_inspector_command"],
        json!(["research", "board", "--json"])
    );
    let actions = surface_json["data"]["projection"]["actions"]
        .as_array()
        .expect("host surface actions array");
    assert!(actions.iter().any(|action| {
        action["action_id"] == "open_research_board"
            && action["command"] == json!(["research", "board", "--json"])
    }));
    assert!(
        surface_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("research board entries array")
            .iter()
            .any(|entry| {
                entry["source"]["source_kind"] == "goal_run_automation_policy"
                    && entry["title"] == "dispatch_agent_task"
                    && entry["action_policy"] == "allowed"
                    && entry["summary"]
                        .as_str()
                        .expect("entry summary")
                        .contains("allowed by high_autonomy")
            })
    );
    let board_entries = surface_json["data"]["projection"]["research"]["board"]["entries"]
        .as_array()
        .expect("research board entries array");
    assert!(board_entries.iter().any(|entry| {
        entry["source"]["source_kind"] == "agent_task_packet"
            && entry["bucket_id"] == "needs_approval"
            && entry["title"] == "host surface board agent handoff"
    }));
    assert!(board_entries.iter().any(|entry| {
        entry["source"]["source_kind"] == "review_packet"
            && entry["bucket_id"] == "needs_review"
            && entry["title"] == "Review host surface board handoff evidence"
    }));
    assert!(board_entries.iter().any(|entry| {
        entry["source"]["source_kind"] == "projectops_supervisor_lease"
            && entry["bucket_id"] == "running"
            && entry["source"]["source_id"] == lease_id
    }));
    assert!(board_entries.iter().any(|entry| {
        entry["source"]["source_kind"] == "projectops_wake_event"
            && entry["bucket_id"] == "needs_approval"
            && entry["title"] == "host surface board needs operator decision"
    }));
    assert!(
        surface_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("research board entries array")
            .len()
            >= 5
    );
    let board_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "board", "--json"])
        .output()
        .expect("research board should execute");
    assert!(
        board_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&board_output.stderr)
    );
    let board_json = output_json(&board_output);
    assert_eq!(board_json["command"], "research board");
    assert_eq!(
        board_json["data"]["schema_version"],
        "research_board_projection.v1"
    );
    assert_eq!(
        board_json["data"]["authority_model"],
        "derived_from_existing_research_authorities"
    );
    assert_eq!(
        board_json["data"],
        surface_json["data"]["projection"]["research"]["board"]
    );
    let panes = surface_json["data"]["projection"]["pane_status"]["panes"]
        .as_array()
        .expect("panes array");
    assert!(panes
        .iter()
        .any(|pane| pane["pane_id"] == "conversation" && pane["default_visibility"] == "visible"));
    assert!(panes.iter().any(|pane| {
        pane["pane_id"] == "research_dag"
            && pane["status"] == "contract_ready"
            && pane["degraded_reason"] == "no active research thread"
    }));
    assert!(panes.iter().any(|pane| {
        pane["pane_id"] == "diff_artifact"
            && pane["status"] == "contract_ready"
            && pane["data_ref"] == "workbench.artifacts"
    }));

    let snapshot_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "snapshot", "--json"])
        .output()
        .expect("tui snapshot should execute");
    assert!(snapshot_output.status.success());
    let snapshot_json = output_json(&snapshot_output);
    assert_eq!(snapshot_json["command"], "tui snapshot");
    assert_eq!(
        snapshot_json["data"]["projection"]["projection_id"],
        surface_json["data"]["projection"]["projection_id"]
    );
    assert_eq!(
        snapshot_json["data"]["projection"]["workbench"]["source"],
        surface_json["data"]["projection"]["workbench"]["source"]
    );

    let launch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "launch", "--json"])
        .output()
        .expect("tui launch should execute");
    assert!(launch_output.status.success());
    let launch_json = output_json(&launch_output);
    assert_eq!(launch_json["command"], "tui launch");
    assert_eq!(
        launch_json["data"]["launch_mode"],
        "inline_projection_renderer"
    );
    assert_eq!(
        launch_json["data"]["inline_view"]["schema_version"],
        "tui_inline_view.v1"
    );
    assert_eq!(
        launch_json["data"]["inline_view"]["renderer_mode"],
        "inline_terminal"
    );
    assert_eq!(
        launch_json["data"]["inline_view"]["layout"],
        "chat_first_repl"
    );
    let capabilities = launch_json["data"]["inline_view"]["capabilities"]
        .as_array()
        .expect("inline view capabilities should be array");
    assert!(capabilities.iter().any(|capability| {
        capability["capability_id"] == "slash_command_router"
            && capability["status"] == "interactive"
    }));
    assert!(capabilities.iter().any(|capability| {
        capability["capability_id"] == "skill_invocation_router"
            && capability["status"] == "interactive"
    }));
    assert_eq!(
        launch_json["data"]["inline_view"]["command_model"]["command_prefix"],
        "/"
    );
    assert!(
        launch_json["data"]["inline_view"]["command_model"]["groups"]
            .as_array()
            .expect("command groups should be array")
            .iter()
            .any(|group| group["group_id"] == "permissions"
                && group["commands"]
                    .as_array()
                    .expect("permission commands should be array")
                    .iter()
                    .any(|command| command["typed"] == "/approve <request-id>"))
    );
    assert_eq!(
        launch_json["data"]["inline_view"]["skill_model"]["skill_prefix"],
        "$"
    );
    assert!(
        launch_json["data"]["inline_view"]["skill_model"]["total_count"]
            .as_u64()
            .expect("skill total count should be numeric")
            > 0
    );
    assert!(launch_json["data"]["inline_view"]["skill_model"]["entries"]
        .as_array()
        .expect("skill entries should be array")
        .iter()
        .any(|entry| entry["typed"]
            .as_str()
            .is_some_and(|value| value.starts_with('$'))));
    assert!(capabilities.iter().any(|capability| {
        capability["capability_id"] == "composer_text_entry"
            && capability["status"] == "interactive"
    }));
    for capability_id in [
        "collapsible_tool_output",
        "colored_diff",
        "permission_overlay",
        "session_picker",
    ] {
        assert!(
            capabilities.iter().any(|capability| {
                capability["capability_id"] == capability_id
                    && capability["status"] == "projection_only"
            }),
            "missing projection-only inline TUI capability {capability_id}"
        );
    }
    let pane_plans = launch_json["data"]["inline_view"]["pane_plans"]
        .as_array()
        .expect("inline pane plans should be array");
    assert!(pane_plans.iter().any(|pane| {
        pane["pane_id"] == "conversation" && pane["renderer"] == "markdown_message_list"
    }));
    let status_hud = launch_json["data"]["inline_view"]["status_hud"]
        .as_array()
        .expect("status hud should be array");
    for badge_id in [
        "project",
        "remote",
        "permission_mode",
        "model",
        "reasoning",
        "theme",
    ] {
        assert!(
            status_hud.iter().any(|badge| badge["badge_id"] == badge_id),
            "missing status hud badge {badge_id}"
        );
    }
    let keymap = launch_json["data"]["inline_view"]["keymap"]
        .as_array()
        .expect("keymap should be array");
    assert!(keymap
        .iter()
        .any(|binding| { binding["key"] == "ctrl+p" && binding["action_id"] == "submit_prompt" }));
    assert!(keymap
        .iter()
        .any(|binding| { binding["key"] == "ctrl+s" && binding["action_id"] == "switch_session" }));

    let tui_text_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "launch"])
        .output()
        .expect("tui launch text mode should execute");
    assert!(tui_text_output.status.success());
    let tui_text = stdout_text(&tui_text_output);
    assert!(tui_text.contains("Astra Code 代码智能体"));
    assert!(tui_text.contains("raw_mode: degraded_non_tty"));
    assert!(tui_text.contains("对话优先的代码智能体"));
    assert!(tui_text.contains("研究："));
    assert!(tui_text.contains("输入：直接描述要改的代码、错误或实验目标"));
    assert!(tui_text.contains("/research"));
    assert!(tui_text.contains("命令："));
    assert!(tui_text.contains("/language"));
    assert!(tui_text.contains("/approve <request-id>"));
    assert!(tui_text.contains("/deny <request-id>"));
    assert!(tui_text.contains("/terminal"));
    assert!(tui_text.contains("技能："));
    assert!(tui_text.contains("$skill-name"));
    assert!(tui_text.contains("$list"));
    assert!(!tui_text.contains("pane conversation [expanded] markdown_message_list"));
    assert!(!tui_text.contains("Tool stream"));

    let fullscreen_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "launch", "--fullscreen", "--json"])
        .output()
        .expect("fullscreen tui launch should execute");
    assert!(fullscreen_output.status.success());
    let fullscreen_json = output_json(&fullscreen_output);
    assert_eq!(fullscreen_json["command"], "tui launch");
    assert_eq!(
        fullscreen_json["data"]["launch_mode"],
        "fullscreen_split_pane_projection_renderer"
    );
    assert_eq!(
        fullscreen_json["data"]["inline_view"]["renderer_mode"],
        "alternate_screen_split_pane"
    );
    assert_eq!(
        fullscreen_json["data"]["inline_view"]["layout"],
        "advanced_fullscreen_chat_first"
    );
    assert_eq!(
        fullscreen_json["data"]["inline_view"]["authority_model"],
        "kernel_authoritative_projection"
    );

    let fullscreen_text_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "launch", "--fullscreen"])
        .output()
        .expect("fullscreen tui launch text mode should execute");
    assert!(fullscreen_text_output.status.success());
    let fullscreen_text = stdout_text(&fullscreen_text_output);
    assert!(fullscreen_text.contains("Astra Code 代码智能体"));
    assert!(fullscreen_text.contains("对话优先的代码智能体"));
    assert!(fullscreen_text.contains("研究："));
    assert!(fullscreen_text.contains("命令："));
    assert!(fullscreen_text.contains("技能："));
    assert!(!fullscreen_text.contains("pane conversation [expanded] markdown_message_list"));

    let plain_workspace = unique_temp_dir("tui_launch_plain_directory");
    let plain_registry = registry_root_env();
    let plain_tui_output = Command::new(cargo_bin())
        .current_dir(&plain_workspace)
        .env("RESEARCH_CLI_STATE_HOME", &plain_registry)
        .args(["tui", "launch", "--fullscreen", "--json"])
        .output()
        .expect("tui launch should auto-bootstrap plain directories");
    assert!(
        plain_tui_output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&plain_tui_output.stderr),
        String::from_utf8_lossy(&plain_tui_output.stdout)
    );
    let plain_tui_json = output_json(&plain_tui_output);
    assert_eq!(plain_tui_json["command"], "tui launch");
    assert_eq!(
        plain_tui_json["data"]["projection"]["remote"]["runtime_descriptor"]["runtime_state"],
        "project_registered"
    );
    assert!(plain_workspace.join(".pmcli").is_dir());

    let actions_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["tui", "actions", "--json"])
        .output()
        .expect("tui actions should execute");
    assert!(actions_output.status.success());
    let actions_json = output_json(&actions_output);
    assert_eq!(actions_json["command"], "tui actions");
    let actions = actions_json["data"]["actions"]
        .as_array()
        .expect("actions should be array");
    assert!(actions
        .iter()
        .any(|action| action["action_id"] == "submit_prompt" && action["enabled"] == true));
    assert!(actions.iter().any(|action| {
        action["action_id"] == "approve_permission"
            && action["gate"] == "permission_queue"
            && action["enabled"] == false
            && action["command"].as_array().unwrap()
                == &vec![
                    json!("permissions"),
                    json!("approve"),
                    json!("<request-id>"),
                ]
    }));
    assert!(actions.iter().any(|action| {
        action["action_id"] == "deny_permission"
            && action["command"].as_array().unwrap()
                == &vec![json!("permissions"), json!("deny"), json!("<request-id>")]
    }));
    assert!(actions.iter().any(|action| {
        action["action_id"] == "switch_session"
            && action["command"].as_array().unwrap()
                == &vec![json!("sessions"), json!("resume"), json!("<session-id>")]
    }));
    assert!(actions.iter().any(|action| {
        action["action_id"] == "interrupt_turn"
            && action["command"].as_array().unwrap()
                == &vec![
                    json!("remote"),
                    json!("tui"),
                    json!("action"),
                    json!("interrupt_turn"),
                    json!("<reason>"),
                ]
    }));
    for required_action in [
        "ack_hitl",
        "approve_permission",
        "advance_research_loop",
        "deny_permission",
        "inspect_memory",
        "inspect_recovery_governance",
        "interrupt_turn",
        "open_artifact",
        "retry_routine_trigger",
        "remote_attach",
        "resize_terminal",
        "steer_turn",
        "submit_prompt",
        "switch_session",
        "terminal_attach",
        "terminal_replay",
    ] {
        assert!(
            actions
                .iter()
                .any(|action| action["action_id"] == required_action),
            "missing host surface action {required_action}"
        );
    }
    assert!(actions.iter().any(|action| {
        action["action_id"] == "terminal_replay"
            && action["enabled"] == true
            && action["command"].as_array().unwrap()[0] == "remote"
    }));
    assert!(actions.iter().any(|action| {
        action["action_id"] == "resize_terminal"
            && action["enabled"] == true
            && action["gate"] == "remote_control_lease"
            && action["disabled_reason"].as_null().is_some()
    }));
}

#[test]
fn tailscale_helpers_and_terminal_lane_are_governed_projection_surfaces() {
    let workspace_root = init_git_workspace("tailscale_terminal_lane");
    let registry_root = registry_root_env();

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
            "pair-ticket-terminal",
            "--json",
        ])
        .status()
        .expect("remote pair should execute");
    assert!(pair_status.success());

    let tailscale_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_TAILSCALE_IP", "100.101.102.103")
        .args(["remote", "tailscale", "status", "--json"])
        .output()
        .expect("remote tailscale status should execute");
    assert!(tailscale_output.status.success());
    let tailscale_json = output_json(&tailscale_output);
    assert_eq!(tailscale_json["command"], "remote tailscale status");
    assert_eq!(tailscale_json["data"]["overlay_detected"], true);
    assert_eq!(tailscale_json["data"]["addresses"][0], "100.101.102.103");

    let serve_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_TAILSCALE_IP", "100.101.102.103")
        .args([
            "remote",
            "tailscale",
            "serve-plan",
            "--daemon-port",
            "8787",
            "--json",
        ])
        .output()
        .expect("remote tailscale serve-plan should execute");
    assert!(serve_plan_output.status.success());
    let serve_plan_json = output_json(&serve_plan_output);
    assert_eq!(serve_plan_json["command"], "remote tailscale serve-plan");
    assert_eq!(
        serve_plan_json["data"]["direct_url"],
        "http://100.101.102.103:8787/"
    );
    assert_eq!(serve_plan_json["data"]["serve_command"][0], "tailscale");

    let attach_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "terminal", "attach", "--json"])
        .output()
        .expect("remote terminal attach should execute");
    assert!(attach_output.status.success());
    let attach_json = output_json(&attach_output);
    assert_eq!(attach_json["command"], "remote terminal attach");
    assert_eq!(attach_json["data"]["terminal"]["lease_state"], "active");
    assert_eq!(
        attach_json["data"]["terminal"]["lane_state"],
        "governed_pty_projection"
    );
    assert_eq!(
        attach_json["data"]["terminal"]["authority"],
        "control_lease_required"
    );

    let replay_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["remote", "terminal", "replay", "--json"])
        .output()
        .expect("remote terminal replay should execute");
    assert!(replay_output.status.success());
    let replay_json = output_json(&replay_output);
    assert_eq!(replay_json["command"], "remote terminal replay");
    assert_eq!(
        replay_json["data"]["replay"]["replay_policy"],
        "cursor_based_scrollback"
    );
    assert_eq!(
        replay_json["data"]["replay"]["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path"),
    );
    assert!(events.iter().any(|event| {
        event["event_name"] == "remote_terminal_attach"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "remote_terminal_replay"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
    }));
}

#[test]
fn host_surface_branch_count_uses_branch_index_not_remote_cursor_seq() {
    let workspace_root = init_real_git_workspace("host_surface_branch_count");
    let registry_root = registry_root_env();

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
            "pair-ticket-branch-count",
            "--json",
        ])
        .status()
        .expect("remote pair should execute");
    assert!(pair_status.success());

    let branch_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "verify host surface branch count",
            "--max-branches",
            "2",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(
        branch_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&branch_output.stderr)
    );

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(surface_output.status.success());
    let surface_json = output_json(&surface_output);
    assert_eq!(surface_json["data"]["projection"]["branches"]["count"], 2);
    assert_eq!(
        surface_json["data"]["projection"]["workbench"]["branches"]["count"],
        2
    );
}

#[test]
fn projects_audit_canonical_reports_git_and_unique_surface_violations() {
    let workspace_root = init_git_workspace("projects_audit_canonical");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "canonical status\n");
    write_text_file(
        workspace_root.join("docs/plan-a.md"),
        r#"---
doc_frame:
  id: plan.a
  title: Plan A
  doc_type: implementation_plan
  lifecycle: active
  scope: project
  summary: First active implementation plan.
  generated_by: test
  updated_at: 2026-04-25
---

# Plan A
"#,
    );
    write_text_file(
        workspace_root.join("docs/plan-b.md"),
        r#"---
doc_frame:
  id: plan.b
  title: Plan B
  doc_type: implementation_plan
  lifecycle: active
  scope: project
  summary: Second active implementation plan.
  generated_by: test
  updated_at: 2026-04-25
---

# Plan B
"#,
    );

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let audit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "audit", "--canonical", "--json"])
        .output()
        .expect("projects audit should execute");
    assert!(
        audit_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&audit_output.stderr)
    );
    let audit_json = output_json(&audit_output);
    assert_eq!(audit_json["command"], "projects audit");
    assert_eq!(audit_json["data"]["ok"], false);
    assert_eq!(audit_json["data"]["git_state"]["is_repository"], false);
    assert_eq!(
        audit_json["data"]["canonical_surface"]["active_status_doc"],
        "README.md"
    );
    assert!(audit_json["data"]["blocking_violations"]
        .as_array()
        .expect("blocking violations array")
        .iter()
        .any(|violation| violation["violation_type"] == "duplicate_active_doc"));
}

#[test]
fn projects_audit_flags_stale_canonical_lineage_chain() {
    let workspace_root = init_git_workspace("canonical_lineage_guard");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "canonical status\n");
    write_explicit_canonical_manifests(&workspace_root);
    write_text_file(
        workspace_root.join("docs/plan-current.md"),
        "# Current plan\n",
    );
    write_text_file(workspace_root.join("docs/plan-old.md"), "# Old plan\n");
    write_text_file(
        workspace_root.join("src/runtime_current.rs"),
        "fn current() {}\n",
    );
    write_text_file(workspace_root.join("src/runtime_old.rs"), "fn old() {}\n");
    write_text_file(
        workspace_root.join("experiments/ablation-old.json"),
        r#"{"status":"old"}"#,
    );
    write_text_file(
        workspace_root.join("results/metrics.json"),
        r#"{"accuracy":0.9}"#,
    );
    write_text_file(
        workspace_root.join("paper/main.tex"),
        "\\section{Current}\n",
    );
    write_text_file(
        workspace_root.join(".pmcli/canonical_lineage.json"),
        &serde_json::to_string_pretty(&json!({
            "schema_version": "canonical_lineage.v1",
            "source": "test",
            "current_generation": "g2",
            "policy": "current_nodes_must_not_depend_on_non_current_sources",
            "nodes": [
                {
                    "artifact_id": "doc.plan.current",
                    "family": "doc",
                    "role": "implementation_plan",
                    "path": "docs/plan-current.md",
                    "status": "current",
                    "generation": "g2",
                    "derived_from": [],
                    "evidence_refs": ["README.md"]
                },
                {
                    "artifact_id": "doc.plan.old",
                    "family": "doc",
                    "role": "implementation_plan",
                    "path": "docs/plan-old.md",
                    "status": "superseded",
                    "generation": "g1",
                    "derived_from": [],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "code.runtime.current",
                    "family": "code",
                    "role": "runtime",
                    "path": "src/runtime_current.rs",
                    "status": "current",
                    "generation": "g2",
                    "derived_from": ["doc.plan.old"],
                    "evidence_refs": ["docs/plan-current.md"]
                },
                {
                    "artifact_id": "code.runtime.old",
                    "family": "code",
                    "role": "runtime",
                    "path": "src/runtime_old.rs",
                    "status": "superseded",
                    "generation": "g1",
                    "derived_from": ["doc.plan.old"],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "experiment.ablation.old",
                    "family": "experiment",
                    "role": "ablation",
                    "path": "experiments/ablation-old.json",
                    "status": "superseded",
                    "generation": "g1",
                    "derived_from": ["code.runtime.old"],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "result.metrics.current",
                    "family": "result",
                    "role": "metrics",
                    "path": "results/metrics.json",
                    "status": "current",
                    "generation": "g2",
                    "derived_from": ["experiment.ablation.old"],
                    "evidence_refs": ["results/metrics.json"]
                },
                {
                    "artifact_id": "paper.main.current",
                    "family": "paper",
                    "role": "main",
                    "path": "paper/main.tex",
                    "status": "current",
                    "generation": "g2",
                    "derived_from": ["result.metrics.current"],
                    "evidence_refs": ["paper/main.tex"]
                }
            ]
        }))
        .expect("lineage manifest should serialize"),
    );

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let audit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "audit", "--canonical", "--json"])
        .output()
        .expect("projects audit should execute");
    assert!(audit_output.status.success());
    let audit_json = output_json(&audit_output);
    assert_eq!(audit_json["command"], "projects audit");
    assert_eq!(audit_json["data"]["ok"], false);
    assert_eq!(
        audit_json["data"]["canonical_lineage"]["source"],
        "explicit"
    );
    assert!(audit_json["data"]["stale_lineage_refs"]
        .as_array()
        .expect("stale lineage refs")
        .iter()
        .any(
            |violation| violation["violation_type"] == "stale_lineage_ref"
                && violation["path"] == "src/runtime_current.rs"
        ));
    assert!(audit_json["data"]["stale_lineage_refs"]
        .as_array()
        .expect("stale lineage refs")
        .iter()
        .any(|violation| violation["path"] == "results/metrics.json"));
    assert!(audit_json["data"]["superseded_public_artifacts"]
        .as_array()
        .expect("superseded public artifacts")
        .iter()
        .any(|violation| violation["path"] == "src/runtime_old.rs"));
    assert!(audit_json["data"]["blocking_violations"]
        .as_array()
        .expect("blocking violations")
        .iter()
        .any(|violation| violation["violation_type"] == "superseded_public_artifact"));

    let cleanup_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--canonical", "--json"])
        .output()
        .expect("repo cleanup-plan should execute");
    assert!(cleanup_plan_output.status.success());
    let cleanup_plan_json = output_json(&cleanup_plan_output);
    assert!(cleanup_plan_json["data"]["review_gates"]
        .as_array()
        .expect("review gates")
        .iter()
        .any(|gate| gate["gate_id"] == "canonicality_audit" && gate["status"] == "blocked"));
}

#[test]
fn repo_cleanup_canonical_moves_superseded_public_artifacts_to_archive() {
    let workspace_root = init_real_git_workspace("canonical_lineage_cleanup_apply");
    let registry_root = registry_root_env();
    write_text_file(
        workspace_root.join(".gitignore"),
        ".pmcli/*\n!.pmcli/\n!.pmcli/canonical_surface.json\n!.pmcli/code_owner_manifest.json\n!.pmcli/canonical_lineage.json\n",
    );
    write_text_file(workspace_root.join("README.md"), "canonical status\n");
    write_explicit_canonical_manifests(&workspace_root);
    write_text_file(
        workspace_root.join("docs/plan-current.md"),
        "# Current plan\n",
    );
    write_text_file(workspace_root.join("docs/plan-old.md"), "# Old plan\n");
    write_text_file(
        workspace_root.join("src/runtime_current.rs"),
        "fn current() {}\n",
    );
    write_text_file(workspace_root.join("src/runtime_old.rs"), "fn old() {}\n");
    write_text_file(
        workspace_root.join(".pmcli/canonical_lineage.json"),
        &serde_json::to_string_pretty(&json!({
            "schema_version": "canonical_lineage.v1",
            "source": "test",
            "current_generation": "g2",
            "policy": "current_nodes_must_not_depend_on_non_current_sources",
            "nodes": [
                {
                    "artifact_id": "doc.plan.current",
                    "family": "doc",
                    "role": "implementation_plan",
                    "path": "docs/plan-current.md",
                    "status": "current",
                    "generation": "g2",
                    "derived_from": [],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "code.runtime.current",
                    "family": "code",
                    "role": "runtime",
                    "path": "src/runtime_current.rs",
                    "status": "current",
                    "generation": "g2",
                    "derived_from": ["doc.plan.current"],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "doc.plan.old",
                    "family": "doc",
                    "role": "implementation_plan_legacy",
                    "path": "docs/plan-old.md",
                    "status": "superseded",
                    "generation": "g1",
                    "derived_from": [],
                    "evidence_refs": []
                },
                {
                    "artifact_id": "code.runtime.old",
                    "family": "code",
                    "role": "runtime_legacy",
                    "path": "src/runtime_old.rs",
                    "status": "superseded",
                    "generation": "g1",
                    "derived_from": ["doc.plan.old"],
                    "evidence_refs": []
                }
            ]
        }))
        .expect("lineage manifest should serialize"),
    );

    let add_fixture = Command::new("git")
        .current_dir(&workspace_root)
        .args([
            "add",
            "README.md",
            ".gitignore",
            "docs",
            "src",
            ".pmcli/canonical_surface.json",
            ".pmcli/code_owner_manifest.json",
            ".pmcli/canonical_lineage.json",
        ])
        .status()
        .expect("git add fixture should execute");
    assert!(add_fixture.success());
    let commit_fixture = Command::new("git")
        .current_dir(&workspace_root)
        .args(["commit", "-q", "-m", "canonical lineage fixture"])
        .status()
        .expect("git commit fixture should execute");
    assert!(commit_fixture.success());

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let cleanup_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--canonical", "--json"])
        .output()
        .expect("repo cleanup-plan should execute");
    assert!(
        cleanup_plan_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cleanup_plan_output.stderr)
    );
    let cleanup_plan_json = output_json(&cleanup_plan_output);
    assert!(cleanup_plan_json["data"]["review_gates"]
        .as_array()
        .expect("review gates")
        .iter()
        .any(|gate| gate["gate_id"] == "canonicality_audit" && gate["status"] == "satisfied"));
    let git_governance = &cleanup_plan_json["data"]["git_governance"];
    assert_eq!(
        git_governance["authority"],
        "git_worktree_diff_is_canonical_cleanup_record"
    );
    assert_eq!(git_governance["is_repository"], true);
    assert_eq!(git_governance["commit_required"], true);
    assert!(
        git_governance["head_commit_before"]
            .as_str()
            .expect("head commit before")
            .len()
            >= 7
    );
    let expected_deletions = git_governance["expected_tracked_deletions"]
        .as_array()
        .expect("expected tracked deletions");
    assert!(expected_deletions
        .iter()
        .any(|path| path == "docs/plan-old.md"));
    assert!(expected_deletions
        .iter()
        .any(|path| path == "src/runtime_old.rs"));
    assert!(cleanup_plan_json["data"]["review_gates"]
        .as_array()
        .expect("review gates")
        .iter()
        .any(|gate| {
            gate["gate_id"] == "git_native_commit_required"
                && gate["status"] == "satisfied"
                && gate["detail"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("git working tree")
        }));
    let actions = cleanup_plan_json["data"]["proposed_actions"]
        .as_array()
        .expect("proposed actions");
    let old_code_action = actions
        .iter()
        .find(|action| {
            action["action_type"] == "move_to_archive"
                && action["target_path"]
                    .as_str()
                    .unwrap_or_default()
                    .ends_with("src/runtime_old.rs")
        })
        .expect("old code should be scheduled for archive move");
    let old_code_destination = old_code_action["destination_path"]
        .as_str()
        .expect("destination path should exist")
        .to_string();
    assert!(actions.iter().any(|action| {
        action["action_type"] == "move_to_archive"
            && action["target_path"]
                .as_str()
                .unwrap_or_default()
                .ends_with("docs/plan-old.md")
            && action["git_effect"] == "tracked_public_deletion_with_ignored_rollback_copy"
    }));
    let plan_id = cleanup_plan_json["data"]["plan_id"]
        .as_str()
        .expect("plan id should exist");

    let projectops_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status should execute");
    assert!(
        projectops_status_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&projectops_status_output.stderr)
    );
    let projectops_status_json = output_json(&projectops_status_output);
    assert_eq!(projectops_status_json["data"]["pending_cleanup_count"], 1);
    assert!(projectops_status_json["data"]["cleanup_proposals"]
        .as_array()
        .expect("cleanup proposals should exist")
        .iter()
        .any(|proposal| proposal["plan_id"] == plan_id
            && proposal["status"] == "pending_review"
            && proposal["git_authority"] == "git_worktree_diff_is_canonical_cleanup_record"
            && proposal["git_commit_required"] == true
            && proposal["expected_tracked_deletion_count"] == 2
            && proposal["move_to_archive_count"] == 2
            && proposal["canonical_repair_count"] == 2));

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(
        surface_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&surface_output.stderr)
    );
    let surface_json = output_json(&surface_output);
    assert!(
        surface_json["data"]["projection"]["research"]["board"]["entries"]
            .as_array()
            .expect("board entries should exist")
            .iter()
            .any(
                |entry| entry["source"]["source_kind"] == "repo_cleanup_proposal"
                    && entry["source"]["source_id"] == plan_id
                    && entry["bucket_id"] == "needs_approval"
            )
    );

    let cleanup_apply_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-apply", plan_id, "--json"])
        .output()
        .expect("repo cleanup-apply should execute");
    assert!(
        cleanup_apply_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cleanup_apply_output.stderr)
    );
    assert!(!workspace_root.join("src/runtime_old.rs").exists());
    assert!(Path::new(&old_code_destination).exists());
    let cleanup_apply_json = output_json(&cleanup_apply_output);
    let post_apply_dirty = cleanup_apply_json["data"]["git_governance"]["post_apply_dirty_files"]
        .as_array()
        .expect("post apply dirty files");
    assert!(post_apply_dirty
        .iter()
        .any(|path| path == "docs/plan-old.md"));
    assert!(post_apply_dirty
        .iter()
        .any(|path| path == "src/runtime_old.rs"));
    let git_status_output = Command::new("git")
        .current_dir(&workspace_root)
        .args(["status", "--porcelain=v1"])
        .output()
        .expect("git status should execute");
    assert!(git_status_output.status.success());
    let git_status = String::from_utf8_lossy(&git_status_output.stdout);
    assert!(
        git_status.contains(" D docs/plan-old.md"),
        "status: {git_status}"
    );
    assert!(
        git_status.contains(" D src/runtime_old.rs"),
        "status: {git_status}"
    );
    assert!(
        !git_status.contains(".pmcli/archive"),
        "archive rollback copies must remain git-ignored: {git_status}"
    );
    assert!(
        !git_status.contains(".pmcli/repo_governance"),
        "cleanup plan snapshots must remain git-ignored: {git_status}"
    );

    let projectops_after_apply_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status after apply should execute");
    assert!(projectops_after_apply_output.status.success());
    let projectops_after_apply_json = output_json(&projectops_after_apply_output);
    assert_eq!(
        projectops_after_apply_json["data"]["applied_cleanup_count"],
        1
    );
    assert!(projectops_after_apply_json["data"]["cleanup_proposals"]
        .as_array()
        .expect("cleanup proposals should exist")
        .iter()
        .any(|proposal| proposal["plan_id"] == plan_id
            && proposal["status"] == "applied"
            && proposal["git_commit_required"] == true
            && proposal["expected_tracked_deletion_count"] == 2));

    let audit_after_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "audit", "--canonical", "--json"])
        .output()
        .expect("projects audit should execute");
    assert!(audit_after_output.status.success());
    let audit_after_json = output_json(&audit_after_output);
    assert_eq!(audit_after_json["data"]["ok"], true);

    let cleanup_restore_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-restore", plan_id, "--json"])
        .output()
        .expect("repo cleanup-restore should execute");
    assert!(
        cleanup_restore_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cleanup_restore_output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(workspace_root.join("src/runtime_old.rs"))
            .expect("old code should be restored"),
        "fn old() {}\n"
    );
    assert!(!Path::new(&old_code_destination).exists());
    let restored_status_output = Command::new("git")
        .current_dir(&workspace_root)
        .args(["status", "--porcelain=v1"])
        .output()
        .expect("git status after restore should execute");
    assert!(restored_status_output.status.success());
    let restored_status = String::from_utf8_lossy(&restored_status_output.stdout);
    assert!(
        !restored_status.contains("docs/plan-old.md")
            && !restored_status.contains("src/runtime_old.rs")
            && !restored_status.contains(".pmcli/archive"),
        "restore should remove tracked deletions and ignored rollback copy: {restored_status}"
    );
}

#[test]
fn projects_audit_release_requires_explicit_canonical_manifests() {
    let workspace_root = init_git_workspace("projects_audit_release_manifest");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "canonical status\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let audit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "audit", "--canonical", "--release", "--json"])
        .output()
        .expect("release audit should execute");
    assert!(audit_output.status.success());
    let audit_json = output_json(&audit_output);
    assert_eq!(audit_json["command"], "projects audit");
    assert_eq!(audit_json["data"]["ok"], false);
    assert_eq!(audit_json["data"]["audit_mode"], "canonical_release");
    assert!(audit_json["data"]["blocking_violations"]
        .as_array()
        .expect("blocking violations")
        .iter()
        .any(|violation| violation["violation_type"] == "missing_explicit_manifest"));
}

#[test]
fn repo_cleanup_plan_canonical_blocks_apply_on_active_surface_violations() {
    let workspace_root = init_git_workspace("repo_cleanup_canonical");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "canonical status\n");
    write_text_file(
        workspace_root.join("docs/plan-a.md"),
        r#"---
doc_frame:
  id: cleanup.plan.a
  title: Cleanup Plan A
  doc_type: implementation_plan
  lifecycle: active
  scope: project
  summary: First active implementation plan.
  generated_by: test
  updated_at: 2026-04-25
---

# Plan A
"#,
    );
    write_text_file(
        workspace_root.join("docs/plan-b.md"),
        r#"---
doc_frame:
  id: cleanup.plan.b
  title: Cleanup Plan B
  doc_type: implementation_plan
  lifecycle: active
  scope: project
  summary: Second active implementation plan.
  generated_by: test
  updated_at: 2026-04-25
---

# Plan B
"#,
    );

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let cleanup_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--canonical", "--json"])
        .output()
        .expect("canonical cleanup-plan should execute");
    assert!(
        cleanup_plan_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cleanup_plan_output.stderr)
    );
    let cleanup_plan_json = output_json(&cleanup_plan_output);
    assert_eq!(cleanup_plan_json["command"], "repo cleanup-plan");
    assert!(cleanup_plan_json["data"]["review_gates"]
        .as_array()
        .expect("review gates array")
        .iter()
        .any(|gate| gate["gate_id"] == "canonicality_audit" && gate["status"] == "blocked"));
    let plan_id = cleanup_plan_json["data"]["plan_id"]
        .as_str()
        .expect("plan id should be string");

    let cleanup_apply_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-apply", plan_id, "--json"])
        .output()
        .expect("canonical cleanup-apply should execute");
    assert!(!cleanup_apply_output.status.success());
    let cleanup_apply_json = output_json(&cleanup_apply_output);
    assert_eq!(cleanup_apply_json["command"], "repo cleanup-apply");
    assert_eq!(cleanup_apply_json["error"]["code"], "cleanup_plan_invalid");
}

#[test]
fn artifacts_promote_blocks_when_explicit_manifest_has_active_surface_violations() {
    let workspace_root = init_git_workspace("artifacts_promote_canonical_block");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "canonical status\n");
    write_explicit_canonical_manifests(&workspace_root);
    write_text_file(
        workspace_root.join("docs/active-a.md"),
        r#"---
doc_frame:
  id: promote.active.a
  title: Active A
  doc_type: implementation_plan
  lifecycle: active
  scope: project
  summary: First active implementation plan.
  generated_by: test
  updated_at: 2026-04-26
---

# Active A
"#,
    );
    write_text_file(
        workspace_root.join("docs/active-b.md"),
        r#"---
doc_frame:
  id: promote.active.b
  title: Active B
  doc_type: implementation_plan
  lifecycle: active
  scope: project
  summary: Second active implementation plan.
  generated_by: test
  updated_at: 2026-04-26
---

# Active B
"#,
    );

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Blocked Promote", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let candidate_id = output_json(&compact_output)["data"]["promotion_candidate_id"]
        .as_str()
        .expect("candidate id")
        .to_string();

    let promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "promote", &candidate_id, "--json"])
        .output()
        .expect("artifacts promote should execute");
    assert!(!promote_output.status.success());
    let promote_json = output_json(&promote_output);
    assert_eq!(promote_json["command"], "artifacts promote");
    assert_eq!(promote_json["error"]["code"], "canonicality_gate_blocked");
    assert!(promote_json["data"]["blocking_violations"]
        .as_array()
        .expect("blocking violations")
        .iter()
        .any(|violation| violation["violation_type"] == "duplicate_active_doc"));
}

#[test]
fn sessions_create_list_and_resume_latest_are_project_scoped() {
    let workspace_root = init_git_workspace("cli_sessions");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Boot Session", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );

    let create_json: Value = serde_json::from_slice(&create_output.stdout).expect("json");
    let session_id = create_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "list", "--json"])
        .output()
        .expect("sessions list should execute");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );

    let list_json: Value = serde_json::from_slice(&list_output.stdout).expect("json");
    assert_eq!(list_json["ok"], true);
    assert_eq!(list_json["data"]["sessions"][0]["session_id"], session_id);

    let resume_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "resume", "latest", "--json"])
        .output()
        .expect("sessions resume latest should execute");
    assert!(
        resume_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resume_output.stderr)
    );

    let resume_json: Value = serde_json::from_slice(&resume_output.stdout).expect("json");
    assert_eq!(resume_json["ok"], true);
    assert_eq!(resume_json["data"]["session"]["session_id"], session_id);
}

#[test]
fn sessions_rename_delete_and_prune_are_project_scoped() {
    let workspace_root = init_git_workspace("cli_sessions_manage");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_keep = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Keep", "--json"])
        .output()
        .expect("sessions create keep should execute");
    assert!(create_keep.status.success());
    let keep_id = output_json(&create_keep)["data"]["session"]["session_id"]
        .as_str()
        .expect("keep id")
        .to_string();

    let rename = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "sessions",
            "rename",
            &keep_id,
            "--title",
            "Renamed Session",
            "--json",
        ])
        .output()
        .expect("sessions rename should execute");
    assert!(
        rename.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rename.stderr)
    );
    let rename_json = output_json(&rename);
    assert_eq!(rename_json["command"], "sessions rename");
    assert_eq!(rename_json["data"]["session"]["session_id"], keep_id);
    assert_eq!(rename_json["data"]["session"]["title"], "Renamed Session");

    let create_delete = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Delete Me", "--json"])
        .output()
        .expect("sessions create delete should execute");
    assert!(create_delete.status.success());
    let delete_id = output_json(&create_delete)["data"]["session"]["session_id"]
        .as_str()
        .expect("delete id")
        .to_string();

    let delete = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "delete", &delete_id, "--json"])
        .output()
        .expect("sessions delete should execute");
    assert!(
        delete.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&delete.stderr)
    );
    let delete_json = output_json(&delete);
    assert_eq!(delete_json["command"], "sessions delete");
    assert_eq!(delete_json["data"]["deleted_session_id"], delete_id);
    assert_eq!(delete_json["data"]["deleted"], true);

    let list_after_delete = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "list", "--json"])
        .output()
        .expect("sessions list should execute");
    assert!(list_after_delete.status.success());
    let listed = output_json(&list_after_delete)["data"]["sessions"]
        .as_array()
        .expect("sessions array")
        .clone();
    assert!(listed
        .iter()
        .any(|session| session["session_id"] == keep_id));
    assert!(!listed
        .iter()
        .any(|session| session["session_id"] == delete_id));

    let create_stale = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Stale", "--json"])
        .output()
        .expect("sessions create stale should execute");
    assert!(create_stale.status.success());
    let stale_id = output_json(&create_stale)["data"]["session"]["session_id"]
        .as_str()
        .expect("stale id")
        .to_string();
    let stale_session_path = workspace_root
        .join(".pmcli/sessions")
        .join(&stale_id)
        .join("session.json");
    let mut stale_session: Value = serde_json::from_str(
        &std::fs::read_to_string(&stale_session_path).expect("stale session should read"),
    )
    .expect("stale session should parse");
    stale_session["status"] = json!("archived");
    std::fs::write(
        &stale_session_path,
        serde_json::to_string_pretty(&stale_session).expect("stale session should serialize"),
    )
    .expect("stale session should write");

    let prune_dry_run = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "prune", "--json"])
        .output()
        .expect("sessions prune dry-run should execute");
    assert!(prune_dry_run.status.success());
    let prune_dry_json = output_json(&prune_dry_run);
    assert_eq!(prune_dry_json["command"], "sessions prune");
    assert_eq!(prune_dry_json["data"]["dry_run"], true);
    assert!(prune_dry_json["data"]["candidate_session_ids"]
        .as_array()
        .expect("candidate ids")
        .iter()
        .any(|candidate| candidate == &stale_id));
    assert!(workspace_root
        .join(".pmcli/sessions")
        .join(&stale_id)
        .exists());

    let prune_apply = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "prune", "--apply", "--json"])
        .output()
        .expect("sessions prune apply should execute");
    assert!(
        prune_apply.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&prune_apply.stderr)
    );
    let prune_apply_json = output_json(&prune_apply);
    assert_eq!(prune_apply_json["data"]["dry_run"], false);
    assert!(prune_apply_json["data"]["pruned_session_ids"]
        .as_array()
        .expect("pruned ids")
        .iter()
        .any(|candidate| candidate == &stale_id));
    assert!(!workspace_root
        .join(".pmcli/sessions")
        .join(&stale_id)
        .exists());
}

#[test]
fn projects_prune_returns_typed_dry_run_result_without_mutating_registry_state() {
    let workspace_root = init_git_workspace("projects_prune_typed");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let before = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "list", "--json"])
        .output()
        .expect("projects list should execute");
    assert!(before.status.success());
    let before_json = output_json(&before);

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "prune", "--json"])
        .output()
        .expect("projects prune should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "projects prune");
    assert_eq!(json["data"]["dry_run"], true);
    assert!(json["data"]["candidate_refs"].is_array());
    assert!(json["data"]["pruned_refs"].is_array());
    assert!(json["data"]["blocking_conflicts"].is_array());

    let after = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "list", "--json"])
        .output()
        .expect("projects list should execute");
    assert!(after.status.success());
    let after_json = output_json(&after);
    assert_eq!(
        before_json["data"]["projects"],
        after_json["data"]["projects"]
    );
}

#[test]
fn unknown_commands_return_typed_failure_json_and_usage_exit_code() {
    let registry_root = registry_root_env();
    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("unknown_command_workspace"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["wat", "--json"])
        .output()
        .expect("unknown command should execute");

    assert_eq!(output.status.code(), Some(2));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "wat");
    assert_eq!(json["error"]["code"], "usage_invalid");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("unknown command"));
}

#[test]
fn corrupted_project_event_log_does_not_block_typed_unknown_command_failures() {
    let workspace_root = init_git_workspace("corrupt_command_audit");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    write_text_file(
        workspace_root.join(".pmcli/events/events.jsonl"),
        "{\"event_id\":\"broken\"",
    );

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["wat", "--json"])
        .output()
        .expect("unknown command should still execute");

    assert_eq!(output.status.code(), Some(2));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "wat");
    assert_eq!(json["error"]["code"], "usage_invalid");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("unknown command"));
}

#[test]
fn unknown_subcommands_return_typed_failure_json_and_usage_exit_code() {
    let workspace_root = init_git_workspace("unknown_subcommand");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "wat", "--json"])
        .output()
        .expect("unknown subcommand should execute");

    assert_eq!(output.status.code(), Some(2));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "projects wat");
    assert_eq!(json["error"]["code"], "usage_invalid");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("unknown projects subcommand"));
}

#[test]
fn permissions_pending_and_history_return_empty_typed_lists() {
    let workspace_root = init_git_workspace("permissions_typed_lists");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let pending = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "pending", "--json"])
        .output()
        .expect("permissions pending should execute");
    assert!(
        pending.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pending.stderr)
    );
    let pending_json = output_json(&pending);
    assert_eq!(pending_json["ok"], true);
    assert_eq!(pending_json["command"], "permissions pending");
    assert!(pending_json["data"]["requests"].is_array());
    assert_eq!(pending_json["data"]["total_count"], 0);

    let history = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "history", "--json"])
        .output()
        .expect("permissions history should execute");
    assert!(
        history.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&history.stderr)
    );
    let history_json = output_json(&history);
    assert_eq!(history_json["ok"], true);
    assert_eq!(history_json["command"], "permissions history");
    assert!(history_json["data"]["decisions"].is_array());
    assert_eq!(history_json["data"]["total_count"], 0);
}

#[test]
fn permissions_approve_resolves_pending_request_into_history() {
    let workspace_root = init_git_workspace("permissions_approve");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "prompt",
            "please write a file",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("permission-blocked prompt should execute");
    assert_eq!(blocked.status.code(), Some(5));
    let blocked_json = output_json(&blocked);
    assert_eq!(blocked_json["error"]["code"], "permission_unresolved");
    let request_id = blocked_json["data"]["request_id"]
        .as_str()
        .expect("blocked prompt should expose request id")
        .to_string();

    let pending = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "pending", "--json"])
        .output()
        .expect("permissions pending should execute");
    assert!(pending.status.success());
    let pending_json = output_json(&pending);
    assert_eq!(pending_json["data"]["total_count"], 1);
    assert_eq!(
        pending_json["data"]["requests"][0]["request_id"],
        request_id
    );
    assert_eq!(pending_json["data"]["requests"][0]["status"], "pending");

    let approve = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "approve", &request_id, "--json"])
        .output()
        .expect("permissions approve should execute");
    assert!(
        approve.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&approve.stderr)
    );
    let approve_json = output_json(&approve);
    assert_eq!(approve_json["ok"], true);
    assert_eq!(approve_json["command"], "permissions approve");
    assert_eq!(approve_json["data"]["request_id"], request_id);
    assert_eq!(approve_json["data"]["decision"], "approved");

    let pending_after = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "pending", "--json"])
        .output()
        .expect("permissions pending should execute after approve");
    assert!(pending_after.status.success());
    let pending_after_json = output_json(&pending_after);
    assert_eq!(pending_after_json["data"]["total_count"], 0);

    let history = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "history", "--json"])
        .output()
        .expect("permissions history should execute");
    assert!(history.status.success());
    let history_json = output_json(&history);
    assert_eq!(history_json["data"]["total_count"], 1);
    assert_eq!(
        history_json["data"]["decisions"][0]["request_id"],
        request_id
    );
    assert_eq!(history_json["data"]["decisions"][0]["decision"], "approved");

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "permission"
            && event["phase"] == "terminal"
            && event["object_id"] == request_id
            && event["payload"]["decision"] == "approved"
    }));
}

#[test]
fn permissions_deny_and_expire_are_single_use_decisions() {
    let workspace_root = init_git_workspace("permissions_deny_expire");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let denied_blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "prompt",
            "please edit a file",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("permission-blocked prompt should execute");
    assert_eq!(denied_blocked.status.code(), Some(5));
    let denied_request_id = output_json(&denied_blocked)["data"]["request_id"]
        .as_str()
        .expect("denied request id")
        .to_string();

    let deny = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "deny", &denied_request_id, "--json"])
        .output()
        .expect("permissions deny should execute");
    assert!(deny.status.success());
    let deny_json = output_json(&deny);
    assert_eq!(deny_json["data"]["decision"], "denied");

    let deny_again = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "deny", &denied_request_id, "--json"])
        .output()
        .expect("second permissions deny should execute");
    assert_eq!(deny_again.status.code(), Some(6));
    let deny_again_json = output_json(&deny_again);
    assert_eq!(
        deny_again_json["error"]["code"],
        "permission_request_resolved"
    );

    let expired_blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_PERMISSION_REQUEST_TTL_MS", "0")
        .args([
            "prompt",
            "please modify another file",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("expired permission-blocked prompt should execute");
    assert_eq!(expired_blocked.status.code(), Some(5));
    let expired_request_id = output_json(&expired_blocked)["data"]["request_id"]
        .as_str()
        .expect("expired request id")
        .to_string();

    let approve_expired = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "approve", &expired_request_id, "--json"])
        .output()
        .expect("expired permissions approve should execute");
    assert_eq!(approve_expired.status.code(), Some(6));
    let approve_expired_json = output_json(&approve_expired);
    assert_eq!(
        approve_expired_json["error"]["code"],
        "permission_request_expired"
    );

    let history = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "history", "--json"])
        .output()
        .expect("permissions history should execute");
    assert!(history.status.success());
    let history_json = output_json(&history);
    assert_eq!(history_json["data"]["total_count"], 2);
    assert!(history_json["data"]["decisions"]
        .as_array()
        .expect("history decisions")
        .iter()
        .any(|decision| {
            decision["request_id"] == denied_request_id && decision["decision"] == "denied"
        }));
    assert!(history_json["data"]["decisions"]
        .as_array()
        .expect("history decisions")
        .iter()
        .any(|decision| {
            decision["request_id"] == expired_request_id && decision["decision"] == "expired"
        }));
}

#[test]
fn tools_run_enforces_permission_policy_and_reuses_approved_request_once() {
    let workspace_root = init_git_workspace("tools_permission_policy");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "fixture read");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let read = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "read_file",
            "--path",
            "README.md",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("read_file tool should execute");
    assert!(
        read.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    let read_json = output_json(&read);
    assert_eq!(read_json["ok"], true);
    assert_eq!(read_json["command"], "tools run");
    assert_eq!(read_json["data"]["tool_name"], "read_file");
    assert_eq!(read_json["data"]["status"], "succeeded");
    assert_eq!(read_json["data"]["structured"]["path"], "README.md");
    assert_eq!(read_json["data"]["output"], "fixture read");

    let blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "notes.txt",
            "--content",
            "created by tool",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("write_file should request permission under read-only");
    assert_eq!(blocked.status.code(), Some(5));
    let blocked_json = output_json(&blocked);
    assert_eq!(blocked_json["error"]["code"], "permission_unresolved");
    assert_eq!(blocked_json["data"]["tool_name"], "write_file");
    assert_eq!(blocked_json["data"]["target_path"], "notes.txt");
    let request_id = blocked_json["data"]["request_id"]
        .as_str()
        .expect("permission request id")
        .to_string();
    assert!(!workspace_root.join("notes.txt").exists());

    let approve = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "approve", &request_id, "--json"])
        .output()
        .expect("permissions approve should execute");
    assert!(approve.status.success());

    let approved_write = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "notes.txt",
            "--content",
            "created by tool",
            "--permission-mode",
            "read-only",
            "--permission-request",
            &request_id,
            "--json",
        ])
        .output()
        .expect("approved write_file should execute");
    assert!(
        approved_write.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&approved_write.stderr)
    );
    let approved_write_json = output_json(&approved_write);
    assert_eq!(approved_write_json["data"]["status"], "succeeded");
    assert_eq!(
        approved_write_json["data"]["permission_request_id"],
        request_id
    );
    assert_eq!(
        std::fs::read_to_string(workspace_root.join("notes.txt"))
            .expect("written note should be readable"),
        "created by tool"
    );

    let history_after_write = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "history", "--json"])
        .output()
        .expect("permissions history should execute after consuming approval");
    assert!(history_after_write.status.success());
    let history_after_write_json = output_json(&history_after_write);
    assert_eq!(history_after_write_json["data"]["total_count"], 1);
    assert_eq!(
        history_after_write_json["data"]["decisions"][0]["decision"],
        "approved"
    );

    let replay = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "notes2.txt",
            "--content",
            "replay",
            "--permission-mode",
            "read-only",
            "--permission-request",
            &request_id,
            "--json",
        ])
        .output()
        .expect("replayed permission request should execute");
    assert_eq!(replay.status.code(), Some(6));
    let replay_json = output_json(&replay);
    assert_eq!(replay_json["error"]["code"], "permission_request_resolved");
    assert!(!workspace_root.join("notes2.txt").exists());
}

#[test]
fn tools_run_permission_request_binds_full_effective_input() {
    let workspace_root = init_git_workspace("tools_permission_input_digest");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "shell",
            "--command",
            "touch approved.txt",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("shell should request permission under read-only");
    assert_eq!(blocked.status.code(), Some(5));
    let request_id = output_json(&blocked)["data"]["request_id"]
        .as_str()
        .expect("permission request id")
        .to_string();

    let approve = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "approve", &request_id, "--json"])
        .output()
        .expect("permissions approve should execute");
    assert!(approve.status.success());

    let mismatched = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "shell",
            "--command",
            "touch bypass.txt",
            "--permission-mode",
            "read-only",
            "--permission-request",
            &request_id,
            "--json",
        ])
        .output()
        .expect("mismatched shell should execute");
    assert_eq!(mismatched.status.code(), Some(6));
    let mismatched_json = output_json(&mismatched);
    assert_eq!(
        mismatched_json["error"]["code"],
        "permission_request_mismatch"
    );
    assert!(!workspace_root.join("bypass.txt").exists());

    let approved = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "shell",
            "--command",
            "touch approved.txt",
            "--permission-mode",
            "read-only",
            "--permission-request",
            &request_id,
            "--json",
        ])
        .output()
        .expect("matching shell should execute");
    assert!(approved.status.success(), "{approved:?}");
    assert!(workspace_root.join("approved.txt").exists());
}

#[test]
fn remote_daemon_public_bind_requires_control_token() {
    let workspace_root = init_git_workspace("remote_daemon_public_token_required");
    let registry_root = registry_root_env();

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
        .args([
            "remote", "daemon", "--host", "0.0.0.0", "--port", "0", "--json",
        ])
        .output()
        .expect("remote daemon should reject missing token");
    assert_eq!(output.status.code(), Some(2));
    let json = output_json(&output);
    assert_eq!(json["error"]["code"], "usage_invalid");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("control token"));
}

#[test]
fn tools_run_blocks_workspace_escape_without_permission_request() {
    let workspace_root = init_git_workspace("tools_workspace_escape");
    let registry_root = registry_root_env();
    let outside_root = unique_temp_dir("tools_symlink_escape_outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_root, workspace_root.join("link_out"))
        .expect("symlink fixture should be created");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let escaped = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "../outside.txt",
            "--content",
            "escape",
            "--permission-mode",
            "workspace-write",
            "--json",
        ])
        .output()
        .expect("workspace escape should execute");
    assert_eq!(escaped.status.code(), Some(7));
    let escaped_json = output_json(&escaped);
    assert_eq!(escaped_json["error"]["code"], "policy_refusal");
    assert_eq!(
        escaped_json["data"]["policy_refusal"]["reason_code"],
        "workspace_scope_violation"
    );

    let pending = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "pending", "--json"])
        .output()
        .expect("permissions pending should execute");
    assert!(pending.status.success());
    assert_eq!(output_json(&pending)["data"]["total_count"], 0);

    #[cfg(unix)]
    {
        let symlink_escape = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args([
                "tools",
                "run",
                "write_file",
                "--path",
                "link_out/escaped.txt",
                "--content",
                "escape",
                "--permission-mode",
                "workspace-write",
                "--json",
            ])
            .output()
            .expect("symlink workspace escape should execute");
        assert_eq!(symlink_escape.status.code(), Some(7));
        let symlink_escape_json = output_json(&symlink_escape);
        assert_eq!(symlink_escape_json["error"]["code"], "policy_refusal");
        assert_eq!(
            symlink_escape_json["data"]["policy_refusal"]["reason_code"],
            "workspace_scope_violation"
        );
        assert!(!outside_root.join("escaped.txt").exists());
    }
}

#[test]
fn tools_run_rejects_denied_and_expired_permission_requests() {
    let workspace_root = init_git_workspace("tools_denied_expired_permission");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let denied_blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "denied.txt",
            "--content",
            "denied",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("denied tool request should execute");
    assert_eq!(denied_blocked.status.code(), Some(5));
    let denied_request_id = output_json(&denied_blocked)["data"]["request_id"]
        .as_str()
        .expect("denied request id")
        .to_string();

    let deny = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "deny", &denied_request_id, "--json"])
        .output()
        .expect("permissions deny should execute");
    assert!(deny.status.success());

    let denied_retry = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "denied.txt",
            "--content",
            "denied",
            "--permission-mode",
            "read-only",
            "--permission-request",
            &denied_request_id,
            "--json",
        ])
        .output()
        .expect("denied tool retry should execute");
    assert_eq!(denied_retry.status.code(), Some(6));
    assert_eq!(
        output_json(&denied_retry)["error"]["code"],
        "permission_denied"
    );
    assert!(!workspace_root.join("denied.txt").exists());

    let expired_blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_PERMISSION_REQUEST_TTL_MS", "0")
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "expired.txt",
            "--content",
            "expired",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("expired tool request should execute");
    assert_eq!(expired_blocked.status.code(), Some(5));
    let expired_request_id = output_json(&expired_blocked)["data"]["request_id"]
        .as_str()
        .expect("expired request id")
        .to_string();

    let expired_retry = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "expired.txt",
            "--content",
            "expired",
            "--permission-mode",
            "read-only",
            "--permission-request",
            &expired_request_id,
            "--json",
        ])
        .output()
        .expect("expired tool retry should execute");
    assert_eq!(expired_retry.status.code(), Some(6));
    assert_eq!(
        output_json(&expired_retry)["error"]["code"],
        "permission_request_expired"
    );
    assert!(!workspace_root.join("expired.txt").exists());
}

#[test]
fn tools_run_executes_registered_file_and_shell_tools() {
    let workspace_root = init_git_workspace("tools_registered_execution");
    let registry_root = registry_root_env();
    std::fs::create_dir_all(workspace_root.join("src")).expect("src directory should write");
    write_text_file(workspace_root.join("README.md"), "hello fixture\n");
    write_text_file(workspace_root.join("src/lib.rs"), "fn fixture() {}\n");
    write_text_file(workspace_root.join("tmp.txt"), "temporary\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let list = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "list_files",
            "--path",
            ".",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("list_files should execute");
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let list_json = output_json(&list);
    let files = list_json["data"]["structured"]["files"]
        .as_array()
        .expect("list_files should return files");
    assert!(files.iter().any(|file| file == "README.md"));
    assert!(files.iter().any(|file| file == "src/lib.rs"));

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "search",
            "--path",
            ".",
            "--pattern",
            "fixture",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("search should execute");
    assert!(
        search.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json = output_json(&search);
    assert_eq!(search_json["data"]["structured"]["match_count"], 2);

    let listener = TcpListener::bind("127.0.0.1:0").expect("fetch server should bind");
    let addr = listener
        .local_addr()
        .expect("fetch server addr should resolve");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fetch server should accept");
        let mut buffer = [0u8; 1024];
        let _ = stream.read(&mut buffer).expect("fetch request should read");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 16\r\n\r\nfixture fetched\n",
            )
            .expect("fetch response should write");
    });

    let fetch = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "fetch",
            "--url",
            &format!("http://{addr}/fixture"),
            "--allow-private-network",
            "true",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("fetch should execute");
    handle.join().expect("fetch server should join");
    assert!(
        fetch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&fetch.stderr)
    );
    let fetch_json = output_json(&fetch);
    assert_eq!(fetch_json["data"]["tool_name"], "fetch");
    assert_eq!(fetch_json["data"]["structured"]["status_code"], 200);
    assert_eq!(fetch_json["data"]["structured"]["bytes"], 16);
    assert_eq!(fetch_json["data"]["output"], "fixture fetched\n");

    let refused_fetch = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "fetch",
            "--url",
            "http://127.0.0.1:9/blocked",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("private-network fetch should execute");
    assert!(!refused_fetch.status.success());
    let refused_fetch_json = output_json(&refused_fetch);
    assert_eq!(refused_fetch_json["error"]["code"], "tool_execution_failed");
    assert!(refused_fetch_json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("private network"));

    let patch = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "apply_patch",
            "--path",
            "README.md",
            "--old",
            "fixture",
            "--new",
            "patched",
            "--permission-mode",
            "workspace-write",
            "--json",
        ])
        .output()
        .expect("apply_patch should execute");
    assert!(
        patch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&patch.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(workspace_root.join("README.md"))
            .expect("patched readme should read"),
        "hello patched\n"
    );

    let delete_blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "delete_file",
            "--path",
            "tmp.txt",
            "--permission-mode",
            "workspace-write",
            "--json",
        ])
        .output()
        .expect("delete_file should request approval");
    assert_eq!(delete_blocked.status.code(), Some(5));
    let request_id = output_json(&delete_blocked)["data"]["request_id"]
        .as_str()
        .expect("delete request id")
        .to_string();
    assert!(workspace_root.join("tmp.txt").exists());

    let approve = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "approve", &request_id, "--json"])
        .output()
        .expect("permissions approve should execute");
    assert!(approve.status.success());

    let delete = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "delete_file",
            "--path",
            "tmp.txt",
            "--permission-mode",
            "workspace-write",
            "--permission-request",
            &request_id,
            "--json",
        ])
        .output()
        .expect("approved delete_file should execute");
    assert!(
        delete.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&delete.stderr)
    );
    assert!(!workspace_root.join("tmp.txt").exists());

    let shell = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "shell",
            "--command",
            "cat README.md",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("read-only shell should execute");
    assert!(
        shell.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&shell.stderr)
    );
    assert_eq!(output_json(&shell)["data"]["output"], "hello patched\n");

    let shell_blocked = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "tools",
            "run",
            "shell",
            "--command",
            "touch blocked.txt",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("mutating shell should request approval");
    assert_eq!(shell_blocked.status.code(), Some(5));
    assert!(!workspace_root.join("blocked.txt").exists());
}

#[test]
fn conformance_command_returns_typed_summary() {
    let registry_root = registry_root_env();
    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("conformance_command"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFORMANCE_SELFTEST", "1")
        .args(["conformance", "--json"])
        .output()
        .expect("conformance should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "conformance");
    assert!(json["data"]["scope"].is_string());
    assert!(json["data"]["passed_families"].is_array());
    assert!(json["data"]["failed_families"].is_array());
    assert!(json["data"]["failure_refs"].is_array());
    assert_eq!(json["data"]["execution_mode"], "selftest_guard");
}

#[test]
fn setup_status_and_install_routes_are_machine_readable_and_emit_setup_events() {
    let workspace_root = init_git_workspace("setup_status");
    let registry_root = registry_root_env();

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
        .args(["setup", "status", "--json"])
        .output()
        .expect("setup status should execute");
    assert!(
        status_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status_json = output_json(&status_output);
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["command"], "setup status");
    assert_eq!(status_json["data"]["overall_status"], "ready");
    assert!(status_json["data"]["state_home"].is_string());
    assert!(status_json["data"]["schema_versions"].is_array());
    assert!(status_json["data"]["data_directories"].is_array());
    assert!(status_json["data"]["install_routes"].is_object());

    let routes_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["setup", "install-routes", "--json"])
        .output()
        .expect("setup install-routes should execute");
    assert!(
        routes_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&routes_output.stderr)
    );
    let routes_json = output_json(&routes_output);
    assert_eq!(routes_json["ok"], true);
    assert_eq!(routes_json["command"], "setup install-routes");
    assert!(routes_json["data"]["platform"].is_string());
    assert!(routes_json["data"]["state_home"].is_string());
    assert!(routes_json["data"]["registry_root"].is_string());
    assert!(routes_json["data"]["pmcli_root"].is_string());
    assert!(routes_json["data"]["schema_root"].is_string());
    assert!(routes_json["data"]["skills"].is_array());
    assert!(routes_json["data"]["plugins"].is_array());
    assert!(routes_json["data"]["hooks"].is_array());
    assert!(routes_json["data"]["mcp"].is_array());
    assert!(routes_json["data"]["remote"].is_array());

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    let setup_events = events
        .iter()
        .filter(|event| event["event_name"] == "setup")
        .count();
    assert!(
        setup_events >= 2,
        "expected setup status and install-routes events, got {setup_events}"
    );
}

#[test]
fn skills_plugins_hooks_and_mcp_surfaces_discover_workspace_state() {
    let workspace_root = init_git_workspace("registry_surfaces");
    let registry_root = registry_root_env();

    write_skill_manifest(&workspace_root.join(".codex/skills"), "workspace_skill");
    write_skill_manifest(
        &Path::new(&registry_root).join("skills"),
        "state_home_skill",
    );
    write_plugin_manifest(&workspace_root.join(".agents/plugins"), "workspace_plugin");
    write_mcp_manifest(&workspace_root.join(".pmcli/mcp/servers"), "workspace_mcp");

    let skills_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "list", "--json"])
        .output()
        .expect("skills list should execute");
    assert!(
        skills_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&skills_output.stderr)
    );
    let skills_json = output_json(&skills_output);
    assert_eq!(skills_json["ok"], true);
    assert_eq!(skills_json["command"], "skills list");
    assert!(
        skills_json["data"]["total_count"]
            .as_u64()
            .expect("total count")
            >= 2
    );
    assert!(skills_json["data"]["search_paths"].is_array());
    assert!(skills_json["data"]["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .any(|entry| entry["skill_id"] == "workspace_skill"));
    assert!(skills_json["data"]["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .any(|entry| entry["skill_id"] == "state_home_skill"));

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "inspect", "workspace_skill", "--json"])
        .output()
        .expect("skills inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["command"], "skills inspect");
    assert_eq!(inspect_json["data"]["skill_id"], "workspace_skill");
    assert!(inspect_json["data"]["availability"]["enabled"]
        .as_bool()
        .unwrap());
    assert!(inspect_json["data"]["availability"]["stage_compatibility"]
        .as_array()
        .unwrap()
        .iter()
        .any(|stage| stage == "implementation"));

    let paths_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "paths", "--json"])
        .output()
        .expect("skills paths should execute");
    assert!(paths_output.status.success());
    let paths_json = output_json(&paths_output);
    assert_eq!(paths_json["command"], "skills paths");
    assert!(paths_json["data"]["winners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["skill_id"] == "workspace_skill"));

    let validate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "validate", "workspace_skill", "--json"])
        .output()
        .expect("skills validate should execute");
    assert!(validate_output.status.success());
    let validate_json = output_json(&validate_output);
    assert_eq!(validate_json["command"], "skills validate");
    assert_eq!(validate_json["data"]["scope"], "workspace_skill");
    assert_eq!(validate_json["data"]["overall_status"], "ready");
    assert_eq!(
        validate_json["data"]["availability_records"][0]["skill_id"],
        "workspace_skill"
    );

    let plugins_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["plugins", "list", "--json"])
        .output()
        .expect("plugins list should execute");
    assert!(
        plugins_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&plugins_output.stderr)
    );
    let plugins_json = output_json(&plugins_output);
    assert_eq!(plugins_json["ok"], true);
    assert_eq!(plugins_json["command"], "plugins list");
    assert_eq!(plugins_json["data"]["total_count"], 1);
    assert_eq!(
        plugins_json["data"]["plugins"][0]["plugin_id"],
        "workspace_plugin"
    );
    assert_eq!(plugins_json["data"]["plugins"][0]["hook_count"], 2);

    let hooks_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["hooks", "list", "--json"])
        .output()
        .expect("hooks list should execute");
    assert!(
        hooks_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&hooks_output.stderr)
    );
    let hooks_json = output_json(&hooks_output);
    assert_eq!(hooks_json["ok"], true);
    assert_eq!(hooks_json["command"], "hooks list");
    assert_eq!(hooks_json["data"]["total_count"], 2);
    assert!(hooks_json["data"]["hooks"]
        .as_array()
        .expect("hooks array")
        .iter()
        .any(|entry| entry["owner_plugin_id"] == "workspace_plugin"));

    let mcp_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["mcp", "list", "--json"])
        .output()
        .expect("mcp list should execute");
    assert!(
        mcp_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&mcp_output.stderr)
    );
    let mcp_json = output_json(&mcp_output);
    assert_eq!(mcp_json["ok"], true);
    assert_eq!(mcp_json["command"], "mcp list");
    assert_eq!(mcp_json["data"]["total_count"], 1);
    assert_eq!(mcp_json["data"]["servers"][0]["server_id"], "workspace_mcp");
    assert_eq!(mcp_json["data"]["servers"][0]["tool_count"], 3);
}

#[test]
fn skills_list_filters_by_stage_source_and_disabled_state() {
    let workspace_root = init_git_workspace("skills_list_filters");
    let registry_root = registry_root_env();

    let workspace_skills_root = workspace_root.join(".codex/skills");
    let state_skills_root = Path::new(&registry_root).join("skills");
    write_skill_manifest(&workspace_skills_root, "workspace_skill");
    write_skill_manifest(&state_skills_root, "state_home_skill");
    write_text_file(
        workspace_skills_root
            .join("disabled_skill")
            .join("SKILL.md"),
        "",
    );

    let filtered_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "list",
            "--stage",
            "implementation",
            "--source",
            workspace_skills_root
                .to_str()
                .expect("source path should be utf8"),
            "--json",
        ])
        .output()
        .expect("skills list filtered should execute");
    assert!(
        filtered_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&filtered_output.stderr)
    );
    let filtered_json = output_json(&filtered_output);
    let filtered_skills = filtered_json["data"]["skills"]
        .as_array()
        .expect("skills array should exist");
    assert!(filtered_skills
        .iter()
        .any(|entry| entry["skill_id"] == "workspace_skill"));
    assert!(!filtered_skills
        .iter()
        .any(|entry| entry["skill_id"] == "state_home_skill"));
    assert!(!filtered_skills
        .iter()
        .any(|entry| entry["skill_id"] == "disabled_skill"));

    let disabled_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "list", "--include-disabled", "--json"])
        .output()
        .expect("skills list include-disabled should execute");
    assert!(disabled_output.status.success());
    let disabled_json = output_json(&disabled_output);
    let disabled_entry = disabled_json["data"]["skills"]
        .as_array()
        .expect("skills array should exist")
        .iter()
        .find(|entry| entry["skill_id"] == "disabled_skill")
        .expect("disabled skill should be visible when requested");
    assert_eq!(disabled_entry["enabled"], false);
    assert_eq!(disabled_entry["degraded"], true);

    let incompatible_stage = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "list", "--stage", "paper", "--json"])
        .output()
        .expect("skills list incompatible stage should execute");
    assert!(incompatible_stage.status.success());
    assert_eq!(output_json(&incompatible_stage)["data"]["total_count"], 0);
}

#[test]
fn skills_output_envelope_publish_gate_promotes_single_public_latest() {
    let workspace_root = init_git_workspace("skill_output_publication");
    let registry_root = registry_root_env();

    write_skill_manifest(&workspace_root.join(".codex/skills"), "paper_skill");
    write_skill_manifest(&workspace_root.join(".codex/skills"), "loose_skill");
    write_text_file(
        workspace_root
            .join(".codex/skills")
            .join("paper_skill")
            .join("research_skill_contract.json"),
        &json!({
            "schema_version": "research_skill_contract.v1",
            "conformance_line": "M11.skill_publication",
            "skill_id": "paper_skill",
            "compatible_stage_classes": ["paper"],
            "required_outputs": ["doc_frame", "report"],
            "allowed_output_kinds": ["report", "experiment_result"],
            "doc_frame_required": true,
            "canonicality_policy": "requires_review",
            "human_gate_required": true
        })
        .to_string(),
    );
    write_text_file(
        workspace_root.join("docs").join("candidate.md"),
        r#"---
doc_frame:
  doc_id: docs.skill_candidate
  schema_version: "1"
  source_path: docs/candidate.md
  title: Skill Candidate
  doc_type: report
  lifecycle: active
  scope: project
  summary: Candidate generated by a skill.
  generated_by: skill
  updated_at: 9999999999999
---

# Candidate
"#,
    );
    write_text_file(
        workspace_root.join("reports").join("metrics.json"),
        r#"{"accuracy":0.91,"source":"paper_skill"}"#,
    );

    let empty_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "outputs", "list", "--json"])
        .output()
        .expect("skills outputs list should execute");
    assert!(
        empty_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&empty_output.stderr)
    );
    let empty_json = output_json(&empty_output);
    assert_eq!(empty_json["command"], "skills outputs list");
    assert_eq!(empty_json["data"]["total_count"], 0);

    let contract_blocked_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "submit",
            "--skill",
            "paper_skill",
            "--kind",
            "dataset",
            "--artifact",
            "docs/candidate.md",
            "--family",
            "skill_reports",
            "--doc-frame",
            "docs/candidate.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("contract-blocked skills output submit should execute");
    assert!(!contract_blocked_output.status.success());
    let contract_blocked_json = output_json(&contract_blocked_output);
    assert_eq!(
        contract_blocked_json["error"]["code"],
        "skill_contract_violation"
    );

    let submit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "submit",
            "--skill",
            "paper_skill",
            "--kind",
            "report",
            "--artifact",
            "docs/candidate.md",
            "--artifact",
            "reports/metrics.json",
            "--artifact-kind",
            "report",
            "--artifact-kind",
            "experiment_result",
            "--family",
            "skill_reports",
            "--doc-frame",
            "docs/candidate.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("skills output submit should execute");
    assert!(
        submit_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&submit_output.stderr)
    );
    let submit_json = output_json(&submit_output);
    assert_eq!(submit_json["command"], "skills output submit");
    assert_eq!(submit_json["data"]["status"], "candidate_recorded");
    assert_eq!(submit_json["data"]["envelope"]["skill_id"], "paper_skill");
    assert_eq!(
        submit_json["data"]["envelope"]["visibility"],
        "review_candidate"
    );
    assert_eq!(
        submit_json["data"]["envelope"]["artifact_refs"]
            .as_array()
            .expect("artifact refs should be an array")
            .len(),
        2
    );
    let envelope_id = submit_json["data"]["envelope"]["envelope_id"]
        .as_str()
        .expect("envelope id should exist")
        .to_string();

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "output", "inspect", &envelope_id, "--json"])
        .output()
        .expect("skills output inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["command"], "skills output inspect");
    assert_eq!(inspect_json["data"]["envelope"]["envelope_id"], envelope_id);
    assert_eq!(inspect_json["data"]["publication_state"], "candidate");

    let blocked_publish_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "publish", "--execute", &envelope_id, "--json"])
        .output()
        .expect("skills publish should execute");
    assert!(!blocked_publish_output.status.success());
    let blocked_publish_json = output_json(&blocked_publish_output);
    assert_eq!(
        blocked_publish_json["error"]["code"],
        "publication_gate_required"
    );

    let publish_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "publish",
            "--execute",
            &envelope_id,
            "--approve-human-gate",
            "--json",
        ])
        .output()
        .expect("approved skills publish should execute");
    assert!(
        publish_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&publish_output.stderr)
    );
    let publish_json = output_json(&publish_output);
    assert_eq!(publish_json["command"], "skills publish");
    assert_eq!(publish_json["data"]["publication_state"], "public_latest");
    assert_eq!(
        publish_json["data"]["published_envelope"]["visibility"],
        "canonical_public"
    );
    assert_eq!(
        publish_json["data"]["artifact_family"]["artifact_family"],
        "skill_reports"
    );
    assert_eq!(
        publish_json["data"]["artifact_family"]["latest_id"],
        publish_json["data"]["published_envelope"]["artifact_refs"][0]["artifact_id"]
    );
    assert_eq!(
        publish_json["data"]["publication_gate"]["decision"],
        "execute"
    );
    assert_eq!(
        publish_json["data"]["publication_gate"]["human_gate_approved"],
        true
    );
    let doc_index: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root.join(".pmcli/docs/index.json"))
            .expect("skills publish should refresh the public doc index"),
    )
    .expect("doc index should parse");
    assert!(doc_index["doc_frames"]
        .as_array()
        .expect("doc frames should be an array")
        .iter()
        .any(|frame| frame["doc_id"] == "docs.skill_candidate"));

    let canonical_surface: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root.join(".pmcli/canonical_surface.json"))
            .expect("canonical surface should be written"),
    )
    .expect("canonical surface should parse");
    assert_eq!(canonical_surface["source"], "skill_output_publication");
    assert_eq!(
        canonical_surface["skill_outputs"]["skill_reports"]["envelope_id"],
        envelope_id
    );

    write_text_file(
        workspace_root.join("docs").join("candidate-v2.md"),
        r#"---
doc_frame:
  doc_id: docs.skill_candidate_v2
  schema_version: "1"
  source_path: docs/candidate-v2.md
  title: Skill Candidate V2
  doc_type: report
  lifecycle: active
  scope: project
  summary: Second candidate generated by a skill.
  generated_by: skill
  updated_at: 2026-04-27
---

# Candidate V2
"#,
    );
    let second_submit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "submit",
            "--skill",
            "paper_skill",
            "--kind",
            "report",
            "--artifact",
            "docs/candidate-v2.md",
            "--family",
            "skill_reports",
            "--doc-frame",
            "docs/candidate-v2.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("second skills output submit should execute");
    assert!(second_submit_output.status.success());
    let second_submit_json = output_json(&second_submit_output);
    let second_envelope_id = second_submit_json["data"]["envelope"]["envelope_id"]
        .as_str()
        .expect("second envelope id")
        .to_string();
    let second_publish_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "publish",
            "--execute",
            &second_envelope_id,
            "--approve-human-gate",
            "--json",
        ])
        .output()
        .expect("second skills publish should execute");
    assert!(second_publish_output.status.success());

    let old_inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["skills", "output", "inspect", &envelope_id, "--json"])
        .output()
        .expect("old skills output inspect should execute");
    assert!(old_inspect_output.status.success());
    let old_inspect_json = output_json(&old_inspect_output);
    assert_eq!(old_inspect_json["data"]["publication_state"], "superseded");

    let updated_canonical_surface: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root.join(".pmcli/canonical_surface.json"))
            .expect("canonical surface should still exist"),
    )
    .expect("updated canonical surface should parse");
    assert_eq!(
        updated_canonical_surface["skill_outputs"]["skill_reports"]["envelope_id"],
        second_envelope_id
    );

    let private_submit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "submit",
            "--skill",
            "loose_skill",
            "--kind",
            "report",
            "--artifact",
            "docs/candidate-v2.md",
            "--family",
            "skill_reports",
            "--doc-frame",
            "docs/candidate-v2.md",
            "--policy",
            "private_only",
            "--json",
        ])
        .output()
        .expect("private-only skills output submit should execute");
    assert!(private_submit_output.status.success());
    let private_submit_json = output_json(&private_submit_output);
    let private_envelope_id = private_submit_json["data"]["envelope"]["envelope_id"]
        .as_str()
        .expect("private envelope id")
        .to_string();
    let private_publish_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "publish",
            "--execute",
            &private_envelope_id,
            "--approve-human-gate",
            "--json",
        ])
        .output()
        .expect("private-only skills publish should execute");
    assert!(!private_publish_output.status.success());
    let private_publish_json = output_json(&private_publish_output);
    assert_eq!(
        private_publish_json["error"]["code"],
        "publication_policy_blocked"
    );

    let init_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init should execute");
    assert!(init_output.status.success());

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(prompt_output.status.success());
    let prompt_json = output_json(&prompt_output);
    assert_eq!(
        prompt_json["data"]["skill_outputs"]["projection_policy"],
        "public_latest_plus_verified_evolution_recommendations"
    );
    assert_eq!(
        prompt_json["data"]["skill_outputs"]["public_latest"]["skill_reports"]["envelope_id"],
        second_envelope_id
    );
    assert_eq!(
        prompt_json["data"]["skill_outputs"]["private_candidate_count"],
        1
    );
    assert!(
        !serde_json::to_string(&prompt_json["data"]["skill_outputs"])
            .unwrap()
            .contains(&private_envelope_id),
        "default prompt context must not expose private candidate envelope ids"
    );
    let prompt_session_id = prompt_json["session_id"].as_str().unwrap().to_string();

    let resume_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["resume", &prompt_session_id, "--json"])
        .output()
        .expect("resume should execute");
    assert!(resume_output.status.success());
    let resume_json = output_json(&resume_output);
    assert_eq!(
        resume_json["data"]["skill_outputs"]["public_latest"]["skill_reports"]["envelope_id"],
        second_envelope_id
    );
    assert!(
        !serde_json::to_string(&resume_json["data"]["skill_outputs"])
            .unwrap()
            .contains(&private_envelope_id),
        "default resume context must not expose private candidate envelope ids"
    );

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &prompt_session_id, "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let compact_json = output_json(&compact_output);
    assert_eq!(
        compact_json["data"]["skill_outputs"]["public_latest"]["skill_reports"]["envelope_id"],
        second_envelope_id
    );
    assert!(compact_json["data"]["derived_views_updated"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "skill_output_context"));
    assert!(
        !serde_json::to_string(&compact_json["data"]["skill_outputs"])
            .unwrap()
            .contains(&private_envelope_id),
        "default compact context must not expose private candidate envelope ids"
    );

    write_text_file(
        workspace_root.join("docs").join("manual-candidate.md"),
        r#"---
doc_frame:
  doc_id: docs.manual_candidate
  schema_version: "1"
  source_path: docs/manual-candidate.md
  title: Manual Candidate
  doc_type: report
  lifecycle: active
  scope: project
  summary: Candidate submitted through docs publish-candidate.
  generated_by: human
  updated_at: 2026-04-27
---

# Manual Candidate
"#,
    );
    let docs_candidate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "docs",
            "publish-candidate",
            "docs/manual-candidate.md",
            "--family",
            "manual_docs",
            "--kind",
            "report",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("docs publish-candidate should execute");
    assert!(
        docs_candidate_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&docs_candidate_output.stderr)
    );
    let docs_candidate_json = output_json(&docs_candidate_output);
    assert_eq!(docs_candidate_json["command"], "docs publish-candidate");
    assert_eq!(
        docs_candidate_json["data"]["envelope"]["skill_id"],
        "docs.publish_candidate"
    );
    assert_eq!(
        docs_candidate_json["data"]["envelope"]["artifact_refs"][0]["artifact_path"],
        "docs/manual-candidate.md"
    );
}

#[test]
fn skills_run_local_command_adapter_records_side_effects_and_submits_envelope() {
    let workspace_root = init_git_workspace("skill_runner_adapter");
    let registry_root = registry_root_env();

    write_skill_manifest(&workspace_root.join(".codex/skills"), "runner_skill");
    write_text_file(
        workspace_root
            .join(".codex/skills")
            .join("runner_skill")
            .join("research_skill_contract.json"),
        &json!({
            "schema_version": "research_skill_contract.v1",
            "conformance_line": "M11.skill_publication",
            "skill_id": "runner_skill",
            "compatible_stage_classes": ["paper"],
            "required_outputs": ["doc_frame", "report"],
            "allowed_output_kinds": ["report"],
            "doc_frame_required": true,
            "canonicality_policy": "requires_review",
            "human_gate_required": true,
            "runner_adapter": "local-command",
            "allowed_write_scopes": ["docs/runner-output.md"]
        })
        .to_string(),
    );

    let run_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            "runner_skill",
            "--adapter",
            "local-command",
            "--command",
            "mkdir -p docs && cat > docs/runner-output.md <<'EOF'\n---\ndoc_frame:\n  doc_id: docs.runner_output\n  schema_version: \"1\"\n  source_path: docs/runner-output.md\n  title: Runner Output\n  doc_type: report\n  lifecycle: active\n  scope: project\n  summary: Candidate generated through the local command adapter.\n  generated_by: skill\n  updated_at: 2026-04-27\n---\n\n# Runner Output\nEOF\n",
            "--kind",
            "report",
            "--artifact",
            "docs/runner-output.md",
            "--family",
            "runner_reports",
            "--doc-frame",
            "docs/runner-output.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("skills run should execute");
    assert!(
        run_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run_output.stderr)
    );
    let run_json = output_json(&run_output);
    assert_eq!(run_json["command"], "skills run");
    assert_eq!(run_json["data"]["runner_adapter"], "local-command");
    assert_eq!(run_json["data"]["execution_status"], "succeeded");
    assert_eq!(run_json["data"]["exit_code"], 0);
    assert_eq!(
        run_json["data"]["submit_result"]["envelope"]["skill_id"],
        "runner_skill"
    );
    assert_eq!(
        run_json["data"]["submit_result"]["envelope"]["artifact_refs"][0]["artifact_path"],
        "docs/runner-output.md"
    );
    assert!(
        run_json["data"]["side_effect_audit"]["mutated_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "docs/runner-output.md"),
        "{}",
        serde_json::to_string_pretty(&run_json["data"]["side_effect_audit"]).unwrap()
    );

    let blocked_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            "runner_skill",
            "--adapter",
            "local-command",
            "--command",
            "mkdir -p docs reports && printf '# allowed' > docs/runner-output.md && printf 'bad' > reports/outside.txt",
            "--kind",
            "report",
            "--artifact",
            "docs/runner-output.md",
            "--family",
            "runner_reports",
            "--doc-frame",
            "docs/runner-output.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("scope-blocked skills run should execute");
    assert!(!blocked_output.status.success());
    let blocked_json = output_json(&blocked_output);
    assert_eq!(blocked_json["error"]["code"], "skill_contract_violation");
    assert!(
        !workspace_root.join("reports").join("outside.txt").exists(),
        "blocked skill runs must not pollute the real workspace before governance rejects them"
    );
}

#[test]
fn skills_run_isolation_does_not_copy_back_traversal_artifacts() {
    let workspace_root = init_git_workspace("skill_runner_traversal");
    let registry_root = registry_root_env();
    let outside_path = workspace_root
        .parent()
        .expect("workspace should have a parent")
        .join("runner_escape.md");
    let _ = std::fs::remove_file(&outside_path);

    write_skill_manifest(&workspace_root.join(".codex/skills"), "loose_runner");

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            "loose_runner",
            "--adapter",
            "local-command",
            "--command",
            "printf 'escaped' > ../runner_escape.md",
            "--kind",
            "report",
            "--artifact",
            "../runner_escape.md",
            "--family",
            "runner_reports",
            "--json",
        ])
        .output()
        .expect("path traversal skills run should execute");
    assert!(!output.status.success());
    let json = output_json(&output);
    assert!(
        matches!(
            json["error"]["code"].as_str(),
            Some("scope_violation") | Some("usage_invalid") | Some("skill_contract_violation")
        ),
        "{}",
        serde_json::to_string_pretty(&json).unwrap()
    );
    assert!(
        !outside_path.exists(),
        "failed skill runs must reject ../ artifacts before copying them outside the workspace"
    );
}

#[test]
fn skills_run_isolation_rejects_declared_artifact_symlink_escape() {
    let workspace_root = init_git_workspace("skill_runner_symlink_escape");
    let registry_root = registry_root_env();
    let outside_path = workspace_root
        .parent()
        .expect("workspace should have a parent")
        .join("symlink_escape.md");
    let _ = std::fs::remove_file(&outside_path);
    write_text_file(&outside_path, "outside before");
    std::fs::create_dir_all(workspace_root.join("docs")).expect("docs dir should exist");
    std::os::unix::fs::symlink(&outside_path, workspace_root.join("docs/runner-output.md"))
        .expect("symlink should be created");

    write_skill_manifest(&workspace_root.join(".codex/skills"), "symlink_runner");

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            "symlink_runner",
            "--adapter",
            "local-command",
            "--command",
            "mkdir -p docs && printf 'governed output' > docs/runner-output.md",
            "--kind",
            "report",
            "--artifact",
            "docs/runner-output.md",
            "--family",
            "runner_reports",
            "--json",
        ])
        .output()
        .expect("symlink escape skills run should execute");
    assert!(!output.status.success());
    let json = output_json(&output);
    assert_eq!(json["error"]["code"], "scope_violation");
    assert_eq!(
        std::fs::read_to_string(&outside_path).expect("outside file should remain readable"),
        "outside before"
    );
}

#[test]
fn skills_run_isolation_does_not_pollute_real_worktree_git_index() {
    let repo_root = unique_temp_dir("skill_runner_git_pointer_repo");
    let main_root = repo_root.join("main");
    let workspace_root = repo_root.join("worktree");
    let init = Command::new("git")
        .args(["init", "-q", main_root.to_str().unwrap()])
        .status()
        .expect("git init should execute");
    assert!(init.success());
    for (key, value) in [
        ("user.name", "Research CLI Test"),
        ("user.email", "research-cli@example.test"),
    ] {
        let config = Command::new("git")
            .current_dir(&main_root)
            .args(["config", key, value])
            .status()
            .expect("git config should execute");
        assert!(config.success());
    }
    write_text_file(main_root.join("README.md"), "# test workspace\n");
    let add = Command::new("git")
        .current_dir(&main_root)
        .args(["add", "README.md"])
        .status()
        .expect("git add should execute");
    assert!(add.success());
    let commit = Command::new("git")
        .current_dir(&main_root)
        .args(["commit", "-q", "-m", "initial"])
        .status()
        .expect("git commit should execute");
    assert!(commit.success());
    let worktree = Command::new("git")
        .current_dir(&main_root)
        .args(["worktree", "add", "-q", workspace_root.to_str().unwrap()])
        .status()
        .expect("git worktree add should execute");
    assert!(worktree.success());
    assert!(
        workspace_root.join(".git").is_file(),
        "test must exercise git worktree pointer metadata"
    );
    let registry_root = registry_root_env();

    write_skill_manifest(&workspace_root.join(".codex/skills"), "git_runner");
    write_text_file(
        workspace_root
            .join(".codex/skills")
            .join("git_runner")
            .join("research_skill_contract.json"),
        &json!({
            "schema_version": "research_skill_contract.v1",
            "conformance_line": "M11.skill_publication",
            "skill_id": "git_runner",
            "compatible_stage_classes": ["paper"],
            "required_outputs": ["doc_frame", "report"],
            "allowed_output_kinds": ["report"],
            "doc_frame_required": true,
            "canonicality_policy": "requires_review",
            "human_gate_required": true,
            "runner_adapter": "local-command",
            "allowed_write_scopes": ["docs/runner-output.md"]
        })
        .to_string(),
    );

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            "git_runner",
            "--adapter",
            "local-command",
            "--command",
            "mkdir -p docs reports && cat > docs/runner-output.md <<'EOF'\n---\ndoc_frame:\n  doc_id: docs.runner_output\n  schema_version: \"1\"\n  source_path: docs/runner-output.md\n  title: Runner Output\n  doc_type: report\n  lifecycle: active\n  scope: project\n  summary: Candidate generated through the local command adapter.\n  generated_by: skill\n  updated_at: 2026-04-27\n---\n\n# Runner Output\nEOF\nprintf 'bad' > reports/outside.txt\ngit add reports/outside.txt\n",
            "--kind",
            "report",
            "--artifact",
            "docs/runner-output.md",
            "--family",
            "runner_reports",
            "--doc-frame",
            "docs/runner-output.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("git-index-polluting skills run should execute");
    assert!(!output.status.success());
    let json = output_json(&output);
    assert_eq!(json["error"]["code"], "skill_contract_violation");

    let status = Command::new("git")
        .current_dir(&workspace_root)
        .args(["status", "--short"])
        .output()
        .expect("git status should execute");
    assert!(status.status.success());
    let status_text = String::from_utf8_lossy(&status.stdout);
    assert!(
        !status_text.contains("reports/outside.txt"),
        "isolated skill git operations must not stage entries in the real worktree index: {status_text}"
    );
}

#[test]
fn skills_submit_and_run_enforce_active_research_stage_compatibility() {
    let workspace_root = init_real_git_workspace("skill_stage_compatibility");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "implementation",
            "--title",
            "implement runtime patch",
            "--stage",
            "implement-solution",
            "--mode",
            "debugging",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );
    let record_json = output_json(&record_output);
    assert_eq!(
        record_json["data"]["stage_execution"]["stage_class"],
        "implement"
    );

    write_skill_manifest(&workspace_root.join(".codex/skills"), "document_only_skill");
    write_text_file(
        workspace_root
            .join(".codex/skills")
            .join("document_only_skill")
            .join("research_skill_contract.json"),
        &json!({
            "schema_version": "research_skill_contract.v1",
            "conformance_line": "M11.skill_publication",
            "skill_id": "document_only_skill",
            "compatible_stage_classes": ["document"],
            "required_outputs": ["doc_frame", "report"],
            "allowed_output_kinds": ["report"],
            "doc_frame_required": true,
            "canonicality_policy": "requires_review",
            "human_gate_required": true,
            "runner_adapter": "local-command",
            "allowed_write_scopes": ["docs/stage-candidate.md", "docs/stage-output.md"]
        })
        .to_string(),
    );
    write_text_file(
        workspace_root.join("docs").join("stage-candidate.md"),
        r#"---
doc_frame:
  doc_id: docs.stage_candidate
  schema_version: "1"
  source_path: docs/stage-candidate.md
  title: Stage Candidate
  doc_type: report
  lifecycle: active
  scope: project
  summary: Candidate for stage compatibility validation.
  generated_by: skill
  updated_at: 2026-04-27
---

# Stage Candidate
"#,
    );

    let submit_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "output",
            "submit",
            "--skill",
            "document_only_skill",
            "--kind",
            "report",
            "--artifact",
            "docs/stage-candidate.md",
            "--family",
            "stage_reports",
            "--doc-frame",
            "docs/stage-candidate.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("stage-blocked skills output submit should execute");
    assert!(!submit_output.status.success());
    let submit_json = output_json(&submit_output);
    assert_eq!(submit_json["error"]["code"], "skill_contract_violation");
    assert!(submit_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("active research stage class"));

    let run_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "skills",
            "run",
            "--skill",
            "document_only_skill",
            "--adapter",
            "local-command",
            "--command",
            "mkdir -p docs && printf 'should not run' > docs/stage-output.md",
            "--kind",
            "report",
            "--artifact",
            "docs/stage-output.md",
            "--family",
            "stage_reports",
            "--doc-frame",
            "docs/stage-output.md",
            "--policy",
            "requires_review",
            "--human-gate",
            "--json",
        ])
        .output()
        .expect("stage-blocked skills run should execute");
    assert!(!run_output.status.success());
    let run_json = output_json(&run_output);
    assert_eq!(run_json["error"]["code"], "skill_contract_violation");
    assert!(
        !workspace_root.join("docs").join("stage-output.md").exists(),
        "stage-incompatible skills run must be rejected before the adapter executes"
    );
}

#[test]
fn mcp_refresh_and_inspect_emit_registry_events() {
    let workspace_root = init_git_workspace("mcp_refresh");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    write_mcp_manifest(&workspace_root.join(".pmcli/mcp/servers"), "refreshable");

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["mcp", "inspect", "refreshable", "--json"])
        .output()
        .expect("mcp inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["command"], "mcp inspect");
    assert_eq!(inspect_json["data"]["server_id"], "refreshable");
    assert_eq!(inspect_json["data"]["transport"], "stdio");

    let refresh_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["mcp", "refresh", "--json"])
        .output()
        .expect("mcp refresh should execute");
    assert!(refresh_output.status.success());
    let refresh_json = output_json(&refresh_output);
    assert_eq!(refresh_json["command"], "mcp refresh");
    assert_eq!(refresh_json["data"]["total_count"], 1);
    assert!(refresh_json["data"]["refreshed_at"].is_string());

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "mcp_registry" && event["object_id"] == "mcp refresh"));
}

#[test]
fn reviews_open_list_and_inspect_are_project_scoped_and_traceable() {
    let workspace_root = init_git_workspace("reviews_runtime");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "review target\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let open_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Audit the README target",
            "--target",
            "README.md",
            "--reviewer-role",
            "external_auditor",
            "--review-model",
            "gpt-5.4",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(
        open_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&open_output.stderr)
    );
    let open_json = output_json(&open_output);
    assert_eq!(open_json["command"], "reviews open");
    assert_eq!(
        open_json["data"]["packet"]["objective"],
        "Audit the README target"
    );
    assert_eq!(
        open_json["data"]["packet"]["reviewer_role"],
        "external_auditor"
    );
    assert_eq!(open_json["data"]["packet"]["review_model"], "gpt-5.4");
    assert_eq!(open_json["data"]["trace"]["verdict"], "pending");
    assert_eq!(open_json["data"]["status"], "open");
    let review_id = open_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("review id should be present")
        .to_string();

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "list", "--json"])
        .output()
        .expect("reviews list should execute");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_json = output_json(&list_output);
    assert_eq!(list_json["command"], "reviews list");
    assert_eq!(list_json["data"]["total_count"], 1);
    assert!(list_json["data"]["reviews"]
        .as_array()
        .expect("reviews array")
        .iter()
        .any(|entry| entry["review_id"] == review_id && entry["status"] == "open"));

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "inspect", review_id.as_str(), "--json"])
        .output()
        .expect("reviews inspect should execute");
    assert!(
        inspect_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["command"], "reviews inspect");
    assert_eq!(inspect_json["data"]["packet"]["review_id"], review_id);
    assert_eq!(inspect_json["data"]["latest_trace"]["verdict"], "pending");
    assert!(inspect_json["data"]["history_paths"].is_array());

    let artifact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "inspect", "README.md", "--json"])
        .output()
        .expect("artifacts inspect should execute");
    assert!(artifact_output.status.success());
    let artifact_json = output_json(&artifact_output);
    assert!(artifact_json["data"]["review_links"]
        .as_array()
        .expect("review links array")
        .iter()
        .any(|entry| entry == &Value::String(review_id.clone())));

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "review" && event["object_id"] == review_id));
}

#[test]
fn reviews_resolve_updates_review_trace() {
    let workspace_root = init_git_workspace("reviews_resolve_trace");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "review target\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let open_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Audit the README target",
            "--target",
            "README.md",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(
        open_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&open_output.stderr)
    );
    let open_json = output_json(&open_output);
    let review_id = open_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("review id")
        .to_string();

    let resolve_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "resolve",
            &review_id,
            "--verdict",
            "approved",
            "--response",
            "approved with evidence",
            "--json",
        ])
        .output()
        .expect("reviews resolve should execute");
    assert!(
        resolve_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resolve_output.stderr)
    );
    let resolve_json = output_json(&resolve_output);
    assert_eq!(resolve_json["command"], "reviews resolve");
    assert_eq!(resolve_json["data"]["packet"]["review_id"], review_id);
    assert_eq!(resolve_json["data"]["status"], "resolved");
    assert_eq!(resolve_json["data"]["trace"]["verdict"], "approved");
    assert_eq!(
        resolve_json["data"]["trace"]["response_text"],
        "approved with evidence"
    );

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "inspect", &review_id, "--json"])
        .output()
        .expect("reviews inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["data"]["status"], "resolved");
    assert_eq!(inspect_json["data"]["latest_trace"]["verdict"], "approved");
    assert_eq!(
        inspect_json["data"]["history_paths"]
            .as_array()
            .expect("history paths")
            .len(),
        2
    );

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "list", "--status", "resolved", "--json"])
        .output()
        .expect("reviews list should execute");
    assert!(list_output.status.success());
    let list_json = output_json(&list_output);
    assert_eq!(list_json["data"]["total_count"], 1);
    assert_eq!(list_json["data"]["reviews"][0]["review_id"], review_id);

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path should be utf8"),
    );
    assert!(events.iter().any(|event| {
        event["event_name"] == "review"
            && event["object_id"] == review_id
            && event["payload"]["status"] == "resolved"
    }));
}

#[test]
fn prompt_and_review_emit_goal_alignment_traces() {
    let workspace_root = init_git_workspace("goal_alignment_all_lanes");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "review target\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Deliver governed research runtime orchestration.",
            "--milestone-goal",
            "Preserve non-linear stage alignment.",
            "--current-implementation-goal",
            "Detect irrelevant command context without compact-specific wording.",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(set_output.status.success());

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["prompt", "coffee", "packing", "weather", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(prompt_output.status.success());

    let review_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "review the implementation",
            "--target",
            "README.md",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(review_output.status.success());

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path should be utf8"),
    );
    let prompt_trace = events
        .iter()
        .rev()
        .find(|event| {
            event["event_name"] == "goal_alignment_trace" && event["payload"]["command"] == "prompt"
        })
        .expect("prompt goal alignment trace should exist");
    assert_eq!(
        prompt_trace["payload"]["recommended_next_action"],
        "review_or_continue_with_mission_frame_priority"
    );
    let prompt_reasons = prompt_trace["payload"]["drift_reasons"]
        .as_array()
        .expect("prompt drift reasons should be an array")
        .iter()
        .map(|value| value.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(
        prompt_reasons
            .iter()
            .any(|reason| reason.contains("prompt text")),
        "prompt drift reasons should cite prompt text: {prompt_reasons:?}"
    );
    assert!(
        prompt_reasons
            .iter()
            .all(|reason| !reason.contains("compact")),
        "prompt drift reasons must not use compact-specific wording: {prompt_reasons:?}"
    );

    let review_trace = events
        .iter()
        .rev()
        .find(|event| {
            event["event_name"] == "goal_alignment_trace"
                && event["payload"]["command"] == "reviews open"
        })
        .expect("review goal alignment trace should exist");
    assert_eq!(
        review_trace["payload"]["alignment_method"],
        "deterministic_command_context_token_overlap_v1"
    );
    let review_reasons = review_trace["payload"]["drift_reasons"]
        .as_array()
        .expect("review drift reasons should be an array")
        .iter()
        .map(|value| value.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(
        review_reasons
            .iter()
            .any(|reason| reason.contains("review packet")),
        "review drift reasons should cite review packet evidence: {review_reasons:?}"
    );
    assert!(
        review_reasons
            .iter()
            .all(|reason| !reason.contains("compact")),
        "review drift reasons must not use compact-specific wording: {review_reasons:?}"
    );
}

#[test]
fn reviews_open_redacts_executor_interpretation_from_default_blinded_packet() {
    let workspace_root = init_git_workspace("reviews_blinding");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "primary evidence\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let leaked_interpretation = "EXECUTOR_SECRET_INTERPRETATION this implementation is perfect";
    let open_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Audit the primary evidence independently",
            "--target",
            "README.md",
            "--context",
            "Primary evidence is README.md",
            "--executor-summary",
            leaked_interpretation,
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(
        open_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&open_output.stderr)
    );
    let open_json = output_json(&open_output);
    let serialized = serde_json::to_string(&open_json["data"]["packet"])
        .expect("packet should serialize for leakage check");
    let prompt_snapshot = open_json["data"]["trace"]["prompt_snapshot"]
        .as_str()
        .expect("prompt snapshot should exist");

    assert_eq!(open_json["data"]["packet"]["blinded"], true);
    assert_eq!(
        open_json["data"]["packet"]["blinding_policy"],
        "default_reviewer_blinding"
    );
    assert_eq!(
        open_json["data"]["packet"]["redaction"]["redacted_count"],
        1
    );
    assert_eq!(
        open_json["data"]["packet"]["redaction"]["redacted_sources"][0]["source_kind"],
        "executor_summary"
    );
    assert_eq!(
        open_json["data"]["packet"]["blind_context"][0],
        "Primary evidence is README.md"
    );
    assert!(!serialized.contains("EXECUTOR_SECRET_INTERPRETATION"));
    assert!(!prompt_snapshot.contains("EXECUTOR_SECRET_INTERPRETATION"));
}

#[test]
fn reviews_inspect_backfills_blinding_fields_for_legacy_packets() {
    let workspace_root = init_git_workspace("reviews_legacy_blinding");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "legacy review target\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let open_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Audit a legacy packet",
            "--target",
            "README.md",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(
        open_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&open_output.stderr)
    );
    let open_json = output_json(&open_output);
    let review_id = open_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("review id should be present");
    let packet_path = open_json["data"]["packet_path"]
        .as_str()
        .expect("packet path should be present");
    let mut legacy_packet: Value =
        serde_json::from_str(&std::fs::read_to_string(packet_path).expect("packet should exist"))
            .expect("packet should parse");
    let packet_obj = legacy_packet
        .as_object_mut()
        .expect("packet should be a JSON object");
    packet_obj.remove("blinded");
    packet_obj.remove("blinding_policy");
    packet_obj.remove("redaction");
    std::fs::write(
        packet_path,
        serde_json::to_string_pretty(&legacy_packet).expect("legacy packet should serialize"),
    )
    .expect("legacy packet should be writable");

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "inspect", review_id, "--json"])
        .output()
        .expect("reviews inspect should execute");
    assert!(
        inspect_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["data"]["packet"]["blinded"], true);
    assert_eq!(
        inspect_json["data"]["packet"]["blinding_policy"],
        "default_reviewer_blinding"
    );
    assert_eq!(
        inspect_json["data"]["packet"]["redaction"]["redacted_count"],
        0
    );
}

#[test]
fn reviews_retry_preserves_compare_linkage_and_blocks_unresolved_reviews() {
    let workspace_root = init_git_workspace("reviews_retry_compare");
    let registry_root = registry_root_env();
    write_text_file(workspace_root.join("README.md"), "review retry target\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let base_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Base review",
            "--target",
            "README.md",
            "--verdict",
            "failed",
            "--response",
            "base finding",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(base_output.status.success());
    let base_json = output_json(&base_output);
    let base_review_id = base_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("base review id")
        .to_string();

    let compare_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Compare retry target",
            "--target",
            "README.md",
            "--compare-against",
            &base_review_id,
            "--verdict",
            "timeout",
            "--response",
            "review worker timed out",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(compare_output.status.success());
    let compare_json = output_json(&compare_output);
    let compare_review_id = compare_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("compare review id")
        .to_string();
    assert_eq!(
        compare_json["data"]["packet"]["compare_against"],
        base_review_id
    );

    let retry_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "retry", &compare_review_id, "--json"])
        .output()
        .expect("reviews retry should execute");
    assert!(
        retry_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&retry_output.stderr)
    );
    let retry_json = output_json(&retry_output);
    assert_eq!(retry_json["command"], "reviews retry");
    assert_eq!(retry_json["data"]["status"], "open");
    assert_ne!(retry_json["data"]["packet"]["review_id"], compare_review_id);
    assert_eq!(retry_json["data"]["packet"]["retry_of"], compare_review_id);
    assert_eq!(retry_json["data"]["packet"]["retry_attempt"], 1);
    assert_eq!(
        retry_json["data"]["packet"]["compare_against"],
        base_review_id
    );
    assert_eq!(retry_json["data"]["packet"]["fresh_thread"], true);
    assert_eq!(retry_json["data"]["packet"]["blinded"], true);
    assert!(retry_json["data"]["trace"]["thread_id"]
        .as_str()
        .expect("thread id")
        .starts_with("fresh::"));
    assert!(retry_json["data"]["trace"]["prompt_snapshot"]
        .as_str()
        .expect("prompt snapshot")
        .contains("compare_against"));

    let pending_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "reviews",
            "open",
            "--objective",
            "Pending review",
            "--target",
            "README.md",
            "--json",
        ])
        .output()
        .expect("reviews open should execute");
    assert!(pending_output.status.success());
    let pending_json = output_json(&pending_output);
    let pending_review_id = pending_json["data"]["packet"]["review_id"]
        .as_str()
        .expect("pending review id");
    let rejected_retry = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["reviews", "retry", pending_review_id, "--json"])
        .output()
        .expect("reviews retry should execute");
    assert_eq!(rejected_retry.status.code(), Some(9));
    let rejected_json = output_json(&rejected_retry);
    assert_eq!(rejected_json["error"]["code"], "review_retry_not_allowed");
    assert_eq!(
        rejected_json["data"]["failure_code"],
        "review_retry_not_allowed"
    );
}

#[test]
fn artifacts_and_repo_cleanup_surfaces_are_traceable_and_project_scoped() {
    let workspace_root = init_git_workspace("artifacts_cleanup");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "sessions",
            "create",
            "--title",
            "cleanup candidate",
            "--json",
        ])
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

    let stale_path = workspace_root.join(".pmcli/archive/stale.tmp");
    write_text_file(&stale_path, "stale");

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "list", "--include-archive", "--json"])
        .output()
        .expect("artifacts list should execute");
    assert!(list_output.status.success());
    let list_json = output_json(&list_output);
    assert_eq!(list_json["command"], "artifacts list");
    assert!(list_json["data"]["families"]
        .as_array()
        .expect("families array")
        .iter()
        .any(|entry| entry["family"] == "sessions"));
    assert!(list_json["data"]["families"]
        .as_array()
        .expect("families array")
        .iter()
        .any(|entry| entry["family"] == "archive"
            && entry["manifest_path"].is_string()
            && entry["canonical_id"].is_string()
            && entry["latest_id"].is_string()
            && entry["archive_ids"].is_array()));

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "inspect", "sessions", "--json"])
        .output()
        .expect("artifacts inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["command"], "artifacts inspect");
    assert_eq!(inspect_json["data"]["target"], "sessions");
    assert!(inspect_json["data"]["canonical_path"].is_string());
    assert!(inspect_json["data"]["lineage"].is_array());
    assert!(inspect_json["data"]["manifest"]["canonical_id"].is_string());
    assert!(inspect_json["data"]["promotion_history"]
        .as_array()
        .expect("promotion history array")
        .iter()
        .any(|entry| entry["event"] == "artifact_family_manifested"));

    let cleanup_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--json"])
        .output()
        .expect("repo cleanup-plan should execute");
    assert!(cleanup_plan_output.status.success());
    let cleanup_plan_json = output_json(&cleanup_plan_output);
    assert_eq!(cleanup_plan_json["command"], "repo cleanup-plan");
    assert_eq!(cleanup_plan_json["data"]["dry_run"], true);
    assert!(cleanup_plan_json["data"]["plan_id"].is_string());
    assert!(cleanup_plan_json["data"]["rollback_snapshot_ref"].is_string());
    assert!(cleanup_plan_json["data"]["proposed_actions"]
        .as_array()
        .expect("proposed actions array")
        .iter()
        .any(|entry| entry["target_path"].as_str() == stale_path.to_str()));

    let plan_id = cleanup_plan_json["data"]["plan_id"]
        .as_str()
        .expect("plan id");
    let cleanup_apply_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-apply", plan_id, "--json"])
        .output()
        .expect("repo cleanup-apply should execute");
    assert!(cleanup_apply_output.status.success());
    let cleanup_apply_json = output_json(&cleanup_apply_output);
    assert_eq!(cleanup_apply_json["command"], "repo cleanup-apply");
    assert_eq!(cleanup_apply_json["data"]["dry_run"], false);
    assert!(cleanup_apply_json["data"]["review_gates"]
        .as_array()
        .expect("review gates array")
        .iter()
        .any(|entry| entry["gate_id"] == "rollback_snapshot_created"
            && entry["status"] == "satisfied"));
    assert!(
        !stale_path.exists(),
        "cleanup apply should remove stale target"
    );

    let cleanup_restore_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-restore", plan_id, "--json"])
        .output()
        .expect("repo cleanup-restore should execute");
    assert!(
        cleanup_restore_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cleanup_restore_output.stderr)
    );
    let cleanup_restore_json = output_json(&cleanup_restore_output);
    assert_eq!(cleanup_restore_json["command"], "repo cleanup-restore");
    assert_eq!(cleanup_restore_json["data"]["plan_id"], plan_id);
    assert!(cleanup_restore_json["data"]["restored_actions"]
        .as_array()
        .expect("restored actions array")
        .iter()
        .any(|entry| entry["target_path"].as_str() == stale_path.to_str()));
    assert_eq!(
        std::fs::read_to_string(&stale_path).expect("stale target should be restored"),
        "stale"
    );
}

#[test]
fn repo_cleanup_restore_invalidates_memory_referencing_restored_artifacts() {
    let workspace_root = init_git_workspace("cleanup_restore_memory");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let stale_path = workspace_root.join(".pmcli/archive/memory-bound.tmp");
    write_text_file(&stale_path, "stale");

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "cleanup memory", "--json"])
        .output()
        .expect("sessions create should execute");
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
    let mut transcript = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    writeln!(
        transcript,
        "{}",
        serde_json::json!({
            "line_type": "message",
            "role": "assistant",
            "content": "cleanup restore should invalidate memory-bound artifact"
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
    let candidate_path = workspace_root
        .join(".pmcli")
        .join("memory")
        .join("promotion_queue")
        .join(format!("{candidate_id}.json"));
    let mut candidate_json: Value = serde_json::from_str(
        &std::fs::read_to_string(&candidate_path).expect("candidate should exist"),
    )
    .expect("candidate should parse");
    candidate_json["support_refs"]
        .as_array_mut()
        .expect("support refs should be an array")
        .push(Value::String(
            stale_path
                .to_str()
                .expect("stale path should stringify")
                .to_string(),
        ));
    std::fs::write(
        &candidate_path,
        serde_json::to_string_pretty(&candidate_json).expect("candidate should serialize"),
    )
    .expect("candidate should update");

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

    let cleanup_plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--json"])
        .output()
        .expect("repo cleanup-plan should execute");
    assert!(cleanup_plan_output.status.success());
    let plan_id = output_json(&cleanup_plan_output)["data"]["plan_id"]
        .as_str()
        .expect("plan id should exist")
        .to_string();

    let cleanup_apply_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-apply", &plan_id, "--json"])
        .output()
        .expect("repo cleanup-apply should execute");
    assert!(cleanup_apply_output.status.success());

    let restore_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-restore", &plan_id, "--json"])
        .output()
        .expect("repo cleanup-restore should execute");
    assert!(restore_output.status.success());

    let explain_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["memory", "explain", "--record-id", &record_id, "--json"])
        .output()
        .expect("memory explain should execute");
    assert!(explain_output.status.success());
    let explain_json = output_json(&explain_output);
    assert_eq!(explain_json["data"]["status"], "invalidated");
    assert_eq!(
        explain_json["data"]["injection_decision"],
        "explain_visible_only"
    );
    assert!(explain_json["data"]["invalidation_reason"]
        .as_str()
        .expect("invalidation reason should exist")
        .contains("cleanup restore"));
}

#[test]
fn projectops_supervisor_lease_and_wake_events_are_persisted_and_listed() {
    let workspace_root = init_git_workspace("projectops_supervision");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let lease_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "lease",
            "acquire",
            "--run-id",
            "run_eval_1",
            "--owner-agent",
            "agent_reviewer",
            "--stale-after-sec",
            "60",
            "--json",
        ])
        .output()
        .expect("projectops lease acquire should execute");
    assert!(
        lease_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&lease_output.stderr)
    );
    let lease_json = output_json(&lease_output);
    assert_eq!(lease_json["command"], "projectops lease acquire");
    assert_eq!(lease_json["data"]["state"], "active");
    let lease_id = lease_json["data"]["lease_id"]
        .as_str()
        .expect("lease id should exist")
        .to_string();

    let wake_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "emit",
            "--run-id",
            "run_eval_1",
            "--lease-id",
            &lease_id,
            "--owner-agent",
            "agent_reviewer",
            "--kind",
            "blocked",
            "--urgency",
            "high",
            "--summary",
            "evaluation needs operator decision",
            "--requires-main-system",
            "--json",
        ])
        .output()
        .expect("projectops wake emit should execute");
    assert!(wake_output.status.success());
    let wake_json = output_json(&wake_output);
    assert_eq!(wake_json["command"], "projectops wake emit");
    assert_eq!(wake_json["data"]["lease_id"], lease_id);
    assert_eq!(wake_json["data"]["state"], "pending");
    assert_eq!(wake_json["data"]["requires_main_system"], true);

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(status_json["command"], "projectops status");
    assert_eq!(status_json["data"]["active_lease_count"], 1);
    assert_eq!(status_json["data"]["pending_wake_count"], 1);
    assert!(status_json["data"]["leases"]
        .as_array()
        .expect("leases should exist")
        .iter()
        .any(|lease| lease["lease_id"] == lease_id));

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path should be utf8"),
    );
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "projectops_lease"));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "projectops_wake"));
}

#[test]
fn routines_create_trigger_and_project_background_agent() {
    let workspace_root = init_real_git_workspace("routines_background_agent");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "create",
            "--name",
            "Webhook review",
            "--trigger-kind",
            "webhook",
            "--intent",
            "review",
            "--role-profile",
            "reviewer",
            "--message",
            "review routine payload",
            "--command",
            "printf routine-output > routine.txt",
            "--delivery-target",
            "research-board",
            "--json",
        ])
        .output()
        .expect("routines create should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let create_json = output_json(&create_output);
    assert_eq!(create_json["command"], "routines create");
    assert_eq!(create_json["data"]["status"], "created");
    assert_eq!(create_json["data"]["routine"]["trigger_kind"], "webhook");
    let routine_id = create_json["data"]["routine"]["routine_id"]
        .as_str()
        .expect("routine id should exist")
        .to_string();
    let definition_path = create_json["data"]["definition_path"]
        .as_str()
        .expect("definition path should exist");
    assert!(Path::new(definition_path).exists());

    let trigger_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "trigger",
            &routine_id,
            "--trigger-kind",
            "webhook",
            "--source",
            "demo-webhook",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("routines trigger should execute");
    assert!(
        trigger_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&trigger_output.stderr)
    );
    let trigger_json = output_json(&trigger_output);
    assert_eq!(trigger_json["command"], "routines trigger");
    assert_eq!(trigger_json["data"]["status"], "triggered");
    assert_eq!(trigger_json["data"]["routine"]["routine_id"], routine_id);
    assert_eq!(trigger_json["data"]["trigger"]["trigger_kind"], "webhook");
    assert_eq!(trigger_json["data"]["trigger"]["source"], "demo-webhook");
    assert_eq!(trigger_json["data"]["lease"]["state"], "active");
    let agent_id = trigger_json["data"]["agent"]["agent_id"]
        .as_str()
        .expect("agent id should exist")
        .to_string();
    let lease_id = trigger_json["data"]["lease"]["lease_id"]
        .as_str()
        .expect("lease id should exist")
        .to_string();
    let run_id = trigger_json["data"]["trigger"]["run_id"]
        .as_str()
        .expect("run id should exist")
        .to_string();
    assert!(run_id.contains(&routine_id));

    let inspect_agent_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["agents", "inspect", &agent_id, "--json"])
        .output()
        .expect("agents inspect should execute");
    assert!(
        inspect_agent_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspect_agent_output.stderr)
    );
    let inspect_agent_json = output_json(&inspect_agent_output);
    assert_eq!(inspect_agent_json["data"]["agent_id"], agent_id);
    assert_eq!(
        inspect_agent_json["data"]["output_manifest"]["validation_status"],
        "valid"
    );
    assert_eq!(
        inspect_agent_json["data"]["runtime_record"]["lifecycle_status"],
        "succeeded"
    );

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert!(status_json["data"]["leases"]
        .as_array()
        .expect("leases should exist")
        .iter()
        .any(|lease| lease["lease_id"] == lease_id
            && lease["run_id"] == run_id
            && lease["owner_agent_id"] == agent_id));

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "list", "--json"])
        .output()
        .expect("routines list should execute");
    assert!(list_output.status.success());
    let list_json = output_json(&list_output);
    assert_eq!(list_json["data"]["total_count"], 1);
    assert_eq!(list_json["data"]["routines"][0]["routine_id"], routine_id);
    assert_eq!(list_json["data"]["routines"][0]["trigger_count"], 1);
    assert_eq!(
        list_json["data"]["routines"][0]["last_trigger"]["agent_id"],
        agent_id
    );

    let inspect_routine_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "inspect", &routine_id, "--json"])
        .output()
        .expect("routines inspect should execute");
    assert!(inspect_routine_output.status.success());
    let inspect_routine_json = output_json(&inspect_routine_output);
    assert_eq!(
        inspect_routine_json["data"]["routine"]["routine_id"],
        routine_id
    );
    assert_eq!(inspect_routine_json["data"]["trigger_count"], 1);
    assert_eq!(
        inspect_routine_json["data"]["triggers"][0]["lease_id"],
        lease_id
    );

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path should be utf8"),
    );
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "routine_definition"));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "routine_trigger"));
}

#[test]
fn routines_ingress_run_due_and_dedupe_trigger() {
    let workspace_root = init_real_git_workspace("routines_ingress_run_due");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "create",
            "--name",
            "Webhook ingress",
            "--trigger-kind",
            "webhook",
            "--intent",
            "review ingress",
            "--role-profile",
            "reviewer",
            "--message",
            "review ingress payload",
            "--command",
            "printf ingress-output > ingress.txt",
            "--delivery-target",
            "research-board",
            "--json",
        ])
        .output()
        .expect("routines create should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let create_json = output_json(&create_output);
    let routine_id = create_json["data"]["routine"]["routine_id"]
        .as_str()
        .expect("routine id should exist")
        .to_string();

    let invalid_ready_at_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "webhook",
            "--source",
            "invalid-ready-at",
            "--ready-at",
            "not-a-number",
            "--json",
        ])
        .output()
        .expect("invalid ready_at routines ingress should execute");
    assert!(
        !invalid_ready_at_output.status.success(),
        "invalid ready_at must be rejected"
    );
    let invalid_ready_at_json = output_json(&invalid_ready_at_output);
    assert_eq!(
        invalid_ready_at_json["error"]["message"],
        "ready_at must be a unix timestamp in milliseconds"
    );

    let future_ingress_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "webhook",
            "--source",
            "future-schedule",
            "--dedupe-key",
            "future-delivery",
            "--ready-at",
            "9999999999999",
            "--json",
        ])
        .output()
        .expect("future routines ingress should execute");
    assert!(
        future_ingress_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&future_ingress_output.stderr)
    );
    let future_run_due_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "run-due", "--trigger-kind", "webhook", "--json"])
        .output()
        .expect("future routines run-due should execute");
    assert!(future_run_due_output.status.success());
    let future_run_due_json = output_json(&future_run_due_output);
    assert_eq!(future_run_due_json["data"]["selected_count"], 0);

    let ingress_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "webhook",
            "--source",
            "demo-webhook",
            "--dedupe-key",
            "delivery-1",
            "--json",
        ])
        .output()
        .expect("routines ingress should execute");
    assert!(
        ingress_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingress_output.stderr)
    );
    let ingress_json = output_json(&ingress_output);
    assert_eq!(ingress_json["command"], "routines ingress");
    assert_eq!(ingress_json["data"]["status"], "ready");
    assert_eq!(ingress_json["data"]["ingress"]["status"], "ready");
    assert_eq!(ingress_json["data"]["ingress"]["dedupe_key"], "delivery-1");
    let ingress_id = ingress_json["data"]["ingress"]["ingress_id"]
        .as_str()
        .expect("ingress id should exist")
        .to_string();

    let duplicate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "ingress",
            &routine_id,
            "--trigger-kind",
            "webhook",
            "--source",
            "demo-webhook-redelivery",
            "--dedupe-key",
            "delivery-1",
            "--json",
        ])
        .output()
        .expect("duplicate routines ingress should execute");
    assert!(
        duplicate_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&duplicate_output.stderr)
    );
    let duplicate_json = output_json(&duplicate_output);
    assert_eq!(duplicate_json["data"]["status"], "duplicate_ignored");
    assert_eq!(
        duplicate_json["data"]["ingress"]["duplicate_of_ingress_id"],
        ingress_id
    );

    let run_due_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "run-due",
            "--trigger-kind",
            "webhook",
            "--stale-after-sec",
            "120",
            "--json",
        ])
        .output()
        .expect("routines run-due should execute");
    assert!(
        run_due_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run_due_output.stderr)
    );
    let run_due_json = output_json(&run_due_output);
    assert_eq!(run_due_json["command"], "routines run-due");
    assert_eq!(run_due_json["data"]["selected_count"], 1);
    assert_eq!(run_due_json["data"]["triggered_count"], 1);
    let outcome = &run_due_json["data"]["outcomes"][0];
    assert_eq!(outcome["status"], "triggered");
    assert_eq!(outcome["ingress"]["ingress_id"], ingress_id);
    assert_eq!(outcome["ingress"]["status"], "triggered");
    assert_eq!(
        outcome["trigger_result"]["trigger"]["ingress_id"],
        ingress_id
    );
    assert_eq!(
        outcome["trigger_result"]["trigger"]["trigger_kind"],
        "webhook"
    );
    assert_eq!(outcome["trigger_result"]["lease"]["state"], "active");

    let list_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "list", "--json"])
        .output()
        .expect("routines list should execute");
    assert!(list_output.status.success());
    let list_json = output_json(&list_output);
    assert_eq!(list_json["data"]["routines"][0]["trigger_count"], 1);
    assert_eq!(list_json["data"]["routines"][0]["ingress_count"], 3);
    assert!(
        list_json["data"]["routines"][0]["last_ingress"]["status"] == "triggered"
            || list_json["data"]["routines"][0]["last_ingress"]["status"] == "duplicate_ignored"
    );

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "inspect", &routine_id, "--json"])
        .output()
        .expect("routines inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["data"]["trigger_count"], 1);
    assert_eq!(inspect_json["data"]["ingress_count"], 3);
    assert!(inspect_json["data"]["ingress"]
        .as_array()
        .expect("ingress should be an array")
        .iter()
        .any(|record| record["ingress_id"] == ingress_id && record["status"] == "triggered"));

    let rerun_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "run-due", "--trigger-kind", "webhook", "--json"])
        .output()
        .expect("second routines run-due should execute");
    assert!(rerun_output.status.success());
    let rerun_json = output_json(&rerun_output);
    assert_eq!(rerun_json["data"]["selected_count"], 0);
    assert_eq!(rerun_json["data"]["triggered_count"], 0);

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path should be utf8"),
    );
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "routine_ingress"));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "routine_run_due"));
}

#[test]
fn routines_retry_failed_trigger_records_recovery() {
    let workspace_root = init_real_git_workspace("routines_retry_failed_trigger");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "create",
            "--name",
            "Failing routine",
            "--trigger-kind",
            "manual",
            "--intent",
            "exercise retry",
            "--role-profile",
            "tester",
            "--message",
            "fail once",
            "--command",
            "printf retry-failure >&2; exit 7",
            "--json",
        ])
        .output()
        .expect("routines create should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let create_json = output_json(&create_output);
    let routine_id = create_json["data"]["routine"]["routine_id"]
        .as_str()
        .expect("routine id should exist")
        .to_string();

    let trigger_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "trigger",
            &routine_id,
            "--trigger-kind",
            "manual",
            "--source",
            "retry-test",
            "--json",
        ])
        .output()
        .expect("routines trigger should execute");
    assert!(
        trigger_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&trigger_output.stderr)
    );
    let trigger_json = output_json(&trigger_output);
    let failed_trigger_id = trigger_json["data"]["trigger"]["trigger_id"]
        .as_str()
        .expect("trigger id should exist")
        .to_string();
    assert_eq!(
        trigger_json["data"]["trigger"]["status"],
        "agent_run_failed"
    );
    assert_eq!(trigger_json["data"]["trigger"]["attempt"], 1);
    assert_eq!(
        trigger_json["data"]["trigger"]["recovery_decision"],
        "retry_available_after_backoff"
    );
    assert_eq!(trigger_json["data"]["trigger"]["retry_budget_remaining"], 2);
    assert!(trigger_json["data"]["trigger"]["next_retry_at"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(
        trigger_json["data"]["recovery_wake"]["kind"],
        "routine_agent_failed"
    );

    let retry_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "routines",
            "retry",
            &failed_trigger_id,
            "--source",
            "operator-retry",
            "--json",
        ])
        .output()
        .expect("routines retry should execute");
    assert!(!retry_output.status.success());
    let retry_json = output_json(&retry_output);
    assert_eq!(retry_json["command"], "routines retry");
    assert_eq!(retry_json["ok"], false);
    assert_eq!(retry_json["error"]["code"], "usage_invalid");
    assert!(retry_json["error"]["message"]
        .as_str()
        .expect("error message")
        .contains("retry backoff"));

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["routines", "inspect", &routine_id, "--json"])
        .output()
        .expect("routines inspect should execute");
    assert!(inspect_output.status.success());
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["data"]["trigger_count"], 1);
    assert!(inspect_json["data"]["triggers"]
        .as_array()
        .expect("triggers should be an array")
        .iter()
        .any(|trigger| trigger["trigger_id"] == failed_trigger_id
            && trigger["attempt"] == 1
            && trigger["recovery_decision"] == "retry_available_after_backoff"));

    let projectops_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status should execute");
    assert!(projectops_output.status.success());
    let projectops_json = output_json(&projectops_output);
    assert!(projectops_json["data"]["wake_events"]
        .as_array()
        .expect("wake events should exist")
        .iter()
        .any(|wake| wake["kind"] == "routine_agent_failed"));

    let surface_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["host", "surface", "status", "--json"])
        .output()
        .expect("host surface status should execute");
    assert!(surface_output.status.success());
    let surface_json = output_json(&surface_output);
    assert_eq!(
        surface_json["data"]["projection"]["research"]["recovery_governance_status"],
        "backoff_active"
    );
    assert_eq!(
        surface_json["data"]["projection"]["research"]["open_recovery_wake_count"],
        1
    );

    let events = read_json_lines(
        workspace_root
            .join(".pmcli/events/events.jsonl")
            .to_str()
            .expect("events path should be utf8"),
    );
    assert!(!events
        .iter()
        .any(|event| event["event_name"] == "routine_retry"));
}

#[test]
fn projectops_enforces_supervision_state_machine_transitions() {
    let workspace_root = init_git_workspace("projectops_state_machine");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let lease_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "lease",
            "acquire",
            "--run-id",
            "run_state_machine",
            "--owner-agent",
            "agent_alpha",
            "--stale-after-sec",
            "60",
            "--json",
        ])
        .output()
        .expect("lease acquire should execute");
    assert!(lease_output.status.success());
    let lease_json = output_json(&lease_output);
    let lease_id = lease_json["data"]["lease_id"]
        .as_str()
        .expect("lease id should exist")
        .to_string();

    let duplicate_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "lease",
            "acquire",
            "--run-id",
            "run_state_machine",
            "--owner-agent",
            "agent_beta",
            "--json",
        ])
        .output()
        .expect("duplicate lease acquire should execute");
    assert!(
        !duplicate_output.status.success(),
        "duplicate live run ownership must be rejected"
    );

    let missing_lease_wake = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "emit",
            "--run-id",
            "run_state_machine",
            "--lease-id",
            "lease_missing",
            "--owner-agent",
            "agent_alpha",
            "--json",
        ])
        .output()
        .expect("wake emit should execute");
    assert!(
        !missing_lease_wake.status.success(),
        "wake events must not silently reference a missing lease"
    );

    let orphan_wake = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "emit",
            "--run-id",
            "run_unowned",
            "--no-owner-reason",
            "manual checkpoint has no active supervisor",
            "--kind",
            "checkpoint",
            "--urgency",
            "critical",
            "--summary",
            "operator should assign a supervisor",
            "--requires-main-system",
            "--json",
        ])
        .output()
        .expect("orphan wake emit should execute");
    assert!(orphan_wake.status.success());
    let orphan_json = output_json(&orphan_wake);
    assert_eq!(orphan_json["data"]["state"], "escalated");
    assert_eq!(
        orphan_json["data"]["no_owner_reason"],
        "manual checkpoint has no active supervisor"
    );

    let wake_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "emit",
            "--run-id",
            "run_state_machine",
            "--lease-id",
            &lease_id,
            "--owner-agent",
            "agent_alpha",
            "--kind",
            "blocked",
            "--summary",
            "needs acknowledgement",
            "--json",
        ])
        .output()
        .expect("wake emit should execute");
    assert!(wake_output.status.success());
    let wake_id = output_json(&wake_output)["data"]["wake_id"]
        .as_str()
        .expect("wake id should exist")
        .to_string();

    let ack_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "ack",
            "--wake-id",
            &wake_id,
            "--actor",
            "operator",
            "--json",
        ])
        .output()
        .expect("wake ack should execute");
    assert!(ack_output.status.success());
    assert_eq!(output_json(&ack_output)["data"]["state"], "acknowledged");

    let resolve_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "wake",
            "resolve",
            "--wake-id",
            &wake_id,
            "--resolution",
            "operator assigned next action",
            "--json",
        ])
        .output()
        .expect("wake resolve should execute");
    assert!(resolve_output.status.success());
    assert_eq!(output_json(&resolve_output)["data"]["state"], "resolved");

    let resolved_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status should execute");
    assert!(resolved_status_output.status.success());
    let resolved_status = output_json(&resolved_status_output);
    let resolved_lease = resolved_status["data"]["leases"]
        .as_array()
        .expect("leases should exist")
        .iter()
        .find(|lease| lease["lease_id"] == lease_id)
        .expect("lease should remain visible");
    assert_eq!(resolved_lease["pending_wake_count"], 0);
    assert_eq!(resolved_status["data"]["resolved_wake_count"], 1);

    let lease_path = workspace_root
        .join(".pmcli/projectops/leases")
        .join(format!("{lease_id}.json"));
    let mut stale_lease: Value =
        serde_json::from_str(&std::fs::read_to_string(&lease_path).expect("lease should read"))
            .expect("lease json should parse");
    stale_lease["heartbeat_at"] = Value::String("1".to_string());
    write_text_file(
        &lease_path,
        &serde_json::to_string_pretty(&stale_lease).unwrap(),
    );

    let stale_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projectops", "status", "--json"])
        .output()
        .expect("projectops status should execute");
    assert!(stale_status_output.status.success());
    let stale_status = output_json(&stale_status_output);
    let stale_lease = stale_status["data"]["leases"]
        .as_array()
        .expect("leases should exist")
        .iter()
        .find(|lease| lease["lease_id"] == lease_id)
        .expect("stale lease should remain visible");
    assert_eq!(stale_lease["state"], "stale");

    let reclaimed_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "projectops",
            "lease",
            "acquire",
            "--run-id",
            "run_state_machine",
            "--owner-agent",
            "agent_beta",
            "--json",
        ])
        .output()
        .expect("reclaim lease should execute");
    assert!(reclaimed_output.status.success());
    let reclaimed = output_json(&reclaimed_output);
    assert_eq!(reclaimed["data"]["state"], "reclaimed");
    assert_eq!(reclaimed["data"]["reclaimed_from"], lease_id);
}

#[test]
fn docs_index_inspect_and_frame_refresh_dry_run_expose_docframes() {
    let workspace_root = init_git_workspace("docs_docframe");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    write_text_file(
        workspace_root.join("docs").join("plan.md"),
        r#"---
doc_frame:
  id: docs.plan
  title: Plan
  doc_type: plan
  lifecycle: active
  scope: milestone
  milestone: M7
  summary: Plan summary.
  key_claims:
    - DocFrame is parseable.
  decisions:
    - Use YAML front matter.
  interfaces:
    - schemas/doc_frame.schema.json
  evidence_refs:
    - docs/plan.md
  next_actions:
    - Build docs index.
  non_goals:
    - Replace MissionFrame.
  generated_by: human
  updated_at: 2026-04-25
---

# Plan
"#,
    );
    write_text_file(
        workspace_root.join("docs").join("missing.md"),
        "# Missing\n\nNo DocFrame yet.\n",
    );

    let index_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["docs", "index", "--json"])
        .output()
        .expect("docs index should execute");

    assert!(
        index_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&index_output.stderr)
    );
    let index_json = output_json(&index_output);
    assert_eq!(index_json["ok"], true);
    assert_eq!(index_json["command"], "docs index");
    assert_eq!(index_json["data"]["total_count"], 1);
    assert_eq!(index_json["data"]["missing_required_frames_count"], 1);
    assert!(index_json["data"]["doc_frames"]
        .as_array()
        .expect("doc frames")
        .iter()
        .any(|frame| frame["doc_id"] == "docs.plan"
            && frame["source_path"] == "docs/plan.md"
            && frame["title"] == "Plan"));

    let persisted_index = workspace_root
        .join(".pmcli")
        .join("docs")
        .join("index.json");
    assert!(persisted_index.exists(), "docs index should persist");

    let inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["docs", "inspect", "docs/plan.md", "--json"])
        .output()
        .expect("docs inspect should execute");

    assert!(
        inspect_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspect_json = output_json(&inspect_output);
    assert_eq!(inspect_json["ok"], true);
    assert_eq!(inspect_json["command"], "docs inspect");
    assert_eq!(inspect_json["data"]["status"], "valid");
    assert_eq!(inspect_json["data"]["doc_frame"]["doc_id"], "docs.plan");
    assert_eq!(inspect_json["data"]["doc_frame"]["lifecycle"], "active");

    let before_refresh =
        std::fs::read_to_string(workspace_root.join("docs").join("missing.md")).unwrap();
    let refresh_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "docs",
            "frame",
            "refresh",
            "docs/missing.md",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("docs frame refresh dry-run should execute");

    assert!(
        refresh_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&refresh_output.stderr)
    );
    let refresh_json = output_json(&refresh_output);
    assert_eq!(refresh_json["ok"], true);
    assert_eq!(refresh_json["command"], "docs frame refresh");
    assert_eq!(refresh_json["data"]["dry_run"], true);
    assert_eq!(
        refresh_json["data"]["candidate"]["source_path"],
        "docs/missing.md"
    );
    assert_eq!(refresh_json["data"]["write_applied"], false);
    let after_refresh =
        std::fs::read_to_string(workspace_root.join("docs").join("missing.md")).unwrap();
    assert_eq!(
        before_refresh, after_refresh,
        "dry-run must not rewrite doc"
    );
}

#[test]
fn docs_index_marks_stale_frames_and_runtime_context_uses_active_docframes() {
    let workspace_root = init_git_workspace("docs_stale_context");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    write_text_file(
        workspace_root.join("docs").join("active.md"),
        r#"---
doc_frame:
  doc_id: docs.active
  schema_version: "1"
  source_path: docs/active.md
  title: Active Doc
  doc_type: plan
  lifecycle: active
  scope: project
  summary: Active DocFrame projection should be visible to runtime context.
  generated_by: human
  updated_at: 2999-01-01
---

# Active
"#,
    );
    write_text_file(
        workspace_root.join("docs").join("stale.md"),
        r#"---
doc_frame:
  doc_id: docs.stale
  schema_version: "1"
  source_path: docs/stale.md
  title: Stale Doc
  doc_type: plan
  lifecycle: active
  scope: project
  summary: Stale DocFrame should be reported but not injected.
  generated_by: human
  updated_at: 2000-01-01
---

# Stale
"#,
    );

    let index_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["docs", "index", "--json"])
        .output()
        .expect("docs index should execute");
    assert!(index_output.status.success());
    let index_json = output_json(&index_output);
    assert_eq!(index_json["data"]["stale_count"], 1);
    assert_eq!(
        index_json["data"]["stale_frames"][0]["source_path"],
        "docs/stale.md"
    );

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["prompt", "inspect", "doc", "context", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(prompt_output.status.success());
    let prompt_json = output_json(&prompt_output);
    assert_eq!(
        prompt_json["data"]["doc_context"]["projection_policy"],
        "active_non_stale_docframes_after_mission_frame"
    );
    assert!(prompt_json["data"]["doc_context"]["doc_frames"]
        .as_array()
        .expect("doc frames should exist")
        .iter()
        .any(|frame| frame["doc_id"] == "docs.active"));
    assert!(!serde_json::to_string(&prompt_json["data"]["doc_context"])
        .expect("doc context should serialize")
        .contains("docs.stale"));

    let resume_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["resume", "latest", "--json"])
        .output()
        .expect("resume should execute");
    assert!(resume_output.status.success());
    assert_eq!(
        output_json(&resume_output)["data"]["doc_context"]["active_count"],
        1
    );

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", "latest", "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    assert!(
        output_json(&compact_output)["data"]["derived_views_updated"]
            .as_array()
            .expect("derived views should exist")
            .iter()
            .any(|value| value == "doc_context_projection")
    );
}

#[test]
fn doc_context_reports_total_injected_and_omitted_active_frames() {
    let workspace_root = init_git_workspace("docs_context_counts");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    for index in 0..9 {
        write_text_file(
            workspace_root
                .join("docs")
                .join(format!("active_{index}.md")),
            &format!(
                r#"---
doc_frame:
  id: docs.active_{index}
  title: Active {index}
  doc_type: plan
  lifecycle: active
  scope: project
  summary: Active frame {index}
  generated_by: test
  updated_at: 9999999999999
---

# Active {index}
"#
            ),
        );
    }

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["prompt", "inspect", "doc", "counts", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(prompt_output.status.success());
    let doc_context = output_json(&prompt_output)["data"]["doc_context"].clone();
    assert_eq!(doc_context["active_count"], 9);
    assert_eq!(doc_context["injected_count"], 8);
    assert_eq!(doc_context["omitted_active_count"], 1);
    assert_eq!(
        doc_context["doc_frames"]
            .as_array()
            .expect("doc frames should exist")
            .len(),
        8
    );
}

#[test]
fn artifacts_and_reviews_reject_paths_outside_workspace() {
    let workspace_root = init_git_workspace("artifact_review_scope");
    let registry_root = registry_root_env();
    let outside_root = unique_temp_dir("outside_review_scope");
    let outside_file = outside_root.join("outside.md");
    write_text_file(&outside_file, "outside scope\n");
    write_text_file(workspace_root.join("README.md"), "inside scope\n");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    for command in [
        vec![
            "reviews",
            "open",
            "--objective",
            "must reject absolute target",
            "--target",
            outside_file.to_str().expect("outside path"),
            "--json",
        ],
        vec![
            "artifacts",
            "inspect",
            outside_file.to_str().expect("outside path"),
            "--json",
        ],
    ] {
        let output = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args(command)
            .output()
            .expect("scope command should execute");
        assert!(!output.status.success());
        let json = output_json(&output);
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"]["code"], "workspace_scope_violation");
    }
}

#[test]
fn repo_cleanup_restore_reports_typed_failure_for_unknown_plan() {
    let workspace_root = init_git_workspace("cleanup_restore_unknown");
    let registry_root = registry_root_env();

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
        .args(["repo", "cleanup-restore", "missing_plan", "--json"])
        .output()
        .expect("repo cleanup-restore should execute");

    assert!(!output.status.success());
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "repo cleanup-restore");
    assert_eq!(json["error"]["code"], "cleanup_restore_invalid");
    assert_eq!(output.status.code(), Some(9));
}

#[test]
fn repo_cleanup_apply_reports_typed_failure_when_snapshot_gate_is_missing() {
    let workspace_root = init_git_workspace("cleanup_apply_missing_snapshot");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    write_text_file(workspace_root.join(".pmcli/archive/stale.tmp"), "stale");
    let plan_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-plan", "--json"])
        .output()
        .expect("repo cleanup-plan should execute");
    assert!(plan_output.status.success());
    let plan_json = output_json(&plan_output);
    let plan_id = plan_json["data"]["plan_id"].as_str().expect("plan id");
    let snapshot_ref = plan_json["data"]["rollback_snapshot_ref"]
        .as_str()
        .expect("snapshot ref");
    std::fs::remove_dir_all(snapshot_ref).expect("snapshot should be removable");

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["repo", "cleanup-apply", plan_id, "--json"])
        .output()
        .expect("repo cleanup-apply should execute");

    assert!(!output.status.success());
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "repo cleanup-apply");
    assert_eq!(json["error"]["code"], "cleanup_plan_invalid");
    assert_eq!(output.status.code(), Some(9));
}

#[test]
fn artifact_family_manifest_refreshes_when_new_artifacts_are_added() {
    let workspace_root = init_git_workspace("artifact_manifest_refresh");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let first_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "list", "--include-archive", "--json"])
        .output()
        .expect("artifacts list should execute");
    assert!(first_output.status.success());
    let first_json = output_json(&first_output);
    let archive_family = first_json["data"]["families"]
        .as_array()
        .expect("families array")
        .iter()
        .find(|entry| entry["family"] == "archive")
        .expect("archive family");
    assert_eq!(
        archive_family["archive_ids"]
            .as_array()
            .expect("archive ids array")
            .len(),
        0
    );

    write_text_file(
        workspace_root.join(".pmcli/archive/new.tmp"),
        "new artifact",
    );
    let second_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "list", "--include-archive", "--json"])
        .output()
        .expect("artifacts list should execute");
    assert!(second_output.status.success());
    let second_json = output_json(&second_output);
    let refreshed_archive_family = second_json["data"]["families"]
        .as_array()
        .expect("families array")
        .iter()
        .find(|entry| entry["family"] == "archive")
        .expect("archive family");
    assert!(refreshed_archive_family["archive_ids"]
        .as_array()
        .expect("archive ids array")
        .iter()
        .any(|entry| entry == "artifact:archive:new.tmp"));
}

#[test]
fn stale_current_project_pointer_returns_typed_failure_json() {
    let workspace_root = init_git_workspace("stale_pointer");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    std::fs::remove_dir_all(&workspace_root).expect("workspace should be removable");
    let outside_root = unique_temp_dir("outside_pointer_scope");

    let output = Command::new(cargo_bin())
        .current_dir(&outside_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "current", "--json"])
        .output()
        .expect("projects current should execute");

    assert_eq!(output.status.code(), Some(6));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "projects current");
    assert_eq!(json["error"]["code"], "project_unresolved");
    assert_eq!(json["data"]["resolution_status"], "unresolved");
    assert_eq!(json["data"]["resolution_source"], "current_project_pointer");
    assert_eq!(
        json["data"]["current_pointer"]["workspace_root"],
        workspace_root.display().to_string()
    );
    let warnings = json["data"]["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|value| value == "stale_current_project_pointer"));
}

#[test]
fn unknown_session_resume_returns_typed_failure_json() {
    let workspace_root = init_git_workspace("unknown_session");
    let registry_root = registry_root_env();

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
        .args(["sessions", "resume", "sess_missing", "--json"])
        .output()
        .expect("sessions resume should execute");

    assert_eq!(output.status.code(), Some(6));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "sessions resume");
    assert_eq!(json["error"]["code"], "session_not_found");
    assert_eq!(json["data"]["selector"], "sess_missing");
}

#[test]
fn explicit_scope_flags_follow_documented_precedence() {
    let workspace_a = init_git_workspace("scope_a");
    let workspace_b = init_git_workspace("scope_b");
    let registry_root = registry_root_env();

    let init_a = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init A should execute");
    assert!(init_a.status.success());
    let project_a = output_json(&init_a)["data"]["project_id"]
        .as_str()
        .expect("project id A")
        .to_string();

    let init_b = Command::new(cargo_bin())
        .current_dir(&workspace_b)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init B should execute");
    assert!(init_b.status.success());
    let project_b = output_json(&init_b)["data"]["project_id"]
        .as_str()
        .expect("project id B")
        .to_string();

    let explicit_project = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["--project", &project_b, "projects", "current", "--json"])
        .output()
        .expect("explicit project should execute");
    assert!(explicit_project.status.success());
    let explicit_project_json = output_json(&explicit_project);
    assert_eq!(explicit_project_json["project_id"], project_b);
    assert_eq!(
        explicit_project_json["data"]["requested_project_id"],
        project_b
    );
    assert_eq!(
        explicit_project_json["data"]["resolved_project_id"],
        project_b
    );
    assert_eq!(
        explicit_project_json["data"]["resolution_source"],
        "explicit_project"
    );

    let explicit_cwd = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "--cwd",
            workspace_b.to_str().expect("workspace path"),
            "projects",
            "current",
            "--json",
        ])
        .output()
        .expect("explicit cwd should execute");
    assert!(explicit_cwd.status.success());
    let explicit_cwd_json = output_json(&explicit_cwd);
    assert_eq!(explicit_cwd_json["project_id"], project_b);
    assert_eq!(
        explicit_cwd_json["data"]["requested_cwd"],
        workspace_b.display().to_string()
    );
    assert_eq!(explicit_cwd_json["data"]["resolved_project_id"], project_b);
    assert_eq!(
        explicit_cwd_json["data"]["resolution_source"],
        "explicit_cwd"
    );

    let both = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "--project",
            &project_a,
            "--cwd",
            workspace_b.to_str().expect("workspace path"),
            "projects",
            "current",
            "--json",
        ])
        .output()
        .expect("combined scope should execute");
    assert!(both.status.success());
    let both_json = output_json(&both);
    assert_eq!(both_json["project_id"], project_a);
    assert_eq!(both_json["data"]["requested_project_id"], project_a);
    assert_eq!(both_json["data"]["resolved_project_id"], project_a);
    assert_eq!(both_json["data"]["resolution_source"], "explicit_project");
}

#[test]
fn projects_init_honors_explicit_cwd_scope() {
    let workspace_root = init_git_workspace("projects_init_explicit_cwd");
    let outside_root = unique_temp_dir("projects_init_runner");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&outside_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "projects",
            "init",
            "--json",
        ])
        .output()
        .expect("projects init should execute through explicit cwd");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["workspace_root"],
        workspace_root.display().to_string()
    );
}

#[test]
fn projects_init_uses_stable_ids_and_session_access_refreshes_registry_metadata() {
    let shared_name = "same_name_workspace";
    let parent_a = unique_temp_dir("project_id_parent_a");
    let parent_b = unique_temp_dir("project_id_parent_b");
    let workspace_a = parent_a.join(shared_name);
    let workspace_b = parent_b.join(shared_name);
    std::fs::create_dir_all(workspace_a.join(".git")).expect("workspace A marker should exist");
    std::fs::create_dir_all(workspace_b.join(".git")).expect("workspace B marker should exist");

    let registry_root = registry_root_env();

    let init_a = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init A should execute");
    assert!(init_a.status.success());
    let init_a_json = output_json(&init_a);
    let project_a = init_a_json["data"]["project_id"]
        .as_str()
        .expect("project id A")
        .to_string();

    let init_b = Command::new(cargo_bin())
        .current_dir(&workspace_b)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init B should execute");
    assert!(init_b.status.success());
    let init_b_json = output_json(&init_b);
    let project_b = init_b_json["data"]["project_id"]
        .as_str()
        .expect("project id B")
        .to_string();

    assert_ne!(project_a, project_b);
    assert!(project_a.starts_with("proj_same_name_workspace_"));
    assert!(project_b.starts_with("proj_same_name_workspace_"));

    let initial_list = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "list", "--json"])
        .output()
        .expect("projects list should execute");
    assert!(initial_list.status.success());
    let initial_list_json = output_json(&initial_list);
    let initial_entry = initial_list_json["data"]["projects"]
        .as_array()
        .expect("projects array")
        .iter()
        .find(|entry| entry["project_id"] == project_a)
        .expect("project A entry should exist")
        .clone();
    let initial_last_accessed_at = initial_entry["last_accessed_at"]
        .as_str()
        .expect("last_accessed_at")
        .to_string();

    thread::sleep(Duration::from_millis(2));

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Refresh", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let create_json = output_json(&create_output);
    let session_id = create_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let refreshed_list = Command::new(cargo_bin())
        .current_dir(&workspace_a)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "list", "--json"])
        .output()
        .expect("projects list should execute after session create");
    assert!(refreshed_list.status.success());
    let refreshed_list_json = output_json(&refreshed_list);
    let refreshed_entry = refreshed_list_json["data"]["projects"]
        .as_array()
        .expect("projects array")
        .iter()
        .find(|entry| entry["project_id"] == project_a)
        .expect("project A entry should exist")
        .clone();

    assert_eq!(refreshed_entry["active_session_id"], session_id);
    assert_ne!(
        refreshed_entry["last_accessed_at"]
            .as_str()
            .expect("timestamp"),
        initial_last_accessed_at
    );
}

#[test]
fn text_mode_emits_operator_readable_output() {
    let workspace_root = init_git_workspace("text_mode_output");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init"])
        .output()
        .expect("projects init should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("projects init"));
    assert!(stdout.contains("project_id"));
}

#[test]
fn doctor_json_reports_blocked_state_with_typed_failure() {
    let outside_root = unique_temp_dir("doctor_blocked");
    let output = Command::new(cargo_bin())
        .current_dir(&outside_root)
        .args(["doctor", "--json"])
        .output()
        .expect("doctor should execute");

    assert_eq!(output.status.code(), Some(4));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "doctor");
    assert_eq!(json["error"]["code"], "doctor_blocked");
    assert_eq!(json["data"]["overall_status"], "blocked");
    assert_eq!(json["data"]["preflight"]["overall_status"], "blocked");
    assert_eq!(json["data"]["preflight"]["ready"], false);
}

#[test]
fn doctor_json_reports_ready_checks_machine_readably() {
    let workspace_root = init_git_workspace("doctor_ready");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["doctor", "--json"])
        .output()
        .expect("doctor should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "doctor");
    assert_eq!(json["data"]["overall_status"], "ready");
    assert_eq!(json["data"]["preflight"]["overall_status"], "ready");
    assert_eq!(json["data"]["preflight"]["ready"], true);
    assert!(json["data"]["preflight"]["project_trace"].is_object());
    assert_eq!(
        json["data"]["preflight"]["project_trace"]["resolution_status"],
        "resolved"
    );
    assert_eq!(json["data"]["workspace"]["status"], "ready");
    assert_eq!(json["data"]["project"]["status"], "ready");
}

#[test]
fn doctor_json_honors_explicit_cwd_scope() {
    let workspace_root = init_git_workspace("doctor_explicit_cwd");
    let outside_root = unique_temp_dir("doctor_explicit_cwd_runner");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let output = Command::new(cargo_bin())
        .current_dir(&outside_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("doctor should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["workspace"]["status"], "ready");
    assert_eq!(json["data"]["project"]["status"], "ready");
    assert_eq!(
        json["data"]["workspace"]["detail"],
        workspace_root.display().to_string()
    );
    assert_eq!(
        json["data"]["preflight"]["workspace_root"],
        workspace_root.display().to_string()
    );
}

#[test]
fn smoke_json_reports_blocked_state_with_typed_failure() {
    let outside_root = unique_temp_dir("smoke_blocked");
    let registry_root = registry_root_env();
    let output = Command::new(cargo_bin())
        .current_dir(&outside_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["smoke", "--json"])
        .output()
        .expect("smoke should execute");

    assert_eq!(output.status.code(), Some(4));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "smoke");
    assert_eq!(json["error"]["code"], "smoke_blocked");
    assert_eq!(json["data"]["overall_status"], "blocked");
}

#[test]
fn smoke_json_is_mutation_free_and_project_scoped() {
    let workspace_root = init_git_workspace("smoke_ready");
    let registry_root = registry_root_env();
    let source_file = workspace_root.join("README.md");
    std::fs::write(&source_file, "original\n").expect("source file should exist");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["smoke", "--json"])
        .output()
        .expect("smoke should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "smoke");
    assert_eq!(json["data"]["source_mutation_free"], true);
    let mutated_paths = json["data"]["mutated_paths"]
        .as_array()
        .expect("mutated_paths array");
    let data_dir = workspace_root.join(".pmcli");
    assert!(
        mutated_paths.iter().all(|value| {
            value
                .as_str()
                .map(|path| path.starts_with(data_dir.to_str().expect("data dir path")))
                .unwrap_or(false)
        }),
        "unexpected mutated paths: {mutated_paths:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&source_file).expect("source file should remain readable"),
        "original\n"
    );
    assert!(json["data"]["event_log_path"]
        .as_str()
        .unwrap_or_default()
        .contains("/.pmcli/"));
    assert!(json["data"]["transcript_path"]
        .as_str()
        .unwrap_or_default()
        .contains("/.pmcli/"));
    assert_eq!(json["data"]["preflight"]["overall_status"], "ready");
    assert_eq!(json["data"]["preflight"]["ready"], true);
    assert!(json["data"]["preflight"]["project_trace"].is_object());
    assert!(json["data"]["preflight"]["provider_status"].is_array());
    assert!(json["data"]["provider_trace"].is_object());

    let event_log_path = json["data"]["event_log_path"]
        .as_str()
        .expect("event log path should exist");
    let events = read_json_lines(event_log_path);
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "start"
            && event["object_kind"] == "command"
            && event["object_id"] == "smoke"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
            && event["object_kind"] == "command"
            && event["object_id"] == "smoke"
    }));
    assert!(events.iter().all(|event| event.get("timestamp").is_some()));
    assert!(events.iter().all(|event| event.get("project_id").is_some()));
}

#[test]
fn sessions_browse_returns_machine_readable_recent_sessions() {
    let workspace_root = init_git_workspace("sessions_browse");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Browse Me", "--json"])
        .status()
        .expect("sessions create should execute");
    assert!(create_status.success());

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "browse", "--json"])
        .output()
        .expect("sessions browse should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "sessions browse");
    assert!(json["data"]["sessions"]
        .as_array()
        .map(|v| !v.is_empty())
        .unwrap_or(false));
}

#[test]
fn sessions_export_returns_transcript_and_metadata() {
    let workspace_root = init_git_workspace("sessions_export");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Export Me", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let create_json = output_json(&create_output);
    let session_id = create_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let smoke_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["smoke", "--json"])
        .status()
        .expect("smoke should execute");
    assert!(smoke_status.success());

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "export", &session_id, "--json"])
        .output()
        .expect("sessions export should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "sessions export");
    assert_eq!(json["data"]["session"]["session_id"], session_id);
    assert!(json["data"]["transcript"].as_array().is_some());
    assert!(json["data"]["operator_logs"].as_array().is_some());
}

#[test]
fn sessions_logs_returns_machine_readable_operator_log_manifests() {
    let workspace_root = init_git_workspace("sessions_logs");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let smoke_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["smoke", "--json"])
        .output()
        .expect("smoke should execute");
    assert!(
        smoke_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&smoke_output.stderr)
    );

    let smoke_json = output_json(&smoke_output);
    let session_id = smoke_json["data"]["session_id"]
        .as_str()
        .expect("smoke session id");

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "logs", session_id, "--json"])
        .output()
        .expect("sessions logs should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "sessions logs");
    let manifests = json["data"]["operator_logs"]
        .as_array()
        .expect("operator_logs array");
    assert!(!manifests.is_empty());
    assert_eq!(manifests[0]["session_id"], session_id);
    assert_eq!(manifests[0]["project_id"], json["project_id"]);
    assert_eq!(manifests[0]["consistency_state"], "derived_from_transcript");
    assert!(manifests[0]["tool_results_log_path"]
        .as_str()
        .unwrap_or_default()
        .contains("/.pmcli/sessions/"));
    let read_models = json["data"]["derived_read_models"]
        .as_array()
        .expect("derived read models");
    assert!(!read_models.is_empty());
    assert_eq!(read_models[0]["authoritative"], false);
    assert_eq!(read_models[0]["rebuildable"], true);
}

#[test]
fn sessions_logs_supports_kind_and_request_seq_filters() {
    let workspace_root = init_git_workspace("sessions_logs_filters");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let smoke_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["smoke", "--json"])
        .output()
        .expect("smoke should execute");
    assert!(smoke_output.status.success());
    let session_id = output_json(&smoke_output)["data"]["session_id"]
        .as_str()
        .expect("smoke session id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "sessions",
            "logs",
            &session_id,
            "--request-seq",
            "1",
            "--kind",
            "tool_results",
            "--json",
        ])
        .output()
        .expect("sessions logs should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    let manifests = json["data"]["operator_logs"]
        .as_array()
        .expect("operator_logs array");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0]["request_seq"], 1);
    assert!(manifests[0]["tool_results_log_path"].is_string());
    assert!(manifests[0]["request_log_path"].is_null());
    assert!(manifests[0]["response_log_path"].is_null());
}

#[test]
fn sessions_stats_reports_project_counts_machine_readably() {
    let workspace_root = init_git_workspace("sessions_stats");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    for title in ["One", "Two"] {
        let status = Command::new(cargo_bin())
            .current_dir(&workspace_root)
            .env("RESEARCH_CLI_STATE_HOME", &registry_root)
            .args(["sessions", "create", "--title", title, "--json"])
            .status()
            .expect("sessions create should execute");
        assert!(status.success());
    }

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "stats", "--json"])
        .output()
        .expect("sessions stats should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "sessions stats");
    assert_eq!(json["data"]["session_count"], 2);
}

#[test]
fn inspect_returns_session_inspection_for_latest_session() {
    let workspace_root = init_git_workspace("inspect_session");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Inspect Me", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["inspect", "--json"])
        .output()
        .expect("inspect should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "inspect");
    assert_eq!(json["session_id"], session_id);
    assert_eq!(json["data"]["session_id"], session_id);
    assert_eq!(json["data"]["session"]["title"], "Inspect Me");
    assert!(json["data"]["lineage"].is_array());
    assert!(json["data"]["transcript_source"].is_object());
    assert!(json["data"]["operator_logs"].is_array());
    assert_eq!(json["data"]["section_status"]["provider"], "not_graduated");
    assert_eq!(
        json["data"]["section_status"]["permission"],
        "not_graduated"
    );

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "inspection"
            && event["object_kind"] == "session"
            && event["object_id"] == session_id
    }));
}

#[test]
fn inspect_project_returns_project_core_with_not_graduated_sections() {
    let workspace_root = init_git_workspace("inspect_project");
    let registry_root = registry_root_env();
    write_text_file(
        workspace_root.join("AGENTS.md"),
        "Advisory project guidance: cite local artifacts and follow review gates.",
    );

    let init_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init should execute");
    assert!(init_output.status.success());
    let init_json = output_json(&init_output);
    let project_id = init_json["data"]["project_id"]
        .as_str()
        .expect("project id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["inspect", "--project", "--json"])
        .output()
        .expect("inspect --project should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "inspect");
    assert_eq!(json["project_id"], project_id);
    assert_eq!(json["data"]["project_id"], project_id);
    assert_eq!(json["data"]["registry_entry"]["project_id"], project_id);
    assert_eq!(json["data"]["init_state"], "registered");
    assert_eq!(json["data"]["section_status"]["projectops"], "available");
    assert_eq!(
        json["data"]["section_status"]["memory"],
        "durable_memory_available"
    );
    assert_eq!(json["data"]["section_status"]["branches"], "guarded");
    assert_eq!(json["data"]["section_status"]["reviews"], "available");
    assert_eq!(
        json["data"]["section_status"]["active_runs"],
        "not_graduated"
    );
    assert_eq!(json["data"]["section_status"]["cleanup"], "available");
    assert_eq!(json["data"]["section_status"]["canonicality"], "available");
    assert_eq!(
        json["data"]["project_continuity"]["schema_version"],
        "project_continuity_snapshot.v1"
    );
    let context_files = json["data"]["project_continuity"]["project_context_files"]
        .as_array()
        .expect("project continuity context files");
    assert!(context_files.iter().any(|context| {
        context["path"] == "AGENTS.md"
            && context["advisory_only"] == true
            && context["canonical_authority"] == false
    }));
    assert!(
        json["data"]["project_continuity"]["project_context_warnings"]
            .as_array()
            .expect("project context warnings")
            .is_empty()
    );
    assert!(json["data"]["sections"].is_null());
}

#[test]
fn inspect_project_rejects_unknown_inline_flag() {
    let workspace_root = init_git_workspace("inspect_project_bad_flag");
    let registry_root = registry_root_env();

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
        .args(["inspect", "--project", "--bogus", "--json"])
        .output()
        .expect("inspect should execute");

    assert_eq!(output.status.code(), Some(2));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "inspect");
    assert_eq!(json["error"]["code"], "usage_invalid");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("unknown flag"));
}

#[test]
fn compact_writes_summary_reference_and_reports_derived_views() {
    let workspace_root = init_git_workspace("compact_session");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Compact Me", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let transcript_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("transcript.jsonl");
    let mut transcript = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)
        .expect("transcript should open");
    for seq in 0..3 {
        writeln!(
            transcript,
            "{}",
            serde_json::json!({
                "line_type": "message",
                "role": "user",
                "content": format!("request {seq}")
            })
        )
        .expect("user line should append");
        writeln!(
            transcript,
            "{}",
            serde_json::json!({
                "line_type": "message",
                "role": "assistant",
                "content": format!("response {seq}")
            })
        )
        .expect("assistant line should append");
    }

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", &session_id, "--json"])
        .output()
        .expect("compact should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "compact");
    assert_eq!(json["session_id"], session_id);
    assert_eq!(json["data"]["session_id"], session_id);
    assert_eq!(json["data"]["raw_log_retained"], true);
    assert_eq!(json["data"]["lineage_ok"], true);
    assert!(json["data"]["compaction_record_id"]
        .as_str()
        .unwrap_or_default()
        .starts_with("cmp_"));
    assert!(json["data"]["summary_ref"]
        .as_str()
        .unwrap_or_default()
        .contains("summary_"));
    let candidate_id = json["data"]["promotion_candidate_id"]
        .as_str()
        .expect("promotion candidate id should be present");
    assert!(candidate_id.starts_with("cand_"));
    assert_eq!(json["data"]["promotion_queue_status"], "pending_review");
    assert!(json["data"]["resume_recap_ref"]
        .as_str()
        .unwrap_or_default()
        .contains("resume_recap_"));
    assert!(
        json["data"]["compacted_turn_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    let derived = json["data"]["derived_views_updated"]
        .as_array()
        .expect("derived views");
    assert!(derived.iter().any(|value| value == "resume_recap"));
    assert!(derived.iter().any(|value| value == "session_search_index"));
    let deferred = json["data"]["deferred_repairs"]
        .as_array()
        .expect("deferred repairs");
    assert!(deferred.is_empty());

    let transcript_contents =
        std::fs::read_to_string(&transcript_path).expect("transcript should remain readable");
    assert!(transcript_contents.contains("\"line_type\":\"summary_reference\""));

    let summary_ref = json["data"]["summary_ref"]
        .as_str()
        .expect("summary ref should be present");
    let summary_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("summaries")
        .join(format!("{summary_ref}.md"));
    assert!(summary_path.exists(), "summary artifact should be written");
    let summary_contents =
        std::fs::read_to_string(&summary_path).expect("summary artifact should be readable");
    assert!(summary_contents.contains("request 0"));

    let candidates_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "candidates", "--json"])
        .output()
        .expect("artifacts candidates should execute");
    assert!(candidates_output.status.success());
    let candidates_json = output_json(&candidates_output);
    assert_eq!(candidates_json["command"], "artifacts candidates");
    assert!(candidates_json["data"]["candidates"]
        .as_array()
        .expect("candidates array")
        .iter()
        .any(|entry| entry["candidate_id"] == candidate_id
            && entry["status"] == "pending_review"
            && entry["source_artifact_path"].as_str() == summary_path.to_str()));

    let blocked_promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "promote", candidate_id, "--json"])
        .output()
        .expect("artifacts promote should execute");
    assert!(!blocked_promote_output.status.success());
    let blocked_promote_json = output_json(&blocked_promote_output);
    assert_eq!(blocked_promote_json["command"], "artifacts promote");
    assert_eq!(
        blocked_promote_json["error"]["code"],
        "canonicality_gate_blocked"
    );
    assert!(blocked_promote_json["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("release canonicality audit"));
    assert!(blocked_promote_json["data"]["blocking_violations"]
        .as_array()
        .expect("blocking violations")
        .iter()
        .any(|violation| violation["path"] == ".pmcli/canonical_surface.json"));

    write_explicit_canonical_surface_manifest(&workspace_root);

    let missing_code_owner_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "promote", candidate_id, "--json"])
        .output()
        .expect("artifacts promote should execute");
    assert!(!missing_code_owner_output.status.success());
    let missing_code_owner_json = output_json(&missing_code_owner_output);
    assert_eq!(missing_code_owner_json["command"], "artifacts promote");
    assert_eq!(
        missing_code_owner_json["error"]["code"],
        "canonicality_gate_blocked"
    );
    assert!(missing_code_owner_json["data"]["blocking_violations"]
        .as_array()
        .expect("blocking violations")
        .iter()
        .any(|violation| violation["path"] == ".pmcli/code_owner_manifest.json"));

    write_explicit_code_owner_manifest(&workspace_root);
    write_explicit_canonical_lineage_manifest(&workspace_root);

    let promote_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "promote", candidate_id, "--json"])
        .output()
        .expect("artifacts promote should execute");
    assert!(
        promote_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&promote_output.stderr)
    );
    let promote_json = output_json(&promote_output);
    assert_eq!(promote_json["command"], "artifacts promote");
    assert_eq!(promote_json["data"]["candidate_id"], candidate_id);
    assert_eq!(promote_json["data"]["artifact_family"], "summaries");
    assert!(promote_json["data"]["promoted_artifact_id"]
        .as_str()
        .unwrap_or_default()
        .starts_with("artifact:summaries:"));
    let promoted_artifact_id = promote_json["data"]["promoted_artifact_id"]
        .as_str()
        .expect("promoted artifact id");

    let summaries_inspect_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["artifacts", "inspect", "summaries", "--json"])
        .output()
        .expect("artifacts inspect summaries should execute");
    assert!(summaries_inspect_output.status.success());
    let summaries_inspect_json = output_json(&summaries_inspect_output);
    assert_eq!(
        summaries_inspect_json["data"]["manifest"]["latest_id"],
        promoted_artifact_id
    );
    assert!(summaries_inspect_json["data"]["promotion_history"]
        .as_array()
        .expect("promotion history array")
        .iter()
        .any(|entry| entry["event"] == "artifact_promoted"
            && entry["artifact_id"] == promoted_artifact_id));

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "session_compaction"
            && event["object_id"] == session_id
            && event["session_id"] == session_id
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "goal_alignment_trace"
            && event["object_id"] == session_id
            && event["payload"]["command"] == "compact"
            && event["payload"]["alignment_status"] == "mission_frame_missing"
    }));
}

#[test]
fn continue_resumes_latest_session_in_scope() {
    let workspace_root = init_git_workspace("continue_latest");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Continue Me", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["continue", "--json"])
        .output()
        .expect("continue should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "continue");
    assert_eq!(json["session_id"], session_id);
    assert_eq!(json["data"]["session_id"], session_id);
    assert_eq!(json["data"]["project_id"], json["project_id"]);
    assert_eq!(json["data"]["session_kind"], "interactive");
    assert!(json["data"]["recap"].is_object());
    assert!(json["data"]["lineage"].is_object());

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "session_resume"
            && event["object_id"] == "continue"
            && event["session_id"] == session_id
    }));
}

#[test]
fn bare_continue_flag_resumes_latest_session_in_scope() {
    let workspace_root = init_git_workspace("bare_continue_flag");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Continue Flag", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["--continue", "--json"])
        .output()
        .expect("bare continue should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "continue");
    assert_eq!(json["session_id"], session_id);
}

#[test]
fn resume_returns_selected_session_with_recap_and_lineage() {
    let workspace_root = init_git_workspace("resume_latest");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "create", "--title", "Resume Me", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["resume", "latest", "--json"])
        .output()
        .expect("resume should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "resume");
    assert_eq!(json["session_id"], session_id);
    assert_eq!(json["data"]["session_id"], session_id);
    assert_eq!(json["data"]["project_id"], json["project_id"]);
    assert_eq!(json["data"]["session_kind"], "interactive");
    assert!(json["data"]["recap"].is_object());
    assert!(json["data"]["lineage"].is_object());
}

#[test]
fn sessions_search_returns_ranked_hits_from_canonical_artifacts() {
    let workspace_root = init_git_workspace("sessions_search");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["smoke", "--json"])
        .output()
        .expect("smoke should execute");
    assert!(
        create_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["sessions", "search", "smoke", "--json"])
        .output()
        .expect("sessions search should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "sessions search");
    assert!(json["data"]["hits"]
        .as_array()
        .map(|hits| !hits.is_empty())
        .unwrap_or(false));
}

#[test]
fn providers_inspect_reports_resolution_trace_machine_readably() {
    let workspace_root = init_git_workspace("providers_inspect");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("OPENAI_BASE_URL")
        .args(["providers", "inspect", "openai", "--json"])
        .output()
        .expect("providers inspect should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers inspect");
    assert_eq!(json["data"]["provider_id"], "openai");
    assert_eq!(json["data"]["auth_status"], "configured");
    assert_eq!(json["data"]["auth_source"], "environment");
    assert_eq!(json["data"]["base_url_source"], "default");
    assert_eq!(json["data"]["catalog_source"], "embedded");
    let supported_models = json["data"]["supported_models"]
        .as_array()
        .expect("supported models");
    assert!(!supported_models.is_empty());
    assert!(supported_models.iter().any(|model| model == "gpt-5.4"));
    assert!(supported_models.iter().any(|model| model == "gpt-5.4-mini"));
}

#[test]
fn providers_inspect_unknown_provider_returns_typed_failure() {
    let workspace_root = init_git_workspace("providers_inspect_unknown");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["providers", "inspect", "unknown-provider", "--json"])
        .output()
        .expect("providers inspect should execute");

    assert_eq!(output.status.code(), Some(6));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "providers inspect");
    assert_eq!(json["error"]["code"], "provider_not_found");
}

#[test]
fn providers_list_reports_catalog_and_auth_matrix_machine_readably() {
    let workspace_root = init_git_workspace("providers_list");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env_remove("OPENAI_BASE_URL")
        .args(["providers", "list", "--json"])
        .output()
        .expect("providers list should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers list");
    assert!(json["data"]["total_count"].as_u64().unwrap_or(0) >= 2);

    let providers = json["data"]["providers"]
        .as_array()
        .expect("providers list");
    let openai = providers
        .iter()
        .find(|provider| provider["provider_id"] == "openai")
        .expect("openai provider");
    assert_eq!(openai["auth_status"], "configured");
    assert_eq!(openai["catalog_source"], "embedded");
    assert_eq!(openai["base_url_source"], "default");
    assert!(openai["supported_models"]
        .as_array()
        .map(|models| models.iter().any(|value| value == "gpt-5.4"))
        .unwrap_or(false));
    assert!(providers
        .iter()
        .any(|provider| provider["provider_id"] == "anthropic"));
}

#[test]
fn providers_auth_status_reports_provider_scoped_base_urls_machine_readably() {
    let workspace_root = init_git_workspace("providers_auth_status");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_BASE_URL", "https://proxy.example/v1")
        .env("ANTHROPIC_BASE_URL", "https://anthropic-proxy.example")
        .env("OPENAI_API_KEY", "sk-test")
        .args(["providers", "auth-status", "--json"])
        .output()
        .expect("providers auth-status should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers auth-status");
    let providers = json["data"]["providers"]
        .as_array()
        .expect("providers list");
    let openai = providers
        .iter()
        .find(|provider| provider["provider_id"] == "openai")
        .expect("openai provider");
    let anthropic = providers
        .iter()
        .find(|provider| provider["provider_id"] == "anthropic")
        .expect("anthropic provider");
    assert_eq!(openai["base_url_source"], "environment");
    assert_eq!(openai["base_url"], "https://proxy.example/v1");
    assert_eq!(anthropic["base_url_source"], "environment");
    assert_eq!(anthropic["base_url"], "https://anthropic-proxy.example");
}

#[test]
fn providers_test_reports_live_lane_readiness_machine_readably() {
    let workspace_root = init_git_workspace("providers_test_ready");
    let registry_root = registry_root_env();
    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection should arrive");
        let mut buffer = [0_u8; 512];
        let bytes_read = stream.read(&mut buffer).expect("request should read");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .expect("response should write");
        String::from_utf8_lossy(&buffer[..bytes_read]).to_string()
    });

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", format!("http://127.0.0.1:{port}/v1"))
        .args(["providers", "test", "openai", "--json"])
        .output()
        .expect("providers test should execute");
    let request = server.join().expect("server thread should join");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers test");
    assert_eq!(json["data"]["provider_id"], "openai");
    assert_eq!(json["data"]["live_ready"], true);
    assert_eq!(json["data"]["overall_status"], "ready");
    assert_eq!(json["data"]["transport"]["lane"], "http_request");
    assert_eq!(json["data"]["transport"]["status"], "ready");
    assert_eq!(json["data"]["auth_status"], "configured");
    assert!(
        request.starts_with("GET /v1/models HTTP/1.1\r\n"),
        "unexpected request: {request}"
    );
    assert!(
        request.contains("Authorization: Bearer sk-test\r\n"),
        "missing auth header in request: {request}"
    );

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "provider_test"
            && event["phase"] == "terminal"
            && event["payload"]["provider_id"] == "openai"
    }));
}

#[test]
fn providers_test_https_endpoint_reports_reachability_without_claiming_full_http_readiness() {
    let workspace_root = init_git_workspace("providers_test_https_reachable");
    let registry_root = registry_root_env();
    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let server = thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.peer_addr().expect("peer addr should exist");
                    return true;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("listener accept failed: {err}"),
            }
        }
    });

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", format!("https://127.0.0.1:{port}/v1"))
        .args(["providers", "test", "openai", "--json"])
        .output()
        .expect("providers test should execute");
    let accepted = server.join().expect("server thread should join");
    assert!(accepted, "providers test should attempt a TCP connection");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers test");
    assert_eq!(json["data"]["provider_id"], "openai");
    assert_eq!(json["data"]["auth_status"], "configured");
    assert_eq!(json["data"]["overall_status"], "degraded");
    assert_eq!(json["data"]["live_ready"], false);
    assert_eq!(json["data"]["transport"]["lane"], "tcp_connect");
    assert_eq!(json["data"]["transport"]["status"], "reachable");
    assert_eq!(
        json["data"]["transport"]["endpoint"],
        format!("127.0.0.1:{port}")
    );
    assert!(json["data"]["hints"]
        .as_array()
        .expect("hints should be an array")
        .iter()
        .any(|hint| hint
            .as_str()
            .is_some_and(|value| value.contains("http:// base URL override"))));

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "provider_test"
            && event["phase"] == "terminal"
            && event["payload"]["provider_id"] == "openai"
            && event["payload"]["transport"]["lane"] == "tcp_connect"
            && event["payload"]["overall_status"] == "degraded"
    }));
}

#[test]
fn providers_test_missing_credentials_returns_blocked_typed_failure() {
    let workspace_root = init_git_workspace("providers_test_blocked");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env_remove("ANTHROPIC_API_KEY")
        .args(["providers", "test", "anthropic", "--json"])
        .output()
        .expect("providers test should execute");

    assert_eq!(output.status.code(), Some(4));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "providers test");
    assert_eq!(json["error"]["code"], "provider_test_blocked");
    assert_eq!(json["data"]["provider_id"], "anthropic");
    assert_eq!(json["data"]["overall_status"], "blocked");
    assert_eq!(json["data"]["live_ready"], false);
}

#[test]
fn providers_test_default_https_endpoint_returns_typed_reachability_failure_when_unreachable() {
    let workspace_root = init_git_workspace("providers_test_default_https_unreachable");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .env("OPENAI_BASE_URL", "https://127.0.0.1:0/v1")
        .args(["providers", "test", "openai", "--json"])
        .output()
        .expect("providers test should execute");

    assert_eq!(output.status.code(), Some(4));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "providers test");
    assert_eq!(json["error"]["code"], "provider_test_blocked");
    assert_eq!(json["data"]["provider_id"], "openai");
    assert_eq!(json["data"]["overall_status"], "blocked");
    assert_eq!(json["data"]["live_ready"], false);
    assert_eq!(json["data"]["transport"]["lane"], "tcp_connect");
    assert_eq!(json["data"]["transport"]["status"], "blocked");
    assert_eq!(json["data"]["transport"]["endpoint"], "127.0.0.1:0");
}

#[test]
fn providers_refresh_catalog_reports_embedded_catalog_state_machine_readably() {
    let workspace_root = init_git_workspace("providers_refresh_catalog");
    let registry_root = registry_root_env();
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
        .args(["providers", "refresh-catalog", "--json"])
        .output()
        .expect("providers refresh-catalog should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers refresh-catalog");
    assert_eq!(json["data"]["requested_source"], "embedded");
    assert_eq!(json["data"]["applied_source"], "embedded");
    assert_eq!(json["data"]["total_count"], 2);
    let providers = json["data"]["providers"]
        .as_array()
        .expect("providers array");
    let openai = providers
        .iter()
        .find(|provider| provider["provider_id"] == "openai")
        .expect("openai catalog state");
    assert_eq!(openai["catalog_source"], "embedded");
    assert_eq!(openai["embedded"], true);
    assert_eq!(openai["refreshable"], true);
    assert_eq!(openai["model_count"], 7);

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "provider_catalog"
            && event["phase"] == "terminal"
            && event["payload"]["requested_source"] == "embedded"
    }));
}

#[test]
fn providers_refresh_catalog_remote_reports_not_graduated_typed_failure() {
    let workspace_root = init_git_workspace("providers_refresh_catalog_remote");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["providers", "refresh-catalog", "remote", "--json"])
        .output()
        .expect("providers refresh-catalog should execute");

    assert_eq!(output.status.code(), Some(8));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "providers refresh-catalog");
    assert_eq!(json["error"]["code"], "not_graduated");
    assert_eq!(
        json["data"]["requested_command"],
        "providers refresh-catalog"
    );
    assert_eq!(json["data"]["gate_state"], "not_graduated");
    assert_eq!(json["data"]["feature_id"], "providers.refresh_catalog");
}

#[test]
fn providers_refresh_catalog_degraded_source_reports_exit_11_with_data() {
    let workspace_root = init_git_workspace("providers_refresh_catalog_degraded");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["providers", "refresh-catalog", "degraded-fixture", "--json"])
        .output()
        .expect("providers refresh-catalog should execute");

    assert_eq!(output.status.code(), Some(11));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "providers refresh-catalog");
    assert_eq!(json["error"]["code"], "degraded_but_loadable");
    assert_eq!(json["data"]["feature_id"], "providers.refresh_catalog");
    assert_eq!(
        json["data"]["requested_command"],
        "providers refresh-catalog"
    );
    assert_eq!(json["data"]["gate_state"], "degraded");
    assert_eq!(json["data"]["exit_lane"], 11);
    assert!(json["data"]["next_steps"].is_array());
}

#[test]
fn providers_refresh_catalog_file_source_reports_catalog_state_machine_readably() {
    let workspace_root = init_git_workspace("providers_refresh_catalog_file");
    let registry_root = registry_root_env();
    let file_source = unique_temp_dir("providers_refresh_catalog_file_source").join("catalog.json");
    crate::support::write_text_file(
        &file_source,
        r#"{"version":"fixture-v1","providers":[{"provider_id":"fixture-ai","models":["fixture-large","fixture-small"]},{"provider_id":"local-llm","models":["local-chat"]}]}"#,
    );
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
        .args([
            "providers",
            "refresh-catalog",
            file_source.to_str().expect("file source"),
            "--json",
        ])
        .output()
        .expect("providers refresh-catalog should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "providers refresh-catalog");
    assert_eq!(
        json["data"]["requested_source"],
        file_source.to_str().expect("file source")
    );
    assert_eq!(json["data"]["applied_source"], "file");
    assert_eq!(json["data"]["catalog_version"], "fixture-v1");
    assert_eq!(json["data"]["total_count"], 2);
    let fixture = json["data"]["providers"]
        .as_array()
        .expect("providers array")
        .iter()
        .find(|provider| provider["provider_id"] == "fixture-ai")
        .expect("fixture provider should exist");
    assert_eq!(fixture["catalog_source"], "file");
    assert_eq!(fixture["embedded"], false);
    assert_eq!(fixture["refreshable"], true);
    assert_eq!(fixture["model_count"], 2);

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "provider_catalog"
            && event["phase"] == "terminal"
            && event["object_id"] == "file"
            && event["payload"]["catalog_version"] == "fixture-v1"
    }));
}

#[test]
fn config_effective_reports_merged_view_machine_readably() {
    let workspace_root = init_git_workspace("config_effective");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-6",
  "permission_mode": "read-only",
  "answer_style": "concise",
  "values": {
    "shared": "project",
    "project_only": true
  }
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini",
  "answer_style": "detailed",
  "values": {
    "shared": "private",
    "local_only": true
  }
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_effective_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "effective",
            "--json",
        ])
        .output()
        .expect("config effective should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "config effective");
    assert_eq!(json["data"]["scope"], "project");
    assert!(json["data"]["project_id"].is_string());
    assert_eq!(json["data"]["effective"]["default_provider"], "anthropic");
    assert_eq!(json["data"]["effective"]["default_model"], "gpt-4.1-mini");
    assert_eq!(json["data"]["effective"]["permission_mode"], "read-only");
    assert_eq!(
        json["data"]["effective"]["values"]["answer_style"],
        "detailed"
    );
    assert_eq!(json["data"]["effective"]["values"]["shared"], "private");
    assert_eq!(json["data"]["effective"]["values"]["project_only"], true);
    assert_eq!(json["data"]["effective"]["values"]["local_only"], true);
}

#[test]
fn config_effective_applies_environment_overrides_machine_readably() {
    let workspace_root = init_git_workspace("config_effective_env");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-6"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_effective_env_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_PROVIDER", "openai")
        .env("RESEARCH_CLI_MODEL", "gpt-5.4-mini")
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "effective",
            "--json",
        ])
        .output()
        .expect("config effective should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["effective"]["default_provider"], "openai");
    assert_eq!(json["data"]["effective"]["default_model"], "gpt-5.4-mini");
}

#[test]
fn config_sources_report_precedence_and_overrides_machine_readably() {
    let workspace_root = init_git_workspace("config_sources");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_model": "claude-sonnet-4-6",
  "answer_style": "concise",
  "values": {
    "shared": "project"
  }
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini",
  "answer_style": "detailed",
  "values": {
    "shared": "private"
  }
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_sources_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "sources",
            "--json",
        ])
        .output()
        .expect("config sources should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "config sources");
    assert_eq!(
        json["data"]["sources"]["project"],
        workspace_root
            .join(".pmcli/settings.json")
            .display()
            .to_string()
    );
    assert_eq!(
        json["data"]["sources"]["private"],
        workspace_root
            .join(".pmcli/settings.local.json")
            .display()
            .to_string()
    );
    let overridden = json["data"]["overridden_key"]
        .as_array()
        .expect("overridden keys");
    assert!(overridden.iter().any(|key| key == "default_model"));
    assert!(overridden.iter().any(|key| key == "values.answer_style"));
    assert!(overridden.iter().any(|key| key == "values.shared"));
}

#[test]
fn config_sources_reports_cross_gap_override_without_middle_layer() {
    let workspace_root = init_git_workspace("config_sources_cross_gap");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("config_sources_cross_gap_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        global_root.join("settings.json"),
        r#"{
  "default_provider": "openai"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_provider": "anthropic"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_sources_cross_gap_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "sources",
            "--json",
        ])
        .output()
        .expect("config sources should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    let overridden = json["data"]["overridden_key"]
        .as_array()
        .expect("overridden keys");
    assert!(overridden.iter().any(|key| key == "default_provider"));
}

#[test]
fn config_get_reports_effective_value_source_and_scope_machine_readably() {
    let workspace_root = init_git_workspace("config_get");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("config_get_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        global_root.join("settings.json"),
        r#"{
  "default_model": "gpt-5.4"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.json"),
        r#"{
  "default_model": "gpt-5.4-mini"
}"#,
    );
    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_get_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "get",
            "default_model",
            "--json",
        ])
        .output()
        .expect("config get should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "config get");
    assert_eq!(json["data"]["key"], "default_model");
    assert_eq!(json["data"]["value"], "gpt-4.1-mini");
    assert_eq!(json["data"]["value_source"], "private");
    assert_eq!(json["data"]["resolved_scope"], "project");
}

#[test]
fn config_get_reports_environment_override_source_machine_readably() {
    let workspace_root = init_git_workspace("config_get_env_override");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("config_get_env_override_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    crate::support::write_text_file(
        workspace_root.join(".pmcli/settings.local.json"),
        r#"{
  "default_model": "gpt-4.1-mini"
}"#,
    );

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_get_env_override_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .env("RESEARCH_CLI_MODEL", "gpt-5.4-mini")
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "get",
            "default_model",
            "--json",
        ])
        .output()
        .expect("config get should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["value"], "gpt-5.4-mini");
    assert_eq!(json["data"]["value_source"], "environment");
}

#[test]
fn config_get_unknown_key_returns_typed_failure() {
    let workspace_root = init_git_workspace("config_get_unknown");
    let registry_root = registry_root_env();

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
        .args(["config", "get", "missing.key", "--json"])
        .output()
        .expect("config get should execute");

    assert_eq!(output.status.code(), Some(6));
    let json = output_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "config get");
    assert_eq!(json["error"]["code"], "config_key_not_found");
    assert_eq!(json["data"]["key"], "missing.key");
}

#[test]
fn config_set_requires_scope_and_mutates_exactly_one_file() {
    let workspace_root = init_git_workspace("config_set");
    let registry_root = registry_root_env();
    let global_root = unique_temp_dir("config_set_global");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let missing_scope = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args(["config", "set", "default_model", "gpt-5.4", "--json"])
        .output()
        .expect("config set should execute");
    assert_eq!(missing_scope.status.code(), Some(2));
    let missing_scope_json = output_json(&missing_scope);
    assert_eq!(missing_scope_json["error"]["code"], "usage_invalid");

    let output = Command::new(cargo_bin())
        .current_dir(unique_temp_dir("config_set_runner"))
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("RESEARCH_CLI_CONFIG_HOME", &global_root)
        .args([
            "--cwd",
            workspace_root.to_str().expect("workspace path"),
            "config",
            "set",
            "answer_style",
            "\"detailed\"",
            "--scope",
            "private",
            "--json",
        ])
        .output()
        .expect("config set should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "config set");
    assert_eq!(json["data"]["key"], "answer_style");
    assert_eq!(json["data"]["value"], "detailed");
    assert_eq!(json["data"]["value_source"], "private");
    assert_eq!(json["data"]["resolved_scope"], "project");

    assert!(!global_root.join("settings.json").exists());
    assert!(!workspace_root.join(".pmcli/settings.json").exists());
    let private_file = workspace_root.join(".pmcli/settings.local.json");
    assert_eq!(
        json["data"]["config_path"],
        private_file.display().to_string()
    );
    assert_eq!(
        json["data"]["workspace_root"],
        workspace_root.display().to_string()
    );
    assert_eq!(json["data"]["resolution_source"], "explicit_cwd");
    let private_json: Value = serde_json::from_str(
        &std::fs::read_to_string(&private_file).expect("private settings should exist"),
    )
    .expect("private settings should parse");
    assert_eq!(private_json["answer_style"], "detailed");

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "start"
            && event["object_id"] == "config set"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "command"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
            && event["object_id"] == "config set"
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "config"
            && event["phase"] == "terminal"
            && event["payload"]["key"] == "answer_style"
            && event["payload"]["scope"] == "private"
            && event["payload"]["config_path"] == private_file.display().to_string()
    }));
}

#[test]
fn projects_init_publishes_checkpoint_and_project_event() {
    let workspace_root = init_git_workspace("projects_init_checkpoint");
    let registry_root = registry_root_env();

    let output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .output()
        .expect("projects init should execute");
    assert!(output.status.success());
    let json = output_json(&output);

    let checkpoint_path = workspace_root.join(".pmcli/project_state.json");
    let checkpoint: Value = serde_json::from_str(
        &std::fs::read_to_string(&checkpoint_path).expect("checkpoint should exist"),
    )
    .expect("checkpoint should parse");
    assert_eq!(
        checkpoint["project_state"]["project_id"],
        json["data"]["project_id"]
    );
    assert!(
        checkpoint["checkpoint_epoch"].as_u64().unwrap_or(0) >= 1,
        "checkpoint epoch should advance"
    );

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "project"
            && event["phase"] == "terminal"
            && event["object_id"] == "projects init"
    }));
}

#[test]
fn prompt_publishes_checkpoint_and_canonical_turn_event() {
    let workspace_root = init_git_workspace("prompt_checkpoint");
    let registry_root = registry_root_env();

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
        .env("OPENAI_API_KEY", "sk-test")
        .args(["prompt", "status", "--json"])
        .output()
        .expect("prompt should execute");
    assert!(output.status.success());
    let json = output_json(&output);
    let turn_id = json["data"]["turn_id"].as_str().expect("turn id");

    let checkpoint_path = workspace_root.join(".pmcli/project_state.json");
    let checkpoint: Value = serde_json::from_str(
        &std::fs::read_to_string(&checkpoint_path).expect("checkpoint should exist"),
    )
    .expect("checkpoint should parse");
    assert_eq!(checkpoint["turn_state"]["turn_id"], turn_id);
    assert!(
        checkpoint["checkpoint_epoch"].as_u64().unwrap_or(0) >= 2,
        "checkpoint epoch should advance after prompt"
    );

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "turn" && event["phase"] == "start" && event["object_id"] == turn_id
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "turn"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "succeeded"
            && event["object_id"] == turn_id
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "provider_resolution"
            && event["phase"] == "terminal"
            && event["object_id"] == turn_id
    }));
}

#[test]
fn permissions_mode_reads_and_writes_typed_permission_mode() {
    let workspace_root = init_git_workspace("permissions_mode");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let read_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["permissions", "mode", "--json"])
        .output()
        .expect("permissions mode should execute");
    assert!(read_output.status.success());
    let read_json = output_json(&read_output);
    assert_eq!(read_json["data"]["permission_mode"], "read-only");

    let write_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "permissions",
            "mode",
            "workspace-write",
            "--scope",
            "private",
            "--json",
        ])
        .output()
        .expect("permissions mode write should execute");
    assert!(write_output.status.success());

    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root.join(".pmcli/settings.local.json"))
            .expect("private settings should exist"),
    )
    .expect("settings should parse");
    assert_eq!(settings["permission_mode"], "workspace-write");

    let events_path = workspace_root.join(".pmcli/events/events.jsonl");
    let events = read_json_lines(events_path.to_str().expect("events path"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "permission_mode"
            && event["phase"] == "terminal"
            && event["payload"]["permission_mode"] == "workspace-write"
    }));
}

#[test]
fn m9_branch_search_creates_evolutionary_batch_with_lineage() {
    let workspace_root = init_real_git_workspace("m9_branch_search");
    let registry_root = registry_root_env();

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
        .args([
            "branches",
            "search",
            "--objective",
            "improve terminal cli parity",
            "--max-branches",
            "3",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = output_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "branches search");
    assert_eq!(json["data"]["schema_version"], "search_batch_record.v1");
    assert_eq!(json["data"]["strategy"], "evolutionary");
    assert_eq!(json["data"]["objective"], "improve terminal cli parity");
    assert_eq!(json["data"]["max_branches"], 3);
    assert_eq!(json["data"]["branch_runs"].as_array().unwrap().len(), 3);
    assert_eq!(
        json["data"]["branch_runs"][0]["variation_operator"],
        "mutate_patch"
    );
    assert_eq!(
        json["data"]["branch_runs"][1]["variation_operator"],
        "repair_failure"
    );
    assert_eq!(
        json["data"]["branch_runs"][2]["variation_operator"],
        "expand_tests"
    );
    assert!(json["data"]["lineage_path"]
        .as_str()
        .unwrap()
        .contains(".pmcli/branches/lineage.jsonl"));
    assert!(workspace_root
        .join(".pmcli/branches/lineage.jsonl")
        .exists());

    for branch in json["data"]["branch_runs"].as_array().unwrap() {
        let branch_id = branch["branch_id"].as_str().unwrap();
        let worktree_path = Path::new(branch["worktree_path"].as_str().unwrap());
        assert!(
            worktree_path.join(".git").exists(),
            "branch {branch_id} must be a real git worktree"
        );
        let rev_parse = Command::new("git")
            .current_dir(worktree_path)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .expect("git rev-parse should execute in branch worktree");
        assert!(
            rev_parse.status.success(),
            "worktree git metadata must be usable: {}",
            String::from_utf8_lossy(&rev_parse.stderr)
        );
        assert!(
            worktree_path
                .join(".pmcli_candidate")
                .join(format!("{branch_id}.md"))
                .exists(),
            "variation operator should leave an executable candidate artifact"
        );
    }
}

#[test]
fn research_runtime_tracks_thread_stage_deliberation_decision_and_record() {
    let workspace_root = init_real_git_workspace("research_runtime");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let empty_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute");
    assert!(empty_status_output.status.success());
    let empty_status = output_json(&empty_status_output);
    assert_eq!(empty_status["command"], "research status");
    assert_eq!(empty_status["data"]["active_thread"], Value::Null);
    assert_eq!(
        empty_status["data"]["active_deliberation_span"],
        Value::Null
    );

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "design",
            "--title",
            "memory aware branch search",
            "--stage",
            "refine",
            "--mode",
            "comparing",
            "--decision",
            "Use M9 only when candidate implementations diverge.",
            "--open-question",
            "Which retrieval backend should gate experiment plans?",
            "--evidence-ref",
            "docs/deep_study/15-research-review-and-contracts.md",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );
    let record_json = output_json(&record_output);
    assert_eq!(record_json["command"], "research record");
    assert_eq!(record_json["data"]["record_kind"], "design");
    let thread_id = record_json["data"]["thread"]["thread_id"]
        .as_str()
        .expect("thread id")
        .to_string();
    let stage_id = record_json["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage execution id")
        .to_string();
    let span_id = record_json["data"]["deliberation_span"]["span_id"]
        .as_str()
        .expect("span id")
        .to_string();
    assert_eq!(
        record_json["data"]["deliberation_span"]["agreed_decisions"][0],
        "Use M9 only when candidate implementations diverge."
    );
    assert!(workspace_root
        .join(".pmcli/research/threads")
        .join(format!("{thread_id}.json"))
        .exists());

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(
        status_json["data"]["active_thread"]["thread_id"],
        thread_id.as_str()
    );
    assert_eq!(
        status_json["data"]["active_stage_execution"]["stage_id"],
        "refine"
    );
    assert_eq!(
        status_json["data"]["active_deliberation_span"]["mode"],
        "ready_to_record"
    );

    let decide_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "decide",
            "--thread",
            &thread_id,
            "--operation",
            "fork",
            "--decision",
            "approve",
            "--reason",
            "There are multiple plausible implementation repairs.",
            "--json",
        ])
        .output()
        .expect("research decide should execute");
    assert!(decide_output.status.success());
    let decide_json = output_json(&decide_output);
    assert_eq!(decide_json["data"]["operation"], "fork");
    assert_eq!(decide_json["data"]["decision"], "approve");
    assert_eq!(decide_json["data"]["thread_id"], thread_id);

    let fork_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute");
    assert!(fork_status_output.status.success());
    let fork_status_json = output_json(&fork_status_output);
    assert_eq!(
        fork_status_json["data"]["active_stage_execution"]["operation"],
        "fork"
    );
    let fork_stage_id = fork_status_json["data"]["active_stage_execution"]["execution_id"]
        .as_str()
        .expect("fork stage id")
        .to_string();
    let fork_span_id = fork_status_json["data"]["active_deliberation_span"]["span_id"]
        .as_str()
        .expect("fork span id")
        .to_string();
    let m9_batch_id = fork_status_json["data"]["active_stage_execution"]["m9_batch_id"]
        .as_str()
        .expect("fork should bind M9 batch")
        .to_string();
    assert!(workspace_root
        .join(".pmcli/branches/batches")
        .join(format!("{m9_batch_id}.json"))
        .exists());
    assert!(
        fork_status_json["data"]["active_stage_execution"]["output_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap_or("").contains("debate_packets"))
    );
    assert!(
        fork_status_json["data"]["active_stage_execution"]["output_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap_or("").contains("debate_traces"))
    );

    let stage_map_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "stage", "map", "--json"])
        .output()
        .expect("research stage map should execute");
    assert!(stage_map_output.status.success());
    let stage_map_json = output_json(&stage_map_output);
    assert_eq!(stage_map_json["command"], "research stage map");
    assert!(stage_map_json["data"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["stage_id"] == "refine"
            && entry["allowed_operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation == "pivot")));
    let decisions_dir = workspace_root.join(".pmcli/research/decisions");
    let batches_dir = workspace_root.join(".pmcli/branches/batches");
    let decisions_before = std::fs::read_dir(&decisions_dir)
        .expect("decisions dir should read")
        .count();
    let batches_before = std::fs::read_dir(&batches_dir)
        .expect("batches dir should read")
        .count();

    let invalid_stage_operation_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "decide",
            "--thread",
            &thread_id,
            "--operation",
            "human_override",
            "--decision",
            "approve",
            "--json",
        ])
        .output()
        .expect("research decide invalid stage operation should execute");
    assert!(!invalid_stage_operation_output.status.success());
    let invalid_stage_operation_json = output_json(&invalid_stage_operation_output);
    assert_eq!(invalid_stage_operation_json["ok"], false);
    assert_eq!(
        invalid_stage_operation_json["error"]["code"],
        "usage_invalid"
    );
    assert!(invalid_stage_operation_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("StageExecutionMap"));
    assert_eq!(
        std::fs::read_dir(&decisions_dir)
            .expect("decisions dir should read")
            .count(),
        decisions_before
    );
    assert_eq!(
        std::fs::read_dir(&batches_dir)
            .expect("batches dir should read")
            .count(),
        batches_before
    );

    let classify_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "classify",
            "--thread",
            &thread_id,
            "--text",
            "The current experiment result is bad; this is not just an implementation bug, we may need to change the plan.",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("research classify should execute");
    assert!(classify_output.status.success());
    let classify_json = output_json(&classify_output);
    assert_eq!(classify_json["command"], "research classify");
    assert_eq!(classify_json["data"]["research_relevant"], true);
    assert_eq!(classify_json["data"]["thread_id"], thread_id);
    assert_eq!(classify_json["data"]["candidate_operation"], "pivot");
    assert_eq!(classify_json["data"]["human_gate_required"], true);
    assert_eq!(classify_json["data"]["dry_run"], true);
    assert!(classify_json["data"]["alternative_interpretations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "repair"));

    let thread_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "thread", "inspect", &thread_id, "--json"])
        .output()
        .expect("research thread inspect should execute");
    assert!(thread_output.status.success());
    let thread_json = output_json(&thread_output);
    assert_eq!(thread_json["data"]["thread"]["thread_id"], thread_id);
    assert!(thread_json["data"]["canonical_artifact_refs"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(thread_json["data"]["deliberation_spans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|span| span["span_id"] == span_id));
    assert!(thread_json["data"]["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|decision| decision["operation"] == "fork"));

    let stage_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "stage", "inspect", &stage_id, "--json"])
        .output()
        .expect("research stage inspect should execute");
    assert!(stage_output.status.success());
    let stage_json = output_json(&stage_output);
    assert_eq!(
        stage_json["data"]["stage_execution"]["execution_id"],
        stage_id
    );
    assert_eq!(
        stage_json["data"]["deliberation_spans"][0]["span_id"],
        span_id
    );

    let prompt_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .env("OPENAI_API_KEY", "sk-test")
        .args([
            "prompt",
            "The result is not enough, maybe we need to fork competing repair plans before changing the final design.",
            "--json",
        ])
        .output()
        .expect("prompt should execute");
    assert!(
        prompt_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&prompt_output.stderr)
    );
    let prompt_json = output_json(&prompt_output);
    assert_eq!(
        prompt_json["data"]["research_context"]["active_thread_id"],
        thread_id
    );
    assert_eq!(
        prompt_json["data"]["research_context"]["active_stage_execution_id"],
        fork_stage_id
    );
    assert_eq!(
        prompt_json["data"]["research_context"]["active_deliberation_span_id"],
        fork_span_id
    );
    assert_eq!(
        prompt_json["data"]["research_context"]["projection_policy"],
        "bounded_after_mission_frame_before_memory"
    );
    assert_eq!(
        prompt_json["data"]["research_classification"]["candidate_operation"],
        "fork"
    );
    assert_eq!(
        prompt_json["data"]["research_classification"]["human_gate_required"],
        true
    );
    assert!(
        prompt_json["data"]["research_context"]["pending_operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "fork")
    );
    let session_id = prompt_json["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let updated_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute");
    assert!(updated_status_output.status.success());
    let updated_status_json = output_json(&updated_status_output);
    assert!(
        updated_status_json["data"]["active_deliberation_span"]["turn_refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap_or("").starts_with("turn_"))
    );
    assert!(updated_status_json["data"]["pending_operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "fork"));
    assert_eq!(
        updated_status_json["data"]["next_recommended_action"],
        "confirm_in_natural_language_before_creating_operation_edge"
    );

    let resume_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["resume", &session_id, "--json"])
        .output()
        .expect("resume should execute");
    assert!(resume_output.status.success());
    let resume_json = output_json(&resume_output);
    assert_eq!(
        resume_json["data"]["research_context"]["active_thread_id"],
        thread_id
    );

    let continue_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["continue", "--json"])
        .output()
        .expect("continue should execute");
    assert!(continue_output.status.success());
    let continue_json = output_json(&continue_output);
    assert_eq!(
        continue_json["data"]["research_context"]["active_deliberation_span_id"],
        fork_span_id
    );

    let compact_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["compact", "latest", "--json"])
        .output()
        .expect("compact should execute");
    assert!(compact_output.status.success());
    let compact_json = output_json(&compact_output);
    assert_eq!(
        compact_json["data"]["research_context"]["active_stage_execution_id"],
        fork_stage_id
    );
    assert!(compact_json["data"]["derived_views_updated"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "research_context_projection"));
    let summary_ref = compact_json["data"]["summary_ref"]
        .as_str()
        .expect("summary ref should be present");
    let summary_path = workspace_root
        .join(".pmcli")
        .join("sessions")
        .join(&session_id)
        .join("summaries")
        .join(format!("{summary_ref}.md"));
    let summary_contents =
        std::fs::read_to_string(&summary_path).expect("summary artifact should be readable");
    assert!(summary_contents.contains("## ResearchContext Projection"));
    assert!(summary_contents.contains(&format!("- active_thread_id: {thread_id}")));

    let supersede_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "decide",
            "--thread",
            &thread_id,
            "--operation",
            "supersede",
            "--decision",
            "approve",
            "--reason",
            "The fork discussion replaces the previous public design note.",
            "--evidence-ref",
            "docs/deep_study/33-advanced-systems-implementation-plan.md",
            "--json",
        ])
        .output()
        .expect("research supersede decide should execute");
    assert!(supersede_output.status.success());

    let supersede_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status after supersede should execute");
    assert!(supersede_status_output.status.success());
    let supersede_status_json = output_json(&supersede_status_output);
    let supersede_stage = &supersede_status_json["data"]["active_stage_execution"];
    assert_eq!(supersede_stage["operation"], "supersede");
    assert!(supersede_stage["change_envelope_ref"].is_string());
    assert!(supersede_stage["supersedes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().unwrap_or("").starts_with("stage_")));
    assert!(supersede_stage["output_artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().unwrap_or("").contains("canonical_surface")));
    let envelope_ref = supersede_stage["change_envelope_ref"]
        .as_str()
        .expect("supersede should create a change envelope");
    assert!(workspace_root.join(envelope_ref).exists());
    let canonical_manifest = serde_json::from_str::<Value>(
        &std::fs::read_to_string(workspace_root.join(".pmcli/canonical_surface.json"))
            .expect("supersede should refresh canonical surface manifest"),
    )
    .expect("canonical surface manifest should parse");
    assert_eq!(canonical_manifest["source"], "research_supersede");
    assert!(canonical_manifest["superseded_paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().unwrap_or("").starts_with("stage_")));
    let family_manifest = serde_json::from_str::<Value>(
        &std::fs::read_to_string(
            workspace_root.join(".pmcli/artifacts/families/research_stage_outputs.json"),
        )
        .expect("supersede should update research artifact family manifest"),
    )
    .expect("research artifact family manifest should parse");
    assert_eq!(
        family_manifest["latest_id"],
        supersede_stage["execution_id"].as_str().unwrap()
    );
    assert!(family_manifest["supersession_chain"]
        .as_array()
        .unwrap()
        .iter()
        .any(|link| link["reason"] == "research_supersede"));
    assert!(workspace_root
        .join("docs/research/runtime/latest.md")
        .exists());

    let abandon_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "decide",
            "--thread",
            &thread_id,
            "--operation",
            "abandon",
            "--decision",
            "approve",
            "--reason",
            "The current branch no longer supports the claim.",
            "--evidence-ref",
            "failed_result.json",
            "--json",
        ])
        .output()
        .expect("research abandon decide should execute");
    assert!(abandon_output.status.success());

    let abandon_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status after abandon should execute");
    assert!(abandon_status_output.status.success());
    let abandon_status_json = output_json(&abandon_status_output);
    assert_eq!(
        abandon_status_json["data"]["active_thread"]["status"],
        "abandoned"
    );
    assert_eq!(
        abandon_status_json["data"]["active_stage_execution"]["operation"],
        "abandon"
    );
    assert!(
        abandon_status_json["data"]["active_stage_execution"]["change_envelope_ref"].is_string()
    );
    assert!(
        abandon_status_json["data"]["active_stage_execution"]["output_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap_or("").contains("failure_lessons"))
    );
}

#[test]
fn research_decide_advance_follows_stage_execution_map_edges() {
    let workspace_root = init_git_workspace("research_advance_stage_map");
    let registry_root = registry_root_env();

    run_cli_json(
        &workspace_root,
        &registry_root,
        &["projects", "init", "--json"],
    );

    let record_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "research",
            "record",
            "--kind",
            "paper",
            "--title",
            "stage map advance regression",
            "--stage",
            "idea",
            "--mode",
            "ready_to_execute",
            "--json",
        ],
    );
    let thread_id = record_json["data"]["thread"]["thread_id"]
        .as_str()
        .expect("thread id");

    for expected_stage in [
        "refine",
        "experiment-plan",
        "implement-solution",
        "run",
        "monitor",
        "result-to-claim",
        "paper-plan",
        "paper-write",
        "paper-compile",
        "research-review",
        "rebuttal",
        "meta-optimize",
        "refine",
    ] {
        run_cli_json(
            &workspace_root,
            &registry_root,
            &[
                "research",
                "decide",
                "--thread",
                thread_id,
                "--operation",
                "advance",
                "--decision",
                "approve",
                "--reason",
                "advance along the declared StageExecutionMap",
                "--json",
            ],
        );
        let status_json = run_cli_json(
            &workspace_root,
            &registry_root,
            &["research", "status", "--json"],
        );
        assert_eq!(
            status_json["data"]["active_stage_execution"]["stage_id"],
            expected_stage
        );
    }

    let literature_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "research",
            "record",
            "--kind",
            "paper",
            "--title",
            "literature entry stage",
            "--stage",
            "literature",
            "--mode",
            "ready_to_execute",
            "--json",
        ],
    );
    let literature_thread_id = literature_json["data"]["thread"]["thread_id"]
        .as_str()
        .expect("literature thread id");
    run_cli_json(
        &workspace_root,
        &registry_root,
        &[
            "research",
            "decide",
            "--thread",
            literature_thread_id,
            "--operation",
            "advance",
            "--decision",
            "approve",
            "--reason",
            "advance literature along the declared StageExecutionMap",
            "--json",
        ],
    );
    let literature_status_json = run_cli_json(
        &workspace_root,
        &registry_root,
        &["research", "status", "--json"],
    );
    assert_eq!(
        literature_status_json["data"]["active_stage_execution"]["stage_id"],
        "novelty"
    );
}

#[test]
fn m13_research_middleware_records_role_tool_reflection_and_hitl_gate() {
    let workspace_root = init_real_git_workspace("m13_research_middleware");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "plan",
            "--title",
            "benchmark repair strategy",
            "--stage",
            "experiment-plan",
            "--mode",
            "comparing",
            "--open-question",
            "Should we run the heavy benchmark now?",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(record_output.status.success());
    let record_json = output_json(&record_output);
    let thread_id = record_json["data"]["thread"]["thread_id"]
        .as_str()
        .expect("thread id")
        .to_string();
    let stage_id = record_json["data"]["stage_execution"]["execution_id"]
        .as_str()
        .expect("stage id")
        .to_string();

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
            &thread_id,
            "--stage-execution",
            &stage_id,
            "--intent",
            "Reflect on a failed result and decide whether to launch a heavy benchmark rerun.",
            "--candidate-tool",
            "think_tool",
            "--candidate-tool",
            "search",
            "--candidate-tool",
            "search_backup",
            "--candidate-tool",
            "mcp.search.paper",
            "--candidate-tool",
            "shell",
            "--candidate-tool",
            "write_file",
            "--tool-error",
            "shell:timeout",
            "--json",
        ])
        .output()
        .expect("research middleware plan should execute");
    assert!(
        middleware_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&middleware_output.stderr)
    );
    let middleware_json = output_json(&middleware_output);
    assert_eq!(middleware_json["command"], "research middleware plan");
    assert_eq!(
        middleware_json["data"]["schema_version"],
        "research_ux_middleware_result.v1"
    );
    assert_eq!(
        middleware_json["data"]["role_template"]["schema_version"],
        "research_role_template.v1"
    );
    assert_eq!(
        middleware_json["data"]["role_template"]["role_id"],
        "planner"
    );
    assert_eq!(
        middleware_json["data"]["tool_selection_trace"]["schema_version"],
        "tool_selection_trace.v1"
    );
    assert!(
        middleware_json["data"]["tool_selection_trace"]["selected_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "think_tool")
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
            .any(|tool| tool == "shell")
    );
    assert!(
        middleware_json["data"]["tool_selection_trace"]["filtered_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "search_backup")
    );
    assert_eq!(
        middleware_json["data"]["stage_reflection_packet"]["schema_version"],
        "stage_reflection_packet.v1"
    );
    assert!(
        middleware_json["data"]["stage_reflection_packet"]["unmet_success_signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|signal| signal == "failed_result_requires_plan_repair_or_rerun_gate")
    );
    assert_eq!(
        middleware_json["data"]["hitl_pending_gate"]["kind"],
        "approval"
    );
    assert_eq!(middleware_json["data"]["human_gate_required"], true);
    assert_eq!(
        middleware_json["data"]["tool_error_recovery"]["failed_tool"],
        "shell"
    );
    assert_eq!(
        middleware_json["data"]["tool_error_recovery"]["error_class"],
        "timeout"
    );
    let gate_id = middleware_json["data"]["hitl_pending_gate"]["gate_id"]
        .as_str()
        .expect("gate id")
        .to_string();
    assert!(workspace_root
        .join(".pmcli/research/middleware/results")
        .join(format!(
            "{}.json",
            middleware_json["data"]["middleware_id"].as_str().unwrap()
        ))
        .exists());

    let status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status should execute");
    assert!(status_output.status.success());
    let status_json = output_json(&status_output);
    assert_eq!(
        status_json["data"]["latest_middleware_packet"],
        middleware_json["data"]["middleware_id"]
    );
    assert!(status_json["data"]["middleware_pending_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate["gate_id"] == gate_id));

    let second_record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "record",
            "--kind",
            "plan",
            "--title",
            "separate active thread",
            "--stage",
            "refine",
            "--json",
        ])
        .output()
        .expect("second research record should execute");
    assert!(second_record_output.status.success());
    let second_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status for second thread should execute");
    assert!(second_status_output.status.success());
    let second_status_json = output_json(&second_status_output);
    assert_eq!(
        second_status_json["data"]["latest_middleware_packet"],
        Value::Null
    );
    assert!(second_status_json["data"]["middleware_pending_gates"]
        .as_array()
        .unwrap()
        .is_empty());

    let second_middleware_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "middleware",
            "plan",
            "--role",
            "planner",
            "--intent",
            "Reflect on the current active thread without a heavy action.",
            "--candidate-tool",
            "think_tool",
            "--json",
        ])
        .output()
        .expect("second thread middleware plan should execute");
    assert!(second_middleware_output.status.success());
    let second_middleware_json = output_json(&second_middleware_output);

    let first_thread_middleware_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "research",
            "middleware",
            "plan",
            "--role",
            "planner",
            "--thread",
            &thread_id,
            "--stage-execution",
            &stage_id,
            "--intent",
            "Refresh the original thread projection.",
            "--candidate-tool",
            "think_tool",
            "--json",
        ])
        .output()
        .expect("first thread middleware refresh should execute");
    assert!(first_thread_middleware_output.status.success());

    let final_active_status_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["research", "status", "--json"])
        .output()
        .expect("research status after first thread refresh should execute");
    assert!(final_active_status_output.status.success());
    let final_active_status_json = output_json(&final_active_status_output);
    assert_eq!(
        final_active_status_json["data"]["latest_middleware_packet"],
        second_middleware_json["data"]["middleware_id"]
    );
}

#[test]
fn m9_branch_promotion_requires_evaluation_and_debate_gates() {
    let workspace_root = init_real_git_workspace("m9_promote_gates");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "prove gated promotion",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let branch_id = output_json(&search)["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &branch_id, "--json"])
        .output()
        .expect("branches promote should execute");

    assert!(!promote.status.success());
    let json = output_json(&promote);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "branch_promotion_blocked");
    assert!(json["data"]["missing_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate == "evaluation_packet"));
    assert!(json["data"]["missing_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate == "debate_trace"));
}

#[test]
fn m12_verifier_tournament_scores_branch_pairs_and_can_gate_promotion() {
    let workspace_root = init_real_git_workspace("m12_verifier_tournament");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "choose verifier-backed branch winner",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let left = search_json["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();
    let right = search_json["data"]["branch_runs"][1]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    let premature_verify = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "verify", &left, "--against", &right, "--json"])
        .output()
        .expect("premature branches verify should execute");
    assert!(!premature_verify.status.success());
    let premature_verify_json = output_json(&premature_verify);
    assert_eq!(premature_verify_json["ok"], false);
    assert!(premature_verify_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("evaluation_packet"));

    let left_eval = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "evaluate",
            &left,
            "--eval-command",
            "true",
            "--json",
        ])
        .output()
        .expect("left evaluate should execute");
    assert!(left_eval.status.success());

    let right_eval = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "evaluate",
            &right,
            "--eval-command",
            "false",
            "--json",
        ])
        .output()
        .expect("right evaluate should execute");
    assert!(right_eval.status.success());

    let debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &left, "--against", &right, "--json"])
        .output()
        .expect("branches debate should execute");
    assert!(debate.status.success());

    let blocked_without_verifier = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "promote",
            &left,
            "--require-verifier",
            "--no-merge",
            "--json",
        ])
        .output()
        .expect("branches promote should execute");
    assert!(!blocked_without_verifier.status.success());
    assert!(
        output_json(&blocked_without_verifier)["data"]["missing_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "verifier_tournament")
    );

    let verify = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "verify",
            &left,
            "--against",
            &right,
            "--repetitions",
            "2",
            "--json",
        ])
        .output()
        .expect("branches verify should execute");
    assert!(
        verify.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let verify_json = output_json(&verify);
    assert_eq!(verify_json["command"], "branches verify");
    assert_eq!(
        verify_json["data"]["schema_version"],
        "verifier_tournament.v1"
    );
    assert_eq!(verify_json["data"]["scope"], "branch_promotion");
    assert_eq!(verify_json["data"]["repetitions"], 2);
    assert_eq!(verify_json["data"]["winner_ref"], left.as_str());
    assert_eq!(
        verify_json["data"]["recommendation"],
        "recommend_candidate_a"
    );
    assert_eq!(
        verify_json["data"]["deterministic_gate_result_ref"],
        output_json(&left_eval)["data"]["evaluation_id"]
    );
    assert!(verify_json["data"]["criteria"].as_array().unwrap().len() >= 3);
    assert_eq!(
        verify_json["data"]["pair_scores"].as_array().unwrap().len(),
        verify_json["data"]["criteria"].as_array().unwrap().len() * 2
    );
    assert_eq!(
        verify_json["data"]["pair_scores"][0]["scoring_mode"],
        "deterministic_local_evidence"
    );
    assert!(verify_json["data"]["pair_scores"][0]["evidence_digest"]
        .as_str()
        .unwrap()
        .starts_with("digest_"));

    let inspect = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "inspect", &left, "--json"])
        .output()
        .expect("branches inspect should execute");
    assert!(inspect.status.success());
    let inspect_json = output_json(&inspect);
    assert_eq!(
        inspect_json["data"]["branch_run"]["verifier_tournament_id"],
        verify_json["data"]["tournament_id"]
    );
    assert_eq!(
        inspect_json["data"]["verifier_tournament"]["winner_ref"],
        left.as_str()
    );

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "promote",
            &left,
            "--require-verifier",
            "--no-merge",
            "--json",
        ])
        .output()
        .expect("branches promote should execute");
    assert!(
        promote.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&promote.stderr)
    );
    let promote_json = output_json(&promote);
    assert_eq!(promote_json["data"]["status"], "promoted");
    assert_eq!(promote_json["data"]["verifier_gate_status"], "passed");
}

#[test]
fn m9_evaluation_executes_real_command_and_blocks_failed_candidates() {
    let workspace_root = init_real_git_workspace("m9_eval_real_command");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "reject failed proof",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let left = search_json["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();
    let right = search_json["data"]["branch_runs"][1]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    let evaluate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "evaluate",
            &left,
            "--eval-command",
            "false",
            "--json",
        ])
        .output()
        .expect("branches evaluate should execute");
    assert!(
        evaluate.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&evaluate.stderr)
    );
    let eval_json = output_json(&evaluate);
    assert_eq!(eval_json["data"]["promotion_eligible"], false);
    assert!(eval_json["data"]["blocking_issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue == "test_command:false"));

    let debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &left, "--against", &right, "--json"])
        .output()
        .expect("branches debate should execute");
    assert!(debate.status.success());

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &left, "--no-merge", "--json"])
        .output()
        .expect("branches promote should execute");
    assert!(!promote.status.success());
    let promote_json = output_json(&promote);
    assert_eq!(promote_json["error"]["code"], "branch_promotion_blocked");
    assert!(promote_json["data"]["missing_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate == "test_command:false"));
}

#[test]
fn m9_evaluation_runs_canonicality_gate_before_promotion() {
    let workspace_root = init_real_git_workspace("m9_eval_canonicality");
    write_explicit_canonical_manifests(&workspace_root);
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "respect canonical public surface",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let left = search_json["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();
    let right = search_json["data"]["branch_runs"][1]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    write_text_file(
        workspace_root.join("ambiguous_latest_new_plan.md"),
        "stale public fork\n",
    );

    let evaluate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "evaluate",
            &left,
            "--eval-command",
            "true",
            "--json",
        ])
        .output()
        .expect("branches evaluate should execute");
    assert!(evaluate.status.success());
    let eval_json = output_json(&evaluate);
    assert_eq!(eval_json["data"]["canonicality_gate"]["status"], "blocked");
    assert_eq!(eval_json["data"]["promotion_eligible"], false);

    let debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &left, "--against", &right, "--json"])
        .output()
        .expect("branches debate should execute");
    assert!(debate.status.success());

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &left, "--no-merge", "--json"])
        .output()
        .expect("branches promote should execute");
    assert!(!promote.status.success());
    assert!(output_json(&promote)["data"]["missing_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate == "canonicality_gate"));
}

#[test]
fn m9_evaluate_debate_promote_enforces_single_winner_and_archive() {
    let workspace_root = init_real_git_workspace("m9_full_loop");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "choose best branch",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let left = search_json["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();
    let right = search_json["data"]["branch_runs"][1]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    let evaluate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "evaluate", &left, "--json"])
        .output()
        .expect("branches evaluate should execute");
    assert!(
        evaluate.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&evaluate.stderr)
    );
    let eval_json = output_json(&evaluate);
    assert_eq!(eval_json["data"]["schema_version"], "evaluation_packet.v1");
    assert_eq!(eval_json["data"]["promotion_eligible"], true);

    let debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &left, "--against", &right, "--json"])
        .output()
        .expect("branches debate should execute");
    assert!(
        debate.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&debate.stderr)
    );
    assert_eq!(
        output_json(&debate)["data"]["schema_version"],
        "debate_trace.v1"
    );

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &left, "--json"])
        .output()
        .expect("branches promote should execute");
    assert!(
        promote.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&promote.stderr)
    );
    assert_eq!(output_json(&promote)["data"]["status"], "promoted");

    let second_eval = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "evaluate", &right, "--json"])
        .output()
        .expect("second evaluate should execute");
    assert!(second_eval.status.success());
    let second_debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &right, "--against", &left, "--json"])
        .output()
        .expect("second debate should execute");
    assert!(second_debate.status.success());

    let second_promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &right, "--json"])
        .output()
        .expect("second promote should execute");
    assert!(!second_promote.status.success());
    assert_eq!(
        output_json(&second_promote)["error"]["code"],
        "branch_winner_exists"
    );

    let archive = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "archive",
            &right,
            "--reason",
            "lost promoted comparison",
            "--json",
        ])
        .output()
        .expect("branches archive should execute");
    assert!(
        archive.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&archive.stderr)
    );
    assert_eq!(output_json(&archive)["data"]["status"], "archived");
}

#[test]
fn m9_mutate_runs_llm_command_in_candidate_worktree_and_commits() {
    let workspace_root = init_real_git_workspace("m9_llm_mutate");
    let registry_root = registry_root_env();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "llm edits candidate code",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let branch = &search_json["data"]["branch_runs"][0];
    let branch_id = branch["branch_id"].as_str().unwrap().to_string();
    let worktree_path = Path::new(branch["worktree_path"].as_str().unwrap()).to_path_buf();

    let mutate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "mutate",
            &branch_id,
            "--llm-command",
            "mkdir -p src && printf 'pub const WINNER: &str = \"mutated\";\\n' > src/winner.rs",
            "--json",
        ])
        .output()
        .expect("branches mutate should execute");
    assert!(
        mutate.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&mutate.stderr)
    );
    let mutate_json = output_json(&mutate);
    assert_eq!(
        mutate_json["data"]["schema_version"],
        "branch_mutation_result.v1"
    );
    assert_eq!(mutate_json["data"]["status"], "mutated");
    assert_eq!(
        mutate_json["data"]["mutation_source"],
        "llm_command_adapter"
    );
    assert!(mutate_json["data"]["changed_paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "src/winner.rs"));
    assert!(worktree_path.join("src/winner.rs").exists());

    let log = Command::new("git")
        .current_dir(&worktree_path)
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .expect("git log should execute");
    assert!(String::from_utf8_lossy(&log.stdout).contains("m9 llm mutation"));
}

#[test]
fn m9_promote_auto_merges_winner_public_diff_into_source_worktree() {
    let workspace_root = init_real_git_workspace("m9_auto_merge_winner");
    let registry_root = registry_root_env();
    let initial_head = Command::new("git")
        .current_dir(&workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse should execute");
    assert!(initial_head.status.success());
    let initial_head = String::from_utf8_lossy(&initial_head.stdout)
        .trim()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "merge winning code",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let left = search_json["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();
    let right = search_json["data"]["branch_runs"][1]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    let mutate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "mutate",
            &left,
            "--llm-command",
            "mkdir -p src && printf 'pub const WINNER: &str = \"merged\";\\n' > src/winner.rs",
            "--json",
        ])
        .output()
        .expect("branches mutate should execute");
    assert!(mutate.status.success());

    let evaluate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "evaluate",
            &left,
            "--eval-command",
            "true",
            "--json",
        ])
        .output()
        .expect("branches evaluate should execute");
    assert!(evaluate.status.success());
    assert_eq!(output_json(&evaluate)["data"]["promotion_eligible"], true);

    let debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &left, "--against", &right, "--json"])
        .output()
        .expect("branches debate should execute");
    assert!(debate.status.success());

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &left, "--json"])
        .output()
        .expect("branches promote should execute");
    assert!(
        promote.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&promote.stderr)
    );
    let promote_json = output_json(&promote);
    assert_eq!(promote_json["data"]["status"], "promoted");
    assert_eq!(promote_json["data"]["merge_status"], "merged");
    assert!(promote_json["data"]["merged_paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "src/winner.rs"));
    assert_eq!(
        std::fs::read_to_string(workspace_root.join("src/winner.rs")).unwrap(),
        "pub const WINNER: &str = \"merged\";\n"
    );

    let new_head = Command::new("git")
        .current_dir(&workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse should execute");
    assert!(new_head.status.success());
    assert_ne!(
        String::from_utf8_lossy(&new_head.stdout).trim(),
        initial_head
    );

    let log = Command::new("git")
        .current_dir(&workspace_root)
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .expect("git log should execute");
    assert!(String::from_utf8_lossy(&log.stdout).contains("m9 promote"));
}

#[test]
fn m9_auto_merge_rolls_back_when_winner_breaks_canonicality() {
    let workspace_root = init_real_git_workspace("m9_auto_merge_canonical_rollback");
    write_explicit_canonical_manifests(&workspace_root);
    let registry_root = registry_root_env();

    let initial_head = Command::new("git")
        .current_dir(&workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse should execute");
    assert!(initial_head.status.success());
    let initial_head = String::from_utf8_lossy(&initial_head.stdout)
        .trim()
        .to_string();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let search = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "search",
            "--objective",
            "reject ambiguous winner",
            "--max-branches",
            "2",
            "--strategy",
            "evolutionary",
            "--json",
        ])
        .output()
        .expect("branches search should execute");
    assert!(search.status.success());
    let search_json = output_json(&search);
    let left = search_json["data"]["branch_runs"][0]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();
    let right = search_json["data"]["branch_runs"][1]["branch_id"]
        .as_str()
        .unwrap()
        .to_string();

    let mutate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "mutate",
            &left,
            "--llm-command",
            "cat > ambiguous_a.md <<'EOF'\n---\ndoc_frame:\ndoc_type: status\nlifecycle: active\n---\n# status a\nEOF\ncat > ambiguous_b.md <<'EOF'\n---\ndoc_frame:\ndoc_type: status\nlifecycle: active\n---\n# status b\nEOF\n",
            "--json",
        ])
        .output()
        .expect("branches mutate should execute");
    assert!(mutate.status.success());

    let evaluate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args([
            "branches",
            "evaluate",
            &left,
            "--eval-command",
            "true",
            "--json",
        ])
        .output()
        .expect("branches evaluate should execute");
    assert!(evaluate.status.success());
    assert_eq!(output_json(&evaluate)["data"]["promotion_eligible"], true);

    let debate = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "debate", &left, "--against", &right, "--json"])
        .output()
        .expect("branches debate should execute");
    assert!(debate.status.success());

    let promote = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &registry_root)
        .args(["branches", "promote", &left, "--json"])
        .output()
        .expect("branches promote should execute");
    assert!(!promote.status.success());
    let promote_json = output_json(&promote);
    assert_eq!(promote_json["error"]["code"], "branch_promotion_blocked");
    assert!(promote_json["data"]["missing_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate == "post_merge_canonicality_gate"));
    assert!(!workspace_root.join("ambiguous_a.md").exists());
    assert!(!workspace_root.join("ambiguous_b.md").exists());

    let current_head = Command::new("git")
        .current_dir(&workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse should execute");
    assert!(current_head.status.success());
    assert_eq!(
        String::from_utf8_lossy(&current_head.stdout).trim(),
        initial_head
    );
}
