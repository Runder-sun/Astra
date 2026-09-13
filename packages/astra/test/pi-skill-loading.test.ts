import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadSkillsFromDir } from "@earendil-works/pi-coding-agent";
import { describe, expect, it, vi } from "vitest";
import { writeMainDecisionManifest, writeStagePlanManifest } from "../src/contracts.ts";
import { PiChildSessionRunner, PiMainAgentAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import type { Evidence } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const skillsRoot = fileURLToPath(new URL("../skills/astra", import.meta.url));

describe("Pi-native Astra skills", () => {
	it("loads packaged Astra skills through Pi's skill loader", () => {
		const loaded = loadSkillsFromDir({ dir: skillsRoot, source: "astra" });

		expect(loaded.diagnostics).toEqual([]);
		expect(loaded.skills.map((skill) => skill.name).sort()).toEqual([
			"astra-experiment-plan",
			"astra-idea",
			"astra-implement-solution",
			"astra-literature",
			"astra-main-agent",
			"astra-monitor",
			"astra-novelty",
			"astra-paper-compile",
			"astra-paper-plan",
			"astra-paper-write",
			"astra-refine",
			"astra-research-review",
			"astra-result-to-claim",
			"astra-reviewer",
			"astra-run",
			"astra-validation",
			"astra-worker",
		]);
	});

	it("teaches paper writers to submit a manuscript path instead of embedding the manuscript", async () => {
		const skill = await readFile(join(skillsRoot, "paper-write.md"), "utf8");

		expect(skill).toContain("`paper-manuscript.md`");
		expect(skill).toContain("`content.manuscript` must be the relative path");
		expect(skill).toContain("Do not create `paper-write-output.json`");
		expect(skill).toContain("Do not embed the manuscript text");
	});

	it("keeps final research-review submissions compact", async () => {
		const skill = await readFile(join(skillsRoot, "research-review.md"), "utf8");

		expect(skill).toContain("under 3,500 characters");
		expect(skill).toContain("at most 3 strengths");
		expect(skill).toContain("at most 5 claim-audit entries");
	});

	it("treats installable system runtime prerequisites as implementation work", async () => {
		const skill = await readFile(join(skillsRoot, "implement-solution.md"), "utf8");

		expect(skill).toContain("missing system executable, shared library, display service, or driver-side utility");
		expect(skill).toContain("install vulkan-tools");
		expect(skill).toContain("start Xorg");
		expect(skill).toContain("implementation action, not a reason to submit early");
		expect(skill).toContain("A blocked `sudo` command is not a final setup blocker");
		expect(skill).toContain("`$ASTRA_RESOURCE_ROOT/system`");
		expect(skill).toContain("`PATH` or `LD_LIBRARY_PATH`");
	});

	it("binds only the current stage and role skills to a Pi child session", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-skills-"));
		try {
			const launcherPath = join(root, "capture-args.mjs");
			await writeFile(
				launcherPath,
				'console.log(JSON.stringify({ type: "captured_args", args: process.argv.slice(2) }));\n',
				"utf8",
			);
			const runner = new PiChildSessionRunner({ launcherPath, sessionDir: join(root, "sessions") });

			const result = await runner.run(root, "job-skills", "task-skills", 1, "worker", "run task", {
				ASTRA_STAGE_ID: "literature",
			});

			expect(result.exitCode).toBe(0);
			const event = result.jsonEvents[0] as { type: string; args: string[] };
			expect(event.type).toBe("captured_args");
			expect(event.args).toContain("--no-skills");
			const skillPaths = event.args.flatMap((arg, index) =>
				arg === "--skill" && event.args[index + 1] ? [event.args[index + 1]] : [],
			);
			expect(skillPaths.map((path) => basename(path)).sort()).toEqual(["literature.md", "worker.md"]);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it.each([
		{
			name: "stage planning",
			env: { ASTRA_STAGE_ID: "refine", ASTRA_STAGE_PLAN_ID: "plan-refine" },
			expectedTools: "astra_submit_stage_plan",
		},
		{
			name: "main decision",
			env: { ASTRA_STAGE_ID: "refine", ASTRA_DECISION_TYPE: "adoption" },
			expectedTools: "astra_submit_main_decision",
		},
	])("gives a $name child only its terminal tool", async ({ env, expectedTools }) => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-main-tools-"));
		try {
			const launcherPath = join(root, "capture-args.mjs");
			await writeFile(
				launcherPath,
				'console.log(JSON.stringify({ type: "captured_args", args: process.argv.slice(2) }));\n',
				"utf8",
			);
			const runner = new PiChildSessionRunner({ launcherPath, sessionDir: join(root, "sessions") });

			const result = await runner.run(root, "job-main-tools", "task-main-tools", 1, "main-agent", "act", env);

			const event = result.jsonEvents[0] as { type: string; args: string[] };
			const toolsIndex = event.args.indexOf("--tools");
			expect(event.args[toolsIndex + 1]).toBe(expectedTools);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("tells the stage planner that sibling tasks cannot consume same-plan outputs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-planner-prompt-"));
		try {
			const job = await ResearchJob.create(new MemoryAstraStore(), {
				jobId: "job_planner_prompt",
				objective: "test isolated planning",
				workspaceRoot: root,
				automation: "full",
			});
			const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
			let plannerPrompt = "";
			vi.spyOn(runner, "run").mockImplementation(async (_cwd, jobId, _taskId, _attempt, _role, prompt, env = {}) => {
				plannerPrompt = prompt;
				const planId = env.ASTRA_STAGE_PLAN_ID;
				const decisionRef = env.ASTRA_DECISION_REF;
				if (!planId || !decisionRef) throw new Error("planner identity is missing");
				await writeStagePlanManifest(
					{
						schemaVersion: "astra.stage_plan_manifest.v1",
						id: planId,
						jobId,
						stageId: "validation",
						decisionRef,
						tasks: [
							{
								key: "validate",
								objective: "validate the question",
								inputArtifactRefs: [],
								requiredOutputFields: job.definitions.validation.requiredOutputFields,
								acceptanceChecks: ["question is bounded"],
								failureSignals: ["question is vague"],
								successCriteria: ["question is falsifiable"],
							},
						],
						rationale: "one bounded task",
						sessionRef: "pi-session:planner",
						createdAt: new Date().toISOString(),
					},
					root,
				);
				return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [] };
			});

			await new PiMainAgentAdapter(runner, root).planStage(job);

			expect(plannerPrompt).toContain("run concurrently in isolated workspaces");
			expect(plannerPrompt).toContain("cannot consume another task's output from the same plan");
			expect(plannerPrompt).toContain("plan only the prerequisite task");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("tells the main agent that a grounded negative final review is valid evidence", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-negative-review-"));
		try {
			const job = await ResearchJob.create(new MemoryAstraStore(), {
				jobId: "job_negative_review",
				objective: "retain an honest final assessment",
				workspaceRoot: root,
				automation: "full",
			});
			const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
			let decisionPrompt = "";
			vi.spyOn(runner, "run").mockImplementation(async (_cwd, jobId, _taskId, _attempt, _role, prompt, env = {}) => {
				decisionPrompt = prompt;
				const decisionRef = env.ASTRA_DECISION_REF;
				const evidenceId = env.ASTRA_EVIDENCE_ID;
				if (!decisionRef || !evidenceId) throw new Error("decision identity is missing");
				await writeMainDecisionManifest(
					{
						schemaVersion: "astra.main_agent_decision_manifest.v1",
						manifestId: `manifest_${decisionRef}`,
						jobId,
						decisionType: "evidence",
						decisionRef,
						evidenceId,
						decision: "accept",
						rationale: "the negative assessment is complete and grounded",
						sessionRef: "pi-session:main-agent",
						createdAt: new Date().toISOString(),
					},
					root,
				);
				return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [] };
			});
			const evidence: Evidence = {
				id: "evidence_negative_review",
				taskId: "task_negative_review",
				stageId: "research-review",
				type: "research-review",
				content: {
					verdict: "FAIL",
					strengths: [],
					weaknesses: ["claim evidence is missing"],
					claimAudit: [],
					requiredRepairs: ["restore evidence refs"],
				},
				refs: ["pi-session:review-worker"],
				checksum: "negative-review-checksum",
				createdAt: new Date().toISOString(),
				status: "candidate",
			};

			await new PiMainAgentAdapter(runner, root).decideEvidence(evidence, job);

			expect(decisionPrompt).toContain("A FAIL verdict is valid negative evidence");
			expect(decisionPrompt).toContain("do not reject it merely because the assessment is negative");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("projects route context by durable ref instead of reinlining full artifact history", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-route-projection-"));
		try {
			const job = await ResearchJob.create(new MemoryAstraStore(), {
				jobId: "job_route_projection",
				objective: "keep route context bounded",
				workspaceRoot: root,
				automation: "full",
			});
			const task = await job.dispatchTask({
				stageId: "validation",
				stageExecutionId: "stage_exec_validation",
				role: "worker",
				objective: "produce a large validation artifact",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: job.definitions.validation.acceptanceChecks,
				failureSignals: job.definitions.validation.failureSignals,
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: job.definitions.validation.acceptanceChecks,
			});
			await job.setTaskStatus(task.id, "succeeded");
			const sentinel = `unbounded-tail-${"x".repeat(200_000)}`;
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: {
					researchQuestion: "Is the route projection bounded?",
					scope: sentinel,
					nonGoals: [],
					acceptanceCriteria: ["prompt remains bounded"],
					falsifiableNextStep: "inspect the captured prompt",
				},
				refs: ["pi-session:large-evidence"],
			});
			await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
			await job.decideEvidence(evidence.id, true, "accept-large-evidence");
			const artifact = await job.adoptEvidence(evidence.id);
			await job.recordUserGuidance("Prefer the route that tests one-GPU reproducibility");
			const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
			let routePrompt = "";
			vi.spyOn(runner, "run").mockImplementation(async (_cwd, jobId, _taskId, _attempt, _role, prompt, env = {}) => {
				routePrompt = prompt;
				const decisionRef = env.ASTRA_DECISION_REF;
				if (!decisionRef) throw new Error("route decision identity is missing");
				await writeMainDecisionManifest(
					{
						schemaVersion: "astra.main_agent_decision_manifest.v1",
						manifestId: `manifest_${decisionRef}`,
						jobId,
						decisionType: "route",
						decisionRef,
						stageId: "validation",
						routeAction: "continue",
						evidenceRefs: [artifact.id],
						rationale: "capture bounded route context",
						sessionRef: "pi-session:main-agent",
						createdAt: new Date().toISOString(),
					},
					root,
				);
				return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [] };
			});

			await new PiMainAgentAdapter(runner, root).decideRoute(job);

			expect(routePrompt.length).toBeLessThan(64 * 1024);
			expect(routePrompt).toContain(artifact.id);
			expect(routePrompt).toContain(artifact.checksum);
			expect(routePrompt).toContain("Is the route projection bounded?");
			expect(routePrompt).toContain("scientific outcome has not been recorded");
			expect(routePrompt).toContain("Prefer the route that tests one-GPU reproducibility");
			expect(routePrompt).not.toContain(sentinel.slice(-1024));
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("rotates the persistent main-agent session after a failed protocol round", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-main-recovery-"));
		try {
			const job = await ResearchJob.create(new MemoryAstraStore(), {
				jobId: "job_main_recovery",
				objective: "recover main-agent control after a missing manifest",
				workspaceRoot: root,
				automation: "full",
			});
			const staleSessionId = job.state.mainAgentSessionId;
			await job.recordChildSession({
				sessionId: staleSessionId,
				role: "main-agent",
				taskId: "decision-route-stale",
				status: "failed",
				attempt: 1,
				error: "decision manifest missing",
				updatedAt: new Date().toISOString(),
			});
			const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
			let recoverySessionId: string | undefined;
			vi.spyOn(runner, "run").mockImplementation(
				async (_cwd, jobId, _taskId, _attempt, _role, _prompt, env = {}) => {
					recoverySessionId = env.ASTRA_SESSION_ID;
					const decisionRef = env.ASTRA_DECISION_REF;
					if (!decisionRef) throw new Error("route decision identity is missing");
					await writeMainDecisionManifest(
						{
							schemaVersion: "astra.main_agent_decision_manifest.v1",
							manifestId: `manifest_${decisionRef}`,
							jobId,
							decisionType: "route",
							decisionRef,
							stageId: "validation",
							routeAction: "continue",
							evidenceRefs: [],
							rationale: "recover with a clean protocol session",
							sessionRef: `pi-session:${recoverySessionId ?? "missing"}`,
							createdAt: new Date().toISOString(),
						},
						root,
					);
					return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [] };
				},
			);

			await new PiMainAgentAdapter(runner, root).decideRoute(job);

			expect(recoverySessionId).toContain("-recovery-");
			expect(recoverySessionId).not.toBe(staleSessionId);
			expect(job.state.mainAgentSessionId).toBe(recoverySessionId);
			expect(job.state.sessions[recoverySessionId ?? "missing"]?.status).toBe("completed");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("records a failed main-agent session when its decision manifest is missing", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-main-failure-"));
		try {
			const job = await ResearchJob.create(new MemoryAstraStore(), {
				jobId: "job_main_failure",
				objective: "persist a failed decision session",
				workspaceRoot: root,
				automation: "full",
			});
			const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
			vi.spyOn(runner, "run").mockResolvedValue({ exitCode: 0, stdout: "", stderr: "", jsonEvents: [] });
			vi.spyOn(runner, "waitForManifest").mockRejectedValue(new Error("manifest missing"));
			const evidence: Evidence = {
				id: "evidence_missing_manifest",
				taskId: "task_missing_manifest",
				stageId: "validation",
				type: "validation",
				content: { researchQuestion: "bounded" },
				refs: [],
				checksum: "missing-manifest-checksum",
				createdAt: new Date().toISOString(),
				status: "candidate",
			};

			await expect(new PiMainAgentAdapter(runner, root).decideEvidence(evidence, job)).rejects.toThrow("manifest");
			const session = Object.values(job.state.sessions).find((entry) =>
				entry.taskId?.startsWith("decision-evidence-"),
			);
			expect(session?.status).toBe("failed");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
});
