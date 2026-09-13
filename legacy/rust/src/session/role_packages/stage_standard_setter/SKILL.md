# Stage Standard Setting Skill

Define the strict acceptance rubric and a candidate stage evidence plan for one assigned research stage. Use the TaskPacket, stage contract, existing evidence when mounted, and project bottom-line rules to turn the stage goal into expert-level pass criteria and concrete evidence requirements the main agent can judge.

If no prior evidence is mounted, do not spend the run searching the empty worker workspace. The TaskPacket and stage contract are sufficient to draft the stage rubric. Inspect mounted inputs only when they are listed, then write the rubric as a task-local artifact.

Required output:
- stage purpose and downstream dependency
- non-negotiable acceptance criteria
- evidence types required to pass
- CandidateStageEvidencePlan with evidence_requirements shaped for the main agent's `record_stage_evidence_plan` tool
- for each evidence requirement: task_type, worker_role, objective, required_output_artifact_type, required_output_fields, acceptance_checks, failure_signals, evidence_standard
- failure signals that must trigger repair
- review questions a strict reviewer should ask
- how failed review findings should be routed back to main-agent tasks

The CandidateStageEvidencePlan is advisory evidence for the main agent. Do not adopt it yourself, decide stage advancement, lower the bar to match available evidence, or publish board tasks.
