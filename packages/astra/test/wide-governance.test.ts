import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import type { StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function setup() {
	const root = await mkdtemp(join(tmpdir(), "astra-wide-governance-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Verify current lineage",
		automation: "full",
	});
	await job.reload();
	return { root, store, job };
}
async function delivery(
	job: ResearchJob,
	stageId: string,
	key: string,
	refs: string[] = [],
	kind: "stage" | "local" | "synthesis" = "stage",
	repairOfEvidenceId?: string,
	verdict: "pass" | "fail" = "pass",
) {
	const repairChecks = repairOfEvidenceId
		? Object.values(job.state.obligations)
				.filter((issue) => job.state.reviews[issue.sourceReviewId]?.evidenceId === repairOfEvidenceId)
				.flatMap((issue) =>
					(issue.items ?? []).map((item) => ({
						issueId: item.id,
						criterion: job.repairCriterion(`[${item.id}] ${item.criterion}`),
					})),
				)
		: [];
	const task = await job.dispatchTask({
		stageId,
		stageExecutionId: stageId,
		role: "worker",
		deliveryKind: kind,
		repairOfEvidenceId,
		repairChecks,
		objective: key,
		replayKey: key,
		inputArtifactRefs: refs,
		requiredCanonicalArtifacts: [],
		requiredOutputType: kind === "local" ? `${stageId}:local` : stageId,
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified", ...repairChecks.map((check) => check.criterion)],
		successCriteria: [],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId,
		type: task.requiredOutputType,
		content: { content: key },
		refs: [],
		currentEvidenceSetId: repairOfEvidenceId
			? job.state.evidence[repairOfEvidenceId].currentEvidenceSetId
			: undefined,
	});
	await job.recordReview(
		reviewFixture(job, {
			evidenceId: evidence.id,
			verdict,
			findings: verdict === "fail" ? ["repair this local result"] : [],
		}),
	);
	await job.decideEvidence(evidence.id, verdict === "pass");
	return evidence;
}
it.each([false, true])("invalidates downstream artifacts through local evidence bridge=%s", async (bridge) => {
	const { job } = await setup();
	const source = await delivery(job, "validation", "source");
	const a = await job.adoptEvidence(source.id);
	const local = bridge ? await delivery(job, "literature", "local", [a.id], "local") : undefined;
	const consumer = await delivery(job, "literature", "consumer", [local?.id ?? a.id], bridge ? "synthesis" : "stage");
	const b = await job.adoptEvidence(consumer.id);
	const replacement = await delivery(job, "validation", "replacement");
	await job.adoptEvidence(replacement.id, a.id);
	console.log(
		JSON.stringify({
			bridge,
			sourceRetired: !!job.state.retiredArtifacts[a.id],
			downstreamStatus: job.state.canonical[b.id].status,
			route: job.state.canonicalRoute.stageArtifactIds.literature,
		}),
	);
	expect(job.state.canonical[b.id].status).toBe("stale");
	expect(job.state.canonicalRoute.stageArtifactIds.literature).toBeUndefined();
});

it("repeating adoption never destroys the accepted evidence and event replay", async () => {
	const { job, store } = await setup();
	const evidence = await delivery(job, "validation", "once");
	await job.adoptEvidence(evidence.id);
	let error: unknown;
	try {
		await job.adoptEvidence(evidence.id);
	} catch (e) {
		error = e;
	}
	console.log(
		JSON.stringify({
			repeatError: String(error),
			evidencePresent: !!job.state.evidence[evidence.id],
			events: (await store.readEvents(job.state.frame.jobId)).at(-1)?.event.type,
		}),
	);
	let reopenError: unknown;
	try {
		await ResearchJob.open(store, job.state.frame.jobId);
	} catch (e) {
		reopenError = e;
	}
	console.log(JSON.stringify({ reopenError: String(reopenError) }));
	expect(job.state.evidence[evidence.id]).toBeDefined();
	await expect(ResearchJob.open(store, job.state.frame.jobId)).resolves.toBeDefined();
});

it.each([false, true])("recovers search review-to-evaluation interruption=%s", async (interrupted) => {
	const { job, store } = await setup();
	const definition = job.definitions.validation;
	const plan: StagePlanManifest = {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: "search",
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: "search",
		mode: "search",
		tasks: ["a", "b"].map((key) => ({
			key,
			objective: key,
			hypothesis: key,
			inputArtifactRefs: [],
			requiredOutputFields: definition.requiredOutputFields,
			acceptanceChecks: [],
			failureSignals: [],
			successCriteria: [],
		})),
		rationale: "search",
		sessionRef: "fixture",
		createdAt: new Date().toISOString(),
	};
	await job.recordStagePlan(plan);
	const pe = await preparePlanEvidence(job, plan);
	await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [] }));
	const batch = Object.values(job.state.searchBatches)[0];
	for (const [index, planned] of plan.tasks.entries()) {
		const contract = buildEffectiveTaskContract(job, plan, planned);
		const task = await job.dispatchTask({
			...contract,
			effectiveContractHash: semanticContractHash(contract),
			replayKey: `stage-plan:${plan.id}:${planned.key}`,
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: Object.fromEntries(task.requiredOutputFields.map((f) => [f, "verified"])),
			refs: [],
		});
		const review = await job.recordReview(
			reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [], blocking: false }),
		);
		if (!interrupted || index !== 0)
			await job.recordCandidateEvaluation({
				batchId: batch.id,
				candidateId: task.searchCandidateId!,
				evidenceId: evidence.id,
				reviewId: review.id,
				verdict: review.verdict,
				score: review.score!,
				criteria: review.criteria!,
				findings: review.findings,
			});
	}
	const mainAgent = {
		planStage: vi.fn(),
		decideEvidence: vi.fn(),
		decideAdoption: vi.fn(async () => ({ adopt: true })),
		decideSearch: vi.fn(async () => ({
			selectedCandidateId: Object.values(batch.candidates)[0].id,
			decisionRef: "select",
		})),
		decideRoute: vi.fn(async () => {
			throw new NonRetryableResearchError("end after adoption");
		}),
	};
	const reviewer = { review: vi.fn() };
	const worker = { run: vi.fn() };
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	const supervisor = new ResearchSupervisor(reopened, store, { mainAgent: mainAgent as never, reviewer, worker });
	await supervisor.tick();
	await supervisor.tick();
	console.log(
		JSON.stringify({
			interrupted,
			evaluations: Object.keys(reopened.state.candidateEvaluations).length,
			searchCalls: mainAgent.decideSearch.mock.calls.length,
			reviewCalls: reviewer.review.mock.calls.length,
			planCalls: mainAgent.planStage.mock.calls.length,
			batch: reopened.state.searchBatches[batch.id].status,
		}),
	);
	expect(mainAgent.decideSearch).toHaveBeenCalledOnce();
});

it.each(["replace", "reopen"])("does not recommend invalidated local evidence before synthesis (%s)", async (mode) => {
	const { job, store } = await setup();
	const source = await delivery(job, "validation", "source");
	const a = await job.adoptEvidence(source.id);
	const local = await delivery(job, "literature", "local", [a.id], "local");
	const before = job.state;
	expect(job.unsynthesizedLocalEvidence("literature").map((e) => e.id)).toContain(local.id);
	if (mode === "replace") {
		const next = await delivery(job, "validation", "next");
		await job.adoptEvidence(next.id, a.id);
	} else await job.reopenStage("validation", "reopen", "test upstream");
	expect(job.unsynthesizedLocalEvidence("literature")).toEqual([]);
	expect(job.state.evidence[local.id]).toEqual(before.evidence[local.id]);
	expect(job.state.reviews).toEqual(
		expect.objectContaining(
			Object.fromEntries(Object.entries(before.reviews).filter(([, review]) => review.evidenceId === local.id)),
		),
	);
	expect((await ResearchJob.open(store, job.state.frame.jobId))!.unsynthesizedLocalEvidence("literature")).toEqual([]);
	await expect(delivery(job, "literature", "invalid synthesis", [local.id], "synthesis")).rejects.toThrow(
		/stale|accepted/,
	);
});

it.each(["artifact", "evidence"])(
	"propagates through multiple local layers without invalidating unrelated work (%s)",
	async (refKind) => {
		const { job, store } = await setup();
		const source = await delivery(job, "validation", "source");
		const a = await job.adoptEvidence(source.id);
		const local = await delivery(
			job,
			"literature",
			"first local",
			[refKind === "artifact" ? a.id : source.id],
			"local",
		);
		const second = await delivery(job, "literature", "second local", [local.id], "local");
		const consumer = await delivery(job, "literature", "combined", [local.id, second.id], "synthesis");
		const b = await job.adoptEvidence(consumer.id);
		const unrelated = await delivery(job, "idea", "unrelated");
		const u = await job.adoptEvidence(unrelated.id);
		const initial = job.state;
		const replacement = await delivery(job, "validation", "replacement");
		await job.adoptEvidence(replacement.id, a.id);
		expect(job.state.canonical[b.id].status).toBe("stale");
		expect(job.state.canonical[u.id].status).toBe("active");
		expect(job.state.evidence[local.id]).toEqual(initial.evidence[local.id]);
		expect((await ResearchJob.open(store, job.state.frame.jobId))!.state).toEqual(job.state);
	},
);

it("returns the same adopted identity without events and rejects a conflicting replacement", async () => {
	const { job, store } = await setup();
	const e = await delivery(job, "validation", "once");
	const a = await job.adoptEvidence(e.id);
	const before = job.state;
	expect(await job.adoptEvidence(e.id)).toEqual(a);
	expect(job.state).toEqual(before);
	await expect(job.adoptEvidence(e.id, "another-artifact")).rejects.toThrow(/repeated/);
	expect(job.state).toEqual(before);
	expect((await ResearchJob.open(store, job.state.frame.jobId))!.state).toEqual(before);
});

it.each(["replace", "reopen"])(
	"preserves an independent current route after old local dependencies change (%s)",
	async (mode) => {
		const { job, store } = await setup();
		const a = await job.adoptEvidence((await delivery(job, "validation", "source")).id);
		const local = await delivery(job, "literature", "local", [a.id], "local");
		const b = await job.adoptEvidence((await delivery(job, "literature", "synthesis", [local.id], "synthesis")).id);
		const c = await job.adoptEvidence((await delivery(job, "literature", "independent")).id, b.id);
		const before = job.state;
		if (mode === "replace") await job.adoptEvidence((await delivery(job, "validation", "replacement")).id, a.id);
		else await job.reopenStage("validation", "reopen", "revisit upstream");
		expect(job.state.canonical[c.id]).toEqual(before.canonical[c.id]);
		expect(job.state.canonicalRoute.stageArtifactIds.literature).toBe(c.id);
		expect(job.state.stages.literature).toEqual(before.stages.literature);
		expect(job.state.evidence[local.id]).toEqual(before.evidence[local.id]);
		expect(job.unsynthesizedLocalEvidence("literature")).toEqual([]);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(reopened.state).toEqual(job.state);
		expect(reopened.unsynthesizedLocalEvidence("literature")).toEqual([]);
		await expect(delivery(job, "literature", "stale synthesis", [local.id], "synthesis")).rejects.toThrow(/stale/);
	},
);

it("propagates malformed local input errors instead of hiding programming failures", async () => {
	const { job, store } = await setup();
	const local = await delivery(job, "literature", "local", [], "local");
	const snapshot = job.state;
	Reflect.deleteProperty(snapshot.tasks[local.taskId], "inputArtifactRefs");
	await store.writeSnapshot(snapshot);
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	expect(() => reopened.unsynthesizedLocalEvidence("literature")).toThrow(TypeError);
});

it.each([
	["replace", "live"],
	["replace", "reopened"],
	["reopen", "live"],
	["reopen", "reopened"],
])("excludes historical accepted repairs after %s without crashing recommendations (%s)", async (mode, reader) => {
	const { job, store } = await setup();
	const a = await job.adoptEvidence((await delivery(job, "validation", "source")).id);
	const failed = await delivery(job, "literature", "failed local", [a.id], "local", undefined, "fail");
	const repaired = await delivery(job, "literature", "repaired local", [failed.id, a.id], "local", failed.id);
	expect(job.unsynthesizedLocalEvidence("literature").map((evidence) => evidence.id)).toContain(repaired.id);
	const b = await job.adoptEvidence((await delivery(job, "literature", "synthesis", [repaired.id], "synthesis")).id);
	const c = await job.adoptEvidence((await delivery(job, "literature", "independent")).id, b.id);
	const before = job.state;
	if (mode === "replace") await job.adoptEvidence((await delivery(job, "validation", "replacement")).id, a.id);
	else await job.reopenStage("validation", "reopen", "revisit upstream");
	const current = reader === "live" ? job : (await ResearchJob.open(store, job.state.frame.jobId))!;
	expect(current.state.canonical[c.id]).toEqual(before.canonical[c.id]);
	expect(current.state.canonicalRoute.stageArtifactIds.literature).toBe(c.id);
	expect(current.state.stages.literature).toEqual(before.stages.literature);
	expect(current.state.evidence[repaired.id]).toEqual(before.evidence[repaired.id]);
	expect(current.state.reviews).toEqual(
		expect.objectContaining(
			Object.fromEntries(Object.entries(before.reviews).filter(([, review]) => review.evidenceId === repaired.id)),
		),
	);
	expect(() => current.unsynthesizedLocalEvidence("literature")).not.toThrow();
	expect(current.unsynthesizedLocalEvidence("literature")).toEqual([]);
	await expect(
		delivery(current, "literature", "stale repair", [failed.id, a.id], "local", failed.id),
	).rejects.toThrow();
});

it("rejects a repair genuinely missing its unchanged current input with an ordinary error", async () => {
	const { job } = await setup();
	const a = await job.adoptEvidence((await delivery(job, "validation", "source")).id);
	const failed = await delivery(job, "literature", "failed local", [a.id], "local", undefined, "fail");
	await expect(delivery(job, "literature", "invalid repair", [failed.id], "local", failed.id)).rejects.toMatchObject({
		constructor: Error,
		message: `Repair omits current input ${a.id}`,
	});
});

it("rejects stale cleanup before modifying canonical, task, resource or archived bytes", async () => {
	const { root, job, store } = await setup();
	const e = await delivery(job, "validation", "once");
	const a = await job.adoptEvidence(e.id);
	const paths = [
		a.materializationRef!,
		join(root, ".astra/jobs", job.state.frame.jobId, "workspaces", e.taskId, "partial"),
		join(root, ".astra/jobs", job.state.frame.jobId, "tasks", e.taskId, "packet"),
		join(root, ".astra/jobs", job.state.frame.jobId, "resources", e.taskId, "weights"),
		join(root, ".astra/jobs", job.state.frame.jobId, "archive", "tasks", e.taskId, "workspace", "partial"),
	];
	for (const path of paths) {
		await mkdir(join(path, ".."), { recursive: true });
		await writeFile(path, "retained bytes");
	}
	const stale = (await ResearchJob.open(new JsonlAstraStore(root), job.state.frame.jobId))!;
	await job.consumeTurns(1);
	await expect(stale.reopenStage("validation", "stale", "reject")).rejects.toThrow(/stale/);
	for (const path of paths) expect(await readFile(path, "utf8")).toBe("retained bytes");
	expect((await store.readEvents(job.state.frame.jobId)).at(-1)?.event.type).toBe("budget_usage_recorded");
});

it.each(["wrong-version", "missing-criterion", "low-score", "insufficient-reviews", "failed-review"])(
	"does not select a recovered pass with %s",
	async (mode) => {
		const { job, store } = await setup();
		const definition = job.definitions.validation;
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "quality-search",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "quality-search",
			mode: "search",
			tasks: ["a", "b"].map((key) => ({
				key,
				objective: key,
				hypothesis: key,
				inputArtifactRefs: [],
				requiredOutputFields: definition.requiredOutputFields,
				acceptanceChecks: [],
				failureSignals: [],
				successCriteria: [],
			})),
			rationale: "quality",
			sessionRef: "fixture",
			createdAt: "now",
		};
		await job.recordStagePlan(plan);
		const pe = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [] }));
		const batch = Object.values(job.state.searchBatches)[0];
		let firstReviewId = "";
		for (const [index, planned] of plan.tasks.entries()) {
			const contract = buildEffectiveTaskContract(job, plan, planned);
			const task = await job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:${planned.key}`,
			});
			await job.setTaskStatus(task.id, "succeeded");
			const e = await job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: task.requiredOutputType,
				content: { content: "quality" },
				refs: [],
			});
			const review = await job.recordReview(
				reviewFixture(job, {
					evidenceId: e.id,
					verdict: "pass",
					findings: [],
					blocking: false,
					score: index === 0 && mode === "low-score" ? 0.1 : 1,
				}),
			);
			if (index === 0) {
				firstReviewId = review.id;
				if (mode === "failed-review")
					await job.recordReview(
						reviewFixture(job, {
							evidenceId: e.id,
							verdict: "fail",
							findings: ["negative independent review"],
							blocking: false,
						}),
					);
			}
			await job.recordCandidateEvaluationFromReview(review.id);
		}
		let active = job;
		if (mode === "wrong-version" || mode === "missing-criterion") {
			const snapshot = job.state;
			if (mode === "wrong-version") snapshot.reviews[firstReviewId].targetVersionHash = "wrong";
			else
				snapshot.candidateEvaluations[
					Object.values(snapshot.candidateEvaluations).find((e) => e.reviewId === firstReviewId)!.id
				].criteria = [];
			await store.writeSnapshot(snapshot);
			active = (await ResearchJob.open(store, job.state.frame.jobId))!;
		}
		if (mode === "insufficient-reviews") active.definitions.validation.qualityPolicy!.minPassingReviews = 2;
		if (mode === "wrong-version")
			await expect(active.recordCandidateEvaluationFromReview(firstReviewId)).rejects.toThrow(/match/);
		await expect(
			active.selectSearchCandidate(batch.id, Object.values(batch.candidates)[0].id, "invalid-selection"),
		).rejects.toThrow(/quality|passing|stale|incomplete/);
		expect(active.state.searchBatches[batch.id].status).toBe("evaluating");
	},
);
