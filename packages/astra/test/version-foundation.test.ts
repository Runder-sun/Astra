import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { afterEach, expect, it } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { prepareTaskWorkspace } from "../src/task-workspace.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function setup() {
	const root = await mkdtemp(join(tmpdir(), "astra-version-"));
	roots.push(root);
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "versioned research",
		automation: "full",
	});
	return { root, store, job };
}

async function task(job: ResearchJob, stageId: string, objective: string, refs: string[] = []) {
	return job.dispatchTask({
		stageId,
		stageExecutionId: stageId,
		role: "worker",
		objective,
		inputArtifactRefs: refs,
		requiredCanonicalArtifacts: [],
		requiredOutputType: stageId,
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: ["incorrect"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read", "write"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["verified"],
	});
}

async function adopt(job: ResearchJob, stageId: string, objective: string, refs: string[] = [], claims?: unknown[]) {
	const worker = await task(job, stageId, objective, refs);
	await job.setTaskStatus(worker.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: worker.id,
		stageId,
		type: stageId,
		refs: [],
		content:
			stageId === "result-to-claim"
				? {
						scientificOutcome: "supported",
						missionCoverage: "sufficient",
						claims: claims ?? [{ statement: objective, assessment: "supported" }],
						conclusion: objective,
					}
				: { content: objective },
	});
	const rounds = job.definitions[stageId].qualityPolicy?.minPassingReviews ?? 1;
	for (let i = 0; i < rounds; i++)
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
	return job.adoptEvidence(evidence.id);
}

it("does not promote unresolved or unsupported claims to supported graph nodes or edges", async () => {
	const { job } = await setup();
	await adopt(
		job,
		"result-to-claim",
		"unverified claims",
		[],
		[
			{ statement: "pending support", assessment: "unresolved" },
			{ statement: "unsupported generalization", assessment: "unsupported" },
		],
	);
	expect(job.state.graph.acceptedClaimIds).toHaveLength(0);
	const claimIds = Object.values(job.state.graph.nodes)
		.filter((node) => node.kind === "claim")
		.map((node) => node.id);
	expect(
		Object.values(job.state.graph.edges).filter(
			(edge) => claimIds.includes(edge.toNodeId) && edge.kind === "supports",
		),
	).toHaveLength(0);
	expect(job.completionBlockers()).toContain("supported scientific outcome has no accepted research claim");
});

it("replays the full version replacement history with identical current state", async () => {
	const { job, store } = await setup();
	const initial = job.state;
	const code = await adopt(job, "implement-solution", "code A");
	const result = await adopt(job, "result-to-claim", "result A", [code.id]);
	await adopt(job, "paper-write", "paper A", [result.id]);
	await adopt(job, "implement-solution", "code B");
	const expected = (await ResearchJob.open(store, initial.frame.jobId))!.state;
	await store.writeSnapshot(initial);
	expect((await ResearchJob.open(store, initial.frame.jobId))!.state).toEqual(expected);
});

it("rejects unassessed claims at adoption even when a caller bypasses submission validation", async () => {
	const { job } = await setup();
	await expect(adopt(job, "result-to-claim", "invalid claim", [], ["unassessed"])).rejects.toThrow(
		/claim.*assessment/,
	);
	expect(Object.values(job.state.canonical)).toHaveLength(0);
});

it("replays acceptance superseding older evidence in the same revision set", async () => {
	const { job, store } = await setup();
	const first = await task(job, "validation", "revision A");
	await job.setTaskStatus(first.id, "succeeded");
	const old = await job.recordEvidence({
		taskId: first.id,
		stageId: "validation",
		type: "validation",
		refs: [],
		content: { content: "A" },
		currentEvidenceSetId: "revision-set",
	});
	const second = await task(job, "validation", "revision B");
	await job.setTaskStatus(second.id, "succeeded");
	const replacement = await job.recordEvidence({
		taskId: second.id,
		stageId: "validation",
		type: "validation",
		refs: [],
		content: { content: "B" },
		currentEvidenceSetId: "revision-set",
	});
	await job.recordReview(reviewFixture(job, { evidenceId: replacement.id, verdict: "pass", findings: [] }));
	const before = job.state;
	await job.decideEvidence(replacement.id, true);
	await store.writeSnapshot(before);
	const reopened = await ResearchJob.open(store, before.frame.jobId);
	expect(reopened?.state.evidence[old.id]).toMatchObject({ status: "rejected", supersededByTaskId: second.id });
	expect(reopened?.state.evidence[replacement.id].status).toBe("accepted");
});

it("reopens affected progress and removes old supported claims after upstream replacement", async () => {
	const { job } = await setup();
	const code = await adopt(job, "implement-solution", "code A");
	const results = await adopt(job, "result-to-claim", "results A", [code.id]);
	const paper = await adopt(job, "paper-write", "paper A", [results.id]);
	const unrelated = await adopt(job, "literature", "literature");
	expect(job.state.graph.acceptedClaimIds).toHaveLength(1);
	await adopt(job, "implement-solution", "code B");
	expect(job.state.canonical[results.id].status).toBe("stale");
	expect(job.state.canonical[paper.id].status).toBe("stale");
	expect(job.state.canonical[unrelated.id].status).toBe("active");
	expect(job.state.canonicalRoute.stageArtifactIds["paper-write"]).toBeUndefined();
	expect(job.state.graph.acceptedClaimIds).toHaveLength(0);
	expect(job.state.frame.scientificOutcome).toBe("pending");
	expect(job.state.reviews).not.toEqual({});
	expect(job.status().progress).toContainEqual(
		expect.objectContaining({ stageId: "paper-write", status: "pending", invalidatedBy: code.id }),
	);
	expect(job.completionBlockers().some((blocker) => blocker.includes("paper-write requires revalidation"))).toBe(true);
	await expect(task(job, "paper-write", "reuse stale", [paper.id])).rejects.toThrow(/stale|invalid/);
});

it("reopens with the original stage policy", async () => {
	const { root, store } = await setup();
	const definitions = structuredClone(DEFAULT_STAGES);
	definitions[0].acceptanceChecks = ["frozen project-specific criterion"];
	const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "frozen criteria", definitions });
	const reopened = await ResearchJob.open(store, job.state.frame.jobId);
	expect(reopened?.definitions.validation.acceptanceChecks).toEqual(["frozen project-specific criterion"]);
});

it("records Git HEAD and dirty bytes without modifying the user's index", async () => {
	const { root, job } = await setup();
	const git = (...args: string[]) => promisify(execFile)("git", ["-C", root, ...args]);
	await git("init");
	await writeFile(join(root, "code.py"), "print(1)\n");
	await git("add", "code.py");
	await git("-c", "user.name=Astra test", "-c", "user.email=astra@example.invalid", "commit", "-m", "initial");
	await writeFile(join(root, "code.py"), "print(2)\n");
	await git("add", "code.py");
	await writeFile(join(root, "config.json"), '{"seed":42}\n');
	const before = (await git("diff", "--cached")).stdout;
	const worker = await task(job, "implement-solution", "capture dirty code");
	const cwd = await prepareTaskWorkspace(worker, job);
	const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
	expect(context.version.git.head).toBe((await git("rev-parse", "HEAD")).stdout.trim());
	expect(context.version.git.dirty).toBe(true);
	const gitRoot = join(root, ".astra", "jobs", worker.jobId, "versions", "git");
	expect(await readFile(join(gitRoot, context.version.git.patchSha256), "utf8")).toContain("+print(2)");
	expect(context.version.git.untracked).toHaveLength(1);
	expect(await readFile(join(gitRoot, context.version.git.untracked[0].sha256), "utf8")).toBe('{"seed":42}\n');
	await prepareTaskWorkspace(worker, job);
	expect((await git("diff", "--cached")).stdout).toBe(before);
	await writeFile(join(root, "code.py"), "print(3)\n");
	await expect(prepareTaskWorkspace(worker, job)).rejects.toThrow(/version|changed/);
});

it("rejects a review bound to a different version", async () => {
	const { job } = await setup();
	const worker = await task(job, "validation", "review version");
	await prepareTaskWorkspace(worker, job);
	await job.setTaskStatus(worker.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: worker.id,
		stageId: worker.stageId,
		type: "validation",
		refs: [],
		content: { content: "version A" },
	});
	await expect(
		job.recordReview(
			reviewFixture(job, {
				evidenceId: evidence.id,
				verdict: "pass",
				findings: [],
				targetVersionHash: "different-version",
			}),
		),
	).rejects.toThrow(/version/);
	const review = await job.recordReview(
		reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
	);
	expect(review.targetVersionHash).toBe(evidence.versionHash);
	expect(evidence.taskVersion?.hash).toBe(job.state.tasks[worker.id].version?.hash);
});

it("rejects pending evidence whose input was replaced before acceptance", async () => {
	const { job } = await setup();
	const code = await adopt(job, "implement-solution", "code A");
	const worker = await task(job, "run", "pending experiment", [code.id]);
	await job.setTaskStatus(worker.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: worker.id,
		stageId: "run",
		type: "run",
		refs: [],
		content: { content: "old results" },
	});
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await adopt(job, "implement-solution", "code B");
	await expect(job.decideEvidence(evidence.id, true)).rejects.toThrow(/stale/);
	await expect(task(job, "monitor", "indirect stale", [evidence.id])).rejects.toThrow(/stale/);
});
