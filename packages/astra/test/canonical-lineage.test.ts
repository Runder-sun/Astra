import { mkdir, mkdtemp, readFile, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import {
	canonicalArtifactPath,
	reviewerManifestPath,
	reviewPacketPath,
	taskDir,
	writeReviewerOutputManifest,
} from "../src/contracts.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { inputHasCompletedAdoptionFromSnapshot, planReviewStatus, preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type {
	Evidence,
	MainAgentDecisionManifest,
	ReviewerOutputManifest,
	StagePlanManifest,
	TaskPacket,
} from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it.each(["candidate", "accepted", "incomplete"] as const)(
	"does not turn %s evidence into a completed input",
	async (state) => {
		const { job, store, artifact } = await setup();
		const child = await approved(job, "unfinished", "literature", [artifact.id]);
		const target = await job.completeWorkerTask(child.task.id, output(child.task));
		let ref = target.id;
		if (state !== "candidate") {
			await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
			await job.decideEvidence(target.id, true);
		}
		if (state === "incomplete") {
			const append = store.append.bind(store);
			const fault = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
				if (event.type === "canonical_artifact_status" && event.status === "active")
					throw new Error("Injected before adoption completion");
				return append(id, event);
			});
			await expect(job.adoptEvidence(target.id)).rejects.toThrow(/Injected/);
			fault.mockRestore();
			ref = Object.values(job.state.canonical).find((entry) => entry.evidenceId === target.id)!.id;
		}
		await job.recordUserGuidance("Replace unfinished downstream work");
		expect(inputHasCompletedAdoptionFromSnapshot(job.state, ref)).toBe(false);
		const next = await job.recordStagePlan(plan(job, "after-guidance", "literature", [ref]));
		await expect(preparePlanEvidence(job, next)).rejects.toThrow(/stale/);
	},
);

it.each(["canonical", "evidence"] as const)("rejects explicitly backtracked %s input", async (kind) => {
	const { job, artifact, evidence } = await setup();
	await job.applyRouteDecision({
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: "backtrack",
		jobId: job.state.frame.jobId,
		stageId: "literature",
		decisionType: "route",
		decisionRef: "backtrack",
		routeAction: "backtrack",
		targetStageId: "validation",
		rationale: "Invalidate adopted premises",
		sessionRef: "offline",
		createdAt: new Date().toISOString(),
	});
	const ref = kind === "canonical" ? artifact.id : evidence.id;
	expect(inputHasCompletedAdoptionFromSnapshot(job.state, ref)).toBe(false);
	await expect(job.recordStagePlan(plan(job, "new-validation", "validation", [ref]))).rejects.toThrow(
		/unknown input artifact/,
	);
});

it.each([
	"job",
	"type",
	"revision",
	"route",
	"content",
	"source-sha",
	"target-sha",
	"version",
	"contract",
	"review",
	"completion",
	"active-only",
] as const)("rejects corrupted completed input binding (%s)", async (fault) => {
	const { job, artifact, evidence, source } = await setup();
	const snapshot = job.state;
	// Explicit snapshot corruption tests the negative guard, not a claimed ordinary producer path.
	if (fault === "job") snapshot.tasks[source.task.id].jobId = "foreign-job";
	if (fault === "type") snapshot.canonical[artifact.id].type = "foreign-type";
	if (fault === "revision") snapshot.stages.validation.revision = (snapshot.stages.validation.revision ?? 1) + 1;
	if (fault === "route") snapshot.canonicalRoute.stageArtifactIds.validation = "other-artifact";
	if (fault === "content") snapshot.canonical[artifact.id].content = { content: "tampered" };
	if (fault === "source-sha") snapshot.canonical[artifact.id].sourceSha256 = "0".repeat(64);
	if (fault === "target-sha") snapshot.canonical[artifact.id].targetSha256 = "0".repeat(64);
	if (fault === "version") snapshot.evidence[evidence.id].versionHash = "0".repeat(64);
	if (fault === "contract") snapshot.tasks[source.task.id].effectiveContractHash = "0".repeat(64);
	if (fault === "review")
		for (const review of Object.values(snapshot.reviews))
			if (review.evidenceId === evidence.id) review.targetVersionHash = "0".repeat(64);
	if (fault === "completion" || fault === "active-only") delete snapshot.canonical[artifact.id].adoptionCompletedAt;
	expect(inputHasCompletedAdoptionFromSnapshot(snapshot, artifact.id)).toBe(false);
	expect(inputHasCompletedAdoptionFromSnapshot(snapshot, evidence.id)).toBe(false);
});

it.each(["directory", "symlink", "missing-hash", "invalid-hash", "logical-content"] as const)(
	"refuses malformed required canonical input without repairing it (%s)",
	async (fault) => {
		const { root, store, job, artifact } = await setup();
		const consumer = await approved(job, "consumer", "literature", [artifact.id]);
		const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
		const path = canonicalArtifactPath(root, job.state.frame.jobId, artifact.id);
		if (fault === "directory" || fault === "symlink") {
			await rm(path);
			if (fault === "directory") await mkdir(path);
			else {
				const external = join(root, "external.json");
				await writeFile(external, JSON.stringify(artifact.content));
				await symlink(external, path);
			}
		}
		let current = job;
		if (["missing-hash", "invalid-hash", "logical-content"].includes(fault)) {
			const snapshot = job.state;
			// Explicit metadata corruption probes validation only; no production failure is inferred.
			if (fault === "missing-hash") delete snapshot.canonical[artifact.id].targetSha256;
			if (fault === "invalid-hash") snapshot.canonical[artifact.id].targetSha256 = "not-a-hash";
			if (fault === "logical-content")
				snapshot.canonical[artifact.id].content = { content: "unauthorized logical content" };
			await store.writeSnapshot(snapshot);
			current = (await ResearchJob.open(store, job.state.frame.jobId))!;
		}
		const { adapter, run } = reviewer(root);
		await expect(adapter.review(target, current)).rejects.toThrow();
		expect(run).not.toHaveBeenCalled();
		expect(Object.values(current.state.reviews).filter((review) => review.evidenceId === target.id)).toHaveLength(0);
	},
);

function plan(job: ResearchJob, id: string, stageId: string, refs: string[]): StagePlanManifest {
	return {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id,
		jobId: job.state.frame.jobId,
		stageId,
		decisionRef: id,
		mode: "decompose",
		tasks: [
			{
				key: "delivery",
				objective: "Offline bounded delivery",
				inputArtifactRefs: refs,
				requiredOutputFields: job.definitions[stageId].requiredOutputFields,
				acceptanceChecks: ["verified"],
				failureSignals: ["missing"],
				successCriteria: ["verified"],
			},
		],
		rationale: "Offline fixture",
		sessionRef: "offline",
		createdAt: new Date().toISOString(),
	};
}

async function approved(job: ResearchJob, id: string, stage: string, refs: string[]) {
	const p = await job.recordStagePlan(plan(job, id, stage, refs));
	const pe = await preparePlanEvidence(job, p);
	await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [] }));
	const contract = buildEffectiveTaskContract(job, p, p.tasks[0]);
	const task = await job.dispatchTask({
		...contract,
		effectiveContractHash: semanticContractHash(contract),
		replayKey: `stage-plan:${id}:delivery`,
	});
	return { p, pe, task };
}

function output(task: TaskPacket) {
	return {
		artifactType: task.requiredOutputType,
		content: Object.fromEntries(task.requiredOutputFields.map((key) => [key, "Offline synthetic result"])),
		refs: [],
	};
}

async function setup(withPlan = true, moveDownstream = true, workspaceAlias = false) {
	const directory = await mkdtemp(join(tmpdir(), "astra-canonical-lineage-"));
	roots.push(directory);
	const root = workspaceAlias ? join(directory, "workspace-alias") : directory;
	if (workspaceAlias) await symlink(directory, root, "dir");
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Offline lineage",
		automation: "full",
	});
	const source = await approved(job, "source", "validation", []);
	if (!withPlan) {
		const { id: _id, attempt: _attempt, status: _status, version: _version, ...fields } = source.task;
		source.task = await job.dispatchTask({
			...fields,
			planId: undefined,
			effectiveContractHash: undefined,
			replayKey: "manual-source",
		});
	}
	await job.captureTaskVersion(source.task.id);
	const evidence = await job.completeWorkerTask(source.task.id, output(source.task));
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
	const artifact = await job.adoptEvidence(evidence.id);
	if (moveDownstream) await advance(job, "literature");
	return { root, store, job, source, evidence, artifact };
}

async function advance(job: ResearchJob, stage: string) {
	await job.applyRouteDecision({
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: `advance-${stage}`,
		jobId: job.state.frame.jobId,
		stageId: job.state.frame.activeStageId,
		decisionType: "route",
		decisionRef: `advance-${stage}`,
		routeAction: "advance",
		targetStageId: stage,
		rationale: "Offline adopted predecessor",
		sessionRef: "offline",
		createdAt: new Date().toISOString(),
	});
}

it.each(["canonical", "evidence", "clean"] as const)(
	"consumes adopted %s input through a new supervised delivery",
	async (kind) => {
		const { store, job, artifact, evidence, source } = await setup();
		const ref = kind === "evidence" ? evidence.id : artifact.id;
		const old = await approved(job, "old-consumer", "literature", [ref]);
		const oldBytes = JSON.stringify(old.pe.content);
		if (kind !== "clean") await job.recordUserGuidance("Narrow downstream literature while retaining validation");
		if (kind !== "clean") {
			expect(planReviewStatus(job, source.p.id)).toBe("stale");
			expect(planReviewStatus(job, old.p.id)).toBe("stale");
		}
		const calls = { worker: 0, review: 0 };
		const forbidden = async () => {
			throw new Error("unexpected search");
		};
		const decision = (fields: Partial<MainAgentDecisionManifest>): MainAgentDecisionManifest => ({
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: `control-${fields.decisionType}`,
			jobId: job.state.frame.jobId,
			stageId: job.state.frame.activeStageId,
			decisionType: "route",
			decisionRef: `control-${fields.decisionType}`,
			rationale: "Offline fixture",
			sessionRef: "offline",
			createdAt: new Date().toISOString(),
			...fields,
		});
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => {
					calls.worker++;
					expect(task.id).not.toBe(source.task.id);
					await job.captureTaskVersion(task.id);
					return output(task);
				},
			},
			reviewer: {
				review: async (target, current) => {
					calls.review++;
					return reviewFixture(current, { evidenceId: target.id, verdict: "pass", findings: [] });
				},
			},
			mainAgent: {
				planStage: async () => plan(job, "new-consumer", "literature", [ref]),
				decideSearch: forbidden,
				decideEvidence: async () => decision({ decisionType: "evidence", decision: "accept" }),
				decideAdoption: async () => decision({ decisionType: "adoption", adopt: true }),
				decideRoute: async () =>
					decision({ decisionType: "route", routeAction: "ask-user", question: "Stop after adopted delivery" }),
			},
		});
		for (let i = 0; i < 3 && !job.state.canonicalRoute.stageArtifactIds.literature; i++) await supervisor.tick();
		expect(calls.worker).toBe(1);
		expect(job.state.canonicalRoute.stageArtifactIds.literature).toBeTruthy();
		expect(JSON.stringify(job.state.evidence[old.pe.id].content)).toBe(oldBytes);
		const worker = Object.values(job.state.tasks).find(
			(task) => task.stageId === "literature" && task.status === "succeeded" && task.role === "worker",
		)!;
		expect(worker.version?.inputs).toContainEqual({ ref, checksum: evidence.versionHash });
	},
);

it("rejects a replaced completed canonical and its evidence alias", async () => {
	const { job, artifact, evidence } = await setup(true, false);
	const next = await approved(job, "replacement", "validation", []);
	const replacement = await job.completeWorkerTask(next.task.id, output(next.task));
	await job.recordReview(reviewFixture(job, { evidenceId: replacement.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(replacement.id, true);
	const current = await job.adoptEvidence(replacement.id, artifact.id);
	expect(current.replacementOf).toBe(artifact.id);
	expect(inputHasCompletedAdoptionFromSnapshot(job.state, artifact.id)).toBe(false);
	expect(inputHasCompletedAdoptionFromSnapshot(job.state, evidence.id)).toBe(false);
	expect(inputHasCompletedAdoptionFromSnapshot(job.state, current.id)).toBe(true);
});

it.each(["worker-identity", "worker-retired", "reviewer-identity", "reviewer-retired"] as const)(
	"does not recover an invalid durable consumer (%s)",
	async (fault) => {
		const { root, store, job, artifact } = await setup();
		await job.recordUserGuidance("Start a fresh consumer of the retained upstream");
		const consumer = await approved(job, "consumer", "literature", [artifact.id]);
		const append = store.append.bind(store);
		const worker = fault.startsWith("worker");
		let reviewerId: string | undefined;
		let targetId: string | undefined;
		if (worker) {
			const stop = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
				if (event.type === "evidence_recorded" && event.evidence.taskId === consumer.task.id)
					throw new Error("Injected registration tail");
				return append(id, event);
			});
			await expect(job.completeWorkerTask(consumer.task.id, output(consumer.task))).rejects.toThrow(/Injected/);
			stop.mockRestore();
			if (fault === "worker-identity") {
				const path = join(taskDir(root, job.state.frame.jobId, consumer.task.id), "evidence-completion.json");
				const prepared = JSON.parse(await readFile(path, "utf8")) as { task: { jobId: string } };
				prepared.task.jobId = "foreign-job";
				await rm(path);
				await writeFile(path, JSON.stringify(prepared));
			}
		} else {
			const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
			targetId = target.id;
			const { adapter } = reviewer(root);
			const result = await adapter.review(target, job);
			reviewerId = result.reviewerTaskId;
			if (fault === "reviewer-identity") {
				const path = reviewerManifestPath(root, job.state.frame.jobId, reviewerId!);
				const manifest = JSON.parse(await readFile(path, "utf8")) as ReviewerOutputManifest;
				manifest.jobId = "foreign-job";
				await writeFile(path, JSON.stringify(manifest));
			}
		}
		if (fault.endsWith("retired")) {
			await job.applyRouteDecision({
				schemaVersion: "astra.main_agent_decision_manifest.v1",
				manifestId: "invalidate",
				jobId: job.state.frame.jobId,
				stageId: "literature",
				decisionType: "route",
				decisionRef: "invalidate",
				routeAction: "backtrack",
				targetStageId: "validation",
				rationale: "Explicit invalidation",
				sessionRef: "offline",
				createdAt: new Date().toISOString(),
			});
		}
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		if (fault === "worker-identity")
			await expect(reopened.recoverPendingOperations()).rejects.toThrow(/integrity|identity/);
		else if (fault === "reviewer-identity")
			await expect(reopened.recoverReviewerTaskCompletions(targetId!, reviewerId)).rejects.toThrow(/identity/);
		else await reopened.recoverPendingOperations();
		if (worker)
			expect(
				Object.values(reopened.state.evidence).filter((entry) => entry.taskId === consumer.task.id),
			).toHaveLength(0);
		else
			expect(
				Object.values(reopened.state.reviews).filter((entry) => entry.reviewerTaskId === reviewerId),
			).toHaveLength(0);
	},
);

it("consumes a legitimate no-plan completed adoption after guidance", async () => {
	const { job, artifact, evidence } = await setup(false);
	await job.recordUserGuidance("Guide literature, keep manually adopted validation");
	expect(inputHasCompletedAdoptionFromSnapshot(job.state, artifact.id)).toBe(true);
	expect(inputHasCompletedAdoptionFromSnapshot(job.state, evidence.id)).toBe(true);
	const next = await approved(job, "next", "literature", [artifact.id, evidence.id]);
	await job.captureTaskVersion(next.task.id);
	const target = await job.completeWorkerTask(next.task.id, output(next.task));
	await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(target.id, true);
	expect((await job.adoptEvidence(target.id)).status).toBe("active");
});

it("consumes two completed upstream stages after repeated downstream guidance", async () => {
	const { job, artifact } = await setup();
	const literature = await approved(job, "literature", "literature", [artifact.id]);
	const evidence = await job.completeWorkerTask(literature.task.id, output(literature.task));
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
	const second = await job.adoptEvidence(evidence.id);
	await advance(job, "idea");
	await job.recordUserGuidance("First bounded idea refinement");
	await job.recordUserGuidance("Second bounded idea refinement");
	const consumer = await approved(job, "idea", "idea", [artifact.id, second.id]);
	const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
	await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(target.id, true);
	expect((await job.adoptEvidence(target.id)).status).toBe("active");
	expect(planReviewStatus(job, literature.p.id)).toBe("stale");
});

it.each(["worker", "reviewer"] as const)("recovers durable downstream %s without another model call", async (kind) => {
	const { root, store, job, artifact } = await setup();
	await job.recordUserGuidance("Refine downstream input after upstream adoption");
	const consumer = await approved(job, "consumer", "literature", [artifact.id]);
	await job.captureTaskVersion(consumer.task.id);
	const append = store.append.bind(store);
	let hit = false;
	const fault = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
		if (
			!hit &&
			event.type === "evidence_recorded" &&
			event.evidence.taskId === consumer.task.id &&
			kind === "worker"
		) {
			hit = true;
			throw new Error("Injected durable registration tail failure");
		}
		return append(id, event);
	});
	if (kind === "worker") {
		await expect(job.completeWorkerTask(consumer.task.id, output(consumer.task))).rejects.toThrow(/Injected/);
		fault.mockRestore();
		const prepared = JSON.parse(
			await readFile(
				join(taskDir(root, job.state.frame.jobId, consumer.task.id), "evidence-completion.json"),
				"utf8",
			),
		) as { evidence: Evidence };
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		const target = reopened.state.evidence[prepared.evidence.id];
		expect(target.taskId).toBe(consumer.task.id);
		expect(Object.values(reopened.state.evidence).filter((entry) => entry.taskId === consumer.task.id)).toHaveLength(
			1,
		);
		const before = reopened.state;
		await reopened.recoverPendingOperations();
		expect(reopened.state).toEqual(before);
	} else {
		const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
		const { adapter, run } = reviewer(root);
		const result = await adapter.review(target, job);
		const registration = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (event.type === "review_recorded") throw new Error("Injected review registration tail failure");
			return append(id, event);
		});
		await expect(job.recordReview({ ...result, evidenceId: target.id })).rejects.toThrow(/Injected/);
		registration.mockRestore();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.recoverReviewerTaskCompletions(target.id, result.reviewerTaskId);
		await reopened.decideEvidence(target.id, true);
		expect((await reopened.adoptEvidence(target.id)).status).toBe("active");
		expect(
			Object.values(reopened.state.reviews).filter((review) => review.reviewerTaskId === result.reviewerTaskId),
		).toHaveLength(1);
		expect(run).toHaveBeenCalledOnce();
	}
});

function reviewer(root: string) {
	const runner = new PiChildSessionRunner();
	const run = vi
		.spyOn(runner, "run")
		.mockImplementation(async (_cwd, jobId, taskId, _attempt, role, _prompt, env = {}) => {
			expect(role).toBe("reviewer");
			const refs = [`evidence:${env.ASTRA_EVIDENCE_ID}`];
			await writeReviewerOutputManifest(
				{
					schemaVersion: "astra.reviewer_output_manifest.v1",
					manifestId: `manifest-${taskId}`,
					jobId,
					taskId,
					evidenceId: env.ASTRA_EVIDENCE_ID!,
					verdict: "pass",
					score: 1,
					findings: [],
					verifiedRefs: refs,
					criteria: (JSON.parse(env.ASTRA_REVIEW_CRITERIA!) as string[]).map((criterion) => ({
						criterion,
						passed: true,
						score: 1,
						evidenceRefs: refs,
						rationale: "Explicit offline assessment",
					})),
					sessionRef: "offline",
					createdAt: new Date().toISOString(),
				},
				root,
			);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
		});
	return { adapter: new PiReviewerAdapter(runner), run };
}

function codexReviewer(root: string) {
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
	vi.stubEnv("ASTRA_CODEX_MODEL", "offline-fixture");
	vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "model-calls.jsonl"));
	const runner = new CodexAppServerRunner({
		executable: process.execPath,
		prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
	});
	return { adapter: new CodexResearchAdapters(runner), run: vi.spyOn(runner, "run") };
}

it.each(["changed", "missing", "clean"] as const)(
	"requires adopted canonical bytes before Pi review (%s)",
	async (fault) => {
		const { root, job, artifact } = await setup();
		const consumer = await approved(job, "consumer", "literature", [artifact.id]);
		const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
		const path = canonicalArtifactPath(root, job.state.frame.jobId, artifact.id);
		if (fault === "changed") await writeFile(path, "Unauthorized replacement bytes");
		if (fault === "missing") await rm(path);
		const { adapter, run } = reviewer(root);
		if (fault !== "clean") {
			await expect(adapter.review(target, job)).rejects.toThrow(/canonical.*(integrity|missing|version)/);
			expect(run).not.toHaveBeenCalled();
			expect(Object.values(job.state.reviews).filter((review) => review.evidenceId === target.id)).toHaveLength(0);
			if (fault === "changed") expect(await readFile(path, "utf8")).toBe("Unauthorized replacement bytes");
			return;
		}
		const result = await adapter.review(target, job);
		await job.recordReview({ ...result, evidenceId: target.id });
		await job.decideEvidence(target.id, true);
		expect(job.state.evidence[target.id].status).toBe("accepted");
		expect(run).toHaveBeenCalledOnce();
		expect(
			await readFile(
				join(taskDir(root, job.state.frame.jobId, result.reviewerTaskId!), "review-packet.json"),
				"utf8",
			),
		).toContain(artifact.targetSha256!);
	},
);

const bundleCases = (["pi", "codex"] as const).flatMap((backend) =>
	(["plan", "worker"] as const).flatMap((kind) =>
		(["clean", "changed", "missing"] as const).map((fault) => ({ backend, kind, fault })),
	),
);
it.each(bundleCases)(
	"$backend/$kind shares required canonical collection ($fault)",
	async ({ backend, kind, fault }) => {
		const { root, job, artifact } = await setup();
		const p = await job.recordStagePlan(plan(job, "consumer", "literature", [artifact.id]));
		let target = await preparePlanEvidence(job, p);
		if (kind === "worker") {
			await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
			const c = buildEffectiveTaskContract(job, p, p.tasks[0]);
			const task = await job.dispatchTask({
				...c,
				effectiveContractHash: semanticContractHash(c),
				replayKey: `stage-plan:${p.id}:delivery`,
			});
			target = await job.completeWorkerTask(task.id, output(task));
		}
		const path = canonicalArtifactPath(root, job.state.frame.jobId, artifact.id);
		if (fault === "changed") await writeFile(path, "Wrong canonical version");
		if (fault === "missing") await rm(path);
		const { adapter, run } = backend === "pi" ? reviewer(root) : codexReviewer(root);
		if (fault !== "clean") {
			await expect(adapter.review(target, job)).rejects.toThrow(/canonical.*(integrity|missing)/);
			expect(run).not.toHaveBeenCalled();
			return;
		}
		const result = await adapter.review(target, job);
		await job.recordReview({ ...result, evidenceId: target.id });
		if (kind === "worker") await job.decideEvidence(target.id, true);
		else expect(planReviewStatus(job, p.id)).toBe("passed");
		expect(run).toHaveBeenCalledOnce();
	},
);

it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		(["plan", "worker"] as const).flatMap((kind) =>
			(["clean", "canonical", "bundle", "missing", "committed"] as const).map((fault) => ({ backend, kind, fault })),
		),
	),
)(
	"$backend/$kind preserves original review files during durable recovery ($fault)",
	async ({ backend, kind, fault }) => {
		const { root, store, job, artifact } = await setup();
		const p = await job.recordStagePlan(plan(job, "consumer", "literature", [artifact.id]));
		let target = await preparePlanEvidence(job, p);
		if (kind === "worker") {
			await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
			const contract = buildEffectiveTaskContract(job, p, p.tasks[0]);
			const task = await job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${p.id}:delivery`,
			});
			target = await job.completeWorkerTask(task.id, output(task));
		}
		const { adapter, run } = backend === "pi" ? reviewer(root) : codexReviewer(root);
		const result = await adapter.review(target, job);
		const taskId = result.reviewerTaskId!;
		if (fault === "committed") await job.recordReview({ ...result, evidenceId: target.id });
		const packetPath = reviewPacketPath(root, job.state.frame.jobId, taskId);
		const manifestPath = reviewerManifestPath(root, job.state.frame.jobId, taskId);
		const packetBytes = await readFile(packetPath, "utf8");
		const manifestBytes = await readFile(manifestPath, "utf8");
		const canonical = canonicalArtifactPath(root, job.state.frame.jobId, artifact.id);
		if (fault === "canonical" || fault === "committed") await writeFile(canonical, "Changed after freezing");
		if (fault === "missing") await rm(canonical);
		if (fault === "bundle") {
			const copy = join(taskDir(root, job.state.frame.jobId, taskId), `canonical/${artifact.id}.json`);
			await rm(copy);
			await writeFile(copy, "Changed frozen copy");
		}
		if (!["clean", "committed"].includes(fault))
			await expect(job.recordReview({ ...result, evidenceId: target.id })).rejects.toThrow(/canonical|integrity/);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		if (fault === "clean" || fault === "committed") {
			await reopened.recoverReviewerTaskCompletions(target.id, taskId);
			expect(Object.values(reopened.state.reviews).filter((review) => review.evidenceId === target.id)).toHaveLength(
				1,
			);
			if (fault === "clean" && kind === "worker") {
				await reopened.decideEvidence(target.id, true);
				await reopened.adoptEvidence(target.id);
			}
			if (fault === "clean" && kind === "plan") expect(planReviewStatus(reopened, p.id)).toBe("passed");
		} else {
			await expect(reopened.recoverReviewerTaskCompletions(target.id, taskId)).rejects.toThrow(
				/canonical|integrity/,
			);
			expect(Object.values(reopened.state.reviews).filter((review) => review.evidenceId === target.id)).toHaveLength(
				0,
			);
		}
		expect(await readFile(packetPath, "utf8")).toBe(packetBytes);
		expect(await readFile(manifestPath, "utf8")).toBe(manifestBytes);
		expect(run).toHaveBeenCalledOnce();
	},
);

it("requires canonical materialization even through the adopted evidence alias", async () => {
	const { root, job, artifact, evidence } = await setup();
	await job.recordUserGuidance("Use the formally adopted upstream evidence alias");
	const consumer = await approved(job, "consumer", "literature", [evidence.id]);
	const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
	await rm(canonicalArtifactPath(root, job.state.frame.jobId, artifact.id));
	const { adapter, run } = reviewer(root);
	await expect(adapter.review(target, job)).rejects.toThrow(/canonical.*missing/);
	expect(run).not.toHaveBeenCalled();
});

const pathCases = (["pi", "codex"] as const).flatMap((backend) =>
	(["canonical", "evidence"] as const).flatMap((reference) =>
		(["canonical", "job"] as const).flatMap((parent) =>
			(["prepare", "recovery"] as const).map((point) => ({ backend, reference, parent, point })),
		),
	),
);
it.each(pathCases)(
	"$backend/$reference rejects redirected $parent directory at $point",
	async ({ backend, reference, parent, point }) => {
		const { root, store, job, artifact, evidence } = await setup();
		const consumer = await approved(job, "consumer", "literature", [
			reference === "canonical" ? artifact.id : evidence.id,
		]);
		const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
		const { adapter, run } = backend === "pi" ? reviewer(root) : codexReviewer(root);
		const result = point === "recovery" ? await adapter.review(target, job) : undefined;
		const packetPath = result ? reviewPacketPath(root, job.state.frame.jobId, result.reviewerTaskId!) : undefined;
		const manifestPath = result
			? reviewerManifestPath(root, job.state.frame.jobId, result.reviewerTaskId!)
			: undefined;
		const packetBytes = packetPath ? await readFile(packetPath) : undefined;
		const manifestBytes = manifestPath ? await readFile(manifestPath) : undefined;
		const sourcePath = canonicalArtifactPath(root, job.state.frame.jobId, artifact.id);
		const sourceBytes = await readFile(sourcePath);
		const original =
			parent === "canonical"
				? join(root, ".astra", "jobs", job.state.frame.jobId, "canonical")
				: join(root, ".astra", "jobs", job.state.frame.jobId);
		const relocated = join(root, `relocated-${parent}`);
		await rename(original, relocated);
		await symlink(relocated, original, "dir");
		if (point === "prepare") {
			await expect(adapter.review(target, job)).rejects.toThrow(/canonical.*(location|symbolic link)/);
			expect(run).not.toHaveBeenCalled();
		} else {
			await expect(job.recordReview({ ...result!, evidenceId: target.id })).rejects.toThrow(
				/canonical|symbolic link/,
			);
			const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
			await expect(reopened.recoverReviewerTaskCompletions(target.id, result!.reviewerTaskId)).rejects.toThrow(
				/canonical|symbolic link/,
			);
			expect(Object.values(reopened.state.reviews).filter((review) => review.evidenceId === target.id)).toHaveLength(
				0,
			);
			expect(await readFile(packetPath!)).toEqual(packetBytes);
			expect(await readFile(manifestPath!)).toEqual(manifestBytes);
			expect(run).toHaveBeenCalledOnce();
		}
		expect(await readFile(sourcePath)).toEqual(sourceBytes);
		expect(job.state.canonical[artifact.id].targetSha256).toBe(artifact.targetSha256);
		expect(job.state.evidence[target.id].status).toBe("candidate");
	},
);

it.each(["pi", "codex"] as const)("%s allows the workspace root's normal physical alias", async (backend) => {
	const { root, store, job, artifact } = await setup(true, true, true);
	const consumer = await approved(job, "consumer", "literature", [artifact.id]);
	const target = await job.completeWorkerTask(consumer.task.id, output(consumer.task));
	const { adapter, run } = backend === "pi" ? reviewer(root) : codexReviewer(root);
	const result = await adapter.review(target, job);
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	await reopened.recoverReviewerTaskCompletions(target.id, result.reviewerTaskId);
	await reopened.decideEvidence(target.id, true);
	expect(reopened.state.evidence[target.id].status).toBe("accepted");
	expect(run).toHaveBeenCalledOnce();
});
