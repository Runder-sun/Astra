import { planReviewStatusFromSnapshot } from "./plan-review.ts";
import { stageMap } from "./stages.ts";
import type { JobSnapshot } from "./types.ts";

/** Evidence-backed milestones; execution success alone never marks a delivery accepted. */
export function researchMilestones(state: JobSnapshot) {
	return Object.entries(state.stages).map(([stageId, stage]) => {
		const artifactId = state.canonicalRoute.stageArtifactIds[stageId];
		const tasks = Object.values(state.tasks).filter(
			(task) =>
				task.stageId === stageId && task.role === "worker" && (task.stageRevision ?? 1) === (stage.revision ?? 1),
		);
		const deliveries = tasks.map((task) => {
			const evidence = Object.values(state.evidence)
				.filter((item) => item.taskId === task.id)
				.at(-1);
			const reviews = Object.values(state.reviews).filter((review) => review.evidenceId === evidence?.id);
			return {
				taskId: task.id,
				objective: task.objective,
				kind: task.deliveryKind ?? "stage",
				status: evidence?.status ?? task.status,
				evidenceId: evidence?.id,
				version: evidence?.versionHash,
				codeVersion: task.version?.git.status === "captured" ? task.version.git.head : undefined,
				reviews: reviews.map((review) => ({ id: review.id, verdict: review.verdict, createdAt: review.createdAt })),
			};
		});
		const plans = Object.values(state.stagePlans)
			.filter((plan) => plan.stageId === stageId)
			.map((plan) => {
				const evidence = Object.values(state.evidence).find(
					(item) => item.type === "stage-plan" && state.tasks[item.taskId]?.planId === plan.id,
				);
				const reviews = Object.values(state.reviews).filter((review) => review.evidenceId === evidence?.id);
				const status = planReviewStatusFromSnapshot(state, state.stageDefinitions ?? stageMap(), plan.id);
				return { id: plan.id, createdAt: plan.createdAt, status, reviews };
			});
		const repairs = Object.values(state.obligations).filter(
			(issue) =>
				(issue.stageId ?? state.evidence[state.reviews[issue.sourceReviewId]?.evidenceId]?.stageId) === stageId,
		);
		return {
			stageId,
			revision: stage.revision ?? 1,
			status: stage.invalidatedBy
				? "stale"
				: artifactId &&
						state.canonical[artifactId]?.status === "active" &&
						state.canonical[artifactId]?.adoptionCompletedAt
					? "adopted"
					: stage.status,
			invalidatedBy: stage.invalidatedBy,
			artifactId,
			plans,
			deliveries,
			repairs,
		};
	});
}
