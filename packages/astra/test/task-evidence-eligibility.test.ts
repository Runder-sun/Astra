import { rm } from "node:fs/promises";
import { afterEach, expect, it, vi } from "vitest";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { evidenceHasCurrentPlanApproval, planReviewStatus, preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { Evidence } from "../src/types.ts";
import { lifecycleDefinition, lifecycleScenario } from "./lifecycle-fixture.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function scenario(backend: "pi" | "codex") {
	const f = await lifecycleScenario(backend);
	roots.push(f.root);
	return f;
}
async function candidate(f: Awaited<ReturnType<typeof scenario>>, objective: string, refs: string[] = []) {
	const task = await f.task(objective, refs);
	await f.job.setTaskStatus(task.id, "succeeded");
	return f.job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: objective },
		refs: [],
	});
}
async function pass(f: Awaited<ReturnType<typeof scenario>>, evidence: Evidence) {
	await f.job.recordReview(reviewFixture(f.job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
}
async function adopt(f: Awaited<ReturnType<typeof scenario>>, evidence: Evidence) {
	await pass(f, evidence);
	await f.job.decideEvidence(evidence.id, true);
	return f.job.adoptEvidence(evidence.id);
}

async function approvedPlan(f: Awaited<ReturnType<typeof scenario>>) {
	f.control.allowPlanning = true;
	const plan = await f.mainAgent.planStage(f.job);
	await f.job.recordStagePlan(plan);
	const evidence = await preparePlanEvidence(f.job, plan);
	await pass(f, evidence);
	f.control.allowPlanning = false;
	return { plan, evidence };
}

async function adoptedRepair() {
	const f = await scenario("pi");
	const first = await candidate(f, "original incomplete delivery");
	await f.job.recordReview(
		reviewFixture(f.job, { evidenceId: first.id, verdict: "fail", findings: ["missing control"] }),
	);
	const obligation = Object.values(f.job.state.obligations).find((item) => item.evidenceId === first.id)!;
	const plan = await f.job.recordStagePlan({
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: "adopted_repair",
		jobId: f.job.state.frame.jobId,
		stageId: first.stageId,
		decisionRef: "adopted_repair",
		mode: "repair",
		obligationId: obligation.id,
		tasks: [
			{
				key: "repair",
				objective: "repair the missing control",
				deliveryKind: "stage",
				inputArtifactRefs: [],
				requiredOutputFields: ["content"],
				acceptanceChecks: lifecycleDefinition.acceptanceChecks,
				successCriteria: lifecycleDefinition.acceptanceChecks,
				failureSignals: lifecycleDefinition.failureSignals,
			},
		],
		rationale: "repair the original comparison",
		sessionRef: "offline_manual_plan",
		createdAt: new Date().toISOString(),
	});
	await pass(f, await preparePlanEvidence(f.job, plan));
	const contract = buildEffectiveTaskContract(f.job, plan, plan.tasks[0]);
	const task = await f.job.dispatchTask({
		...contract,
		effectiveContractHash: semanticContractHash(contract),
		replayKey: `stage-plan:${plan.id}:repair`,
	});
	await f.job.setTaskStatus(task.id, "succeeded");
	const repair = await f.job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		refs: [],
		content: { content: "repaired control" },
		currentEvidenceSetId: first.currentEvidenceSetId,
	});
	const artifact = await adopt(f, repair);
	f.job = (await ResearchJob.open(f.store, task.jobId))!;
	expect(f.job.state.evidence[first.id]).toBeUndefined();
	expect(f.job.state.discardedEvidence[first.id].cleanupStatus).toBe("completed");
	expect(f.job.state.canonical[artifact.id].status).toBe("active");
	return { f, first, repair, artifact, obligation, task, plan };
}

it("T07/T09 an adopted repair remains a legal downstream input after its failed comparison is pruned", async () => {
	const { f, repair, artifact } = await adoptedRepair();
	expect(evidenceHasCurrentPlanApproval(f.job, f.job.state.evidence[repair.id])).toBe(true);
	const downstream = await f.task("consume the reviewed repaired canonical", [artifact.id]);
	expect(downstream.inputArtifactRefs).toEqual([artifact.id]);
	expect(f.calls).toHaveLength(0);
});

it.each(["version", "receipt", "cleanup", "winner", "review", "chain", "retired", "missing-proof"] as const)(
	"T07/T09 pruned comparison cannot use invalid %s proof",
	async (change) => {
		const { f, first, repair, artifact, obligation, task } = await adoptedRepair();
		// Constructed persisted-state defense cases; no claim that normal APIs generate these corruptions.
		const state = f.job.state;
		if (change === "version") state.obligations[obligation.id].targetVersionHash = "foreign-version";
		if (change === "receipt") state.discardedEvidence[first.id].checksum = "foreign-checksum";
		if (change === "cleanup") delete state.cleanupIntents![`evidence:${first.id}`];
		if (change === "winner") state.canonical[artifact.id].checksum = "foreign-winner";
		if (change === "review") state.obligations[obligation.id].items![0].evidenceId = "foreign-winner";
		if (change === "chain") state.tasks[task.id].repairOfEvidenceId = "foreign-comparison";
		if (change === "retired") state.canonical[artifact.id].status = "stale";
		if (change === "missing-proof") delete state.discardedEvidence[first.id];
		await f.store.writeSnapshot(state);
		f.job = (await ResearchJob.open(f.store, task.jobId))!;
		const seq = f.job.state.eventSeq;
		expect(evidenceHasCurrentPlanApproval(f.job, f.job.state.evidence[repair.id]), change).toBe(false);
		await expect(f.task("refuse unproved comparison consumption", [artifact.id])).rejects.toThrow(/stale|current/);
		expect(f.job.state.eventSeq).toBe(seq);
		expect(f.calls).toHaveLength(0);
	},
);

it.each(["pi", "codex"] as const)(
	"T12 %s rejected approved plan loses status/reuse before old execution while candidate-pass control works",
	async (backend) => {
		for (const rejected of [false, true]) {
			const f = await scenario(backend);
			const { plan, evidence } = await approvedPlan(f);
			if (rejected) await f.job.decideEvidence(evidence.id, false, "reject_approved_plan");
			f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
			expect(planReviewStatus(f.job, plan.id)).toBe(rejected ? "stale" : "passed");
			await f.job.updateBudget({ maxTurns: 1 });
			await new ResearchSupervisor(f.job, f.store, f).tick();
			expect(f.calls.filter((call) => call.role === "worker")).toHaveLength(rejected ? 0 : 1);
			expect(f.job.state.budgetUsage!.turnsUsed).toBe(rejected ? 0 : 1);
			expect(Object.values(f.job.state.evidence).filter((entry) => entry.type !== "stage-plan")).toHaveLength(
				rejected ? 0 : 1,
			);
		}
	},
);

it.each(["pi", "codex"] as const)(
	"T07/T08/T12 %s rejects saved but unapplied planned output and retains applied completion after plan rejection",
	async (backend) => {
		for (const applied of [false, true]) {
			const f = await scenario(backend);
			const { plan, evidence: planEvidence } = await approvedPlan(f);
			const contract = buildEffectiveTaskContract(f.job, plan, plan.tasks[0]);
			const task = await f.job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:${plan.tasks[0].key}`,
			});
			await f.job.setTaskStatus(task.id, "running");
			const output = await f.worker.run(task, f.job);
			const original = applied ? await f.job.completeWorkerTask(task.id, output) : undefined;
			await f.job.decideEvidence(planEvidence.id, false, "reject_after_output");
			f.job = (await ResearchJob.open(f.store, task.jobId))!;
			const before = f.job.state.eventSeq;
			if (applied) {
				expect(await f.job.recoverWorkerTaskCompletion(task.id)).toEqual(original);
				expect(f.job.state.eventSeq).toBe(before);
			} else {
				await expect(f.job.completeWorkerTask(task.id, output)).rejects.toThrow(/stale|current|plan/);
				await f.job.recoverPendingOperations();
				expect(Object.values(f.job.state.evidence).filter((item) => item.type !== "stage-plan")).toHaveLength(0);
				await new ResearchSupervisor(f.job, f.store, f).tick();
				expect(f.job.state.tasks[task.id].status).toBe("blocked");
			}
			expect(f.calls.filter((call) => call.role === "worker")).toHaveLength(1);
		}
	},
);

it.each(["pi", "codex"] as const)(
	"T01 %s blocks a direct old ready/running task before any invalid model turn",
	async (backend) => {
		for (const status of ["ready", "running"] as const) {
			const f = await scenario(backend);
			const task = await f.task("old direct execution");
			if (status === "running") await f.job.setTaskStatus(task.id, status);
			await f.job.reopenStage(task.stageId, "new_revision", "reconsider original execution");
			f.job = (await ResearchJob.open(f.store, task.jobId))!;
			await new ResearchSupervisor(f.job, f.store, f).tick();
			expect(f.calls.filter((call) => call.role === "worker" && call.taskId === task.id)).toHaveLength(0);
			expect(f.job.state.tasks[task.id].status).toBe("blocked");
			expect(f.job.state.budgetUsage!.turnsUsed).toBe(0);
			expect(f.job.state.paused).toBe(false);
			await f.job.clearProviderBackoff();
			f.control.allowPlanning = true;
			await f.job.updateBudget({ maxTurns: 2 });
			await new ResearchSupervisor(f.job, f.store, f).tick();
			expect(Object.values(f.job.state.stagePlans)).toHaveLength(1);
			expect(f.calls.some((call) => call.role === "reviewer")).toBe(true);
		}
	},
);

it.each(["pi", "codex"] as const)("T02 %s still completes a current direct task", async (backend) => {
	const f = await scenario(backend);
	const task = await f.task("current direct task");
	await f.job.updateBudget({ maxTurns: 1 });
	await new ResearchSupervisor(f.job, f.store, f).tick();
	expect(f.calls).toEqual([{ role: "worker", taskId: task.id }]);
	expect(f.job.state.tasks[task.id].status).toBe("succeeded");
	expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
	expect(Object.values(f.job.state.evidence)).toHaveLength(1);
});

it("T03 first review and acceptance of a direct candidate from an old revision have zero side effects", async () => {
	const f = await scenario("pi");
	const evidence = await candidate(f, "old direct candidate");
	await f.job.reopenStage(evidence.stageId, "old_candidate_reopen", "reconsider candidate");
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	const seq = f.job.state.eventSeq;
	await expect(pass(f, evidence)).rejects.toThrow(/stale|current|revision/);
	await expect(f.job.decideEvidence(evidence.id, true, "old_candidate_accept")).rejects.toThrow(
		/stale|current|revision/,
	);
	expect(f.job.state.eventSeq).toBe(seq);
	expect(f.job.state.evidence[evidence.id].status).toBe("candidate");
	expect(Object.values(f.job.state.reviews)).toHaveLength(0);
});

it.each(["stage", "type"] as const)(
	"T05 raw direct evidence with wrong %s is refused without registration",
	async (field) => {
		const f = await scenario("pi");
		const task = await f.task("direct raw delivery");
		await f.job.setTaskStatus(task.id, "succeeded");
		const seq = f.job.state.eventSeq;
		await expect(
			f.job.recordEvidence({
				taskId: task.id,
				stageId: field === "stage" ? "literature" : task.stageId,
				type: field === "type" ? "literature" : task.requiredOutputType,
				content: { content: "wrong delivery" },
				refs: [],
			}),
		).rejects.toThrow(/contract|identity|stage|type/);
		expect(f.job.state.eventSeq).toBe(seq);
		expect(Object.values(f.job.state.evidence)).toHaveLength(0);
	},
);

it.each(["pi", "codex"] as const)(
	"T04/T11 %s excludes retired and rejected ordinary worker/reviewer inputs before budget",
	async (backend) => {
		for (const lifecycle of ["rejected", "retired"] as const)
			for (const role of ["worker", "reviewer"] as const) {
				const f = await scenario(backend);
				const source = await candidate(f, "ordinary source");
				await pass(f, source);
				const artifact = lifecycle === "retired" ? await adopt(f, source) : undefined;
				const refs = [artifact?.id ?? source.id];
				const task = role === "worker" ? await f.task("ordinary consumer", refs) : undefined;
				const consumer = role === "reviewer" ? await candidate(f, "completed ordinary consumer", refs) : undefined;
				if (lifecycle === "rejected") await f.job.decideEvidence(source.id, false);
				else await adopt(f, await candidate(f, "legitimate replacement"));
				f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
				await new ResearchSupervisor(f.job, f.store, f).tick();
				expect(f.calls.filter((call) => call.role !== "main-agent")).toHaveLength(0);
				expect(f.job.state.budgetUsage!.turnsUsed).toBe(0);
				expect(f.job.state.paused).toBe(false);
				if (task) expect(f.job.state.tasks[task.id].status).toBe("blocked");
				if (consumer)
					expect(Object.values(f.job.state.reviews).some((review) => review.evidenceId === consumer.id)).toBe(
						false,
					);
			}
	},
);

it.each(["pi", "codex"] as const)(
	"T03/T05 %s direct adapter cannot consume a different packet or evidence under a registered ID",
	async (backend) => {
		const f = await scenario(backend);
		const task = await f.task("immutable packet binding");
		await expect(f.worker.run({ ...task, objective: "different packet" }, f.job)).rejects.toThrow(
			/identity|binding|packet/,
		);
		const evidence = await candidate(f, "immutable evidence binding");
		await expect(
			f.reviewer.review({ ...evidence, content: { content: "foreign evidence bytes" } }, f.job),
		).rejects.toThrow(/identity|binding|evidence/);
		expect(f.calls).toHaveLength(0);
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(0);
	},
);

it.each(["pi", "codex"] as const)(
	"T10 %s N=2 search reviews survive capacity and JSONL reopen as independent approvals",
	async (backend) => {
		const f = await lifecycleScenario(backend, {
			...lifecycleDefinition,
			qualityPolicy: { ...lifecycleDefinition.qualityPolicy!, minPassingReviews: 2 },
			searchPolicy: {
				strategy: "diverse-candidates",
				minCandidates: 2,
				maxCandidates: 2,
				maxRounds: 1,
				criteria: lifecycleDefinition.acceptanceChecks,
			},
		});
		roots.push(f.root);
		const plan = await f.job.recordStagePlan({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "n2_search",
			jobId: f.job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "n2_search",
			mode: "search",
			tasks: ["first", "second"].map((key) => ({
				key,
				objective: key,
				hypothesis: key,
				deliveryKind: "stage",
				inputArtifactRefs: [],
				requiredOutputFields: ["content"],
				acceptanceChecks: lifecycleDefinition.acceptanceChecks,
				successCriteria: lifecycleDefinition.acceptanceChecks,
				failureSignals: lifecycleDefinition.failureSignals,
			})),
			rationale: "bounded independent comparison",
			sessionRef: "offline_manual_plan",
			createdAt: new Date().toISOString(),
		});
		const planEvidence = await preparePlanEvidence(f.job, plan);
		await pass(f, planEvidence);
		let evidence: Evidence | undefined;
		for (const planned of plan.tasks) {
			const contract = buildEffectiveTaskContract(f.job, plan, planned);
			const task = await f.job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:${planned.key}`,
			});
			await f.job.setTaskStatus(task.id, planned.key === "first" ? "succeeded" : "blocked");
			if (planned.key === "first")
				evidence = await f.job.recordEvidence({
					taskId: task.id,
					stageId: task.stageId,
					type: task.requiredOutputType,
					content: { content: "offline candidate" },
					refs: [],
				});
		}
		await f.job.updateBudget({ maxTurns: 2 });
		f.control.reviewerCapacityFailures = 1;
		await new ResearchSupervisor(f.job, f.store, f).tick();
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(0);
		expect(f.job.state.providerBackoff).toBeDefined();
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await f.job.clearProviderBackoff();
		await new ResearchSupervisor(f.job, f.store, f).tick();
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await new ResearchSupervisor(f.job, f.store, f).tick();
		const reviews = Object.values(f.job.state.reviews).filter((review) => review.evidenceId === evidence!.id);
		expect(reviews).toHaveLength(2);
		expect(new Set(reviews.map((review) => f.job.state.tasks[review.reviewerTaskId!].replayKey)).size).toBe(2);
		expect(f.calls.filter((call) => call.role === "reviewer")).toHaveLength(3);
		expect(f.calls.filter((call) => call.role === "main-agent" || call.role === "worker")).toHaveLength(0);
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(2);
		const batch = Object.values(f.job.state.searchBatches)[0];
		const candidateId = Object.values(batch.candidates).find(
			(candidate) => candidate.evidenceId === evidence!.id,
		)!.id;
		expect(
			f.job.searchQualification(batch.id).candidates.find((candidate) => candidate.candidateId === candidateId)
				?.eligible,
		).toBe(true);
		await new ResearchSupervisor(f.job, f.store, f).tick();
		expect(f.calls.filter((call) => call.role === "reviewer")).toHaveLength(3);
	},
);
