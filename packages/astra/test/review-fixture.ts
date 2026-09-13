import type { ResearchJob } from "../src/research.ts";
import type { Review } from "../src/types.ts";

/** Explicit synthetic assessments for state-machine fixtures, never used by production adapters. */
export function reviewFixture(job: ResearchJob, input: Omit<Review, "id" | "createdAt">) {
	const evidence = job.state.evidence[input.evidenceId];
	const task = job.state.tasks[evidence.taskId];
	const passed = input.verdict === "pass";
	const refs = [`evidence:${evidence.id}`];
	return {
		...input,
		score: input.score ?? (passed ? 1 : 0),
		verifiedRefs: input.verifiedRefs ?? refs,
		criteria:
			input.criteria ??
			[...new Set([...task.acceptanceChecks, ...task.successCriteria])].map((criterion) => ({
				criterion,
				passed,
				score: input.score ?? (passed ? 1 : 0),
				evidenceRefs: refs,
				rationale: input.findings.join("; ") || "Synthetic state-machine fixture assessment",
			})),
	};
}
