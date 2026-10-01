import { describe, expect, it } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { groupRepairCriteria, validateReviewAssessment } from "../src/review-validation.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { MemoryAstraStore } from "../src/store.ts";

describe("shared review contract", () => {
	it("groups only identical requirements after removing registered leading repair IDs", () => {
		const required = [
			"check source",
			"[issue_a] check source",
			"[issue_b] [issue_a] check source",
			"[unknown] check source",
			"check sources",
			"verify [issue_a] source",
		];
		expect(groupRepairCriteria(required, new Set(["issue_a", "issue_b"]))).toEqual([
			{ criterion: "check source", frozenCriteria: required.slice(0, 3) },
			...required.slice(3).map((criterion) => ({ criterion, frozenCriteria: [criterion] })),
		]);
	});
	it("identifies omitted obligation criteria and duplicated assessments without accepting them", () => {
		const required = ["check source", "[obligation_1] check source"];
		const assessment = {
			criterion: "check source",
			passed: true,
			score: 1,
			evidenceRefs: ["source.json"],
			rationale: "Read source.json",
		};
		expect(() =>
			validateReviewAssessment(
				{ verdict: "pass", score: 1, verifiedRefs: ["source.json"], criteria: [assessment, assessment] },
				required,
			),
		).toThrow('missing=["[obligation_1] check source"]; duplicate=["check source"]');
	});

	it.each(DEFAULT_STAGES)("rejects implicit approval and malformed assessments in $id", async (stage) => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			objective: "Verify the shared review boundary",
			workspaceRoot: "/workspace",
			definitions: [stage],
		});
		const task = await job.dispatchTask({
			stageId: stage.id,
			stageExecutionId: stage.id,
			role: "worker",
			objective: "Produce evidence",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: stage.id,
			requiredOutputFields: stage.requiredOutputFields,
			acceptanceChecks: stage.acceptanceChecks,
			successCriteria: ["task-specific check"],
			failureSignals: stage.failureSignals,
			dependencies: [],
			scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: stage.workerBudget ?? { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: stage.id,
			type: stage.id,
			content: {},
			refs: ["result.json"],
		});
		const base = {
			evidenceId: evidence.id,
			verdict: "pass" as const,
			findings: [],
			score: 1,
			verifiedRefs: ["result.json"],
		};
		const criteria = [...stage.acceptanceChecks, "task-specific check"].map((criterion) => ({
			criterion,
			passed: true,
			score: 1,
			evidenceRefs: ["result.json"],
			rationale: "Inspected result.json",
		}));
		await expect(job.recordReview(base)).rejects.toThrow("explicit");
		await expect(job.recordReview({ ...base, criteria: criteria.slice(0, -1) })).rejects.toThrow("criteria");
		await expect(job.recordReview({ ...base, criteria: [...criteria, criteria[0]] })).rejects.toThrow("exactly once");
		await expect(job.recordReview({ ...base, criteria, verdict: "fail" })).rejects.toThrow("failed");
		await expect(job.recordReview({ ...base, criteria, score: Number.NaN })).rejects.toThrow("score");
		await expect(
			job.recordReview({ ...base, criteria: criteria.map((item) => ({ ...item, evidenceRefs: [] })) }),
		).rejects.toThrow("evidence");
		expect(Object.keys(job.state.reviews)).toHaveLength(0);
		await expect(job.recordReview({ ...base, criteria })).resolves.toMatchObject({ verdict: "pass" });
	});
});
