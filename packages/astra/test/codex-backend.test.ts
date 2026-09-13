import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import * as codingAgent from "@earendil-works/pi-coding-agent";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { fetchAstraFixtureOpenAlex } from "../src/fixture-provider.ts";
import { runAstra } from "../src/launcher.ts";
import { ResearchJob } from "../src/research.ts";
import { parseResearchControlArgs, researchBackend, runResearchControl } from "../src/research-control.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import { taskResourcePath } from "../src/task-workspace.ts";

vi.mock("@earendil-works/pi-coding-agent", async (importOriginal) => ({
	...(await importOriginal<typeof codingAgent>()),
	main: vi.fn(),
}));

const roots: string[] = [];
const fixture = fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url));
async function workspace(): Promise<string> {
	const root = await mkdtemp(join(tmpdir(), "astra-codex-integration-"));
	roots.push(root);
	return root;
}
afterEach(async () => {
	vi.restoreAllMocks();
	vi.clearAllMocks();
	vi.unstubAllEnvs();
	vi.unstubAllGlobals();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("Codex research backend", () => {
	it.each(["review-contradiction", "review-preflight"])(
		"keeps report validity separate from a negative scientific assessment (%s)",
		async (mode) => {
			const root = await workspace();
			const job = await ResearchJob.create(new JsonlAstraStore(root), {
				objective: "Validate a negative review report",
				workspaceRoot: root,
			});
			const task = await job.dispatchTask({
				stageId: "research-review",
				stageExecutionId: "research-review",
				role: "worker",
				objective: "Audit the research and report needed repairs",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "research-review",
				requiredOutputFields: job.definitions["research-review"].requiredOutputFields,
				acceptanceChecks: ["blocking weaknesses become explicit repairs"],
				failureSignals: ["missing repairs"],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["blocking weaknesses become explicit repairs"],
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: task.requiredOutputType,
				content: {
					verdict: "blocked",
					scientificOutcome: "partially-supported",
					missionCoverage: "insufficient",
					strengths: [],
					weaknesses: ["clipped text"],
					claimAudit: [],
					requiredRepairs: ["repair clipped text"],
				},
				refs: [],
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", mode);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			if (mode === "review-contradiction") {
				await expect(adapters.review(evidence, job)).rejects.toThrow("failed frozen criterion");
			} else {
				expect((await adapters.review(evidence, job)).verdict).toBe("pass");
				expect(job.state.evidence[evidence.id].content).toMatchObject({
					verdict: "blocked",
					requiredRepairs: ["repair clipped text"],
				});
				expect(job.completionBlockers()).toContain("no passing whole-research review");
				expect(Object.values(job.state.sessions)).toHaveLength(1);
			}
		},
	);
	it.each([false, true])(
		"repairs missing refs inside one session and rechecks the final submission (invalid final: %s)",
		async (invalidFinal) => {
			const root = await workspace();
			const job = await ResearchJob.create(new JsonlAstraStore(root), {
				objective: "Repair a submission without rerunning research",
				workspaceRoot: root,
			});
			const task = await job.dispatchTask({
				stageId: "literature",
				stageExecutionId: "literature",
				role: "worker",
				objective: "Return a source-grounded review",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "literature",
				requiredOutputFields: ["synthesis"],
				acceptanceChecks: ["retain source refs"],
				failureSignals: ["missing source refs"],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read", "astra_search_papers"],
				writeAuthority: "none",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["retain source refs"],
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "submission-preflight");
			vi.stubEnv("ASTRA_FAKE_CODEX_INVALID_FINAL", invalidFinal ? "1" : "0");
			vi.stubGlobal("fetch", fetchAstraFixtureOpenAlex);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			if (invalidFinal) {
				await expect(adapters.run(task, job)).rejects.toThrow("missing required output fields: synthesis");
			} else {
				const result = await adapters.run(task, job);
				expect(result.refs).toEqual(expect.arrayContaining(["openalex:W1", "openalex:W2", "openalex:W3"]));
				expect(Object.values(job.state.sessions)).toHaveLength(1);
				expect(Object.values(job.state.sessions)[0].status).toBe("completed");
				expect(Object.values(job.state.evidence)).toHaveLength(0);
			}
		},
	);
	it("constrains source-required submissions before host source validation", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Retain citations",
			workspaceRoot: root,
		});
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			objective: "Review existing sources",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["retain sources"],
			failureSignals: ["missing sources"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["retain sources"],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		await expect(adapters.run(task, job)).rejects.toThrow("requested research schema");
	});
	it.runIf(process.platform === "linux" && existsSync("/etc/texmf") && existsSync("/var/lib/texmf"))(
		"mounts system TeX configuration read-only for compilation workers",
		async () => {
			const root = await workspace();
			const job = await ResearchJob.create(new JsonlAstraStore(root), {
				objective: "Compile a paper",
				workspaceRoot: root,
			});
			const task = await job.dispatchTask({
				stageId: "paper-compile",
				stageExecutionId: "compile",
				role: "worker",
				objective: "Compile the manuscript",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "paper-compile",
				requiredOutputFields: ["artifact"],
				acceptanceChecks: ["PDF exists"],
				failureSignals: ["missing PDF"],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read", "bash"],
				writeAuthority: "workspace-write",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["PDF exists"],
			});
			const cwd = join(root, ".astra", "jobs", task.jobId, "workspaces", task.id);
			await mkdir(cwd, { recursive: true });
			await writeFile(join(cwd, "paper.pdf"), "Offline permission fixture, not a research PDF");
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "ok");
			vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
			vi.stubEnv(
				"ASTRA_FAKE_CODEX_OUTPUT",
				JSON.stringify({
					artifactType: "paper-compile",
					contentJson: JSON.stringify({ artifact: "paper.pdf" }),
					refs: [{ kind: "artifact", ref: "paper.pdf", summary: "Offline permission fixture" }],
				}),
			);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			await adapters.run(task, job);
			const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line) as { method: string; params: { config?: Record<string, unknown> } });
			const filesystem = calls.find((call) => call.method === "thread/start")?.params.config?.[
				"permissions.astra.filesystem"
			] as Record<string, string>;
			expect(filesystem["/etc/texmf"]).toBe("read");
			expect(filesystem["/var/lib/texmf"]).toBe("read");
			expect(filesystem["/etc"]).toBeUndefined();
			expect(filesystem["/var/lib"]).toBeUndefined();
		},
	);
	it("gives reviewers read-only access to declared runtime results without other task directories", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Inspect actual results",
			workspaceRoot: root,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		const output = await adapters.run(task, job);
		await job.setTaskStatus(task.id, "succeeded");
		const resourceRoot = taskResourcePath(root, task.jobId, task.id);
		const unrelatedRoot = taskResourcePath(root, task.jobId, "unrelated");
		await mkdir(resourceRoot, { recursive: true });
		await mkdir(unrelatedRoot, { recursive: true });
		await writeFile(join(resourceRoot, "results.csv"), "replicate,error\n0,0.1\n");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { ...(output.content as object), runtimeResults: join(resourceRoot, "results.csv") },
			refs: output.refs,
		});
		const review = await adapters.review(evidence, job);
		const snapshot = JSON.parse(
			await readFile(
				join(root, ".astra", "jobs", task.jobId, "tasks", review.reviewerTaskId!, "review-target-snapshot.json"),
				"utf8",
			),
		);
		expect(snapshot.resources).toEqual(
			expect.arrayContaining([expect.objectContaining({ artifactId: evidence.id, root: resourceRoot })]),
		);
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map(
				(line) =>
					JSON.parse(line) as {
						method?: string;
						params?: { config?: Record<string, unknown>; developerInstructions?: string };
					},
			);
		const reviewerStart = calls.reverse().find((call) => call.method === "thread/start");
		expect(reviewerStart?.params?.developerInstructions).toContain("Astra Validation Stage");
		expect(reviewerStart?.params?.developerInstructions).toContain("Astra Reviewer Role");
		const filesystem = reviewerStart?.params?.config?.["permissions.astra.filesystem"] as Record<string, string>;
		expect(filesystem[resourceRoot]).toBe("read");
		expect(filesystem[unrelatedRoot]).toBeUndefined();
		expect(filesystem[join(root, ".astra", "jobs", task.jobId)]).toBeUndefined();
		expect(Object.values(filesystem)).not.toContain("write");
		expect(filesystem["/etc/texmf"]).toBeUndefined();
		expect(filesystem["/var/lib/texmf"]).toBeUndefined();
		await rm(resourceRoot, { recursive: true });
		await symlink(unrelatedRoot, resourceRoot);
		await expect(adapters.review(evidence, job)).rejects.toBeInstanceOf(NonRetryableResearchError);
		expect(
			Object.values(job.state.tasks)
				.filter((item) => item.role === "reviewer")
				.at(-1)?.status,
		).toBe("failed");
		const supervisor = new ResearchSupervisor(job, new JsonlAstraStore(root), {
			worker: adapters,
			reviewer: adapters,
			mainAgent: adapters,
			maxParallel: 1,
		});
		const before = Object.keys(job.state.tasks).length;
		await supervisor.tick();
		expect(job.state.paused).toBe(true);
		expect(Object.keys(job.state.tasks)).toHaveLength(before + 1);
		await supervisor.tick();
		expect(Object.keys(job.state.tasks)).toHaveLength(before + 1);
	});
	it("audits every Codex decision log and rejects missing, incomplete or misbound records", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, { objective: "Audit native decisions", workspaceRoot: root });
		const jobRoot = join(root, ".astra", "jobs", job.state.frame.jobId);
		await writeFile(join(jobRoot, "backend.json"), JSON.stringify({ backend: "codex" }));
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		await adapters.planStage(job);
		const firstLog = Object.values(job.state.sessions)[0].sessionFile!;
		const original = await readFile(firstLog, "utf8");
		await job.pause("test restart");
		await job.resume();
		await adapters.planStage(job);
		const audit = () =>
			JSON.parse(
				execFileSync(
					process.execPath,
					[
						fileURLToPath(new URL("../../../scripts/audit-astra-run.mjs", import.meta.url)),
						root,
						job.state.frame.jobId,
					],
					{ encoding: "utf8" },
				),
			);
		const valid = audit();
		expect(valid.runtimeIntegrity.codexSessionsVerified).toBe(true);
		expect(valid.runtimeIntegrity.recoveryCheckpointed).toBe(true);
		expect(valid.sessions.files).toBe(2);
		expect(valid.sessions.models).toEqual(["gpt-5.6-luna"]);
		expect(valid.passed).toBe(false); // A valid session alone does not complete research.
		for (const corrupt of [
			original.replaceAll('"modelProvider":"openai"', '"modelProvider":"untrusted"'),
			original.replaceAll('"permissionProfile":"astra"', '"permissionProfile":"unrestricted"'),
			original
				.split("\n")
				.filter((line) => !line.includes('"method":"turn/completed"'))
				.join("\n"),
		]) {
			await writeFile(firstLog, corrupt);
			expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(false);
		}
		await rm(firstLog);
		expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(false);
	});
	it("indexes large evidence for main-agent decisions without truncating original state", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			objective: "Inspect evidence without repeatedly dumping it",
			workspaceRoot: root,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		await job.setTaskStatus(task.id, "succeeded");
		const content = { rawResults: "measurements\n".repeat(20000) };
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: "validation",
			content,
			refs: ["codex-session:fixture"],
		});
		const guidance = await job.recordUserGuidance("Preserve negative results and keep the fixed CPU-only scope");
		await adapters.decideEvidence(evidence, job);
		const contextRoot = join(root, ".astra", "jobs", task.jobId, "main-agent", "codex-context");
		const indexText = await readFile(join(contextRoot, "research-summary.json"), "utf8");
		expect(Buffer.byteLength(indexText)).toBeLessThan(20000);
		expect(JSON.parse(indexText)).toMatchObject({
			activeCapability: { id: "validation" },
			evidence: [{ id: evidence.id, taskId: task.id }],
			userGuidance: expect.arrayContaining([guidance]),
			fullStatePath: "research-context.json",
		});
		const full = JSON.parse(await readFile(join(contextRoot, "research-context.json"), "utf8"));
		expect(full.state.evidence[evidence.id].content).toEqual(content);
		expect(await readFile(join(root, "requests.jsonl"), "utf8")).toContain("Read research-summary.json first");
	});
	it.each(["openalex", "crossref", "codex-web"])(
		"completes all capabilities using %s with search continuation, executable repairs, paper delivery and durable review gates",
		async (provider) => {
			const root = await workspace();
			const store = new JsonlAstraStore(root);
			let job = await ResearchJob.create(store, {
				objective: "Offline full research contract",
				workspaceRoot: root,
				automation: "full",
				requiredArtifactTypes: ["paper-write", "paper-compile"],
				maxTasks: 128,
				maxTurns: 256,
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "full-research");
			vi.stubEnv("ASTRA_FAKE_CODEX_WEB", provider === "codex-web" ? "1" : "0");
			vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
			const fetcher = vi.fn(async (input: string | URL) => {
				if (provider === "codex-web") return new Response("metadata outage", { status: 503 });
				if (provider === "openalex") return fetchAstraFixtureOpenAlex(input);
				if (new URL(input).hostname === "api.openalex.org") return new Response("quota exhausted", { status: 429 });
				if (new URL(input).hostname !== "api.crossref.org") throw new Error("Unexpected literature provider");
				return Response.json({
					message: {
						items: [1, 2, 3].map((index) => ({
							DOI: `10.1000/paper-${index}`,
							title: [`Fixture paper ${index}`],
						})),
					},
				});
			});
			vi.stubGlobal("fetch", fetcher);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			for (let tick = 0; tick < 60 && job.state.frame.status !== "completed"; tick++) {
				const supervisor = new ResearchSupervisor(job, store, {
					worker: adapters,
					reviewer: adapters,
					mainAgent: adapters,
					maxParallel: 1,
				});
				await supervisor.tick();
				expect(
					job.state.paused,
					JSON.stringify({
						frame: job.state.frame,
						sessions: Object.values(job.state.sessions).filter((session) => session.status === "failed"),
					}),
				).toBe(false);
				job = (await ResearchJob.open(store, job.state.frame.jobId))!;
			}
			expect(
				job.state.frame.status,
				JSON.stringify({ frame: job.state.frame, blockers: job.completionBlockers() }),
			).toBe("completed");
			expect(job.completionBlockers()).toEqual([]);
			expect(fetcher).toHaveBeenCalledTimes(provider === "openalex" ? 1 : provider === "crossref" ? 2 : 3);
			expect(Object.keys(job.state.canonicalRoute.stageArtifactIds)).toHaveLength(14);
			expect(
				Object.values(job.state.searchBatches)
					.filter((batch) => batch.stageId === "idea")
					.map((batch) => batch.round)
					.sort(),
			).toEqual([1, 2]);
			expect(Object.keys(job.state.discardedCandidates).length).toBeGreaterThan(0);
			expect(Object.values(job.state.obligations).some((obligation) => obligation.status === "resolved")).toBe(true);
			for (const stage of ["result-to-claim", "research-review"]) {
				const artifact = job.state.canonical[job.state.canonicalRoute.stageArtifactIds[stage]];
				expect(
					Object.values(job.state.reviews).filter(
						(review) => review.evidenceId === artifact.evidenceId && review.verdict === "pass",
					),
				).toHaveLength(2);
			}
			await writeFile(
				join(root, ".astra", "jobs", job.state.frame.jobId, "backend.json"),
				JSON.stringify({ backend: "codex" }),
			);
			const audit = JSON.parse(
				execFileSync(
					process.execPath,
					[
						fileURLToPath(new URL("../../../scripts/audit-astra-run.mjs", import.meta.url)),
						root,
						job.state.frame.jobId,
					],
					{ encoding: "utf8" },
				),
			);
			expect(audit.passed, JSON.stringify({ checks: audit.contractChecks, sessions: audit.sessions })).toBe(true);
		},
		60000,
	);
	it("resumes retrieved sources and a rate-limited reviewer without creating another task", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			objective: "Resume verifiable work",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
		vi.stubGlobal("fetch", fetchAstraFixtureOpenAlex);
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		const sourceTask = { ...task, allowedTools: [...task.allowedTools, "astra_search_papers"] };
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "literature-interrupt");
		await expect(adapters.run(sourceTask, job)).rejects.toThrow("test provider refusal");
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "literature-resume");
		const output = await adapters.run(sourceTask, job);
		expect(output.refs).toContain("openalex:W1");
		const unrelated = await job.dispatchTask({
			...sourceTask,
			id: "unrelated_sources",
			replayKey: "unrelated_sources",
		});
		await expect(adapters.run(unrelated, job)).rejects.toThrow("not retrieved in this task");
		await job.setTaskStatus(task.id, "running");
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: output.content,
			refs: output.refs,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "rate-limit");
		await expect(adapters.review(evidence, job)).rejects.toThrow("test provider refusal");
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const reviewed = await adapters.review(evidence, job);
		expect(reviewed.verdict).toBe("pass");
		expect(Object.values(job.state.tasks).filter((task) => task.role === "reviewer")).toHaveLength(1);
	});
	it.each(["valid", "foreign", "paraphrased"])(
		"governs all role sessions and validates %s reviews",
		async (reviewMode) => {
			const root = await workspace();
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				objective: "Bound a scientific question",
				workspaceRoot: root,
				automation: "full",
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
			vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
			vi.stubEnv("ASTRA_FAKE_CODEX_FOREIGN_REVIEW_REF", reviewMode === "foreign" ? "1" : "0");
			vi.stubEnv("ASTRA_FAKE_CODEX_PARAPHRASE", reviewMode === "paraphrased" ? "1" : "0");
			vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			const supervisor = new ResearchSupervisor(job, store, {
				worker: adapters,
				reviewer: adapters,
				mainAgent: adapters,
				maxParallel: 1,
			});
			await supervisor.tick();
			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			if (reviewMode !== "valid") {
				expect(reopened?.state.paused).toBe(true);
				expect(Object.keys(job.state.canonical)).toHaveLength(0);
				expect(Object.values(job.state.sessions).find((session) => session.role === "reviewer")).toMatchObject({
					status: "failed",
					error: "Codex final output does not match the requested research schema",
				});
				return;
			}
			expect(reopened?.state.frame.userGate).toMatchObject({
				kind: "research",
				question: "Which benchmark should define the scientific scope?",
			});
			expect(Object.keys(job.state.canonical)).toHaveLength(1);
			const sessions = Object.values(job.state.sessions);
			expect(sessions.map((session) => session.role).sort()).toEqual(["main-agent", "reviewer", "worker"]);
			expect(sessions.every((session) => session.status === "completed")).toBe(true);
			expect(new Set(sessions.map((session) => session.sessionId)).size).toBe(3);
			const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line) as { method?: string; params?: { model?: string } });
			expect(calls.filter((call) => call.method === "thread/start")).toHaveLength(3);
			expect(calls.filter((call) => call.method === "thread/resume")).toHaveLength(3);
			expect(
				calls
					.filter((call) => call.method === "thread/start" || call.method === "thread/resume")
					.every((call) => call.params?.model === "gpt-5.6-luna"),
			).toBe(true);
			const artifact = Object.values(job.state.canonical)[0];
			expect(job.state.evidence[artifact.evidenceId].refs[0]).toMatch(/^codex-session:/);
			const reviewer = Object.values(job.state.tasks).find((task) => task.role === "reviewer");
			expect(reviewer?.status).toBe("succeeded");
		},
	);

	it("persists the backend and resumes it without changing to Pi", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, { objective: "backend identity", workspaceRoot: root });
		await mkdir(join(root, ".astra"), { recursive: true });
		await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
		await writeFile(
			join(root, ".astra", "jobs", job.state.frame.jobId, "backend.json"),
			JSON.stringify({ backend: "codex" }),
		);
		expect(await researchBackend({ action: "resume" }, root)).toBe("codex");
		await expect(researchBackend({ action: "resume", backend: "pi" }, root)).rejects.toThrow("Cannot change");
		const status = await runResearchControl({ action: "status" }, root);
		expect(status).toMatchObject({ backend: "codex", costAccounting: "subscription-unavailable" });
		vi.spyOn(process, "cwd").mockReturnValue(root);
		const printed = vi.spyOn(console, "log").mockImplementation(() => {});
		await runAstra(["research", "status"]);
		expect(codingAgent.main).not.toHaveBeenCalled();
		expect(JSON.parse(String(printed.mock.calls[0][0]))).toMatchObject({ backend: "codex" });
	});

	it("preserves Pi's normal launcher modes", async () => {
		await runAstra(["--help"]);
		expect(codingAgent.main).toHaveBeenCalledWith(
			["--help"],
			expect.objectContaining({ extensionFactories: expect.any(Array) }),
		);
	});

	it("parses explicit backend selection and rejects an unsupported dollar limit before creating work", async () => {
		expect(parseResearchControlArgs(["run", "--backend", "codex", "question"])).toEqual({
			action: "run",
			backend: "codex",
			objective: "question",
		});
		expect(() => parseResearchControlArgs(["run", "--backend", "unknown", "question"])).toThrow(
			"unknown Astra backend",
		);
		await expect(
			runResearchControl(
				{ action: "run", backend: "codex", objective: "question", maxCostUsd: 10 },
				await workspace(),
			),
		).rejects.toThrow("USD bill");
	});
});
