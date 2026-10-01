import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import {
	evidenceHasCurrentPlanApproval,
	evidenceHasCurrentPlanApprovalFromSnapshot,
	hasFrozenPlanContract,
	hasFrozenPlanContractFromSnapshot,
	planReviewStatus,
	preparePlanEvidence,
} from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { Evidence, SearchBatch, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
interface Decisions {
	pendingEvidence(stageId: string): Evidence[];
	reusablePlan(stageId: string): StagePlanManifest | undefined;
	activeSearch(stageId: string): SearchBatch | undefined;
}
async function setup() {
	const root = await mkdtemp(join(tmpdir(), "astra-snapshot-decisions-"));
	roots.push(root);
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "snapshot decisions",
		automation: "full",
	});
	const plans: StagePlanManifest[] = [];
	const evidenceIds: string[][] = [];
	const planEvidenceIds: string[] = [];
	for (let index = 0; index < 4; index++) {
		const definition = job.definitions.validation;
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: `plan-${index}`,
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: `decision-${index}`,
			mode: "search",
			tasks: ["a", "b"].map((key) => ({
				key,
				objective: key,
				hypothesis: `${index}-${key}`,
				inputArtifactRefs: [],
				requiredOutputFields: definition.requiredOutputFields,
				acceptanceChecks: [],
				failureSignals: [],
				successCriteria: [],
			})),
			rationale: "offline fixture",
			sessionRef: "fixture",
			createdAt: `2026-10-01T00:00:0${index}.000Z`,
		};
		await job.recordStagePlan(plan);
		plans.push(plan);
		const pe = await preparePlanEvidence(job, plan);
		planEvidenceIds.push(pe.id);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [] }));
		const ids: string[] = [];
		if (index !== 1) {
			for (const planned of plan.tasks) {
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
					content: { verified: true },
					refs: [],
				});
				await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
				await job.decideEvidence(evidence.id, true);
				ids.push(evidence.id);
			}
		}
		evidenceIds.push(ids);
	}
	const snapshot = job.state;
	snapshot.tasks[snapshot.evidence[planEvidenceIds[2]].taskId].stageRevision = 0;
	Reflect.deleteProperty(snapshot.evidence[planEvidenceIds[3]].content as object, "effectiveContracts");
	await store.writeSnapshot(snapshot);
	await job.reload();
	const supervisor = new ResearchSupervisor(job, store, {
		worker: { run: vi.fn() },
		reviewer: { review: vi.fn() },
		mainAgent: {
			planStage: vi.fn(),
			decideEvidence: vi.fn(),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn(),
		},
	});
	return { job, store, plans, evidenceIds, decisions: supervisor as unknown as Decisions };
}

it.each(["pendingEvidence", "reusablePlan", "activeSearch"] as const)(
	"uses one fresh snapshot for multiple eligible rows in %s",
	async (method) => {
		const { job, plans, evidenceIds, decisions } = await setup();
		expect(planReviewStatus(job, plans[0].id)).toBe("passed");
		expect(planReviewStatus(job, plans[2].id)).toBe("stale");
		expect(hasFrozenPlanContract(job, plans[3].id)).toBe(false);
		const read = vi.spyOn(job, "state", "get");
		const result = decisions[method]("validation");
		const copies = read.mock.calls.length;
		if (method === "pendingEvidence")
			expect((result as Evidence[]).map((evidence) => evidence.id)).toEqual(evidenceIds[0]);
		else if (method === "reusablePlan") expect((result as StagePlanManifest).id).toBe(plans[1].id);
		else expect((result as SearchBatch).planId).toBe(plans[1].id);
		expect(copies).toBe(1);
	},
);

it("keeps approval and frozen-contract gates for passed, stale and unfrozen plans", async () => {
	const { job, plans, evidenceIds } = await setup();
	expect(evidenceHasCurrentPlanApproval(job, job.state.evidence[evidenceIds[0][0]])).toBe(true);
	expect(evidenceHasCurrentPlanApproval(job, job.state.evidence[evidenceIds[2][0]])).toBe(false);
	expect(evidenceHasCurrentPlanApproval(job, job.state.evidence[evidenceIds[3][0]])).toBe(false);
	expect(hasFrozenPlanContract(job, plans[0].id)).toBe(true);
	expect(hasFrozenPlanContract(job, plans[2].id)).toBe(true);
	expect(hasFrozenPlanContract(job, plans[3].id)).toBe(false);
	const snapshot = job.state;
	const before = structuredClone(snapshot);
	for (const index of [0, 2, 3]) {
		const evidence = snapshot.evidence[evidenceIds[index][0]];
		expect(evidenceHasCurrentPlanApprovalFromSnapshot(snapshot, evidence)).toBe(
			evidenceHasCurrentPlanApproval(job, evidence),
		);
		expect(hasFrozenPlanContractFromSnapshot(snapshot, plans[index].id)).toBe(
			hasFrozenPlanContract(job, plans[index].id),
		);
	}
	expect(snapshot).toEqual(before);
});

it("isolates hostile snapshot and result mutations while observing a later committed change", async () => {
	const { job, plans, evidenceIds, decisions } = await setup();
	const snapshot = job.state;
	snapshot.stagePlans[plans[1].id].tasks[0].objective = "tampered";
	snapshot.frame.status = "completed";
	expect(job.state.stagePlans[plans[1].id].tasks[0].objective).toBe("a");
	expect(job.state.frame.status).toBe("running");
	const pending = decisions.pendingEvidence("validation");
	pending[0].status = "rejected";
	decisions.reusablePlan("validation")!.tasks[0].objective = "tampered";
	decisions.activeSearch("validation")!.status = "selected";
	expect(decisions.pendingEvidence("validation").map((evidence) => evidence.id)).toEqual(evidenceIds[0]);
	expect(decisions.reusablePlan("validation")!.tasks[0].objective).toBe("a");
	expect(decisions.activeSearch("validation")!.status).toBe("planning");
	await job.recordUserGuidance("Change the approach");
	expect(decisions.pendingEvidence("validation")).toEqual([]);
	expect(decisions.reusablePlan("validation")).toBeUndefined();
	expect(decisions.activeSearch("validation")).toBeUndefined();
});

it("reuses an unfrozen host-recorded basis until a new committed instruction makes it stale", async () => {
	const { job, plans, decisions } = await setup();
	const saved = await job.recordStagePlan(
		{
			...plans[1],
			id: "host-saved",
			mode: "decompose",
			tasks: plans[1].tasks.slice(0, 1),
			createdAt: "2026-10-01T00:00:05.000Z",
		},
		job.state,
	);
	expect(hasFrozenPlanContract(job, saved.id)).toBe(false);
	expect(decisions.reusablePlan("validation")?.id).toBe(saved.id);
	await job.recordUserGuidance("Replace the saved strategy");
	expect(decisions.reusablePlan("validation")).toBeUndefined();
});
