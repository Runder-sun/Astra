use crate::board_task_refs;
use crate::canonical_artifacts;
use crate::permissions::{ToolClassification, ToolSpec};
use crate::projects::current::ResolvedProject;
use crate::review_refs;
use crate::runtime;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use ureq::ResponseExt;

const MAX_READ_SIZE: u64 = 10 * 1024 * 1024;
const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024;
const MAX_LIST_RESULTS: usize = 200;
const MAX_SEARCH_MATCHES: usize = 200;
const MAX_LLM_TOOL_RESULT_CHARS: usize = 4_000;
const MAX_FETCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_FETCH_REDIRECTS: u32 = 3;
const MAX_PAPER_SEARCH_RESULTS: usize = 20;
const DEFAULT_SHELL_TIMEOUT_MS: u64 = 30_000;
const MAX_SHELL_TIMEOUT_MS: u64 = 10 * 60 * 1_000;
const SKIPPED_LIST_DIRS: &[&str] = &[
    ".git",
    ".claude",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
];

#[derive(Debug, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolSpec>,
}

impl ToolRegistry {
    pub fn new(tools: Vec<ToolSpec>) -> Self {
        Self {
            tools: tools
                .into_iter()
                .map(|tool| (tool.name.clone(), tool))
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&ToolSpec> {
        self.tools.values().collect()
    }
}

pub fn builtin_registry() -> ToolRegistry {
    ToolRegistry::new(vec![
        ToolSpec::new("read_file", ToolClassification::ReadOnly),
        ToolSpec::new("list_files", ToolClassification::ReadOnly),
        ToolSpec::new("search", ToolClassification::ReadOnly),
        ToolSpec::new("fetch", ToolClassification::ReadOnly),
        ToolSpec::new("paper_search", ToolClassification::ReadOnly),
        ToolSpec::new("write_file", ToolClassification::Mutating),
        ToolSpec::new("apply_patch", ToolClassification::Mutating),
        ToolSpec::new("worker_shell", ToolClassification::Mutating),
        ToolSpec::new("publish_board_tasks", ToolClassification::Mutating),
        ToolSpec::new("update_board_task", ToolClassification::Mutating),
        ToolSpec::new("merge_board_tasks", ToolClassification::Mutating),
        ToolSpec::new("record_obligation_decision", ToolClassification::Mutating),
        ToolSpec::new("record_stage_evidence_plan", ToolClassification::Mutating),
        ToolSpec::new(
            "record_stage_closure_decision",
            ToolClassification::Mutating,
        ),
        ToolSpec::new(
            "record_worker_artifact_decision",
            ToolClassification::Mutating,
        ),
        ToolSpec::new("adopt_stage_artifact", ToolClassification::Mutating),
        ToolSpec::new(
            "record_canonical_artifact_integration_check",
            ToolClassification::Mutating,
        ),
        ToolSpec::new("request_review_rerun", ToolClassification::Mutating),
        ToolSpec::new("request_route_change", ToolClassification::Mutating),
        ToolSpec::new("request_cleanup_plan", ToolClassification::Mutating),
        ToolSpec::new("delete_file", ToolClassification::Destructive),
        ToolSpec::new("shell", ToolClassification::Destructive),
    ])
}

/// OpenAI function-calling tool definition for the LLM API.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Return the builtin tool definitions in OpenAI function-calling format.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a text file. Returns the file content as a string. For long files, pass offset and max_chars to read the file in character ranges.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to the file from the workspace root" },
                    "offset": { "type": "integer", "description": "Optional zero-based character offset to start reading from" },
                    "max_chars": { "type": "integer", "description": "Optional maximum number of characters to return from offset" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "list_files".to_string(),
            description: "List files recursively in a directory. Returns one relative path per line.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative directory path (defaults to workspace root)" }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "search".to_string(),
            description: "Search for a text pattern across files in the workspace. Returns matching lines with file paths and line numbers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative directory or file path to search in (defaults to workspace root)" },
                    "pattern": { "type": "string", "description": "Substring pattern to search for" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "fetch".to_string(),
            description: "Fetch content from a URL via HTTP GET. Returns the response body as text.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "HTTP or HTTPS URL to fetch" },
                    "allow_private_network": { "type": "boolean", "description": "Allow fetching private/local network addresses (default: false)" }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "paper_search".to_string(),
            description: "Search public literature indexes and return structured paper candidates. This is a read-only retrieval tool: it does not decide which papers satisfy a research task.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Paper search query" },
                    "source": {
                        "type": "string",
                        "enum": ["auto", "arxiv", "semantic_scholar", "crossref"],
                        "description": "Literature index to query. auto tries arxiv first, then semantic_scholar."
                    },
                    "max_results": { "type": "integer", "description": "Maximum number of candidates to return, capped at 20" },
                    "year_from": { "type": "integer", "description": "Optional inclusive lower year bound" },
                    "year_to": { "type": "integer", "description": "Optional inclusive upper year bound" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file, creating it if it does not exist. Overwrites existing content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to the file from the workspace root" },
                    "content": { "type": "string", "description": "Content to write to the file" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "apply_patch".to_string(),
            description: "Apply a single string replacement in a file. Replaces the first occurrence of 'old' with 'new'.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to the file from the workspace root" },
                    "old": { "type": "string", "description": "Exact text to find and replace" },
                    "new": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old", "new"]
            }),
        },
        ToolDefinition {
            name: "delete_file".to_string(),
            description: "Delete a single file. Cannot delete directories.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to the file from the workspace root" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "shell".to_string(),
            description: "Execute a shell command in the workspace directory. Use for running builds, tests, git commands, etc. Commands are time-bounded; pass timeout_ms for known long-running checks.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "timeout_ms": { "type": "integer", "description": "Optional command timeout in milliseconds, capped by the local executor" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "worker_shell".to_string(),
            description: "Run a read-only or verification shell command inside the worker's isolated worktree. Mutating project commands are rejected; use write_file or apply_patch for task-local edits.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Read-only or verification command to execute in the worker worktree" },
                    "timeout_ms": { "type": "integer", "description": "Optional command timeout in milliseconds, capped by the local executor" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "publish_board_tasks".to_string(),
            description: "Publish one or more structured board tasks for the main agent. These tasks are persisted into the active orchestration run and projected into the existing goal task pool.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task_id": { "type": "string" },
                                "stage_id": { "type": "string" },
                                "stage_execution_id": { "type": "string" },
                                "task_type": { "type": "string" },
                                "worker_role": { "type": "string" },
                                "objective": { "type": "string" },
                                "input_artifact_refs": { "type": "array", "items": { "type": "string" } },
                                "required_canonical_artifacts": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "target_artifact_path": { "type": "string" },
                                            "required_status": { "type": "string", "enum": ["baseline_visible", "integration_verified", "active_stage_evidence"] },
                                            "dependency_kind": { "type": "string", "enum": ["file", "runnable", "stage_evidence"] },
                                            "reason": { "type": "string" }
                                        },
                                        "required": ["target_artifact_path", "required_status", "dependency_kind"]
                                    }
                                },
                                "required_output_artifact_type": { "type": "string" },
                                "required_output_fields": { "type": "array", "items": { "type": "string" } },
                                "acceptance_checks": { "type": "array", "items": { "type": "string" } },
                                "failure_signals": { "type": "array", "items": { "type": "string" } },
                                "depends_on_task_ids": { "type": "array", "items": { "type": "string" } },
                                "priority": { "type": "integer" },
                                "evidence_standard": { "type": "string" },
                                "review_findings_refs": { "type": "array", "items": { "type": "string" } },
                                "blocker_refs": { "type": "array", "items": { "type": "string" } },
                                "review_target_task_ids": { "type": "array", "items": { "type": "string" } },
                                "review_target_evidence_refs": { "type": "array", "items": { "type": "string" } },
                                "supersedes_task_ids": { "type": "array", "items": { "type": "string" } },
                                "replacement_of_task_ids": { "type": "array", "items": { "type": "string" } },
                                "current_evidence_set_id": { "type": "string" },
                                "consumed_input_required": { "type": "boolean" }
                            },
                            "required": [
                                "task_type",
                                "worker_role",
                                "objective",
                                "required_output_artifact_type",
                                "required_output_fields",
                                "acceptance_checks",
                                "failure_signals",
                                "evidence_standard"
                            ]
                        }
                    }
                },
                "required": ["tasks"]
            }),
        },
        ToolDefinition {
            name: "record_obligation_decision".to_string(),
            description: "Record a main-agent decision for a blocking obligation. This preserves the route decision, rationale, cleanup requirement, and readiness references in the active orchestration run.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "obligation_id": { "type": "string" },
                    "route": { "type": "string" },
                    "rationale": { "type": "string" },
                    "task_refs": { "type": "array", "items": { "type": "string" } },
                    "evidence_standard": { "type": "string" },
                    "koujing_change": { "type": "boolean" },
                    "cleanup_required": { "type": "boolean" },
                    "readiness_refs": { "type": "array", "items": { "type": "string" } }
                },
                "required": [
                    "obligation_id",
                    "route",
                    "rationale",
                    "evidence_standard",
                    "koujing_change",
                    "cleanup_required"
                ]
            }),
        },
        ToolDefinition {
            name: "record_stage_evidence_plan".to_string(),
            description: "Record the main agent's adopted stage evidence plan. Runtime uses this plan, not advisory default task catalogs, as the mechanical required-evidence gate for the active stage.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "stage_id": { "type": "string" },
                    "stage_execution_id": { "type": "string" },
                    "rationale": { "type": "string" },
                    "plan_source_refs": { "type": "array", "items": { "type": "string" } },
                    "evidence_requirements": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task_type": { "type": "string" },
                                "worker_role": { "type": "string" },
                                "objective": { "type": "string" },
                                "required_output_artifact_type": { "type": "string" },
                                "required_output_fields": { "type": "array", "items": { "type": "string" } },
                                "acceptance_checks": { "type": "array", "items": { "type": "string" } },
                                "failure_signals": { "type": "array", "items": { "type": "string" } },
                                "evidence_standard": { "type": "string" }
                            },
                            "required": [
                                "task_type",
                                "worker_role",
                                "objective",
                                "required_output_artifact_type",
                                "required_output_fields",
                                "acceptance_checks",
                                "failure_signals",
                                "evidence_standard"
                            ]
                        }
                    }
                },
                "required": [
                    "stage_id",
                    "stage_execution_id",
                    "rationale",
                    "plan_source_refs",
                    "evidence_requirements"
                ]
            }),
        },
        ToolDefinition {
            name: "record_stage_closure_decision".to_string(),
            description: "Record the main agent's explicit semantic decision about whether the active stage is closed, should continue, should route backward or pivot, or needs a human gate. Runtime only validates this record mechanically before applying a route change.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "stage_id": { "type": "string" },
                    "stage_execution_id": { "type": "string" },
                    "decision": { "type": "string", "enum": ["close_and_advance", "continue_stage", "rollback_or_pivot", "human_gate"] },
                    "target_stage_id": { "type": "string" },
                    "closure_rationale": { "type": "string" },
                    "why_no_more_stage_work_is_needed": { "type": "string" },
                    "accepted_evidence_refs": { "type": "array", "items": { "type": "string" } },
                    "review_ref": { "type": "string" },
                    "stage_artifact_ref": { "type": "string" },
                    "remaining_risks": { "type": "array", "items": { "type": "string" } },
                    "cleanup_required": { "type": "boolean" },
                    "cleanup_rationale": { "type": "string" },
                    "readiness_refs": { "type": "array", "items": { "type": "string" } }
                },
                "required": [
                    "stage_id",
                    "stage_execution_id",
                    "decision",
                    "target_stage_id",
                    "closure_rationale",
                    "why_no_more_stage_work_is_needed",
                    "accepted_evidence_refs",
                    "review_ref",
                    "stage_artifact_ref",
                    "remaining_risks",
                    "cleanup_required",
                    "cleanup_rationale",
                    "readiness_refs"
                ]
            }),
        },
        ToolDefinition {
            name: "record_worker_artifact_decision".to_string(),
            description: "Record the main agent's explicit accept, reject, or defer decision for candidate-only worker worktree artifacts. This is an audit decision only; it does not apply patches or mutate canonical project files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "candidate_refs": { "type": "array", "items": { "type": "string" } },
                    "decision": { "type": "string", "enum": ["accept", "reject", "defer"] },
                    "rationale": { "type": "string" },
                    "adoption_scope": { "type": "string" },
                    "canonical_target_refs": { "type": "array", "items": { "type": "string" } },
                    "review_required": { "type": "boolean" },
                    "cleanup_required": { "type": "boolean" },
                    "readiness_refs": { "type": "array", "items": { "type": "string" } }
                },
                "required": [
                    "agent_id",
                    "task_id",
                    "candidate_refs",
                    "decision",
                    "rationale",
                    "review_required",
                    "cleanup_required"
                ]
            }),
        },
        ToolDefinition {
            name: "adopt_stage_artifact".to_string(),
            description: "Record the main agent's explicit decision to promote an accepted worker candidate artifact into the active canonical stage artifact. Runtime validates accepted-worker provenance, stage target, and reviewability before writing the canonical artifact.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "stage_id": { "type": "string" },
                    "stage_execution_id": { "type": "string" },
                    "source_agent_id": { "type": "string" },
                    "source_task_id": { "type": "string" },
                    "source_ref": { "type": "string", "description": "Accepted worker evidence ref, output manifest, worktree candidate manifest, or direct candidate artifact ref" },
                    "source_artifact_path": { "type": "string", "description": "Path inside a worker worktree candidate manifest, such as literature_matrix.md" },
                    "target_artifact_path": { "type": "string" },
                    "rationale": { "type": "string" },
                    "evidence_refs": { "type": "array", "items": { "type": "string" } },
                    "replacement_of_artifact_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Canonical artifact ids the main agent explicitly replaces at the same target path. Always include this field: use [] for the first adoption at a target path; when replacing an existing active canonical artifact, copy the active artifact ids from CanonicalArtifactLedger."
                    },
                    "cleanup_required": { "type": "boolean" },
                    "request_review_rerun": { "type": "boolean" }
                },
                "required": [
                    "stage_id",
                    "stage_execution_id",
                    "source_agent_id",
                    "source_ref",
                    "target_artifact_path",
                    "rationale",
                    "evidence_refs"
                ]
            }),
        },
        ToolDefinition {
            name: "record_canonical_artifact_integration_check".to_string(),
            description: "Record the main agent's explicit decision that a baseline-visible canonical project artifact has passed or failed a concrete integration check, based on cited worker evidence. This records facts in the canonical artifact ledger; it does not publish tasks, choose candidates, or close the stage.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "target_artifact_path": { "type": "string" },
                    "status": { "type": "string", "enum": ["passed", "failed"] },
                    "check_id": { "type": "string" },
                    "command": { "type": "string" },
                    "detail": { "type": "string" },
                    "evidence_refs": { "type": "array", "items": { "type": "string" } }
                },
                "required": [
                    "target_artifact_path",
                    "status",
                    "check_id",
                    "command",
                    "detail",
                    "evidence_refs"
                ]
            }),
        },
        ToolDefinition {
            name: "update_board_task".to_string(),
            description: "Update an existing main-agent-published board task without creating a duplicate. Use this to refine objective, role, acceptance, priority, dependencies, or evidence requirements.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "properties": {
                            "stage_id": { "type": "string" },
                            "stage_execution_id": { "type": "string" },
                            "task_type": { "type": "string" },
                            "worker_role": { "type": "string" },
                            "objective": { "type": "string" },
                            "input_artifact_refs": { "type": "array", "items": { "type": "string" } },
                            "required_canonical_artifacts": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "target_artifact_path": { "type": "string" },
                                        "required_status": { "type": "string", "enum": ["baseline_visible", "integration_verified", "active_stage_evidence"] },
                                        "dependency_kind": { "type": "string", "enum": ["file", "runnable", "stage_evidence"] },
                                        "reason": { "type": "string" }
                                    },
                                    "required": ["target_artifact_path", "required_status", "dependency_kind"]
                                }
                            },
                            "required_output_artifact_type": { "type": "string" },
                            "required_output_fields": { "type": "array", "items": { "type": "string" } },
                            "acceptance_checks": { "type": "array", "items": { "type": "string" } },
                            "failure_signals": { "type": "array", "items": { "type": "string" } },
                            "depends_on_task_ids": { "type": "array", "items": { "type": "string" } },
                            "priority": { "type": "integer" },
                            "evidence_standard": { "type": "string" },
                            "review_findings_refs": { "type": "array", "items": { "type": "string" } },
                            "blocker_refs": { "type": "array", "items": { "type": "string" } },
                            "review_target_task_ids": { "type": "array", "items": { "type": "string" } },
                            "review_target_evidence_refs": { "type": "array", "items": { "type": "string" } },
                            "supersedes_task_ids": { "type": "array", "items": { "type": "string" } },
                            "replacement_of_task_ids": { "type": "array", "items": { "type": "string" } },
                            "current_evidence_set_id": { "type": "string" },
                            "consumed_input_required": { "type": "boolean" },
                            "status": { "type": "string" }
                        }
                    },
                    "rationale": { "type": "string" }
                },
                "required": ["task_id", "patch", "rationale"]
            }),
        },
        ToolDefinition {
            name: "merge_board_tasks".to_string(),
            description: "Merge duplicate or overlapping main-agent-published board tasks. Source tasks are marked merged and the target task remains the canonical dispatchable task.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "source_ids": { "type": "array", "items": { "type": "string" } },
                    "target_id": { "type": "string" },
                    "rationale": { "type": "string" }
                },
                "required": ["source_ids", "target_id", "rationale"]
            }),
        },
        ToolDefinition {
            name: "request_review_rerun".to_string(),
            description: "Declare that the main agent believes the active stage is ready for strict review rerun, with readiness evidence refs. This records a request; review gates still enforce policy.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "stage_id": { "type": "string" },
                    "stage_execution_id": { "type": "string" },
                    "readiness_refs": { "type": "array", "items": { "type": "string" } },
                    "rationale": { "type": "string" }
                },
                "required": ["stage_id", "readiness_refs", "rationale"]
            }),
        },
        ToolDefinition {
            name: "request_route_change".to_string(),
            description: "Request a stage-DAG route change such as rollback, pivot, fork, supersede, abandon, or advance. This records the main-agent decision for policy/DAG/cleanup handling; runtime must not infer it from prose.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string" },
                    "target_stage_id": { "type": "string" },
                    "rationale": { "type": "string" },
                    "cleanup_required": { "type": "boolean" },
                    "invalidated_refs": { "type": "array", "items": { "type": "string" } },
                    "readiness_refs": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["operation", "target_stage_id", "rationale", "cleanup_required"]
            }),
        },
        ToolDefinition {
            name: "request_cleanup_plan".to_string(),
            description: "Request repository cleanup after a main-agent route or project口径 change. This records invalidated refs and rationale for ProjectOps/repo cleanup handling.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "reason": { "type": "string" },
                    "invalidated_refs": { "type": "array", "items": { "type": "string" } },
                    "route_change_ref": { "type": "string" },
                    "cleanup_scope": { "type": "string" }
                },
                "required": ["reason", "invalidated_refs"]
            }),
        },
    ]
}

/// Return tool definitions filtered by permission mode.
/// - `read-only`: only read_file, list_files, search, fetch
/// - `workspace-write`: all except shell
/// - `danger-full-access`: all tools
pub fn builtin_tool_definitions_for_mode(permission_mode: &str) -> Vec<ToolDefinition> {
    let all = builtin_tool_definitions();
    let write_tools = [
        "write_file",
        "apply_patch",
        "delete_file",
        "publish_board_tasks",
        "update_board_task",
        "merge_board_tasks",
        "record_obligation_decision",
        "record_stage_evidence_plan",
        "record_stage_closure_decision",
        "record_worker_artifact_decision",
        "adopt_stage_artifact",
        "record_canonical_artifact_integration_check",
        "request_review_rerun",
        "request_route_change",
        "request_cleanup_plan",
    ];
    let dangerous_tools = ["shell"];
    all.into_iter()
        .filter(|t| match permission_mode {
            "read-only" => {
                !write_tools.contains(&t.name.as_str())
                    && !dangerous_tools.contains(&t.name.as_str())
            }
            "workspace-write" => !dangerous_tools.contains(&t.name.as_str()),
            _ => true,
        })
        .collect()
}

/// Return the scoped tool surface for Astra's autonomous-research main agent.
///
/// The main agent owns research strategy, board task publication, worker
/// artifact decisions, route changes, review readiness, and cleanup requests.
/// It can inspect local context, but it must not perform research execution
/// itself: literature fetching, project mutation, patching, and shell work
/// belong to agent-team workers or dedicated runtime paths.
pub fn main_agent_tool_definitions_for_mode(permission_mode: &str) -> Vec<ToolDefinition> {
    let allowed = main_agent_safe_tool_names_for_mode(permission_mode);
    builtin_tool_definitions()
        .into_iter()
        .filter(|definition| allowed.contains(&definition.name.as_str()))
        .collect()
}

pub fn main_agent_safe_tool_names_for_mode(permission_mode: &str) -> Vec<&'static str> {
    let mut tools = vec!["read_file", "list_files", "search"];
    if permission_mode != "read-only" {
        tools.extend([
            "publish_board_tasks",
            "update_board_task",
            "merge_board_tasks",
            "record_obligation_decision",
            "record_stage_evidence_plan",
            "record_stage_closure_decision",
            "record_worker_artifact_decision",
            "adopt_stage_artifact",
            "record_canonical_artifact_integration_check",
            "request_review_rerun",
            "request_route_change",
            "request_cleanup_plan",
        ]);
    }
    tools
}

/// Return the scoped tool surface for provider-backed agent-team workers.
///
/// Agent-team provider workers can inspect project state, retrieve source
/// material, and write task-local artifacts inside their isolated worker
/// worktree. Main-agent control tools are intentionally absent: workers must
/// not publish board tasks, close obligations, request cleanup, or change the
/// research route.
pub fn agent_team_worker_tool_definitions() -> Vec<ToolDefinition> {
    let allowed = agent_team_worker_safe_tool_names();
    builtin_tool_definitions()
        .into_iter()
        .filter(|definition| allowed.contains(&definition.name.as_str()))
        .collect()
}

pub fn agent_team_worker_safe_tool_names() -> Vec<&'static str> {
    vec![
        "read_file",
        "list_files",
        "search",
        "fetch",
        "paper_search",
        "write_file",
        "apply_patch",
        "worker_shell",
    ]
}

pub fn agent_team_worker_tool_definitions_for_policy(
    allowed_tools: &[String],
) -> Vec<ToolDefinition> {
    let safe_tools = agent_team_worker_safe_tool_names();
    builtin_tool_definitions()
        .into_iter()
        .filter(|definition| {
            safe_tools.contains(&definition.name.as_str())
                && allowed_tools
                    .iter()
                    .any(|allowed| allowed.trim() == definition.name)
        })
        .collect()
}

/// Convert tool definitions into the OpenAI API `"tools"` JSON array.
pub fn tool_definitions_to_openai_json(defs: &[ToolDefinition]) -> serde_json::Value {
    serde_json::Value::Array(
        defs.iter()
            .map(|def| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": def.name,
                        "description": def.description,
                        "parameters": def.parameters
                    }
                })
            })
            .collect(),
    )
}

/// Convert an LLM API `ChatToolCall` into the internal `ToolCall` struct
/// by parsing the JSON arguments into the appropriate fields.
pub fn convert_api_tool_call_to_tool_call(api_call: &crate::providers::ChatToolCall) -> ToolCall {
    let args: serde_json::Value =
        serde_json::from_str(&api_call.function.arguments).unwrap_or(serde_json::json!({}));
    let mut call = ToolCall::new(&api_call.function.name);
    call.arguments = args.clone();

    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
        call.target_path = Some(path.to_string());
    }
    if let Some(content) = args.get("content").and_then(|v| v.as_str()) {
        call.content = Some(content.to_string());
    }
    if let Some(command) = args.get("command").and_then(|v| v.as_str()) {
        call.parameters
            .insert("command".to_string(), command.to_string());
    }
    if let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) {
        call.parameters
            .insert("pattern".to_string(), pattern.to_string());
    }
    if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
        call.parameters.insert("url".to_string(), url.to_string());
    }
    if let Some(old) = args.get("old").and_then(|v| v.as_str()) {
        call.parameters.insert("old".to_string(), old.to_string());
    }
    if let Some(new) = args.get("new").and_then(|v| v.as_str()) {
        call.parameters.insert("new".to_string(), new.to_string());
    }
    if let Some(allow) = args.get("allow_private_network").and_then(|v| v.as_bool()) {
        call.parameters
            .insert("allow_private_network".to_string(), allow.to_string());
    }
    for key in [
        "offset",
        "max_chars",
        "timeout_ms",
        "max_results",
        "year_from",
        "year_to",
    ] {
        if let Some(value) = args.get(key) {
            if let Some(number) = value.as_u64() {
                call.parameters.insert(key.to_string(), number.to_string());
            } else if let Some(text) = value.as_str() {
                call.parameters.insert(key.to_string(), text.to_string());
            }
        }
    }

    call
}

/// Extract the target from a tool call's arguments JSON for permission checks.
/// Checks `path`, then `command` (shell), then `url` (fetch).
pub fn extract_target_path_from_args(arguments_json: &str) -> Option<String> {
    let args: serde_json::Value = serde_json::from_str(arguments_json).ok()?;
    args.get("path")
        .or_else(|| args.get("command"))
        .or_else(|| args.get("url"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Format a tool result for LLM consumption, truncating if necessary.
pub fn format_tool_result_for_llm(result: &ToolResult) -> String {
    let mut base = if result.tool_name == "fetch" {
        result
            .structured
            .as_ref()
            .and_then(|structured| structured.get("llm_preview"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                result
                    .output
                    .as_ref()
                    .filter(|output| !output.is_empty())
                    .cloned()
            })
            .unwrap_or_else(|| {
                result
                    .error_message
                    .as_ref()
                    .map(|err| format!("Error: {err}"))
                    .unwrap_or_else(|| result.status.clone())
            })
    } else if let Some(output) = result.output.as_ref().filter(|output| !output.is_empty()) {
        output.clone()
    } else if let Some(ref err) = result.error_message {
        format!("Error: {err}")
    } else {
        result.status.clone()
    };
    if let Some(handoff_reason) = result
        .structured
        .as_ref()
        .and_then(|structured| structured.get("agent_loop_control"))
        .and_then(|control| control.get("main_agent_turn_complete"))
        .and_then(|handoff| handoff.as_bool())
        .filter(|handoff| *handoff)
        .and_then(|_| {
            result
                .structured
                .as_ref()
                .and_then(|structured| structured.get("agent_loop_control"))
                .and_then(|control| control.get("reason"))
                .and_then(|reason| reason.as_str())
        })
    {
        base.push_str("\n\nAstra control: ");
        base.push_str(handoff_reason);
    }
    if base.len() <= MAX_LLM_TOOL_RESULT_CHARS {
        base
    } else if result.tool_name == "read_file" {
        let prefix = safe_char_prefix(&base, MAX_LLM_TOOL_RESULT_CHARS);
        let shown_chars = prefix.chars().count();
        let offset = result
            .structured
            .as_ref()
            .and_then(|structured| structured.get("offset"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as usize;
        let total_chars = result
            .structured
            .as_ref()
            .and_then(|structured| structured.get("total_chars"))
            .and_then(|value| value.as_u64())
            .unwrap_or_else(|| base.chars().count() as u64) as usize;
        let next_offset = offset.saturating_add(shown_chars).min(total_chars);
        format!(
            "{}\n\n[... truncated: {} chars omitted; continue with read_file offset={} max_chars={}]",
            prefix,
            base.chars().count().saturating_sub(shown_chars),
            next_offset,
            MAX_LLM_TOOL_RESULT_CHARS
        )
    } else {
        format!(
            "{}\n\n[... truncated: {} chars omitted]",
            safe_char_prefix(&base, MAX_LLM_TOOL_RESULT_CHARS),
            base.len() - MAX_LLM_TOOL_RESULT_CHARS
        )
    }
}

fn safe_char_prefix(value: &str, max_chars: usize) -> &str {
    if value.len() <= max_chars {
        return value;
    }
    let mut end = 0;
    for (idx, _) in value.char_indices() {
        if idx > max_chars {
            break;
        }
        end = idx;
    }
    &value[..end]
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool_name: String,
    pub target_path: Option<String>,
    pub content: Option<String>,
    pub parameters: BTreeMap<String, String>,
    pub arguments: serde_json::Value,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
}

impl ToolCall {
    pub fn new(tool_name: &str) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            target_path: None,
            content: None,
            parameters: BTreeMap::new(),
            arguments: serde_json::json!({}),
            session_id: None,
            turn_id: None,
        }
    }

    pub fn argument(&self, name: &str) -> Option<&str> {
        self.parameters.get(name).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
struct PreparedBoardTask {
    task_id: String,
    stage_execution_id: Option<String>,
    task_obj: serde_json::Map<String, serde_json::Value>,
    published_task: serde_json::Value,
    task_ref: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug)]
pub enum ToolExecutionError {
    MissingPath(String),
    MissingContent(String),
    MissingArgument { tool: String, argument: String },
    InvalidArgument { tool: String, reason: String },
    WorkspaceBoundary(String),
    UnsupportedTool(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ToolExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPath(tool) => write!(f, "tool '{tool}' requires --path"),
            Self::MissingContent(tool) => write!(f, "tool '{tool}' requires --content"),
            Self::MissingArgument { tool, argument } => {
                write!(f, "tool '{tool}' requires {argument}")
            }
            Self::InvalidArgument { reason, .. } => write!(f, "{reason}"),
            Self::WorkspaceBoundary(reason) => write!(f, "{reason}"),
            Self::UnsupportedTool(tool) => write!(f, "unsupported executable tool: {tool}"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ToolExecutionError {}

impl ToolExecutionError {
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::WorkspaceBoundary(_) => "workspace_scope_violation",
            Self::MissingPath(_) => "missing_path",
            Self::MissingContent(_) => "missing_content",
            Self::MissingArgument { .. } => "missing_argument",
            Self::InvalidArgument { .. } => "invalid_argument",
            Self::UnsupportedTool(_) => "unsupported_tool",
            Self::Io(_) => "io_error",
        }
    }
}

impl From<std::io::Error> for ToolExecutionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone)]
pub struct LocalToolExecutor {
    workspace_root: PathBuf,
    context: Option<LocalToolExecutorContext>,
}

#[derive(Debug, Clone)]
pub struct LocalToolExecutorContext {
    pub data_dir: PathBuf,
    pub project_id: String,
    pub job_id: Option<String>,
    pub stage_id: Option<String>,
    pub stage_execution_id: Option<String>,
    pub stage_artifact_type: Option<String>,
}

impl LocalToolExecutor {
    pub fn new<P>(workspace_root: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
            context: None,
        }
    }

    pub fn with_autonomous_research_context<P>(
        workspace_root: P,
        context: LocalToolExecutorContext,
    ) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
            context: Some(context),
        }
    }

    pub fn execute(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        match call.tool_name.as_str() {
            "read_file" => self.read_file(call),
            "list_files" => self.list_files(call),
            "search" => self.search(call),
            "fetch" => self.fetch(call),
            "paper_search" => self.paper_search(call),
            "write_file" => self.write_file(call),
            "apply_patch" => self.apply_patch(call),
            "delete_file" => self.delete_file(call),
            "shell" => self.shell(call),
            "worker_shell" => self.worker_shell(call),
            "publish_board_tasks" => self.publish_board_tasks(call),
            "update_board_task" => self.update_board_task(call),
            "merge_board_tasks" => self.merge_board_tasks(call),
            "record_obligation_decision" => self.record_obligation_decision(call),
            "record_stage_evidence_plan" => self.record_stage_evidence_plan(call),
            "record_stage_closure_decision" => self.record_stage_closure_decision(call),
            "record_worker_artifact_decision" => self.record_worker_artifact_decision(call),
            "adopt_stage_artifact" => self.record_stage_artifact_adoption(call),
            "record_canonical_artifact_integration_check" => {
                self.record_canonical_artifact_integration_check(call)
            }
            "request_review_rerun" => self.record_control_request(call, "review_rerun_requests"),
            "request_route_change" => self.record_control_request(call, "route_change_requests"),
            "request_cleanup_plan" => self.record_control_request(call, "cleanup_plan_requests"),
            _ => Err(ToolExecutionError::UnsupportedTool(call.tool_name.clone())),
        }
    }

    fn publish_board_tasks(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let tasks = call
            .arguments
            .get("tasks")
            .and_then(|value| value.as_array())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "tasks".to_string(),
            })?;
        let mut prepared_tasks = Vec::new();
        let board_dir = context.data_dir.join("main-agent-board").join("tasks");
        for (index, task) in tasks.iter().enumerate() {
            let mut task_obj =
                task.as_object()
                    .cloned()
                    .ok_or_else(|| ToolExecutionError::InvalidArgument {
                        tool: call.tool_name.clone(),
                        reason: format!("tasks[{index}] must be an object"),
                    })?;
            normalize_board_task_ref_fields(call, index, &mut task_obj)?;
            let task_id = task_obj
                .get("task_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "main_agent_task_{}_{}",
                        context
                            .job_id
                            .as_deref()
                            .unwrap_or(context.project_id.as_str()),
                        index
                    )
                });
            let stage_id = task_obj
                .get("stage_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
            let stage_execution_id = task_obj
                .get("stage_execution_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
            let task_type = task_obj
                .get("task_type")
                .and_then(|value| value.as_str())
                .ok_or_else(|| ToolExecutionError::MissingArgument {
                    tool: call.tool_name.clone(),
                    argument: "task_type".to_string(),
                })?
                .to_string();
            let worker_role = task_obj
                .get("worker_role")
                .and_then(|value| value.as_str())
                .ok_or_else(|| ToolExecutionError::MissingArgument {
                    tool: call.tool_name.clone(),
                    argument: "worker_role".to_string(),
                })?
                .to_string();
            let objective = task_obj
                .get("objective")
                .and_then(|value| value.as_str())
                .ok_or_else(|| ToolExecutionError::MissingArgument {
                    tool: call.tool_name.clone(),
                    argument: "objective".to_string(),
                })?
                .to_string();
            let required_output_artifact_type = task_obj
                .get("required_output_artifact_type")
                .and_then(|value| value.as_str())
                .ok_or_else(|| ToolExecutionError::MissingArgument {
                    tool: call.tool_name.clone(),
                    argument: "required_output_artifact_type".to_string(),
                })?
                .to_string();
            validate_board_task_publication_contract(
                call,
                index,
                &task_type,
                &worker_role,
                &objective,
                &required_output_artifact_type,
                task_obj.get("required_output_fields"),
                task_obj.get("acceptance_checks"),
                task_obj.get("failure_signals"),
                task_obj.get("evidence_standard"),
            )?;
            validate_board_task_artifact_repair_output_contract(
                call,
                index,
                context,
                &task_type,
                &worker_role,
                &required_output_artifact_type,
            )?;
            validate_board_task_synthesis_role_contract(call, index, &task_type, &worker_role)?;
            validate_board_task_stage_slot_identity_contract(
                call, index, context, &task_obj, tasks,
            )?;
            validate_failed_review_artifact_repair_contract(call, index, &task_obj)?;
            validate_board_task_required_slot_repair_type_contract(
                call, index, context, &task_obj, tasks,
            )?;
            validate_board_task_context_refs(call, index, context, &task_obj)?;
            validate_rejected_worker_evidence_authority_contract(
                call,
                index,
                &self.workspace_root,
                context,
                &task_obj,
            )?;
            validate_required_canonical_artifacts_value(
                call,
                &format!("tasks[{index}]"),
                task_obj.get("required_canonical_artifacts"),
            )?;
            validate_required_canonical_artifacts_not_active_stage_target(
                call,
                &format!("tasks[{index}]"),
                context,
                &task_obj,
                task_obj.get("required_canonical_artifacts"),
            )?;
            validate_board_task_dependency_publication_contract(call, index, &task_obj, tasks)?;
            self.validate_board_task_evidence_gap_repair_contract(
                call, index, context, &task_obj, tasks,
            )?;
            validate_board_task_dependency_refs_resolve(
                call, index, context, &task_id, &task_obj, tasks,
            )?;
            let task_ref = format!("main_agent_board_task::{task_id}");
            let published_task = serde_json::json!({
                "schema_version": "main_agent_board_task.v1",
                "task_id": task_id.as_str(),
                "stage_id": stage_id.as_deref(),
                "stage_execution_id": stage_execution_id.as_deref(),
                "task_type": task_type,
                "worker_role": worker_role,
                "objective": objective,
                "input_artifact_refs": task_obj.get("input_artifact_refs").cloned().unwrap_or(serde_json::json!([])),
                "required_canonical_artifacts": task_obj.get("required_canonical_artifacts").cloned().unwrap_or(serde_json::json!([])),
                "required_output_artifact_type": required_output_artifact_type,
                "required_output_fields": task_obj.get("required_output_fields").cloned().unwrap_or(serde_json::json!([])),
                "acceptance_checks": task_obj.get("acceptance_checks").cloned().unwrap_or(serde_json::json!([])),
                "failure_signals": task_obj.get("failure_signals").cloned().unwrap_or(serde_json::json!([])),
                "depends_on_task_ids": task_obj.get("depends_on_task_ids").cloned().unwrap_or(serde_json::json!([])),
                "priority": task_obj.get("priority").cloned().unwrap_or(serde_json::json!(0)),
                "evidence_standard": task_obj.get("evidence_standard").cloned(),
                "review_findings_refs": task_obj.get("review_findings_refs").cloned().unwrap_or(serde_json::json!([])),
                "blocker_refs": task_obj.get("blocker_refs").cloned().unwrap_or(serde_json::json!([])),
                "review_target_task_ids": task_obj.get("review_target_task_ids").cloned().unwrap_or(serde_json::json!([])),
                "review_target_evidence_refs": task_obj.get("review_target_evidence_refs").cloned().unwrap_or(serde_json::json!([])),
                "supersedes_task_ids": task_obj.get("supersedes_task_ids").cloned().unwrap_or(serde_json::json!([])),
                "replacement_of_task_ids": task_obj.get("replacement_of_task_ids").cloned().unwrap_or(serde_json::json!([])),
                "current_evidence_set_id": task_obj.get("current_evidence_set_id").cloned(),
                "consumed_input_required": task_obj.get("consumed_input_required").and_then(|value| value.as_bool()).unwrap_or_else(|| board_task_consumes_upstream_evidence(&task_type, &objective)),
                "project_id": context.project_id.as_str(),
                "job_id": context.job_id.as_deref(),
                "created_at": timestamp_string(),
                "published_by": "main_agent",
            });
            prepared_tasks.push(PreparedBoardTask {
                task_id,
                stage_execution_id,
                task_obj,
                published_task,
                task_ref,
            });
        }
        fs::create_dir_all(&board_dir)?;
        let mut published = Vec::new();
        let mut artifact_refs = Vec::new();
        for prepared in prepared_tasks {
            let task_path = board_dir.join(format!(
                "{}.json",
                sanitize_file_component(&prepared.task_id)
            ));
            let merged_refs = merge_duplicate_stage_task_type_before_publish(
                call,
                &board_dir,
                &prepared.task_id,
                prepared.stage_execution_id.as_deref(),
                &prepared.task_obj,
            )?;
            fs::write(
                &task_path,
                serde_json::to_string_pretty(&prepared.published_task).map_err(|err| {
                    ToolExecutionError::InvalidArgument {
                        tool: call.tool_name.clone(),
                        reason: err.to_string(),
                    }
                })?,
            )?;
            published.push(prepared.task_ref.clone());
            artifact_refs.push(prepared.task_ref);
            artifact_refs.extend(merged_refs);
        }
        self.append_main_agent_artifact_record("published_tasks", artifact_refs)?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "published_count": published.len(),
                "task_refs": published,
                "agent_loop_control": {
                    "main_agent_turn_complete": true,
                    "reason": "board-visible worker tasks have been published; stop this main-agent round so the next research tick can dispatch workers instead of continuing research execution in the main agent"
                }
            })),
            error_message: None,
        })
    }

    fn update_board_task(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let task_id = required_string_argument(call, "task_id")?;
        let patch = call
            .arguments
            .get("patch")
            .and_then(|value| value.as_object())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "patch".to_string(),
            })?;
        let rationale = required_string_argument(call, "rationale")?;
        let task_path = self.main_agent_task_path(&context.data_dir, task_id);
        let content = fs::read_to_string(&task_path)?;
        let mut value: serde_json::Value =
            serde_json::from_str(&content).map_err(|err| ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("task {task_id} is not valid JSON: {err}"),
            })?;
        {
            let Some(obj) = value.as_object_mut() else {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!("task {task_id} is not an object"),
                });
            };
            for (key, patch_value) in patch {
                obj.insert(key.clone(), patch_value.clone());
            }
            normalize_board_task_ref_fields(call, 0, obj)?;
            validate_mount_required_review_input_refs(call, 0, context, obj)?;
            validate_rejected_worker_evidence_authority_contract(
                call,
                0,
                &self.workspace_root,
                context,
                obj,
            )?;
        }
        validate_board_task_value_contract(call, Some(task_id), &value)?;
        if let Some(task) = value.as_object() {
            validate_required_canonical_artifacts_value(
                call,
                &format!("task `{task_id}`"),
                task.get("required_canonical_artifacts"),
            )?;
            validate_required_canonical_artifacts_not_active_stage_target(
                call,
                &format!("task `{task_id}`"),
                context,
                task,
                task.get("required_canonical_artifacts"),
            )?;
            let task_type = task
                .get("task_type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let required_output_artifact_type = task
                .get("required_output_artifact_type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            validate_board_task_artifact_repair_output_contract(
                call,
                0,
                context,
                task_type,
                task.get("worker_role")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
                required_output_artifact_type,
            )?;
            validate_board_task_synthesis_role_contract(
                call,
                0,
                task_type,
                task.get("worker_role")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            )?;
            validate_board_task_stage_slot_identity_contract(call, 0, context, task, &[])?;
            validate_board_task_required_slot_repair_type_contract(call, 0, context, task, &[])?;
            validate_failed_review_artifact_repair_contract(call, 0, task)?;
            self.validate_board_task_evidence_gap_repair_contract(call, 0, context, task, &[])?;
            validate_board_task_dependency_refs_resolve(call, 0, context, task_id, task, &[])?;
        }
        let Some(obj) = value.as_object_mut() else {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("task {task_id} is not an object"),
            });
        };
        if patch.contains_key("status")
            && obj
                .get("status")
                .and_then(|value| value.as_str())
                .map(normalize_board_task_type_key)
                .as_deref()
                != Some("merged")
        {
            obj.remove("merged_into");
            obj.remove("merge_rationale");
        }
        obj.insert(
            "updated_at".to_string(),
            serde_json::json!(timestamp_string()),
        );
        obj.insert("updated_by".to_string(), serde_json::json!("main_agent"));
        obj.insert("update_rationale".to_string(), serde_json::json!(rationale));
        fs::write(
            &task_path,
            serde_json::to_string_pretty(&value).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        let task_ref = format!("main_agent_board_task::{task_id}");
        let update_ref = format!(
            "main_agent_board_task_update::{}::{}",
            sanitize_file_component(task_id),
            timestamp_string()
        );
        self.write_control_record("task-updates", &update_ref, call, Some(task_id))?;
        self.append_main_agent_artifact_record("task_updates", vec![task_ref.clone(), update_ref])?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "task_ref": task_ref,
                "task_id": task_id,
                "updated": true,
                "agent_loop_control": {
                    "main_agent_turn_complete": true,
                    "reason": "board-visible worker tasks have been updated; stop this main-agent round so the next research tick can recompute dispatch eligibility instead of continuing research execution in the main agent"
                }
            })),
            error_message: None,
        })
    }

    fn merge_board_tasks(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let target_id = required_string_argument(call, "target_id")?;
        let rationale = required_string_argument(call, "rationale")?;
        let source_ids = string_array_argument(call, "source_ids")?;
        if source_ids.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "source_ids must not be empty".to_string(),
            });
        }
        let now = timestamp_string();
        let mut merged_refs = Vec::new();
        for source_id in &source_ids {
            if source_id == target_id {
                continue;
            }
            let source_path = self.main_agent_task_path(&context.data_dir, source_id);
            let content =
                fs::read_to_string(&source_path).map_err(|err| match err.kind() {
                    std::io::ErrorKind::NotFound => ToolExecutionError::InvalidArgument {
                        tool: call.tool_name.clone(),
                        reason: format!(
                            "source task `{source_id}` is not a published main-agent board task. `merge_board_tasks` only merges persisted tasks under `.pmcli/main-agent-board/tasks`; to absorb runtime candidate tasks such as `research_stage_task::...`, update the published target with `update_board_task` and cite the candidate ids in `supersedes_task_ids`."
                        ),
                    },
                    _ => ToolExecutionError::Io(err),
                })?;
            let mut value: serde_json::Value = serde_json::from_str(&content).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!("task {source_id} is not valid JSON: {err}"),
                }
            })?;
            let Some(obj) = value.as_object_mut() else {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!("task {source_id} is not an object"),
                });
            };
            obj.insert("status".to_string(), serde_json::json!("merged"));
            obj.insert("merged_into".to_string(), serde_json::json!(target_id));
            obj.insert("merge_rationale".to_string(), serde_json::json!(rationale));
            obj.insert("updated_at".to_string(), serde_json::json!(now.clone()));
            fs::write(
                &source_path,
                serde_json::to_string_pretty(&value).map_err(|err| {
                    ToolExecutionError::InvalidArgument {
                        tool: call.tool_name.clone(),
                        reason: err.to_string(),
                    }
                })?,
            )?;
            merged_refs.push(format!("main_agent_board_task::{source_id}"));
        }
        let merge_ref = format!(
            "main_agent_board_task_merge::{}::{}",
            sanitize_file_component(target_id),
            now
        );
        self.write_control_record("task-merges", &merge_ref, call, Some(target_id))?;
        let mut artifact_refs = vec![format!("main_agent_board_task::{target_id}"), merge_ref];
        artifact_refs.extend(merged_refs.clone());
        self.append_main_agent_artifact_record("task_merges", artifact_refs)?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "target_ref": format!("main_agent_board_task::{target_id}"),
                "merged_refs": merged_refs,
                "agent_loop_control": {
                    "main_agent_turn_complete": true,
                    "reason": "board-visible worker tasks have been merged; stop this main-agent round so the next research tick can dispatch the canonical task instead of continuing research execution in the main agent"
                }
            })),
            error_message: None,
        })
    }

    fn record_obligation_decision(
        &self,
        call: &ToolCall,
    ) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let obligation_id = call
            .arguments
            .get("obligation_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "obligation_id".to_string(),
            })?;
        let route = call
            .arguments
            .get("route")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "route".to_string(),
            })?;
        let rationale = call
            .arguments
            .get("rationale")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "rationale".to_string(),
            })?;
        let evidence_standard = call
            .arguments
            .get("evidence_standard")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "evidence_standard".to_string(),
            })?;
        let koujing_change = call
            .arguments
            .get("koujing_change")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "koujing_change".to_string(),
            })?;
        let cleanup_required = call
            .arguments
            .get("cleanup_required")
            .and_then(|value| value.as_bool())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "cleanup_required".to_string(),
            })?;
        let decision_id = format!(
            "obligation_decision_{}_{}",
            context.project_id, obligation_id
        );
        let decision_path = context
            .data_dir
            .join("main-agent-board")
            .join("obligation-decisions");
        fs::create_dir_all(&decision_path)?;
        let decision_ref = format!("main_agent_obligation_decision::{decision_id}");
        let decision_file =
            decision_path.join(format!("{}.json", sanitize_file_component(&decision_id)));
        let decision = serde_json::json!({
            "schema_version": "main_agent_obligation_decision.v1",
            "decision_id": decision_id.as_str(),
            "obligation_id": obligation_id,
            "route": route,
            "rationale": rationale,
            "task_refs": call.arguments.get("task_refs").cloned().unwrap_or(serde_json::json!([])),
            "evidence_standard": evidence_standard,
            "koujing_change": koujing_change,
            "cleanup_required": cleanup_required,
            "readiness_refs": call.arguments.get("readiness_refs").cloned().unwrap_or(serde_json::json!([])),
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "created_at": timestamp_string(),
            "published_by": "main_agent",
        });
        fs::write(
            &decision_file,
            serde_json::to_string_pretty(&decision).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        self.append_main_agent_artifact_record("obligation_decisions", vec![decision_ref.clone()])?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "decision_ref": decision_ref,
                "obligation_id": obligation_id,
                "route": route
            })),
            error_message: None,
        })
    }

    fn record_stage_evidence_plan(
        &self,
        call: &ToolCall,
    ) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let stage_id = required_string_argument(call, "stage_id")?;
        let stage_execution_id = required_string_argument(call, "stage_execution_id")?;
        if context
            .stage_id
            .as_deref()
            .is_some_and(|active| active != stage_id)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "stage_id `{stage_id}` does not match active stage `{}`",
                    context.stage_id.as_deref().unwrap_or_default()
                ),
            });
        }
        if context
            .stage_execution_id
            .as_deref()
            .is_some_and(|active| active != stage_execution_id)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "stage_execution_id `{stage_execution_id}` does not match active stage execution `{}`",
                    context.stage_execution_id.as_deref().unwrap_or_default()
                ),
            });
        }
        let rationale = required_string_argument(call, "rationale")?;
        let plan_source_refs = string_array_argument(call, "plan_source_refs")?;
        if plan_source_refs.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "plan_source_refs must cite standard-setting evidence, main-agent analysis, or other concrete stage context".to_string(),
            });
        }
        let Some(requirements) = call
            .arguments
            .get("evidence_requirements")
            .and_then(|value| value.as_array())
        else {
            return Err(ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "evidence_requirements".to_string(),
            });
        };
        if requirements.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "evidence_requirements must not be empty".to_string(),
            });
        }
        let mut normalized_requirements = Vec::new();
        for (index, requirement) in requirements.iter().enumerate() {
            let requirement_obj =
                requirement
                    .as_object()
                    .ok_or_else(|| ToolExecutionError::InvalidArgument {
                        tool: call.tool_name.clone(),
                        reason: format!("evidence_requirements[{index}] must be an object"),
                    })?;
            let task_type =
                required_string_field_from_object(call, requirement_obj, index, "task_type")?;
            let worker_role =
                required_string_field_from_object(call, requirement_obj, index, "worker_role")?;
            let objective =
                required_string_field_from_object(call, requirement_obj, index, "objective")?;
            let required_output_artifact_type = required_string_field_from_object(
                call,
                requirement_obj,
                index,
                "required_output_artifact_type",
            )?;
            validate_board_task_publication_contract(
                call,
                index,
                &task_type,
                &worker_role,
                &objective,
                &required_output_artifact_type,
                requirement_obj.get("required_output_fields"),
                requirement_obj.get("acceptance_checks"),
                requirement_obj.get("failure_signals"),
                requirement_obj.get("evidence_standard"),
            )?;
            normalized_requirements.push(serde_json::json!({
                "task_type": task_type,
                "worker_role": worker_role,
                "objective": objective,
                "required_output_artifact_type": required_output_artifact_type,
                "required_output_fields": requirement_obj.get("required_output_fields").cloned().unwrap_or(serde_json::json!([])),
                "acceptance_checks": requirement_obj.get("acceptance_checks").cloned().unwrap_or(serde_json::json!([])),
                "failure_signals": requirement_obj.get("failure_signals").cloned().unwrap_or(serde_json::json!([])),
                "evidence_standard": requirement_obj.get("evidence_standard").cloned().unwrap_or(serde_json::json!("")),
            }));
        }

        let now = timestamp_string();
        let plan_id = format!(
            "stage_evidence_plan_{}_{}_{}",
            context.project_id, stage_execution_id, now
        );
        let plan_ref = format!("main_agent_stage_evidence_plan::{plan_id}");
        let dir = context
            .data_dir
            .join("main-agent-board")
            .join("stage-evidence-plans");
        fs::create_dir_all(&dir)?;
        let plan_file = dir.join(format!("{}.json", sanitize_file_component(&plan_id)));
        let latest_file = dir.join(format!(
            "latest_{}.json",
            sanitize_file_component(stage_execution_id)
        ));
        let record = serde_json::json!({
            "schema_version": "main_agent_stage_evidence_plan.v1",
            "plan_id": plan_id,
            "plan_ref": plan_ref,
            "stage_id": stage_id,
            "stage_execution_id": stage_execution_id,
            "rationale": rationale,
            "plan_source_refs": plan_source_refs,
            "evidence_requirements": normalized_requirements,
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "created_at": now,
            "published_by": "main_agent",
            "authority_note": "main agent adopts the stage evidence requirements; runtime only records and mechanically gates accepted evidence against this plan"
        });
        let content = serde_json::to_string_pretty(&record).map_err(|err| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: err.to_string(),
            }
        })?;
        fs::write(&plan_file, &content)?;
        fs::write(&latest_file, &content)?;
        self.append_main_agent_artifact_record(
            "stage_evidence_plans",
            vec![plan_ref.clone(), latest_file.display().to_string()],
        )?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "plan_ref": plan_ref,
                "stage_id": stage_id,
                "stage_execution_id": stage_execution_id,
                "required_task_types": record
                    .get("evidence_requirements")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("task_type").and_then(|value| value.as_str()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })),
            error_message: None,
        })
    }

    fn record_stage_closure_decision(
        &self,
        call: &ToolCall,
    ) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let stage_id = required_string_argument(call, "stage_id")?;
        let stage_execution_id = required_string_argument(call, "stage_execution_id")?;
        if context
            .stage_id
            .as_deref()
            .is_some_and(|active| active != stage_id)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "stage_id `{stage_id}` does not match active stage `{}`",
                    context.stage_id.as_deref().unwrap_or_default()
                ),
            });
        }
        if context
            .stage_execution_id
            .as_deref()
            .is_some_and(|active| active != stage_execution_id)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "stage_execution_id `{stage_execution_id}` does not match active stage execution `{}`",
                    context.stage_execution_id.as_deref().unwrap_or_default()
                ),
            });
        }
        let decision = required_string_argument(call, "decision")?;
        if !matches!(
            decision,
            "close_and_advance" | "continue_stage" | "rollback_or_pivot" | "human_gate"
        ) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "decision must be one of close_and_advance, continue_stage, rollback_or_pivot, or human_gate".to_string(),
            });
        }
        let target_stage_id = required_string_argument(call, "target_stage_id")?;
        let closure_rationale = required_string_argument(call, "closure_rationale")?;
        let why_no_more_stage_work_is_needed =
            required_string_argument(call, "why_no_more_stage_work_is_needed")?;
        let accepted_evidence_refs = string_array_argument(call, "accepted_evidence_refs")?;
        if accepted_evidence_refs.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "accepted_evidence_refs must not be empty".to_string(),
            });
        }
        let raw_review_ref = required_string_argument(call, "review_ref")?;
        let (review_ref, raw_review_ref_for_record) =
            normalize_stage_closure_review_ref(call, context, decision, raw_review_ref)?;
        let stage_artifact_ref = required_string_argument(call, "stage_artifact_ref")?;
        let remaining_risks = string_array_argument(call, "remaining_risks")?;
        let cleanup_required = required_bool_argument(call, "cleanup_required")?;
        let cleanup_rationale = required_string_argument(call, "cleanup_rationale")?;
        let readiness_refs = string_array_argument(call, "readiness_refs")?;
        if readiness_refs.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "readiness_refs must not be empty".to_string(),
            });
        }

        let now = timestamp_string();
        let decision_id = format!(
            "stage_closure_decision_{}_{}_{}",
            context.project_id, stage_execution_id, now
        );
        let decision_ref = format!("main_agent_stage_closure_decision::{decision_id}");
        let dir = context
            .data_dir
            .join("main-agent-board")
            .join("stage-closure-decisions");
        fs::create_dir_all(&dir)?;
        let decision_file = dir.join(format!("{}.json", sanitize_file_component(&decision_id)));
        let latest_file = dir.join(format!(
            "latest_{}.json",
            sanitize_file_component(stage_execution_id)
        ));
        let record = serde_json::json!({
            "schema_version": "main_agent_stage_closure_decision.v1",
            "decision_id": decision_id,
            "decision_ref": decision_ref,
            "stage_id": stage_id,
            "stage_execution_id": stage_execution_id,
            "decision": decision,
            "target_stage_id": target_stage_id,
            "closure_rationale": closure_rationale,
            "why_no_more_stage_work_is_needed": why_no_more_stage_work_is_needed,
            "accepted_evidence_refs": accepted_evidence_refs,
            "review_ref": review_ref,
            "raw_review_ref": raw_review_ref_for_record,
            "stage_artifact_ref": stage_artifact_ref,
            "remaining_risks": remaining_risks,
            "cleanup_required": cleanup_required,
            "cleanup_rationale": cleanup_rationale,
            "readiness_refs": readiness_refs,
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "created_at": now,
            "published_by": "main_agent",
            "authority_note": "main agent makes the semantic stage-closure decision; runtime only validates cited facts and applies the route when mechanically allowed"
        });
        let content = serde_json::to_string_pretty(&record).map_err(|err| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: err.to_string(),
            }
        })?;
        fs::write(&decision_file, &content)?;
        fs::write(&latest_file, &content)?;
        self.append_main_agent_artifact_record(
            "stage_closure_decisions",
            vec![decision_ref.clone(), latest_file.display().to_string()],
        )?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "decision_ref": decision_ref,
                "stage_id": stage_id,
                "stage_execution_id": stage_execution_id,
                "decision": decision,
                "target_stage_id": target_stage_id
            })),
            error_message: None,
        })
    }

    fn record_worker_artifact_decision(
        &self,
        call: &ToolCall,
    ) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let agent_id = required_string_argument(call, "agent_id")?;
        let task_id = required_string_argument(call, "task_id")?;
        let decision = required_string_argument(call, "decision")?;
        if !matches!(decision, "accept" | "reject" | "defer") {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "decision must be one of accept, reject, or defer".to_string(),
            });
        }
        let rationale = required_string_argument(call, "rationale")?;
        let candidate_refs = normalize_review_artifact_refs_for_data_dir(
            &context.data_dir,
            string_array_argument(call, "candidate_refs")?,
        );
        if candidate_refs.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "candidate_refs must not be empty".to_string(),
            });
        }
        let canonical_target_refs = normalize_review_artifact_refs_for_data_dir(
            &context.data_dir,
            optional_string_array_argument(call, "canonical_target_refs")?,
        );
        let readiness_refs = normalize_review_artifact_refs_for_data_dir(
            &context.data_dir,
            optional_string_array_argument(call, "readiness_refs")?,
        );
        let review_required = required_bool_argument(call, "review_required")?;
        let cleanup_required = required_bool_argument(call, "cleanup_required")?;
        let decision_id = format!(
            "worker_artifact_decision_{}_{}",
            context.project_id,
            timestamp_string()
        );
        let decision_ref = format!("main_agent_worker_artifact_decision::{decision_id}");
        let dir = context
            .data_dir
            .join("main-agent-board")
            .join("worker-artifact-decisions");
        fs::create_dir_all(&dir)?;
        let decision_file = dir.join(format!("{}.json", sanitize_file_component(&decision_id)));
        let record = serde_json::json!({
            "schema_version": "main_agent_worker_artifact_decision.v1",
            "decision_id": decision_id,
            "decision_ref": decision_ref,
            "agent_id": agent_id,
            "task_id": task_id,
            "candidate_refs": candidate_refs,
            "decision": decision,
            "rationale": rationale,
            "adoption_scope": call.arguments.get("adoption_scope").and_then(|value| value.as_str()),
            "canonical_target_refs": canonical_target_refs,
            "review_required": review_required,
            "cleanup_required": cleanup_required,
            "readiness_refs": readiness_refs,
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "stage_id": context.stage_id.as_deref(),
            "stage_execution_id": context.stage_execution_id.as_deref(),
            "created_at": timestamp_string(),
            "published_by": "main_agent",
            "authority_note": "candidate-only worker artifacts are not canonical until a separate adoption path applies them after required review"
        });
        fs::write(
            &decision_file,
            serde_json::to_string_pretty(&record).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        self.append_main_agent_artifact_record(
            "worker_artifact_decisions",
            vec![decision_ref.clone()],
        )?;
        let accepted_worker_evidence_projection_refs =
            project_main_agent_worker_artifact_decisions_into_accepted_evidence(
                call,
                &self.workspace_root,
                context,
            )?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "decision_ref": decision_ref,
                "agent_id": agent_id,
                "task_id": task_id,
                "decision": decision,
                "accepted_worker_evidence_projection_refs": accepted_worker_evidence_projection_refs
            })),
            error_message: None,
        })
    }

    fn record_stage_artifact_adoption(
        &self,
        call: &ToolCall,
    ) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let stage_id = required_string_argument(call, "stage_id")?;
        let stage_execution_id = required_string_argument(call, "stage_execution_id")?;
        let source_agent_id = required_string_argument(call, "source_agent_id")?;
        let source_task_id = call
            .arguments
            .get("source_task_id")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        let mut source_ref = required_string_argument(call, "source_ref")?.to_string();
        let mut source_artifact_path = call
            .arguments
            .get("source_artifact_path")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        let target_artifact_path = required_string_argument(call, "target_artifact_path")?;
        let rationale = required_string_argument(call, "rationale")?;
        let evidence_refs = string_array_argument(call, "evidence_refs")?;
        let replacement_of_artifact_ids = match string_array_argument(
            call,
            "replacement_of_artifact_ids",
        ) {
            Ok(values) => values,
            Err(ToolExecutionError::MissingArgument { .. }) => {
                return Err(ToolExecutionError::InvalidArgument {
                        tool: call.tool_name.clone(),
                        reason: "adopt_stage_artifact requires `replacement_of_artifact_ids`. If this is the first adoption for the target artifact path, pass `replacement_of_artifact_ids: []`. If replacing an existing active canonical artifact, copy the active artifact_id values from CanonicalArtifactLedger into `replacement_of_artifact_ids` before retrying.".to_string(),
                    });
            }
            Err(err) => return Err(err),
        };
        let snapshot_rebind_artifact_id = normalize_snapshot_rebind_source(
            call,
            context,
            &source_agent_id,
            source_task_id.as_deref(),
            &source_ref,
            source_artifact_path.as_deref(),
            target_artifact_path,
            &replacement_of_artifact_ids,
        )?;
        if let Some((artifact_id, ledger_source_ref, ledger_source_artifact_path)) =
            snapshot_rebind_artifact_id.as_ref()
        {
            source_ref = ledger_source_ref.clone();
            source_artifact_path = ledger_source_artifact_path.clone().or(source_artifact_path);
            debug_assert!(replacement_of_artifact_ids.contains(artifact_id));
        }
        if evidence_refs.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "evidence_refs must not be empty".to_string(),
            });
        }
        if context
            .stage_id
            .as_deref()
            .is_some_and(|active| active != stage_id)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "stage_id `{stage_id}` does not match active stage `{}`",
                    context.stage_id.as_deref().unwrap_or_default()
                ),
            });
        }
        if context
            .stage_execution_id
            .as_deref()
            .is_some_and(|active| active != stage_execution_id)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "stage_execution_id `{stage_execution_id}` does not match active stage execution `{}`",
                    context.stage_execution_id.as_deref().unwrap_or_default()
                ),
            });
        }
        if Path::new(target_artifact_path).is_absolute() {
            return Err(ToolExecutionError::WorkspaceBoundary(
                "target_artifact_path must be workspace-relative".to_string(),
            ));
        }
        if stage_governance_artifact_target_path(target_artifact_path) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "`adopt_stage_artifact` cannot target stage governance documents such as `stage_plan.md` or `stage_acceptance_rubric.md`. Use `record_stage_evidence_plan` to adopt or raise stage evidence requirements; runtime maintains review support docframes separately from final stage artifact adoption.".to_string(),
            });
        }
        self.resolve_writable_target(target_artifact_path)?;
        if let Some(reason) = stage_artifact_adoption_previous_rejection_reason(
            &self.workspace_root,
            context,
            stage_id,
            stage_execution_id,
            source_agent_id,
            source_task_id.as_deref(),
            &source_ref,
            source_artifact_path.as_deref(),
            target_artifact_path,
            snapshot_rebind_artifact_id
                .as_ref()
                .map(|(artifact_id, _, _)| artifact_id.as_str()),
        )? {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason,
            });
        }
        let preflight = preflight_stage_artifact_adoption_tool_call(
            call,
            &self.workspace_root,
            context,
            stage_id,
            stage_execution_id,
            source_agent_id,
            source_task_id.as_deref(),
            &source_ref,
            source_artifact_path.as_deref(),
            target_artifact_path,
            rationale,
            &evidence_refs,
            &replacement_of_artifact_ids,
        )?;
        let adoption_id = format!(
            "stage_artifact_adoption_{}_{}",
            context.project_id,
            timestamp_string()
        );
        let adoption_ref = format!("main_agent_stage_artifact_adoption::{adoption_id}");
        let dir = context
            .data_dir
            .join("main-agent-board")
            .join("stage-artifact-adoptions");
        fs::create_dir_all(&dir)?;
        let adoption_file = dir.join(format!("{}.json", sanitize_file_component(&adoption_id)));
        let record = serde_json::json!({
            "schema_version": "main_agent_stage_artifact_adoption.v1",
            "adoption_id": adoption_id,
            "adoption_ref": adoption_ref,
            "stage_id": stage_id,
            "stage_execution_id": stage_execution_id,
            "source_agent_id": source_agent_id,
            "source_task_id": source_task_id.as_deref(),
            "source_ref": source_ref,
            "source_artifact_path": source_artifact_path.as_deref(),
            "target_artifact_path": target_artifact_path,
            "rationale": rationale,
            "evidence_refs": evidence_refs,
            "evidence_snapshot_hash": preflight.evidence_snapshot_hash,
            "accepted_evidence_revision_refs": preflight.accepted_evidence_revision_refs,
            "replacement_of_artifact_ids": replacement_of_artifact_ids,
            "cleanup_required": call.arguments.get("cleanup_required").and_then(|value| value.as_bool()).unwrap_or(false),
            "request_review_rerun": call.arguments.get("request_review_rerun").and_then(|value| value.as_bool()).unwrap_or(false),
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "created_at": timestamp_string(),
            "published_by": "main_agent",
            "authority_note": "main agent selected the candidate; runtime may only validate provenance, target, and reviewability before canonical write"
        });
        fs::write(
            &adoption_file,
            serde_json::to_string_pretty(&record).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        self.append_main_agent_artifact_record(
            "stage_artifact_adoptions",
            vec![
                adoption_ref.clone(),
                format!("stage_artifact:{target_artifact_path}"),
            ],
        )?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "adoption_ref": adoption_ref,
                "stage_id": stage_id,
                "stage_execution_id": stage_execution_id,
                "source_agent_id": source_agent_id,
                "source_ref": source_ref,
                "source_task_id": source_task_id.as_deref(),
                "source_artifact_path": source_artifact_path.as_deref(),
                "target_artifact_path": target_artifact_path,
                "replacement_of_artifact_ids": replacement_of_artifact_ids,
                "resolved_source_refs": preflight.resolved_source_refs,
                "candidate_relative_paths": preflight.candidate_relative_paths,
                "evidence_snapshot_hash": preflight.evidence_snapshot_hash,
                "accepted_evidence_revision_refs": preflight.accepted_evidence_revision_refs,
                "adoption_target_kind": preflight.target_kind
            })),
            error_message: None,
        })
    }

    fn record_canonical_artifact_integration_check(
        &self,
        call: &ToolCall,
    ) -> Result<ToolResult, ToolExecutionError> {
        let context = self.require_context(call)?;
        let target_artifact_path = required_string_argument(call, "target_artifact_path")?;
        if Path::new(target_artifact_path).is_absolute() {
            return Err(ToolExecutionError::WorkspaceBoundary(
                "target_artifact_path must be workspace-relative".to_string(),
            ));
        }
        self.resolve_writable_target(target_artifact_path)?;
        let status = required_string_argument(call, "status")?;
        let check_id = required_string_argument(call, "check_id")?;
        let command = required_string_argument(call, "command")?;
        let detail = required_string_argument(call, "detail")?;
        let evidence_refs = string_array_argument(call, "evidence_refs")?;
        if evidence_refs.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "evidence_refs must not be empty".to_string(),
            });
        }
        let ledger = canonical_artifacts::load_ledger(&context.data_dir).map_err(|err| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("canonical artifact ledger could not be loaded: {err}"),
            }
        })?;
        let normalized_target = normalize_canonical_artifact_target(target_artifact_path);
        let entry = ledger
            .entries
            .iter()
            .rev()
            .find(|entry| {
                normalize_canonical_artifact_target(&entry.target_artifact_path)
                    == normalized_target
                    && entry.status
                        != canonical_artifacts::CanonicalArtifactStatus::RetiredOrSuperseded
            })
            .cloned()
            .ok_or_else(|| ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "no active canonical artifact ledger entry matches `{target_artifact_path}`"
                ),
            })?;
        let updated = match status {
            "passed" => canonical_artifacts::record_integration_verified(
                &context.data_dir,
                &entry.artifact_id,
                check_id.to_string(),
                command.to_string(),
                detail.to_string(),
                evidence_refs.clone(),
            ),
            "failed" => canonical_artifacts::record_integration_failed(
                &context.data_dir,
                &entry.artifact_id,
                check_id.to_string(),
                command.to_string(),
                detail.to_string(),
                evidence_refs.clone(),
            ),
            _ => {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: "status must be `passed` or `failed`".to_string(),
                });
            }
        }
        .map_err(|err| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: err.to_string(),
        })?;
        let check_record_id = format!(
            "canonical_artifact_integration_check_{}_{}",
            sanitize_file_component(check_id),
            timestamp_string()
        );
        let check_ref =
            format!("main_agent_canonical_artifact_integration_check::{check_record_id}");
        let dir = context
            .data_dir
            .join("main-agent-board")
            .join("canonical-artifact-integration-checks");
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join(format!("{}.json", sanitize_file_component(&check_record_id))),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "main_agent_canonical_artifact_integration_check.v1",
                "check_ref": check_ref,
                "artifact_id": updated.artifact_id,
                "target_artifact_path": updated.target_artifact_path,
                "status": status,
                "check_id": check_id,
                "command": command,
                "detail": detail,
                "evidence_refs": evidence_refs,
                "project_id": context.project_id.as_str(),
                "job_id": context.job_id.as_deref(),
                "stage_id": context.stage_id.as_deref(),
                "stage_execution_id": context.stage_execution_id.as_deref(),
                "created_at": timestamp_string(),
                "authority_note": "main agent selected cited evidence; runtime only records integration-check state for an existing canonical artifact"
            }))
            .map_err(|err| ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: err.to_string(),
            })?,
        )?;
        self.append_main_agent_artifact_record(
            "canonical_artifact_integration_checks",
            vec![
                check_ref.clone(),
                format!("canonical_artifact:{}", updated.artifact_id),
                format!("canonical_target:{}", updated.target_artifact_path),
            ],
        )?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "check_ref": check_ref,
                "artifact_id": updated.artifact_id,
                "target_artifact_path": updated.target_artifact_path,
                "canonical_artifact_status": updated.status.as_str(),
                "integration_check_count": updated.integration_checks.len()
            })),
            error_message: None,
        })
    }

    fn require_context(
        &self,
        call: &ToolCall,
    ) -> Result<&LocalToolExecutorContext, ToolExecutionError> {
        self.context
            .as_ref()
            .ok_or_else(|| ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "tool requires autonomous research context".to_string(),
            })
    }

    fn validate_board_task_evidence_gap_repair_contract(
        &self,
        call: &ToolCall,
        index: usize,
        context: &LocalToolExecutorContext,
        task: &serde_json::Map<String, serde_json::Value>,
        batch: &[serde_json::Value],
    ) -> Result<(), ToolExecutionError> {
        if !board_task_matches_active_stage(context, task) {
            return Ok(());
        }
        if board_task_is_stage_synthesis(task)
            && board_task_refs_include_control_plane_diagnosis(task)
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "tasks[{index}] is stage artifact synthesis but consumes control-plane diagnosis evidence. Runtime/adoption/repair-loop diagnosis may guide main-agent routing, but it must not enter synthesis inputs or review-target evidence for the canonical stage artifact. Use only stage-content evidence refs."
                ),
            });
        }
        if let Some(review_id) = active_adoption_snapshot_only_review_id(context, call)? {
            if board_task_targets_review(task, &review_id) && !board_task_has_terminal_status(task)
            {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!(
                        "tasks[{index}] binds snapshot-only failed review `{review_id}`. This is a control-plane adoption drift, not a worker evidence gap: record a fresh adoption against the current stage-relevant accepted evidence snapshot, then request review rerun. Do not publish or update diagnosis, evidence-gap, or synthesis repair work for this review."
                    ),
                });
            }
        }
        let is_repair_or_synthesis = board_task_is_evidence_gap_repair_or_synthesis(task);
        let declares_accepted_evidence = board_task_declares_accepted_evidence_consumption(task);
        if !is_repair_or_synthesis && !declares_accepted_evidence {
            return Ok(());
        }
        if board_task_has_terminal_status(task) {
            return Ok(());
        }
        let Some(gate) = active_evidence_gap_review_gate(context, call)? else {
            return Ok(());
        };
        let has_post_review_evidence = evidence_gap_gate_has_post_review_worker_evidence(
            &self.workspace_root,
            context,
            &gate,
        )?;
        if has_post_review_evidence {
            if board_task_refs_include_accepted_worker_evidence(task) {
                return Ok(());
            }
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "tasks[{index}] consumes evidence for active evidence-gap failed review `{}` after accepted post-review worker evidence exists, but it does not mount that accepted evidence. Repair/synthesis/verification tasks that consume accepted evidence must cite `accepted_worker_evidence_task:*` or `accepted_worker_evidence_index:*` in `input_artifact_refs` or `review_target_evidence_refs`; `depends_on_task_ids` alone is only a scheduling dependency and does not give the worker the accepted evidence bundle.",
                    gate.review_id
                ),
            });
        }
        if !is_repair_or_synthesis {
            return Ok(());
        }
        if board_task_has_evidence_gap_worker_dependency(context, task, batch)? {
            return Ok(());
        }
        let obligation_refs = if gate.obligation_ids.is_empty() {
            "none".to_string()
        } else {
            gate.obligation_ids.join(", ")
        };
        Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] consumes evidence for active evidence-gap failed review `{}` but no post-review evidence-gap worker evidence exists yet and the task declares no dependency on targeted evidence/search/verification/audit/comparison work. Publish or update targeted agent-team evidence tasks first, or make this synthesis task depend on those evidence tasks. Open obligation refs: {obligation_refs}.",
                gate.review_id
            ),
        })
    }

    fn record_control_request(
        &self,
        call: &ToolCall,
        category: &str,
    ) -> Result<ToolResult, ToolExecutionError> {
        self.require_context(call)?;
        validate_control_request_arguments(call)?;
        let request_ref = format!(
            "main_agent_{}::{}",
            category.trim_end_matches('s'),
            timestamp_string()
        );
        self.write_control_record(category, &request_ref, call, None)?;
        self.append_main_agent_artifact_record(category, vec![request_ref.clone()])?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "request_ref": request_ref,
                "category": category
            })),
            error_message: None,
        })
    }

    fn write_control_record(
        &self,
        category: &str,
        record_ref: &str,
        call: &ToolCall,
        target_id: Option<&str>,
    ) -> Result<PathBuf, ToolExecutionError> {
        let context = self.require_context(call)?;
        let dir = context.data_dir.join("main-agent-board").join(category);
        fs::create_dir_all(&dir)?;
        let record_id = record_ref
            .rsplit_once("::")
            .map(|(_, value)| value)
            .unwrap_or(record_ref);
        let path = dir.join(format!("{}.json", sanitize_file_component(record_id)));
        let record = serde_json::json!({
            "schema_version": "main_agent_control_record.v1",
            "record_ref": record_ref,
            "tool_name": call.tool_name,
            "category": category,
            "target_id": target_id,
            "arguments": call.arguments,
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "stage_id": context.stage_id.as_deref(),
            "stage_execution_id": context.stage_execution_id.as_deref(),
            "created_at": timestamp_string(),
            "published_by": "main_agent",
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&record).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        Ok(path)
    }

    fn append_main_agent_artifact_record(
        &self,
        category: &str,
        artifact_refs: Vec<String>,
    ) -> Result<(), ToolExecutionError> {
        let Some(context) = self.context.as_ref() else {
            return Ok(());
        };
        let path = self
            .data_dir_for_context()
            .join("main-agent-board")
            .join(format!("{category}_latest.json"));
        let record = serde_json::json!({
            "schema_version": "main_agent_board_artifact_record.v1",
            "category": category,
            "project_id": context.project_id.as_str(),
            "job_id": context.job_id.as_deref(),
            "stage_id": context.stage_id.as_deref(),
            "stage_execution_id": context.stage_execution_id.as_deref(),
            "artifact_refs": artifact_refs,
            "created_at": timestamp_string(),
        });
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            path,
            serde_json::to_string_pretty(&record).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: "publish_board_tasks".to_string(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        crate::orchestration::append_active_run_step_artifacts(
            &context.data_dir,
            "goal_task_pool",
            &artifact_refs,
        )
        .map_err(|err| ToolExecutionError::InvalidArgument {
            tool: "publish_board_tasks".to_string(),
            reason: err.to_string(),
        })?;
        Ok(())
    }

    fn data_dir_for_context(&self) -> PathBuf {
        self.context
            .as_ref()
            .map(|context| context.data_dir.clone())
            .unwrap_or_else(|| self.workspace_root.clone())
    }

    fn main_agent_task_path(&self, data_dir: &Path, task_id: &str) -> PathBuf {
        data_dir
            .join("main-agent-board")
            .join("tasks")
            .join(format!("{}.json", sanitize_file_component(task_id)))
    }

    fn read_file(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let target_path = call
            .target_path
            .as_deref()
            .ok_or_else(|| ToolExecutionError::MissingPath(call.tool_name.clone()))?;
        let absolute_path = self.resolve_existing_target(target_path)?;
        ensure_text_file(&absolute_path)?;
        let content = fs::read_to_string(&absolute_path)?;
        let total_chars = content.chars().count();
        let offset = parse_u64_argument(call.argument("offset"), 0, "offset").map_err(|reason| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason,
            }
        })? as usize;
        if offset > total_chars {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("offset {offset} is beyond file length {total_chars} chars"),
            });
        }
        let max_chars = match call.argument("max_chars") {
            Some(value) => Some(parse_u64_argument(Some(value), 0, "max_chars").map_err(
                |reason| ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason,
                },
            )? as usize),
            None => None,
        };
        if matches!(max_chars, Some(0)) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "max_chars must be positive when provided".to_string(),
            });
        }
        let output = match max_chars {
            Some(max_chars) => content
                .chars()
                .skip(offset)
                .take(max_chars)
                .collect::<String>(),
            None if offset > 0 => content.chars().skip(offset).collect::<String>(),
            None => content.clone(),
        };
        let returned_chars = output.chars().count();
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: Some(output),
            structured: Some(serde_json::json!({
                "path": target_path,
                "bytes": fs::metadata(&absolute_path)?.len(),
                "offset": offset,
                "returned_chars": returned_chars,
                "total_chars": total_chars,
                "truncated": offset + returned_chars < total_chars,
                "next_offset": if offset + returned_chars < total_chars {
                    Some(offset + returned_chars)
                } else {
                    None
                }
            })),
            error_message: None,
        })
    }

    fn write_file(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let target_path = call
            .target_path
            .as_deref()
            .ok_or_else(|| ToolExecutionError::MissingPath(call.tool_name.clone()))?;
        let content = call
            .content
            .as_deref()
            .ok_or_else(|| ToolExecutionError::MissingContent(call.tool_name.clone()))?;
        if content.len() > MAX_WRITE_SIZE {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "content is too large ({} bytes, max {} bytes)",
                    content.len(),
                    MAX_WRITE_SIZE
                ),
            });
        }
        let absolute_path = self.resolve_writable_target(target_path)?;
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute_path, content)?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "path": target_path,
                "bytes": content.len()
            })),
            error_message: None,
        })
    }

    fn list_files(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let target_path = call.target_path.as_deref().unwrap_or(".");
        let absolute_path = self.resolve_existing_target(target_path)?;
        let canonical_root = self.workspace_root.canonicalize()?;
        let mut files = Vec::new();
        collect_files(
            &absolute_path,
            &canonical_root,
            &mut files,
            MAX_LIST_RESULTS,
        )?;
        files.sort();
        let truncated = files.len() >= MAX_LIST_RESULTS;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: Some(files.join("\n")),
            structured: Some(serde_json::json!({
                "path": target_path,
                "files": files,
                "count": files.len(),
                "truncated": truncated
            })),
            error_message: None,
        })
    }

    fn search(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let target_path = call.target_path.as_deref().unwrap_or(".");
        let pattern = call
            .argument("pattern")
            .or(call.content.as_deref())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "--pattern".to_string(),
            })?;
        let absolute_path = self.resolve_existing_target(target_path)?;
        let canonical_root = self.workspace_root.canonicalize()?;
        let mut files = Vec::new();
        collect_files(
            &absolute_path,
            &canonical_root,
            &mut files,
            MAX_LIST_RESULTS,
        )?;
        files.sort();

        let mut matches = Vec::new();
        for relative_path in &files {
            if matches.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            let file_path = canonical_root.join(relative_path);
            if ensure_text_file(&file_path).is_err() {
                continue;
            }
            let content = fs::read_to_string(&file_path)?;
            for (line_index, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    matches.push(serde_json::json!({
                        "path": relative_path,
                        "line": line_index + 1,
                        "text": line
                    }));
                    if matches.len() >= MAX_SEARCH_MATCHES {
                        break;
                    }
                }
            }
        }
        let rendered_matches = matches
            .iter()
            .filter_map(|item| {
                Some(format!(
                    "{}:{}:{}",
                    item.get("path")?.as_str()?,
                    item.get("line")?.as_u64()?,
                    item.get("text")?.as_str()?
                ))
            })
            .collect::<Vec<_>>();
        let rendered = if rendered_matches.is_empty() {
            format!(
                "No local workspace file matches for pattern `{pattern}` under `{target_path}`. The `search` tool only searches files already present in the worker workspace; use `fetch` with a concrete HTTP URL for external literature retrieval."
            )
        } else {
            rendered_matches.join("\n")
        };
        let truncated = matches.len() >= MAX_SEARCH_MATCHES;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: Some(rendered),
            structured: Some(serde_json::json!({
                "path": target_path,
                "pattern": pattern,
                "matches": matches,
                "match_count": matches.len(),
                "truncated": truncated
            })),
            error_message: None,
        })
    }

    fn apply_patch(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let target_path = call
            .target_path
            .as_deref()
            .ok_or_else(|| ToolExecutionError::MissingPath(call.tool_name.clone()))?;
        let (old, new) = match (call.argument("old"), call.argument("new")) {
            (Some(old), Some(new)) => (old, new),
            _ => call
                .content
                .as_deref()
                .and_then(|content| content.split_once("=>"))
                .ok_or_else(|| ToolExecutionError::MissingArgument {
                    tool: call.tool_name.clone(),
                    argument: "--old and --new".to_string(),
                })?,
        };
        let absolute_path = self.resolve_existing_target(target_path)?;
        ensure_text_file(&absolute_path)?;
        let original = fs::read_to_string(&absolute_path)?;
        let replacements = original.matches(old).count();
        if replacements == 0 {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("old text was not found in {target_path}"),
            });
        }
        let updated = original.replacen(old, new, 1);
        if updated.len() > MAX_WRITE_SIZE {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "patched content is too large ({} bytes, max {} bytes)",
                    updated.len(),
                    MAX_WRITE_SIZE
                ),
            });
        }
        fs::write(&absolute_path, updated)?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "path": target_path,
                "replacements": 1,
                "available_matches": replacements
            })),
            error_message: None,
        })
    }

    fn delete_file(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let target_path = call
            .target_path
            .as_deref()
            .ok_or_else(|| ToolExecutionError::MissingPath(call.tool_name.clone()))?;
        let absolute_path = self.resolve_existing_target(target_path)?;
        if absolute_path.is_dir() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("delete_file only removes files: {target_path}"),
            });
        }
        fs::remove_file(&absolute_path)?;
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "path": target_path,
                "deleted": true
            })),
            error_message: None,
        })
    }

    fn shell(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let command = call
            .argument("command")
            .or(call.content.as_deref())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "--command".to_string(),
            })?;
        let timeout_ms = parse_u64_argument(
            call.argument("timeout_ms"),
            DEFAULT_SHELL_TIMEOUT_MS,
            "timeout_ms",
        )
        .map_err(|reason| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason,
        })?
        .clamp(1_000, MAX_SHELL_TIMEOUT_MS);
        let started_at = Instant::now();
        let mut child_command = Command::new("sh");
        child_command
            .arg("-c")
            .arg(command)
            .current_dir(&self.workspace_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                child_command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let mut child = child_command.spawn()?;
        loop {
            if let Some(status) = child.try_wait()? {
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = status.code();
                return Ok(ToolResult {
                    tool_name: call.tool_name.clone(),
                    status: if status.success() {
                        "succeeded".to_string()
                    } else {
                        "failed".to_string()
                    },
                    exit_code,
                    output: Some(stdout.clone()),
                    structured: Some(serde_json::json!({
                        "command": command,
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": exit_code,
                        "duration_ms": started_at.elapsed().as_millis(),
                        "timeout_ms": timeout_ms,
                        "timed_out": false
                    })),
                    error_message: if status.success() { None } else { Some(stderr) },
                });
            }
            if started_at.elapsed() >= Duration::from_millis(timeout_ms) {
                terminate_child_process_tree(&mut child);
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let error = format!("shell command timed out after {timeout_ms} ms");
                return Ok(ToolResult {
                    tool_name: call.tool_name.clone(),
                    status: "timeout".to_string(),
                    exit_code: None,
                    output: Some(stdout.clone()),
                    structured: Some(serde_json::json!({
                        "command": command,
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": serde_json::Value::Null,
                        "duration_ms": started_at.elapsed().as_millis(),
                        "timeout_ms": timeout_ms,
                        "timed_out": true,
                        "error": error
                    })),
                    error_message: Some(error),
                });
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn worker_shell(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let command = call
            .argument("command")
            .or(call.content.as_deref())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "--command".to_string(),
            })?;
        if !is_worker_shell_command(command) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: "worker_shell accepts only safe inspection or verification commands; use write_file/apply_patch for task-local edits".to_string(),
            });
        }
        self.shell(call)
    }

    fn fetch(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let url = call
            .argument("url")
            .or(call.content.as_deref())
            .ok_or_else(|| ToolExecutionError::MissingArgument {
                tool: call.tool_name.clone(),
                argument: "--url".to_string(),
            })?;
        let endpoint =
            parse_http_url(url).map_err(|reason| ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason,
            })?;
        let allow_private_network = parse_bool_argument(
            call.argument("allow_private_network"),
            false,
            "allow_private_network",
        )
        .map_err(|reason| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason,
        })?;
        enforce_fetch_network_policy(&endpoint, allow_private_network).map_err(|reason| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason,
            }
        })?;

        let fetched = fetch_url_with_policy(url, allow_private_network).map_err(|reason| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason,
            }
        })?;
        let output = String::from_utf8_lossy(&fetched.body).to_string();
        let llm_preview = fetched_content_preview_for_llm(url, &fetched.final_url, &output);
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: if (200..300).contains(&fetched.status_code) {
                "succeeded".to_string()
            } else {
                "failed".to_string()
            },
            exit_code: Some(if (200..300).contains(&fetched.status_code) {
                0
            } else {
                1
            }),
            output: Some(output.clone()),
            structured: Some(serde_json::json!({
                "url": url,
                "final_url": fetched.final_url,
                "scheme": endpoint.scheme,
                "status_code": fetched.status_code,
                "bytes": fetched.body.len(),
                "body_preview": output.chars().take(4096).collect::<String>(),
                "llm_preview": llm_preview,
                "redirect_count": fetched.redirect_chain.len(),
                "redirect_chain": fetched.redirect_chain,
                "network_policy": {
                    "private_network_allowed": allow_private_network,
                    "checked": true
                }
            })),
            error_message: if (200..300).contains(&fetched.status_code) {
                None
            } else {
                Some(format!(
                    "fetch returned HTTP status {}",
                    fetched.status_code
                ))
            },
        })
    }

    fn paper_search(&self, call: &ToolCall) -> Result<ToolResult, ToolExecutionError> {
        let query = required_string_argument(call, "query")?;
        let source = call
            .argument("source")
            .or_else(|| {
                call.arguments
                    .get("source")
                    .and_then(|value| value.as_str())
            })
            .unwrap_or("auto")
            .trim()
            .to_ascii_lowercase();
        let max_results = call
            .arguments
            .get("max_results")
            .and_then(|value| value.as_u64())
            .unwrap_or(10)
            .clamp(1, MAX_PAPER_SEARCH_RESULTS as u64) as usize;
        let year_from = call
            .arguments
            .get("year_from")
            .and_then(|value| value.as_u64());
        let year_to = call
            .arguments
            .get("year_to")
            .and_then(|value| value.as_u64());
        if let (Some(from), Some(to)) = (year_from, year_to) {
            if from > to {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: "year_from must be less than or equal to year_to".to_string(),
                });
            }
        }

        let mut candidates = Vec::new();
        let sources: Vec<&str> = match source.as_str() {
            "auto" => vec!["arxiv", "semantic_scholar"],
            "arxiv" | "semantic_scholar" | "crossref" => vec![source.as_str()],
            other => {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!(
                        "source must be auto, arxiv, semantic_scholar, or crossref; got {other}"
                    ),
                })
            }
        };
        let mut errors = Vec::new();
        for selected_source in sources {
            match paper_search_via_source(query, selected_source, max_results, year_from, year_to) {
                Ok(mut result) => {
                    candidates.append(&mut result);
                    if !candidates.is_empty() && source != "auto" {
                        break;
                    }
                }
                Err(err) => errors.push(format!("{selected_source}: {err}")),
            }
            if source == "auto" && !candidates.is_empty() {
                break;
            }
        }
        if candidates.is_empty() {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: if errors.is_empty() {
                    "no paper search candidates returned".to_string()
                } else {
                    format!("paper search failed: {}", errors.join("; "))
                },
            });
        }
        if candidates.len() > max_results {
            candidates.truncate(max_results);
        }
        Ok(ToolResult {
            tool_name: call.tool_name.clone(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: Some(serde_json::to_string_pretty(&candidates).unwrap_or_default()),
            structured: Some(serde_json::json!({
                "query": query,
                "source": source,
                "max_results": max_results,
                "year_from": year_from,
                "year_to": year_to,
                "candidates": candidates,
            })),
            error_message: None,
        })
    }

    fn resolve_existing_target(&self, target_path: &str) -> Result<PathBuf, ToolExecutionError> {
        let candidate = self.workspace_root.join(target_path);
        let canonical = candidate.canonicalize()?;
        self.ensure_inside_workspace(&canonical, target_path)?;
        Ok(canonical)
    }

    fn resolve_writable_target(&self, target_path: &str) -> Result<PathBuf, ToolExecutionError> {
        let candidate = self.workspace_root.join(target_path);
        let parent = candidate.parent().ok_or_else(|| {
            ToolExecutionError::WorkspaceBoundary(format!(
                "target path has no parent inside workspace boundary: {target_path}"
            ))
        })?;
        let canonical_parent = if parent.exists() {
            parent.canonicalize()?
        } else {
            let existing_parent = nearest_existing_parent(parent)?;
            let canonical_existing_parent = existing_parent.canonicalize()?;
            let relative_suffix = candidate.strip_prefix(&existing_parent).map_err(|_| {
                ToolExecutionError::WorkspaceBoundary(format!(
                    "target path escapes workspace boundary: {target_path}"
                ))
            })?;
            let resolved = canonical_existing_parent.join(relative_suffix);
            let resolved_parent = resolved.parent().ok_or_else(|| {
                ToolExecutionError::WorkspaceBoundary(format!(
                    "target path has no parent inside workspace boundary: {target_path}"
                ))
            })?;
            self.ensure_inside_workspace(resolved_parent, target_path)?;
            return Ok(resolved);
        };
        self.ensure_inside_workspace(&canonical_parent, target_path)?;
        Ok(canonical_parent.join(candidate.file_name().ok_or_else(|| {
            ToolExecutionError::WorkspaceBoundary(format!(
                "target path has no file name inside workspace boundary: {target_path}"
            ))
        })?))
    }

    fn ensure_inside_workspace(
        &self,
        canonical_path: &Path,
        target_path: &str,
    ) -> Result<(), ToolExecutionError> {
        let canonical_root = self.workspace_root.canonicalize()?;
        if canonical_path.starts_with(&canonical_root) {
            Ok(())
        } else {
            Err(ToolExecutionError::WorkspaceBoundary(format!(
                "target path escapes workspace boundary: {target_path}"
            )))
        }
    }
}

struct HttpEndpoint {
    scheme: String,
    host: String,
    host_header: String,
    port: u16,
    path: String,
    url: String,
}

fn parse_http_url(url: &str) -> Result<HttpEndpoint, String> {
    let trimmed = url.trim();
    if trimmed.chars().any(char::is_control) {
        return Err(format!("fetch URL contains control characters: {url}"));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(format!("fetch URL contains whitespace: {url}"));
    }
    let (scheme, no_scheme, default_port) = if let Some(no_scheme) = trimmed.strip_prefix("http://")
    {
        ("http", no_scheme, 80)
    } else if let Some(no_scheme) = trimmed.strip_prefix("https://") {
        ("https", no_scheme, 443)
    } else {
        return Err(format!(
            "fetch supports http:// and https:// URLs only: {url}"
        ));
    };
    let split_index = no_scheme.find(['/', '?']).unwrap_or(no_scheme.len());
    let (authority, remainder) = no_scheme.split_at(split_index);
    if authority.is_empty() {
        return Err(format!("fetch URL is missing a host: {url}"));
    }
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        let parsed_port = port
            .parse::<u16>()
            .map_err(|_| format!("invalid fetch URL port: {url}"))?;
        (host.to_string(), parsed_port)
    } else {
        (authority.to_string(), default_port)
    };
    if host.is_empty() {
        return Err(format!("fetch URL is missing a host: {url}"));
    }
    if authority.chars().any(char::is_whitespace) || host.chars().any(char::is_whitespace) {
        return Err(format!("fetch URL host contains whitespace: {url}"));
    }
    let path = if remainder.is_empty() {
        "/".to_string()
    } else if remainder.starts_with('?') {
        format!("/{remainder}")
    } else {
        remainder.to_string()
    };
    Ok(HttpEndpoint {
        scheme: scheme.to_string(),
        host,
        host_header: authority.to_string(),
        port,
        path,
        url: trimmed.to_string(),
    })
}

fn parse_bool_argument(value: Option<&str>, default: bool, name: &str) -> Result<bool, String> {
    match value {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(other) => Err(format!("{name} must be true or false, got {other}")),
    }
}

fn parse_u64_argument(value: Option<&str>, default: u64, name: &str) -> Result<u64, String> {
    match value {
        None => Ok(default),
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an unsigned integer")),
    }
}

fn required_string_argument<'a>(
    call: &'a ToolCall,
    name: &str,
) -> Result<&'a str, ToolExecutionError> {
    call.arguments
        .get(name)
        .and_then(|value| value.as_str())
        .or_else(|| call.parameters.get(name).map(String::as_str))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolExecutionError::MissingArgument {
            tool: call.tool_name.clone(),
            argument: name.to_string(),
        })
}

#[derive(Debug, Clone)]
struct EvidenceGapReviewGate {
    review_id: String,
    created_at: String,
    obligation_ids: Vec<String>,
}

fn active_adoption_snapshot_only_review_id(
    context: &LocalToolExecutorContext,
    call: &ToolCall,
) -> Result<Option<String>, ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Ok(None);
    };
    let job_path = context
        .data_dir
        .join("research")
        .join("jobs")
        .join(job_id)
        .join("job.json");
    if !job_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&job_path)?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|err| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("active autonomous research job state is not valid JSON: {err}"),
        })?;
    let Some(review) = value.get("last_review").and_then(|value| value.as_object()) else {
        return Ok(None);
    };
    if review
        .get("verdict")
        .and_then(|value| value.as_str())
        .is_some_and(|verdict| verdict == "pass")
    {
        return Ok(None);
    }
    let review_text = review
        .get("response_text")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let snapshot_only = review_text.lines().any(|line| {
        line.trim()
            .strip_prefix("fail blocking_stage_gate:")
            .is_some_and(|gates| {
                gates
                    .split(',')
                    .all(|gate| gate.trim() == "stage_artifact_adoption_snapshot")
            })
    });
    if !snapshot_only {
        return Ok(None);
    }
    Ok(review
        .get("review_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string))
}

fn board_task_targets_review(
    task: &serde_json::Map<String, serde_json::Value>,
    review_id: &str,
) -> bool {
    [
        "input_artifact_refs",
        "review_findings_refs",
        "blocker_refs",
        "review_target_evidence_refs",
    ]
    .into_iter()
    .flat_map(|field| board_task_string_array(task, field))
    .any(|reference| reference.contains(review_id))
}

fn board_task_is_stage_synthesis(task: &serde_json::Map<String, serde_json::Value>) -> bool {
    task.get("worker_role")
        .and_then(|value| value.as_str())
        .is_some_and(|role| role == "research_synthesizer")
        && task
            .get("task_type")
            .and_then(|value| value.as_str())
            .map(normalize_board_task_type_key)
            .is_some_and(|task_type| task_type.contains("synthesis"))
}

fn board_task_refs_include_control_plane_diagnosis(
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    ["input_artifact_refs", "review_target_evidence_refs"]
        .into_iter()
        .flat_map(|field| board_task_string_array(task, field))
        .map(|reference| reference.to_ascii_lowercase())
        .any(|reference| {
            reference.contains("runtime_diagnosis")
                || reference.contains("repair_loop_strategy_diagnosis")
                || reference.contains("adoption_snapshot_binding_diagnosis")
        })
}

fn active_evidence_gap_review_gate(
    context: &LocalToolExecutorContext,
    call: &ToolCall,
) -> Result<Option<EvidenceGapReviewGate>, ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Ok(None);
    };
    let job_path = context
        .data_dir
        .join("research")
        .join("jobs")
        .join(job_id)
        .join("job.json");
    if !job_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&job_path)?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|err| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("active autonomous research job state is not valid JSON: {err}"),
        })?;
    let Some(review) = value.get("last_review").and_then(|value| value.as_object()) else {
        return Ok(None);
    };
    if review
        .get("verdict")
        .and_then(|value| value.as_str())
        .is_some_and(|verdict| verdict == "pass")
    {
        return Ok(None);
    }
    let Some(review_id) = review
        .get("review_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let review_text = review
        .get("response_text")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let mut obligation_ids = Vec::new();
    if let Some(obligations) = value.get("obligations").and_then(|value| value.as_array()) {
        for obligation in obligations {
            if !obligation
                .get("blocking")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            let status = obligation
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if matches!(status, "satisfied" | "closed") {
                continue;
            }
            if obligation
                .get("review_id")
                .and_then(|value| value.as_str())
                .is_some_and(|id| id != review_id)
            {
                continue;
            }
            if context.stage_id.as_deref().is_some_and(|active_stage| {
                obligation
                    .get("stage_id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|stage_id| stage_id != active_stage)
            }) {
                continue;
            }
            let failure_class = obligation
                .get("failure_class")
                .and_then(|value| value.as_str());
            let detail = obligation
                .get("detail")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if !failure_text_requires_evidence_gap_worker_repair(failure_class, detail) {
                continue;
            }
            if let Some(obligation_id) = obligation
                .get("obligation_id")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                obligation_ids.push(obligation_id.to_string());
            }
        }
    }
    if obligation_ids.is_empty()
        && !failure_text_requires_evidence_gap_worker_repair(None, review_text)
    {
        return Ok(None);
    }
    Ok(Some(EvidenceGapReviewGate {
        review_id: review_id.to_string(),
        created_at: review
            .get("created_at")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        obligation_ids,
    }))
}

#[derive(Debug, Clone)]
struct CurrentPassingStageReview {
    review_id: String,
}

fn normalize_stage_closure_review_ref(
    call: &ToolCall,
    context: &LocalToolExecutorContext,
    decision: &str,
    review_ref: &str,
) -> Result<(String, Option<String>), ToolExecutionError> {
    if decision != "close_and_advance" {
        return Ok((review_ref.to_string(), None));
    }
    let Some(review) = current_passing_stage_review(context, call)? else {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: "close_and_advance requires the active autonomous research job to have a current passing review; run review first or record continue_stage instead".to_string(),
        });
    };
    if stage_closure_review_ref_matches_review_id(review_ref, &review.review_id)
        || stage_closure_review_ref_is_latest_pass_alias(review_ref, context)
    {
        let canonical = format!("review:{}", review.review_id);
        let raw = (review_ref != canonical).then(|| review_ref.to_string());
        return Ok((canonical, raw));
    }
    Err(ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason: format!(
            "close_and_advance review_ref must cite the current passing review `{}` or an explicit latest-pass alias, not `{}`",
            review.review_id, review_ref
        ),
    })
}

fn current_passing_stage_review(
    context: &LocalToolExecutorContext,
    call: &ToolCall,
) -> Result<Option<CurrentPassingStageReview>, ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Ok(None);
    };
    let job_path = context
        .data_dir
        .join("research")
        .join("jobs")
        .join(job_id)
        .join("job.json");
    if !job_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&job_path)?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|err| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("active autonomous research job state is not valid JSON: {err}"),
        })?;
    if context.stage_execution_id.as_deref().is_some_and(|active| {
        value
            .get("stage_execution_id")
            .and_then(|value| value.as_str())
            .is_some_and(|stage_execution_id| stage_execution_id != active)
    }) {
        return Ok(None);
    }
    let Some(review) = value.get("last_review").and_then(|value| value.as_object()) else {
        return Ok(None);
    };
    if !review
        .get("verdict")
        .and_then(|value| value.as_str())
        .is_some_and(|verdict| verdict == "pass")
    {
        return Ok(None);
    }
    let Some(review_id) = review
        .get("review_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(CurrentPassingStageReview {
        review_id: review_id.to_string(),
    }))
}

fn stage_closure_review_ref_matches_review_id(review_ref: &str, review_id: &str) -> bool {
    let trimmed = review_ref.trim().trim_matches('`');
    trimmed == review_id
        || trimmed == format!("review:{review_id}")
        || trimmed == format!("review_packet:{review_id}")
        || trimmed == format!("review_trace:{review_id}")
        || review_refs::review_ids_in_text(trimmed).contains(review_id)
}

fn stage_closure_review_ref_is_latest_pass_alias(
    review_ref: &str,
    context: &LocalToolExecutorContext,
) -> bool {
    let normalized = review_ref.trim().to_ascii_lowercase().replace('-', "_");
    if !normalized.contains("latest_pass") {
        return false;
    }
    if matches!(
        normalized.as_str(),
        "latest_pass"
            | "review:latest_pass"
            | "review_packet:latest_pass"
            | "review_trace:latest_pass"
    ) {
        return true;
    }
    context
        .stage_execution_id
        .as_deref()
        .map(|stage_execution_id| {
            let stage_execution_id = stage_execution_id.to_ascii_lowercase().replace('-', "_");
            normalized.contains(&stage_execution_id)
        })
        .unwrap_or(false)
}

fn evidence_gap_gate_has_post_review_worker_evidence(
    workspace_root: &Path,
    context: &LocalToolExecutorContext,
    gate: &EvidenceGapReviewGate,
) -> Result<bool, ToolExecutionError> {
    if gate.created_at.trim().is_empty() {
        return Ok(false);
    }
    let (Some(job_id), Some(stage_execution_id)) = (
        context.job_id.as_deref(),
        context.stage_execution_id.as_deref(),
    ) else {
        return Ok(false);
    };
    let index_path = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence")
        .join("index.json");
    if !index_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&index_path)?;
    let Ok(index) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(false);
    };
    let Some(entries) = index.get("entries").and_then(|value| value.as_array()) else {
        return Ok(false);
    };
    Ok(entries.iter().any(|entry| {
        entry
            .get("created_at")
            .and_then(|value| value.as_str())
            .map(|created_at| timestamp_string_is_after(created_at, &gate.created_at))
            .unwrap_or(false)
            && accepted_worker_evidence_value_can_repair_evidence_gap(entry)
    }))
}

fn failure_text_requires_evidence_gap_worker_repair(
    failure_class: Option<&str>,
    text: &str,
) -> bool {
    let joined = format!("{} {}", failure_class.unwrap_or_default(), text).to_ascii_lowercase();
    let lowered_text = text.to_ascii_lowercase();
    if lowered_text.contains("stage_artifact_adoption_snapshot")
        && ![
            "evidence_gap",
            "evidence gap",
            "evidence-gap",
            "unexamined closest prior",
            "missing source",
            "missing-source",
            "source gap",
            "citation gap",
            "unsupported claim",
        ]
        .iter()
        .any(|needle| lowered_text.contains(needle))
    {
        return false;
    }
    [
        "evidence_gap",
        "evidence gap",
        "evidence-gap",
        "unexamined closest prior",
        "unexamined_closest_prior",
        "closest-prior",
        "closest prior",
        "strongest-prior",
        "prior family",
        "prior families",
        "source grounding",
        "source-grounding",
        "insufficient_source_grounding",
        "literature_evidence_failure",
        "missing source",
        "missing-source",
        "source gap",
        "citation gap",
        "unsupported claim",
        "targeted evidence",
        "not mounted here",
    ]
    .iter()
    .any(|needle| joined.contains(needle))
}

fn accepted_worker_evidence_value_can_repair_evidence_gap(entry: &serde_json::Value) -> bool {
    let task_type = entry
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if board_task_is_artifact_repair_task(task_type) {
        return false;
    }
    board_task_profile_can_repair_evidence_gap(
        task_type,
        entry
            .get("worker_role")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
        entry
            .get("required_output_artifact_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
    )
}

fn board_task_profile_can_repair_evidence_gap(
    task_type: &str,
    worker_role: &str,
    required_output_artifact_type: &str,
) -> bool {
    let profile =
        format!("{task_type} {worker_role} {required_output_artifact_type}").to_ascii_lowercase();
    let tokens = profile
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    [
        "search",
        "verification",
        "citation",
        "survey",
        "comparison",
        "extraction",
        "analysis",
        "audit",
        "review",
        "cluster",
        "clustering",
        "prior",
        "source",
        "evidence",
        "baseline",
        "metric",
        "ablation",
        "experiment",
        "run",
        "log",
        "result",
        "claim",
    ]
    .iter()
    .any(|needle| tokens.iter().any(|token| token == needle))
}

fn board_task_matches_active_stage(
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    if context.stage_execution_id.as_deref().is_some_and(|active| {
        task.get("stage_execution_id")
            .and_then(|value| value.as_str())
            .is_some_and(|stage_execution_id| stage_execution_id != active)
    }) {
        return false;
    }
    if context.stage_id.as_deref().is_some_and(|active| {
        task.get("stage_id")
            .and_then(|value| value.as_str())
            .is_some_and(|stage_id| stage_id != active)
    }) {
        return false;
    }
    true
}

fn active_stage_artifact_path_for_context(context: &LocalToolExecutorContext) -> Option<String> {
    let job_id = context.job_id.as_deref()?.trim();
    let stage_id = context.stage_id.as_deref()?.trim();
    let artifact_type = context.stage_artifact_type.as_deref()?.trim();
    if job_id.is_empty() || stage_id.is_empty() || artifact_type.is_empty() {
        return None;
    }
    Some(match stage_id {
        "paper-write" => format!("papers/{job_id}/main.tex"),
        "paper-compile" => format!("papers/{job_id}/compiled_pdf_bundle.md"),
        "research-review" => format!("research/stages/{job_id}/{stage_id}/hard_review_packet.md"),
        _ => format!("research/stages/{job_id}/{stage_id}/{artifact_type}.md"),
    })
}

fn board_task_is_artifact_repair_task(task_type: &str) -> bool {
    normalize_board_task_type_key(task_type) == "artifact repair"
}

fn board_task_has_terminal_status(task: &serde_json::Map<String, serde_json::Value>) -> bool {
    matches!(
        task.get("status")
            .and_then(|value| value.as_str())
            .map(normalize_board_task_type_key)
            .as_deref(),
        Some("merged" | "cancelled" | "canceled" | "rejected" | "superseded")
    )
}

fn board_task_has_evidence_gap_worker_dependency(
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<bool, ToolExecutionError> {
    let mut refs = board_task_string_array(task, "depends_on_task_ids");
    refs.extend(board_task_string_array(task, "input_artifact_refs"));
    if refs.is_empty() {
        return Ok(false);
    }
    for reference in refs {
        if batch
            .iter()
            .filter_map(|value| value.as_object())
            .any(|candidate| {
                board_task_ref_matches_candidate(&reference, candidate)
                    && board_task_value_can_repair_evidence_gap(candidate)
            })
        {
            return Ok(true);
        }
        if existing_board_task_ref_can_repair_evidence_gap(context, &reference)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn board_task_refs_include_accepted_worker_evidence(
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    ["input_artifact_refs", "review_target_evidence_refs"]
        .iter()
        .flat_map(|field| board_task_string_array(task, field))
        .any(|reference| {
            let normalized = reference.trim().trim_matches('`').replace('\\', "/");
            matches!(
                board_task_refs::classify_board_task_ref(&normalized),
                board_task_refs::BoardTaskRefKind::AcceptedWorkerEvidenceTask
                    | board_task_refs::BoardTaskRefKind::AcceptedWorkerEvidenceIndex
            ) || normalized.contains("/accepted_worker_evidence/index.json")
        })
}

fn board_task_is_evidence_gap_repair_or_synthesis(
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if board_task_is_artifact_repair_task(task_type)
        || board_task_is_stage_synthesis_task(task_type)
        || board_task_is_repair_or_replacement_task(&normalize_board_task_type_key(task_type))
    {
        return true;
    }
    false
}

fn board_task_declares_accepted_evidence_consumption(
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let objective = task
        .get("objective")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let evidence_standard = task
        .get("evidence_standard")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let acceptance_checks = board_task_string_array(task, "acceptance_checks").join(" ");
    let profile = format!("{objective} {evidence_standard} {acceptance_checks}")
        .to_ascii_lowercase()
        .replace('-', " ");
    [
        "accepted evidence",
        "accepted worker evidence",
        "accepted literature evidence",
        "from accepted worker evidence",
        "from accepted evidence",
        "use only accepted evidence",
        "grounded only in accepted",
        "consume accepted",
    ]
    .iter()
    .any(|marker| profile.contains(marker))
}

fn board_task_value_can_repair_evidence_gap(
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if board_task_is_artifact_repair_task(task_type) {
        return false;
    }
    board_task_profile_can_repair_evidence_gap(
        task_type,
        task.get("worker_role")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
        task.get("required_output_artifact_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
    )
}

fn existing_board_task_ref_can_repair_evidence_gap(
    context: &LocalToolExecutorContext,
    reference: &str,
) -> Result<bool, ToolExecutionError> {
    let board_dir = context.data_dir.join("main-agent-board").join("tasks");
    if !board_dir.exists() {
        return Ok(false);
    }
    let normalized_ref = normalize_board_task_dependency_ref(reference);
    let direct_path = board_dir.join(format!("{}.json", sanitize_file_component(&normalized_ref)));
    if direct_path.exists() {
        return board_task_file_can_repair_evidence_gap(&direct_path);
    }
    for entry in fs::read_dir(board_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(task) = value.as_object() else {
            continue;
        };
        if board_task_ref_matches_candidate(&normalized_ref, task)
            && board_task_value_can_repair_evidence_gap(task)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn board_task_file_can_repair_evidence_gap(path: &Path) -> Result<bool, ToolExecutionError> {
    let content = fs::read_to_string(path)?;
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(false);
    };
    Ok(value
        .as_object()
        .map(board_task_value_can_repair_evidence_gap)
        .unwrap_or(false))
}

fn board_task_ref_matches_candidate(
    reference: &str,
    candidate: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let task_id = candidate
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if task_id.trim().is_empty() {
        return false;
    }
    board_task_dependency_ref_matches_id(reference, task_id)
}

fn board_task_dependency_ref_matches_id(reference: &str, task_id: &str) -> bool {
    let normalized_ref = normalize_board_task_dependency_ref(reference);
    normalized_ref == task_id
        || normalized_ref.ends_with(task_id)
        || task_id.ends_with(normalized_ref.as_str())
}

fn normalize_board_task_dependency_ref(reference: &str) -> String {
    let trimmed = reference.trim().trim_matches('`');
    trimmed
        .strip_prefix("main_agent_board_task::")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn normalize_board_task_ref_fields(
    call: &ToolCall,
    index: usize,
    task: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    for (field, field_name) in board_task_refs::BOARD_TASK_REF_ARRAY_FIELDS {
        let Some(value) = task.get_mut(*field_name) else {
            continue;
        };
        let Some(items) = value.as_array_mut() else {
            continue;
        };
        let mut refs = Vec::with_capacity(items.len());
        for item in items.iter() {
            let Some(reference) = item.as_str() else {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!("tasks[{index}] `{field_name}` must contain only strings"),
                });
            };
            refs.push(reference.to_string());
        }
        let normalized =
            board_task_refs::canonicalize_refs_for_field(*field, &refs).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!("tasks[{index}] {}", err.message()),
                }
            })?;
        *items = normalized
            .into_iter()
            .map(serde_json::Value::String)
            .collect();
    }
    Ok(())
}

fn validate_mount_required_review_input_refs(
    call: &ToolCall,
    index: usize,
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    for reference in board_task_string_array(task, "input_artifact_refs") {
        if !crate::review_refs::starts_with_review_artifact_prefix(&reference) {
            continue;
        }
        let Some(path) =
            crate::review_refs::review_artifact_path_in_data_dir(&context.data_dir, &reference)
        else {
            continue;
        };
        if path.is_file() {
            continue;
        }
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] input_artifact_refs contains unresolved required review artifact `{reference}`; expected `{}`",
                path.display()
            ),
        });
    }
    Ok(())
}

fn validate_rejected_worker_evidence_authority_contract(
    call: &ToolCall,
    index: usize,
    workspace_root: &Path,
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let (Some(job_id), Some(stage_execution_id)) = (
        context.job_id.as_deref(),
        context.stage_execution_id.as_deref(),
    ) else {
        return Ok(());
    };
    let index_path = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence")
        .join("index.json");
    let Ok(content) = fs::read_to_string(index_path) else {
        return Ok(());
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(());
    };
    let rejected_agent_ids = value
        .get("entries")
        .and_then(|entries| entries.as_array())
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry.get("active_status").and_then(|value| value.as_str()) == Some("rejected")
        })
        .filter_map(|entry| entry.get("agent_id").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();
    for reference in board_task_string_array(task, "input_artifact_refs") {
        let normalized = reference.replace('\\', "/");
        let Some(agent_id) = rejected_agent_ids.iter().find(|agent_id| {
            normalized.contains(&format!(
                "/accepted_worker_evidence/materialized/{agent_id}/"
            ))
        }) else {
            continue;
        };
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] input_artifact_refs contains materialized artifact `{reference}` from rejected worker evidence `{agent_id}`. Rejected evidence is diagnostic history, not supporting input: move the ref to `review_target_evidence_refs` and require the worker to quarantine or supersede it explicitly before producing new evidence."
            ),
        });
    }
    Ok(())
}

fn timestamp_string_is_after(candidate: &str, baseline: &str) -> bool {
    let Ok(candidate) = candidate.trim().parse::<i128>() else {
        return candidate.trim() > baseline.trim();
    };
    let Ok(baseline) = baseline.trim().parse::<i128>() else {
        return true;
    };
    candidate > baseline
}

fn validate_board_task_publication_contract(
    call: &ToolCall,
    index: usize,
    task_type: &str,
    worker_role: &str,
    objective: &str,
    required_output_artifact_type: &str,
    required_output_fields: Option<&serde_json::Value>,
    acceptance_checks: Option<&serde_json::Value>,
    failure_signals: Option<&serde_json::Value>,
    evidence_standard: Option<&serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let prefix = format!("tasks[{index}]");
    validate_non_empty_board_task_string(call, &prefix, "task_type", task_type)?;
    validate_non_empty_board_task_string(call, &prefix, "worker_role", worker_role)?;
    validate_non_empty_board_task_string(call, &prefix, "objective", objective)?;
    validate_non_empty_board_task_string(
        call,
        &prefix,
        "required_output_artifact_type",
        required_output_artifact_type,
    )?;
    validate_non_empty_string_array_value(
        call,
        &prefix,
        "required_output_fields",
        required_output_fields,
    )?;
    validate_non_empty_string_array_value(call, &prefix, "acceptance_checks", acceptance_checks)?;
    validate_non_empty_string_array_value(call, &prefix, "failure_signals", failure_signals)?;
    let standard = evidence_standard
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    validate_non_empty_board_task_string(call, &prefix, "evidence_standard", standard)?;
    Ok(())
}

fn validate_board_task_artifact_repair_output_contract(
    call: &ToolCall,
    index: usize,
    context: &LocalToolExecutorContext,
    task_type: &str,
    worker_role: &str,
    required_output_artifact_type: &str,
) -> Result<(), ToolExecutionError> {
    let task_type_key = normalize_board_task_type_key(task_type);
    if worker_role != "research_synthesizer"
        || (!board_task_is_artifact_repair_task(task_type)
            && !task_type_key.contains("stage artifact synthesis"))
    {
        return Ok(());
    }
    let Some(expected) = context.stage_artifact_type.as_deref() else {
        return Ok(());
    };
    if required_output_artifact_type == expected {
        return Ok(());
    }
    Err(ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason: format!(
            "tasks[{index}] research_synthesizer task_type `{task_type}` produces the active stage artifact, so required_output_artifact_type must be `{expected}`, not `{required_output_artifact_type}`. Use task_type to express synthesis or repair work and the stage artifact type to express the candidate artifact that workers must produce."
        ),
    })
}

fn validate_failed_review_artifact_repair_contract(
    call: &ToolCall,
    index: usize,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let task_id = task
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let objective = task
        .get("objective")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let review_findings_refs = board_task_string_array(task, "review_findings_refs");
    let blocker_refs = board_task_string_array(task, "blocker_refs");
    let replacement_of_task_ids = board_task_string_array(task, "replacement_of_task_ids");
    let acceptance_checks = board_task_string_array(task, "acceptance_checks");
    let context = crate::board_task_contract::BoardTaskRepairContext {
        task_id,
        task_type,
        objective,
        review_findings_refs: &review_findings_refs,
        blocker_refs: &blocker_refs,
        replacement_of_task_ids: &replacement_of_task_ids,
    };
    if let Some(violation) =
        crate::board_task_contract::repair_task_contract_violation(&context, &acceptance_checks)
    {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("tasks[{index}] {violation}"),
        });
    }
    Ok(())
}

fn board_task_is_stage_synthesis_task(task_type: &str) -> bool {
    normalize_board_task_type_key(task_type).contains("synthesis")
}

fn validate_board_task_synthesis_role_contract(
    call: &ToolCall,
    index: usize,
    task_type: &str,
    worker_role: &str,
) -> Result<(), ToolExecutionError> {
    if !board_task_is_stage_synthesis_task(task_type) {
        return Ok(());
    }
    if worker_role == "research_synthesizer" {
        return Ok(());
    }
    Err(ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason: format!(
            "tasks[{index}] task_type `{task_type}` produces a stage synthesis artifact and must use worker_role `research_synthesizer`, not `{worker_role}`. Runtime only enforces the role contract; the main agent still decides when to publish the synthesis task and what evidence it must integrate."
        ),
    })
}

#[derive(Debug, Clone)]
struct BoardTaskSlotProfile {
    task_type: String,
    worker_role: String,
    required_output_artifact_type: String,
}

fn validate_board_task_stage_slot_identity_contract(
    call: &ToolCall,
    index: usize,
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<(), ToolExecutionError> {
    if !board_task_matches_active_stage(context, task) {
        return Ok(());
    }
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if board_task_is_artifact_repair_task(task_type) {
        return Ok(());
    }
    let task_type_key = normalize_board_task_type_key(task_type);
    let profiles = active_stage_slot_profiles(context, task, batch)?;
    let task_id = task
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let stage_execution_id = task
        .get("stage_execution_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let standard_prefix = format!("research_stage_task::{stage_execution_id}::");
    if task_id.starts_with(&standard_prefix) {
        if let Some(profile) = profiles
            .iter()
            .find(|profile| normalize_board_task_type_key(&profile.task_type) == task_type_key)
        {
            let canonical_task_id = format!(
                "{standard_prefix}{}",
                sanitize_file_component(&profile.task_type)
            );
            let review_findings_refs = board_task_string_array(task, "review_findings_refs");
            let blocker_refs = board_task_string_array(task, "blocker_refs");
            let replacement_of_task_ids = board_task_string_array(task, "replacement_of_task_ids");
            let repair_context = crate::board_task_contract::BoardTaskRepairContext {
                task_id,
                task_type,
                objective: task
                    .get("objective")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
                review_findings_refs: &review_findings_refs,
                blocker_refs: &blocker_refs,
                replacement_of_task_ids: &replacement_of_task_ids,
            };
            if crate::board_task_contract::task_is_repair_like(&repair_context)
                && task_id != canonical_task_id
            {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!(
                        "tasks[{index}] repair work for canonical stage slot `{}` uses alias task_id `{task_id}`. Reuse canonical task_id `{canonical_task_id}` and call `update_board_task` to revise that durable slot; keep repair lineage in review/blocker/replacement refs instead of creating another task file.",
                        profile.task_type
                    ),
                });
            }
        }
    }
    if let Some((profile, canonical_task_id)) = canonical_stage_slot_for_task_id(task, &profiles) {
        if task_type_key != normalize_board_task_type_key(&profile.task_type) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "tasks[{index}] task_id names canonical stage slot `{}` but task_type is `{task_type}`. Use task_type `{}` for that slot; use a custom non-stage-slot task_id only when publishing genuinely distinct work.",
                    profile.task_type, profile.task_type
                ),
            });
        }
        if task_id != canonical_task_id {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "tasks[{index}] task_id `{task_id}` is a non-canonical spelling of stage slot `{}`. Reuse canonical task_id `{canonical_task_id}` so republishing updates one durable slot instead of creating a duplicate.",
                    profile.task_type
                ),
            });
        }
    }
    if crate::board_task_contract::task_type_is_lifecycle_only(&task_type_key) {
        let matched_slot = unique_slot_profile_for_worker_and_output(task, &profiles)
            .map(|profile| profile.task_type.as_str())
            .unwrap_or("the concrete required worker task type");
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] task_type `{task_type}` is only a lifecycle label, not a stage evidence slot. Use task_type `{matched_slot}` when repairing that slot, and put repair context in `review_findings_refs`, `blocker_refs`, `replacement_of_task_ids`, `review_target_task_ids`, and `acceptance_checks`; use `artifact_repair` only for active stage artifact repair."
            ),
        });
    }

    let Some(profile) = profiles
        .iter()
        .find(|profile| normalize_board_task_type_key(&profile.task_type) == task_type_key)
    else {
        return Ok(());
    };
    let required_output_artifact_type = task
        .get("required_output_artifact_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if !required_output_type_is_lifecycle_report(required_output_artifact_type) {
        return Ok(());
    }
    if normalize_board_task_type_key(required_output_artifact_type)
        == normalize_board_task_type_key(&profile.required_output_artifact_type)
    {
        return Ok(());
    }
    Err(ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason: format!(
            "tasks[{index}] task_type `{}` is the stage evidence slot `{}`, so required_output_artifact_type must stay `{}`, not `{}`. Keep repair/reporting context in refs and acceptance checks; do not change the slot output type.",
            task_type, profile.task_type, profile.required_output_artifact_type, required_output_artifact_type
        ),
    })
}

fn canonical_stage_slot_for_task_id<'a>(
    task: &serde_json::Map<String, serde_json::Value>,
    profiles: &'a [BoardTaskSlotProfile],
) -> Option<(&'a BoardTaskSlotProfile, String)> {
    let task_id = task.get("task_id")?.as_str()?.trim();
    let stage_execution_id = task.get("stage_execution_id")?.as_str()?.trim();
    if stage_execution_id.is_empty() {
        return None;
    }
    let prefix = format!("research_stage_task::{stage_execution_id}::");
    let slot_component = task_id.strip_prefix(&prefix)?;
    let slot_key = normalize_board_task_type_key(slot_component);
    let profile = profiles
        .iter()
        .find(|profile| normalize_board_task_type_key(&profile.task_type) == slot_key)?;
    let canonical_task_id = format!("{prefix}{}", sanitize_file_component(&profile.task_type));
    Some((profile, canonical_task_id))
}

fn required_output_type_is_lifecycle_report(required_output_artifact_type: &str) -> bool {
    matches!(
        normalize_board_task_type_key(required_output_artifact_type).as_str(),
        "repair report" | "repair memo" | "repair note" | "cleanup report" | "blocked report"
    )
}

fn active_stage_slot_profiles(
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<Vec<BoardTaskSlotProfile>, ToolExecutionError> {
    let mut profiles = Vec::new();
    let stage_id = task
        .get("stage_id")
        .and_then(|value| value.as_str())
        .or(context.stage_id.as_deref())
        .unwrap_or_default()
        .trim()
        .to_string();
    let default_artifact_type = context
        .stage_artifact_type
        .as_deref()
        .unwrap_or_else(|| runtime::autonomous_research_stage_artifact_type(&stage_id));
    if !stage_id.is_empty() {
        push_slot_profile(
            &mut profiles,
            "acceptance standard setting",
            "stage_standard_setter",
            "stage_acceptance_rubric",
        );
        for task_type in runtime::autonomous_research_stage_worker_task_types(&stage_id) {
            push_slot_profile(
                &mut profiles,
                task_type,
                runtime::autonomous_research_stage_worker_role(&stage_id, task_type),
                runtime::autonomous_research_stage_task_required_output_artifact_type(
                    &stage_id,
                    task_type,
                    default_artifact_type,
                ),
            );
        }
    }
    for profile in adopted_stage_evidence_plan_slot_profiles(context, task) {
        push_slot_profile(
            &mut profiles,
            &profile.task_type,
            &profile.worker_role,
            &profile.required_output_artifact_type,
        );
    }
    for candidate in batch.iter().filter_map(|value| value.as_object()) {
        let Some(task_type) = required_slot_task_type_from_board_task(candidate) else {
            continue;
        };
        let worker_role = candidate
            .get("worker_role")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let required_output_artifact_type = candidate
            .get("required_output_artifact_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if !worker_role.trim().is_empty() && !required_output_artifact_type.trim().is_empty() {
            push_slot_profile(
                &mut profiles,
                &task_type,
                worker_role,
                required_output_artifact_type,
            );
        }
    }
    Ok(profiles)
}

fn push_slot_profile(
    profiles: &mut Vec<BoardTaskSlotProfile>,
    task_type: &str,
    worker_role: &str,
    required_output_artifact_type: &str,
) {
    if task_type.trim().is_empty()
        || worker_role.trim().is_empty()
        || required_output_artifact_type.trim().is_empty()
    {
        return;
    }
    if profiles.iter().any(|profile| {
        normalize_board_task_type_key(&profile.task_type)
            == normalize_board_task_type_key(task_type)
            && normalize_board_task_type_key(&profile.worker_role)
                == normalize_board_task_type_key(worker_role)
            && normalize_board_task_type_key(&profile.required_output_artifact_type)
                == normalize_board_task_type_key(required_output_artifact_type)
    }) {
        return;
    }
    profiles.push(BoardTaskSlotProfile {
        task_type: task_type.to_string(),
        worker_role: worker_role.to_string(),
        required_output_artifact_type: required_output_artifact_type.to_string(),
    });
}

fn unique_slot_profile_for_worker_and_output<'a>(
    task: &serde_json::Map<String, serde_json::Value>,
    profiles: &'a [BoardTaskSlotProfile],
) -> Option<&'a BoardTaskSlotProfile> {
    let worker_role = task
        .get("worker_role")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let required_output_artifact_type = task
        .get("required_output_artifact_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let mut matches = profiles.iter().filter(|profile| {
        normalize_board_task_type_key(&profile.worker_role)
            == normalize_board_task_type_key(worker_role)
            && normalize_board_task_type_key(&profile.required_output_artifact_type)
                == normalize_board_task_type_key(required_output_artifact_type)
    });
    let first = matches.next()?;
    if matches.any(|profile| {
        normalize_board_task_type_key(&profile.task_type)
            != normalize_board_task_type_key(&first.task_type)
    }) {
        return None;
    }
    Some(first)
}

fn adopted_stage_evidence_plan_slot_profiles(
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Vec<BoardTaskSlotProfile> {
    let stage_execution_id = task
        .get("stage_execution_id")
        .and_then(|value| value.as_str())
        .or(context.stage_execution_id.as_deref())
        .unwrap_or_default()
        .trim();
    if stage_execution_id.is_empty() {
        return Vec::new();
    }
    let path = context
        .data_dir
        .join("main-agent-board")
        .join("stage-evidence-plans")
        .join(format!(
            "latest_{}.json",
            sanitize_file_component(stage_execution_id)
        ));
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    value
        .get("evidence_requirements")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|requirement| {
            let task_type = requirement.get("task_type")?.as_str()?.trim();
            let worker_role = requirement.get("worker_role")?.as_str()?.trim();
            let required_output_artifact_type = requirement
                .get("required_output_artifact_type")?
                .as_str()?
                .trim();
            if task_type.is_empty()
                || worker_role.is_empty()
                || required_output_artifact_type.is_empty()
            {
                return None;
            }
            Some(BoardTaskSlotProfile {
                task_type: task_type.to_string(),
                worker_role: worker_role.to_string(),
                required_output_artifact_type: required_output_artifact_type.to_string(),
            })
        })
        .collect()
}

fn validate_board_task_required_slot_repair_type_contract(
    call: &ToolCall,
    index: usize,
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<(), ToolExecutionError> {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if board_task_is_artifact_repair_task(task_type) {
        return Ok(());
    }
    let task_type_key = normalize_board_task_type_key(task_type);
    if !board_task_is_repair_or_replacement_task(&task_type_key) {
        return Ok(());
    }

    let mut required_slot_types =
        required_slot_task_types_named_by_repair_alias(context, task_type, task, batch)?;
    for reference in board_task_string_array(task, "blocker_refs") {
        if let Some(required_task_type) =
            required_task_type_from_missing_task_blocker_ref(&reference)
        {
            push_unique_string(&mut required_slot_types, required_task_type);
        }
    }
    for field in [
        "review_target_task_ids",
        "replacement_of_task_ids",
        "supersedes_task_ids",
    ] {
        for reference in board_task_string_array(task, field) {
            if let Some(required_task_type) =
                required_task_type_for_board_task_ref(context, &reference, batch)?
            {
                push_unique_string(&mut required_slot_types, required_task_type);
            }
        }
    }

    for required_task_type in required_slot_types {
        if normalize_board_task_type_key(task_type)
            == normalize_board_task_type_key(&required_task_type)
        {
            continue;
        }
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] repairs required worker task type `{}` but task_type is `{task_type}`. Keep `task_type` equal to the required evidence slot and put repair context in `review_findings_refs`, `blocker_refs`, `replacement_of_task_ids`, `review_target_task_ids`, and `acceptance_checks`; use `artifact_repair` only for active stage artifact repair.",
                display_required_task_type(&required_task_type)
            ),
        });
    }
    Ok(())
}

fn required_slot_task_types_named_by_repair_alias(
    context: &LocalToolExecutorContext,
    task_type: &str,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<Vec<String>, ToolExecutionError> {
    let task_key = normalize_board_task_type_key(task_type);
    if task_key.is_empty() || task_key == "artifact repair" {
        return Ok(Vec::new());
    }
    let mut known_slot_types = active_stage_known_worker_slot_task_types(context, task, batch)?;
    known_slot_types
        .sort_by_key(|value| std::cmp::Reverse(normalize_board_task_type_key(value).len()));
    let mut matches = Vec::new();
    for known_slot_type in known_slot_types {
        let known_key = normalize_board_task_type_key(&known_slot_type);
        if known_key.is_empty() || known_key == task_key {
            continue;
        }
        if repair_alias_task_key_wraps_required_slot(&task_key, &known_key) {
            push_unique_string(&mut matches, known_slot_type);
        }
    }
    Ok(matches)
}

fn active_stage_known_worker_slot_task_types(
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<Vec<String>, ToolExecutionError> {
    let mut task_types = Vec::new();
    push_unique_string(&mut task_types, "acceptance standard setting".to_string());
    let stage_id = task
        .get("stage_id")
        .and_then(|value| value.as_str())
        .or(context.stage_id.as_deref())
        .unwrap_or_default()
        .trim()
        .to_string();
    if !stage_id.is_empty() {
        for task_type in runtime::autonomous_research_stage_worker_task_types(&stage_id) {
            push_unique_string(&mut task_types, task_type.to_string());
        }
    }
    for task_type in adopted_stage_evidence_plan_task_types(context, task) {
        push_unique_string(&mut task_types, task_type);
    }
    let stage_execution_id = task
        .get("stage_execution_id")
        .and_then(|value| value.as_str())
        .or(context.stage_execution_id.as_deref())
        .unwrap_or_default()
        .trim()
        .to_string();
    for candidate in batch.iter().filter_map(|value| value.as_object()) {
        let candidate_stage_execution_id = candidate
            .get("stage_execution_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        if !stage_execution_id.is_empty()
            && !candidate_stage_execution_id.is_empty()
            && candidate_stage_execution_id != stage_execution_id
        {
            continue;
        }
        if let Some(required_task_type) = required_slot_task_type_from_board_task(candidate) {
            push_unique_string(&mut task_types, required_task_type);
        }
    }
    let board_dir = context.data_dir.join("main-agent-board").join("tasks");
    if !board_dir.exists() {
        return Ok(task_types);
    }
    for entry in fs::read_dir(board_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(candidate) = value.as_object() else {
            continue;
        };
        let candidate_stage_execution_id = candidate
            .get("stage_execution_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        if !stage_execution_id.is_empty()
            && !candidate_stage_execution_id.is_empty()
            && candidate_stage_execution_id != stage_execution_id
        {
            continue;
        }
        if let Some(required_task_type) = required_slot_task_type_from_board_task(candidate) {
            push_unique_string(&mut task_types, required_task_type);
        }
    }
    Ok(task_types)
}

fn adopted_stage_evidence_plan_task_types(
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    let stage_execution_id = task
        .get("stage_execution_id")
        .and_then(|value| value.as_str())
        .or(context.stage_execution_id.as_deref())
        .unwrap_or_default()
        .trim();
    if stage_execution_id.is_empty() {
        return Vec::new();
    }
    let path = context
        .data_dir
        .join("main-agent-board")
        .join("stage-evidence-plans")
        .join(format!(
            "latest_{}.json",
            sanitize_file_component(stage_execution_id)
        ));
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    value
        .get("evidence_requirements")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|requirement| {
            requirement
                .get("task_type")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .collect()
}

fn repair_alias_task_key_wraps_required_slot(task_key: &str, required_slot_key: &str) -> bool {
    let suffix_match = task_key
        .strip_prefix(required_slot_key)
        .map(str::trim)
        .filter(|suffix| !suffix.is_empty())
        .is_some_and(repair_alias_affix_contains_only_repair_markers);
    if suffix_match {
        return true;
    }
    task_key
        .strip_suffix(required_slot_key)
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
        .is_some_and(repair_alias_affix_contains_only_repair_markers)
}

fn repair_alias_affix_contains_only_repair_markers(affix: &str) -> bool {
    let tokens = affix.split_whitespace().collect::<Vec<_>>();
    !tokens.is_empty()
        && tokens.iter().all(|token| {
            matches!(
                *token,
                "repair"
                    | "repairs"
                    | "repaired"
                    | "repairing"
                    | "replacement"
                    | "replace"
                    | "replaces"
                    | "replacing"
                    | "blocker"
                    | "fix"
                    | "fixes"
                    | "fixed"
                    | "fixing"
                    | "retry"
                    | "retries"
                    | "rerun"
                    | "rework"
                    | "revised"
                    | "revision"
                    | "followup"
                    | "follow"
                    | "up"
                    | "post"
                    | "review"
            )
        })
}

fn required_task_type_from_missing_task_blocker_ref(reference: &str) -> Option<String> {
    let marker = "missing_required_task_type:";
    let start = reference.find(marker)? + marker.len();
    let required = reference[start..]
        .split([':', '#', '?'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('`');
    if required.is_empty() {
        return None;
    }
    Some(required.replace('_', " ").replace('-', " "))
}

fn required_task_type_for_board_task_ref(
    context: &LocalToolExecutorContext,
    reference: &str,
    batch: &[serde_json::Value],
) -> Result<Option<String>, ToolExecutionError> {
    let normalized_ref = normalize_board_task_dependency_ref(reference);
    if normalized_ref.trim().is_empty() {
        return Ok(None);
    }
    for candidate in batch.iter().filter_map(|value| value.as_object()) {
        if board_task_ref_matches_candidate(&normalized_ref, candidate) {
            return Ok(required_slot_task_type_from_board_task(candidate));
        }
    }

    let board_dir = context.data_dir.join("main-agent-board").join("tasks");
    if !board_dir.exists() {
        return Ok(None);
    }
    let direct_path = board_dir.join(format!("{}.json", sanitize_file_component(&normalized_ref)));
    if direct_path.exists() {
        return required_task_type_from_board_task_file(&direct_path);
    }
    for entry in fs::read_dir(board_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(task) = value.as_object() else {
            continue;
        };
        if board_task_ref_matches_candidate(&normalized_ref, task) {
            return Ok(required_slot_task_type_from_board_task(task));
        }
    }
    Ok(None)
}

fn required_task_type_from_board_task_file(
    path: &Path,
) -> Result<Option<String>, ToolExecutionError> {
    let content = fs::read_to_string(path)?;
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(None);
    };
    Ok(value
        .as_object()
        .and_then(required_slot_task_type_from_board_task))
}

fn required_slot_task_type_from_board_task(
    task: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    if task_type.is_empty()
        || board_task_is_artifact_repair_task(task_type)
        || board_task_is_repair_or_replacement_task(&normalize_board_task_type_key(task_type))
    {
        return None;
    }
    Some(task_type.to_string())
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn display_required_task_type(task_type: &str) -> String {
    normalize_board_task_type_key(task_type)
}

fn validate_board_task_dependency_publication_contract(
    call: &ToolCall,
    index: usize,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<(), ToolExecutionError> {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let objective = task
        .get("objective")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    validate_board_task_target_contract(call, index, task)?;
    if !board_task_consumes_upstream_evidence(task_type, objective) {
        return Ok(());
    }
    if board_task_declares_upstream_dependency(task) {
        return Ok(());
    }
    let upstream_task_ids = board_task_batch_upstream_task_ids(task, batch, index);
    if upstream_task_ids.is_empty() {
        return Ok(());
    }
    Err(ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason: format!(
            "tasks[{index}] consumes upstream evidence but declares no dependency; add one of {} to `depends_on_task_ids` or `input_artifact_refs` so agent-team dataflow is explicit",
            upstream_task_ids.join(", ")
        ),
    })
}

fn validate_board_task_dependency_refs_resolve(
    call: &ToolCall,
    index: usize,
    context: &LocalToolExecutorContext,
    task_id: &str,
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
) -> Result<(), ToolExecutionError> {
    let dependencies = board_task_string_array(task, "depends_on_task_ids");
    if dependencies.is_empty() {
        return Ok(());
    }
    let batch_task_ids = board_task_batch_task_ids(batch);
    let persisted_task_ids = persisted_board_task_ids(&context.data_dir);
    for dependency in dependencies {
        let dependency = normalize_board_task_dependency_ref(&dependency);
        if dependency.is_empty() || dependency.starts_with("accepted_worker_evidence:") {
            continue;
        }
        if board_task_dependency_ref_matches_id(&dependency, task_id) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "tasks[{index}] depends_on_task_ids contains self dependency `{dependency}`; board tasks cannot wait on themselves"
                ),
            });
        }
        if batch_task_ids
            .iter()
            .any(|candidate| board_task_dependency_ref_matches_id(&dependency, candidate))
            || persisted_task_ids
                .iter()
                .any(|candidate| board_task_dependency_ref_matches_id(&dependency, candidate))
            || board_task_dependency_has_explicit_accepted_evidence_ref(task, &dependency)
        {
            continue;
        }
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] depends_on_task_ids references `{dependency}`, but no matching published board task exists and no same-task evidence target mounts that dependency. Use an existing board task id, publish the upstream task in the same batch, or bind the target evidence explicitly."
            ),
        });
    }
    Ok(())
}

fn board_task_batch_task_ids(batch: &[serde_json::Value]) -> Vec<String> {
    batch
        .iter()
        .filter_map(|task| task.get("task_id").and_then(|value| value.as_str()))
        .map(normalize_board_task_dependency_ref)
        .filter(|task_id| !task_id.is_empty())
        .collect()
}

fn persisted_board_task_ids(data_dir: &Path) -> Vec<String> {
    let task_dir = data_dir.join("main-agent-board").join("tasks");
    let Ok(entries) = fs::read_dir(task_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .filter_map(|value| {
            value
                .get("task_id")
                .and_then(|task_id| task_id.as_str())
                .map(normalize_board_task_dependency_ref)
        })
        .filter(|task_id| !task_id.is_empty())
        .collect()
}

fn board_task_dependency_has_explicit_accepted_evidence_ref(
    task: &serde_json::Map<String, serde_json::Value>,
    dependency: &str,
) -> bool {
    ["input_artifact_refs", "review_target_evidence_refs"]
        .iter()
        .flat_map(|field| board_task_string_array(task, field))
        .any(|reference| {
            let trimmed = reference.trim();
            if trimmed.starts_with("accepted_worker_evidence_index:") {
                return true;
            }
            trimmed
                .strip_prefix("accepted_worker_evidence_task:")
                .map(normalize_board_task_dependency_ref)
                .is_some_and(|reference| {
                    board_task_dependency_ref_matches_id(&reference, dependency)
                })
        })
        || board_task_string_array(task, "review_target_evidence_refs")
            .iter()
            .any(|reference| board_task_ref_is_concrete_upstream_dependency(reference))
}

fn validate_board_task_target_contract(
    call: &ToolCall,
    index: usize,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let task_type = task
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let task_type_key = normalize_board_task_type_key(task_type);
    let is_review = board_task_is_review_task(&task_type_key);
    let is_repair_or_replacement =
        !is_review && board_task_is_repair_or_replacement_task(&task_type_key);
    let supersedes_task_ids = board_task_string_array(task, "supersedes_task_ids");
    let replacement_of_task_ids = board_task_string_array(task, "replacement_of_task_ids");
    if let Some(violation) = crate::board_task_contract::self_reference_contract_violation(
        task.get("task_id")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown"),
        &supersedes_task_ids,
        &replacement_of_task_ids,
    ) {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("tasks[{index}] {violation}"),
        });
    }
    if is_review
        && board_task_string_array(task, "review_target_task_ids").is_empty()
        && board_task_string_array(task, "review_target_evidence_refs").is_empty()
    {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] is a review task but declares no `review_target_task_ids` or `review_target_evidence_refs`; `input_artifact_refs` is readable context and `depends_on_task_ids` is scheduling/dataflow, so reviewer work must bind to explicit target evidence"
            ),
        });
    }
    if is_repair_or_replacement
        && board_task_string_array(task, "blocker_refs").is_empty()
        && board_task_string_array(task, "review_findings_refs").is_empty()
        && replacement_of_task_ids.is_empty()
    {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "tasks[{index}] is a repair/replacement task but declares no `blocker_refs`, `review_findings_refs`, or `replacement_of_task_ids`; repair work must target a concrete blocker"
            ),
        });
    }
    Ok(())
}

fn board_task_is_review_task(task_key: &str) -> bool {
    task_key.contains("review")
        || task_key.contains("verdict")
        || task_key.contains("audit")
        || task_key.contains("quality gate")
}

fn board_task_is_repair_or_replacement_task(task_key: &str) -> bool {
    crate::board_task_contract::text_declares_repair_or_replacement(task_key)
}

fn validate_board_task_context_refs(
    call: &ToolCall,
    index: usize,
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Ok(());
    };
    let expected = format!("auto_research_job:{job_id}");
    for reference in board_task_string_array(task, "input_artifact_refs") {
        if let Some(actual_job_id) = reference.trim().strip_prefix("auto_research_job:") {
            if actual_job_id != job_id {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!(
                        "tasks[{index}] input_artifact_refs contains stale auto_research_job ref `{reference}`; use `{expected}` from the active context"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn stage_governance_artifact_target_path(path: &str) -> bool {
    let normalized = path.trim().trim_matches('`').replace('\\', "/");
    if !(normalized.starts_with("research/stages/") || normalized.contains("/research/stages/")) {
        return false;
    }
    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|value| value.to_str()),
        Some("stage_plan.md") | Some("stage_acceptance_rubric.md")
    )
}

fn stage_artifact_adoption_previous_rejection_reason(
    workspace_root: &Path,
    context: &LocalToolExecutorContext,
    stage_id: &str,
    stage_execution_id: &str,
    source_agent_id: &str,
    source_task_id: Option<&str>,
    source_ref: &str,
    source_artifact_path: Option<&str>,
    target_artifact_path: &str,
    snapshot_rebind_artifact_id: Option<&str>,
) -> Result<Option<String>, ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Ok(None);
    };
    let dir = workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("stage_artifact_adoption_rejections");
    if !dir.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if json_str(&manifest, "stage_id") != Some(stage_id)
            || json_str(&manifest, "stage_execution_id") != Some(stage_execution_id)
            || json_str(&manifest, "source_agent_id") != Some(source_agent_id)
            || json_str(&manifest, "target_artifact_path") != Some(target_artifact_path)
        {
            continue;
        }
        let manifest_task_id = json_str(&manifest, "source_task_id");
        let task_matches = source_task_id
            .zip(manifest_task_id)
            .map(|(requested, rejected)| requested == rejected)
            .unwrap_or(false);
        let source_matches = json_str(&manifest, "source_ref")
            .map(|rejected_ref| adoption_reference_matches(rejected_ref, source_ref))
            .unwrap_or(false)
            || source_artifact_path
                .zip(json_str(&manifest, "source_artifact_path"))
                .map(|(requested, rejected)| adoption_reference_matches(rejected, requested))
                .unwrap_or(false);
        if task_matches || source_matches {
            let reason = json_str(&manifest, "reason")
                .unwrap_or("previous runtime rejection")
                .to_string();
            if snapshot_rebind_artifact_id.is_some_and(|artifact_id| {
                reason.contains(&format!(
                    "invalid canonical artifact transition for {artifact_id}: baseline_visible -> adoption_requested"
                ))
            }) {
                continue;
            }
            let rejection_ref = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            return Ok(Some(format!(
                "adopt_stage_artifact rejected stale candidate: source agent/task/target already has a runtime rejection at `{rejection_ref}`: {reason}. Publish or update a replacement board task that produces a new clean canonical artifact before retrying adoption."
            )));
        }
    }
    Ok(None)
}

type SnapshotRebindSource = (String, String, Option<String>);

fn normalize_snapshot_rebind_source(
    call: &ToolCall,
    context: &LocalToolExecutorContext,
    source_agent_id: &str,
    source_task_id: Option<&str>,
    source_ref: &str,
    source_artifact_path: Option<&str>,
    target_artifact_path: &str,
    replacement_of_artifact_ids: &[String],
) -> Result<Option<SnapshotRebindSource>, ToolExecutionError> {
    if !replacement_of_artifact_ids
        .iter()
        .any(|artifact_id| artifact_id == source_ref)
    {
        return Ok(None);
    }
    let ledger = canonical_artifacts::load_ledger(&context.data_dir).map_err(|err| {
        ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("canonical artifact ledger could not be loaded: {err}"),
        }
    })?;
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.artifact_id == source_ref)
        .ok_or_else(|| ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("snapshot rebind references unknown canonical artifact `{source_ref}`"),
        })?;
    if entry.source_agent_id != source_agent_id
        || source_task_id.is_some_and(|task_id| entry.source_task_id.as_deref() != Some(task_id))
        || entry.target_artifact_path != target_artifact_path
        || source_artifact_path.is_some_and(|path| {
            entry
                .source_artifact_path
                .as_deref()
                .is_some_and(|ledger_path| !adoption_reference_matches(ledger_path, path))
        })
        || !entry.status.satisfies_file_dependency()
    {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "canonical artifact `{source_ref}` does not match the requested active source agent/task/target for snapshot rebind"
            ),
        });
    }
    Ok(Some((
        entry.artifact_id.clone(),
        entry.source_ref.clone(),
        entry.source_artifact_path.clone(),
    )))
}

#[allow(clippy::too_many_arguments)]
fn preflight_stage_artifact_adoption_tool_call(
    call: &ToolCall,
    workspace_root: &Path,
    context: &LocalToolExecutorContext,
    stage_id: &str,
    stage_execution_id: &str,
    source_agent_id: &str,
    source_task_id: Option<&str>,
    source_ref: &str,
    source_artifact_path: Option<&str>,
    target_artifact_path: &str,
    rationale: &str,
    evidence_refs: &[String],
    replacement_of_artifact_ids: &[String],
) -> Result<runtime::MainAgentStageArtifactAdoptionPreflight, ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: "`adopt_stage_artifact` requires an active autonomous research job context so runtime can validate accepted-worker provenance before recording the adoption".to_string(),
        });
    };
    let resolved = ResolvedProject {
        project_id: context.project_id.clone(),
        workspace_root: workspace_root.to_path_buf(),
        workspace_hash: String::new(),
        data_dir: context.data_dir.clone(),
        resolution_source: "tool_context".to_string(),
    };
    let job = runtime::read_autonomous_research_job_state(&resolved, job_id).map_err(|err| {
        ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "`adopt_stage_artifact` could not load active autonomous research job `{job_id}` for provenance preflight: {err}"
            ),
        }
    })?;
    runtime::preflight_main_agent_stage_artifact_adoption_from_args(
        &resolved,
        &job,
        stage_id,
        stage_execution_id,
        source_agent_id,
        source_task_id,
        source_ref,
        source_artifact_path,
        target_artifact_path,
        rationale,
        evidence_refs,
        replacement_of_artifact_ids,
    )
    .map_err(|reason| ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason,
    })
}

fn project_main_agent_worker_artifact_decisions_into_accepted_evidence(
    call: &ToolCall,
    workspace_root: &Path,
    context: &LocalToolExecutorContext,
) -> Result<Vec<String>, ToolExecutionError> {
    let Some(job_id) = context.job_id.as_deref() else {
        return Ok(Vec::new());
    };
    let resolved = ResolvedProject {
        project_id: context.project_id.clone(),
        workspace_root: workspace_root.to_path_buf(),
        workspace_hash: String::new(),
        data_dir: context.data_dir.clone(),
        resolution_source: "tool_context".to_string(),
    };
    let Ok(job) = runtime::read_autonomous_research_job_state(&resolved, job_id) else {
        return Ok(Vec::new());
    };
    let contract = runtime::autonomous_research_stage_contract(&resolved, &job);
    runtime::sync_main_agent_worker_artifact_decisions_into_accepted_worker_evidence(
        &resolved,
        &job,
        &contract,
    )
    .map_err(|reason| ToolExecutionError::InvalidArgument {
        tool: call.tool_name.clone(),
        reason: format!(
            "`record_worker_artifact_decision` wrote the decision but could not project it into accepted worker evidence: {reason}"
        ),
    })
}

fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
}

fn adoption_reference_matches(left: &str, right: &str) -> bool {
    let left = normalize_adoption_reference_for_match(left);
    let right = normalize_adoption_reference_for_match(right);
    !left.is_empty()
        && !right.is_empty()
        && (left == right || left.ends_with(&right) || right.ends_with(&left))
}

fn normalize_adoption_reference_for_match(value: &str) -> String {
    let normalized = value.trim().trim_matches('`').replace('\\', "/");
    for marker in ["/.pmcli/", "/research/"] {
        if let Some(index) = normalized.find(marker) {
            return normalized[index + 1..].trim_start_matches("./").to_string();
        }
    }
    normalized.trim_start_matches("./").to_string()
}

fn normalize_canonical_artifact_target(value: &str) -> String {
    value
        .trim()
        .trim_matches('`')
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn board_task_consumes_upstream_evidence(task_type: &str, objective: &str) -> bool {
    let task_key = normalize_board_task_type_key(task_type);
    if [
        "verification",
        "verify",
        "audit",
        "auditor",
        "cluster",
        "clustering",
        "comparison",
        "compare",
        "synthesis",
        "synthesize",
        "extraction",
        "extract",
        "repair",
        "replacement",
        "closest family survey",
    ]
    .iter()
    .any(|marker| task_key.contains(marker))
    {
        return true;
    }
    let objective_key = normalize_board_task_type_key(objective);
    [
        "consume",
        "consumes",
        "consuming",
        "using accepted",
        "from accepted",
        "accepted evidence",
        "accepted worker evidence",
        "worker evidence",
        "upstream",
        "previous",
        "found by",
        "produced by",
        "output of",
        "verify every",
        "audit",
        "normalize",
        "every source",
        "all source",
        "all row",
    ]
    .iter()
    .any(|marker| objective_key.contains(marker))
}

fn board_task_declares_upstream_dependency(
    task: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let has_dependency_id = !board_task_string_array(task, "depends_on_task_ids").is_empty();
    if has_dependency_id {
        return true;
    }
    board_task_string_array(task, "input_artifact_refs")
        .iter()
        .any(|reference| board_task_ref_is_concrete_upstream_dependency(reference))
}

fn board_task_ref_is_concrete_upstream_dependency(reference: &str) -> bool {
    board_task_refs::is_concrete_upstream_dependency_ref(reference)
}

fn board_task_string_array(
    task: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    task.get(field)
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn board_task_batch_upstream_task_ids(
    task: &serde_json::Map<String, serde_json::Value>,
    batch: &[serde_json::Value],
    index: usize,
) -> Vec<String> {
    let stage_id = task.get("stage_id").and_then(|value| value.as_str());
    let stage_execution_id = task
        .get("stage_execution_id")
        .and_then(|value| value.as_str());
    let priority = board_task_priority(task);
    let mut candidates = Vec::new();
    for (candidate_index, candidate) in batch.iter().enumerate() {
        if candidate_index == index {
            continue;
        }
        let Some(candidate) = candidate.as_object() else {
            continue;
        };
        if !same_optional_string(
            stage_id,
            candidate.get("stage_id").and_then(|value| value.as_str()),
        ) || !same_optional_string(
            stage_execution_id,
            candidate
                .get("stage_execution_id")
                .and_then(|value| value.as_str()),
        ) {
            continue;
        }
        let candidate_priority = board_task_priority(candidate);
        if candidate_priority >= priority && candidate_index >= index {
            continue;
        }
        let task_id = candidate
            .get("task_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                candidate
                    .get("task_type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("upstream_task")
            });
        candidates.push(task_id.to_string());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn same_optional_string(left: Option<&str>, right: Option<&str>) -> bool {
    left.unwrap_or_default().trim() == right.unwrap_or_default().trim()
}

fn board_task_priority(task: &serde_json::Map<String, serde_json::Value>) -> i64 {
    task.get("priority")
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
}

fn merge_duplicate_stage_task_type_before_publish(
    call: &ToolCall,
    board_dir: &Path,
    incoming_task_id: &str,
    incoming_stage_execution_id: Option<&str>,
    incoming_task: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<String>, ToolExecutionError> {
    let Some(stage_execution_id) =
        incoming_stage_execution_id.filter(|value| !value.trim().is_empty())
    else {
        return Ok(Vec::new());
    };
    if !board_dir.exists() {
        return Ok(Vec::new());
    }
    let incoming_task_type_key = incoming_task
        .get("task_type")
        .and_then(|value| value.as_str())
        .map(normalize_board_task_type_key)
        .unwrap_or_default();
    let incoming_task_path = board_dir.join(format!(
        "{}.json",
        sanitize_file_component(incoming_task_id)
    ));
    let mut merged_refs = Vec::new();
    for entry in fs::read_dir(board_dir)? {
        let path = entry?.path();
        if path == incoming_task_path
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let task_id = value
            .get("task_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        if task_id.trim().is_empty() || task_id == incoming_task_id {
            continue;
        }
        if value
            .get("merged_into")
            .and_then(|value| value.as_str())
            .is_some()
            || value.get("status").and_then(|value| value.as_str()) == Some("merged")
        {
            continue;
        }
        if value
            .get("stage_execution_id")
            .and_then(|value| value.as_str())
            != Some(stage_execution_id)
        {
            continue;
        }
        let existing_task_type_key = value
            .get("task_type")
            .and_then(|value| value.as_str())
            .map(normalize_board_task_type_key)
            .unwrap_or_default();
        if existing_task_type_key != incoming_task_type_key {
            continue;
        }
        let Some(existing_obj) = value.as_object() else {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("task {task_id} is not an object"),
            });
        };
        if !board_tasks_are_explicit_merge_duplicates(
            incoming_task_id,
            incoming_task,
            &task_id,
            existing_obj,
        ) {
            continue;
        }
        let Some(obj) = value.as_object_mut() else {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("task {task_id} is not an object"),
            });
        };
        obj.insert("status".to_string(), serde_json::json!("merged"));
        obj.insert(
            "merged_into".to_string(),
            serde_json::json!(incoming_task_id),
        );
        obj.insert(
            "merge_rationale".to_string(),
            serde_json::json!(
                "superseded by explicit replacement/supersedes metadata or matching semantic task key"
            ),
        );
        obj.insert(
            "updated_at".to_string(),
            serde_json::json!(timestamp_string()),
        );
        fs::write(
            &path,
            serde_json::to_string_pretty(&value).map_err(|err| {
                ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: err.to_string(),
                }
            })?,
        )?;
        merged_refs.push(format!("main_agent_board_task::{task_id}"));
    }
    Ok(merged_refs)
}

fn board_tasks_are_explicit_merge_duplicates(
    incoming_task_id: &str,
    incoming: &serde_json::Map<String, serde_json::Value>,
    existing_task_id: &str,
    existing: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    if board_task_string_array(incoming, "supersedes_task_ids")
        .iter()
        .any(|reference| board_task_dependency_ref_matches_id(reference, existing_task_id))
        || board_task_string_array(incoming, "replacement_of_task_ids")
            .iter()
            .any(|reference| board_task_dependency_ref_matches_id(reference, existing_task_id))
    {
        return true;
    }
    if board_task_string_array(existing, "supersedes_task_ids")
        .iter()
        .any(|reference| board_task_dependency_ref_matches_id(reference, incoming_task_id))
        || board_task_string_array(existing, "replacement_of_task_ids")
            .iter()
            .any(|reference| board_task_dependency_ref_matches_id(reference, incoming_task_id))
    {
        return true;
    }
    let incoming_evidence_set = incoming
        .get("current_evidence_set_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    let existing_evidence_set = existing
        .get("current_evidence_set_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    if !incoming_evidence_set.is_empty() && incoming_evidence_set == existing_evidence_set {
        return true;
    }
    let incoming_objective = incoming
        .get("objective")
        .and_then(|value| value.as_str())
        .map(normalize_board_task_type_key)
        .unwrap_or_default();
    let existing_objective = existing
        .get("objective")
        .and_then(|value| value.as_str())
        .map(normalize_board_task_type_key)
        .unwrap_or_default();
    !incoming_objective.is_empty() && incoming_objective == existing_objective
}

fn normalize_board_task_type_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_board_task_value_contract(
    call: &ToolCall,
    task_id: Option<&str>,
    value: &serde_json::Value,
) -> Result<(), ToolExecutionError> {
    let label = task_id
        .map(|task_id| format!("task `{task_id}`"))
        .unwrap_or_else(|| "task".to_string());
    for field in [
        "task_type",
        "worker_role",
        "objective",
        "required_output_artifact_type",
        "evidence_standard",
    ] {
        let field_value = value
            .get(field)
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        validate_non_empty_board_task_string(call, &label, field, field_value)?;
    }
    validate_non_empty_string_array_value(
        call,
        &label,
        "required_output_fields",
        value.get("required_output_fields"),
    )?;
    validate_non_empty_string_array_value(
        call,
        &label,
        "acceptance_checks",
        value.get("acceptance_checks"),
    )?;
    validate_non_empty_string_array_value(
        call,
        &label,
        "failure_signals",
        value.get("failure_signals"),
    )?;
    validate_required_canonical_artifacts_value(
        call,
        &label,
        value.get("required_canonical_artifacts"),
    )?;
    if let Some(task) = value.as_object() {
        validate_board_task_target_contract(call, 0, task)?;
    }
    Ok(())
}

fn validate_required_canonical_artifacts_value(
    call: &ToolCall,
    label: &str,
    value: Option<&serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let Some(value) = value else {
        return Ok(());
    };
    let Some(requirements) = value.as_array() else {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("{label} `required_canonical_artifacts` must be an array"),
        });
    };
    for (index, requirement) in requirements.iter().enumerate() {
        let Some(object) = requirement.as_object() else {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "{label} `required_canonical_artifacts[{index}]` must be an object"
                ),
            });
        };
        let target = object
            .get("target_artifact_path")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        let required_status = object
            .get("required_status")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        let dependency_kind = object
            .get("dependency_kind")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        let parsed = canonical_artifacts::RequiredCanonicalArtifact {
            target_artifact_path: target.to_string(),
            required_status: required_status.to_string(),
            dependency_kind: dependency_kind.to_string(),
            reason: object
                .get("reason")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string),
        };
        if !canonical_artifacts::required_canonical_artifact_is_valid(&parsed) {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "{label} `required_canonical_artifacts[{index}]` must name a safe workspace-relative canonical project path, required_status in baseline_visible/integration_verified/active_stage_evidence, and dependency_kind in file/runnable/stage_evidence; `.pmcli/input-bundles` refs are historical evidence, not canonical project dependencies"
                ),
            });
        }
    }
    Ok(())
}

fn validate_required_canonical_artifacts_not_active_stage_target(
    call: &ToolCall,
    label: &str,
    context: &LocalToolExecutorContext,
    task: &serde_json::Map<String, serde_json::Value>,
    value: Option<&serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    if !board_task_matches_active_stage(context, task) {
        return Ok(());
    }
    let Some(value) = value else {
        return Ok(());
    };
    let Some(requirements) = value.as_array() else {
        return Ok(());
    };
    let active_target = active_stage_artifact_path_for_context(context);
    let active_target_key = active_target
        .as_deref()
        .map(normalize_canonical_artifact_target_for_dependency_match);
    for (index, requirement) in requirements.iter().enumerate() {
        let Some(object) = requirement.as_object() else {
            continue;
        };
        let target = object
            .get("target_artifact_path")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let target_key = normalize_canonical_artifact_target_for_dependency_match(target);
        if target_key.starts_with("research/stages/")
            && (target_key.ends_with("/stage_plan.md")
                || target_key.ends_with("/stage_acceptance_rubric.md"))
        {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!(
                    "{label} `required_canonical_artifacts[{index}]` points at runtime-maintained stage support document `{target}`. Keep stage plans and acceptance rubrics in `input_artifact_refs`; reserve `required_canonical_artifacts` for external canonical project artifacts that already exist in the canonical artifact ledger."
                ),
            });
        }
        if active_target_key.as_deref() != Some(target_key.as_str()) {
            continue;
        }
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "{label} `required_canonical_artifacts[{index}]` points at the active stage artifact `{}`. Board tasks cannot require the artifact they are helping produce. Remove this entry from `required_canonical_artifacts`: if the worker should read the current candidate or failed review, put those refs in `input_artifact_refs` and `review_target_evidence_refs`; if the task follows an upstream worker, use `depends_on_task_ids`; reserve `required_canonical_artifacts` only for external artifacts that already exist before this task starts.",
                active_target.as_deref().unwrap_or_default()
            ),
        });
    }
    Ok(())
}

fn normalize_canonical_artifact_target_for_dependency_match(value: &str) -> String {
    normalize_canonical_artifact_target(value)
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
        .trim_end_matches('/')
        .to_string()
}

fn validate_non_empty_board_task_string(
    call: &ToolCall,
    label: &str,
    field: &str,
    value: &str,
) -> Result<(), ToolExecutionError> {
    if value.trim().is_empty() {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "{label} must include non-empty `{field}` for main-agent board publication"
            ),
        });
    }
    Ok(())
}

fn required_string_field_from_object(
    call: &ToolCall,
    object: &serde_json::Map<String, serde_json::Value>,
    index: usize,
    field: &str,
) -> Result<String, ToolExecutionError> {
    let Some(value) = object.get(field).and_then(|value| value.as_str()) else {
        return Err(ToolExecutionError::MissingArgument {
            tool: call.tool_name.clone(),
            argument: format!("evidence_requirements[{index}].{field}"),
        });
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!("evidence_requirements[{index}].{field} must not be empty"),
        });
    }
    Ok(trimmed.to_string())
}

fn validate_non_empty_string_array_value(
    call: &ToolCall,
    label: &str,
    field: &str,
    value: Option<&serde_json::Value>,
) -> Result<(), ToolExecutionError> {
    let Some(values) = value.and_then(|value| value.as_array()) else {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "{label} must include non-empty `{field}` array for main-agent board publication"
            ),
        });
    };
    if values.is_empty()
        || values.iter().any(|value| {
            value
                .as_str()
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        })
    {
        return Err(ToolExecutionError::InvalidArgument {
            tool: call.tool_name.clone(),
            reason: format!(
                "{label} must include non-empty `{field}` array for main-agent board publication"
            ),
        });
    }
    Ok(())
}

fn required_bool_argument(call: &ToolCall, name: &str) -> Result<bool, ToolExecutionError> {
    if let Some(value) = call.arguments.get(name).and_then(|value| value.as_bool()) {
        return Ok(value);
    }
    if let Some(raw) = call.parameters.get(name) {
        return parse_bool_argument(Some(raw.as_str()), false, name).map_err(|reason| {
            ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason,
            }
        });
    }
    Err(ToolExecutionError::MissingArgument {
        tool: call.tool_name.clone(),
        argument: name.to_string(),
    })
}

fn string_array_argument(call: &ToolCall, name: &str) -> Result<Vec<String>, ToolExecutionError> {
    if let Some(value) = call.arguments.get(name) {
        let Some(items) = value.as_array() else {
            return Err(ToolExecutionError::InvalidArgument {
                tool: call.tool_name.clone(),
                reason: format!("{name} must be an array of strings"),
            });
        };
        let mut strings = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let Some(raw) = item.as_str() else {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: format!("{name}[{index}] must be a string"),
                });
            };
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                strings.push(trimmed.to_string());
            }
        }
        return Ok(strings);
    }
    if let Some(raw) = call.parameters.get(name) {
        return Ok(raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect());
    }
    Err(ToolExecutionError::MissingArgument {
        tool: call.tool_name.clone(),
        argument: name.to_string(),
    })
}

fn optional_string_array_argument(
    call: &ToolCall,
    name: &str,
) -> Result<Vec<String>, ToolExecutionError> {
    if call.arguments.get(name).is_some() || call.parameters.contains_key(name) {
        return string_array_argument(call, name);
    }
    Ok(Vec::new())
}

fn normalize_review_artifact_refs_for_data_dir(data_dir: &Path, refs: Vec<String>) -> Vec<String> {
    refs.into_iter()
        .map(|reference| {
            review_refs::canonical_review_artifact_ref_in_data_dir(data_dir, &reference)
                .unwrap_or(reference)
        })
        .collect()
}

fn validate_control_request_arguments(call: &ToolCall) -> Result<(), ToolExecutionError> {
    match call.tool_name.as_str() {
        "request_review_rerun" => {
            required_string_argument(call, "stage_id")?;
            required_string_argument(call, "rationale")?;
            if string_array_argument(call, "readiness_refs")?.is_empty() {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: "readiness_refs must not be empty".to_string(),
                });
            }
        }
        "request_route_change" => {
            required_string_argument(call, "operation")?;
            required_string_argument(call, "target_stage_id")?;
            required_string_argument(call, "rationale")?;
            required_bool_argument(call, "cleanup_required")?;
        }
        "request_cleanup_plan" => {
            required_string_argument(call, "reason")?;
            if string_array_argument(call, "invalidated_refs")?.is_empty() {
                return Err(ToolExecutionError::InvalidArgument {
                    tool: call.tool_name.clone(),
                    reason: "invalidated_refs must not be empty".to_string(),
                });
            }
        }
        _ => {}
    }
    Ok(())
}

fn sanitize_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        "item".to_string()
    } else {
        sanitized
    }
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn terminate_child_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pgid = -(child.id() as libc::pid_t);
        unsafe {
            libc::kill(pgid, libc::SIGTERM);
        }
        thread::sleep(Duration::from_millis(100));
        if matches!(child.try_wait(), Ok(None)) {
            unsafe {
                libc::kill(pgid, libc::SIGKILL);
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

fn fetched_content_preview_for_llm(request_url: &str, final_url: &str, body: &str) -> String {
    let normalized = html_to_text_preview(body);
    let preview = safe_char_prefix(&normalized, MAX_LLM_TOOL_RESULT_CHARS);
    let omitted = normalized.len().saturating_sub(preview.len());
    if omitted == 0 {
        format!("Fetched URL: {final_url}\nRequested URL: {request_url}\n\n{preview}")
    } else {
        format!(
            "Fetched URL: {final_url}\nRequested URL: {request_url}\n\n{preview}\n\n[... compact fetch preview truncated: {omitted} chars omitted; use a narrower source URL or query if more detail is needed]"
        )
    }
}

fn html_to_text_preview(body: &str) -> String {
    let mut text = String::with_capacity(body.len().min(MAX_LLM_TOOL_RESULT_CHARS * 2));
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity = String::new();
    let mut last_was_space = true;

    for ch in body.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                if !last_was_space {
                    text.push('\n');
                    last_was_space = true;
                }
            }
            continue;
        }
        if in_entity {
            if ch == ';' {
                if let Some(decoded) = decode_basic_html_entity(&entity) {
                    push_preview_char(&mut text, decoded, &mut last_was_space);
                } else {
                    push_preview_char(&mut text, ' ', &mut last_was_space);
                }
                entity.clear();
                in_entity = false;
            } else if entity.len() < 16 {
                entity.push(ch);
            } else {
                entity.clear();
                in_entity = false;
                push_preview_char(&mut text, ' ', &mut last_was_space);
            }
            continue;
        }
        match ch {
            '<' => in_tag = true,
            '&' => {
                in_entity = true;
                entity.clear();
            }
            _ => push_preview_char(&mut text, ch, &mut last_was_space),
        }
    }
    text.trim().to_string()
}

fn push_preview_char(text: &mut String, ch: char, last_was_space: &mut bool) {
    if ch.is_whitespace() {
        if !*last_was_space {
            text.push(' ');
            *last_was_space = true;
        }
        return;
    }
    text.push(ch);
    *last_was_space = false;
}

fn decode_basic_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(decimal) = entity.strip_prefix('#') {
                decimal.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

fn enforce_fetch_network_policy(
    endpoint: &HttpEndpoint,
    allow_private_network: bool,
) -> Result<(), String> {
    let address = format!("{}:{}", endpoint.host, endpoint.port);
    let resolved = address
        .to_socket_addrs()
        .map_err(|err| format!("failed to resolve fetch URL host {address}: {err}"))?
        .collect::<Vec<_>>();
    if resolved.is_empty() {
        return Err(format!(
            "fetch URL host resolved to no addresses: {address}"
        ));
    }
    if !allow_private_network {
        for socket in resolved {
            if is_sensitive_ip(socket.ip()) {
                return Err(format!(
                    "fetch URL resolves to private network address {}; pass allow_private_network=true only for explicit local fixtures",
                    socket.ip()
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct FetchResponse {
    status_code: u16,
    final_url: String,
    redirect_chain: Vec<String>,
    body: Vec<u8>,
}

fn fetch_url_with_policy(
    initial_url: &str,
    allow_private_network: bool,
) -> Result<FetchResponse, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent("research-cli/0.1")
        .accept("*/*")
        .build()
        .into();
    let mut current_url = initial_url.to_string();
    let mut redirect_chain = Vec::new();

    for _ in 0..=MAX_FETCH_REDIRECTS {
        let endpoint = parse_http_url(&current_url)?;
        enforce_fetch_network_policy(&endpoint, allow_private_network)?;
        let mut response = agent
            .get(&endpoint.url)
            .call()
            .map_err(|err| format!("fetch request failed: {err}"))?;
        let status_code = response.status().as_u16();

        if (300..400).contains(&status_code) {
            let location = response
                .headers()
                .get("location")
                .ok_or_else(|| format!("fetch redirect {status_code} missing Location header"))?
                .to_str()
                .map_err(|err| format!("fetch redirect Location header was invalid: {err}"))?;
            let next_url = resolve_redirect_url(&endpoint, location)?;
            redirect_chain.push(next_url.clone());
            current_url = next_url;
            continue;
        }

        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_FETCH_BYTES as u64)
            .read_to_vec()
            .map_err(|err| format!("fetch response body failed: {err}"))?;
        return Ok(FetchResponse {
            status_code,
            final_url: response.get_uri().to_string(),
            redirect_chain,
            body,
        });
    }

    Err(format!(
        "fetch exceeded redirect limit of {MAX_FETCH_REDIRECTS}"
    ))
}

fn paper_search_via_source(
    query: &str,
    source: &str,
    max_results: usize,
    year_from: Option<u64>,
    year_to: Option<u64>,
) -> Result<Vec<serde_json::Value>, String> {
    match source {
        "arxiv" => paper_search_arxiv(query, max_results, year_from, year_to),
        "semantic_scholar" => paper_search_semantic_scholar(query, max_results, year_from, year_to),
        "crossref" => paper_search_crossref(query, max_results, year_from, year_to),
        other => Err(format!("unsupported paper search source: {other}")),
    }
}

fn paper_search_arxiv(
    query: &str,
    max_results: usize,
    year_from: Option<u64>,
    year_to: Option<u64>,
) -> Result<Vec<serde_json::Value>, String> {
    let url = format!(
        "https://export.arxiv.org/api/query?search_query=all:{}&start=0&max_results={}",
        percent_encode_component(query),
        max_results
    );
    let fetched = fetch_url_with_policy(&url, false)?;
    if !(200..300).contains(&fetched.status_code) {
        return Err(format!("arxiv returned HTTP {}", fetched.status_code));
    }
    let body = String::from_utf8_lossy(&fetched.body);
    Ok(parse_arxiv_paper_candidates(&body, year_from, year_to))
}

fn parse_arxiv_paper_candidates(
    body: &str,
    year_from: Option<u64>,
    year_to: Option<u64>,
) -> Vec<serde_json::Value> {
    let mut candidates = Vec::new();
    for entry in body.split("<entry>").skip(1) {
        let entry = entry.split("</entry>").next().unwrap_or(entry);
        let title = normalize_xml_text(&xml_tag_text(entry, "title").unwrap_or_default());
        if title.is_empty() {
            continue;
        }
        let abstract_text = normalize_xml_text(&xml_tag_text(entry, "summary").unwrap_or_default());
        let url = xml_tag_text(entry, "id").unwrap_or_default();
        let published = xml_tag_text(entry, "published").unwrap_or_default();
        let year = published
            .get(0..4)
            .and_then(|value| value.parse::<u64>().ok());
        if !paper_year_matches(year, year_from, year_to) {
            continue;
        }
        let authors = xml_tag_texts(entry, "name")
            .into_iter()
            .map(|author| normalize_xml_text(&author))
            .filter(|author| !author.is_empty())
            .collect::<Vec<_>>();
        let external_id = url
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        candidates.push(serde_json::json!({
            "title": title,
            "authors": authors,
            "year": year,
            "abstract": abstract_text,
            "source": "arxiv",
            "url": url,
            "external_id": if external_id.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(external_id) },
            "venue": "arXiv",
            "citation_count": serde_json::Value::Null,
            "confidence_note": "Retrieved from arXiv Atom metadata; worker must still judge relevance and claim support."
        }));
    }
    candidates
}

fn paper_search_semantic_scholar(
    query: &str,
    max_results: usize,
    year_from: Option<u64>,
    year_to: Option<u64>,
) -> Result<Vec<serde_json::Value>, String> {
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={}&limit={}&fields=title,authors,year,abstract,url,venue,citationCount,externalIds",
        percent_encode_component(query),
        max_results
    );
    let fetched = fetch_url_with_policy(&url, false)?;
    if !(200..300).contains(&fetched.status_code) {
        return Err(format!(
            "semantic_scholar returned HTTP {}",
            fetched.status_code
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&fetched.body)
        .map_err(|err| format!("semantic_scholar JSON parse failed: {err}"))?;
    let mut candidates = Vec::new();
    for item in value
        .get("data")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        let title = item
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        if title.is_empty() {
            continue;
        }
        let year = item.get("year").and_then(|value| value.as_u64());
        if !paper_year_matches(year, year_from, year_to) {
            continue;
        }
        let authors = item
            .get("authors")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|author| author.get("name").and_then(|value| value.as_str()))
            .map(str::trim)
            .filter(|author| !author.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        candidates.push(serde_json::json!({
            "title": title,
            "authors": authors,
            "year": year,
            "abstract": item.get("abstract").and_then(|value| value.as_str()).unwrap_or_default(),
            "source": "semantic_scholar",
            "url": item.get("url").and_then(|value| value.as_str()).unwrap_or_default(),
            "external_id": item.get("externalIds").cloned().unwrap_or(serde_json::Value::Null),
            "venue": item.get("venue").and_then(|value| value.as_str()).unwrap_or_default(),
            "citation_count": item.get("citationCount").cloned().unwrap_or(serde_json::Value::Null),
            "confidence_note": "Retrieved from Semantic Scholar metadata; worker must still verify bibliographic details and relevance."
        }));
    }
    Ok(candidates)
}

fn paper_search_crossref(
    query: &str,
    max_results: usize,
    year_from: Option<u64>,
    year_to: Option<u64>,
) -> Result<Vec<serde_json::Value>, String> {
    let url = format!(
        "https://api.crossref.org/works?query={}&rows={}&select=title,author,issued,published-print,published-online,URL,DOI,container-title,is-referenced-by-count,abstract",
        percent_encode_component(query),
        max_results
    );
    let fetched = fetch_url_with_policy(&url, false)?;
    if !(200..300).contains(&fetched.status_code) {
        return Err(format!("crossref returned HTTP {}", fetched.status_code));
    }
    let value: serde_json::Value = serde_json::from_slice(&fetched.body)
        .map_err(|err| format!("crossref JSON parse failed: {err}"))?;
    let mut candidates = Vec::new();
    for item in value
        .get("message")
        .and_then(|value| value.get("items"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        let title = item
            .get("title")
            .and_then(|value| value.as_array())
            .and_then(|titles| titles.first())
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        if title.is_empty() {
            continue;
        }
        let year = crossref_year(item);
        if !paper_year_matches(year, year_from, year_to) {
            continue;
        }
        let authors = item
            .get("author")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .map(crossref_author_name)
            .filter(|author| !author.is_empty())
            .collect::<Vec<_>>();
        let venue = item
            .get("container-title")
            .and_then(|value| value.as_array())
            .and_then(|venues| venues.first())
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        candidates.push(serde_json::json!({
            "title": title,
            "authors": authors,
            "year": year,
            "abstract": item.get("abstract").and_then(|value| value.as_str()).map(html_to_text_preview).unwrap_or_default(),
            "source": "crossref",
            "url": item.get("URL").and_then(|value| value.as_str()).unwrap_or_default(),
            "external_id": item.get("DOI").and_then(|value| value.as_str()).unwrap_or_default(),
            "venue": venue,
            "citation_count": item.get("is-referenced-by-count").cloned().unwrap_or(serde_json::Value::Null),
            "confidence_note": "Retrieved from Crossref metadata; worker must still inspect source pages for claim support."
        }));
    }
    Ok(candidates)
}

fn paper_year_matches(year: Option<u64>, year_from: Option<u64>, year_to: Option<u64>) -> bool {
    if let Some(from) = year_from {
        if year.is_some_and(|year| year < from) {
            return false;
        }
    }
    if let Some(to) = year_to {
        if year.is_some_and(|year| year > to) {
            return false;
        }
    }
    true
}

fn crossref_year(item: &serde_json::Value) -> Option<u64> {
    for key in ["issued", "published-print", "published-online"] {
        let year = item
            .get(key)
            .and_then(|value| value.get("date-parts"))
            .and_then(|value| value.as_array())
            .and_then(|parts| parts.first())
            .and_then(|value| value.as_array())
            .and_then(|parts| parts.first())
            .and_then(|value| value.as_u64());
        if year.is_some() {
            return year;
        }
    }
    None
}

fn crossref_author_name(author: &serde_json::Value) -> String {
    let given = author
        .get("given")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    let family = author
        .get("family")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim();
    match (given.is_empty(), family.is_empty()) {
        (true, true) => String::new(),
        (true, false) => family.to_string(),
        (false, true) => given.to_string(),
        (false, false) => format!("{given} {family}"),
    }
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn xml_tag_text(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = body.find(&open)? + open.len();
    let end = body[start..].find(&close)? + start;
    Some(body[start..end].to_string())
}

fn xml_tag_texts(body: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut remaining = body;
    let mut values = Vec::new();
    while let Some(start_offset) = remaining.find(&open) {
        let start = start_offset + open.len();
        let Some(end_offset) = remaining[start..].find(&close) else {
            break;
        };
        values.push(remaining[start..start + end_offset].to_string());
        remaining = &remaining[start + end_offset + close.len()..];
    }
    values
}

fn normalize_xml_text(value: &str) -> String {
    decode_xml_entities(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn resolve_redirect_url(endpoint: &HttpEndpoint, location: &str) -> Result<String, String> {
    if location.chars().any(char::is_control) || location.chars().any(char::is_whitespace) {
        return Err(
            "fetch redirect Location contains control characters or whitespace".to_string(),
        );
    }
    if location.starts_with("http://") || location.starts_with("https://") {
        parse_http_url(location)?;
        return Ok(location.to_string());
    }
    if location.starts_with("//") {
        let redirected = format!("{}:{location}", endpoint.scheme);
        parse_http_url(&redirected)?;
        return Ok(redirected);
    }
    if location.starts_with('/') {
        let redirected = format!("{}://{}{}", endpoint.scheme, endpoint.host_header, location);
        parse_http_url(&redirected)?;
        return Ok(redirected);
    }
    let base = endpoint
        .path
        .rsplit_once('/')
        .map(|(prefix, _)| if prefix.is_empty() { "/" } else { prefix })
        .unwrap_or("/");
    let redirected = if base == "/" {
        format!(
            "{}://{}/{}",
            endpoint.scheme, endpoint.host_header, location
        )
    } else {
        format!(
            "{}://{}{}/{}",
            endpoint.scheme, endpoint.host_header, base, location
        )
    };
    parse_http_url(&redirected)?;
    Ok(redirected)
}

fn is_sensitive_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_multicast()
                || ip.is_unspecified()
                || matches!(ip.segments()[0] & 0xfe00, 0xfc00)
                || matches!(ip.segments()[0] & 0xffc0, 0xfe80)
        }
    }
}

fn nearest_existing_parent(path: &Path) -> Result<PathBuf, ToolExecutionError> {
    let mut cursor = path;
    loop {
        if cursor.exists() {
            return Ok(cursor.to_path_buf());
        }
        cursor = cursor.parent().ok_or_else(|| {
            ToolExecutionError::WorkspaceBoundary(format!(
                "target parent does not exist inside workspace boundary: {}",
                path.display()
            ))
        })?;
    }
}

fn collect_files(
    root: &Path,
    canonical_workspace_root: &Path,
    files: &mut Vec<String>,
    limit: usize,
) -> Result<(), ToolExecutionError> {
    if files.len() >= limit {
        return Ok(());
    }
    if root.is_file() {
        let canonical = root.canonicalize()?;
        if let Ok(relative) = canonical.strip_prefix(canonical_workspace_root) {
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
        return Ok(());
    }
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        if files.len() >= limit {
            break;
        }
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if SKIPPED_LIST_DIRS.contains(&file_name.as_ref()) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let canonical = path.canonicalize()?;
            if !canonical.starts_with(canonical_workspace_root) {
                continue;
            }
        }
        if path.is_dir() {
            collect_files(&path, canonical_workspace_root, files, limit)?;
        } else if path.is_file() {
            let canonical = path.canonicalize()?;
            if let Ok(relative) = canonical.strip_prefix(canonical_workspace_root) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

fn ensure_text_file(path: &Path) -> Result<(), ToolExecutionError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_READ_SIZE {
        return Err(ToolExecutionError::InvalidArgument {
            tool: "file".to_string(),
            reason: format!(
                "file is too large ({} bytes, max {} bytes)",
                metadata.len(),
                MAX_READ_SIZE
            ),
        });
    }
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 8192];
    let bytes_read = file.read(&mut buffer)?;
    if buffer[..bytes_read].contains(&0) {
        return Err(ToolExecutionError::InvalidArgument {
            tool: "file".to_string(),
            reason: "file appears to be binary".to_string(),
        });
    }
    Ok(())
}

pub fn is_read_only_shell_command(command: &str) -> bool {
    let first_token = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    matches!(
        first_token,
        "cat"
            | "head"
            | "tail"
            | "less"
            | "more"
            | "wc"
            | "ls"
            | "find"
            | "grep"
            | "rg"
            | "awk"
            | "sed"
            | "echo"
            | "printf"
            | "which"
            | "where"
            | "whoami"
            | "pwd"
            | "env"
            | "printenv"
            | "date"
            | "df"
            | "du"
            | "free"
            | "uptime"
            | "uname"
            | "file"
            | "stat"
            | "diff"
            | "sort"
            | "uniq"
            | "tr"
            | "cut"
            | "paste"
            | "test"
            | "true"
            | "false"
            | "type"
            | "readlink"
            | "realpath"
            | "basename"
            | "dirname"
            | "sha256sum"
            | "md5sum"
            | "xxd"
            | "hexdump"
            | "od"
            | "strings"
            | "tree"
            | "jq"
            | "yq"
            | "git"
    ) && !command.contains(" > ")
        && !command.contains(" >> ")
        && !command.contains(" 2> ")
        && !command.contains("| tee")
        && !command.contains("-i ")
        && !command.contains("--in-place")
        && !command.contains(" rm ")
        && !command.contains(" touch ")
        && !command.contains(" mv ")
        && !command.contains(" cp ")
}

fn is_worker_shell_command(command: &str) -> bool {
    let first_token = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    let allowed_first_token = matches!(
        first_token,
        "cat"
            | "head"
            | "tail"
            | "wc"
            | "ls"
            | "find"
            | "grep"
            | "rg"
            | "awk"
            | "sed"
            | "echo"
            | "printf"
            | "pwd"
            | "env"
            | "printenv"
            | "date"
            | "file"
            | "stat"
            | "diff"
            | "sort"
            | "uniq"
            | "tr"
            | "cut"
            | "paste"
            | "test"
            | "true"
            | "false"
            | "type"
            | "readlink"
            | "realpath"
            | "basename"
            | "dirname"
            | "sha256sum"
            | "md5sum"
            | "jq"
            | "cargo"
            | "pytest"
            | "python"
            | "python3"
            | "node"
            | "npm"
    );
    allowed_first_token
        && !command.contains(" > ")
        && !command.contains(" >> ")
        && !command.contains(" 2> ")
        && !command.contains("| tee")
        && !command.contains("-i ")
        && !command.contains("--in-place")
        && !command_contains_forbidden_worker_shell_token(command)
}

fn command_contains_forbidden_worker_shell_token(command: &str) -> bool {
    command
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '&' | '|' | '(' | ')'))
        .filter(|token| !token.is_empty())
        .any(|token| {
            matches!(
                token,
                "rm" | "rmdir"
                    | "mv"
                    | "cp"
                    | "touch"
                    | "mkdir"
                    | "chmod"
                    | "chown"
                    | "ln"
                    | "git"
                    | "curl"
                    | "wget"
                    | "bash"
                    | "sh"
                    | "zsh"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{
        agent_team_worker_safe_tool_names, agent_team_worker_tool_definitions_for_policy,
        enforce_fetch_network_policy, failure_text_requires_evidence_gap_worker_repair,
        format_tool_result_for_llm, is_worker_shell_command, main_agent_safe_tool_names_for_mode,
        main_agent_tool_definitions_for_mode, normalize_snapshot_rebind_source,
        parse_arxiv_paper_candidates, parse_http_url, sanitize_file_component,
        stage_artifact_adoption_previous_rejection_reason, LocalToolExecutor,
        LocalToolExecutorContext, ToolCall, ToolExecutionError, ToolResult,
    };
    use crate::canonical_artifacts;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TOOL_TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_tool_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "research_cli_tool_test_{}_{}_{}",
            label,
            std::process::id(),
            TOOL_TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(root.join(".pmcli")).expect("tool workspace should create");
        root
    }

    fn tool_test_sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn adoption_snapshot_only_failure_does_not_require_evidence_gap_worker() {
        assert!(!failure_text_requires_evidence_gap_worker_repair(
            Some("literature_evidence_failure"),
            "stage_artifact_adoption_snapshot: adoption evidence snapshot changed",
        ));
    }

    fn write_passing_job_state(
        data_dir: &std::path::Path,
        job_id: &str,
        stage_execution_id: &str,
        review_id: &str,
    ) {
        let job_dir = data_dir.join("research").join("jobs").join(job_id);
        fs::create_dir_all(&job_dir).expect("job dir should create");
        fs::write(
            job_dir.join("job.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research_job_state.v1",
                "job_id": job_id,
                "stage_execution_id": stage_execution_id,
                "last_review": {
                    "review_id": review_id,
                    "verdict": "pass",
                    "score": 88,
                    "created_at": "2",
                    "response_text": "verdict: pass\nscore: 88"
                }
            }))
            .expect("job state should serialize"),
        )
        .expect("job state should write");
    }

    fn write_evidence_gap_job_state(
        data_dir: &std::path::Path,
        job_id: &str,
        stage_id: &str,
        stage_execution_id: &str,
        review_id: &str,
    ) {
        let job_dir = data_dir.join("research").join("jobs").join(job_id);
        fs::create_dir_all(&job_dir).expect("job dir should create");
        fs::write(
            job_dir.join("job.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research_job_state.v1",
                "job_id": job_id,
                "stage_execution_id": stage_execution_id,
                "last_review": {
                    "review_id": review_id,
                    "verdict": "fail",
                    "score": 58,
                    "created_at": "2",
                    "response_text": "verdict: fail\nfailure class: evidence_gap_blocker\nRequired repairs: mount unexamined closest prior families with targeted evidence-gap worker evidence before synthesis."
                },
                "obligations": [{
                    "obligation_id": "obl_evidence_gap",
                    "kind": "failed_stage_review",
                    "stage_id": stage_id,
                    "stage_execution_id": stage_execution_id,
                    "status": "open",
                    "blocking": true,
                    "review_id": review_id,
                    "failure_class": "evidence_gap_blocker",
                    "detail": "unexamined closest prior family requires targeted evidence work"
                }]
            }))
            .expect("job state should serialize"),
        )
        .expect("job state should write");
    }

    fn write_adoption_snapshot_only_job_state(
        data_dir: &std::path::Path,
        job_id: &str,
        stage_id: &str,
        stage_execution_id: &str,
        review_id: &str,
    ) {
        let job_dir = data_dir.join("research").join("jobs").join(job_id);
        fs::create_dir_all(&job_dir).expect("job dir should create");
        fs::write(
            job_dir.join("job.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research_job_state.v1",
                "job_id": job_id,
                "stage_execution_id": stage_execution_id,
                "last_review": {
                    "review_id": review_id,
                    "verdict": "fail",
                    "score": 200,
                    "created_at": "2",
                    "response_text": "verdict: fail\nFAIL stage_artifact_adoption_snapshot\nFAIL blocking_stage_gate: stage_artifact_adoption_snapshot"
                },
                "obligations": [{
                    "obligation_id": "obl_snapshot",
                    "kind": "stage_gate_blocking_failure",
                    "stage_id": stage_id,
                    "stage_execution_id": stage_execution_id,
                    "status": "open",
                    "blocking": true,
                    "review_id": review_id,
                    "failure_class": "literature_evidence_failure",
                    "detail": "stage_artifact_adoption_snapshot: adoption evidence snapshot changed"
                }]
            }))
            .expect("job state should serialize"),
        )
        .expect("job state should write");
    }

    fn write_post_review_evidence_index(
        workspace_root: &std::path::Path,
        job_id: &str,
        stage_execution_id: &str,
    ) {
        let dir = workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join(stage_execution_id)
            .join("accepted_worker_evidence");
        fs::create_dir_all(&dir).expect("accepted evidence dir should create");
        fs::write(
            dir.join("index.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "accepted_worker_evidence_index.v1",
                "generated_at": "3",
                "entries": [{
                    "created_at": "3",
                    "task_type": "closest-prior search",
                    "worker_role": "novelty_reviewer",
                    "required_output_artifact_type": "novelty_report",
                    "task_id": "research_stage_task::stage_1::closest_prior_search"
                }]
            }))
            .expect("accepted evidence should serialize"),
        )
        .expect("accepted evidence should write");
    }

    fn write_tool_adoption_job_state(
        data_dir: &std::path::Path,
        project_id: &str,
        job_id: &str,
        stage_execution_id: &str,
    ) {
        let job_dir = data_dir.join("research").join("jobs").join(job_id);
        fs::create_dir_all(&job_dir).expect("job dir should create");
        let mut job = serde_json::json!({
            "schema_version": "autonomous_research_job.v1",
            "job_id": job_id,
            "project_id": project_id,
            "prompt": "tool adoption fixture",
            "status": "running",
            "phase": "main_agent_round",
            "automation_mode": "full_auto",
            "permission_mode": "workspace-write",
            "stage_task_semantic_review_mode": "provider",
            "created_at": "1",
            "updated_at": "1",
            "max_ticks": 10,
            "ticks_completed": 1,
            "interval_ms": 1,
            "max_runtime_ms": 1000,
            "max_review_rounds": 5,
            "review_rounds_completed": 0,
            "report_path": format!("papers/{job_id}/main.tex"),
            "job_state_path": job_dir.join("job.json").display().to_string(),
            "job_events_path": job_dir.join("events.jsonl").display().to_string(),
            "runner_log_path": job_dir.join("runner.log").display().to_string(),
            "thread_id": "thread_fixture",
            "stage_execution_id": stage_execution_id,
            "last_error": null,
            "last_review": null,
            "last_loop_closure": null,
            "tick_summaries": [],
            "artifact_refs": [],
            "warnings": [],
            "cleanup_plan_ids": [],
            "failure_patterns": [],
            "obligations": [],
            "continuity_packets": [],
            "provider_faults": [],
            "active_provider_fault": null,
            "provider_rounds": []
        });
        job["provider_rounds"] = serde_json::json!([{
            "round_index": 1,
            "artifact_path": "research/auto/main-agent-round-1.md",
            "provider_id": "fixture",
            "model": "fixture-model",
            "execution_mode": "test",
            "iterations": 1,
            "tool_calls_made": 0,
            "finish_reason": "completed",
            "content_sha256": "fixture"
        }]);
        fs::write(
            job_dir.join("job.json"),
            serde_json::to_string_pretty(&job).expect("job state should serialize"),
        )
        .expect("job state should write");
    }

    fn write_tool_worker_candidate_manifest(
        workspace_root: &std::path::Path,
        agent_id: &str,
        candidate_path: &str,
        candidate_body: &str,
    ) -> String {
        let worktree = workspace_root
            .join(".pmcli")
            .join("fixture-worker-worktrees")
            .join(agent_id);
        let worktree_candidate = worktree.join(candidate_path);
        if let Some(parent) = worktree_candidate.parent() {
            fs::create_dir_all(parent).expect("candidate parent should create");
        }
        fs::write(&worktree_candidate, candidate_body).expect("candidate should write");
        let archive_path = workspace_root
            .join(".pmcli")
            .join("agents")
            .join(agent_id)
            .join("candidate_archive")
            .join(candidate_path.replace(['/', '\\'], "_"))
            .join(candidate_path.replace(['/', '\\'], "_"));
        if let Some(parent) = archive_path.parent() {
            fs::create_dir_all(parent).expect("archive parent should create");
        }
        fs::write(&archive_path, candidate_body).expect("archive should write");
        let manifest_path = workspace_root
            .join(".pmcli")
            .join("agents")
            .join(agent_id)
            .join("worktree_artifact_candidates.json");
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent).expect("manifest parent should create");
        }
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "agent_worktree_artifact_candidate_manifest.v1",
                "agent_id": agent_id,
                "authority_scope": "worker_evidence_only",
                "adoption_status": "candidate_only",
                "worktree_path": worktree.display().to_string(),
                "changed_paths": [candidate_path],
                "untracked_paths": [candidate_path],
                "candidate_entries": [{
                    "relative_path": candidate_path,
                    "path_kind": "file",
                    "artifact_kind": "file",
                    "size_bytes": candidate_body.as_bytes().len(),
                    "sha256": tool_test_sha256_hex(candidate_body.as_bytes()),
                    "candidate_archive_ref": archive_path.display().to_string(),
                    "safe_status": "safe",
                    "from_input_bundle": false,
                    "is_directory": false,
                    "captured_at": "1"
                }],
                "generated_at": "1"
            }))
            .expect("manifest should serialize"),
        )
        .expect("manifest should write");
        manifest_path.display().to_string()
    }

    fn write_tool_adoption_accepted_evidence_index(
        workspace_root: &std::path::Path,
        project_id: &str,
        job_id: &str,
        stage_execution_id: &str,
        agent_id: &str,
        task_id: &str,
        task_type: &str,
        required_output_artifact_type: &str,
        candidate_manifest_ref: &str,
    ) {
        let dir = workspace_root
            .join("research")
            .join("stages")
            .join(job_id)
            .join(stage_execution_id)
            .join("accepted_worker_evidence");
        fs::create_dir_all(&dir).expect("accepted evidence dir should create");
        fs::write(
            dir.join("index.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research.accepted_worker_evidence_index.v1",
                "job_id": job_id,
                "project_id": project_id,
                "stage_execution_id": stage_execution_id,
                "stage_id": "literature",
                "generated_at": "1",
                "entries": [{
                    "agent_id": agent_id,
                    "task_id": task_id,
                    "task_type": task_type,
                    "worker_role": "implementation_worker",
                    "required_output_artifact_type": required_output_artifact_type,
                    "output_manifest_ref": workspace_root.join(".pmcli/agents").join(agent_id).join("output_manifest.json").display().to_string(),
                    "task_packet_ref": workspace_root.join(".pmcli/agents").join(agent_id).join("task_packet.json").display().to_string(),
                    "evidence_refs": [candidate_manifest_ref],
                    "matched_required_fields": ["candidate_artifact_path"],
                    "matched_acceptance_checks": ["candidate exists"],
                    "matched_quality_signals": ["provider_tool_completed"],
                    "quality_profile": {
                        "schema_version": "goal_stage_task_quality_profile.v1",
                        "score": 92,
                        "level": "strong",
                        "strengths": ["fixture candidate is explicit"],
                        "risks": []
                    },
                    "semantic_review": {
                        "schema_version": "goal_stage_task_semantic_review_result.v1",
                        "verdict": "pass",
                        "score": 90,
                        "reviewer_role": "fixture_reviewer",
                        "review_model": "fixture-model",
                        "execution_mode": "test",
                        "review_packet_ref": "review:fixture",
                        "review_trace_ref": "review_trace:fixture",
                        "findings": ["fixture passes"],
                        "failure_class": null,
                        "suggested_operation": "accept",
                        "rollback_target": null,
                        "cleanup_required": false,
                        "provider_id": null,
                        "model": null
                    },
                    "acceptance_authority": "fixture",
                    "main_agent_decision_ref": "main_agent_worker_artifact_decision::fixture",
                    "review_required": false,
                    "active_status": "active",
                    "current_evidence_set_id": format!("{stage_execution_id}::{task_id}"),
                    "replacement_of_task_ids": [],
                    "created_at": "1"
                }]
            }))
            .expect("accepted evidence should serialize"),
        )
        .expect("accepted evidence should write");
    }

    #[test]
    fn parse_http_url_rejects_request_control_characters() {
        let err = match parse_http_url("http://example.test/path\r\nX-Injected: yes") {
            Ok(_) => panic!("control characters must not enter the request line"),
            Err(err) => err,
        };

        assert!(err.contains("control characters"), "{err}");
    }

    #[test]
    fn parse_http_url_rejects_private_network_targets_by_default() {
        let endpoint = parse_http_url("http://127.0.0.1:8080/fixture")
            .expect("loopback URL syntax should parse");
        let err = enforce_fetch_network_policy(&endpoint, false)
            .expect_err("loopback fetches must be explicitly opted in");

        assert!(err.contains("private network"), "{err}");
    }

    #[test]
    fn parse_http_url_allows_https_and_reports_scheme() {
        let endpoint = parse_http_url("https://example.test/docs").expect("https URL should parse");

        assert_eq!(endpoint.scheme, "https");
        assert_eq!(endpoint.port, 443);
        assert_eq!(endpoint.path, "/docs");
    }

    #[test]
    fn paper_search_is_worker_safe_but_not_main_agent_tool() {
        assert!(agent_team_worker_safe_tool_names().contains(&"paper_search"));
        assert!(
            !main_agent_safe_tool_names_for_mode("danger-full-access").contains(&"paper_search")
        );

        let policy = vec!["read_file".to_string(), "paper_search".to_string()];
        let worker_tools = agent_team_worker_tool_definitions_for_policy(&policy)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(worker_tools.iter().any(|tool| tool == "paper_search"));
    }

    #[test]
    fn paper_search_requires_non_empty_query() {
        let workspace_root = temp_tool_workspace("paper_search_missing_query");
        let executor = LocalToolExecutor::new(&workspace_root);
        let mut call = ToolCall::new("paper_search");
        call.arguments = serde_json::json!({ "source": "arxiv" });

        let err = executor
            .execute(&call)
            .expect_err("paper_search should require query");

        assert!(matches!(err, ToolExecutionError::MissingArgument { .. }));
    }

    #[test]
    fn parse_arxiv_paper_candidates_extracts_structured_metadata() {
        let body = r#"<?xml version="1.0"?>
<feed>
<entry>
  <id>http://arxiv.org/abs/2307.03172v3</id>
  <updated>2023-07-08T00:00:00Z</updated>
  <published>2023-07-06T00:00:00Z</published>
  <title>Lost in the Middle: How Language Models Use Long Contexts</title>
  <summary>We analyze how language models use relevant information.</summary>
  <author><name>Nelson F. Liu</name></author>
  <author><name>Percy Liang</name></author>
</entry>
</feed>"#;

        let candidates = parse_arxiv_paper_candidates(body, Some(2020), Some(2026));

        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0]["title"],
            "Lost in the Middle: How Language Models Use Long Contexts"
        );
        assert_eq!(candidates[0]["year"], 2023);
        assert_eq!(candidates[0]["source"], "arxiv");
        assert!(candidates[0]["authors"]
            .as_array()
            .expect("authors should be an array")
            .iter()
            .any(|author| author == "Nelson F. Liu"));
    }

    #[test]
    fn worker_shell_allows_inspection_and_blocks_git_or_mutation() {
        assert!(is_worker_shell_command("rg benchmark src"));
        assert!(is_worker_shell_command("cargo test --lib runtime::tests"));
        assert!(!is_worker_shell_command("git status --short"));
        assert!(!is_worker_shell_command("rm -rf target"));
        assert!(!is_worker_shell_command("python -m pytest > report.txt"));
    }

    #[test]
    fn search_empty_result_explains_local_workspace_scope() {
        let workspace_root = temp_tool_workspace("search_empty_scope");
        fs::write(workspace_root.join("README.md"), "alpha beta\n")
            .expect("fixture file should write");
        let executor = LocalToolExecutor::new(&workspace_root);
        let mut call = ToolCall::new("search");
        call.target_path = Some(".".to_string());
        call.parameters
            .insert("pattern".to_string(), "not-present-in-fixture".to_string());
        call.arguments = serde_json::json!({ "pattern": "not-present-in-fixture" });
        let result = executor.execute(&call).expect("search should execute");

        assert_eq!(result.status, "succeeded");
        assert_eq!(
            result.structured.as_ref().unwrap()["match_count"],
            serde_json::json!(0)
        );
        assert!(result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("only searches files already present"));
    }

    #[test]
    fn read_file_supports_character_ranges_for_long_inputs() {
        let workspace_root = temp_tool_workspace("read_file_ranges");
        fs::write(workspace_root.join("rubric.md"), "abcdefghi").expect("fixture should write");
        let executor = LocalToolExecutor::new(&workspace_root);
        let mut call = ToolCall::new("read_file");
        call.target_path = Some("rubric.md".to_string());
        call.parameters
            .insert("offset".to_string(), "3".to_string());
        call.parameters
            .insert("max_chars".to_string(), "4".to_string());
        call.arguments = serde_json::json!({
            "path": "rubric.md",
            "offset": 3,
            "max_chars": 4
        });

        let result = executor.execute(&call).expect("read_file should execute");

        assert_eq!(result.output.as_deref(), Some("defg"));
        assert_eq!(
            result.structured.as_ref().unwrap()["offset"],
            serde_json::json!(3)
        );
        assert_eq!(
            result.structured.as_ref().unwrap()["next_offset"],
            serde_json::json!(7)
        );
        assert_eq!(
            result.structured.as_ref().unwrap()["truncated"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn read_file_llm_truncation_points_to_next_offset() {
        let result = ToolResult {
            tool_name: "read_file".to_string(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: Some("x".repeat(4_500)),
            structured: Some(serde_json::json!({
                "path": "long.md",
                "offset": 0,
                "returned_chars": 4500,
                "total_chars": 4500,
                "truncated": false,
                "next_offset": null
            })),
            error_message: None,
        };

        let rendered = format_tool_result_for_llm(&result);

        assert!(rendered.contains("continue with read_file offset=4000 max_chars=4000"));
    }

    #[test]
    fn fetch_result_for_llm_prefers_compact_preview() {
        let result = ToolResult {
            tool_name: "fetch".to_string(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: Some(
                "<html><head><title>Long Page</title></head><body>raw html</body></html>"
                    .repeat(200),
            ),
            structured: Some(serde_json::json!({
                "llm_preview": "Fetched URL: https://example.test\n\nLong Page raw html"
            })),
            error_message: None,
        };

        let rendered = format_tool_result_for_llm(&result);

        assert!(rendered.contains("Long Page raw html"));
        assert!(!rendered.contains("<html>"));
    }

    #[test]
    fn board_task_tool_result_for_llm_includes_main_agent_turn_complete() {
        let result = ToolResult {
            tool_name: "publish_board_tasks".to_string(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            output: None,
            structured: Some(serde_json::json!({
                "published_count": 1,
                "task_refs": ["main_agent_board_task::paper_search"],
                "agent_loop_control": {
                    "main_agent_turn_complete": true,
                    "reason": "board-visible worker tasks have been published"
                }
            })),
            error_message: None,
        };

        let rendered = format_tool_result_for_llm(&result);

        assert!(rendered.contains("Astra control"));
        assert!(rendered.contains("worker tasks"));
    }

    #[test]
    fn autonomous_research_main_agent_tool_surface_excludes_execution_tools() {
        let tool_names = main_agent_tool_definitions_for_mode("workspace-write")
            .into_iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();

        assert!(tool_names.contains(&"read_file".to_string()));
        assert!(tool_names.contains(&"list_files".to_string()));
        assert!(tool_names.contains(&"search".to_string()));
        assert!(tool_names.contains(&"publish_board_tasks".to_string()));
        assert!(tool_names.contains(&"record_stage_evidence_plan".to_string()));
        assert!(tool_names.contains(&"record_stage_closure_decision".to_string()));
        assert!(tool_names.contains(&"record_worker_artifact_decision".to_string()));
        assert!(tool_names.contains(&"request_cleanup_plan".to_string()));
        assert!(!tool_names.contains(&"fetch".to_string()));
        assert!(!tool_names.contains(&"write_file".to_string()));
        assert!(!tool_names.contains(&"apply_patch".to_string()));
        assert!(!tool_names.contains(&"worker_shell".to_string()));
        assert!(!tool_names.contains(&"shell".to_string()));
    }

    #[test]
    fn record_stage_evidence_plan_persists_main_agent_adopted_requirements() {
        let workspace_root = temp_tool_workspace("stage_evidence_plan");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_stage_plan".to_string(),
                job_id: Some("job_stage_plan".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_literature_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("record_stage_evidence_plan");
        call.arguments = serde_json::json!({
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "rationale": "Adopt strict literature evidence requirements from standard-setting worker output.",
            "plan_source_refs": ["accepted_worker_evidence:acceptance standard setting"],
            "evidence_requirements": [{
                "task_type": "closest-family survey",
                "worker_role": "literature_comparison_researcher",
                "objective": "Identify closest verified prior families that constrain novelty.",
                "required_output_artifact_type": "closest_family_survey",
                "required_output_fields": ["source entries", "closest-family ranking"],
                "acceptance_checks": ["compares closest families against active objective"],
                "failure_signals": ["generic summary"],
                "evidence_standard": "A strict reviewer can trace every closest-family claim to source rows."
            }]
        });

        let result = executor.execute(&call).expect("plan should record");

        assert_eq!(result.status, "succeeded");
        let latest_path = data_dir
            .join("main-agent-board")
            .join("stage-evidence-plans")
            .join("latest_stage_literature_1.json");
        let record: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(latest_path).expect("latest plan should exist"),
        )
        .expect("latest plan should parse");
        assert_eq!(record["published_by"], "main_agent");
        assert_eq!(
            record["evidence_requirements"][0]["task_type"],
            "closest-family survey"
        );
        assert!(record["authority_note"]
            .as_str()
            .unwrap_or_default()
            .contains("runtime only records"));
    }

    #[test]
    fn record_stage_evidence_plan_rejects_empty_source_refs() {
        let workspace_root = temp_tool_workspace("stage_evidence_plan_rejects_empty_refs");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_stage_plan".to_string(),
                job_id: Some("job_stage_plan".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_literature_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("record_stage_evidence_plan");
        call.arguments = serde_json::json!({
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "rationale": "missing source refs should fail",
            "plan_source_refs": [],
            "evidence_requirements": [{
                "task_type": "paper search",
                "worker_role": "literature_researcher",
                "objective": "Find papers.",
                "required_output_artifact_type": "literature_matrix",
                "required_output_fields": ["source entries"],
                "acceptance_checks": ["source rows exist"],
                "failure_signals": ["no sources"],
                "evidence_standard": "Sources are traceable."
            }]
        });

        let err = executor
            .execute(&call)
            .expect_err("empty source refs must fail");

        assert!(format!("{err}").contains("plan_source_refs"));
    }

    #[test]
    fn record_stage_closure_decision_persists_main_agent_decision() {
        let workspace_root = temp_tool_workspace("stage_closure_decision");
        let data_dir = workspace_root.join(".pmcli");
        write_passing_job_state(
            &data_dir,
            "job_stage_closure",
            "stage_literature_1",
            "rev_pass_literature",
        );
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_stage_closure".to_string(),
                job_id: Some("job_stage_closure".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_literature_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("record_stage_closure_decision");
        call.arguments = serde_json::json!({
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "decision": "close_and_advance",
            "target_stage_id": "novelty",
            "closure_rationale": "The accepted literature evidence and strict review establish that this stage has satisfied its goal.",
            "why_no_more_stage_work_is_needed": "Remaining risks are explicitly scoped and can be handled in the novelty stage without more literature collection.",
            "accepted_evidence_refs": ["accepted_worker_evidence:paper search", "accepted_worker_evidence:method comparison"],
            "review_ref": "review_packet:rev_stage_literature_1_latest_pass",
            "stage_artifact_ref": "research/stages/arj_fixture/literature/literature_matrix.md",
            "remaining_risks": ["Novelty still needs independent stress testing."],
            "cleanup_required": false,
            "cleanup_rationale": "No project口径 change is introduced by advancing.",
            "readiness_refs": ["research/stages/arj_fixture/literature/literature_matrix.md", "research/stages/arj_fixture/literature/reviews/rev_pass.md"]
        });

        let result = executor.execute(&call).expect("closure should record");

        assert_eq!(result.status, "succeeded");
        let latest_path = data_dir
            .join("main-agent-board")
            .join("stage-closure-decisions")
            .join("latest_stage_literature_1.json");
        let record: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(latest_path).expect("latest closure should exist"),
        )
        .expect("latest closure should parse");
        assert_eq!(record["published_by"], "main_agent");
        assert_eq!(record["decision"], "close_and_advance");
        assert_eq!(record["target_stage_id"], "novelty");
        assert_eq!(record["review_ref"], "review:rev_pass_literature");
        assert_eq!(
            record["raw_review_ref"],
            "review_packet:rev_stage_literature_1_latest_pass"
        );
        assert!(record["authority_note"]
            .as_str()
            .unwrap_or_default()
            .contains("runtime only validates"));
    }

    #[test]
    fn record_stage_closure_decision_rejects_non_current_review_ref() {
        let workspace_root = temp_tool_workspace("stage_closure_stale_review");
        let data_dir = workspace_root.join(".pmcli");
        write_passing_job_state(
            &data_dir,
            "job_stage_closure",
            "stage_literature_1",
            "rev_current_pass",
        );
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_stage_closure".to_string(),
                job_id: Some("job_stage_closure".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_literature_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("record_stage_closure_decision");
        call.arguments = serde_json::json!({
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "decision": "close_and_advance",
            "target_stage_id": "novelty",
            "closure_rationale": "close with stale review",
            "why_no_more_stage_work_is_needed": "close with stale review",
            "accepted_evidence_refs": ["accepted_worker_evidence:paper search"],
            "review_ref": "review:rev_old_pass",
            "stage_artifact_ref": "research/stages/arj_fixture/literature/literature_matrix.md",
            "remaining_risks": [],
            "cleanup_required": false,
            "cleanup_rationale": "none",
            "readiness_refs": ["research/stages/arj_fixture/literature/literature_matrix.md"]
        });

        let err = executor
            .execute(&call)
            .expect_err("stale review ref must fail before route change");

        assert!(format!("{err}").contains("current passing review"));
    }

    #[test]
    fn record_stage_closure_decision_rejects_other_stage_latest_pass_alias() {
        let workspace_root = temp_tool_workspace("stage_closure_wrong_latest_alias");
        let data_dir = workspace_root.join(".pmcli");
        write_passing_job_state(
            &data_dir,
            "job_stage_closure",
            "stage_literature_1",
            "rev_current_pass",
        );
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_stage_closure".to_string(),
                job_id: Some("job_stage_closure".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_literature_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("record_stage_closure_decision");
        call.arguments = serde_json::json!({
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_1",
            "decision": "close_and_advance",
            "target_stage_id": "novelty",
            "closure_rationale": "close with wrong alias",
            "why_no_more_stage_work_is_needed": "close with wrong alias",
            "accepted_evidence_refs": ["accepted_worker_evidence:paper search"],
            "review_ref": "review_packet:rev_stage_novelty_1_latest_pass",
            "stage_artifact_ref": "research/stages/arj_fixture/literature/literature_matrix.md",
            "remaining_risks": [],
            "cleanup_required": false,
            "cleanup_rationale": "none",
            "readiness_refs": ["research/stages/arj_fixture/literature/literature_matrix.md"]
        });

        let err = executor
            .execute(&call)
            .expect_err("other-stage latest-pass alias must fail before route change");

        assert!(format!("{err}").contains("current passing review"));
    }

    #[test]
    fn record_stage_closure_decision_rejects_stage_execution_mismatch() {
        let workspace_root = temp_tool_workspace("stage_closure_mismatch");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_stage_closure".to_string(),
                job_id: Some("job_stage_closure".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_literature_current".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("record_stage_closure_decision");
        call.arguments = serde_json::json!({
            "stage_id": "literature",
            "stage_execution_id": "stage_literature_old",
            "decision": "close_and_advance",
            "target_stage_id": "novelty",
            "closure_rationale": "stale decision",
            "why_no_more_stage_work_is_needed": "stale decision",
            "accepted_evidence_refs": ["accepted_worker_evidence:paper search"],
            "review_ref": "review:rev_pass_literature",
            "stage_artifact_ref": "research/stages/arj_fixture/literature/literature_matrix.md",
            "remaining_risks": [],
            "cleanup_required": false,
            "cleanup_rationale": "none",
            "readiness_refs": ["research/stages/arj_fixture/literature/literature_matrix.md"]
        });

        let err = executor
            .execute(&call)
            .expect_err("stale stage execution must fail");

        assert!(format!("{err}").contains("does not match active stage execution"));
    }

    #[test]
    fn publish_board_tasks_merges_duplicate_same_stage_task_type() {
        let workspace_root = temp_tool_workspace("merge_board_tasks");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_merge".to_string(),
                job_id: Some("job_tool_merge".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let publish_old = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::paper_search_old",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "older task",
                    "input_artifact_refs": [],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["source entries"],
                    "acceptance_checks": ["check"],
                    "failure_signals": ["fail"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "standard",
                    "review_findings_refs": []
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&publish_old)
            .expect("old task should publish");

        let publish_new = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::paper_search_new",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "newer task",
                    "input_artifact_refs": [],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["source entries"],
                    "acceptance_checks": ["check"],
                    "failure_signals": ["fail"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "standard",
                    "review_findings_refs": [],
                    "replacement_of_task_ids": ["board::stage_1::paper_search_old"]
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&publish_new)
            .expect("new task should publish");

        let old_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join("board__stage_1__paper_search_old.json");
        let old_value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&old_path).expect("old task should exist"))
                .expect("old task should parse");
        assert_eq!(
            old_value.get("status").and_then(|value| value.as_str()),
            Some("merged")
        );
        assert_eq!(
            old_value
                .get("merged_into")
                .and_then(|value| value.as_str()),
            Some("board::stage_1::paper_search_new")
        );
    }

    #[test]
    fn merge_board_tasks_rejects_runtime_candidate_source_with_actionable_error() {
        let workspace_root = temp_tool_workspace("merge_candidate_task");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_merge_candidate".to_string(),
                job_id: Some("job_tool_merge_candidate".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut call = ToolCall::new("merge_board_tasks");
        call.arguments = serde_json::json!({
            "target_id": "board::stage_1::method_comparison",
            "source_ids": ["research_stage_task::stage_1::paper_clustering"],
            "rationale": "candidate overlaps with method comparison"
        });

        let err = executor
            .execute(&call)
            .expect_err("candidate refs are not persisted board tasks");
        let message = format!("{err}");

        assert!(message.contains("not a published main-agent board task"));
        assert!(message.contains("update_board_task"));
        assert!(message.contains("supersedes_task_ids"));
    }

    #[test]
    fn publish_board_tasks_keeps_distinct_same_type_evidence_tasks() {
        let workspace_root = temp_tool_workspace("distinct_same_type_tasks");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_distinct_same_type".to_string(),
                job_id: Some("job_tool_distinct_same_type".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "research_stage_task::stage_1::contradiction_family_prior_audit",
                        "stage_id": "novelty",
                        "stage_execution_id": "stage_1",
                        "task_type": "closest-prior search",
                        "worker_role": "novelty_reviewer",
                        "objective": "Mount contradiction/conflict-resolution benchmark priors.",
                        "input_artifact_refs": ["auto_research_job:job_tool_distinct_same_type"],
                        "required_output_artifact_type": "novelty_report",
                        "required_output_fields": ["named comparator table"],
                        "acceptance_checks": ["covers contradiction priors"],
                        "failure_signals": ["missing contradiction family"],
                        "depends_on_task_ids": [],
                        "priority": 1,
                        "evidence_standard": "audits contradiction closest-prior family",
                        "review_findings_refs": ["rev_gap"]
                    },
                    {
                        "task_id": "research_stage_task::stage_1::abstention_family_prior_audit",
                        "stage_id": "novelty",
                        "stage_execution_id": "stage_1",
                        "task_type": "closest-prior search",
                        "worker_role": "novelty_reviewer",
                        "objective": "Mount abstention/selective-prediction/refusal-calibration benchmark priors.",
                        "input_artifact_refs": ["auto_research_job:job_tool_distinct_same_type"],
                        "required_output_artifact_type": "novelty_report",
                        "required_output_fields": ["named comparator table"],
                        "acceptance_checks": ["covers abstention priors"],
                        "failure_signals": ["missing abstention family"],
                        "depends_on_task_ids": [],
                        "priority": 1,
                        "evidence_standard": "audits abstention closest-prior family",
                        "review_findings_refs": ["rev_gap"]
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("distinct same-type evidence tasks should publish");
        for task_id in [
            "research_stage_task::stage_1::contradiction_family_prior_audit",
            "research_stage_task::stage_1::abstention_family_prior_audit",
        ] {
            let path = data_dir
                .join("main-agent-board")
                .join("tasks")
                .join(format!("{}.json", sanitize_file_component(task_id)));
            let value: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).expect("task should exist"))
                    .expect("task should parse");
            assert_ne!(
                value.get("status").and_then(|value| value.as_str()),
                Some("merged"),
                "{task_id} should remain dispatchable"
            );
        }
    }

    #[test]
    fn update_board_task_ready_status_clears_stale_merge_metadata() {
        let workspace_root = temp_tool_workspace("update_unmerges_ready_task");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_unmerge".to_string(),
                job_id: Some("job_tool_unmerge".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::contradiction_family_prior_audit",
                    "stage_id": "novelty",
                    "stage_execution_id": "stage_1",
                    "task_type": "closest-prior search",
                    "worker_role": "novelty_reviewer",
                    "objective": "Mount contradiction priors.",
                    "input_artifact_refs": ["auto_research_job:job_tool_unmerge"],
                    "required_output_artifact_type": "novelty_report",
                    "required_output_fields": ["named comparator table"],
                    "acceptance_checks": ["covers contradiction priors"],
                    "failure_signals": ["missing contradiction family"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "audits contradiction closest-prior family",
                    "review_findings_refs": ["rev_gap"]
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor.execute(&publish).expect("task should publish");
        let task_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join(format!(
                "{}.json",
                sanitize_file_component(
                    "research_stage_task::stage_1::contradiction_family_prior_audit"
                )
            ));
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&task_path).expect("task should exist"))
                .expect("task should parse");
        value["status"] = serde_json::json!("merged");
        value["merged_into"] =
            serde_json::json!("research_stage_task::stage_1::abstention_family_prior_audit");
        fs::write(
            &task_path,
            serde_json::to_string_pretty(&value).expect("task should serialize"),
        )
        .expect("task should write");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "research_stage_task::stage_1::contradiction_family_prior_audit",
                "patch": { "status": "ready" },
                "rationale": "restore distinct evidence task after over-broad merge"
            }),
            session_id: None,
            turn_id: None,
        };
        executor.execute(&update).expect("update should succeed");
        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(task_path).expect("task should exist"))
                .expect("task should parse");
        assert_eq!(
            updated.get("status").and_then(|value| value.as_str()),
            Some("ready")
        );
        assert!(
            updated.get("merged_into").is_none(),
            "ready tasks must not retain stale merged_into metadata"
        );
    }

    #[test]
    fn publish_board_tasks_rejects_implicit_upstream_dependency() {
        let workspace_root = temp_tool_workspace("implicit_board_dependencies");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_dependencies".to_string(),
                job_id: Some("job_tool_dependencies".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let mut publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "board::stage_1::paper_search",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "paper search",
                        "worker_role": "literature_researcher",
                        "objective": "Find concrete sources and produce row-level source entries.",
                        "input_artifact_refs": [],
                        "required_output_artifact_type": "literature_matrix",
                        "required_output_fields": ["source entries"],
                        "acceptance_checks": ["cites concrete sources"],
                        "failure_signals": ["generic narrative only"],
                        "depends_on_task_ids": [],
                        "priority": 1,
                        "evidence_standard": "source rows must include concrete refs",
                        "review_findings_refs": []
                    },
                    {
                        "task_id": "board::stage_1::source_verification",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "source verification",
                        "worker_role": "citation_auditor",
                        "objective": "Verify every source row found by paper search.",
                        "input_artifact_refs": [],
                        "required_output_artifact_type": "source_verification_report",
                        "required_output_fields": ["verification ledger"],
                        "acceptance_checks": ["uses accepted paper-search evidence"],
                        "failure_signals": ["cannot find upstream evidence"],
                        "depends_on_task_ids": [],
                        "priority": 2,
                        "evidence_standard": "each verified row must cite the source evidence it audited",
                        "review_findings_refs": []
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("implicit upstream dependency should be rejected");
        assert!(
            err.to_string().contains("consumes upstream evidence"),
            "{err}"
        );
        assert!(
            err.to_string().contains("board::stage_1::paper_search"),
            "{err}"
        );

        publish.arguments["tasks"][1]["depends_on_task_ids"] =
            serde_json::json!(["board::stage_1::paper_search"]);
        publish.arguments["tasks"][1]["input_artifact_refs"] =
            serde_json::json!(["board::stage_1::paper_search"]);
        executor
            .execute(&publish)
            .expect("declared upstream dependency should publish");
    }

    #[test]
    fn publish_board_tasks_rejects_invalid_batch_without_partial_task_files() {
        let workspace_root = temp_tool_workspace("board_publish_atomic_validation");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_atomic_publish".to_string(),
                job_id: Some("job_tool_atomic_publish".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "board::stage_1::paper_search",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "paper search",
                        "worker_role": "literature_researcher",
                        "objective": "Find concrete sources and produce row-level source entries.",
                        "input_artifact_refs": [],
                        "required_output_artifact_type": "literature_source_evidence",
                        "required_output_fields": ["source entries"],
                        "acceptance_checks": ["cites concrete sources"],
                        "failure_signals": ["generic narrative only"],
                        "depends_on_task_ids": [],
                        "priority": 1,
                        "evidence_standard": "source rows must include concrete refs",
                        "review_findings_refs": []
                    },
                    {
                        "task_id": "board::stage_1::source_verification",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "source verification",
                        "worker_role": "citation_auditor",
                        "objective": "Verify every source row found by paper search.",
                        "input_artifact_refs": [],
                        "required_output_artifact_type": "source_verification_report",
                        "required_output_fields": ["verification ledger"],
                        "acceptance_checks": ["uses accepted paper-search evidence"],
                        "failure_signals": ["cannot find upstream evidence"],
                        "depends_on_task_ids": [],
                        "priority": 2,
                        "evidence_standard": "each verified row must cite the source evidence it audited",
                        "review_findings_refs": []
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("implicit upstream dependency should reject the batch");
        assert!(
            err.to_string().contains("consumes upstream evidence"),
            "{err}"
        );
        let task_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join("board__stage_1__paper_search.json");
        assert!(
            !task_path.exists(),
            "semantic validation failure must not leave a partially published board task"
        );
        assert!(
            !data_dir
                .join("main-agent-board")
                .join("published_tasks_latest.json")
                .exists(),
            "failed publish must not update the board artifact record"
        );
    }

    #[test]
    fn publish_board_tasks_allows_paper_search_that_names_downstream_verification() {
        let workspace_root = temp_tool_workspace("paper_search_downstream_mentions");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_downstream_mentions".to_string(),
                job_id: Some("job_tool_downstream_mentions".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "acceptance standard setting",
                        "worker_role": "stage_standard_setter",
                        "objective": "Draft the strict literature-stage acceptance rubric and evidence plan, binding source quality, verification, synthesis, repair routing, and novelty-stage handoff criteria.",
                        "input_artifact_refs": [
                            "prompt_sha256:test",
                            "auto_research_job:job_tool_downstream_mentions"
                        ],
                        "required_output_artifact_type": "stage_acceptance_rubric",
                        "required_output_fields": ["rubric"],
                        "acceptance_checks": ["specific to the active stage"],
                        "failure_signals": ["generic rubric"],
                        "depends_on_task_ids": [],
                        "priority": 1,
                        "evidence_standard": "reviewer-facing acceptance rubric",
                        "review_findings_refs": []
                    },
                    {
                        "task_id": "research_stage_task::stage_1::paper_search",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "paper search",
                        "worker_role": "literature_researcher",
                        "objective": "Retrieve and read concrete prior-work sources relevant to TRELLIS-like sparse-structured 3D generation, image/text-to-3D generation, 3D evaluation benchmarks, and automated research/evaluation pipelines; produce row-level literature evidence for later verification and synthesis.",
                        "input_artifact_refs": [
                            "prompt_sha256:test",
                            "auto_research_job:job_tool_downstream_mentions",
                            "main_agent_stage_evidence_plan:stage_1:v1"
                        ],
                        "required_output_artifact_type": "literature_source_evidence",
                        "required_output_fields": ["source entries"],
                        "acceptance_checks": ["includes row-level source entries"],
                        "failure_signals": ["generic narrative only"],
                        "depends_on_task_ids": [],
                        "priority": 2,
                        "evidence_standard": "concrete source-backed literature evidence",
                        "review_findings_refs": []
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        let result = executor
            .execute(&publish)
            .expect("paper search should not depend on standard-setting just because it names downstream verification");
        assert_eq!(result.status, "succeeded");
        assert!(data_dir
            .join("main-agent-board")
            .join("tasks")
            .join("research_stage_task__stage_1__paper_search.json")
            .exists());
    }

    #[test]
    fn publish_board_tasks_accepts_concrete_file_ref_as_upstream_dependency() {
        let workspace_root = temp_tool_workspace("concrete_file_dependency");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_file_dependency".to_string(),
                job_id: Some("job_tool_file_dependency".to_string()),
                stage_id: Some("implement-solution".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("implementation_manifest".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "research_stage_task::stage_1::rubric_v1",
                        "stage_id": "implement-solution",
                        "stage_execution_id": "stage_1",
                        "task_type": "acceptance standard setting",
                        "worker_role": "stage_standard_setter",
                        "objective": "Write a strict implementation acceptance rubric.",
                        "input_artifact_refs": ["auto_research_job:job_tool_file_dependency"],
                        "required_output_artifact_type": "stage_acceptance_rubric",
                        "required_output_fields": ["expert target"],
                        "acceptance_checks": ["rubric has concrete thresholds"],
                        "failure_signals": ["generic rubric"],
                        "depends_on_task_ids": [],
                        "priority": 10,
                        "evidence_standard": "reproducibility_review"
                    },
                    {
                        "task_id": "research_stage_task::stage_1::fixture_tests_v1",
                        "stage_id": "implement-solution",
                        "stage_execution_id": "stage_1",
                        "task_type": "fixture/test creation",
                        "worker_role": "implementation_worker",
                        "objective": "Verify baseline_runner.py by writing pytest fixtures and capturing raw test output.",
                        "input_artifact_refs": ["baseline_runner.py"],
                        "required_output_artifact_type": "test_suite",
                        "required_output_fields": ["test source", "pytest output"],
                        "acceptance_checks": ["tests verify baseline return schema"],
                        "failure_signals": ["missing raw pytest output"],
                        "depends_on_task_ids": [],
                        "priority": 20,
                        "evidence_standard": "reproducibility_review"
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("concrete file refs should satisfy explicit dataflow");
    }

    #[test]
    fn publish_board_tasks_persists_required_canonical_artifacts() {
        let workspace_root = temp_tool_workspace("required_canonical_artifacts_persisted");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_required_canonical".to_string(),
                job_id: Some("job_tool_required_canonical".to_string()),
                stage_id: Some("implement-solution".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("implementation_manifest".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::runner_integration",
                    "stage_id": "implement-solution",
                    "stage_execution_id": "stage_1",
                    "task_type": "integration check",
                    "worker_role": "implementation_worker",
                    "objective": "Inspect the canonical runner and report whether it can be used by downstream tests.",
                    "input_artifact_refs": ["accepted_worker_evidence_task:runner_build"],
                    "required_canonical_artifacts": [{
                        "target_artifact_path": "baseline_runner.py",
                        "required_status": "integration_verified",
                        "dependency_kind": "runnable",
                        "reason": "downstream test work must execute the canonical runner"
                    }],
                    "required_output_artifact_type": "integration_report",
                    "required_output_fields": ["command", "result"],
                    "acceptance_checks": ["uses canonical project path"],
                    "failure_signals": ["uses input bundle as project source"],
                    "depends_on_task_ids": ["runner_build"],
                    "priority": 1,
                    "evidence_standard": "must cite canonical runner execution"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("task with required canonical artifact should publish");
        let task_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join(format!(
                "{}.json",
                sanitize_file_component("research_stage_task::stage_1::runner_integration")
            ));
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(task_path).expect("task should exist"))
                .expect("task should parse");
        let requirement = value["required_canonical_artifacts"][0]
            .as_object()
            .expect("requirement should be an object");
        assert_eq!(
            requirement
                .get("target_artifact_path")
                .and_then(|value| value.as_str()),
            Some("baseline_runner.py")
        );
        assert_eq!(
            requirement
                .get("required_status")
                .and_then(|value| value.as_str()),
            Some("integration_verified")
        );
        assert_eq!(
            requirement
                .get("dependency_kind")
                .and_then(|value| value.as_str()),
            Some("runnable")
        );
    }

    #[test]
    fn publish_board_tasks_rejects_input_bundle_as_required_canonical_artifact() {
        let workspace_root = temp_tool_workspace("required_canonical_artifacts_reject_bundle");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_required_canonical_invalid".to_string(),
                job_id: Some("job_tool_required_canonical_invalid".to_string()),
                stage_id: Some("implement-solution".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("implementation_manifest".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::bad_required_path",
                    "stage_id": "implement-solution",
                    "stage_execution_id": "stage_1",
                    "task_type": "integration check",
                    "worker_role": "implementation_worker",
                    "objective": "Try to run a bundle copy.",
                    "input_artifact_refs": ["accepted_worker_evidence_task:runner_build"],
                    "required_canonical_artifacts": [{
                        "target_artifact_path": ".pmcli/input-bundles/runner/baseline_runner.py",
                        "required_status": "baseline_visible",
                        "dependency_kind": "file"
                    }],
                    "required_output_artifact_type": "integration_report",
                    "required_output_fields": ["command"],
                    "acceptance_checks": ["uses canonical project path"],
                    "failure_signals": ["uses bundle"],
                    "depends_on_task_ids": ["runner_build"],
                    "priority": 1,
                    "evidence_standard": "must not use bundle copies"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("bundle paths are not canonical project dependencies");
        assert!(err.to_string().contains("required_canonical_artifacts"));
        assert!(err.to_string().contains(".pmcli/input-bundles"));
    }

    #[test]
    fn publish_board_tasks_rejects_active_stage_artifact_required_canonical_dependency() {
        let workspace_root =
            temp_tool_workspace("required_canonical_artifacts_reject_active_stage");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_required_canonical_self".to_string(),
                job_id: Some("job_tool_required_canonical_self".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::self_dependency",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "Find supporting papers for the active literature stage.",
                    "input_artifact_refs": [],
                    "required_canonical_artifacts": [{
                        "target_artifact_path": "./research/stages/job_tool_required_canonical_self/literature/literature_matrix.md",
                        "required_status": "active_stage_evidence",
                        "dependency_kind": "stage_evidence",
                        "reason": "invalidly treats the active stage target as pre-existing evidence"
                    }],
                    "required_output_artifact_type": "literature_evidence_packet",
                    "required_output_fields": ["source refs"],
                    "acceptance_checks": ["reports inspected sources"],
                    "failure_signals": ["no sources"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "must inspect real source candidates"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("active stage artifact cannot be a required canonical dependency");
        let message = err.to_string();
        assert!(message.contains("required_canonical_artifacts[0]"));
        assert!(message.contains("active stage artifact"));
        assert!(message.contains("literature_matrix.md"));
        assert!(message.contains("input_artifact_refs"));
        assert!(message.contains("review_target_evidence_refs"));
        assert!(message.contains("depends_on_task_ids"));
    }

    #[test]
    fn publish_board_tasks_rejects_stage_support_doc_as_required_canonical_dependency() {
        let workspace_root =
            temp_tool_workspace("required_canonical_artifacts_reject_stage_support_doc");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_required_canonical_support_doc".to_string(),
                job_id: Some("job_tool_required_canonical_support_doc".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::synthesis_with_rubric_dependency",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "stage artifact synthesis",
                    "worker_role": "research_synthesizer",
                    "objective": "Produce the revised literature matrix using the stage rubric as context.",
                    "input_artifact_refs": [
                        "research/stages/job_tool_required_canonical_support_doc/literature/stage_acceptance_rubric.md"
                    ],
                    "required_canonical_artifacts": [{
                        "target_artifact_path": "research/stages/job_tool_required_canonical_support_doc/literature/stage_acceptance_rubric.md",
                        "required_status": "baseline_visible",
                        "dependency_kind": "file",
                        "reason": "incorrectly treats runtime-maintained stage guidance as a canonical project artifact"
                    }],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["citation ledger"],
                    "acceptance_checks": ["satisfies the stage rubric"],
                    "failure_signals": ["missing citation ledger"],
                    "depends_on_task_ids": [],
                    "evidence_standard": "produce a reviewable stage artifact"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor.execute(&publish).expect_err(
            "stage support docs must remain input refs rather than canonical dependencies",
        );
        let message = err.to_string();
        assert!(message.contains("required_canonical_artifacts[0]"));
        assert!(message.contains("stage_acceptance_rubric.md"));
        assert!(message.contains("input_artifact_refs"));
    }

    #[test]
    fn update_board_task_rejects_active_stage_artifact_required_canonical_dependency() {
        let workspace_root =
            temp_tool_workspace("required_canonical_artifacts_update_active_stage");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_required_canonical_update_self".to_string(),
                job_id: Some("job_tool_required_canonical_update_self".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let task_id = "research_stage_task::stage_1::update_self_dependency";
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": task_id,
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "Find supporting papers for the active literature stage.",
                    "input_artifact_refs": [],
                    "required_canonical_artifacts": [],
                    "required_output_artifact_type": "literature_evidence_packet",
                    "required_output_fields": ["source refs"],
                    "acceptance_checks": ["reports inspected sources"],
                    "failure_signals": ["no sources"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "must inspect real source candidates"
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&publish)
            .expect("initial task without self dependency should publish");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": task_id,
                "patch": {
                    "required_canonical_artifacts": [{
                        "target_artifact_path": "research/stages/job_tool_required_canonical_update_self/literature/literature_matrix.md",
                        "required_status": "active_stage_evidence",
                        "dependency_kind": "stage_evidence",
                        "reason": "invalidly treats the active stage target as pre-existing evidence"
                    }]
                },
                "rationale": "try to add an invalid self dependency"
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&update)
            .expect_err("update must not add active stage artifact as dependency");
        let message = err.to_string();
        assert!(message.contains("required_canonical_artifacts[0]"));
        assert!(message.contains("active stage artifact"));
        assert!(message.contains("literature_matrix.md"));
        let task_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join(format!("{}.json", sanitize_file_component(task_id)));
        let persisted: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(task_path).expect("task should persist"))
                .expect("persisted task should parse");
        assert_eq!(
            persisted["required_canonical_artifacts"]
                .as_array()
                .expect("required canonical artifacts should stay an array")
                .len(),
            0
        );
    }

    #[test]
    fn publish_board_tasks_accepts_code_review_with_concrete_target_ref() {
        let workspace_root = temp_tool_workspace("code_review_target_ref");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_code_review_target".to_string(),
                job_id: Some("job_tool_code_review_target".to_string()),
                stage_id: Some("implement-solution".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("implementation_manifest".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::baseline_code_review_v1",
                    "stage_id": "implement-solution",
                    "stage_execution_id": "stage_1",
                    "task_type": "code review",
                    "worker_role": "code_reviewer",
                    "objective": "Review baseline_runner.py and include bug fix suggestions with line references when bugs are found.",
                    "input_artifact_refs": ["baseline_runner.py"],
                    "required_output_artifact_type": "code_review_report",
                    "required_output_fields": ["file summary", "bug report", "sign off"],
                    "acceptance_checks": ["review binds findings to concrete code lines"],
                    "failure_signals": ["vague review without target evidence"],
                    "depends_on_task_ids": ["research_stage_task::stage_1::baseline_runner_v1"],
                    "review_target_evidence_refs": ["baseline_runner.py"],
                    "priority": 7,
                    "evidence_standard": "reproducibility_review"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("code review with concrete target refs should publish");
    }

    #[test]
    fn publish_board_tasks_rejects_artifact_repair_with_non_stage_output_type() {
        let workspace_root = temp_tool_workspace("artifact_repair_output_type");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_artifact_repair".to_string(),
                job_id: Some("job_tool_artifact_repair".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::repair_bad_output",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "artifact_repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Repair the active literature matrix.",
                    "input_artifact_refs": ["auto_research_job:job_tool_artifact_repair"],
                    "required_output_artifact_type": "artifact_repair",
                    "required_output_fields": ["candidate_artifact_path"],
                    "acceptance_checks": ["rewrite the contradicted claim identified by review finding rev_bad_output#artifact-type"],
                    "failure_signals": ["wrong artifact type"],
                    "depends_on_task_ids": [],
                    "review_findings_refs": ["review:rev_bad_output#artifact-type"],
                    "blocker_refs": ["failed_stage_review:rev_bad_output"],
                    "evidence_standard": "stage artifact candidate must be reviewable"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("artifact repair tasks must produce the stage artifact type");

        assert!(matches!(err, ToolExecutionError::InvalidArgument { .. }));
        assert!(
            err.to_string()
                .contains("required_output_artifact_type must be `literature_matrix`"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_rejects_stage_synthesis_with_generic_output_type() {
        let workspace_root = temp_tool_workspace("stage_synthesis_output_type");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_stage_synthesis".to_string(),
                job_id: Some("job_tool_stage_synthesis".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::synthesis_bad_output",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "stage artifact synthesis",
                    "worker_role": "research_synthesizer",
                    "objective": "Produce the revised literature matrix.",
                    "input_artifact_refs": ["auto_research_job:job_tool_stage_synthesis"],
                    "required_output_artifact_type": "stage_artifact_candidate",
                    "required_output_fields": ["candidate_artifact_path"],
                    "acceptance_checks": ["produce a reviewable literature matrix"],
                    "failure_signals": ["wrong artifact type"],
                    "depends_on_task_ids": [],
                    "evidence_standard": "stage artifact candidate must be reviewable"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("stage synthesis tasks must produce the concrete stage artifact type");

        assert!(matches!(err, ToolExecutionError::InvalidArgument { .. }));
        assert!(
            err.to_string()
                .contains("required_output_artifact_type must be `literature_matrix`"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_rejects_vague_failed_review_artifact_repair() {
        let workspace_root = temp_tool_workspace("vague_artifact_repair");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_vague_artifact_repair".to_string(),
                job_id: Some("job_tool_vague_artifact_repair".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::generic_repair",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "artifact_repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Repair the failed review with better evidence.",
                    "input_artifact_refs": ["review:rev_vague"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["literature_matrix"],
                    "acceptance_checks": ["repair is complete"],
                    "failure_signals": ["generic repair"],
                    "depends_on_task_ids": [],
                    "evidence_standard": "stage artifact candidate must be reviewable"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("vague artifact repair should be rejected at publication");

        assert!(err.to_string().contains("review_findings_refs"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_stage_synthesis_with_non_synthesizer_role() {
        let workspace_root = temp_tool_workspace("synthesis_wrong_role");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_synthesis_wrong_role".to_string(),
                job_id: Some("job_tool_synthesis_wrong_role".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::literature_synthesis_wrong_role",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "stage artifact synthesis",
                    "worker_role": "literature_comparison_researcher",
                    "objective": "Synthesize accepted literature evidence into the canonical literature matrix candidate.",
                    "input_artifact_refs": ["accepted_worker_evidence_index:research/stages/stage_1/accepted_worker_evidence/index.json"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["accepted input evidence refs", "evidence-to-section mapping"],
                    "acceptance_checks": ["candidate integrates accepted evidence"],
                    "failure_signals": ["disconnected fragments"],
                    "depends_on_task_ids": [],
                    "evidence_standard": "stage synthesis must cite accepted worker evidence"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("stage synthesis must use research_synthesizer");

        assert!(
            err.to_string()
                .contains("worker_role `research_synthesizer`"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_allows_failed_review_repair_without_review_targets() {
        let workspace_root = temp_tool_workspace("failed_review_repair_targeting");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_failed_review_repair".to_string(),
                job_id: Some("job_tool_failed_review_repair".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::row_level_source_entry_repair_v1",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "artifact_repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Repair the literature matrix after failed review rev_1 by adding source-level rows.",
                    "input_artifact_refs": ["auto_research_job:job_tool_failed_review_repair"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["row_level_source_entries_section"],
                    "acceptance_checks": ["each verified source has a row-level source entry tied to review finding rev_1#missing-rows"],
                    "failure_signals": ["aggregate-only clustering without row-level source entries"],
                    "depends_on_task_ids": [],
                    "blocker_refs": ["review:rev_1"],
                    "review_findings_refs": ["review:rev_1#missing-rows"],
                    "review_target_task_ids": [],
                    "review_target_evidence_refs": [],
                    "evidence_standard": "repair task grounded in failed review findings"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("failed-review repair tasks are worker repair tasks, not reviewer tasks");
    }

    #[test]
    fn publish_board_tasks_rejects_required_slot_repair_task_type_alias() {
        let workspace_root = temp_tool_workspace("required_slot_repair_alias");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_required_slot_repair_alias".to_string(),
                job_id: Some("job_tool_required_slot_repair_alias".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let original = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "main_agent_task_literature_5",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "literature synthesis",
                    "worker_role": "research_synthesizer",
                    "objective": "Produce required literature synthesis evidence.",
                    "input_artifact_refs": [],
                    "required_output_artifact_type": "literature_synthesis",
                    "required_output_fields": ["synthesis findings"],
                    "acceptance_checks": ["claims are source-grounded"],
                    "failure_signals": ["unsupported claims"],
                    "depends_on_task_ids": [],
                    "evidence_standard": "required literature synthesis evidence"
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&original)
            .expect("original required slot task should publish");

        let repair_alias = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "main_agent_task_literature_8",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "literature synthesis repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Repair the blocked literature synthesis evidence after failed review.",
                    "input_artifact_refs": ["review_packet:rev_source_grounding"],
                    "required_output_artifact_type": "literature_handoff",
                    "required_output_fields": ["synthesis findings", "claim support boundaries"],
                    "acceptance_checks": ["rewrite contradicted claim and add citation mapping for review finding rev_source_grounding#unsupported-claim"],
                    "failure_signals": ["unsupported positive claim remains"],
                    "depends_on_task_ids": [],
                    "review_findings_refs": ["review:rev_source_grounding#unsupported-claim"],
                    "blocker_refs": ["stage_closure_blocker:missing_required_task_type:literature_synthesis"],
                    "review_target_task_ids": ["main_agent_task_literature_5"],
                    "evidence_standard": "surgical repair of the required literature synthesis slot"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&repair_alias)
            .expect_err("required slot repair must keep the canonical task_type");

        assert!(
            err.to_string()
                .contains("repairs required worker task type `literature synthesis`"),
            "{err}"
        );
        assert!(err.to_string().contains("Keep `task_type` equal"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_lifecycle_only_repair_task_type() {
        let workspace_root = temp_tool_workspace("lifecycle_only_repair_task_type");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_lifecycle_only_repair_task_type".to_string(),
                job_id: Some("job_tool_lifecycle_only_repair_task_type".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "ignored_artifact_capture_probe::stage_1::standard_and_probe",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "repair",
                "worker_role": "stage_standard_setter",
                "objective": "Repair the stage-standard probe after failed review while preserving ignored-artifact capture evidence.",
                "input_artifact_refs": ["review_trace:rev_standard"],
                "required_output_artifact_type": "stage_acceptance_rubric",
                "required_output_fields": ["CandidateStageEvidencePlan", "probe artifact paths"],
                "acceptance_checks": ["add citation mapping for review finding rev_standard#capture-note"],
                "failure_signals": ["capture note remains missing"],
                "depends_on_task_ids": [],
                "review_findings_refs": ["rev_standard:capture-note"],
                "blocker_refs": ["worker_review_failure_route::rev_standard"],
                "evidence_standard": "surgical repair of the standard-setting slot"
            }]
        });

        let err = executor
            .execute(&publish)
            .expect_err("generic repair must not replace the stage slot identity");

        assert!(
            err.to_string()
                .contains("task_type `repair` is only a lifecycle label"),
            "{err}"
        );
        assert!(
            err.to_string()
                .contains("task_type `acceptance standard setting`"),
            "{err}"
        );
    }

    #[test]
    fn update_board_task_rejects_required_slot_output_type_drift() {
        let workspace_root = temp_tool_workspace("required_slot_output_type_drift");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_required_slot_output_type_drift".to_string(),
                job_id: Some("job_tool_required_slot_output_type_drift".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "research_stage_task::stage_1::paper_search",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "paper search",
                "worker_role": "literature_researcher",
                "objective": "Retrieve source-grounded literature rows.",
                "input_artifact_refs": ["auto_research_job:job_tool_required_slot_output_type_drift"],
                "required_output_artifact_type": "literature_matrix",
                "required_output_fields": ["source entries", "citation ledger"],
                "acceptance_checks": ["source rows include citation mapping"],
                "failure_signals": ["missing source refs"],
                "depends_on_task_ids": [],
                "evidence_standard": "source-grounded literature evidence"
            }]
        });
        executor
            .execute(&publish)
            .expect("canonical paper-search task should publish");

        let mut update = ToolCall::new("update_board_task");
        update.arguments = serde_json::json!({
            "task_id": "research_stage_task::stage_1::paper_search",
            "patch": {
                "objective": "Produce a repair report instead of source-grounded literature rows.",
                "required_output_artifact_type": "repair_report",
                "review_findings_refs": ["rev_sources:probe"],
                "blocker_refs": ["worker_review_failure_route::rev_sources"],
                "acceptance_checks": ["add citation mapping for review finding rev_sources#probe"],
                "status": "ready"
            },
            "rationale": "bad update fixture"
        });

        let err = executor
            .execute(&update)
            .expect_err("required slot output type must not drift to repair_report");

        assert!(
            err.to_string()
                .contains("stage evidence slot `paper search`"),
            "{err}"
        );
        assert!(
            err.to_string()
                .contains("required_output_artifact_type must stay `literature_matrix`"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_rejects_standard_setting_repair_alias_without_target_task_ref() {
        let workspace_root = temp_tool_workspace("standard_setting_repair_alias");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_standard_setting_repair_alias".to_string(),
                job_id: Some("job_tool_standard_setting_repair_alias".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish_original = ToolCall::new("publish_board_tasks");
        publish_original.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "board_task::stage_1::acceptance_standard_setting",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "acceptance standard setting",
                "worker_role": "stage_standard_setter",
                "objective": "Draft the stage rubric and CandidateStageEvidencePlan.",
                "input_artifact_refs": ["auto_research_job:job_tool_standard_setting_repair_alias"],
                "required_output_artifact_type": "stage_acceptance_rubric",
                "required_output_fields": ["CandidateStageEvidencePlan"],
                "acceptance_checks": ["candidate plan can be adopted with record_stage_evidence_plan"],
                "failure_signals": ["generic rubric"],
                "depends_on_task_ids": [],
                "evidence_standard": "strict stage standard"
            }]
        });
        executor
            .execute(&publish_original)
            .expect("original standard-setting task should publish");

        let mut publish_repair = ToolCall::new("publish_board_tasks");
        publish_repair.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "board_task::stage_1::acceptance_standard_setting_repair",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "acceptance standard setting repair",
                "worker_role": "stage_standard_setter",
                "objective": "Repair the stage rubric after failed review.",
                "input_artifact_refs": ["review_trace:rev_standard"],
                "required_output_artifact_type": "stage_acceptance_rubric",
                "required_output_fields": ["CandidateStageEvidencePlan", "citation-ledger status taxonomy"],
                "acceptance_checks": ["add citation mapping for review finding rev_standard#taxonomy"],
                "failure_signals": ["weak source taxonomy remains"],
                "depends_on_task_ids": [],
                "review_findings_refs": ["rev_standard:taxonomy"],
                "blocker_refs": ["worker_review_failure_route::rev_standard"],
                "review_target_evidence_refs": ["review_trace:rev_standard"],
                "evidence_standard": "surgical repair of the standard-setting slot"
            }]
        });

        let err = executor
            .execute(&publish_repair)
            .expect_err("standard-setting repair must keep the canonical task_type");

        assert!(
            err.to_string()
                .contains("repairs required worker task type `acceptance standard setting`"),
            "{err}"
        );
        assert!(err.to_string().contains("Keep `task_type` equal"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_stage_catalog_repair_alias_without_existing_task() {
        let workspace_root = temp_tool_workspace("stage_catalog_repair_alias");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_stage_catalog_repair_alias".to_string(),
                job_id: Some("job_tool_stage_catalog_repair_alias".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "board_task::stage_1::paper_search_repair",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "paper search repair",
                "worker_role": "literature_researcher",
                "objective": "Repair source search after failed review.",
                "input_artifact_refs": ["review_trace:rev_sources"],
                "required_output_artifact_type": "literature_matrix",
                "required_output_fields": ["source entries", "citation ledger"],
                "acceptance_checks": ["add citation mapping for review finding rev_sources#missing-source"],
                "failure_signals": ["source gap remains"],
                "depends_on_task_ids": [],
                "review_findings_refs": ["rev_sources:missing-source"],
                "blocker_refs": ["worker_review_failure_route::rev_sources"],
                "review_target_evidence_refs": ["review_trace:rev_sources"],
                "evidence_standard": "surgical repair of the paper search slot"
            }]
        });

        let err = executor
            .execute(&publish)
            .expect_err("catalog slot repair must keep the canonical task_type");

        assert!(
            err.to_string()
                .contains("repairs required worker task type `paper search`"),
            "{err}"
        );
        assert!(err.to_string().contains("Keep `task_type` equal"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_generic_type_and_alias_for_canonical_stage_slot() {
        let workspace_root = temp_tool_workspace("canonical_stage_slot_identity");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_canonical_stage_slot_identity".to_string(),
                job_id: Some("job_tool_canonical_stage_slot_identity".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "research_stage_task::stage_1::open_problem_extraction",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "research",
                "worker_role": "literature_gap_analyst",
                "objective": "Republish open-problem work after a dependency correction.",
                "input_artifact_refs": ["auto_research_job:job_tool_canonical_stage_slot_identity"],
                "required_output_artifact_type": "literature_matrix",
                "required_output_fields": ["open problems"],
                "acceptance_checks": ["open problems remain evidence-bound"],
                "failure_signals": ["generic unsupported gaps"],
                "depends_on_task_ids": [],
                "evidence_standard": "source-grounded open problems"
            }]
        });

        let err = executor
            .execute(&publish)
            .expect_err("canonical stage slot must reject generic task_type aliases");

        assert!(
            err.to_string()
                .contains("task_id names canonical stage slot `open-problem extraction`"),
            "{err}"
        );
        assert!(err
            .to_string()
            .contains("Use task_type `open-problem extraction`"));
    }

    #[test]
    fn publish_board_tasks_rejects_repair_alias_for_canonical_stage_slot() {
        let workspace_root = temp_tool_workspace("canonical_stage_slot_repair_alias");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_canonical_repair_alias".to_string(),
                job_id: Some("job_tool_canonical_repair_alias".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "research_stage_task::stage_1::stage_artifact_synthesis_repair_gap_rows",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "stage artifact synthesis",
                "worker_role": "research_synthesizer",
                "objective": "Repair the literature matrix after failed review rev_gap_rows.",
                "input_artifact_refs": ["review_packet:rev_gap_rows"],
                "required_output_artifact_type": "literature_matrix",
                "required_output_fields": ["Evidence Binding Ledger"],
                "acceptance_checks": ["repair review finding rev_gap_rows#missing-gap-row"],
                "failure_signals": ["gap rows remain unbound"],
                "depends_on_task_ids": [],
                "review_findings_refs": ["review:rev_gap_rows#missing-gap-row"],
                "blocker_refs": ["failed_stage_review:rev_gap_rows"],
                "evidence_standard": "contract-clean synthesis repair"
            }]
        });

        let err = executor
            .execute(&publish)
            .expect_err("repair aliases in the standard stage namespace must reuse the slot id");

        assert!(
            err.to_string()
                .contains("research_stage_task::stage_1::stage_artifact_synthesis"),
            "{err}"
        );
        assert!(err.to_string().contains("update_board_task"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_rejected_worker_artifact_as_supporting_input() {
        let workspace_root = temp_tool_workspace("rejected_worker_artifact_input");
        let data_dir = workspace_root.join(".pmcli");
        let evidence_dir = workspace_root
            .join("research/stages/job_rejected_input/stage_1/accepted_worker_evidence");
        fs::create_dir_all(&evidence_dir).expect("evidence dir should create");
        fs::write(
            evidence_dir.join("index.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "entries": [{
                    "agent_id": "agent_rejected",
                    "active_status": "rejected"
                }]
            }))
            .expect("index should serialize"),
        )
        .expect("index should write");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_rejected_input".to_string(),
                job_id: Some("job_rejected_input".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "acceptance standard setting",
                "worker_role": "stage_standard_setter",
                "objective": "Repair the rejected standard-setting artifact.",
                "input_artifact_refs": [
                    "research/stages/job_rejected_input/stage_1/accepted_worker_evidence/materialized/agent_rejected/candidates/rubric.md"
                ],
                "required_output_artifact_type": "stage_acceptance_rubric",
                "required_output_fields": ["pass criteria"],
                "acceptance_checks": ["rejected evidence is quarantined"],
                "failure_signals": ["rejected evidence reused as support"],
                "depends_on_task_ids": [],
                "evidence_standard": "authority-clean repair evidence"
            }]
        });

        let err = executor
            .execute(&publish)
            .expect_err("rejected worker artifacts must not be supporting inputs");

        assert!(err.to_string().contains("diagnostic history"), "{err}");
        assert!(
            err.to_string().contains("review_target_evidence_refs"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_allows_required_slot_repair_with_canonical_task_type() {
        let workspace_root = temp_tool_workspace("required_slot_repair_canonical");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_required_slot_repair_canonical".to_string(),
                job_id: Some("job_tool_required_slot_repair_canonical".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "main_agent_task_literature_5",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "literature synthesis",
                        "worker_role": "research_synthesizer",
                        "objective": "Produce required literature synthesis evidence.",
                        "input_artifact_refs": [],
                        "required_output_artifact_type": "literature_synthesis",
                        "required_output_fields": ["synthesis findings"],
                        "acceptance_checks": ["claims are source-grounded"],
                        "failure_signals": ["unsupported claims"],
                        "depends_on_task_ids": [],
                        "evidence_standard": "required literature synthesis evidence"
                    },
                    {
                        "task_id": "main_agent_task_literature_5_retry",
                        "stage_id": "literature",
                        "stage_execution_id": "stage_1",
                        "task_type": "literature synthesis",
                        "worker_role": "research_synthesizer",
                        "objective": "Repair the blocked literature synthesis evidence after failed review while preserving the required evidence slot identity.",
                        "input_artifact_refs": ["review_packet:rev_source_grounding"],
                        "required_output_artifact_type": "literature_synthesis",
                        "required_output_fields": ["synthesis findings", "claim support boundaries"],
                        "acceptance_checks": ["rewrite contradicted claim and add citation mapping for review finding rev_source_grounding#unsupported-claim"],
                        "failure_signals": ["unsupported positive claim remains"],
                        "depends_on_task_ids": [],
                        "review_findings_refs": ["review:rev_source_grounding#unsupported-claim"],
                        "blocker_refs": ["stage_closure_blocker:missing_required_task_type:literature_synthesis"],
                        "replacement_of_task_ids": ["main_agent_task_literature_5"],
                        "review_target_task_ids": ["main_agent_task_literature_5"],
                        "evidence_standard": "surgical repair of the required literature synthesis slot"
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("canonical required slot repair should publish");
    }

    #[test]
    fn publish_board_tasks_allows_non_repair_tasks_that_mention_failed_review_routing() {
        let workspace_root = temp_tool_workspace("non_repair_failed_review_mention");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_non_repair_failed_review_mention".to_string(),
                job_id: Some("job_tool_non_repair_failed_review_mention".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "acceptance standard setting",
                    "worker_role": "stage_standard_setter",
                    "objective": "Set the acceptance standard for failed-review routing without generic repair drift so duplicate repair dispatch prevention and repair_report substitution checks route into precise follow-up tasks without making this standard-setting task a repair task.",
                    "input_artifact_refs": ["auto_research_job:job_tool_non_repair_failed_review_mention"],
                    "required_output_artifact_type": "stage_acceptance_rubric",
                    "required_output_fields": ["full rubric markdown"],
                    "acceptance_checks": ["rubric is explicit enough to drive failed-review routing without generic repair drift or turning this standard-setting task into a repair task"],
                    "failure_signals": ["generic rubric"],
                    "depends_on_task_ids": [],
                    "priority": 10,
                    "evidence_standard": "reproducibility_review"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("non-repair tasks may mention failed-review routing without repair contracts");
    }

    #[test]
    fn update_board_task_rejects_repair_like_standard_task_without_blocker_binding() {
        let workspace_root = temp_tool_workspace("repair_like_update_contract");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_repair_like_update_contract".to_string(),
                job_id: Some("job_tool_repair_like_update_contract".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "acceptance standard setting",
                    "worker_role": "stage_standard_setter",
                    "objective": "Set the literature acceptance standard for row-level source evidence.",
                    "input_artifact_refs": ["auto_research_job:job_tool_repair_like_update_contract"],
                    "required_output_artifact_type": "stage_acceptance_rubric",
                    "required_output_fields": ["full rubric markdown"],
                    "acceptance_checks": ["rubric defines row-level citation ledger requirements"],
                    "failure_signals": ["generic rubric"],
                    "depends_on_task_ids": [],
                    "priority": 10,
                    "evidence_standard": "reproducibility_review"
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&publish)
            .expect("base non-repair standard-setting task should publish");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                "patch": {
                    "objective": "Cure both failed reviews by requiring citation ledger fields and claim-support boundaries.",
                    "review_findings_refs": ["rev_1", "rev_2"],
                    "acceptance_checks": [
                        "rubric rewrites the failed review finding into concrete citation-ledger checks",
                        "rubric separates verified, provisional, and quarantined source classes"
                    ],
                    "status": "ready"
                },
                "rationale": "tighten rubric after failed reviews"
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&update)
            .expect_err("repair-like task update must bind the blocker or replacement target");
        assert!(err.to_string().contains("blocker_refs"), "{err}");
        assert!(err.to_string().contains("replacement_of_task_ids"), "{err}");
    }

    #[test]
    fn update_board_task_rejects_self_replacement_ref() {
        let workspace_root = temp_tool_workspace("self_replacement_update_contract");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_self_replacement_update".to_string(),
                job_id: Some("job_tool_self_replacement_update".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let task_id = "research_stage_task::stage_1::acceptance_standard_setting_v1";
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": task_id,
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "acceptance standard setting",
                    "worker_role": "stage_standard_setter",
                    "objective": "Set the literature acceptance standard for row-level source evidence.",
                    "input_artifact_refs": ["auto_research_job:job_tool_self_replacement_update"],
                    "required_output_artifact_type": "stage_acceptance_rubric",
                    "required_output_fields": ["full rubric markdown"],
                    "acceptance_checks": ["rubric defines row-level citation ledger requirements"],
                    "failure_signals": ["generic rubric"],
                    "depends_on_task_ids": [],
                    "priority": 10,
                    "evidence_standard": "reproducibility_review"
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&publish)
            .expect("base task should publish");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": task_id,
                "patch": {
                    "replacement_of_task_ids": [task_id]
                },
                "rationale": "try to bind replacement to self"
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&update)
            .expect_err("board tasks must not replace themselves");
        assert!(err.to_string().contains("must not cite itself"), "{err}");
        assert!(err.to_string().contains("replacement_of_task_ids"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_artifact_repair_for_open_evidence_gap_without_worker_dependency()
    {
        let workspace_root = temp_tool_workspace("evidence_gap_artifact_repair_blocked");
        let data_dir = workspace_root.join(".pmcli");
        write_evidence_gap_job_state(&data_dir, "job_gap", "novelty", "stage_1", "rev_gap");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_evidence_gap".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::novelty_report_rewrite",
                    "stage_id": "novelty",
                    "stage_execution_id": "stage_1",
                    "task_type": "artifact_repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Rewrite the novelty report after failed review rev_gap.",
                    "input_artifact_refs": ["auto_research_job:job_gap"],
                    "required_output_artifact_type": "novelty_report",
                    "required_output_fields": ["clean canonical artifact"],
                    "acceptance_checks": ["rewrite the unsupported novelty claim identified by review finding rev_gap#evidence-gap"],
                    "failure_signals": ["prose-only memo"],
                    "depends_on_task_ids": [],
                    "blocker_refs": ["rev_gap"],
                    "review_findings_refs": ["review:rev_gap#evidence-gap"],
                    "evidence_standard": "post-review evidence-gap worker evidence must exist before synthesis"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("artifact rewrite must not close an evidence gap before evidence work");
        assert!(
            err.to_string().contains("evidence-gap failed review"),
            "{err}"
        );
        assert!(err.to_string().contains("rev_gap"), "{err}");
    }

    #[test]
    fn publish_board_tasks_allows_targeted_evidence_task_for_open_evidence_gap() {
        let workspace_root = temp_tool_workspace("evidence_gap_targeted_task");
        let data_dir = workspace_root.join(".pmcli");
        write_evidence_gap_job_state(&data_dir, "job_gap", "novelty", "stage_1", "rev_gap");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_evidence_gap_task".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::contradiction_prior_audit",
                    "stage_id": "novelty",
                    "stage_execution_id": "stage_1",
                    "task_type": "closest-prior search",
                    "worker_role": "novelty_reviewer",
                    "objective": "Mount and audit named contradiction/conflict-resolution benchmark priors required by rev_gap.",
                    "input_artifact_refs": ["auto_research_job:job_gap", "review:rev_gap"],
                    "required_output_artifact_type": "novelty_report",
                    "required_output_fields": ["named comparator table", "overlap dimensions", "residual risk"],
                    "acceptance_checks": ["mounts named priors", "states novelty impact"],
                    "failure_signals": ["generic prose without named comparators"],
                    "depends_on_task_ids": [],
                    "blocker_refs": ["rev_gap"],
                    "review_findings_refs": ["rev_gap"],
                    "evidence_standard": "post-review accepted evidence must audit the missing closest-prior family"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("targeted evidence work should remain a main-agent choice");
    }

    #[test]
    fn publish_board_tasks_allows_artifact_repair_depending_on_evidence_gap_worker_task() {
        let workspace_root = temp_tool_workspace("evidence_gap_synthesis_dependency");
        let data_dir = workspace_root.join(".pmcli");
        write_evidence_gap_job_state(&data_dir, "job_gap", "novelty", "stage_1", "rev_gap");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_evidence_gap_dependency".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [
                    {
                        "task_id": "research_stage_task::stage_1::contradiction_prior_audit",
                        "stage_id": "novelty",
                        "stage_execution_id": "stage_1",
                        "task_type": "closest-prior search",
                        "worker_role": "novelty_reviewer",
                        "objective": "Mount named contradiction priors.",
                        "input_artifact_refs": ["auto_research_job:job_gap", "review:rev_gap"],
                        "required_output_artifact_type": "novelty_report",
                        "required_output_fields": ["named comparator table"],
                        "acceptance_checks": ["mounts named priors"],
                        "failure_signals": ["no named priors"],
                        "depends_on_task_ids": [],
                        "priority": 1,
                        "blocker_refs": ["rev_gap"],
                        "review_findings_refs": ["rev_gap"],
                        "evidence_standard": "post-review accepted evidence must audit the missing closest-prior family"
                    },
                    {
                        "task_id": "research_stage_task::stage_1::novelty_synthesis_after_prior_audit",
                        "stage_id": "novelty",
                        "stage_execution_id": "stage_1",
                        "task_type": "artifact repair",
                        "worker_role": "research_synthesizer",
                        "objective": "Synthesize the novelty report only after the contradiction prior audit is accepted.",
                        "input_artifact_refs": ["auto_research_job:job_gap", "research_stage_task::stage_1::contradiction_prior_audit"],
                        "required_output_artifact_type": "novelty_report",
                        "required_output_fields": ["clean canonical artifact", "accepted evidence citations"],
                        "acceptance_checks": ["citation mapping cites accepted prior audit for review finding rev_gap#evidence-gap"],
                        "failure_signals": ["synthesis runs without accepted evidence"],
                        "depends_on_task_ids": ["research_stage_task::stage_1::contradiction_prior_audit"],
                        "priority": 2,
                        "blocker_refs": ["rev_gap"],
                        "review_findings_refs": ["review:rev_gap#evidence-gap"],
                        "evidence_standard": "synthesis must cite post-review accepted evidence-gap worker evidence"
                    }
                ]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("dependent synthesis task should be publishable after evidence task");
    }

    #[test]
    fn update_board_task_rejects_artifact_repair_ready_for_open_evidence_gap_without_dependency() {
        let workspace_root = temp_tool_workspace("evidence_gap_update_blocked");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_evidence_gap_update".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let publish_without_active_gap = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::novelty_report_rewrite",
                    "stage_id": "novelty",
                    "stage_execution_id": "stage_1",
                    "task_type": "artifact_repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Rewrite the novelty report.",
                    "input_artifact_refs": ["auto_research_job:job_gap"],
                    "required_output_artifact_type": "novelty_report",
                    "required_output_fields": ["clean canonical artifact"],
                    "acceptance_checks": ["rewrite the unsupported novelty claim identified by review finding rev_gap#evidence-gap"],
                    "failure_signals": ["prose-only memo"],
                    "depends_on_task_ids": [],
                    "blocker_refs": ["rev_gap"],
                    "review_findings_refs": ["review:rev_gap#evidence-gap"],
                    "evidence_standard": "stage artifact candidate must be reviewable"
                }]
            }),
            session_id: None,
            turn_id: None,
        };
        executor
            .execute(&publish_without_active_gap)
            .expect("fixture task should publish before evidence gap job exists");
        write_evidence_gap_job_state(&data_dir, "job_gap", "novelty", "stage_1", "rev_gap");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "research_stage_task::stage_1::novelty_report_rewrite",
                "patch": {
                    "status": "ready",
                    "objective": "Repair rev_gap by rewriting the novelty report body."
                },
                "rationale": "try prose repair"
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&update)
            .expect_err("update must not mark artifact-only repair as evidence-gap closure");
        assert!(
            err.to_string().contains("evidence-gap failed review"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_allows_artifact_repair_after_post_review_evidence_gap_worker_evidence() {
        let workspace_root = temp_tool_workspace("evidence_gap_post_evidence_allows_synthesis");
        let data_dir = workspace_root.join(".pmcli");
        write_evidence_gap_job_state(&data_dir, "job_gap", "novelty", "stage_1", "rev_gap");
        write_post_review_evidence_index(&workspace_root, "job_gap", "stage_1");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_post_evidence".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::novelty_report_synthesis",
                    "stage_id": "novelty",
                    "stage_execution_id": "stage_1",
                    "task_type": "artifact_repair",
                    "worker_role": "research_synthesizer",
                    "objective": "Synthesize the accepted post-review closest-prior evidence into the canonical novelty report.",
                    "input_artifact_refs": ["auto_research_job:job_gap", "accepted_worker_evidence_task:research_stage_task::stage_1::closest_prior_search"],
                    "required_output_artifact_type": "novelty_report",
                    "required_output_fields": ["clean canonical artifact", "accepted evidence citations"],
                    "acceptance_checks": ["citation mapping cites accepted closest-prior evidence for review finding rev_gap#evidence-gap"],
                    "failure_signals": ["synthesis omits accepted evidence"],
                    "depends_on_task_ids": [],
                    "blocker_refs": ["rev_gap"],
                    "review_findings_refs": ["review:rev_gap#evidence-gap"],
                    "evidence_standard": "synthesis must cite post-review accepted evidence-gap worker evidence"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("post-review evidence enables synthesis repair");
    }

    #[test]
    fn publish_board_tasks_rejects_worker_repair_for_snapshot_only_review() {
        let workspace_root = temp_tool_workspace("snapshot_only_rejects_worker_repair");
        let data_dir = workspace_root.join(".pmcli");
        write_adoption_snapshot_only_job_state(
            &data_dir,
            "job_snapshot",
            "literature",
            "stage_1",
            "rev_snapshot",
        );
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_snapshot_only".to_string(),
                job_id: Some("job_snapshot".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::snapshot_diagnosis",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "runtime diagnosis",
                    "worker_role": "research_analyst",
                    "objective": "Diagnose snapshot drift for rev_snapshot.",
                    "input_artifact_refs": ["review:rev_snapshot"],
                    "required_output_artifact_type": "runtime_diagnosis_report",
                    "required_output_fields": ["root cause", "repair recommendation"],
                    "acceptance_checks": ["explains rev_snapshot snapshot drift"],
                    "failure_signals": ["does not bind rev_snapshot"],
                    "blocker_refs": ["rev_snapshot"],
                    "review_findings_refs": ["review:rev_snapshot#stage_artifact_adoption_snapshot"],
                    "evidence_standard": "diagnose the snapshot blocker"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("snapshot-only review should require fresh adoption, not worker repair");
        assert!(err.to_string().contains("fresh adoption"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_synthesis_consuming_runtime_diagnosis() {
        let workspace_root = temp_tool_workspace("synthesis_rejects_runtime_diagnosis");
        let data_dir = workspace_root.join(".pmcli");
        write_passing_job_state(&data_dir, "job_pass", "stage_1", "rev_pass");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_synthesis_runtime_diagnosis".to_string(),
                job_id: Some("job_pass".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::stage_artifact_synthesis",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "stage artifact synthesis",
                    "worker_role": "research_synthesizer",
                    "objective": "Produce the literature matrix from literature evidence.",
                    "input_artifact_refs": ["accepted_worker_evidence_task:research_stage_task::stage_1::repair_loop_strategy_diagnosis"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["literature matrix", "evidence binding ledger"],
                    "acceptance_checks": ["all claims bind to literature evidence"],
                    "failure_signals": ["control-plane prose appears in the artifact"],
                    "evidence_standard": "only stage-content evidence may support synthesis"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("stage synthesis must not consume runtime diagnosis evidence");
        assert!(err.to_string().contains("control-plane diagnosis"), "{err}");
    }

    #[test]
    fn publish_board_tasks_rejects_stage_synthesis_after_post_review_evidence_without_accepted_evidence_refs(
    ) {
        let workspace_root =
            temp_tool_workspace("evidence_gap_post_evidence_synthesis_missing_refs");
        let data_dir = workspace_root.join(".pmcli");
        write_evidence_gap_job_state(&data_dir, "job_gap", "literature", "stage_1", "rev_gap");
        write_post_review_evidence_index(&workspace_root, "job_gap", "stage_1");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_post_evidence_missing_refs".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::stage_artifact_synthesis",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "stage artifact synthesis",
                    "worker_role": "research_synthesizer",
                    "objective": "Rebuild the literature matrix body after failed review rev_gap.",
                    "input_artifact_refs": ["review:rev_gap", "research/stages/job_gap/literature/literature_matrix.md"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["clean canonical artifact", "citation ledger"],
                    "acceptance_checks": ["rewrite unsupported claim with auditable citation mapping for review finding rev_gap#evidence-gap"],
                    "failure_signals": ["uses S1-S9 shorthand without mounted accepted evidence"],
                    "depends_on_task_ids": ["research_stage_task::stage_1::closest_prior_search"],
                    "blocker_refs": ["rev_gap"],
                    "review_findings_refs": ["review:rev_gap#evidence-gap"],
                    "evidence_standard": "synthesis must consume accepted post-review worker evidence"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("post-review synthesis must mount accepted evidence bundles");
        assert!(
            err.to_string().contains("accepted_worker_evidence_task:*"),
            "{err}"
        );
        assert!(
            err.to_string().contains("depends_on_task_ids")
                && err.to_string().contains("scheduling dependency"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_allows_stage_synthesis_after_post_review_evidence_with_accepted_index_ref(
    ) {
        let workspace_root = temp_tool_workspace("evidence_gap_post_evidence_synthesis_index_ref");
        let data_dir = workspace_root.join(".pmcli");
        write_evidence_gap_job_state(&data_dir, "job_gap", "literature", "stage_1", "rev_gap");
        write_post_review_evidence_index(&workspace_root, "job_gap", "stage_1");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_post_evidence_index_ref".to_string(),
                job_id: Some("job_gap".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "research_stage_task::stage_1::stage_artifact_synthesis",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "stage artifact synthesis",
                    "worker_role": "research_synthesizer",
                    "objective": "Rebuild the literature matrix body after failed review rev_gap.",
                    "input_artifact_refs": [
                        "review:rev_gap",
                        "accepted_worker_evidence_index:research/stages/job_gap/stage_1/accepted_worker_evidence/index.json"
                    ],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["clean canonical artifact", "citation ledger"],
                    "acceptance_checks": ["rewrite unsupported claim with auditable citation mapping for review finding rev_gap#evidence-gap"],
                    "failure_signals": ["uses S1-S9 shorthand without mounted accepted evidence"],
                    "depends_on_task_ids": ["research_stage_task::stage_1::closest_prior_search"],
                    "blocker_refs": ["rev_gap"],
                    "review_findings_refs": ["review:rev_gap#evidence-gap"],
                    "evidence_standard": "synthesis must consume accepted post-review worker evidence"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("accepted evidence index ref should mount the post-review evidence set");
    }

    #[test]
    fn publish_board_tasks_rejects_review_without_explicit_target() {
        let workspace_root = temp_tool_workspace("review_target_required");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_review_target".to_string(),
                job_id: Some("job_tool_review_target".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::semantic_review",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "semantic review",
                    "worker_role": "research_quality_reviewer",
                    "objective": "Review the accepted evidence.",
                    "input_artifact_refs": [],
                    "required_output_artifact_type": "semantic_review_report",
                    "required_output_fields": ["verdict"],
                    "acceptance_checks": ["binds verdict to target evidence"],
                    "failure_signals": ["no target evidence"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "review must target explicit evidence",
                    "review_findings_refs": []
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("review task without target should be rejected");
        assert!(
            err.to_string().contains("review task")
                && err.to_string().contains("review_target_task_ids"),
            "{err}"
        );
    }

    #[test]
    fn publish_board_tasks_persists_review_targets() {
        let workspace_root = temp_tool_workspace("review_target_persisted");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_review_target_persisted".to_string(),
                job_id: Some("job_tool_review_target_persisted".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::semantic_review",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "semantic review",
                    "worker_role": "research_quality_reviewer",
                    "objective": "Review the accepted evidence.",
                    "input_artifact_refs": ["accepted_worker_evidence_task:paper_search"],
                    "required_output_artifact_type": "semantic_review_report",
                    "required_output_fields": ["verdict"],
                    "acceptance_checks": ["binds verdict to target evidence"],
                    "failure_signals": ["no target evidence"],
                    "depends_on_task_ids": ["paper_search"],
                    "priority": 1,
                    "evidence_standard": "review must target explicit evidence",
                    "review_findings_refs": [],
                    "review_target_task_ids": ["paper_search"],
                    "review_target_evidence_refs": ["accepted_worker_evidence_task:paper_search"],
                    "consumed_input_required": true
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("targeted review should publish");
        let task_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join("board__stage_1__semantic_review.json");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(task_path).expect("task should exist"))
                .expect("task should parse");
        assert_eq!(
            value
                .get("review_target_task_ids")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("paper_search")
        );
        assert_eq!(
            value
                .get("consumed_input_required")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn publish_board_tasks_rejects_nonexistent_dependency_task_id() {
        let workspace_root = temp_tool_workspace("nonexistent_dependency_task_id");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_nonexistent_dependency".to_string(),
                job_id: Some("job_tool_nonexistent_dependency".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "board::stage_1::postrepair_synthesis",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "source verification",
                "worker_role": "citation_auditor",
                "objective": "Verify post-review evidence.",
                "input_artifact_refs": ["review_packet:rev_failed"],
                "required_output_artifact_type": "source_verification_report",
                "required_output_fields": ["ledger"],
                "acceptance_checks": ["checks concrete evidence"],
                "failure_signals": ["stale dependency id"],
                "depends_on_task_ids": ["board::stage_1::evidence_binding_repair_v8"],
                "priority": 1,
                "evidence_standard": "must use real upstream evidence",
                "review_findings_refs": []
            }]
        });

        let err = executor
            .execute(&publish)
            .expect_err("stale dependency should be rejected before persistence");
        let message = format!("{err}");

        assert!(message.contains("depends_on_task_ids"));
        assert!(message.contains("evidence_binding_repair_v8"));
        assert!(message.contains("no matching published board task"));
    }

    #[test]
    fn update_board_task_rejects_nonexistent_dependency_task_id() {
        let workspace_root = temp_tool_workspace("update_nonexistent_dependency_task_id");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_update_nonexistent_dependency".to_string(),
                job_id: Some("job_tool_update_nonexistent_dependency".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let mut publish = ToolCall::new("publish_board_tasks");
        publish.arguments = serde_json::json!({
            "tasks": [{
                "task_id": "board::stage_1::source_verification",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "source verification",
                "worker_role": "citation_auditor",
                "objective": "Verify source evidence.",
                "input_artifact_refs": ["main_agent_round:1"],
                "required_output_artifact_type": "source_verification_report",
                "required_output_fields": ["ledger"],
                "acceptance_checks": ["checks concrete evidence"],
                "failure_signals": ["stale dependency id"],
                "depends_on_task_ids": [],
                "priority": 1,
                "evidence_standard": "must use real upstream evidence",
                "review_findings_refs": []
            }]
        });
        executor
            .execute(&publish)
            .expect("baseline task should publish");

        let mut update = ToolCall::new("update_board_task");
        update.arguments = serde_json::json!({
            "task_id": "board::stage_1::source_verification",
            "patch": {
                "depends_on_task_ids": ["board::stage_1::evidence_binding_repair_v8"]
            },
            "rationale": "try to add stale dependency"
        });

        let err = executor
            .execute(&update)
            .expect_err("stale dependency should be rejected on update");
        let message = format!("{err}");

        assert!(message.contains("depends_on_task_ids"));
        assert!(message.contains("evidence_binding_repair_v8"));
        assert!(message.contains("no matching published board task"));
    }

    #[test]
    fn publish_board_tasks_rejects_stale_auto_research_job_ref() {
        let workspace_root = temp_tool_workspace("stale_auto_research_job_ref");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_context_refs".to_string(),
                job_id: Some("arj_current".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let mut publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::paper_search",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "Find concrete sources.",
                    "input_artifact_refs": ["auto_research_job:arj_stale"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["source entries"],
                    "acceptance_checks": ["cites concrete sources"],
                    "failure_signals": ["generic narrative only"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "source rows must include concrete refs",
                    "review_findings_refs": []
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("stale job refs should be rejected");
        assert!(err.to_string().contains("stale auto_research_job ref"));
        assert!(err.to_string().contains("auto_research_job:arj_current"));

        publish.arguments["tasks"][0]["input_artifact_refs"] =
            serde_json::json!(["auto_research_job:arj_current"]);
        executor
            .execute(&publish)
            .expect("current job ref should publish");
    }

    #[test]
    fn publish_and_update_board_tasks_reject_stage_closure_ledger_refs() {
        let workspace_root = temp_tool_workspace("stage_closure_ledger_board_refs");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_ledger_ref".to_string(),
                job_id: Some("arj_current".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let mut publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::paper_search",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "Find concrete sources for a missing task type.",
                    "input_artifact_refs": ["stage_closure_ledger:stage_1:missing_task_type:paper_search"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["source entries"],
                    "acceptance_checks": ["cites concrete sources"],
                    "failure_signals": ["generic narrative only"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "source rows must include concrete refs",
                    "review_findings_refs": []
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("ledger refs are not worker task input refs");
        assert!(err.to_string().contains("stage_closure_ledger"));
        assert!(err.to_string().contains("main-agent-only"));
        assert!(err.to_string().contains("blocker_refs"));

        publish.arguments["tasks"][0]["input_artifact_refs"] =
            serde_json::json!(["auto_research_job:arj_current"]);
        executor
            .execute(&publish)
            .expect("valid context ref should publish");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "board::stage_1::paper_search",
                "patch": {
                    "review_target_evidence_refs": ["stage_closure_ledger:review_rerun_blockers"]
                },
                "rationale": "try to target ledger"
            }),
            session_id: None,
            turn_id: None,
        };
        let err = executor
            .execute(&update)
            .expect_err("ledger refs are not review target evidence refs");
        assert!(err.to_string().contains("review_target_evidence_refs"));
        assert!(err.to_string().contains("stage_closure_ledger"));
    }

    #[test]
    fn publish_and_update_board_tasks_reject_mistyped_accepted_evidence_index_refs() {
        let workspace_root = temp_tool_workspace("accepted_evidence_index_ref_shape");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_index_ref_shape".to_string(),
                job_id: Some("arj_current".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let mut publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::open_problem_extraction",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "open-problem extraction",
                    "worker_role": "literature_gap_analyst",
                    "objective": "Extract benchmark gaps from accepted evidence.",
                    "input_artifact_refs": ["accepted_worker_evidence_index:research/stages/arj_current/literature/literature_matrix.md"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["gap rows", "supporting citations"],
                    "acceptance_checks": ["every row cites accepted evidence"],
                    "failure_signals": ["unsupported gap claims"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "gap rows must be grounded in accepted evidence"
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&publish)
            .expect_err("accepted evidence index refs must not point at stage artifacts");
        assert!(err
            .to_string()
            .contains("accepted_worker_evidence/index.json"));
        assert!(err.to_string().contains("literature_matrix.md"));

        publish.arguments["tasks"][0]["input_artifact_refs"] = serde_json::json!([
            "accepted_worker_evidence_index:research/stages/arj_current/stage_1/accepted_worker_evidence/index.json"
        ]);
        executor
            .execute(&publish)
            .expect("valid accepted evidence index ref shape should publish");

        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "board::stage_1::open_problem_extraction",
                "patch": {
                    "review_target_evidence_refs": [
                        "accepted_worker_evidence_index:research/stages/arj_current/literature/literature_matrix.md"
                    ]
                },
                "rationale": "try to target the accepted evidence index"
            }),
            session_id: None,
            turn_id: None,
        };
        let err = executor
            .execute(&update)
            .expect_err("update must share accepted evidence index ref validation");
        assert!(err.to_string().contains("review_target_evidence_refs"));
        assert!(err
            .to_string()
            .contains("accepted_worker_evidence/index.json"));
        assert!(err.to_string().contains("literature_matrix.md"));
    }

    #[test]
    fn publish_and_update_board_tasks_normalize_review_artifact_refs() {
        let workspace_root = temp_tool_workspace("review_artifact_ref_normalization");
        let data_dir = workspace_root.join(".pmcli");
        for review_id in ["rev_123", "rev_456"] {
            let review_dir = data_dir.join("reviews").join(review_id);
            fs::create_dir_all(&review_dir).expect("review dir should exist");
            fs::write(review_dir.join("packet.json"), "{}").expect("packet should write");
            fs::write(review_dir.join("trace.latest.json"), "{}").expect("trace should write");
        }
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_review_ref".to_string(),
                job_id: Some("arj_current".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let review_trace_ref = format!(
            "review_trace:{}",
            workspace_root
                .join(".pmcli/reviews/rev_123/trace.latest.json")
                .display()
        );

        let publish = ToolCall {
            tool_name: "publish_board_tasks".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "tasks": [{
                    "task_id": "board::stage_1::paper_search",
                    "stage_id": "literature",
                    "stage_execution_id": "stage_1",
                    "task_type": "paper search",
                    "worker_role": "literature_researcher",
                    "objective": "Find concrete sources.",
                    "input_artifact_refs": ["review_packet:.pmcli/reviews/rev_123/packet.json"],
                    "required_output_artifact_type": "literature_matrix",
                    "required_output_fields": ["source entries"],
                    "acceptance_checks": ["cites concrete sources"],
                    "failure_signals": ["generic narrative only"],
                    "depends_on_task_ids": [],
                    "priority": 1,
                    "evidence_standard": "source rows must include concrete refs",
                    "review_target_evidence_refs": [review_trace_ref]
                }]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&publish)
            .expect("mixed review refs should normalize during publish");
        let task_path = data_dir
            .join("main-agent-board")
            .join("tasks")
            .join("board__stage_1__paper_search.json");
        let stored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&task_path).expect("task should exist"))
                .expect("task should parse");
        assert_eq!(
            stored
                .get("input_artifact_refs")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("review_packet:rev_123")
        );
        assert_eq!(
            stored
                .get("review_target_evidence_refs")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("review_trace:rev_123")
        );

        let mut update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "board::stage_1::paper_search",
                "rationale": "tighten review refs"
            }),
            session_id: None,
            turn_id: None,
        };
        update.arguments["patch"] = serde_json::json!({
            "input_artifact_refs": ["review_packet:.pmcli/reviews/rev_456/packet.json"],
            "review_target_evidence_refs": ["review_trace:.pmcli/reviews/rev_456/traces/trc_456.json"]
        });

        executor
            .execute(&update)
            .expect("mixed review refs should normalize during update");
        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&task_path).expect("task should exist"))
                .expect("task should parse");
        assert_eq!(
            updated
                .get("input_artifact_refs")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("review_packet:rev_456")
        );
        assert_eq!(
            updated
                .get("review_target_evidence_refs")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("review_trace:rev_456")
        );
    }

    #[test]
    fn update_board_task_rejects_missing_required_review_input_ref() {
        let workspace_root = temp_tool_workspace("missing_required_review_input_ref");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_missing_review_ref".to_string(),
                job_id: Some("arj_current".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let task_dir = data_dir.join("main-agent-board").join("tasks");
        fs::create_dir_all(&task_dir).expect("task dir should exist");
        fs::write(
            task_dir.join("board__stage_1__paper_search.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "main_agent_board_task.v1",
                "task_id": "board::stage_1::paper_search",
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "task_type": "paper search",
                "worker_role": "literature_researcher",
                "objective": "Find concrete sources.",
                "input_artifact_refs": [],
                "required_output_artifact_type": "literature_matrix",
                "required_output_fields": ["source entries"],
                "acceptance_checks": ["cites concrete sources"],
                "failure_signals": ["generic narrative only"],
                "depends_on_task_ids": [],
                "priority": 1,
                "status": "published"
            }))
            .expect("task should serialize"),
        )
        .expect("task should write");
        let update = ToolCall {
            tool_name: "update_board_task".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "task_id": "board::stage_1::paper_search",
                "patch": {
                    "input_artifact_refs": ["review_packet:rev_missing"]
                },
                "rationale": "attach review context"
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&update)
            .expect_err("missing required review input should fail during update");

        assert!(err
            .to_string()
            .contains("unresolved required review artifact"));
        assert!(err.to_string().contains("review_packet:rev_missing"));
    }

    #[test]
    fn record_worker_artifact_decision_normalizes_review_trace_id_refs() {
        let workspace_root = temp_tool_workspace("worker_decision_review_trace_ref_normalization");
        let data_dir = workspace_root.join(".pmcli");
        let review_dir = data_dir.join("reviews").join("rev_123");
        fs::create_dir_all(review_dir.join("traces")).expect("review trace dir should exist");
        fs::write(
            review_dir.join("trace.latest.json"),
            r#"{"review_id":"rev_123","trace_id":"trc_456"}"#,
        )
        .expect("trace should write");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_worker_decision_ref".to_string(),
                job_id: Some("arj_current".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );

        let call = ToolCall {
            tool_name: "record_worker_artifact_decision".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "agent_id": "agent_1",
                "task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                "candidate_refs": ["candidate.md"],
                "decision": "accept",
                "rationale": "candidate is usable after review",
                "review_required": false,
                "cleanup_required": false,
                "readiness_refs": ["review_trace:trc_456"]
            }),
            session_id: None,
            turn_id: None,
        };

        executor
            .execute(&call)
            .expect("worker decision should record");
        let decision_dir = data_dir
            .join("main-agent-board")
            .join("worker-artifact-decisions");
        let decision_path = fs::read_dir(decision_dir)
            .expect("decision dir should exist")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .expect("decision record should exist");
        let stored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(decision_path).expect("record should read"))
                .expect("record should parse");
        assert_eq!(
            stored
                .get("readiness_refs")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("review_trace:rev_123")
        );
    }

    #[test]
    fn adopt_stage_artifact_rejects_previously_rejected_candidate() {
        let workspace_root = temp_tool_workspace("adoption_previous_rejection");
        let data_dir = workspace_root.join(".pmcli");
        let rejection_dir = workspace_root
            .join("research")
            .join("stages")
            .join("job_tool_adoption")
            .join("stage_1")
            .join("stage_artifact_adoption_rejections");
        fs::create_dir_all(&rejection_dir).expect("rejection dir should be created");
        fs::write(
            rejection_dir.join("old_rejection.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "autonomous_research.stage_artifact_adoption_rejection.v1",
                "adoption_ref": "old_adoption",
                "stage_id": "novelty",
                "stage_execution_id": "stage_1",
                "source_agent_id": "agent_dirty",
                "source_task_id": "research_stage_task::stage_1::artifact_repair",
                "source_ref": ".pmcli/agents/agent_dirty/worktree_artifact_candidates.json",
                "source_artifact_path": "research/stages/job_tool_adoption/novelty/novelty_report_repair_candidate.md",
                "target_artifact_path": "research/stages/job_tool_adoption/novelty/novelty_report.md",
                "reason": "candidate-only lifecycle wording"
            }))
            .expect("manifest should serialize"),
        )
        .expect("manifest should write");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_adoption".to_string(),
                job_id: Some("job_tool_adoption".to_string()),
                stage_id: Some("novelty".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("novelty_report".to_string()),
            },
        );
        let call = ToolCall {
            tool_name: "adopt_stage_artifact".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "stage_id": "novelty",
                "stage_execution_id": "stage_1",
                "source_agent_id": "agent_dirty",
                "source_task_id": "research_stage_task::stage_1::artifact_repair",
                "source_ref": ".pmcli/agents/agent_dirty/worktree_artifact_candidates.json",
                "source_artifact_path": "research/stages/job_tool_adoption/novelty/novelty_report_repair_candidate.md",
                "target_artifact_path": "research/stages/job_tool_adoption/novelty/novelty_report.md",
                "rationale": "Retry the same stale candidate.",
                "evidence_refs": ["review:rev_1"],
                "request_review_rerun": true
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&call)
            .expect_err("stale rejected adoption should fail at tool boundary");

        let err_text = err.to_string();
        assert!(
            err_text.contains("requires `replacement_of_artifact_ids`"),
            "unexpected adoption rejection: {err_text}"
        );
        assert!(
            err_text.contains("first adoption")
                && err_text.contains("replacement_of_artifact_ids: []"),
            "adoption rejection should tell the main agent how to retry first adoption: {err_text}"
        );
        assert!(
            err_text.contains("CanonicalArtifactLedger"),
            "adoption rejection should tell the main agent where replacement ids come from: {err_text}"
        );
        assert!(
            !executor
                .context
                .as_ref()
                .unwrap()
                .data_dir
                .join("main-agent-board")
                .join("stage-artifact-adoptions")
                .exists(),
            "failed tool call must not write a new adoption record"
        );
    }

    #[test]
    fn snapshot_rebind_normalizes_canonical_source_and_ignores_old_transition_rejection() {
        let workspace_root = temp_tool_workspace("snapshot_rebind_old_transition_rejection");
        let data_dir = workspace_root.join(".pmcli");
        let source_ref = ".pmcli/agents/agent_synth/output_manifest.json";
        let source_artifact_path = "literature_matrix_candidate.md";
        let target_artifact_path = "research/stages/job/literature/literature_matrix.md";
        let entry = canonical_artifacts::upsert_adoption_requested(
            &data_dir,
            canonical_artifacts::CanonicalArtifactSeed {
                job_id: "job".to_string(),
                stage_id: "literature".to_string(),
                stage_execution_id: "stage_1".to_string(),
                source_agent_id: "agent_synth".to_string(),
                source_task_id: Some(
                    "research_stage_task::stage_1::stage_artifact_synthesis".to_string(),
                ),
                source_ref: source_ref.to_string(),
                source_artifact_path: Some(source_artifact_path.to_string()),
                target_artifact_path: target_artifact_path.to_string(),
                artifact_kind: "literature_matrix".to_string(),
                task_type: "stage artifact synthesis".to_string(),
                decision_ref: "main_agent_stage_artifact_adoption::first".to_string(),
                source_sha256: None,
            },
            "first adoption",
        )
        .expect("adoption request should record");
        canonical_artifacts::record_materialized(&data_dir, &entry.artifact_id, "abc".to_string())
            .expect("artifact should materialize");
        canonical_artifacts::record_baseline_visible(
            &data_dir,
            &entry.artifact_id,
            "baseline".to_string(),
        )
        .expect("artifact should become visible");
        let rejection_dir =
            workspace_root.join("research/stages/job/stage_1/stage_artifact_adoption_rejections");
        fs::create_dir_all(&rejection_dir).expect("rejection dir should create");
        fs::write(
            rejection_dir.join("old_transition.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "source_agent_id": "agent_synth",
                "source_task_id": "research_stage_task::stage_1::stage_artifact_synthesis",
                "source_ref": source_ref,
                "source_artifact_path": source_artifact_path,
                "target_artifact_path": target_artifact_path,
                "reason": format!(
                    "invalid canonical artifact transition for {}: baseline_visible -> adoption_requested",
                    entry.artifact_id
                )
            }))
            .expect("rejection should serialize"),
        )
        .expect("rejection should write");
        let context = LocalToolExecutorContext {
            data_dir,
            project_id: "project".to_string(),
            job_id: Some("job".to_string()),
            stage_id: Some("literature".to_string()),
            stage_execution_id: Some("stage_1".to_string()),
            stage_artifact_type: Some("literature_matrix".to_string()),
        };
        let call = ToolCall {
            tool_name: "adopt_stage_artifact".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::Value::Null,
            session_id: None,
            turn_id: None,
        };
        let normalized = normalize_snapshot_rebind_source(
            &call,
            &context,
            "agent_synth",
            Some("research_stage_task::stage_1::stage_artifact_synthesis"),
            &entry.artifact_id,
            Some(source_artifact_path),
            target_artifact_path,
            std::slice::from_ref(&entry.artifact_id),
        )
        .expect("canonical source should normalize")
        .expect("canonical source should be a snapshot rebind");
        assert_eq!(normalized.1, source_ref);
        assert!(stage_artifact_adoption_previous_rejection_reason(
            &workspace_root,
            &context,
            "literature",
            "stage_1",
            "agent_synth",
            Some("research_stage_task::stage_1::stage_artifact_synthesis"),
            &normalized.1,
            normalized.2.as_deref(),
            target_artifact_path,
            Some(&entry.artifact_id),
        )
        .expect("rejection lookup should succeed")
        .is_none());
    }

    #[test]
    fn adopt_stage_artifact_rejects_stage_governance_target() {
        let workspace_root = temp_tool_workspace("adoption_governance_target");
        let data_dir = workspace_root.join(".pmcli");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir,
                project_id: "project_tool_governance_target".to_string(),
                job_id: Some("job_tool_governance_target".to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let call = ToolCall {
            tool_name: "adopt_stage_artifact".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "stage_id": "literature",
                "stage_execution_id": "stage_1",
                "source_agent_id": "agent_rubric",
                "source_task_id": "research_stage_task::stage_1::acceptance_standard_setting",
                "source_ref": ".pmcli/agents/agent_rubric/worktree_artifact_candidates.json",
                "source_artifact_path": "stage_acceptance_rubric_literature.md",
                "target_artifact_path": "research/stages/job_tool_governance_target/literature/stage_acceptance_rubric.md",
                "rationale": "Attempt to adopt rubric through final stage artifact tool.",
                "evidence_refs": ["accepted_worker_evidence:acceptance standard setting"],
                "replacement_of_artifact_ids": [],
                "request_review_rerun": false
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&call)
            .expect_err("stage governance docs must not use final artifact adoption");

        let err_text = err.to_string();
        assert!(
            err_text.contains("stage governance documents"),
            "{err_text}"
        );
        assert!(
            err_text.contains("record_stage_evidence_plan"),
            "{err_text}"
        );
        assert!(
            !executor
                .context
                .as_ref()
                .unwrap()
                .data_dir
                .join("main-agent-board")
                .join("stage-artifact-adoptions")
                .exists(),
            "failed governance adoption must not write a final adoption record"
        );
    }

    #[test]
    fn adopt_stage_artifact_persists_replacement_of_artifact_ids() {
        let workspace_root = temp_tool_workspace("adoption_replacement_ids");
        let data_dir = workspace_root.join(".pmcli");
        let project_id = "project_tool_replacement";
        let job_id = "job_tool_replacement";
        let stage_execution_id = "stage_replace_1";
        let source_agent_id = "agent_replacement";
        let source_task_id = "task_replacement";
        let source_ref = write_tool_worker_candidate_manifest(
            &workspace_root,
            source_agent_id,
            "baseline_runner.py",
            "def run():\n    return {'ok': True}\n",
        );
        write_tool_adoption_job_state(&data_dir, project_id, job_id, stage_execution_id);
        write_tool_adoption_accepted_evidence_index(
            &workspace_root,
            project_id,
            job_id,
            stage_execution_id,
            source_agent_id,
            source_task_id,
            "implementation",
            "python_source",
            &source_ref,
        );
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: project_id.to_string(),
                job_id: Some(job_id.to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some(stage_execution_id.to_string()),
                stage_artifact_type: Some("implementation_report".to_string()),
            },
        );
        let call = ToolCall {
            tool_name: "adopt_stage_artifact".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "stage_id": "literature",
                "stage_execution_id": stage_execution_id,
                "source_agent_id": source_agent_id,
                "source_task_id": source_task_id,
                "source_ref": source_ref,
                "source_artifact_path": "baseline_runner.py",
                "target_artifact_path": "baseline_runner.py",
                "rationale": "Main agent explicitly replaces the prior canonical runner after review.",
                "evidence_refs": [source_ref],
                "replacement_of_artifact_ids": ["canonical_artifact_old_runner"],
                "cleanup_required": true,
                "request_review_rerun": false
            }),
            session_id: None,
            turn_id: None,
        };

        let result = executor
            .execute(&call)
            .expect("replacement adoption record should be accepted at tool boundary");
        assert_eq!(result.status, "succeeded");
        let structured = result
            .structured
            .as_ref()
            .expect("tool result should include structured preflight details");
        assert_eq!(
            structured["candidate_relative_paths"],
            serde_json::json!(["baseline_runner.py"])
        );
        let adoption_dir = data_dir
            .join("main-agent-board")
            .join("stage-artifact-adoptions");
        let adoption_path = fs::read_dir(&adoption_dir)
            .expect("adoption dir should exist")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .expect("adoption record should be written");
        let record: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(adoption_path).expect("adoption record should read"),
        )
        .expect("adoption record should parse");
        assert_eq!(
            record["replacement_of_artifact_ids"],
            serde_json::json!(["canonical_artifact_old_runner"])
        );
    }

    #[test]
    fn adopt_stage_artifact_rejects_cross_agent_source_ref_at_tool_boundary() {
        let workspace_root = temp_tool_workspace("adoption_cross_agent_source_ref");
        let data_dir = workspace_root.join(".pmcli");
        let project_id = "project_tool_cross_agent";
        let job_id = "job_tool_cross_agent";
        let stage_execution_id = "stage_literature_1";
        let synthesis_agent_id = "agent_synthesis";
        let synthesis_task_id = "research_stage_task::stage_literature_1::stage_artifact_synthesis";
        let synthesis_ref = write_tool_worker_candidate_manifest(
            &workspace_root,
            synthesis_agent_id,
            "literature_matrix_synthesis_candidate.md",
            "stage: literature\nartifact_type: literature_matrix\nsource entries: fixture\n",
        );
        let paper_ref = write_tool_worker_candidate_manifest(
            &workspace_root,
            "agent_paper_search",
            "literature_matrix_candidate.md",
            "stage: literature\nartifact_type: literature_matrix\npaper search fixture\n",
        );
        let paper_manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&paper_ref).expect("manifest should read"))
                .expect("manifest should parse");
        let paper_archive_ref = paper_manifest["candidate_entries"][0]["candidate_archive_ref"]
            .as_str()
            .expect("archive ref should exist")
            .to_string();
        write_tool_adoption_job_state(&data_dir, project_id, job_id, stage_execution_id);
        write_tool_adoption_accepted_evidence_index(
            &workspace_root,
            project_id,
            job_id,
            stage_execution_id,
            synthesis_agent_id,
            synthesis_task_id,
            "stage artifact synthesis",
            "literature_matrix",
            &synthesis_ref,
        );
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: project_id.to_string(),
                job_id: Some(job_id.to_string()),
                stage_id: Some("literature".to_string()),
                stage_execution_id: Some(stage_execution_id.to_string()),
                stage_artifact_type: Some("literature_matrix".to_string()),
            },
        );
        let call = ToolCall {
            tool_name: "adopt_stage_artifact".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "stage_id": "literature",
                "stage_execution_id": stage_execution_id,
                "source_agent_id": synthesis_agent_id,
                "source_task_id": synthesis_task_id,
                "source_ref": paper_archive_ref,
                "source_artifact_path": "literature_matrix_candidate.md",
                "target_artifact_path": "research/stages/job_tool_cross_agent/literature/literature_matrix.md",
                "rationale": "Main agent accidentally combines the synthesis worker with a paper-search archive.",
                "evidence_refs": [synthesis_ref],
                "replacement_of_artifact_ids": [],
                "request_review_rerun": true
            }),
            session_id: None,
            turn_id: None,
        };

        let err = executor
            .execute(&call)
            .expect_err("cross-agent source_ref must fail before writing an adoption record");
        let err_text = err.to_string();
        assert!(
            err_text.contains("source_ref/source_agent_id mismatch"),
            "{err_text}"
        );
        assert!(
            !data_dir
                .join("main-agent-board")
                .join("stage-artifact-adoptions")
                .exists(),
            "failed tool preflight must not write an adoption record"
        );
    }

    #[test]
    fn record_canonical_artifact_integration_check_updates_ledger() {
        let workspace_root = temp_tool_workspace("canonical_integration_tool");
        let data_dir = workspace_root.join(".pmcli");
        fs::write(
            workspace_root.join("baseline_runner.py"),
            "def run():\n    return 1\n",
        )
        .expect("target file should write");
        let seed = canonical_artifacts::CanonicalArtifactSeed {
            job_id: "job_tool_integration".to_string(),
            stage_id: "implement-solution".to_string(),
            stage_execution_id: "stage_1".to_string(),
            source_agent_id: "agent_impl".to_string(),
            source_task_id: Some("task_impl".to_string()),
            source_ref: ".pmcli/agents/agent_impl/worktree_artifact_candidates.json".to_string(),
            source_artifact_path: Some("baseline_runner.py".to_string()),
            target_artifact_path: "baseline_runner.py".to_string(),
            artifact_kind: "python_source".to_string(),
            task_type: "implementation".to_string(),
            decision_ref: "main_agent_stage_artifact_adoption::impl".to_string(),
            source_sha256: None,
        };
        let entry =
            canonical_artifacts::upsert_adoption_requested(&data_dir, seed, "fixture adoption")
                .expect("adoption should record");
        let entry = canonical_artifacts::record_materialized(
            &data_dir,
            &entry.artifact_id,
            "sha".to_string(),
        )
        .expect("materialized should record");
        canonical_artifacts::record_baseline_visible(
            &data_dir,
            &entry.artifact_id,
            "astra-overlay-baseline-tool".to_string(),
        )
        .expect("baseline visible should record");
        let executor = LocalToolExecutor::with_autonomous_research_context(
            &workspace_root,
            LocalToolExecutorContext {
                data_dir: data_dir.clone(),
                project_id: "project_tool_integration".to_string(),
                job_id: Some("job_tool_integration".to_string()),
                stage_id: Some("implement-solution".to_string()),
                stage_execution_id: Some("stage_1".to_string()),
                stage_artifact_type: Some("benchmark_code".to_string()),
            },
        );
        let call = ToolCall {
            tool_name: "record_canonical_artifact_integration_check".to_string(),
            target_path: None,
            content: None,
            parameters: Default::default(),
            arguments: serde_json::json!({
                "target_artifact_path": "baseline_runner.py",
                "status": "passed",
                "check_id": "py_compile",
                "command": "python -m py_compile baseline_runner.py",
                "detail": "worker imported and compiled the baseline runner from the canonical path",
                "evidence_refs": ["worker_evidence::py_compile_pass"]
            }),
            session_id: None,
            turn_id: None,
        };

        let result = executor
            .execute(&call)
            .expect("integration check should record");
        assert_eq!(result.status, "succeeded");
        let ledger = canonical_artifacts::load_ledger(&data_dir).expect("ledger should load");
        let updated = ledger
            .entries
            .iter()
            .find(|entry| entry.target_artifact_path == "baseline_runner.py")
            .expect("entry should exist");
        assert_eq!(
            updated.status,
            canonical_artifacts::CanonicalArtifactStatus::IntegrationVerified
        );
        assert_eq!(
            updated.integration_checks[0].evidence_refs,
            vec!["worker_evidence::py_compile_pass".to_string()]
        );
        assert!(data_dir
            .join("main-agent-board")
            .join("canonical-artifact-integration-checks")
            .exists());
        assert!(main_agent_safe_tool_names_for_mode("workspace-write")
            .contains(&"record_canonical_artifact_integration_check"));
    }
}
