import type { Review, TaskPacket } from "./types.ts";

/** Definite assessment/ref rejection, after task and frozen-file integrity have been checked. */
export class ReviewAssessmentError extends Error {}

/** Host identity/file failure; a model cannot repair the frozen input. */
export class ReviewIntegrityError extends Error {}

export function frozenReviewCriteria(task: TaskPacket): string[] {
	return [
		...new Set([
			...task.acceptanceChecks,
			...task.successCriteria,
			...(task.repairChecks ?? []).map((check) => check.criterion),
		]),
	];
}

export function validateReviewReferences(
	review: Pick<Review, "criteria" | "verifiedRefs">,
	allowed: ReadonlySet<string>,
): void {
	if (
		[...(review.verifiedRefs ?? []), ...(review.criteria ?? []).flatMap((item) => item.evidenceRefs)].some(
			(ref) => !allowed.has(ref),
		)
	)
		throw new ReviewAssessmentError("review cites references outside its declared evidence or bound packet");
}

export function groupRepairCriteria(
	required: string[],
	issueIds: ReadonlySet<string>,
): Array<{ criterion: string; frozenCriteria: string[] }> {
	const groups = new Map<string, { criterion: string; frozenCriteria: string[] }>();
	for (const frozen of new Set(required)) {
		let criterion = frozen;
		while (true) {
			const prefix = /^\[([^\]]+)\] /.exec(criterion);
			if (!prefix || !issueIds.has(prefix[1]) || criterion.length === prefix[0].length) break;
			criterion = criterion.slice(prefix[0].length);
		}
		const group = groups.get(criterion) ?? { criterion, frozenCriteria: [] };
		group.frozenCriteria.push(frozen);
		groups.set(criterion, group);
	}
	return [...groups.values()];
}

/** Shared by backend preflight and the durable review boundary. Never invent assessments. */
export function validateReviewAssessment(
	review: Pick<Review, "verdict" | "score" | "criteria" | "verifiedRefs">,
	required: string[],
): void {
	if (!review.criteria?.length || !review.verifiedRefs?.length)
		throw new ReviewAssessmentError("review requires explicit criteria and verified evidence refs");
	if (
		!Array.isArray(review.criteria) ||
		!Array.isArray(review.verifiedRefs) ||
		review.criteria.some((item) => !item || typeof item !== "object" || typeof item.criterion !== "string")
	)
		throw new ReviewAssessmentError("review requires valid criterion assessments and verified evidence refs");
	const expected = new Set(required);
	if (
		expected.size !== review.criteria.length ||
		review.criteria.some((item) => !expected.has(item.criterion)) ||
		new Set(review.criteria.map((item) => item.criterion)).size !== expected.size
	) {
		const actual = review.criteria.map((item) => item.criterion);
		const missing = [...expected].filter((criterion) => !actual.includes(criterion));
		const duplicate = [...new Set(actual.filter((criterion, index) => actual.indexOf(criterion) !== index))];
		const unexpected = [...new Set(actual.filter((criterion) => !expected.has(criterion)))];
		throw new ReviewAssessmentError(
			`review must assess all frozen criteria exactly once: missing=${JSON.stringify(missing)}; duplicate=${JSON.stringify(duplicate)}; unexpected=${JSON.stringify(unexpected)}`,
		);
	}
	if (review.score === undefined || !Number.isFinite(review.score) || review.score < 0 || review.score > 1)
		throw new ReviewAssessmentError("review score must be between 0 and 1");
	for (const item of review.criteria) {
		if (!Number.isFinite(item.score) || item.score < 0 || item.score > 1)
			throw new ReviewAssessmentError("criterion score must be between 0 and 1");
		if (
			typeof item.passed !== "boolean" ||
			typeof item.rationale !== "string" ||
			!item.rationale.trim() ||
			!Array.isArray(item.evidenceRefs) ||
			!item.evidenceRefs.length ||
			item.evidenceRefs.some((ref) => typeof ref !== "string" || !ref.trim()) ||
			review.verifiedRefs.some((ref) => typeof ref !== "string" || !ref.trim())
		)
			throw new ReviewAssessmentError("each criterion requires an explicit judgment, rationale and evidence refs");
	}
	if (review.verdict === "pass" && review.criteria.some((item) => !item.passed))
		throw new ReviewAssessmentError("passing review contains a failed criterion");
	if (review.verdict !== "pass" && review.criteria.every((item) => item.passed))
		throw new ReviewAssessmentError(
			"non-passing review must identify a failed frozen criterion; a valid negative research report can pass artifact review",
		);
}
