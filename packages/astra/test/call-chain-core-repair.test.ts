import { access, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext, ToolDefinition } from "@earendil-works/pi-coding-agent";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanupTaskFiles } from "../src/cleanup-files.ts";
import { reviewerManifestPath, writeReviewerOutputManifest } from "../src/contracts.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { createAstraExtension } from "../src/extension.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import { prepareTaskWorkspace } from "../src/task-workspace.ts";
import type { Evidence, StageDefinition, StagePlanManifest, TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
const stage: StageDefinition = {
	id: "validation",
	label: "Validation",
	suggestedInputArtifactTypes: [],
	outputArtifactType: "validation",
	requiredOutputFields: ["content"],
	acceptanceChecks: ["verify content"],
	failureSignals: ["missing content"],
	workerTaskFamily: "validation",
	workerTools: ["read"],
	workspaceWrite: false,
	minSourceRefs: 0,
	gate: "main-agent",
};
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function setup(options: { maxTasks?: number; maxTurns?: number; maxCostUsd?: number } = {}) {
	const root = await mkdtemp(join(tmpdir(), "astra-core-chain-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "offline core chains",
		automation: "full",
		definitions: [stage],
		...options,
	});
	return { root, store, job };
}
function packet(job: ResearchJob, objective: string): Parameters<ResearchJob["dispatchTask"]>[0] {
	return {
		stageId: "validation",
		stageExecutionId: "stage_exec_validation",
		role: "worker",
		objective,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verify content"],
		successCriteria: ["verify content"],
		failureSignals: ["missing content"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	};
}
async function evidence(job: ResearchJob, task?: TaskPacket) {
	const source = task ?? (await job.dispatchTask(packet(job, `target ${Object.keys(job.state.tasks).length}`)));
	await job.setTaskStatus(source.id, "succeeded");
	return job.recordEvidence({
		taskId: source.id,
		stageId: "validation",
		type: "validation",
		content: { content: "initial" },
		refs: [],
	});
}
function reviewer(root: string, invalidFirst = false, verdict: "pass" | "fail" = "pass") {
	const runner = new PiChildSessionRunner();
	const run = vi
		.spyOn(runner, "run")
		.mockImplementation(async (_cwd, jobId, taskId, _attempt, _role, _prompt, env = {}) => {
			const passed = verdict === "pass";
			const refs = [`evidence:${env.ASTRA_EVIDENCE_ID}`];
			const criteria = (JSON.parse(env.ASTRA_REVIEW_CRITERIA!) as string[]).map((criterion) => ({
				criterion,
				passed,
				score: passed ? 1 : 0,
				evidenceRefs: refs,
				rationale: "explicit offline assessment",
			}));
			if (invalidFirst && run.mock.calls.length === 1) criteria.push(structuredClone(criteria[0]));
			await writeReviewerOutputManifest(
				{
					schemaVersion: "astra.reviewer_output_manifest.v1",
					manifestId: `manifest-${taskId}`,
					jobId,
					taskId,
					evidenceId: env.ASTRA_EVIDENCE_ID!,
					verdict,
					score: passed ? 1 : 0,
					findings: passed ? [] : ["verify content requires correction"],
					criteria,
					verifiedRefs: refs,
					sessionRef: "offline",
					createdAt: new Date().toISOString(),
				},
				root,
			);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
		});
	return { adapter: new PiReviewerAdapter(runner), run };
}
async function review(job: ResearchJob, value: Evidence, adapter: PiReviewerAdapter) {
	return job.recordReview({ ...(await adapter.review(value, job)), evidenceId: value.id });
}
async function rejected(job: ResearchJob, value: Evidence, adapter: PiReviewerAdapter) {
	await adapter.review(value, job);
	await job.recoverPendingOperations();
	expect(Object.values(job.state.reviewDeliveryRejections ?? {})).toHaveLength(1);
}
async function approvedPlan(job: ResearchJob, mode: "search" | "decompose" = "decompose") {
	const plan: StagePlanManifest = {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: "plan_two",
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: "plan_two",
		mode,
		tasks: [0, 1].map((i) => ({
			key: `p${i}`,
			objective: `deliver p${i}`,
			hypothesis: `p${i}`,
			inputArtifactRefs: [],
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verify content"],
			successCriteria: ["verify content"],
			failureSignals: ["missing content"],
		})),
		rationale: "offline plan",
		sessionRef: "offline:main",
		createdAt: new Date().toISOString(),
	};
	await job.recordStagePlan(plan);
	const target = await preparePlanEvidence(job, plan);
	await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
	return plan;
}
async function planned(job: ResearchJob, plan: StagePlanManifest, index: number) {
	const contract = buildEffectiveTaskContract(job, plan, plan.tasks[index]);
	const { inputVersions: _inputVersions, ...fields } = contract;
	return job.dispatchTask({
		...fields,
		effectiveContractHash: semanticContractHash(contract),
		role: "worker",
		replayKey: `stage-plan:${plan.id}:${plan.tasks[index].key}`,
		stageExecutionId: "stage_exec_validation",
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
	});
}
async function reopenedRecovery(store: JsonlAstraStore, job: ResearchJob) {
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	await reopened.recoverPendingOperations();
	const sequence = reopened.state.eventSeq;
	await reopened.recoverPendingOperations();
	expect(reopened.state.eventSeq).toBe(sequence);
}

describe("C1 rejected delivery history", () => {
	it("recovers rejected history after canonical retirement and keeps live corruption strict", async () => {
		const { root, job, store } = await setup();
		const target = await evidence(job);
		const r = reviewer(root, true);
		await rejected(job, target, r.adapter);
		await review(job, target, r.adapter);
		await job.decideEvidence(target.id, true, "accept");
		await job.adoptEvidence(target.id);
		await job.reopenStage("validation", "backtrack", "change direction");
		await reopenedRecovery(store, job);
		expect(r.run).toHaveBeenCalledTimes(2);
	});
	it("recovers rejected history after repair discard", async () => {
		const { root, job, store } = await setup();
		const target = await evidence(job);
		const r = reviewer(root, true, "fail");
		await rejected(job, target, r.adapter);
		await review(job, target, r.adapter);
		await job.decideEvidence(target.id, false, "reject");
		const repairs = Object.values(job.state.obligations).flatMap((o) =>
			(o.items ?? []).map((item) => ({
				issueId: item.id,
				criterion: job.repairCriterion(`[${item.id}] ${item.criterion}`),
			})),
		);
		const task = await job.dispatchTask({
			...packet(job, "repair"),
			repairOfEvidenceId: target.id,
			inputArtifactRefs: [target.id],
			repairChecks: repairs,
			acceptanceChecks: ["verify content", ...repairs.map((x) => x.criterion)],
		});
		const fixed = (await job.completeWorkerTask(task.id, {
			artifactType: "validation",
			content: { content: "corrected" },
			refs: [],
		}))!;
		await review(job, fixed, reviewer(root).adapter);
		await job.decideEvidence(fixed.id, true, "accept fixed");
		await job.adoptEvidence(fixed.id);
		expect(job.state.discardedEvidence[target.id].cleanupStatus).toBe("completed");
		await reopenedRecovery(store, job);
	});
	it("recovers rejected history after search loser cleanup", async () => {
		const { root, job, store } = await setup();
		const plan = await approvedPlan(job, "search");
		const targets: Evidence[] = [];
		for (let i = 0; i < 2; i++) {
			const task = await planned(job, plan, i);
			const target = (await job.completeWorkerTask(task.id, {
				artifactType: "validation",
				content: { content: `p${i}` },
				refs: [],
			}))!;
			const r = reviewer(root, i === 1);
			if (i === 1) await rejected(job, target, r.adapter);
			const result = await review(job, target, r.adapter);
			await job.recordCandidateEvaluationFromReview(result.id);
			targets.push(target);
		}
		const batch = Object.values(job.state.searchBatches)[0];
		const winner = Object.values(batch.candidates).find((c) => c.evidenceId === targets[0].id)!;
		await job.selectSearchCandidate(batch.id, winner.id, "choose");
		await reopenedRecovery(store, job);
	});
	it("rejects changed completed rejection manifest while target is live", async () => {
		const { root, job } = await setup();
		const target = await evidence(job);
		await rejected(job, target, reviewer(root, true).adapter);
		const receipt = Object.values(job.state.reviewDeliveryRejections!)[0];
		await writeFile(reviewerManifestPath(root, job.state.frame.jobId, receipt.taskId), "{}");
		await expect(job.recoverPendingOperations()).rejects.toThrow(/rejected review manifest/);
	});
	it("requires the completed historical cleanup intent to match its journal declaration", async () => {
		const { root, job, store } = await setup();
		const target = await evidence(job);
		const r = reviewer(root, true);
		await rejected(job, target, r.adapter);
		await review(job, target, r.adapter);
		await job.decideEvidence(target.id, true);
		await job.adoptEvidence(target.id);
		await job.reopenStage("validation", "history", "retire");
		const invalid = job.state;
		const intent = Object.values(invalid.cleanupIntents!)[0];
		expect(intent.tasks.length).toBeGreaterThan(0);
		intent.tasks = [];
		await store.writeSnapshot(invalid);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await expect(reopened.recoverPendingOperations()).rejects.toThrow(/identity|declaration/);
		expect(reopened.state.eventSeq).toBe(invalid.eventSeq);
	});
	it.each(["healthy", "missing snapshot", "changed snapshot", "missing packet", "changed packet"])(
		"checks incomplete rejection's frozen bundle after pending logical cleanup (%s)",
		async (variant) => {
			const { root, job, store } = await setup();
			const source = await job.dispatchTask(packet(job, "pending rejected target"));
			await prepareTaskWorkspace(source, job);
			const target = await evidence(job, source);
			await reviewer(root, true).adapter.review(target, job);
			const append = store.append.bind(store);
			const interrupted = vi.spyOn(store, "append").mockImplementation(async (jobId, event) => {
				if (event.type === "task_status" && event.status === "failed")
					throw new Error("injected rejection tail interruption");
				return append(jobId, event);
			});
			await expect(job.recoverPendingOperations()).rejects.toThrow("injected rejection tail interruption");
			interrupted.mockRestore();
			const rejection = Object.values(job.state.reviewDeliveryRejections!)[0];
			const folder = join(root, ".astra/jobs", source.jobId, "tasks", rejection.taskId);
			if (variant !== "healthy") {
				const path = join(
					folder,
					variant.endsWith("snapshot") ? "review-target-snapshot.json" : "review-packet.json",
				);
				if (variant.startsWith("missing")) await rm(path);
				else await writeFile(path, "{}");
			}
			const intent = {
				id: `evidence:${target.id}`,
				kind: "evidence" as const,
				receipt: {
					evidenceId: target.id,
					taskId: source.id,
					reviewIds: [],
					checksum: target.checksum,
					reason: "superseded-repair" as const,
					cleanupStatus: "pending" as const,
				},
				status: "pending" as const,
				tasks: [],
			};
			await store.append(source.jobId, { type: "cleanup_requested", intent });
			const reopened = (await ResearchJob.open(store, source.jobId))!;
			const before = reopened.state;
			if (variant === "healthy") {
				await reopened.recoverPendingOperations();
				expect(reopened.state.tasks[rejection.taskId].status).toBe("failed");
				expect(reopened.state.cleanupIntents![intent.id].status).toBe("completed");
				await reopenedRecovery(store, reopened);
			} else {
				await expect(reopened.recoverPendingOperations()).rejects.toThrow();
				expect(reopened.state).toEqual(before);
				expect((await store.readEvents(source.jobId)).length).toBe(before.eventSeq);
			}
			await access(join(root, ".astra/jobs", source.jobId, "tasks", source.id, "task-packet.json"));
		},
	);
});

describe("C2 immutable registration", () => {
	it("rejects task ID and replay declaration conflicts without side effects; exact repeat preserves lifecycle", async () => {
		const { job, store } = await setup();
		const input = { ...packet(job, "immutable"), id: "task_fixed", replayKey: "fixed" };
		const task = await job.dispatchTask(input);
		await job.setTaskStatus(task.id, "succeeded");
		const before = job.state;
		await expect(job.dispatchTask({ ...input, objective: "different", replayKey: "other" })).rejects.toThrow(
			/identity|declaration/,
		);
		await expect(job.dispatchTask({ ...input, acceptanceChecks: ["different"] })).rejects.toThrow(
			/identity|declaration/,
		);
		await expect(job.dispatchTask({ ...input, id: "task_other" })).rejects.toThrow(/identity|declaration/);
		expect(await job.dispatchTask(input)).toEqual(before.tasks[task.id]);
		expect(job.state).toEqual(before);
		expect((await store.readEvents(task.jobId)).length).toBe(before.eventSeq);
	});
	it("rejects evidence ID conflict before freezing files, and exact repeat returns the original accepted fact", async () => {
		const { job } = await setup();
		const target = await evidence(job);
		await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(target.id, true, "accept");
		const input = {
			id: target.id,
			taskId: target.taskId,
			stageId: target.stageId,
			type: target.type,
			content: target.content,
			refs: target.refs,
		};
		const before = job.state;
		await expect(
			job.recordEvidence({ ...input, content: { content: "different" }, refs: ["missing.json"] }),
		).rejects.toThrow(/identity|declaration/);
		expect(await job.recordEvidence(input)).toEqual(before.evidence[target.id]);
		expect(job.state).toEqual(before);
	});
	it("rejects reuse of deleted evidence identity and journal overwrite", async () => {
		const { job, store } = await setup();
		const target = await evidence(job);
		await job.recordReview(reviewFixture(job, { evidenceId: target.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(target.id, true, "accept");
		await job.adoptEvidence(target.id);
		await job.reopenStage("validation", "backtrack", "change direction");
		const task = await job.dispatchTask(packet(job, "new revision"));
		await job.setTaskStatus(task.id, "succeeded");
		const before = job.state;
		await expect(
			job.recordEvidence({
				id: target.id,
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: { content: "new" },
				refs: [],
			}),
		).rejects.toThrow(/identity|declaration/);
		expect(job.state).toEqual(before);
		await store.append(task.jobId, {
			type: "evidence_recorded",
			evidence: { ...target, content: { content: "illegal overwrite" } },
		});
		await expect(ResearchJob.open(store, task.jobId)).rejects.toThrow(/identity|history/);
	});
	it("keeps equivalent worker retry and default-attempt reviewer retry", async () => {
		const { job } = await setup();
		const input = { ...packet(job, "worker retry"), replayKey: "retry" };
		const previous = await job.dispatchTask(input);
		await job.setTaskStatus(previous.id, "failed");
		const next = await job.dispatchTask({ ...input, attempt: 2, supersedesTaskId: previous.id });
		expect(next.id).not.toBe(previous.id);
		const reviewInput = { ...packet(job, "review retry"), role: "reviewer" as const, replayKey: "review retry" };
		const old = await job.dispatchTask(reviewInput);
		await job.setTaskStatus(old.id, "failed");
		const retried = await job.dispatchTask(reviewInput);
		expect(retried.id).not.toBe(old.id);
		expect(retried.attempt).toBe(1);
	});
});

describe("C3 shared task ownership", () => {
	it.each([false, true])("retains only shared live task files and session paths (same task=%s)", async (sameTask) => {
		const { root, store, job: initial } = await setup();
		let job = initial;
		const source = await job.dispatchTask(packet(job, "source"));
		await prepareTaskWorkspace(source, job);
		const folder = join(root, ".astra/jobs", source.jobId);
		await mkdir(join(folder, "resources", source.id), { recursive: true });
		await writeFile(join(folder, "resources", source.id, "data"), "source");
		const log = join(folder, "session.jsonl");
		await writeFile(log, "{}\n");
		await job.recordChildSession({
			sessionId: "source-session",
			taskId: source.id,
			role: "worker",
			status: "completed",
			attempt: 1,
			sessionFile: log,
			manifestRef: "source-manifest",
			updatedAt: new Date().toISOString(),
		});
		const first = (await job.completeWorkerTask(source.id, {
			artifactType: "validation",
			content: { content: "initial" },
			refs: [],
		}))!;
		await job.recordReview(reviewFixture(job, { evidenceId: first.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(first.id, true, "first");
		const artifact = await job.adoptEvidence(first.id);
		let target = source;
		if (!sameTask) {
			target = await job.dispatchTask(packet(job, "replacement"));
			await prepareTaskWorkspace(target, job);
			await job.setTaskStatus(target.id, "succeeded");
		}
		const tools = new Map<string, ToolDefinition>();
		createAstraExtension({ jobId: source.jobId })({
			registerTool: (tool: ToolDefinition) => tools.set(tool.name, tool),
			registerFlag: () => {},
			registerCommand: () => {},
			on: () => {},
		} as unknown as ExtensionAPI);
		const result = await tools
			.get("research_record_evidence")!
			.execute(
				"offline",
				{ taskId: target.id, type: "validation", content: { content: "replacement" }, refs: [] },
				undefined,
				undefined,
				{ cwd: root } as ExtensionContext,
			);
		const second = result.details as Evidence;
		job = (await ResearchJob.open(store, source.jobId))!;
		await job.recordReview(reviewFixture(job, { evidenceId: second.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(second.id, true, "second");
		const adopted = await job.adoptEvidence(second.id, artifact.id);
		expect(() => job.assertEvidenceCurrent(second.id)).not.toThrow();
		await access(join(folder, "tasks", target.id, "task-packet.json"));
		if (sameTask) {
			expect(job.state.sessions["source-session"].sessionFile).toBe(log);
			expect(job.state.sessions["source-session"].manifestRef).toBe("source-manifest");
			await access(join(folder, "resources", source.id, "data"));
		} else await expect(access(join(folder, "tasks", source.id))).rejects.toThrow();
		await expect(
			job.dispatchTask({ ...packet(job, "consume new"), inputArtifactRefs: [adopted.id] }),
		).resolves.toBeDefined();
		await expect(
			job.dispatchTask({ ...packet(job, "consume old"), inputArtifactRefs: [first.id] }),
		).rejects.toThrow();
		await reopenedRecovery(store, job);
	});
	it("filters a persisted pending intent without changing its hash-bound task descriptors", async () => {
		const { root, store, job } = await setup();
		const source = await job.dispatchTask(packet(job, "shared pending"));
		await prepareTaskWorkspace(source, job);
		const log = join(root, ".astra/jobs", source.jobId, "pending-session.jsonl");
		await writeFile(log, "{}\n");
		await job.recordChildSession({
			sessionId: "pending-session",
			taskId: source.id,
			role: "worker",
			status: "completed",
			attempt: 1,
			sessionFile: log,
			manifestRef: "manifest",
			updatedAt: new Date().toISOString(),
		});
		const old = await evidence(job, source);
		const kept = await job.recordEvidence({
			taskId: source.id,
			stageId: old.stageId,
			type: old.type,
			content: { content: "retained" },
			refs: [],
		});
		const tasks = await cleanupTaskFiles(job.state, [source.id], new Set([old.id, kept.id]));
		expect(tasks).toHaveLength(1);
		const intent = {
			id: `evidence:${old.id}`,
			kind: "evidence" as const,
			receipt: {
				evidenceId: old.id,
				taskId: source.id,
				reviewIds: [],
				checksum: old.checksum,
				reason: "superseded-repair" as const,
				cleanupStatus: "pending" as const,
			},
			status: "pending" as const,
			tasks,
		};
		await store.append(source.jobId, { type: "cleanup_requested", intent });
		const reopened = (await ResearchJob.open(store, source.jobId))!;
		await reopened.recoverPendingOperations();
		expect(reopened.state.cleanupIntents![intent.id].tasks).toEqual(tasks);
		expect(reopened.state.cleanupIntents![intent.id].status).toBe("completed");
		expect(reopened.state.sessions["pending-session"]).toMatchObject({ sessionFile: log, manifestRef: "manifest" });
		const again = (await ResearchJob.open(store, source.jobId))!;
		expect(again.state.sessions["pending-session"]).toMatchObject({ sessionFile: log, manifestRef: "manifest" });
		expect(() => reopened.assertEvidenceCurrent(kept.id)).not.toThrow();
		await access(join(root, ".astra/jobs", source.jobId, "tasks", source.id, "task-packet.json"));
	});
	it("does not make previously pruned shared source files current", async () => {
		const { root, store, job } = await setup();
		const source = await job.dispatchTask(packet(job, "old damaged source"));
		await prepareTaskWorkspace(source, job);
		const old = await evidence(job, source);
		const kept = await job.recordEvidence({
			taskId: source.id,
			stageId: old.stageId,
			type: old.type,
			content: { content: "retained" },
			refs: [],
		});
		const tasks = await cleanupTaskFiles(job.state, [source.id], new Set([old.id, kept.id]));
		const intent = {
			id: `evidence:${old.id}`,
			kind: "evidence" as const,
			receipt: {
				evidenceId: old.id,
				taskId: source.id,
				reviewIds: [],
				checksum: old.checksum,
				reason: "superseded-repair" as const,
				cleanupStatus: "pending" as const,
			},
			status: "pending" as const,
			tasks,
		};
		await store.append(source.jobId, { type: "cleanup_requested", intent });
		await rm(join(root, ".astra/jobs", source.jobId, "tasks", source.id), { recursive: true });
		let reopened = (await ResearchJob.open(store, source.jobId))!;
		await expect(reopened.recoverPendingOperations()).rejects.toThrow(/retained task files are missing/);
		await store.append(source.jobId, {
			type: "cleanup_completed",
			intentId: intent.id,
			archiveRefs: [join(root, ".astra/jobs", source.jobId, "archive/tasks", source.id)],
			completedAt: new Date().toISOString(),
		});
		reopened = (await ResearchJob.open(store, source.jobId))!;
		expect(() => reopened.assertEvidenceCurrent(kept.id)).toThrow(/stale|pruned/);
		await expect(
			reopened.dispatchTask({ ...packet(reopened, "consume damaged"), inputArtifactRefs: [kept.id] }),
		).rejects.toThrow(/stale|pruned/);
	});
});

describe("C5 persisted budget capacity", () => {
	it.each(["maxTasks", "maxTurns"] as const)(
		"retains batch %s requirement and merges larger minimum",
		async (limit) => {
			const { job, store } = await setup({ [limit]: 2 });
			if (limit === "maxTasks") await job.dispatchTask(packet(job, "used task"));
			else await job.consumeTurns(1);
			const block = job.budgetBlock(limit === "maxTasks" ? { tasks: 2 } : { turns: 2 })!;
			await job.requireUserGate({ kind: "budget", stageId: "validation", ...block });
			const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
			await expect(reopened.resume()).rejects.toThrow(/budget gate remains/);
			await expect(reopened.resumeWithGuidance("less work")).rejects.toThrow(/cannot bypass/);
			await reopened.requireUserGate({
				kind: "budget",
				stageId: "validation",
				...reopened.budgetBlock(limit === "maxTasks" ? { tasks: 3 } : { turns: 3 })!,
			});
			await reopened.updateBudget({ [limit]: 3 });
			await expect(reopened.resume()).rejects.toThrow(/budget gate remains/);
			await reopened.updateBudget({ [limit]: 4 });
			await reopened.resume();
			expect(reopened.status().paused).toBe(false);
		},
	);
	it("does not guess capacity for a legacy budget gate", async () => {
		const { job } = await setup({ maxTasks: 2 });
		await job.requireUserGate({
			kind: "budget",
			stageId: "validation",
			limit: "maxTasks",
			reason: "legacy text requires 3",
		});
		await expect(job.resume()).rejects.toThrow(/capacity|requirement|minimum/);
	});
	it("runs the approved-plan batch budget gate through supervisor", async () => {
		const { job, store } = await setup({ maxTasks: 2 });
		await approvedPlan(job);
		const unexpected = async () => {
			throw new Error("unexpected model call");
		};
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: unexpected },
			reviewer: { review: unexpected },
			mainAgent: {
				planStage: unexpected,
				decideEvidence: unexpected,
				decideAdoption: unexpected,
				decideSearch: unexpected,
				decideRoute: unexpected,
			},
		});
		await supervisor.tick();
		expect(job.status().userGate?.kind).toBe("budget");
		await expect(job.resume()).rejects.toThrow(/budget gate remains/);
		await job.updateBudget({ maxTasks: 3 });
		await job.resume();
		expect(job.status().paused).toBe(false);
	});
	it("persists capacity for planning's two registrations before creating either task", async () => {
		const { job, store } = await setup({ maxTasks: 1 });
		const unexpected = async () => {
			throw new Error("unexpected model call");
		};
		const planStage = vi.fn(
			async (): Promise<StagePlanManifest> => ({
				schemaVersion: "astra.stage_plan_manifest.v1",
				id: "capacity-plan",
				jobId: job.state.frame.jobId,
				stageId: "validation",
				decisionRef: "capacity-plan",
				mode: "decompose",
				tasks: [
					{
						key: "one",
						objective: "deliver",
						hypothesis: "deliver",
						inputArtifactRefs: [],
						requiredOutputFields: ["content"],
						acceptanceChecks: ["verify content"],
						successCriteria: ["verify content"],
						failureSignals: ["missing content"],
					},
				],
				rationale: "offline",
				sessionRef: "offline:main",
				createdAt: new Date().toISOString(),
			}),
		);
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: unexpected },
			reviewer: { review: unexpected },
			mainAgent: {
				planStage,
				decideEvidence: unexpected,
				decideAdoption: unexpected,
				decideSearch: unexpected,
				decideRoute: unexpected,
			},
		});
		await supervisor.tick();
		expect(planStage).toHaveBeenCalledTimes(1);
		expect(job.state.tasks).toEqual({});
		expect(job.status().userGate).toMatchObject({ kind: "budget", limit: "maxTasks", requiredMinimum: 2 });
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await expect(reopened.resume()).rejects.toThrow(/budget gate remains/);
		await reopened.updateBudget({ maxTasks: 2 });
		await reopened.resume();
	});
	it("persists the complete ready-worker turn batch before running any worker", async () => {
		const { job, store } = await setup({ maxTurns: 2 });
		const plan = await approvedPlan(job);
		await planned(job, plan, 0);
		await planned(job, plan, 1);
		await job.consumeTurns(1);
		const unexpected = vi.fn(async () => {
			throw new Error("unexpected model call");
		});
		const supervisor = new ResearchSupervisor(job, store, {
			maxParallel: 2,
			worker: { run: unexpected },
			reviewer: { review: unexpected },
			mainAgent: {
				planStage: unexpected,
				decideEvidence: unexpected,
				decideAdoption: unexpected,
				decideSearch: unexpected,
				decideRoute: unexpected,
			},
		});
		await supervisor.tick();
		expect(unexpected).not.toHaveBeenCalled();
		expect(job.status().userGate).toMatchObject({ kind: "budget", limit: "maxTurns", requiredMinimum: 3 });
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await expect(reopened.resume()).rejects.toThrow(/budget gate remains/);
		await reopened.updateBudget({ maxTurns: 3 });
		await reopened.resume();
	});
	it("requires cost headroom while allowing unlimited cost", async () => {
		const { job, store } = await setup({ maxCostUsd: 0.1 });
		await job.recordCost(0.25);
		await job.requireUserGate({ kind: "budget", stageId: "validation", ...job.budgetBlock()! });
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.updateBudget({ maxCostUsd: 0.25 });
		await expect(reopened.resume()).rejects.toThrow(/budget gate remains/);
		await reopened.updateBudget({ maxCostUsd: undefined });
		await reopened.resume();
		expect(reopened.status().paused).toBe(false);
	});
});
