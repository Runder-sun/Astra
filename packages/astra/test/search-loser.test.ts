import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { reviewFixture } from "./review-fixture.ts";

it.each([
	{ blocking: true, unrelated: false, shared: false },
	{ blocking: false, unrelated: false, shared: false },
	{ blocking: true, unrelated: true, shared: false },
	{ blocking: true, unrelated: true, shared: true },
])("archives discarded alternative without resolving failures (%j)", async ({ blocking, unrelated, shared }) => {
	const root = await mkdtemp(join(tmpdir(), "astra-flow-search-"));
	try {
		const store = new MemoryAstraStore();
		let job = await ResearchJob.create(store, { objective: "Choose one viable alternative", workspaceRoot: root });
		await job.reload();
		const definition = job.definitions.validation;
		const plan = await job.recordStagePlan({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "audit_search_plan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "audit_search_plan",
			mode: "search",
			tasks: ["a", "b"].map((key) => ({
				key,
				objective: `evaluate ${key}`,
				hypothesis: `hypothesis ${key}`,
				inputArtifactRefs: [],
				requiredOutputFields: definition.requiredOutputFields,
				acceptanceChecks: definition.acceptanceChecks,
				failureSignals: definition.failureSignals,
				successCriteria: definition.acceptanceChecks,
			})),
			rationale: "Evaluate competing alternatives",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		});
		const pe = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [] }));
		const batch = Object.values(job.state.searchBatches)[0]!;
		const candidates = Object.values(batch.candidates);
		for (const [index, candidate] of candidates.entries()) {
			const contract = buildEffectiveTaskContract(job, plan, plan.tasks[index]);
			const task = await job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:${candidate.key}`,
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: { candidate: candidate.id },
				refs: [],
			});
			const review = await job.recordReview(
				reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: index === 0 ? "pass" : "fail",
					findings: index === 0 ? [] : ["The alternative is infeasible"],
					score: index === 0 ? 1 : 0,
					blocking,
				}),
			);
			await job.recordCandidateEvaluation({
				batchId: batch.id,
				candidateId: candidate.id,
				evidenceId: evidence.id,
				reviewId: review.id,
				verdict: review.verdict,
				score: review.score!,
				criteria: review.criteria!,
				findings: review.findings,
			});
		}
		if (unrelated) {
			const old =
				job.state.tasks[
					job.state.evidence[job.state.searchBatches[batch.id].candidates[candidates[0].id].evidenceId!].taskId
				];
			const task = await job.dispatchTask({
				...old,
				id: "unrelated_task",
				replayKey: "unrelated",
				planId: undefined,
				effectiveContractHash: undefined,
				searchBatchId: undefined,
				searchCandidateId: undefined,
			});
			await job.setTaskStatus(task.id, "succeeded");
			const e = await job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: task.requiredOutputType,
				content: { unrelated: true },
				refs: [],
			});
			await job.recordReview(
				reviewFixture(job, { evidenceId: e.id, verdict: "fail", findings: ["Unrelated blocker"] }),
			);
			if (shared) {
				const snapshot = job.state;
				const issues = Object.values(snapshot.obligations);
				issues[1].graphObjectionId = issues[0].graphObjectionId;
				await store.writeSnapshot(snapshot);
				job = (await ResearchJob.open(store, job.state.frame.jobId))!;
			}
		}
		await job.selectSearchCandidate(batch.id, candidates[0]!.id, "audit_select_winner");
		const winnerEvidenceId = job.state.searchBatches[batch.id]!.candidates[candidates[0]!.id]!.evidenceId!;
		await job.adoptEvidence(winnerEvidenceId);
		const open = job.state.frame.openObligationIds.map((id) => job.state.obligations[id]!);
		console.log(
			JSON.stringify({
				winnerAdopted: Boolean(job.state.canonicalRoute.stageArtifactIds.validation),
				openObligations: open.map((item) => ({
					id: item.id,
					status: item.status,
					missingReview: !job.state.reviews[item.sourceReviewId],
					missingEvidence: !job.state.evidence[item.evidenceId!],
				})),
				unresolvedObjections: job.state.graph.unresolvedObjectionIds,
				completionBlockers: job.completionBlockers(),
			}),
		);
		expect(open).toHaveLength(unrelated ? 1 : 0);
		const receipt = job.state.discardedCandidates[candidates[1].id];
		expect(receipt.decisionRef).toBe("audit_select_winner");
		expect(receipt.archivedReviews?.[0].verdict).toBe("fail");
		expect(receipt.archivedObligations).toHaveLength(blocking ? 1 : 0);
		for (const issue of receipt.archivedObligations ?? []) {
			expect(issue.status).toBe("open");
			expect(issue.items?.every((item) => item.status === "open")).toBe(true);
			if (shared) expect(job.state.graph.unresolvedObjectionIds).toContain(issue.graphObjectionId);
		}
		expect(
			(await store.readEvents(job.state.frame.jobId)).some((event) => event.event.type === "obligation_resolved"),
		).toBe(false);
		expect((await ResearchJob.open(store, job.state.frame.jobId))!.state).toEqual(job.state);
	} finally {
		await rm(root, { recursive: true });
	}
});
