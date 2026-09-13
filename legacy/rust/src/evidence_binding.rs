pub(crate) fn stage_evidence_binding_unit_kinds(stage_id: &str) -> Vec<&'static str> {
    match stage_id {
        "literature" => vec![
            "source row",
            "comparison statement",
            "open-problem or gap row",
            "source-status or quarantine decision",
        ],
        "novelty" => vec![
            "novelty claim",
            "closest-prior overlap",
            "novelty risk",
            "rejected novelty angle",
        ],
        "refine" => vec![
            "problem statement",
            "method component",
            "validation path",
            "risk or rollback trigger",
        ],
        "experiment-plan" => vec![
            "experiment question",
            "baseline",
            "metric",
            "ablation",
            "run-matrix entry",
        ],
        "implement-solution" => vec![
            "implemented component",
            "code entry point",
            "config file",
            "test or smoke command",
        ],
        "run" => vec![
            "run command",
            "config snapshot",
            "environment snapshot",
            "log or raw-output ref",
            "typed failure",
        ],
        "monitor" => vec![
            "metric row",
            "anomaly",
            "missing run",
            "baseline comparison",
            "rerun or repair recommendation",
        ],
        "result-to-claim" => vec![
            "claim",
            "support-level decision",
            "limitation",
            "claim deletion or narrowing decision",
        ],
        "paper-plan" => vec![
            "paper section",
            "claim-to-section mapping",
            "figure or table plan",
            "citation plan item",
        ],
        "paper-write" => vec![
            "paper claim",
            "citation use",
            "figure or table reference",
            "limitation statement",
        ],
        "paper-compile" => vec![
            "PDF artifact",
            "build command",
            "build log",
            "reference or warning audit",
        ],
        "research-review" => vec![
            "hard-review finding",
            "reproducibility audit item",
            "claim-evidence audit item",
            "repair route",
        ],
        "rebuttal" => vec![
            "review finding",
            "selected operation",
            "repair task",
            "rejected reviewer demand",
        ],
        "meta-optimize" => vec![
            "failure pattern",
            "root cause",
            "strategy adjustment",
            "next-loop constraint",
        ],
        _ => vec![
            "stage claim",
            "evidence-backed decision",
            "limitation",
            "next action",
        ],
    }
}
