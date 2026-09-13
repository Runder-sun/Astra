use serde_json::Value;

pub(crate) fn json_entry_is_retired(entry: &Value) -> bool {
    entry
        .get("active_status")
        .and_then(|value| value.as_str())
        .map(|status| {
            matches!(
                status.to_ascii_lowercase().as_str(),
                "superseded"
                    | "rejected"
                    | "deferred"
                    | "obsolete"
                    | "inactive"
                    | "retired"
                    | "deprecated"
            )
        })
        .unwrap_or(false)
        || entry
            .get("superseded_by_task_id")
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

pub(crate) fn json_entry_is_main_agent_accepted(entry: &Value) -> bool {
    !json_entry_is_retired(entry)
        && entry
            .get("active_status")
            .and_then(|value| value.as_str())
            .map(|status| status.eq_ignore_ascii_case("active"))
            .unwrap_or(false)
        && entry
            .get("main_agent_decision_ref")
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

pub(crate) fn task_type_is_stage_synthesis(task_type: &str) -> bool {
    let normalized = task_type
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>();
    normalized.contains("synthesis") || normalized.contains("artifact repair")
}

pub(crate) fn stage_synthesis_matches_artifact(
    worker_role: &str,
    task_type: &str,
    required_output_artifact_type: &str,
    artifact_type: &str,
) -> bool {
    worker_role == "research_synthesizer"
        && task_type_is_stage_synthesis(task_type)
        && required_output_artifact_type == artifact_type
}

pub(crate) fn json_entry_is_stage_synthesis_for_artifact(
    entry: &Value,
    artifact_type: &str,
) -> bool {
    stage_synthesis_matches_artifact(
        entry
            .get("worker_role")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
        entry
            .get("task_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
        entry
            .get("required_output_artifact_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
        artifact_type,
    )
}

pub(crate) fn json_entry_is_current_for_task_revision(
    entry: &Value,
    task_revision: Option<u128>,
) -> bool {
    let Some(task_revision) = task_revision else {
        return true;
    };
    entry
        .get("created_at")
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse::<u128>().ok())
        .map(|created_at| created_at >= task_revision)
        .unwrap_or(true)
}
