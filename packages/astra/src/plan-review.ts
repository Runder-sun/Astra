import type { ResearchJob } from "./research.ts";
import type { Evidence, StagePlanManifest } from "./types.ts";

export const PLAN_REVIEW_CHECKS = [
	"The plan advances the mission within its boundaries and addresses the current stage purpose",
	"Task scopes are actionable, feasible within budgets, and do not duplicate or depend on concurrent outputs",
	"Declared inputs provide the evidence needed for the proposed work and are current",
	"Output fields and acceptance criteria can falsify an inadequate result rather than merely confirm execution",
	"Local work has a coherent path to complete synthesis; complete deliveries cover the stage contract",
	"The plan addresses outstanding repair and previous plan-review findings without weakening requirements",
];

export function planEvidence(job: Pick<ResearchJob, "state">, planId: string): Evidence | undefined {
	return Object.values(job.state.evidence).find(
		(evidence) => evidence.type === "stage-plan" && job.state.tasks[evidence.taskId]?.planId === planId,
	);
}

export function planReviewStatus(
	job: Pick<ResearchJob, "state">,
	planId: string,
): "pending" | "passed" | "failed" | "stale" {
	const evidence = planEvidence(job, planId);
	if (!evidence) return "pending";
	const task = job.state.tasks[evidence.taskId];
	const context = evidence.content as { plan: StagePlanManifest; guidanceRefs: string[] };
	const guidanceRefs = Object.values(job.state.graph.nodes)
		.filter((node) => node.actor === "user")
		.map((node) => node.id)
		.sort();
	if (
		task.stageRevision !== (job.state.stages[evidence.stageId]?.revision ?? 1) ||
		JSON.stringify(context.plan) !== JSON.stringify(job.state.stagePlans[planId]) ||
		JSON.stringify(context.guidanceRefs) !== JSON.stringify(guidanceRefs)
	)
		return "stale";
	if (
		task.inputArtifactRefs.some(
			(ref) => job.state.retiredArtifacts[ref] || job.state.canonical[ref]?.status === "stale",
		)
	)
		return "stale";
	const reviews = Object.values(job.state.reviews).filter(
		(review) => review.evidenceId === evidence.id && review.targetVersionHash === evidence.versionHash,
	);
	if (reviews.some((review) => review.verdict !== "pass" || (review.score ?? 0) < 0.8)) return "failed";
	return reviews.length ? "passed" : "pending";
}

export async function preparePlanEvidence(job: ResearchJob, plan: StagePlanManifest): Promise<Evidence> {
	const existing = planEvidence(job, plan.id);
	if (existing) return existing;
	const inputs = [...new Set(plan.tasks.flatMap((task) => task.inputArtifactRefs))];
	const obligation = plan.obligationId ? job.state.obligations[plan.obligationId] : undefined;
	const task = await job.dispatchTask({
		repairOfEvidenceId: obligation?.evidenceId,
		planId: plan.id,
		replayKey: `plan-review:${plan.id}`,
		stageId: plan.stageId,
		stageExecutionId: job.state.stages[plan.stageId].executionId ?? plan.stageId,
		role: "main-agent",
		objective:
			"Independently audit this execution plan before any worker is dispatched. Assess planning adequacy, not whether the planned experiments have already succeeded.",
		inputArtifactRefs: inputs,
		requiredCanonicalArtifacts: inputs.filter((id) => Boolean(job.state.canonical[id])),
		requiredOutputType: "stage-plan",
		requiredOutputFields: ["plan", "mission", "capability"],
		acceptanceChecks: PLAN_REVIEW_CHECKS,
		successCriteria: [],
		failureSignals: ["uncovered requirements", "unverifiable criteria", "ignored repairs"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 8, maxToolCalls: 32, maxRuntimeMs: 180000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	await job.setTaskStatus(task.id, "succeeded");
	return job.recordEvidence({
		taskId: task.id,
		stageId: plan.stageId,
		type: "stage-plan",
		refs: [],
		content: {
			guidanceRefs: Object.values(job.state.graph.nodes)
				.filter((node) => node.actor === "user")
				.map((node) => node.id)
				.sort(),
			plan,
			mission: job.state.frame,
			capability: job.definitions[plan.stageId],
			obligation: plan.obligationId ? job.state.obligations[plan.obligationId] : undefined,
			previousFindings: Object.values(job.state.reviews)
				.filter(
					(review) =>
						job.state.evidence[review.evidenceId]?.type === "stage-plan" &&
						job.state.evidence[review.evidenceId]?.stageId === plan.stageId &&
						review.verdict !== "pass",
				)
				.map((review) => ({ reviewId: review.id, findings: review.findings, criteria: review.criteria })),
		},
	});
}
