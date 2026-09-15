import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { codexPlanSchema } from "../src/codex-schemas.ts";
import { planReviewStatus } from "../src/plan-review.ts";
import { researchMilestones } from "../src/progress.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import { prepareReviewEvidenceBundle } from "../src/task-workspace.ts";
import type { MainAgentDecisionManifest, PlannedTask, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
it("satisfies the Codex strict structured-output requirement for every planned task field", () => {
	const taskSchema = codexPlanSchema.properties.tasks.items;
	expect(taskSchema.required).toEqual(expect.arrayContaining(Object.keys(taskSchema.properties)));
});
afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it("passes a bounded literature contract through the Codex worker and independent reviewer", async () => {
	const { job } = await setup();
	const task = await job.dispatchTask({
		stageId: "literature",
		stageExecutionId: "literature",
		role: "worker",
		deliveryKind: "local",
		objective: "Identify one limitation in the supplied research question",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "literature:local",
		requiredOutputFields: ["limitation"],
		acceptanceChecks: ["limitation is explicit"],
		failureSignals: ["no limitation"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["limitation is explicit"],
	});
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
	const adapters = new CodexResearchAdapters(
		new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		}),
	);
	const output = await adapters.run(task, job);
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: output.artifactType,
		content: output.content,
		refs: output.refs,
	});
	expect((await adapters.review(evidence, job)).verdict).toBe("pass");
	const reviewer = Object.values(job.state.tasks).find((candidate) => candidate.role === "reviewer")!;
	const packet = JSON.parse(
		await readFile(
			join(task.scope.workspaceRoot, ".astra", "jobs", task.jobId, "tasks", reviewer.id, "review-packet.json"),
			"utf8",
		),
	);
	expect(packet.stageContract.requiredOutputFields).toEqual(["limitation"]);
	expect(packet.stageContract.outputArtifactType).toBe("literature:local");
});

async function setup() {
	const root = await mkdtemp(join(tmpdir(), "astra-local-flow-"));
	roots.push(root);
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "combine baseline and data checks",
		automation: "full",
	});
	return { job, store };
}

function local(key: string): PlannedTask {
	return {
		key,
		deliveryKind: "local",
		objective: `Inspect ${key}`,
		requiredOutputFields: [key],
		acceptanceChecks: [`${key} checked`],
		successCriteria: [`${key} checked`],
		failureSignals: [`${key} missing`],
		inputArtifactRefs: [],
	};
}

function plan(job: ResearchJob, id: string, tasks: PlannedTask[], obligationId?: string): StagePlanManifest {
	return {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id,
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: id,
		mode: obligationId ? "repair" : "decompose",
		tasks,
		rationale: "test decomposition",
		sessionRef: "fixture:main",
		createdAt: new Date().toISOString(),
		...(obligationId ? { obligationId } : {}),
	};
}

function decision(job: ResearchJob, fields: Partial<MainAgentDecisionManifest>): MainAgentDecisionManifest {
	return {
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: `decision-${job.state.eventSeq}`,
		jobId: job.state.frame.jobId,
		decisionType: "evidence",
		decisionRef: `decision-${job.state.eventSeq}`,
		rationale: "fixture decision",
		sessionRef: "fixture:main",
		createdAt: new Date().toISOString(),
		...fields,
	};
}

it("blocks workers on a failed plan, reviews a new revision and preserves the audit on restart", async () => {
	const { job, store } = await setup();
	let sequence = 0;
	const worker = vi.fn(async (task) => ({
		artifactType: task.requiredOutputType,
		content: { baseline: "checked" },
		refs: [],
	}));
	const supervisor = new ResearchSupervisor(job, store, {
		worker: { run: worker },
		reviewer: {
			review: async (evidence, current) =>
				reviewFixture(current, {
					evidenceId: evidence.id,
					verdict: evidence.type === "stage-plan" && sequence === 1 ? "fail" : "pass",
					findings:
						evidence.type === "stage-plan" && sequence === 1 ? ["The plan lacks a falsifiable comparison"] : [],
				}),
		},
		mainAgent: {
			planStage: async () => plan(job, `reviewed-plan-${++sequence}`, [local("baseline")]),
			decideEvidence: async (evidence) => decision(job, { evidenceId: evidence.id, decision: "accept" }),
			decideAdoption: async () => {
				throw new Error("local work cannot be adopted");
			},
			decideSearch: async () => {
				throw new Error("unexpected search");
			},
			decideRoute: async () => {
				throw new Error("local work cannot advance the stage");
			},
		},
	});
	await supervisor.tick();
	expect(worker).not.toHaveBeenCalled();
	expect(planReviewStatus(job, "reviewed-plan-1")).toBe("failed");
	expect(job.state.frame.openObligationIds).toHaveLength(0);
	await supervisor.tick();
	expect(worker).toHaveBeenCalledTimes(1);
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	expect(planReviewStatus(reopened, "reviewed-plan-2")).toBe("passed");
	const milestone = researchMilestones(reopened.state).find((item) => item.stageId === "validation")!;
	expect(milestone.plans.map((item) => item.status)).toEqual(["failed", "passed"]);
	expect(milestone.deliveries).toHaveLength(1);
	expect(milestone.artifactId).toBeUndefined();
	const source = Object.values(job.state.tasks).find((task) => task.role === "worker")!;
	await expect(
		job.dispatchTask({ ...source, id: "bypass", replayKey: "bypass", planId: "reviewed-plan-1" }),
	).rejects.toThrow(/approved plan/);
	await job.recordUserGuidance("Change the comparison protocol before further execution");
	expect(planReviewStatus(job, "reviewed-plan-2")).toBe("stale");
	expect(
		researchMilestones(job.state)
			.find((item) => item.stageId === "validation")
			?.plans.at(-1)?.status,
	).toBe("stale");
});

it("accepts complementary local work, then independently reviews one explicit synthesis", async () => {
	const { job, store } = await setup();
	const seenContracts: string[][] = [];
	const adopt = vi.fn(async (evidence) =>
		decision(job, { decisionType: "adoption", evidenceId: evidence.id, adopt: true }),
	);
	const supervisor = new ResearchSupervisor(job, store, {
		worker: {
			run: async (task) => ({
				artifactType: task.requiredOutputType,
				content: Object.fromEntries(task.requiredOutputFields.map((field) => [field, "checked"])),
				refs: ["fixture:worker"],
			}),
		},
		reviewer: {
			review: async (evidence, current) => {
				if (evidence.type !== "stage-plan")
					seenContracts.push(current.state.tasks[evidence.taskId].acceptanceChecks);
				return reviewFixture(current, { evidenceId: evidence.id, verdict: "pass", findings: [] });
			},
		},
		mainAgent: {
			planStage: async () => {
				const accepted = Object.values(job.state.evidence).filter((evidence) => evidence.status === "accepted");
				return accepted.length === 0
					? plan(job, "local-parts", [local("baseline"), local("data")])
					: plan(job, "synthesize", [
							{
								...local("combined"),
								deliveryKind: "synthesis",
								inputArtifactRefs: accepted.map((evidence) => evidence.id),
								requiredOutputFields: job.definitions.validation.requiredOutputFields,
							},
						]);
			},
			decideEvidence: async (evidence) => decision(job, { evidenceId: evidence.id, decision: "accept" }),
			decideAdoption: adopt,
			decideSearch: async () => {
				throw new Error("unexpected search");
			},
			decideRoute: async () =>
				decision(job, {
					decisionType: "route",
					stageId: "validation",
					routeAction: "advance",
					targetStageId: "literature",
				}),
		},
	});
	await supervisor.tick();
	expect(Object.values(job.state.evidence).filter((evidence) => evidence.status === "accepted")).toHaveLength(2);
	expect(adopt).not.toHaveBeenCalled();
	expect(job.state.canonicalRoute.stageArtifactIds.validation).toBeUndefined();
	expect(seenContracts).toEqual([["baseline checked"], ["data checked"]]);
	const locals = Object.values(job.state.evidence).filter((evidence) => evidence.type !== "stage-plan");
	const reopened = await ResearchJob.open(store, job.state.frame.jobId);
	expect(reopened?.unsynthesizedLocalEvidence("validation").map((evidence) => evidence.id)).toEqual(
		locals.map((evidence) => evidence.id),
	);
	const pendingTask = await job.dispatchTask({
		...job.state.tasks[locals[0].taskId],
		planId: undefined,
		id: "pending_local",
		replayKey: "pending_local",
	});
	await expect(
		job.recordStagePlan(
			plan(job, "premature-synthesis", [
				{
					...local("all"),
					deliveryKind: "synthesis",
					inputArtifactRefs: locals.map((evidence) => evidence.id),
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
				},
			]),
		),
	).rejects.toThrow(/unfinished local/);
	await job.setTaskStatus(pendingTask.id, "failed");
	await expect(job.adoptEvidence(locals[0].id)).rejects.toThrow(/local|synthesis/);
	await expect(
		job.recordStagePlan(
			plan(job, "omit-component", [
				{
					...local("combined"),
					deliveryKind: "synthesis",
					inputArtifactRefs: [locals[0].id],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
				},
			]),
		),
	).rejects.toThrow(/omits/);
	await supervisor.tick();
	expect(adopt).toHaveBeenCalledTimes(1);
	expect(seenContracts).toHaveLength(3);
	expect(job.state.frame.activeStageId).toBe("literature");
	expect(locals.every((evidence) => job.state.evidence[evidence.id].status === "accepted")).toBe(true);
	const synthesis = Object.values(job.state.evidence).find(
		(evidence) => job.state.tasks[evidence.taskId].deliveryKind === "synthesis",
	)!;
	const bundle = await prepareReviewEvidenceBundle(job.state.tasks[synthesis.taskId], synthesis, job);
	expect(bundle.map((file) => file.path)).toEqual(
		expect.arrayContaining(locals.map((evidence) => `input-evidence/${evidence.id}.json`)),
	);
});

it.each(["local", "synthesis"] as const)(
	"repairs a failed %s without dropping its contract or bypassing synthesis",
	async (failedKind) => {
		const { job, store } = await setup();
		let failed = false;
		let ordinal = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => ({
					artifactType: task.requiredOutputType,
					content: Object.fromEntries(task.requiredOutputFields.map((field) => [field, "checked"])),
					refs: [],
				}),
			},
			reviewer: {
				review: async (evidence, current) => {
					const task = current.state.tasks[evidence.taskId];
					if (task.deliveryKind === failedKind && !failed) {
						failed = true;
						return reviewFixture(current, {
							evidenceId: evidence.id,
							verdict: "fail",
							findings: ["Verify the missing control comparison"],
						});
					}
					if (task.repairOfEvidenceId && evidence.type !== "stage-plan") {
						expect(task.acceptanceChecks).toContain("Verify the missing control comparison");
						expect(task.requiredOutputFields).toContain(failedKind === "local" ? "baseline" : "researchQuestion");
					}
					return reviewFixture(current, { evidenceId: evidence.id, verdict: "pass", findings: [] });
				},
			},
			mainAgent: {
				planStage: async (_current, obligation) => {
					const pending = job.unsynthesizedLocalEvidence("validation");
					const prior = obligation
						? job.state.tasks[job.state.evidence[job.state.reviews[obligation.sourceReviewId].evidenceId].taskId]
						: undefined;
					const kind = prior?.deliveryKind ?? (pending.length ? "synthesis" : "local");
					return plan(
						job,
						`round-${++ordinal}`,
						[
							{
								...local("baseline"),
								deliveryKind: kind,
								inputArtifactRefs: kind === "synthesis" ? pending.map((evidence) => evidence.id) : [],
								requiredOutputFields:
									kind === "local" ? ["baseline"] : job.definitions.validation.requiredOutputFields,
							},
						],
						obligation?.id,
					);
				},
				decideEvidence: async (evidence) => decision(job, { evidenceId: evidence.id, decision: "accept" }),
				decideAdoption: async (evidence) =>
					decision(job, { decisionType: "adoption", evidenceId: evidence.id, adopt: true }),
				decideSearch: async () => {
					throw new Error("unexpected search");
				},
				decideRoute: async (_current, obligation) =>
					decision(job, {
						decisionType: "route",
						stageId: "validation",
						routeAction: obligation ? "continue" : "advance",
						targetStageId: obligation ? undefined : "literature",
					}),
			},
		});
		for (let round = 0; round < 3; round++) await supervisor.tick();
		expect(failed).toBe(true);
		expect(job.state.frame.openObligationIds, JSON.stringify(job.state.frame)).toHaveLength(0);
		expect(job.state.frame.activeStageId).toBe("literature");
	},
);

it("requires full fields for stage delivery but allows local fields", async () => {
	const { job } = await setup();
	await expect(job.recordStagePlan(plan(job, "local", [local("baseline")]))).resolves.toBeDefined();
	await expect(
		job.recordStagePlan(plan(job, "stage", [{ ...local("baseline"), deliveryKind: "stage" }])),
	).rejects.toThrow(/fields/);
	await expect(
		job.recordStagePlan(
			plan(job, "invalid-synthesis", [
				{
					...local("all"),
					deliveryKind: "synthesis",
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
				},
			]),
		),
	).rejects.toThrow(/synthesis/);
});
