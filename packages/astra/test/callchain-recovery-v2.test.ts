import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { archiveAndPruneTasks, cleanupTaskFiles } from "../src/cleanup-files.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";
import { type ResearchMainAgentAdapter, ResearchSupervisor } from "../src/supervisor.ts";
import type {
	AstraEvent,
	Evidence,
	MainAgentDecisionManifest,
	ReviewVerdict,
	StagePlanManifest,
	TaskPacket,
} from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function setup(reviews = 1, rounds = 1) {
	const root = await mkdtemp(join(tmpdir(), "astra-callchain-v2-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const definition = {
		...DEFAULT_STAGES[0],
		qualityPolicy: { minPassingReviews: reviews, minScore: 0.8, requireResolvableArtifacts: false },
		searchPolicy: {
			strategy: "diverse-candidates" as const,
			minCandidates: 2,
			maxCandidates: 2,
			maxRounds: rounds,
			criteria: DEFAULT_STAGES[0].acceptanceChecks,
		},
	};
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Bounded recovery",
		automation: "full",
		definitions: [definition],
	});
	const plan: StagePlanManifest = {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: "plan_search",
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: "plan_search",
		mode: "search",
		tasks: ["a", "b"].map((key) => ({
			key,
			objective: key,
			hypothesis: key,
			inputArtifactRefs: [],
			requiredOutputFields: definition.requiredOutputFields,
			acceptanceChecks: definition.acceptanceChecks,
			failureSignals: definition.failureSignals,
			successCriteria: [],
		})),
		rationale: "Compare independent alternatives",
		sessionRef: "fixture",
		createdAt: new Date().toISOString(),
	};
	return { root, store, job, plan };
}
async function approved(f: Awaited<ReturnType<typeof setup>>, attempts = 1) {
	await f.job.recordStagePlan(f.plan, f.job.state);
	const pe = await preparePlanEvidence(f.job, f.job.state.stagePlans[f.plan.id]);
	await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }),
	);
	const tasks: TaskPacket[] = [];
	for (const planned of f.plan.tasks) {
		const contract = buildEffectiveTaskContract(f.job, f.job.state.stagePlans[f.plan.id], planned);
		let prior: TaskPacket | undefined;
		for (let attempt = 1; attempt <= attempts; attempt++) {
			prior = await f.job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${f.plan.id}:${planned.key}`,
				attempt,
				...(prior ? { supersedesTaskId: prior.id } : {}),
			});
			if (attempt < attempts) await f.job.setTaskStatus(prior.id, "failed");
		}
		tasks.push(prior!);
	}
	return { batch: Object.values(f.job.state.searchBatches)[0], tasks };
}
async function deliver(
	job: ResearchJob,
	task: TaskPacket,
	verdict: ReviewVerdict = "pass",
	score = verdict === "pass" ? 1 : 0,
) {
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: task.objective },
		refs: [],
	});
	const review = await job.recordReview(
		reviewFixture(job, {
			evidenceId: evidence.id,
			verdict,
			score,
			findings: verdict === "pass" ? [] : ["Unsatisfied requirement"],
			blocking: false,
		}),
	);
	await job.recordCandidateEvaluationFromReview(review.id);
	return evidence;
}
function interrupt(store: JsonlAstraStore, type: AstraEvent["type"], snapshot: boolean) {
	const append = store.append.bind(store);
	let hit = false;
	vi.spyOn(store, "append").mockImplementation(async (id, event) => {
		if (!hit && event.type === type) {
			hit = true;
			if (!snapshot) throw new Error("injected append failure");
			vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("injected snapshot failure"));
		}
		return append(id, event);
	});
}
function supervisor(job: ResearchJob, store: JsonlAstraStore) {
	const forbidden = vi.fn(async () => {
		throw new Error("Unexpected model call");
	});
	const mainAgent: ResearchMainAgentAdapter = {
		planStage: forbidden,
		decideEvidence: forbidden,
		decideAdoption: forbidden,
		decideSearch: forbidden,
		decideRoute: forbidden,
	};
	return {
		mainAgent,
		worker: { run: forbidden },
		reviewer: { review: forbidden },
		tick: () =>
			new ResearchSupervisor(job, store, {
				mainAgent,
				worker: { run: forbidden },
				reviewer: { review: forbidden },
			}).tick(),
	};
}

it.each([false, true])(
	"B1 restores terminal failure consequences before the cost gate (snapshot=%s)",
	async (snapshot) => {
		const f = await setup();
		const { batch, tasks } = await approved(f, 3);
		await f.job.setTaskStatus(tasks[0].id, "failed");
		interrupt(f.store, snapshot ? "search_candidate_updated" : "search_batch_exhausted", snapshot);
		await expect(f.job.setTaskStatus(tasks[1].id, "failed")).rejects.toThrow("injected");
		await f.job.updateBudget({ maxCostUsd: 0 });
		const job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await job.recoverPendingOperations();
		await job.recoverPendingOperations();
		expect(job.state.searchBatches[batch.id].status).toBe("exhausted");
		expect(Object.values(job.state.tasks).filter((t) => t.role === "worker")).toHaveLength(6);
	},
);

it.each([false, true])("S1 restores registered search identity before freezing (snapshot=%s)", async (snapshot) => {
	const f = await setup();
	if (snapshot) {
		interrupt(f.store, "stage_plan_recorded", true);
		await expect(f.job.recordStagePlan(f.plan, f.job.state)).rejects.toThrow("injected");
	} else {
		// Historical protocol saved this plan before its separate batch registration failed.
		await f.store.append(f.job.state.frame.jobId, { type: "stage_plan_recorded", plan: f.plan });
	}
	const job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	await job.recoverPendingOperations();
	const pe = await preparePlanEvidence(job, job.state.stagePlans[f.plan.id]);
	expect(Object.values(job.state.searchBatches)).toHaveLength(1);
	expect(pe.content).toMatchObject({
		effectiveContracts: [
			{ contract: { searchBatchId: expect.any(String) } },
			{ contract: { searchBatchId: expect.any(String) } },
		],
	});
});

it.each(["search_batch_decided", "evidence_decided", "cleanup_requested"] as const)(
	"B2 recovers original winner and exact selection after %s failure",
	async (type) => {
		const f = await setup();
		const { batch, tasks } = await approved(f);
		const winner = await deliver(f.job, tasks[0]);
		await deliver(f.job, tasks[1], "fail");
		if (type === "evidence_decided") {
			// Old durable selections had no winner acceptance in the same event.
			await f.store.append(f.job.state.frame.jobId, {
				type: "search_batch_decided",
				batchId: batch.id,
				candidateId: tasks[0].searchCandidateId!,
				decisionRef: "select_original",
			});
			f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
			interrupt(f.store, type, false);
			await expect(f.job.recoverPendingOperations()).rejects.toThrow("injected");
		} else {
			interrupt(f.store, type, type === "search_batch_decided");
			await expect(
				f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "select_original"),
			).rejects.toThrow("injected");
		}
		const job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await job.recoverPendingOperations();
		await job.recoverPendingOperations();
		expect(job.state.evidence[winner.id].status).toBe("accepted");
		expect(Object.keys(job.state.discardedCandidates)).toHaveLength(1);
		const before = job.state;
		await job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "select_original");
		expect(job.state).toEqual(before);
	},
);

it("S2 retains low scores while selecting from later eligible current reviews", async () => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	const winner = await deliver(f.job, tasks[0], "pass", 0.6);
	await deliver(f.job, tasks[1], "fail");
	const r = await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: winner.id, verdict: "pass", findings: [], score: 1, blocking: false }),
	);
	await f.job.recordCandidateEvaluationFromReview(r.id);
	await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "later_qualified");
	expect(f.job.state.evidence[winner.id].status).toBe("accepted");
	expect(Object.values(f.job.state.candidateEvaluations).some((e) => e.score === 0.6)).toBe(true);
});

async function selectedReplacement() {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	const winner = await deliver(f.job, tasks[0]);
	await deliver(f.job, tasks[1]);
	await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original_selection");
	const original = await f.job.adoptEvidence(winner.id);
	const {
		id: _id,
		planId: _planId,
		effectiveContractHash: _hash,
		searchBatchId: _batch,
		searchCandidateId: _candidate,
		...direct
	} = tasks[0];
	const replacementTask = await f.job.dispatchTask({
		...direct,
		objective: "Legitimate replacement",
		replayKey: "replacement",
	});
	await f.job.setTaskStatus(replacementTask.id, "succeeded");
	const replacementEvidence = await f.job.recordEvidence({
		taskId: replacementTask.id,
		stageId: "validation",
		type: replacementTask.requiredOutputType,
		content: { content: "Replacement result" },
		refs: [],
	});
	await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: replacementEvidence.id, verdict: "pass", findings: [], blocking: false }),
	);
	await f.job.decideEvidence(replacementEvidence.id, true, "accept_replacement");
	const replacement = await f.job.adoptEvidence(replacementEvidence.id, original.id);
	return { f, batch, winner, original, replacement };
}
it("B2 completed legitimate winner replacement preserves selection recovery", async () => {
	const { f, batch, winner, original, replacement } = await selectedReplacement();
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	expect(f.job.state.evidence[winner.id]).toBeUndefined();
	expect(f.job.state.retiredArtifacts[original.id].cleanupStatus).toBe("completed");
	const before = f.job.state;
	await f.job.recoverPendingOperations();
	await f.job.recoverPendingOperations();
	expect(f.job.state).toEqual(before);
	expect(f.job.state.canonical[replacement.id]).toBeDefined();
	expect(f.job.state.searchBatches[batch.id]).toMatchObject({ status: "selected", decisionRef: "original_selection" });
});

it.each([false, true])(
	"B2 a legally accepted successor does not revive the rejected search winner (legacy=%s)",
	async (legacy) => {
		const f = await setup();
		const { batch, tasks } = await approved(f);
		const winner = await deliver(f.job, tasks[0]);
		await deliver(f.job, tasks[1]);
		await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original_selection");
		const {
			id: _id,
			planId: _planId,
			effectiveContractHash: _hash,
			searchBatchId: _batch,
			searchCandidateId: _candidate,
			...direct
		} = tasks[0];
		const successorTask = await f.job.dispatchTask({
			...direct,
			objective: "Accepted successor",
			replayKey: "successor",
		});
		await f.job.setTaskStatus(successorTask.id, "succeeded");
		const successor = await f.job.recordEvidence({
			taskId: successorTask.id,
			stageId: "validation",
			type: successorTask.requiredOutputType,
			currentEvidenceSetId: winner.currentEvidenceSetId ?? winner.taskId,
			content: { content: "Repaired original result" },
			refs: [],
		});
		await f.job.recordReview(
			reviewFixture(f.job, { evidenceId: successor.id, verdict: "pass", findings: [], blocking: false }),
		);
		await f.job.decideEvidence(successor.id, true, "accept_successor");
		expect(f.job.state.evidence[winner.id].status).toBe("rejected");
		if (legacy) {
			const snapshot = f.job.state;
			delete snapshot.searchBatches[batch.id].acceptanceCompleted;
			await f.store.writeSnapshot(snapshot);
		}
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		const before = f.job.state;
		await f.job.recoverPendingOperations();
		const recovered = f.job.state;
		await f.job.recoverPendingOperations();
		expect(f.job.state.evidence[winner.id].status).toBe("rejected");
		expect(f.job.state.evidence[successor.id].status).toBe("accepted");
		expect(f.job.state).toEqual(recovered);
		if (!legacy) expect(recovered).toEqual(before);
		else expect(recovered.searchBatches[batch.id].acceptanceCompleted).toBe(true);
	},
);

it.each(
	(["current", "retired"] as const).flatMap((state) =>
		(["none", "append", "snapshot"] as const).map((fault) => ({ state, fault })),
	),
)("B2 legacy $state winner backfills completion once ($fault)", async ({ state, fault }) => {
	const seeded = state === "retired" ? await selectedReplacement() : undefined;
	const f = seeded?.f ?? (await setup());
	const { batch, tasks } = seeded ? { batch: seeded.batch, tasks: [] } : await approved(f);
	if (!seeded) {
		await deliver(f.job, tasks[0]);
		await deliver(f.job, tasks[1]);
		await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original_selection");
	}
	const legacy = f.job.state;
	delete legacy.searchBatches[batch.id].acceptanceCompleted;
	await f.store.writeSnapshot(legacy);
	f.job = (await ResearchJob.open(f.store, legacy.frame.jobId))!;
	if (fault !== "none") {
		interrupt(f.store, "evidence_acceptance_recovered", fault === "snapshot");
		await expect(f.job.recoverPendingOperations()).rejects.toThrow("injected");
		f.job = (await ResearchJob.open(f.store, legacy.frame.jobId))!;
	}
	await f.job.recoverPendingOperations();
	expect(f.job.state.searchBatches[batch.id].acceptanceCompleted).toBe(true);
	if (seeded) expect(f.job.state.evidence[seeded.winner.id]).toBeUndefined();
	const after = f.job.state;
	await f.job.recoverPendingOperations();
	expect(f.job.state).toEqual(after);
	expect(
		(await f.store.readEvents(legacy.frame.jobId)).filter(
			(saved) =>
				saved.event.type === "evidence_acceptance_recovered" && saved.event.searchSelection?.batchId === batch.id,
		),
	).toHaveLength(1);
});

it("B2 missing legacy winner with no bound retirement stops without changing any state", async () => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	const winner = await deliver(f.job, tasks[0]);
	await deliver(f.job, tasks[1]);
	await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original_selection");
	const legacy = f.job.state;
	delete legacy.searchBatches[batch.id].acceptanceCompleted;
	delete legacy.evidence[winner.id];
	await f.store.writeSnapshot(legacy);
	f.job = (await ResearchJob.open(f.store, legacy.frame.jobId))!;
	const before = f.job.state;
	await expect(f.job.recoverPendingOperations()).rejects.toThrow(/original selected search evidence missing/);
	expect(f.job.state).toEqual(before);
});

it.each(["acceptance", "recorded-identity", "successor"] as const)(
	"Q3 legacy missing %s proof rejects with zero effects",
	async (missing) => {
		const f = await setup();
		const { batch, tasks } = await approved(f);
		const winner = await deliver(f.job, tasks[0]);
		await deliver(f.job, tasks[1]);
		await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original_selection");
		const legacy = f.job.state;
		delete legacy.searchBatches[batch.id].acceptanceCompleted;
		const store = new MemoryAstraStore();
		for (const saved of await f.store.readEvents(legacy.frame.jobId)) {
			const event = saved.event;
			if (missing === "recorded-identity" && event.type === "evidence_recorded" && event.evidence.id === winner.id)
				continue;
			if (
				missing === "acceptance" &&
				event.type === "evidence_decided" &&
				event.evidenceId === winner.id &&
				event.accepted
			)
				continue;
			if (missing === "acceptance" && event.type === "search_batch_decided" && event.batchId === batch.id) {
				const { acceptance: _acceptance, ...selection } = event;
				await store.append(saved.jobId, selection);
			} else await store.append(saved.jobId, event);
		}
		if (missing === "successor") {
			legacy.evidence[winner.id].status = "rejected";
			legacy.evidence[winner.id].supersededByTaskId = tasks[1].id;
		}
		legacy.eventSeq = await store.readEventSeq(legacy.frame.jobId);
		await store.writeSnapshot(legacy);
		const job = (await ResearchJob.open(store, legacy.frame.jobId))!;
		const before = job.state;
		await expect(job.recoverPendingOperations()).rejects.toThrow(/cannot be proved|identity|legal successor/);
		expect(job.state).toEqual(before);
		expect(await store.readEventSeq(legacy.frame.jobId)).toBe(legacy.eventSeq);
	},
);

it.each(["none", "append", "snapshot"] as const)(
	"Q2 legacy original acceptance precedes pending loser completion (%s)",
	async (fault) => {
		const f = await setup();
		const { batch, tasks } = await approved(f);
		const winner = await deliver(f.job, tasks[0]);
		await deliver(f.job, tasks[1]);
		await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original_selection");
		await f.job.recordReview(
			reviewFixture(f.job, {
				evidenceId: winner.id,
				verdict: "fail",
				findings: ["A later independent question"],
				blocking: true,
			}),
		);
		const laterObligation = f.job.state.frame.openObligationIds[0];
		const store = new MemoryAstraStore();
		for (const saved of await f.store.readEvents(f.job.state.frame.jobId)) {
			const event = saved.event;
			if (event.type === "cleanup_completed") continue;
			if (event.type === "search_batch_decided" && event.batchId === batch.id) {
				const { acceptance: _acceptance, ...selection } = event;
				await store.append(saved.jobId, selection);
				await store.append(saved.jobId, {
					type: "evidence_decided",
					evidenceId: winner.id,
					accepted: true,
					decisionRef: "original_selection",
				});
			} else await store.append(saved.jobId, event);
		}
		let legacy = (await ResearchJob.open(store, f.job.state.frame.jobId))!;
		expect(legacy.state.searchBatches[batch.id].acceptanceCompleted).not.toBe(true);
		expect(Object.values(legacy.state.cleanupIntents!).some((intent) => intent.status === "pending")).toBe(true);
		const append = store.append.bind(store);
		let interrupted = false;
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (!interrupted && fault !== "none" && event.type === "evidence_acceptance_recovered") {
				interrupted = true;
				if (fault === "append") throw new Error("Original acceptance append interrupted");
				vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(
					new Error("Original acceptance snapshot interrupted"),
				);
			}
			if (event.type === "cleanup_completed")
				expect(legacy.state.searchBatches[batch.id].acceptanceCompleted).toBe(true);
			return append(id, event);
		});
		if (fault !== "none") {
			await expect(legacy.recoverPendingOperations()).rejects.toThrow(/Original acceptance/);
			legacy = (await ResearchJob.open(store, legacy.state.frame.jobId))!;
		}
		await legacy.recoverPendingOperations();
		expect(legacy.state.obligations[laterObligation].status).toBe("open");
		expect(legacy.state.searchBatches[batch.id].acceptanceCompleted).toBe(true);
		expect(Object.values(legacy.state.cleanupIntents!).every((intent) => intent.status === "completed")).toBe(true);
		const done = legacy.state;
		await legacy.recoverPendingOperations();
		expect(legacy.state).toEqual(done);
	},
);

it.each(["wrong-version", "incomplete"] as const)(
	"S2 excludes saved %s assessments from readiness and selection",
	async (fault) => {
		const f = await setup(2);
		const { batch, tasks } = await approved(f);
		const evidence = await deliver(f.job, tasks[0]);
		await deliver(f.job, tasks[1], "fail");
		const original = Object.values(f.job.state.reviews).find((review) => review.evidenceId === evidence.id)!;
		const originalEvaluation = Object.values(f.job.state.candidateEvaluations).find(
			(evaluation) => evaluation.reviewId === original.id,
		)!;
		const review = {
			...original,
			id: "historical_invalid_review",
			...(fault === "wrong-version"
				? { targetVersionHash: "foreign-version" }
				: { criteria: original.criteria!.slice(1) }),
		};
		await f.store.append(f.job.state.frame.jobId, { type: "review_recorded", review });
		await f.store.append(f.job.state.frame.jobId, {
			type: "candidate_evaluation_recorded",
			evaluation: {
				...originalEvaluation,
				id: "historical_invalid_evaluation",
				reviewId: review.id,
				criteria: review.criteria!,
			},
		});
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		const qualification = f.job
			.searchQualification(batch.id)
			.candidates.find((candidate) => candidate.candidateId === tasks[0].searchCandidateId)!;
		expect(qualification.evaluations).toHaveLength(1);
		expect(qualification.ready).toBe(false);
		expect(qualification.eligible).toBe(false);
		const before = f.job.state;
		await expect(
			f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "invalid_selection"),
		).rejects.toThrow(/quality threshold/);
		expect(f.job.state).toEqual(before);
	},
);

it("S3 stale first selection and conflicting replay leave every state unchanged", async () => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	await deliver(f.job, tasks[0]);
	await deliver(f.job, tasks[1]);
	await f.job.reopenStage("validation", "reopen", "Supersede old execution");
	const before = f.job.state;
	await expect(f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "late")).rejects.toThrow(
		/stale|superseded|current/,
	);
	expect(f.job.state).toEqual(before);
});

it("S3 conflicts reject before reading deleted losers and exact replay remains unchanged", async () => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	await deliver(f.job, tasks[0]);
	const loser = await deliver(f.job, tasks[1]);
	await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original");
	expect(f.job.state.evidence[loser.id]).toBeUndefined();
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	const before = f.job.state;
	for (const [candidate, ref] of [
		[tasks[1].searchCandidateId!, "original"],
		[tasks[0].searchCandidateId!, "different"],
	]) {
		await expect(f.job.selectSearchCandidate(batch.id, candidate, ref)).rejects.toThrow(/conflict/);
		expect(f.job.state).toEqual(before);
	}
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	await f.job.selectSearchCandidate(batch.id, tasks[0].searchCandidateId!, "original");
	expect(f.job.state).toEqual(before);
});

function route(
	job: ResearchJob,
	action: "continue" | "search" | "ask-user" | "backtrack",
	ref: string,
): MainAgentDecisionManifest {
	return {
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: ref,
		jobId: job.state.frame.jobId,
		decisionType: "route",
		stageId: "validation",
		decisionRef: ref,
		routeAction: action,
		...(action === "ask-user" ? { question: "Change direction?" } : {}),
		...(action === "backtrack" ? { targetStageId: "validation" } : {}),
		rationale: "Explicit bounded route",
		sessionRef: "offline",
		createdAt: new Date().toISOString(),
	};
}

it.each(["fail", "partial", "blocked", "low", "mixed"] as const)(
	"B3 final %s routes once without choosing or scientific success",
	async (outcome) => {
		const f = await setup();
		const { batch, tasks } = await approved(f, outcome === "mixed" ? 3 : 1);
		if (outcome === "mixed") await f.job.setTaskStatus(tasks[0].id, "failed");
		else await deliver(f.job, tasks[0], outcome === "low" ? "pass" : outcome, outcome === "low" ? 0.6 : 0);
		await deliver(
			f.job,
			tasks[1],
			outcome === "mixed" ? "fail" : outcome === "low" ? "pass" : outcome,
			outcome === "low" ? 0.6 : 0,
		);
		const guards = supervisor(f.job, f.store);
		const decideRoute = vi.fn(async () => route(f.job, "ask-user", "negative_route"));
		const tick = () =>
			new ResearchSupervisor(f.job, f.store, { ...guards, mainAgent: { ...guards.mainAgent, decideRoute } }).tick();
		await tick();
		expect(f.job.state.searchBatches[batch.id].status).toBe("exhausted");
		expect(guards.mainAgent.decideSearch).not.toHaveBeenCalled();
		expect(f.job.state.frame.userGate).toMatchObject({ kind: "research", question: "Change direction?" });
		expect(f.job.state.frame.scientificOutcome).toBe("pending");
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await tick();
		expect(decideRoute).toHaveBeenCalledOnce();
	},
);

it("S8 final negative continue consumes one ordinary plan while preserving retries and round limits", async () => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	for (const task of tasks) await deliver(f.job, task, "fail");
	await f.job.exhaustNegativeSearch(batch.id);
	await f.job.applyRouteDecision(route(f.job, "continue", "ordinary_continue"));
	let plans = 0;
	const guards = supervisor(f.job, f.store);
	const planStage = vi.fn(async (_job: ResearchJob, _obligation: unknown, mode?: string) => {
		expect(mode).toBe("decompose");
		plans++;
		return {
			...f.plan,
			id: `ordinary_${plans}`,
			decisionRef: `ordinary_${plans}`,
			mode: "decompose" as const,
			tasks: [{ ...f.plan.tasks[0], key: "ordinary" }],
			createdAt: new Date().toISOString(),
		};
	});
	const run = vi.fn(async () => {
		throw new Error("Real no-output worker failure");
	});
	const tick = () =>
		new ResearchSupervisor(f.job, f.store, {
			...guards,
			worker: { run },
			reviewer: {
				review: async (e) =>
					reviewFixture(f.job, { evidenceId: e.id, verdict: "pass", findings: [], blocking: false }),
			},
			mainAgent: { ...guards.mainAgent, planStage },
		}).tick();
	for (let attempt = 0; attempt < 5; attempt++) {
		await tick();
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	}
	expect(plans).toBe(1);
	expect(run).toHaveBeenCalledTimes(3);
	expect(f.job.state.routeDecisions.ordinary_continue.continuedPlanId).toBe("ordinary_1");
	expect(f.job.state.frame.userGate).toMatchObject({
		kind: "research",
		decisionRef: "ordinary_continue",
		planId: "ordinary_1",
	});
	const originalGate = f.job.state.frame.userGate;
	await tick();
	expect(f.job.state.frame.userGate).toEqual(originalGate);
	const before = f.job.state;
	await expect(f.job.applyRouteDecision(route(f.job, "search", "same_revision_search"))).rejects.toThrow(
		/maximum rounds/,
	);
	await expect(
		f.job.recordStagePlan({ ...f.plan, id: "same_revision_plan", decisionRef: "same_revision_plan" }),
	).rejects.toThrow(/maximum rounds/);
	expect(f.job.state).toEqual(before);
	await f.job.recordUserGuidance("New guidance cannot reset rounds");
	expect(f.job.state.stages.validation.lastRouteDecisionRef).toBeUndefined();
	expect(f.job.state.routeDecisions.ordinary_continue.continuedPlanId).toBe("ordinary_1");
	const decideRoute = vi.fn(async () => route(f.job, "ask-user", "new_guided_route"));
	await new ResearchSupervisor(f.job, f.store, {
		...guards,
		mainAgent: { ...guards.mainAgent, planStage, decideRoute },
	}).tick();
	expect(decideRoute).toHaveBeenCalledOnce();
	expect(plans).toBe(1);
	await f.job.recordUserGuidance("Backtrack with new revision");
	await expect(
		f.job.recordStagePlan({ ...f.plan, id: "guidance_search", decisionRef: "guidance_search" }),
	).rejects.toThrow(/maximum rounds/);
	await f.job.applyRouteDecision(route(f.job, "backtrack", "new_revision"));
	await f.job.recordStagePlan({ ...f.plan, id: "new_revision_search", decisionRef: "new_revision_search" });
	expect(Object.values(f.job.state.searchBatches).find((value) => value.planId === "new_revision_search")?.round).toBe(
		1,
	);
});

async function negativeContinue() {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	for (const task of tasks) await deliver(f.job, task, "fail");
	await f.job.exhaustNegativeSearch(batch.id);
	await f.job.applyRouteDecision(route(f.job, "continue", "once_route"));
	const ordinary: StagePlanManifest = {
		...f.plan,
		id: "once_plan",
		decisionRef: "once_plan",
		mode: "decompose",
		tasks: [{ ...f.plan.tasks[0], key: "ordinary" }],
	};
	return { f, ordinary };
}

it.each([false, true])("S8 continuation plan consumption is atomic (snapshot=%s)", async (snapshot) => {
	const { f, ordinary } = await negativeContinue();
	interrupt(f.store, "stage_plan_recorded", snapshot);
	await expect(f.job.recordStagePlan(ordinary, f.job.state)).rejects.toThrow("injected");
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	expect(f.job.state.routeDecisions.once_route.continuedPlanId).toBe(snapshot ? ordinary.id : undefined);
	expect(Boolean(f.job.state.stagePlans[ordinary.id])).toBe(snapshot);
	await f.job.recordStagePlan(ordinary, f.job.state);
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	const before = f.job.state;
	await f.job.recordStagePlan(ordinary);
	expect(f.job.state).toEqual(before);
	await expect(f.job.recordStagePlan({ ...ordinary, id: "second_plan", decisionRef: "second_plan" })).rejects.toThrow(
		/already consumed/,
	);
	expect(f.job.state).toEqual(before);
	const savedEvent = (await f.store.readEvents(f.job.state.frame.jobId)).find(
		(entry) => entry.event.type === "stage_plan_recorded" && entry.event.plan.id === ordinary.id,
	);
	expect(savedEvent?.event).toMatchObject({ continueDecisionRef: "once_route" });
});

it.each([false, true])("S8 exhausted gate and exact guidance recover atomically (snapshot=%s)", async (snapshot) => {
	const { f, ordinary } = await negativeContinue();
	await f.job.recordStagePlan(ordinary, f.job.state);
	const planEvidence = await preparePlanEvidence(f.job, f.job.state.stagePlans[ordinary.id]);
	await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: planEvidence.id, verdict: "pass", findings: [], blocking: false }),
	);
	const contract = buildEffectiveTaskContract(f.job, f.job.state.stagePlans[ordinary.id], ordinary.tasks[0]);
	let prior: TaskPacket | undefined;
	for (let attempt = 1; attempt <= 3; attempt++) {
		prior = await f.job.dispatchTask({
			...contract,
			effectiveContractHash: semanticContractHash(contract),
			replayKey: `stage-plan:${ordinary.id}:ordinary`,
			attempt,
			...(prior ? { supersedesTaskId: prior.id } : {}),
		});
		await f.job.setTaskStatus(prior.id, "failed");
	}
	interrupt(f.store, "user_gate_required", snapshot);
	await expect(supervisor(f.job, f.store).tick()).rejects.toThrow("injected");
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	expect(Boolean(f.job.state.frame.userGate)).toBe(snapshot);
	await supervisor(f.job, f.store).tick();
	const gate = f.job.state.frame.userGate;
	expect(gate).toMatchObject({ kind: "research", decisionRef: "once_route", planId: ordinary.id });
	for (let recovery = 0; recovery < 2; recovery++) {
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await supervisor(f.job, f.store).tick();
		expect(f.job.state.frame.userGate).toEqual(gate);
	}
	vi.restoreAllMocks();
	interrupt(f.store, "user_guidance_recorded", snapshot);
	await expect(f.job.resumeWithGuidance("Change direction")).rejects.toThrow("injected");
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	expect(f.job.state.stages.validation.lastRouteDecisionRef).toBe(snapshot ? undefined : "once_route");
	expect(f.job.state.frame.userGate).toEqual(snapshot ? undefined : gate);
	expect(f.job.state.routeDecisions.once_route).toMatchObject({ action: "continue", continuedPlanId: ordinary.id });
	if (!snapshot) await f.job.resumeWithGuidance("Change direction");
	const guards = supervisor(f.job, f.store);
	const decideRoute = vi.fn(async () => route(f.job, "ask-user", "after_answer"));
	await new ResearchSupervisor(f.job, f.store, { ...guards, mainAgent: { ...guards.mainAgent, decideRoute } }).tick();
	expect(decideRoute).toHaveBeenCalledOnce();
	expect(guards.mainAgent.planStage).not.toHaveBeenCalled();
});

it.each([
	"pause",
	"backoff",
	"research",
	"budget",
	"stage",
	"obligation",
	"ready",
	"running",
	"retry",
	"output",
	"pending",
] as const)("S8 related continuation preserves the %s protection", async (protection) => {
	const { f, ordinary } = await negativeContinue();
	await f.job.recordStagePlan(ordinary, f.job.state);
	const pe = await preparePlanEvidence(f.job, f.job.state.stagePlans[ordinary.id]);
	await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }),
	);
	const contract = buildEffectiveTaskContract(f.job, f.job.state.stagePlans[ordinary.id], ordinary.tasks[0]);
	let latest: TaskPacket | undefined;
	for (let attempt = 1; attempt <= (protection === "retry" ? 2 : 3); attempt++) {
		latest = await f.job.dispatchTask({
			...contract,
			effectiveContractHash: semanticContractHash(contract),
			replayKey: `stage-plan:${ordinary.id}:ordinary`,
			attempt,
			...(latest ? { supersedesTaskId: latest.id } : {}),
		});
		await f.job.setTaskStatus(latest.id, "failed");
	}
	if (protection === "pause") await f.job.pause("Existing ordinary pause");
	else if (protection === "backoff")
		await f.job.recordProviderBackoff({
			attempt: 1,
			reason: "Provider cooling down",
			startedAt: new Date().toISOString(),
			retryAt: new Date(Date.now() + 60000).toISOString(),
		});
	else if (protection === "research")
		await f.job.requireUserGate({
			kind: "research",
			stageId: "validation",
			question: "Existing question?",
			reason: "Existing gate",
		});
	else if (protection === "budget")
		await f.job.requireUserGate({
			kind: "budget",
			stageId: "validation",
			limit: "maxTurns",
			reason: "Existing gate",
		});
	else if (protection === "stage")
		await f.job.requireUserGate({ kind: "stage", stageId: "validation", phase: "route", reason: "Existing gate" });
	else if (protection === "obligation")
		await f.job.recordReview(
			reviewFixture(f.job, {
				evidenceId: pe.id,
				verdict: "fail",
				findings: ["Repair original plan"],
				blocking: true,
			}),
		);
	else if (protection === "ready" || protection === "running") await f.job.setTaskStatus(latest!.id, protection);
	else if (protection === "output") {
		await f.job.setTaskStatus(latest!.id, "succeeded");
		await f.job.recordEvidence({
			taskId: latest!.id,
			stageId: "validation",
			type: latest!.requiredOutputType,
			content: { content: "Saved original output" },
			refs: [],
		});
	} else if (protection === "pending") {
		const path = join(f.root, ".astra", "jobs", f.job.state.frame.jobId, "main-agent", "decisions", "pending.json");
		await f.job.registerMainAgentCall({ id: "pending", type: "route", manifestRef: path, manifestId: "pending" });
		await mkdir(join(path, ".."), { recursive: true });
		await writeFile(path, "invalid saved delivery");
	}
	const originalGate = f.job.state.frame.userGate;
	// A separate budget exit prevents unrelated model work after the S8-specific predicate returns false.
	if (protection === "obligation") expect(f.job.state.frame.openObligationIds.length).toBeGreaterThan(0);
	if (protection === "output")
		expect(Object.values(f.job.state.evidence).some((evidence) => evidence.taskId === latest!.id)).toBe(true);
	await f.job.updateBudget({ maxCostUsd: 0 });
	const guards = supervisor(f.job, f.store);
	if (protection === "pending") await expect(guards.tick()).rejects.toThrow(/JSON|Unexpected|valid/);
	else await guards.tick();
	expect(f.job.state.frame.userGate).not.toMatchObject({ kind: "research", decisionRef: "once_route" });
	if (originalGate) expect(f.job.state.frame.userGate).toEqual(originalGate);
	expect(guards.worker.run).not.toHaveBeenCalled();
	expect(guards.mainAgent.planStage).not.toHaveBeenCalled();
	expect(f.job.state.routeDecisions.once_route.continuedPlanId).toBe(ordinary.id);
});

it("S4 waits for a second full review and B3 bounds repeated low-score review", async () => {
	const f = await setup(2);
	const { batch, tasks } = await approved(f);
	await deliver(f.job, tasks[0]);
	await deliver(f.job, tasks[1]);
	const blocked = supervisor(f.job, f.store);
	const low = vi.fn(async (e: Evidence) =>
		reviewFixture(f.job, { evidenceId: e.id, verdict: "pass", findings: [], score: 0.6, blocking: false }),
	);
	const decideSearch = vi.fn();
	await new ResearchSupervisor(f.job, f.store, {
		...blocked,
		reviewer: { review: low },
		mainAgent: { ...blocked.mainAgent, decideSearch },
	}).tick();
	expect(decideSearch).not.toHaveBeenCalled();
	expect(f.job.state.searchBatches[batch.id].status).toBe("exhausted");
	await new ResearchSupervisor(f.job, f.store, {
		...blocked,
		reviewer: { review: low },
		mainAgent: { ...blocked.mainAgent, decideSearch },
	}).tick();
	expect(low).toHaveBeenCalledTimes(2);
});

it("S4 concurrent registration of one review cannot count as two independent assessments", async () => {
	const f = await setup(2);
	const { batch, tasks } = await approved(f);
	await f.job.setTaskStatus(tasks[0].id, "succeeded");
	const evidence = await f.job.recordEvidence({
		taskId: tasks[0].id,
		stageId: tasks[0].stageId,
		type: tasks[0].requiredOutputType,
		content: { content: "Original result" },
		refs: [],
	});
	const review = await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: evidence.id, verdict: "pass", score: 1, findings: [], blocking: false }),
	);
	const [first, second] = await Promise.all([
		f.job.recordCandidateEvaluationFromReview(review.id),
		f.job.recordCandidateEvaluationFromReview(review.id),
	]);
	expect(first.id).toBe(second.id);
	await f.store.append(f.job.state.frame.jobId, {
		type: "candidate_evaluation_recorded",
		evaluation: { ...first, id: "legacy_duplicate_same_review" },
	});
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	const qualification = f.job
		.searchQualification(batch.id)
		.candidates.find((candidate) => candidate.candidateId === tasks[0].searchCandidateId)!;
	expect(qualification.evaluations).toHaveLength(1);
	expect(qualification.ready).toBe(false);
	expect(qualification.eligible).toBe(false);
});

it.each([1, 3, 5])("S5 rejects %s candidates outside configured or hard bounds before any event", async (count) => {
	const f = await setup();
	f.plan.tasks = Array.from({ length: count }, (_, index) => ({
		...f.plan.tasks[0],
		key: `candidate_${index}`,
		hypothesis: `candidate_${index}`,
	}));
	const before = f.job.state;
	await expect(f.job.recordStagePlan(f.plan)).rejects.toThrow(/candidate|maximum|one to/);
	expect(f.job.state).toEqual(before);
});

it("S7 pause during a search call preserves candidates until explicit resume", async () => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	const winner = await deliver(f.job, tasks[0]);
	await deliver(f.job, tasks[1], "fail");
	const options = supervisor(f.job, f.store);
	options.mainAgent.decideSearch = vi.fn(async (): Promise<MainAgentDecisionManifest> => {
		await f.job.requireUserGate({
			kind: "research",
			stageId: "validation",
			question: "Which direction?",
			reason: "Pause while deciding",
		});
		return {
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: "paused_selection",
			jobId: f.job.state.frame.jobId,
			decisionType: "search-selection",
			searchBatchId: batch.id,
			stageId: "validation",
			sessionRef: "offline",
			createdAt: new Date().toISOString(),
			selectedCandidateId: tasks[0].searchCandidateId!,
			decisionRef: "paused_selection",
			rationale: "Saved decision",
		};
	});
	await options.tick();
	expect(options.mainAgent.decideSearch).toHaveBeenCalledOnce();
	expect(f.job.state.paused).toBe(true);
	expect(f.job.state.searchBatches[batch.id].status).toBe("evaluating");
	expect(f.job.state.evidence[winner.id].status).toBe("candidate");
	expect(Object.keys(f.job.state.discardedCandidates)).toHaveLength(0);
});

it.each([false, true])("C1 guidance and work supersession are atomic (snapshot=%s)", async (snapshot) => {
	const f = await setup();
	const { batch, tasks } = await approved(f);
	const task = await f.job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "old work",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["checked"],
		failureSignals: ["missing"],
		successCriteria: [],
		dependencies: [],
		scope: { workspaceRoot: f.root, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 1, maxToolCalls: 1, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	await f.job.requireUserGate({
		kind: "research",
		stageId: "validation",
		question: "Change?",
		reason: "Need guidance",
	});
	interrupt(f.store, "user_guidance_recorded", snapshot);
	await expect(f.job.resumeWithGuidance("Use a different method")).rejects.toThrow("injected");
	const job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	expect(job.state.paused).toBe(!snapshot);
	expect(job.state.tasks[task.id].status).toBe(snapshot ? "blocked" : "ready");
	expect(tasks.map((previous) => job.state.tasks[previous.id].status)).toEqual([
		snapshot ? "blocked" : "ready",
		snapshot ? "blocked" : "ready",
	]);
	expect(job.state.searchBatches[batch.id].status).toBe(snapshot ? "superseded" : "running");
});

it.each(["budget", "stage"] as const)("C1 guidance cannot release a %s gate", async (kind) => {
	const f = await setup();
	await approved(f);
	await f.job.requireUserGate(
		kind === "budget"
			? { kind, stageId: "validation", limit: "maxTurns", reason: "Budget permission" }
			: { kind, stageId: "validation", phase: "route", reason: "Stage permission" },
	);
	const before = f.job.state;
	await expect(f.job.resumeWithGuidance("New research preference")).rejects.toThrow(/budget|stage|gate/);
	expect(f.job.state).toEqual(before);
});

it.each(["workspace", "task", "resources", "sessions"] as const)(
	"C2 captures four original hashes and blocks legacy missing %s without removing other sources",
	async (missing) => {
		const f = await setup();
		const { tasks } = await approved(f);
		const task = tasks[0];
		const base = join(f.root, ".astra", "jobs", task.jobId);
		const files = [
			join(base, "workspaces", task.id, "work"),
			join(base, "tasks", task.id, "packet"),
			join(base, "resources", task.id, "result"),
			join(f.root, "registered-session.jsonl"),
		];
		for (const file of files) {
			await mkdir(join(file, ".."), { recursive: true });
			await writeFile(file, file);
		}
		await f.job.recordChildSession({
			sessionId: "registered",
			taskId: task.id,
			role: "worker",
			attempt: 1,
			status: "completed",
			sessionFile: files[3],
			updatedAt: new Date().toISOString(),
		});
		expect(await cleanupTaskFiles(f.job.state, [task.id])).toEqual([]);
		await f.job.setTaskStatus(task.id, "failed");
		const captured = await cleanupTaskFiles(f.job.state, [task.id]);
		expect(captured).toHaveLength(1);
		expect(
			[
				captured[0].workspaceHash,
				captured[0].taskHash,
				captured[0].resourcesHash,
				captured[0].sessions[0].expectedHash,
			].every(Boolean),
		).toBe(true);
		const legacy = structuredClone(captured);
		if (missing === "sessions") delete legacy[0].sessions[0].expectedHash;
		else delete legacy[0][`${missing}Hash`];
		await expect(archiveAndPruneTasks(f.job.state, legacy)).rejects.toThrow(/expected hash missing/);
		for (const file of files) expect(await readFile(file, "utf8")).toBe(file);
	},
);

it.each([false, true])(
	"C2 validates the complete archive after partial source deletion (damage=%s)",
	async (damage) => {
		const f = await setup();
		const { tasks } = await approved(f);
		const task = tasks[0];
		const source = join(f.root, ".astra", "jobs", task.jobId, "workspaces", task.id);
		await mkdir(source, { recursive: true });
		await writeFile(join(source, "a"), "A");
		await writeFile(join(source, "b"), "B");
		expect(await cleanupTaskFiles(f.job.state, [task.id])).toEqual([]);
		await f.job.setTaskStatus(task.id, "failed");
		const intent = await cleanupTaskFiles(f.job.state, [task.id]);
		expect(intent).toHaveLength(1);
		expect(intent[0].workspaceHash).toBeTruthy();
		const target = join(f.root, ".astra", "jobs", task.jobId, "archive", "tasks", task.id, "workspace");
		await mkdir(join(target, ".."), { recursive: true });
		await cp(source, target, { recursive: true });
		await rm(join(source, "a"));
		if (damage) await rm(join(target, "a"));
		if (damage) {
			await expect(archiveAndPruneTasks(f.job.state, intent)).rejects.toThrow(/archive|hash|integrity/);
			expect(await readFile(join(source, "b"), "utf8")).toBe("B");
		} else {
			await archiveAndPruneTasks(f.job.state, intent);
			expect(await readFile(join(target, "a"), "utf8")).toBe("A");
		}
	},
);
