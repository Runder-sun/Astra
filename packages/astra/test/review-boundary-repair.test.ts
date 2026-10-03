import { createHash } from "node:crypto";
import fsPromises, { chmod, mkdir, mkdtemp, readFile, rename, rm, symlink, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import type { ExtensionAPI, ExtensionContext, ToolDefinition } from "@earendil-works/pi-coding-agent";
import { afterEach, describe, expect, it, vi } from "vitest";
import { reviewerManifestPath, reviewPacketPath, writeReviewerOutputManifest } from "../src/contracts.ts";
import { createAstraExtension } from "../src/extension.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { ReviewIntegrityError } from "../src/review-validation.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { taskInputResources, taskResourcePath } from "../src/task-workspace.ts";
import type { ReviewerOutputManifest, ReviewPacket, TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	syncBuiltinESMExports();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function setup() {
	const root = await mkdtemp(join(tmpdir(), "astra-review-boundary-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		objective: "offline review boundary",
		workspaceRoot: root,
		automation: "full",
	});
	return { root, store, job };
}

async function delivery(job: ResearchJob, fields: Partial<TaskPacket> = {}, refs: string[] = []) {
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: `frozen target ${Object.keys(job.state.tasks).length}`,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		successCriteria: ["complete"],
		failureSignals: ["missing"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		...fields,
	});
	if (refs.includes("result.json")) {
		const workspace = join(task.scope.workspaceRoot, ".astra/jobs", task.jobId, "workspaces", task.id);
		await mkdir(workspace, { recursive: true });
		await writeFile(join(workspace, "result.json"), '{"metric":1}\n');
	}
	await job.setTaskStatus(task.id, "succeeded");
	return job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: "frozen" },
		refs,
	});
}

function piReviewer(
	root: string,
	action?: (manifest: ReviewerOutputManifest, env: Record<string, string | undefined>) => Promise<void>,
) {
	const runner = new PiChildSessionRunner();
	const run = vi
		.spyOn(runner, "run")
		.mockImplementation(async (_cwd, jobId, taskId, _attempt, _role, _prompt, env = {}) => {
			const refs = [`evidence:${env.ASTRA_EVIDENCE_ID}`];
			const manifest: ReviewerOutputManifest = {
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
					rationale: "explicit offline assessment",
				})),
				sessionRef: "offline",
				createdAt: new Date().toISOString(),
			};
			if (action) await action(manifest, env);
			else await writeReviewerOutputManifest(manifest, root);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
		});
	return { adapter: new PiReviewerAdapter(runner), run };
}

function badAssessment(manifest: ReviewerOutputManifest, fault: string) {
	if (fault === "duplicate") manifest.criteria.push(structuredClone(manifest.criteria[0]));
	if (fault === "foreign-ref") manifest.verifiedRefs = ["undeclared:private"];
	if (fault === "empty-refs") manifest.criteria[0].evidenceRefs = [];
	if (fault === "negative-all-passed") manifest.verdict = "fail";
}

async function resourceReview(historical = false) {
	const fixture = await setup();
	const { root, job } = fixture;
	const upstream = await delivery(job);
	await mkdir(taskResourcePath(root, job.state.frame.jobId, upstream.taskId), { recursive: true });
	await job.recordReview(reviewFixture(job, { evidenceId: upstream.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(upstream.id, true);
	if (historical) {
		await job.adoptEvidence(upstream.id);
		await job.reopenStage(upstream.stageId, "resource_history", "verify historical resources");
	}
	const evidence = await delivery(job, { inputArtifactRefs: historical ? [] : [upstream.id] });
	const { adapter, run } = piReviewer(root);
	const review = await adapter.review(evidence, job);
	const task = job.state.tasks[review.reviewerTaskId!];
	const resources = [
		...(await taskInputResources(task, job)),
		...(await taskInputResources(job.state.tasks[evidence.taskId], job)),
	].map(({ artifactId, artifactType, taskId, root }) => ({ artifactId, artifactType, taskId, root }));
	expect(resources.length).toBeGreaterThan(0);
	const path = join(root, ".astra/jobs", task.jobId, "tasks", task.id, "review-target-snapshot.json");
	const frozen = JSON.parse(await readFile(path, "utf8"));
	frozen.resources = resources;
	await writeFile(path, JSON.stringify(frozen));
	return { ...fixture, evidence, review, resources, frozen, path, run };
}

describe("bound review resources", () => {
	it("keeps resource realpath I/O failure out of semantic rejection recovery", async () => {
		const { job, store, evidence, review, resources } = await resourceReview();
		const original = fsPromises.realpath;
		const failure = Object.assign(new Error("injected resource realpath EIO"), { code: "EIO" });
		const fault = vi.spyOn(fsPromises, "realpath").mockImplementation(async (path, options) => {
			if (String(path) === resources[0].root) throw failure;
			return original(path, options);
		});
		syncBuiltinESMExports();
		try {
			await expect(job.validateReview({ ...review, evidenceId: evidence.id })).rejects.toBeInstanceOf(
				ReviewIntegrityError,
			);
			await expect(
				(await ResearchJob.open(store, job.state.frame.jobId))!.recoverPendingOperations(),
			).rejects.toThrow("injected resource realpath EIO");
			expect(
				(await store.readEvents(job.state.frame.jobId)).filter(
					(saved) => saved.event.type === "review_delivery_rejected",
				),
			).toHaveLength(0);
		} finally {
			fault.mockRestore();
			syncBuiltinESMExports();
		}
	});
	it.each([false, true])("accepts legal resources and restores their review (historical=%s)", async (historical) => {
		const { job, store, evidence, review, resources, run } = await resourceReview(historical);
		const refs = [resources[0].artifactId];
		const saved = await job.recordReview({
			...review,
			evidenceId: evidence.id,
			verifiedRefs: refs,
			criteria: review.criteria?.map((criterion) => ({ ...criterion, evidenceRefs: refs })),
		});
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		expect(reopened.state.reviews[saved.id]).toEqual(saved);
		expect(run).toHaveBeenCalledOnce();
	});

	for (const recovery of [false, true]) {
		it.each(["artifactId", "artifactType", "taskId", "root"] as const)(
			`rejects snapshot resource %s injection as integrity failure (recovery=${recovery})`,
			async (field) => {
				const { root, job, store, evidence, review, resources, frozen, path, run } = await resourceReview();
				frozen.resources[0][field] = field === "root" ? join(root, "private") : "foreign-not-declared";
				await writeFile(path, JSON.stringify(frozen));
				const original = await readFile(reviewerManifestPath(root, job.state.frame.jobId, review.reviewerTaskId!));
				if (recovery) {
					const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
					await expect(reopened.recoverPendingOperations()).rejects.toThrow(
						/resource.*binding|resource.*identity/i,
					);
				} else {
					const refs = [field === "artifactId" ? "foreign-not-declared" : resources[0].artifactId];
					await expect(
						job.recordReview({
							...review,
							evidenceId: evidence.id,
							verifiedRefs: refs,
							criteria: review.criteria?.map((criterion) => ({ ...criterion, evidenceRefs: refs })),
						}),
					).rejects.toThrow(/resource.*binding|resource.*identity/i);
				}
				expect(await readFile(reviewerManifestPath(root, job.state.frame.jobId, review.reviewerTaskId!))).toEqual(
					original,
				);
				expect(
					(await store.readEvents(job.state.frame.jobId)).filter(
						(saved) => saved.event.type === "review_delivery_rejected",
					),
				).toHaveLength(0);
				expect(run).toHaveBeenCalledOnce();
			},
		);
		it.each([false, true])(
			`rejects resource ancestor links (historical=%s, recovery=${recovery})`,
			async (historical) => {
				const { job, store, evidence, review, resources, run } = await resourceReview(historical);
				const outside = await mkdtemp(join(tmpdir(), "astra-resource-outside-"));
				roots.push(outside);
				const parent = dirname(resources[0].root);
				await rename(parent, join(outside, "linked-parent"));
				await symlink(join(outside, "linked-parent"), parent, "dir");
				await expect(taskInputResources(job.state.tasks[evidence.taskId], job)).rejects.toThrow(/symbolic|escape/i);
				if (recovery)
					await expect(
						(await ResearchJob.open(store, job.state.frame.jobId))!.recoverPendingOperations(),
					).rejects.toThrow(/symbolic|escape/i);
				else
					await expect(job.recordReview({ ...review, evidenceId: evidence.id })).rejects.toThrow(
						/symbolic|escape/i,
					);
				expect(
					(await store.readEvents(job.state.frame.jobId)).filter(
						(saved) => saved.event.type === "review_delivery_rejected",
					),
				).toHaveLength(0);
				expect(run).toHaveBeenCalledOnce();
			},
		);
	}
});

describe("shared durable review boundary", () => {
	it("rejects an upstream copy with a different frozen version despite self-consistent copied bytes and indexes", async () => {
		const { root, job } = await setup();
		const upstream = await delivery(job);
		await job.recordReview(reviewFixture(job, { evidenceId: upstream.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(upstream.id, true);
		const evidence = await delivery(job, { inputArtifactRefs: [upstream.id] });
		const { adapter } = piReviewer(root);
		const review = await adapter.review(evidence, job);
		const path = reviewPacketPath(root, job.state.frame.jobId, review.reviewerTaskId!);
		const packet = JSON.parse(await readFile(path, "utf8")) as ReviewPacket;
		const ref = packet.resolvedEvidenceRefs.find((item) => item.sourceRef === `input-evidence/${upstream.id}.json`)!;
		expect(ref).toBeDefined();
		const bytes = JSON.stringify({ ...upstream, versionHash: "f".repeat(64) });
		await chmod(join(dirname(path), ref.path), 0o600);
		await writeFile(join(dirname(path), ref.path), bytes);
		ref.sha256 = createHash("sha256").update(bytes).digest("hex");
		await writeFile(path, JSON.stringify(packet));
		const frozenPath = join(dirname(path), "review-target-snapshot.json");
		const frozen = JSON.parse(await readFile(frozenPath, "utf8"));
		frozen.resolvedEvidenceRefs = packet.resolvedEvidenceRefs;
		await writeFile(frozenPath, JSON.stringify(frozen));
		await expect(job.recordReview({ ...review, evidenceId: evidence.id })).rejects.toThrow(
			/upstream evidence frozen identity/,
		);
	});
	it("rejects an undeclared auxiliary source even when both frozen indexes and actual bytes agree", async () => {
		const { root, job } = await setup();
		const evidence = await delivery(job, {}, ["result.json"]);
		const { adapter } = piReviewer(root);
		const review = await adapter.review(evidence, job);
		const path = reviewPacketPath(root, job.state.frame.jobId, review.reviewerTaskId!);
		const packet = JSON.parse(await readFile(path, "utf8")) as ReviewPacket;
		const extra = "private injected evidence";
		packet.resolvedEvidenceRefs.push({
			sourceRef: "unknown-auxiliary",
			path: "unknown-auxiliary.txt",
			sha256: createHash("sha256").update(extra).digest("hex"),
		});
		await writeFile(join(dirname(path), "unknown-auxiliary.txt"), extra);
		await writeFile(path, JSON.stringify(packet));
		const frozenPath = join(dirname(path), "review-target-snapshot.json");
		const frozen = JSON.parse(await readFile(frozenPath, "utf8"));
		frozen.resolvedEvidenceRefs = packet.resolvedEvidenceRefs;
		await writeFile(frozenPath, JSON.stringify(frozen));
		await expect(job.recordReview({ ...review, evidenceId: evidence.id })).rejects.toThrow(
			/bound sources|declarations/,
		);
		expect(Object.values(job.state.reviews)).toHaveLength(0);
	});
	it.each(["task-root", "ancestor"])(
		"rejects a symlink at the reviewer %s even when every copied file is healthy",
		async (fault) => {
			const { root, job } = await setup();
			const evidence = await delivery(job, {}, ["result.json"]);
			const { adapter } = piReviewer(root);
			const review = await adapter.review(evidence, job);
			const taskRoot = dirname(reviewPacketPath(root, job.state.frame.jobId, review.reviewerTaskId!));
			const target = fault === "task-root" ? taskRoot : dirname(taskRoot);
			const moved = `${target}-original`;
			await rename(target, moved);
			await symlink(moved, target);
			await expect(job.recordReview({ ...review, evidenceId: evidence.id })).rejects.toThrow(/symbolic|symlink/i);
			expect(Object.values(job.state.reviews)).toHaveLength(0);
		},
	);
	it("recovers a valid manifest after the session completion tail fails and preserves exact same-ID submission", async () => {
		const { root, job, store } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		const append = store.append.bind(store);
		let injected = false;
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (!injected && event.type === "child_session_recorded" && event.session.status === "completed") {
				injected = true;
				throw new Error("injected session completion tail");
			}
			return append(id, event);
		});
		await expect(adapter.review(evidence, job)).rejects.toThrow(/session completion/);
		expect(injected).toBe(true);
		await job.recoverPendingOperations();
		const original = Object.values(job.state.reviews)[0];
		const seq = job.state.eventSeq;
		expect(await job.recordReview(original)).toEqual(original);
		expect(job.state.eventSeq).toBe(seq);
		expect(Object.values(job.state.sessions).every((session) => session.status === "completed")).toBe(true);
		expect(run).toHaveBeenCalledOnce();
	});
	it.each(["reject-append-before", "reject-append-after", "reject-snapshot", "task-tail", "session-tail"])(
		"replays %s rejection interruption once without changing bad bytes or invoking another model",
		async (phase) => {
			const { root, job, store } = await setup();
			const evidence = await delivery(job);
			const { adapter, run } = piReviewer(root);
			const output = await adapter.review(evidence, job);
			const taskId = output.reviewerTaskId!;
			const path = reviewerManifestPath(root, job.state.frame.jobId, taskId);
			const manifest = JSON.parse(await readFile(path, "utf8")) as ReviewerOutputManifest;
			badAssessment(manifest, "duplicate");
			const bytes = JSON.stringify(manifest);
			await writeFile(path, bytes);
			const append = store.append.bind(store);
			let injected = false;
			const fault = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
				const reject = String(event.type) === "review_delivery_rejected";
				const match = phase.startsWith("reject-")
					? reject
					: phase === "task-tail"
						? event.type === "task_status" && event.status === "failed"
						: event.type === "child_session_recorded" && event.session.status === "failed";
				if (!injected && match) {
					injected = true;
					if (phase === "reject-snapshot") {
						vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("injected rejection snapshot EIO"));
						return append(id, event);
					}
					if (phase === "reject-append-after") await append(id, event);
					throw new Error(`injected ${phase} EIO`);
				}
				return append(id, event);
			});
			await expect(job.recoverPendingOperations()).rejects.toThrow(/injected/);
			expect(injected).toBe(true);
			fault.mockRestore();
			const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
			await reopened.recoverPendingOperations();
			const done = reopened.state;
			await reopened.recoverPendingOperations();
			expect(reopened.state).toEqual(done);
			expect(Object.keys(done.reviewDeliveryRejections ?? {})).toEqual([taskId]);
			expect(done.tasks[taskId].status).toBe("failed");
			expect(
				Object.values(done.sessions)
					.filter((session) => session.taskId === taskId)
					.every((session) => session.status === "failed"),
			).toBe(true);
			expect(run).toHaveBeenCalledOnce();
			expect(await readFile(path, "utf8")).toBe(bytes);
			expect(
				(await store.readEvents(job.state.frame.jobId)).filter(
					(saved) => String(saved.event.type) === "review_delivery_rejected",
				),
			).toHaveLength(1);
		},
	);

	it("retries a durably rejected delivery with a new reviewer identity only when the existing task budget permits", async () => {
		const { root, job } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		const first = await adapter.review(evidence, job);
		const path = reviewerManifestPath(root, job.state.frame.jobId, first.reviewerTaskId!);
		const manifest = JSON.parse(await readFile(path, "utf8")) as ReviewerOutputManifest;
		badAssessment(manifest, "empty-refs");
		const bytes = JSON.stringify(manifest);
		await writeFile(path, bytes);
		await job.recoverPendingOperations();
		const tasks = Object.keys(job.state.tasks).length;
		await job.updateBudget({ maxTasks: tasks });
		await expect(adapter.review(evidence, job)).rejects.toThrow(/task budget/);
		expect(run).toHaveBeenCalledOnce();
		await job.updateBudget({ maxTasks: tasks + 1 });
		const second = await adapter.review(evidence, job);
		expect(second.reviewerTaskId).not.toBe(first.reviewerTaskId);
		await job.recoverPendingOperations();
		expect(job.state.tasks[first.reviewerTaskId!].status).toBe("failed");
		expect(job.state.tasks[second.reviewerTaskId!].status).toBe("succeeded");
		expect(Object.values(job.state.reviews).filter((review) => review.evidenceId === evidence.id)).toHaveLength(1);
		expect(run).toHaveBeenCalledTimes(2);
		expect(await readFile(path, "utf8")).toBe(bytes);
	});

	it("recovers a committed review when append reports failure after durable write without a second review or changed identity", async () => {
		const { root, job, store } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		const output = await adapter.review(evidence, job);
		const append = store.append.bind(store);
		let injected = false;
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			const saved = await append(id, event);
			if (!injected && event.type === "review_recorded") {
				injected = true;
				throw new Error("injected uncertain committed review EIO");
			}
			return saved;
		});
		await expect(
			job.recordReview({ ...output, evidenceId: evidence.id, id: "original_durable_review" }),
		).rejects.toThrow(/uncertain/);
		const original = job.state.reviews.original_durable_review;
		expect(original).toBeDefined();
		await job.recoverPendingOperations();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		expect(reopened.state.reviews.original_durable_review).toEqual(original);
		expect(run).toHaveBeenCalledOnce();
		expect(
			(await store.readEvents(job.state.frame.jobId)).filter((saved) => saved.event.type === "review_recorded"),
		).toHaveLength(1);
	});
	it("rejects conflicting review IDs without events and returns the original for an exact duplicate", async () => {
		const { job } = await setup();
		const first = await delivery(job);
		const second = await delivery(job);
		const input = {
			...reviewFixture(job, { evidenceId: first.id, verdict: "fail", findings: ["failed"] }),
			id: "immutable_review",
		};
		const original = await job.recordReview(input);
		const seq = job.state.eventSeq;
		expect(await job.recordReview(input)).toEqual(original);
		expect(job.state.eventSeq).toBe(seq);
		await expect(
			job.recordReview({
				...reviewFixture(job, { evidenceId: second.id, verdict: "pass", findings: [] }),
				id: original.id,
			}),
		).rejects.toThrow(/review.*id.*content|review.*immutable/i);
		expect(job.state.eventSeq).toBe(seq);
		expect(job.state.obligations.obligation_immutable_review.evidenceId).toBe(first.id);
	});

	it("rejects conflicting already absorbed history while allowing identical consequence tails", async () => {
		const { job, store } = await setup();
		const evidence = await delivery(job);
		const original = await job.recordReview({
			...reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			id: "history_review",
		});
		await store.withWriteLock(job.state.frame.jobId, () =>
			store.append(job.state.frame.jobId, { type: "review_recorded", review: original }).then(() => undefined),
		);
		const good = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await store.writeSnapshot(good.state);
		await store.withWriteLock(job.state.frame.jobId, () =>
			store
				.append(job.state.frame.jobId, {
					type: "review_recorded",
					review: { ...original, findings: ["conflicting body"] },
				})
				.then(() => undefined),
		);
		const snapshot = good.state;
		snapshot.eventSeq++;
		snapshot.reviews[original.id].findings = ["conflicting body"];
		await store.writeSnapshot(snapshot);
		await expect(ResearchJob.open(store, job.state.frame.jobId)).rejects.toThrow(
			/review.*history|review.*integrity/i,
		);
	});

	it.each(["missing", "worker", "cross-job", "revision"])(
		"rejects explicit %s reviewer identity while preserving manual full review",
		async (fault) => {
			const { root, job, store } = await setup();
			const evidence = await delivery(job);
			let tested = job;
			let reviewerTaskId = "task_missing";
			if (fault === "worker") reviewerTaskId = evidence.taskId;
			if (fault === "cross-job" || fault === "revision") {
				const { adapter } = piReviewer(root);
				const output = await adapter.review(evidence, job);
				reviewerTaskId = output.reviewerTaskId!;
				const snapshot = job.state;
				const task = snapshot.tasks[reviewerTaskId];
				expect(task.role).toBe("reviewer");
				if (fault === "cross-job") task.jobId = "job_foreign";
				else task.stageRevision = 0;
				await store.writeSnapshot(snapshot);
				tested = (await ResearchJob.open(store, job.state.frame.jobId))!;
				if (fault === "cross-job")
					expect(tested.state.tasks[reviewerTaskId].jobId).not.toBe(tested.state.frame.jobId);
				else
					expect(tested.state.tasks[reviewerTaskId].stageRevision).not.toBe(
						tested.state.stages.validation.revision,
					);
			}
			await expect(
				tested.recordReview({
					...reviewFixture(tested, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
					reviewerTaskId,
				}),
			).rejects.toThrow();
			expect(
				(
					await tested.recordReview(
						reviewFixture(tested, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
					)
				).verdict,
			).toBe("pass");
		},
	);

	it.each(["duplicate", "foreign-ref", "empty-refs", "negative-all-passed"])(
		"Pi preflight rejects %s without manifest or terminal state and permits correction",
		async (fault) => {
			const { root, job } = await setup();
			const evidence = await delivery(job);
			const { adapter } = piReviewer(root, async (manifest, env) => {
				for (const [key, value] of Object.entries(env)) vi.stubEnv(key, value);
				vi.stubEnv("ASTRA_JOB_ID", manifest.jobId);
				vi.stubEnv("ASTRA_ROLE", "reviewer");
				const tools = new Map<string, ToolDefinition>();
				createAstraExtension({ role: "reviewer" })({
					registerTool: (tool: ToolDefinition) => tools.set(tool.name, tool),
					on() {},
					registerFlag() {},
					registerCommand() {},
				} as unknown as ExtensionAPI);
				const ctx = { cwd: dirname(env.ASTRA_TASK_PACKET!), ui: {} } as unknown as ExtensionContext;
				const original = structuredClone(manifest);
				badAssessment(manifest, fault);
				const tool = tools.get("astra_submit_review")!;
				const rejected = await tool.execute("invalid", manifest, undefined, undefined, ctx);
				expect(rejected.terminate).not.toBe(true);
				await expect(readFile(reviewerManifestPath(root, manifest.jobId, manifest.taskId))).rejects.toMatchObject({
					code: "ENOENT",
				});
				expect((await tool.execute("corrected", original, undefined, undefined, ctx)).terminate).toBe(true);
			});
			await adapter.review(evidence, job);
		},
	);

	it.each(["duplicate", "foreign-ref", "empty-refs", "negative-all-passed"])(
		"durably rejects old %s manifest once, preserving bytes and failing its task and session",
		async (fault) => {
			const { root, job, store } = await setup();
			const evidence = await delivery(job);
			const { adapter, run } = piReviewer(root);
			const review = await adapter.review(evidence, job);
			const path = reviewerManifestPath(root, job.state.frame.jobId, review.reviewerTaskId!);
			const manifest = JSON.parse(await readFile(path, "utf8")) as ReviewerOutputManifest;
			badAssessment(manifest, fault);
			const bytes = JSON.stringify(manifest);
			await writeFile(path, bytes);
			const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
			await reopened.recoverPendingOperations();
			await reopened.recoverPendingOperations();
			expect(await readFile(path, "utf8")).toBe(bytes);
			expect(
				(await store.readEvents(job.state.frame.jobId)).filter(
					(saved) => String(saved.event.type) === "review_delivery_rejected",
				),
			).toHaveLength(1);
			expect(reopened.state.tasks[review.reviewerTaskId!].status).toBe("failed");
			expect(
				Object.values(reopened.state.sessions)
					.filter((session) => session.taskId === review.reviewerTaskId!)
					.every((session) => session.status === "failed"),
			).toBe(true);
			expect(run).toHaveBeenCalledTimes(1);
			await writeFile(path, `${bytes} `);
			await expect(reopened.recoverPendingOperations()).rejects.toThrow(/digest|manifest.*changed/i);
		},
	);

	it.each(["core", "restart"])(
		"rejects all actual corrupted bundle files at %s boundary without semantic rejection",
		async (mode) => {
			const { root, job, store } = await setup();
			const evidence = await delivery(job, {}, ["result.json"]);
			const { adapter } = piReviewer(root);
			const review = await adapter.review(evidence, job);
			const packet = JSON.parse(
				await readFile(reviewPacketPath(root, job.state.frame.jobId, review.reviewerTaskId!), "utf8"),
			) as ReviewPacket;
			const file = join(
				dirname(reviewPacketPath(root, job.state.frame.jobId, review.reviewerTaskId!)),
				packet.resolvedEvidenceRefs[0].path,
			);
			await chmod(file, 0o600);
			await writeFile(file, '{"metric":999}\n');
			if (mode === "core")
				await expect(job.recordReview({ ...review, evidenceId: evidence.id })).rejects.toThrow(
					/integrity|hash|sha/i,
				);
			else
				await expect(
					(await ResearchJob.open(store, job.state.frame.jobId))!.recoverPendingOperations(),
				).rejects.toThrow(/integrity|hash|sha/i);
			expect(
				(await store.readEvents(job.state.frame.jobId)).filter(
					(saved) => String(saved.event.type) === "review_delivery_rejected",
				),
			).toHaveLength(0);
		},
	);

	it.each(["outside", "symlink", "missing"])("refuses %s bundle paths without semantic retry", async (fault) => {
		const { root, job } = await setup();
		const evidence = await delivery(job, {}, ["result.json"]);
		const { adapter } = piReviewer(root);
		const review = await adapter.review(evidence, job);
		const path = reviewPacketPath(root, job.state.frame.jobId, review.reviewerTaskId!);
		const packet = JSON.parse(await readFile(path, "utf8")) as ReviewPacket;
		const target = join(dirname(path), packet.resolvedEvidenceRefs[0].path);
		await rm(target);
		if (fault === "symlink") await symlink(join(root, ".astra/active-job.json"), target);
		if (fault === "outside") packet.resolvedEvidenceRefs[0].path = "../../../../outside";
		if (fault === "outside") {
			await writeFile(path, JSON.stringify(packet));
			const frozenPath = join(dirname(path), "review-target-snapshot.json");
			const frozen = JSON.parse(await readFile(frozenPath, "utf8"));
			frozen.resolvedEvidenceRefs = packet.resolvedEvidenceRefs;
			await writeFile(frozenPath, JSON.stringify(frozen));
		}
		await expect(job.recordReview({ ...review, evidenceId: evidence.id })).rejects.toThrow();
	});

	it("recovers upstream declared refs after failed registration and includes unique repair criteria in Pi", async () => {
		const { root, job, store } = await setup();
		const upstream = await delivery(job, {}, ["pi-session:upstream"]);
		await job.recordReview(reviewFixture(job, { evidenceId: upstream.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(upstream.id, true);
		const evidence = await delivery(job, {
			inputArtifactRefs: [upstream.id],
			repairChecks: [{ issueId: "synthetic", criterion: "repair-only check" }],
		});
		const { adapter, run } = piReviewer(root, async (manifest) => {
			expect(manifest.criteria.map((item) => item.criterion)).toContain("repair-only check");
			manifest.verifiedRefs = ["pi-session:upstream"];
			for (const item of manifest.criteria) item.evidenceRefs = manifest.verifiedRefs;
			await writeReviewerOutputManifest(manifest, root);
		});
		const output = await adapter.review(evidence, job);
		const append = store.append.bind(store);
		const interruption = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (event.type === "review_recorded") throw new Error("injected pre-append interruption");
			return append(id, event);
		});
		await expect(job.recordReview({ ...output, evidenceId: evidence.id })).rejects.toThrow("injected pre-append");
		interruption.mockRestore();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		expect(Object.values(reopened.state.reviews).filter((review) => review.evidenceId === evidence.id)).toHaveLength(
			1,
		);
		expect(run).toHaveBeenCalledOnce();
	});
});
