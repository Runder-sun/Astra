import { expect, it, vi } from "vitest";
import { stagePlanManifestPath, writeStagePlanManifest } from "../src/contracts.ts";
import {
	buildEffectiveTaskContract,
	planGenerationBasisHash,
	semanticContractHash,
} from "../src/effective-contract.ts";
import { evidenceHasCurrentPlanApproval, preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { Evidence, MainAgentDecisionManifest, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

function plan(job: ResearchJob, id: string, obligationId?: string, inputRefs: string[] = []): StagePlanManifest {
	return {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id,
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: id,
		mode: obligationId ? "repair" : "decompose",
		obligationId,
		tasks: [
			{
				key: "worker",
				objective: "Deliver bounded result",
				deliveryKind: "stage",
				inputArtifactRefs: inputRefs,
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: ["result verified"],
				failureSignals: [],
				successCriteria: [],
			},
		],
		rationale: "bounded",
		sessionRef: "fixture",
		createdAt: new Date().toISOString(),
	};
}
async function candidate(job: ResearchJob, value: StagePlanManifest) {
	await job.recordStagePlan(value);
	const pe = await preparePlanEvidence(job, value);
	await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [] }));
	const contract = buildEffectiveTaskContract(job, value, value.tasks[0]);
	const task = await job.dispatchTask({
		...contract,
		effectiveContractHash: semanticContractHash(contract),
		replayKey: `stage-plan:${value.id}:worker`,
	});
	await job.setTaskStatus(task.id, "succeeded");
	const obligation = value.obligationId ? job.state.obligations[value.obligationId] : undefined;
	const failed = obligation ? job.state.evidence[job.state.reviews[obligation.sourceReviewId].evidenceId] : undefined;
	return job.recordEvidence({
		taskId: task.id,
		stageId: "validation",
		type: task.requiredOutputType,
		refs: [],
		content: Object.fromEntries(task.requiredOutputFields.map((f) => [f, "verified"])),
		currentEvidenceSetId: failed?.currentEvidenceSetId,
	});
}
const decision = (job: ResearchJob, fields: Partial<MainAgentDecisionManifest>): MainAgentDecisionManifest =>
	({
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: "decision",
		jobId: job.state.frame.jobId,
		decisionRef: "decision",
		rationale: "verified",
		sessionRef: "fixture",
		createdAt: new Date().toISOString(),
		...fields,
	}) as MainAgentDecisionManifest;

async function savePlanDelivery(job: ResearchJob, value: StagePlanManifest) {
	await job.registerMainAgentCall({
		id: value.decisionRef,
		type: "plan",
		planId: value.id,
		mode: value.mode,
		manifestRef: stagePlanManifestPath(job.state.frame.permissions.workspaceRoot, job.state.frame.jobId, value.id),
	});
	await writeStagePlanManifest(value, job.state.frame.permissions.workspaceRoot);
}

it("reuses the saved plan after pausing during planning", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		objective: "pause during planning",
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		automation: "full",
	});
	let calls = 0;
	const mainAgent = {
		planStage: vi.fn(async () => {
			calls++;
			const value = plan(job, `plan_${calls}`);
			await savePlanDelivery(job, value);
			await job.pause("operator pause while planner returns");
			return value;
		}),
		decideEvidence: vi.fn(),
		decideAdoption: vi.fn(),
		decideSearch: vi.fn(),
		decideRoute: vi.fn(),
	};
	const supervisor = new ResearchSupervisor(job, store, {
		mainAgent,
		worker: { run: vi.fn() },
		reviewer: {
			review: vi.fn(async (evidence: Evidence) => {
				const review = reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: "pass",
					findings: [],
					blocking: false,
				});
				await job.recordReview(review);
				await job.pause("pause after independent plan review");
				return review;
			}),
		},
	});
	await supervisor.tick();
	expect(Object.keys(job.state.stagePlans)).toEqual([]);
	expect(job.state.mainAgentCalls?.plan_1.deliveryHash).toBeTruthy();
	expect(Object.values(job.state.tasks)).toHaveLength(0);
	await job.resume();
	await supervisor.tick();
	expect(Object.keys(job.state.stagePlans)).toEqual(["plan_1"]);
	expect(calls).toBe(1);
	expect(Object.values(job.state.reviews)).toHaveLength(1);
	expect(job.status().budget.turnsUsed).toBe(2);
});

it("keeps a twice-repaired accepted candidate eligible for adoption", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		objective: "two repair rounds",
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		automation: "full",
	});
	const first = await candidate(job, plan(job, "first"));
	await job.recordReview(reviewFixture(job, { evidenceId: first.id, verdict: "fail", findings: ["missing control"] }));
	const obligation1 = Object.values(job.state.obligations).find((o) => o.evidenceId === first.id)!;
	const repair1 = await candidate(job, plan(job, "repair1", obligation1.id));
	await job.recordReview(
		reviewFixture(job, { evidenceId: repair1.id, verdict: "fail", findings: ["missing second control"] }),
	);
	const obligation2 = Object.values(job.state.obligations).find((o) => o.evidenceId === repair1.id)!;
	const repair2 = await candidate(job, plan(job, "repair2", obligation2.id));
	await job.recordReview(reviewFixture(job, { evidenceId: repair2.id, verdict: "pass", findings: [] }));
	expect(evidenceHasCurrentPlanApproval(job, repair2)).toBe(true);
	await job.decideEvidence(repair2.id, true);
	expect([first, repair1, repair2].every((e) => job.state.evidence[e.id].versionHash === e.versionHash)).toBe(true);
	expect(job.state.frame.openObligationIds).toEqual([]);
	await job.pause("pause after acceptance");
	await job.reload();
	await job.resume();
	const mainAgent = {
		planStage: vi.fn(async () => {
			await job.pause("unexpected new plan");
			return plan(job, "unexpected");
		}),
		decideEvidence: vi.fn(),
		decideAdoption: vi.fn(async (e: Evidence) =>
			decision(job, { decisionType: "adoption", evidenceId: e.id, adopt: true }),
		),
		decideSearch: vi.fn(),
		decideRoute: vi.fn(),
	};
	await new ResearchSupervisor(job, store, {
		mainAgent,
		worker: { run: vi.fn() },
		reviewer: { review: vi.fn() },
	}).tick();
	expect(mainAgent.decideAdoption).toHaveBeenCalledOnce();
	expect(mainAgent.planStage).not.toHaveBeenCalled();
});

it("control: one accepted repair keeps approval across same-byte pause and resume", async () => {
	const job = await ResearchJob.create(new MemoryAstraStore(), {
		objective: "single repair control",
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		automation: "full",
	});
	const first = await candidate(job, plan(job, "single-first"));
	await job.recordReview(reviewFixture(job, { evidenceId: first.id, verdict: "fail", findings: ["missing control"] }));
	const obligation = Object.values(job.state.obligations).find((o) => o.evidenceId === first.id)!;
	const repair = await candidate(job, plan(job, "single-repair", obligation.id));
	await job.recordReview(reviewFixture(job, { evidenceId: repair.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(repair.id, true);
	await job.pause("same-byte control");
	await job.reload();
	await job.resume();
	expect([first, repair].every((e) => job.state.evidence[e.id].versionHash === e.versionHash)).toBe(true);
	expect(evidenceHasCurrentPlanApproval(job, repair)).toBe(true);
});

it("control: real canonical input retirement invalidates the approved candidate", async () => {
	const job = await ResearchJob.create(new MemoryAstraStore(), {
		objective: "retired input control",
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		automation: "full",
	});
	const seed = await candidate(job, plan(job, "seed"));
	await job.recordReview(reviewFixture(job, { evidenceId: seed.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(seed.id, true);
	const canonical = await job.adoptEvidence(seed.id);
	const consumer = await candidate(job, plan(job, "consumer", undefined, [canonical.id]));
	expect(evidenceHasCurrentPlanApproval(job, consumer)).toBe(true);
	const replacement = await candidate(job, plan(job, "replacement"));
	await job.recordReview(reviewFixture(job, { evidenceId: replacement.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(replacement.id, true);
	await job.adoptEvidence(replacement.id, canonical.id);
	expect(job.state.retiredArtifacts[canonical.id]).toBeDefined();
	expect(evidenceHasCurrentPlanApproval(job, consumer)).toBe(false);
});

it("keeps long repair chains approved while rejecting changed ancestor identity and versions", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		objective: "repair ancestry",
	});
	let current = await candidate(job, plan(job, "ancestor_0"));
	const ancestors = [current];
	for (let round = 1; round <= 4; round++) {
		await job.recordReview(
			reviewFixture(job, { evidenceId: current.id, verdict: "fail", findings: [`missing ${round}`] }),
		);
		const obligation = Object.values(job.state.obligations).find((item) => item.evidenceId === current.id)!;
		current = await candidate(job, plan(job, `ancestor_${round}`, obligation.id));
		ancestors.push(current);
	}
	await job.recordReview(reviewFixture(job, { evidenceId: current.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(current.id, true);
	expect(evidenceHasCurrentPlanApproval(job, current)).toBe(true);
	const original = job.state;
	const frozenHashes = Object.values(original.evidence)
		.filter((value) => value.type === "stage-plan")
		.map((value) => value.versionHash);
	for (const change of ["version", "identity", "wrong-superseder", "broken-chain", "retired-input"] as const) {
		const changed = structuredClone(original);
		const first = changed.evidence[ancestors[0].id];
		if (change === "version") first.versionHash = "different-bytes";
		if (change === "identity") first.id = "different-evidence";
		if (change === "wrong-superseder") first.supersededByTaskId = "other-winner";
		if (change === "broken-chain") delete changed.tasks[ancestors[1].taskId].repairOfEvidenceId;
		if (change === "retired-input")
			changed.retiredArtifacts[first.id] = {
				artifactId: first.id,
				evidenceId: first.id,
				taskId: first.taskId,
				checksum: first.checksum,
				type: first.type,
				reviewIds: [],
				retiredAt: "now",
			};
		await store.writeSnapshot(changed);
		const reopened = (await ResearchJob.open(store, original.frame.jobId))!;
		expect(evidenceHasCurrentPlanApproval(reopened, current), change).toBe(false);
		expect(
			Object.values(reopened.state.evidence)
				.filter((value) => value.type === "stage-plan")
				.map((value) => value.versionHash),
		).toEqual(frozenHashes);
	}
});

it("does not exempt a same-lineage input outside the declared repair ancestry", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		objective: "repair sibling",
	});
	const first = await candidate(job, plan(job, "sibling_first"));
	const sibling = await candidate(job, plan(job, "sibling_unrelated"));
	await job.recordReview(reviewFixture(job, { evidenceId: first.id, verdict: "fail", findings: ["missing control"] }));
	const obligation = Object.values(job.state.obligations).find((item) => item.evidenceId === first.id)!;
	const before = job.state;
	before.evidence[sibling.id].currentEvidenceSetId = first.currentEvidenceSetId;
	await store.writeSnapshot(before);
	await job.reload();
	const repair = await candidate(job, plan(job, "sibling_repair", obligation.id, [sibling.id]));
	await job.recordReview(reviewFixture(job, { evidenceId: repair.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(repair.id, true);
	expect(job.state.evidence[sibling.id].supersededByTaskId).toBe(repair.taskId);
	expect(evidenceHasCurrentPlanApproval(job, repair)).toBe(false);
});

it("restores an unfrozen search plan without duplicating its batch or planning turn", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		objective: "search resume",
		automation: "full",
	});
	const mainAgent = {
		planStage: vi.fn(async () => {
			const value = plan(job, "paused_search");
			value.mode = "search";
			value.tasks = [value.tasks[0], { ...value.tasks[0], key: "second", objective: "alternative result" }];
			await savePlanDelivery(job, value);
			await job.pause("pause before search freeze");
			return value;
		}),
		decideEvidence: vi.fn(),
		decideAdoption: vi.fn(),
		decideSearch: vi.fn(),
		decideRoute: vi.fn(),
	};
	const reviewer = {
		review: vi.fn(async (evidence: Evidence) => {
			await job.pause("pause after plan review");
			return reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] });
		}),
	};
	const worker = { run: vi.fn() };
	const supervisor = new ResearchSupervisor(job, store, { mainAgent, reviewer, worker });
	await supervisor.tick();
	expect(job.state.stagePlans.paused_search).toBeUndefined();
	expect(job.state.mainAgentCalls?.paused_search.deliveryHash).toBeTruthy();
	expect(Object.values(job.state.tasks)).toHaveLength(0);
	await job.reload();
	await job.resume();
	await supervisor.tick();
	expect(job.state.stagePlans.paused_search.generationBasisHash).toBeTruthy();
	expect(mainAgent.planStage).toHaveBeenCalledOnce();
	expect(reviewer.review).toHaveBeenCalledOnce();
	expect(worker.run).not.toHaveBeenCalled();
	expect(Object.values(job.state.searchBatches)).toHaveLength(1);
	expect(Object.values(job.state.searchBatches)[0].round).toBe(1);
	expect(job.status().budget.turnsUsed).toBe(2);
	const events = await store.readEvents(job.state.frame.jobId);
	const planIndex = events.findIndex((stored) => stored.event.type === "stage_plan_recorded");
	expect(events[planIndex].event).toMatchObject({ type: "stage_plan_recorded", search: { batch: { round: 1 } } });
});

it.each(["during-planning", "after-planning", "stage-revision", "input-version"] as const)(
	"replans when the generation basis changes %s",
	async (change) => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			workspaceRoot: "/tmp/astra-efficiency-no-files",
			objective: "basis mutation",
			automation: "full",
		});
		const source = await candidate(job, plan(job, "basis_source"));
		// A rejected source has already been decided; it does not cause unrelated promotion work.
		await job.decideEvidence(source.id, false);
		const mainAgent = {
			planStage: vi.fn(async () => {
				const value = plan(job, `basis_plan_${mainAgent.planStage.mock.calls.length}`, undefined, [source.id]);
				if (change === "during-planning" && mainAgent.planStage.mock.calls.length === 1)
					await job.recordUserGuidance("changed while planning");
				await job.pause("pause planner return");
				return value;
			}),
			decideEvidence: vi.fn(),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn(),
		};
		const supervisor = new ResearchSupervisor(job, store, {
			mainAgent,
			reviewer: { review: vi.fn() },
			worker: { run: vi.fn() },
		});
		await supervisor.tick();
		if (change === "after-planning") await job.recordUserGuidance("changed after saving");
		if (change === "stage-revision" || change === "input-version") {
			const changed = job.state;
			if (change === "stage-revision") changed.stages.validation.revision = 2;
			else changed.evidence[source.id].versionHash = "changed-input";
			await store.writeSnapshot(changed);
			await job.reload();
		}
		await job.resume();
		await supervisor.tick();
		expect(mainAgent.planStage).toHaveBeenCalledTimes(2);
		expect(Object.values(job.state.tasks).filter((task) => task.role === "main-agent")).toHaveLength(1);
	},
);

it("ignores a model-supplied basis and never replaces an already saved basis", async () => {
	const job = await ResearchJob.create(new MemoryAstraStore(), {
		workspaceRoot: "/tmp/astra-efficiency-no-files",
		objective: "trusted plan basis",
	});
	const value = plan(job, "trusted_basis");
	value.generationBasisHash = "model-forgery";
	const snapshot = job.state;
	const saved = await job.recordStagePlan(value, snapshot);
	expect(saved.generationBasisHash).toBe(planGenerationBasisHash(snapshot, job.definitions, value));
	await job.recordUserGuidance("new basis");
	const rerecorded = await job.recordStagePlan({ ...saved, generationBasisHash: "overwrite" }, job.state);
	expect(rerecorded.generationBasisHash).toBe(saved.generationBasisHash);
	const legacy = await job.recordStagePlan({ ...value, id: "legacy_basis", generationBasisHash: "forgery" });
	expect(legacy.generationBasisHash).toBeUndefined();
});
