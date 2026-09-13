# Experiment Design Skill

Design experiments for the assigned research plan. Tie every experiment to a claim, baseline, metric, dataset or generated workload, and failure interpretation.

Required output:
- claim-to-experiment map
- baselines and controls
- metrics and success thresholds
- ablations
- run order and resource needs
- reproducibility artifacts to create
- risks and repair tasks

Synthesis mode:
- When the TaskPacket task type is a synthesis task, synthesize from mounted input artifacts and accepted worker evidence refs first.
- Cite the exact input refs used for each baseline, metric, ablation, run, budget, and reproducibility decision.
- Produce a standalone experiment plan candidate that can be reviewed without searching the whole workspace.
- If required evidence is missing, inconsistent, or too weak, return concrete repair tasks and missing-evidence risks instead of broad exploration.
- Do not decide stage completion, route changes, cleanup, candidate adoption, or reviewer verdicts.

Do not run experiments unless the TaskPacket explicitly asks for execution.
