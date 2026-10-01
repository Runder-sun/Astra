import type { Review } from "./types.ts";

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
		throw new Error("review requires explicit criteria and verified evidence refs");
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
		throw new Error(
			`review must assess all frozen criteria exactly once: missing=${JSON.stringify(missing)}; duplicate=${JSON.stringify(duplicate)}; unexpected=${JSON.stringify(unexpected)}`,
		);
	}
	if (review.score === undefined || !Number.isFinite(review.score) || review.score < 0 || review.score > 1)
		throw new Error("review score must be between 0 and 1");
	for (const item of review.criteria) {
		if (!Number.isFinite(item.score) || item.score < 0 || item.score > 1)
			throw new Error("criterion score must be between 0 and 1");
		if (
			typeof item.passed !== "boolean" ||
			!item.rationale.trim() ||
			!item.evidenceRefs.length ||
			item.evidenceRefs.some((ref) => !ref.trim()) ||
			review.verifiedRefs.some((ref) => !ref.trim())
		)
			throw new Error("each criterion requires an explicit judgment, rationale and evidence refs");
	}
	if (review.verdict === "pass" && review.criteria.some((item) => !item.passed))
		throw new Error("passing review contains a failed criterion");
	if (review.verdict !== "pass" && review.criteria.every((item) => item.passed))
		throw new Error(
			"non-passing review must identify a failed frozen criterion; a valid negative research report can pass artifact review",
		);
}
