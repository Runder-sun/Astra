#[derive(Debug, Clone, Copy)]
pub(crate) struct BoardTaskRepairContext<'a> {
    pub task_id: &'a str,
    pub task_type: &'a str,
    pub objective: &'a str,
    pub review_findings_refs: &'a [String],
    pub blocker_refs: &'a [String],
    pub replacement_of_task_ids: &'a [String],
}

pub(crate) fn repair_task_contract_violation(
    context: &BoardTaskRepairContext<'_>,
    acceptance_checks: &[String],
) -> Option<String> {
    if !task_is_repair_like(context) {
        return None;
    }
    if context.review_findings_refs.is_empty() {
        return Some(format!(
            "repair task `{}` must cite the failed review or finding in `review_findings_refs`",
            context.task_id
        ));
    }
    if context.blocker_refs.is_empty() && context.replacement_of_task_ids.is_empty() {
        return Some(format!(
            "repair task `{}` must bind the concrete blocker in `blocker_refs` or the task it replaces in `replacement_of_task_ids`",
            context.task_id
        ));
    }
    if !repair_acceptance_checks_are_specific(acceptance_checks) {
        return Some(format!(
            "repair task `{}` must include acceptance_checks that name the concrete fix, such as rewriting a contradicted claim, adding citation mapping, separating source classes, or adding row-level evidence",
            context.task_id
        ));
    }
    None
}

pub(crate) fn self_reference_contract_violation(
    task_id: &str,
    supersedes_task_ids: &[String],
    replacement_of_task_ids: &[String],
) -> Option<String> {
    for (field, refs) in [
        ("supersedes_task_ids", supersedes_task_ids),
        ("replacement_of_task_ids", replacement_of_task_ids),
    ] {
        if refs
            .iter()
            .any(|reference| task_ref_matches_task_id(reference, task_id))
        {
            return Some(format!(
                "task `{task_id}` must not cite itself in `{field}`; cite a distinct source task or leave `{field}` empty"
            ));
        }
    }
    None
}

fn task_ref_matches_task_id(reference: &str, task_id: &str) -> bool {
    let reference = reference.trim().trim_matches('`').trim();
    let task_id = task_id.trim();
    if reference.is_empty() || task_id.is_empty() {
        return false;
    }
    reference == task_id
        || reference
            .strip_prefix("main_agent_board_task::")
            .is_some_and(|inner| inner.trim() == task_id)
}

pub(crate) fn task_is_repair_like(context: &BoardTaskRepairContext<'_>) -> bool {
    is_artifact_repair_task(context.task_type)
        || text_declares_repair_or_replacement(context.task_type)
        || text_declares_repair_or_replacement(context.task_id)
        || objective_declares_failed_review_repair(context.objective)
        || context
            .blocker_refs
            .iter()
            .any(|reference| blocker_ref_declares_repair_context(reference))
}

pub(crate) fn text_declares_repair_or_replacement(value: &str) -> bool {
    let key = normalize_task_key(value);
    let tokens = key.split_whitespace().collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(index, token)| {
        repair_identity_action_token(token) && !repair_identity_token_is_metadata(&tokens, index)
    })
}

fn objective_declares_failed_review_repair(value: &str) -> bool {
    if !text_declares_repair_or_replacement(value) {
        return false;
    }
    let key = normalize_task_key(value);
    if objective_prohibits_repair_evidence_use(&key) {
        return false;
    }
    key.contains("failed review")
        || key.contains("failed stage review")
        || key.contains("review finding")
        || key.contains("review findings")
        || key.contains("rev ")
}

fn objective_prohibits_repair_evidence_use(key: &str) -> bool {
    [
        "must not be used as post review repair evidence",
        "must not be used as repair evidence",
        "do not use as post review repair evidence",
        "do not use as repair evidence",
        "cannot be used as post review repair evidence",
        "cannot be used as repair evidence",
        "not be used as post review repair evidence",
        "not be used as repair evidence",
    ]
    .iter()
    .any(|phrase| key.contains(phrase))
}

pub(crate) fn task_type_is_lifecycle_only(value: &str) -> bool {
    matches!(
        normalize_task_key(value).as_str(),
        "repair"
            | "repairs"
            | "repairing"
            | "repair task"
            | "fix"
            | "fixes"
            | "fixing"
            | "replacement"
            | "replace"
            | "replacing"
            | "blocker"
            | "blocked"
    )
}

pub(crate) fn normalize_task_key(value: &str) -> String {
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

fn is_artifact_repair_task(task_type: &str) -> bool {
    normalize_task_key(task_type).contains("artifact repair")
}

fn blocker_ref_declares_repair_context(reference: &str) -> bool {
    let reference = reference.trim().trim_matches('`').trim();
    reference.starts_with("failed_stage_review:")
        || reference.starts_with("worker_review_failure_route::")
        || reference.starts_with("stage_closure_blocker:")
        || review_ref_declares_repair_context(reference)
}

fn review_ref_declares_repair_context(reference: &str) -> bool {
    let reference = reference.trim().trim_matches('`').trim();
    reference.starts_with("review_packet:")
        || reference.starts_with("review_trace:")
        || reference.starts_with("failed_stage_review:")
}

fn repair_identity_action_token(token: &str) -> bool {
    matches!(
        token,
        "repair"
            | "repairs"
            | "repaired"
            | "repairing"
            | "replacement"
            | "replace"
            | "replaces"
            | "replacing"
            | "fix"
            | "fixes"
            | "fixed"
            | "fixing"
            | "cure"
            | "cures"
            | "cured"
            | "curing"
            | "rewrite"
            | "rewrites"
            | "rewritten"
            | "rewriting"
            | "rebuild"
            | "rebuilds"
            | "rebuilt"
            | "rebuilding"
            | "blocker"
            | "blockers"
            | "blocked"
    )
}

fn repair_identity_token_is_metadata(tokens: &[&str], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| repair_identity_meta_token(next))
        || index
            .checked_sub(1)
            .and_then(|previous| tokens.get(previous))
            .is_some_and(|previous| repair_identity_meta_token(previous))
}

fn repair_identity_meta_token(token: &str) -> bool {
    matches!(
        token,
        "routing"
            | "route"
            | "routes"
            | "routed"
            | "policy"
            | "policies"
            | "protocol"
            | "protocols"
            | "contract"
            | "contracts"
            | "rule"
            | "rules"
            | "criteria"
            | "check"
            | "checks"
            | "dispatch"
            | "prevention"
            | "prevent"
            | "prevents"
            | "prohibition"
            | "prohibit"
            | "prohibits"
            | "forbid"
            | "forbids"
            | "report"
            | "reports"
            | "memo"
            | "memos"
            | "note"
            | "notes"
            | "task"
            | "tasks"
            | "context"
            | "handling"
            | "guidance"
            | "lineage"
            | "loop"
            | "loops"
            | "lifecycle"
            | "lifecycles"
            | "pipeline"
            | "pipelines"
            | "flow"
            | "flows"
            | "semantics"
            | "integrity"
            | "continuity"
            | "drift"
            | "drifts"
            | "mode"
            | "modes"
    )
}

fn repair_acceptance_checks_are_specific(acceptance_checks: &[String]) -> bool {
    let combined = acceptance_checks.join(" | ").to_ascii_lowercase();
    [
        "review finding",
        "failed review",
        "contradict",
        "claim",
        "rewrite",
        "remove",
        "delete",
        "downgrade",
        "citation",
        "provenance",
        "auditable",
        "source class",
        "source classification",
        "verified",
        "provisional",
        "quarantined",
        "row-level",
        "row level",
        "evidence boundary",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_identity_ignores_metadata_phrases() {
        assert!(!text_declares_repair_or_replacement(
            "failed review repair routing policy"
        ));
        assert!(!text_declares_repair_or_replacement(
            "generic repair drift prevention"
        ));
        assert!(!text_declares_repair_or_replacement(
            "repair loop continuity check"
        ));
    }

    #[test]
    fn repair_identity_keeps_explicit_repair_task_names() {
        assert!(text_declares_repair_or_replacement(
            "paper clustering repair v4"
        ));
        assert!(text_declares_repair_or_replacement(
            "acceptance standard setting repair"
        ));
        assert!(text_declares_repair_or_replacement("blocker"));
    }

    #[test]
    fn ordinary_cross_reference_fields_do_not_make_task_repair_like() {
        let context = BoardTaskRepairContext {
            task_id: "board::stage_experiment_1::contract_roundtrip_audit",
            task_type: "contract roundtrip audit",
            objective: "Audit contract refs across roundtrip evidence.",
            review_findings_refs: &[String::from("review::roundtrip::finding")],
            blocker_refs: &[String::from("blocker::roundtrip::upstream")],
            replacement_of_task_ids: &[String::from(
                "board::stage_experiment_1::stale_contract_task",
            )],
        };

        assert!(!task_is_repair_like(&context));
        assert!(repair_task_contract_violation(
            &context,
            &[String::from("preserves contract identity fields")]
        )
        .is_none());
    }

    #[test]
    fn failed_review_anchors_make_task_repair_like() {
        let context = BoardTaskRepairContext {
            task_id: "research_stage_task::stage_literature_1::stage_artifact_synthesis_v2",
            task_type: "stage artifact synthesis",
            objective: "Synthesize a candidate after a failed stage review.",
            review_findings_refs: &[String::from("finding:evidence-status-separation")],
            blocker_refs: &[String::from("review_packet:rev_failed_stage")],
            replacement_of_task_ids: &[],
        };

        assert!(task_is_repair_like(&context));
    }

    #[test]
    fn ordinary_review_refs_do_not_make_evidence_tasks_repair_like() {
        let context = BoardTaskRepairContext {
            task_id: "research_stage_task::stage_1::contradiction_family_prior_audit",
            task_type: "closest-prior search",
            objective:
                "Mount contradiction/conflict-resolution benchmark priors required by rev_gap.",
            review_findings_refs: &[String::from("rev_gap")],
            blocker_refs: &[],
            replacement_of_task_ids: &[],
        };

        assert!(!task_is_repair_like(&context));
        assert!(repair_task_contract_violation(
            &context,
            &[String::from("covers contradiction priors")]
        )
        .is_none());
    }

    #[test]
    fn prohibited_repair_evidence_use_does_not_make_objective_repair_like() {
        let context = BoardTaskRepairContext {
            task_id: "research_stage_task::stage_1::stage_artifact_synthesis",
            task_type: "stage artifact synthesis",
            objective:
                "Produce pre-review synthesis; must not be used as post-review repair evidence after a failed stage review.",
            review_findings_refs: &[],
            blocker_refs: &[],
            replacement_of_task_ids: &[],
        };

        assert!(!task_is_repair_like(&context));
    }
}
