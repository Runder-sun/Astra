import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import { prepareTaskWorkspace, taskResourcePath } from "../src/task-workspace.ts";
import type { MainAgentDecisionManifest, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

function decision(
	job: ResearchJob,
	type: MainAgentDecisionManifest["decisionType"],
	fields: Partial<MainAgentDecisionManifest>,
): MainAgentDecisionManifest {
	return {
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: `decision-${type}-${job.state.eventSeq}`,
		jobId: job.state.frame.jobId,
		decisionType: type,
		decisionRef: `decision-${type}-${job.state.eventSeq}`,
		rationale: "test decision",
		sessionRef: "pi-session:test-main",
		createdAt: new Date().toISOString(),
		...fields,
	};
}

async function completeValidation(job: ResearchJob, outputFile?: { name: string; content: string }): Promise<string> {
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "stage_exec_validation",
		role: "worker",
		objective: "validate the question",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["researchQuestion", "scope", "acceptanceCriteria", "falsifiableNextStep"],
		acceptanceChecks: ["question is falsifiable"],
		failureSignals: ["missing scope"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["question is falsifiable"],
	});
	await job.setTaskStatus(task.id, "running");
	await job.setTaskStatus(task.id, "succeeded");
	const refs = ["pi-session:test-worker"];
	if (outputFile) {
		const relativePath = join(".astra", "jobs", job.state.frame.jobId, "workspaces", task.id, outputFile.name);
		await mkdir(
			join(
				job.state.frame.permissions.workspaceRoot,
				".astra",
				"jobs",
				job.state.frame.jobId,
				"workspaces",
				task.id,
			),
			{
				recursive: true,
			},
		);
		await writeFile(join(job.state.frame.permissions.workspaceRoot, relativePath), outputFile.content, "utf8");
		refs.push(relativePath);
	}
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: "validation",
		type: "validation",
		content: {
			researchQuestion: "Does the method improve accuracy?",
			scope: ["offline benchmark"],
			acceptanceCriteria: ["accuracy improves"],
			falsifiableNextStep: "run the benchmark",
		},
		refs,
	});
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true, "decision-validation");
	const artifact = await job.adoptEvidence(evidence.id);
	await job.applyRouteDecision({
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: "route-validation-literature",
		jobId: job.state.frame.jobId,
		decisionType: "route",
		decisionRef: "route-validation-literature",
		stageId: "validation",
		routeAction: "advance",
		targetStageId: "literature",
		evidenceRefs: [artifact.id],
		rationale: "literature is the next useful capability",
		sessionRef: "pi-session:test-main",
		createdAt: new Date().toISOString(),
	});
	return artifact.id;
}

describe("main-agent authored stage planning", () => {
	it("gives every stage an explicit worker budget", () => {
		expect(DEFAULT_STAGES.every((definition) => definition.workerBudget !== undefined)).toBe(true);
		expect(DEFAULT_STAGES.find((definition) => definition.id === "idea")?.workerBudget).toEqual({
			maxTurns: 12,
			maxToolCalls: 24,
			maxRuntimeMs: 600_000,
		});
		expect(DEFAULT_STAGES.find((definition) => definition.id === "implement-solution")?.workerBudget).toEqual({
			maxTurns: 40,
			maxToolCalls: 96,
			maxRuntimeMs: 3_600_000,
		});
	});

	it("dispatches only the main-agent plan and binds upstream canonical artifacts", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-stage-plan-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_stage_plan",
			objective: "study a concrete method",
			workspaceRoot: root,
			automation: "full",
		});
		const validationArtifactId = await completeValidation(job);
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_literature_1",
			jobId: job.state.frame.jobId,
			stageId: "literature",
			decisionRef: "plan-literature-1",
			tasks: [
				{
					key: "primary-search",
					objective: "Find the closest primary literature and identify the unresolved gap",
					inputArtifactRefs: [validationArtifactId],
					requiredOutputFields: ["queryStrategy", "sources", "closestWork", "gaps", "synthesis"],
					acceptanceChecks: ["at least three primary sources are traceable"],
					failureSignals: ["source identifiers are missing"],
					successCriteria: ["closest work and gap are explicit"],
				},
			],
			rationale: "The literature stage needs one focused primary-source pass",
			sessionRef: "pi-session:planner",
			createdAt: new Date().toISOString(),
		};
		const planStage = vi.fn(async () => plan);
		const workerRun = vi.fn(async (task) => ({
			artifactType: task.requiredOutputType,
			content: {
				queryStrategy: ["closest method"],
				sources: ["openalex:W1", "openalex:W2", "doi:10.1/test"],
				closestWork: ["work 1"],
				gaps: ["missing evaluation"],
				synthesis: "A traceable synthesis",
			},
			refs: ["openalex:W1", "openalex:W2", "doi:10.1/test"],
		}));
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: workerRun },
			reviewer: {
				review: async (evidence, currentJob) =>
					reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				planStage,
				decideEvidence: async (evidence) =>
					decision(job, "evidence", { evidenceId: evidence.id, decision: "accept" }),
				decideAdoption: async (evidence) => decision(job, "adoption", { evidenceId: evidence.id, adopt: true }),
				decideSearch: async (batch, evaluations) =>
					decision(job, "search-selection", {
						searchBatchId: batch.id,
						selectedCandidateId: evaluations[0]?.candidateId,
					}),
				decideRoute: async () =>
					decision(job, "route", {
						stageId: "literature",
						routeAction: "continue",
						evidenceRefs: Object.values(job.state.canonicalRoute.stageArtifactIds),
					}),
			},
		});

		await supervisor.tick();

		expect(planStage).toHaveBeenCalledOnce();
		expect(workerRun).toHaveBeenCalledOnce();
		const dispatched = workerRun.mock.calls[0]?.[0];
		expect(dispatched?.objective).toBe(plan.tasks[0]?.objective);
		expect(dispatched?.inputArtifactRefs).toEqual([validationArtifactId]);
		expect(Object.values(job.state.tasks).some((task) => task.objective.includes("worker wave"))).toBe(false);
		expect(job.state.stagePlans[plan.id]?.decisionRef).toBe(plan.decisionRef);
	});

	it("writes a task context containing the mission and upstream canonical content", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-task-context-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_task_context",
			objective: "preserve stage context",
			workspaceRoot: root,
			automation: "full",
		});
		const validationArtifactId = await completeValidation(job);
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "stage_exec_literature",
			role: "worker",
			objective: "trace upstream inputs",
			inputArtifactRefs: [validationArtifactId],
			requiredCanonicalArtifacts: [validationArtifactId],
			requiredOutputType: "literature",
			requiredOutputFields: ["sources"],
			acceptanceChecks: ["sources are traceable"],
			failureSignals: ["missing source refs"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read", "astra_search_papers"],
			writeAuthority: "workspace-write",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["sources are traceable"],
		});

		const workspace = await prepareTaskWorkspace(task, job);
		const context = JSON.parse(await readFile(join(workspace, "ASTRA_TASK_CONTEXT.json"), "utf8"));

		expect(context.mission.objective).toBe("preserve stage context");
		expect(context.mission.permissions.workspaceRoot).toBe(".");
		expect(context.task.scope.workspaceRoot).toBe(".");
		expect(context.inputs.canonicalArtifacts).toHaveLength(1);
		expect(context.inputs.canonicalArtifacts[0]).toMatchObject({ id: validationArtifactId, type: "validation" });
		expect(context.inputs.canonicalArtifacts[0]).not.toHaveProperty("materializationRef");
	});

	it("retains every canonical ref explicitly authored by the main agent", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-task-context-projection-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_task_context_projection",
			objective: "project only stage-relevant context",
			workspaceRoot: root,
			automation: "full",
		});
		const validationArtifactId = await completeValidation(job);
		const seedTask = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "stage_exec_literature",
			role: "worker",
			objective: "seed an unrelated canonical artifact",
			inputArtifactRefs: [validationArtifactId],
			requiredCanonicalArtifacts: [validationArtifactId],
			requiredOutputType: "idea",
			requiredOutputFields: ["ideas"],
			acceptanceChecks: ["idea exists"],
			failureSignals: ["idea missing"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["idea exists"],
		});
		await job.setTaskStatus(seedTask.id, "running");
		await job.setTaskStatus(seedTask.id, "succeeded");
		const seedEvidence = await job.recordEvidence({
			taskId: seedTask.id,
			stageId: "literature",
			type: "idea",
			content: { ideas: ["unrelated"] },
			refs: ["pi-session:seed"],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: seedEvidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(seedEvidence.id, true, "decision-seed");
		const unrelatedArtifact = await job.adoptEvidence(seedEvidence.id);
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "stage_exec_literature",
			role: "worker",
			objective: "consume only literature stage inputs",
			inputArtifactRefs: [validationArtifactId, unrelatedArtifact.id],
			requiredCanonicalArtifacts: [validationArtifactId, unrelatedArtifact.id],
			requiredOutputType: "literature",
			requiredOutputFields: ["sources"],
			acceptanceChecks: ["sources are traceable"],
			failureSignals: ["sources missing"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["sources are traceable"],
		});

		const workspace = await prepareTaskWorkspace(task, job);
		const context = JSON.parse(await readFile(join(workspace, "ASTRA_TASK_CONTEXT.json"), "utf8"));

		expect(context.inputs.canonicalArtifacts.map((artifact: { id: string }) => artifact.id)).toEqual([
			validationArtifactId,
			unrelatedArtifact.id,
		]);
		expect(context.inputs.omittedRefs).toEqual([]);
	});

	it("retains every authored canonical ref for full-chain research review", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-review-context-projection-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_review_context_projection",
			objective: "audit the complete claim-evidence chain",
			workspaceRoot: root,
			automation: "full",
		});
		const validationArtifactId = await completeValidation(job, { name: "report.md", content: "Verified evidence\n" });
		const task = await job.dispatchTask({
			stageId: "research-review",
			stageExecutionId: "stage_exec_research_review",
			role: "worker",
			objective: "audit every canonical artifact selected by the main agent",
			inputArtifactRefs: [validationArtifactId],
			requiredCanonicalArtifacts: [validationArtifactId],
			requiredOutputType: "research-review",
			requiredOutputFields: ["verdict", "strengths", "weaknesses", "claimAudit", "requiredRepairs"],
			acceptanceChecks: ["full claim-evidence chain is checked"],
			failureSignals: ["an authored canonical ref is omitted"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["all selected canonical refs are inspectable"],
		});

		const workspace = await prepareTaskWorkspace(task, job);
		const context = JSON.parse(await readFile(join(workspace, "ASTRA_TASK_CONTEXT.json"), "utf8"));

		expect(context.inputs.canonicalArtifacts.map((artifact: { id: string }) => artifact.id)).toEqual([
			validationArtifactId,
		]);
		expect(context.inputs.omittedRefs).toEqual([]);
		expect(context.inputs.canonicalArtifacts[0]).not.toHaveProperty("content");
		expect(context.inputs.canonicalArtifacts[0].contentPath).toMatch(/^canonical\/.*\.json$/);
		expect(context.inputs.reviewSummaryPath).toBe("review-summary.json");
		const summaryText = await readFile(join(workspace, context.inputs.reviewSummaryPath), "utf8");
		const summary = JSON.parse(summaryText);
		expect(summaryText).toBe(`${JSON.stringify(summary)}\n`);
		expect(summary.artifacts[0].summary.researchQuestion).toBe("Does the method improve accuracy?");
		expect(summary.artifacts[0].files).toEqual([`inputs/${validationArtifactId}/report.md`]);
		expect(summary.artifacts[0].resourceRoots).toEqual([]);
		expect(await readFile(join(workspace, summary.artifacts[0].files[0]), "utf8")).toBe("Verified evidence\n");
		const projected = JSON.parse(
			await readFile(join(workspace, context.inputs.canonicalArtifacts[0].contentPath), "utf8"),
		);
		expect(projected.projection.kind).toBe("canonical-full");
		expect(projected.projection.includedFields).toEqual(["content"]);
		expect(projected.content.researchQuestion).toBe("Does the method improve accuracy?");
	});

	it("copies upstream artifact files into the next isolated task workspace", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-task-input-files-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_task_input_files",
			objective: "run an implemented experiment",
			workspaceRoot: root,
			automation: "full",
		});
		const validationArtifactId = await completeValidation(job, {
			name: "experiment.mjs",
			content: "console.log(JSON.stringify({ accuracy: 1 }));\n",
		});
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "stage_exec_literature",
			role: "worker",
			objective: "consume the upstream executable",
			inputArtifactRefs: [validationArtifactId],
			requiredCanonicalArtifacts: [validationArtifactId],
			requiredOutputType: "literature",
			requiredOutputFields: ["sources"],
			acceptanceChecks: ["input is available"],
			failureSignals: ["input is missing"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "workspace-write",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["input is available"],
		});

		const workspace = await prepareTaskWorkspace(task, job);
		const context = JSON.parse(await readFile(join(workspace, "ASTRA_TASK_CONTEXT.json"), "utf8"));
		const input = context.inputs.files[0];

		expect(input).toMatchObject({ artifactId: validationArtifactId });
		expect(input.sha256).toMatch(/^[a-f0-9]{64}$/);
		expect(await readFile(join(workspace, input.path), "utf8")).toContain("accuracy");
	});

	it("exposes durable task resources to downstream canonical consumers", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-task-resources-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_task_resources",
			objective: "reuse an installed experiment environment",
			workspaceRoot: root,
			automation: "full",
		});
		const validationArtifactId = await completeValidation(job);
		const artifact = job.state.canonical[validationArtifactId];
		const upstreamTaskId = artifact ? job.state.evidence[artifact.evidenceId]?.taskId : undefined;
		if (!upstreamTaskId) throw new Error("upstream canonical task not found");
		const upstreamResourceRoot = taskResourcePath(root, job.state.frame.jobId, upstreamTaskId);
		await mkdir(upstreamResourceRoot, { recursive: true });
		await writeFile(join(upstreamResourceRoot, "environment.json"), '{"python":"3.11"}\n', "utf8");
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "stage_exec_literature",
			role: "worker",
			objective: "reuse the upstream environment",
			inputArtifactRefs: [validationArtifactId],
			requiredCanonicalArtifacts: [validationArtifactId],
			requiredOutputType: "literature",
			requiredOutputFields: ["sources"],
			acceptanceChecks: ["environment is reusable"],
			failureSignals: ["environment is missing"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read", "bash"],
			writeAuthority: "workspace-write",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["environment is reusable"],
		});

		const workspace = await prepareTaskWorkspace(task, job);
		const context = JSON.parse(await readFile(join(workspace, "ASTRA_TASK_CONTEXT.json"), "utf8"));

		expect(context.resources.writableRoot).toBe(taskResourcePath(root, job.state.frame.jobId, task.id));
		expect(context.inputs.resources).toEqual([
			{
				artifactId: validationArtifactId,
				artifactType: "validation",
				taskId: upstreamTaskId,
				root: upstreamResourceRoot,
				envVar: "ASTRA_INPUT_RESOURCE_ROOT_0",
			},
		]);
		expect(await readFile(join(context.inputs.resources[0].root, "environment.json"), "utf8")).toContain("3.11");
	});

	it("moves a failed attempt resource root into its retry", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-task-resource-retry-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_task_resource_retry",
			objective: "reuse partial environment setup",
			workspaceRoot: root,
			automation: "full",
		});
		const first = await job.dispatchTask({
			id: "task_resource_attempt_1",
			replayKey: "resource-retry",
			stageId: "validation",
			stageExecutionId: "stage_exec_validation",
			role: "worker",
			objective: "install the environment",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["environment exists"],
			failureSignals: ["installation failed"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["bash"],
			writeAuthority: "workspace-write",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["environment exists"],
		});
		const firstResourceRoot = taskResourcePath(root, job.state.frame.jobId, first.id);
		await mkdir(firstResourceRoot, { recursive: true });
		await writeFile(join(firstResourceRoot, "partial-environment.txt"), "installed\n", "utf8");
		const retry = {
			...first,
			id: "task_resource_attempt_2",
			attempt: 2,
			status: "ready" as const,
			supersedesTaskId: first.id,
		};

		await prepareTaskWorkspace(retry, job);

		const retryResourceRoot = taskResourcePath(root, job.state.frame.jobId, retry.id);
		expect(await readFile(join(retryResourceRoot, "partial-environment.txt"), "utf8")).toBe("installed\n");
		await expect(readFile(join(firstResourceRoot, "partial-environment.txt"), "utf8")).rejects.toThrow();
	});

	it("uses the stage-owned worker budget for long experiment tasks", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-stage-budget-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const runDefinition = DEFAULT_STAGES.find((definition) => definition.id === "run");
		if (!runDefinition) throw new Error("run stage definition not found");
		const job = await ResearchJob.create(store, {
			jobId: "job_stage_budget",
			objective: "run a bounded but nontrivial experiment",
			workspaceRoot: root,
			automation: "full",
			definitions: [runDefinition],
		});
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_run_budget",
			jobId: job.state.frame.jobId,
			stageId: "run",
			decisionRef: "plan-run-budget",
			tasks: [
				{
					key: "execute",
					objective: "Execute the experiment and retain commands, metrics, and logs",
					inputArtifactRefs: [],
					requiredOutputFields: runDefinition.requiredOutputFields,
					acceptanceChecks: runDefinition.acceptanceChecks,
					failureSignals: runDefinition.failureSignals,
					successCriteria: ["the experiment completes with inspectable logs"],
				},
			],
			rationale: "The experiment needs a bounded long-running worker",
			sessionRef: "pi-session:planner",
			createdAt: new Date().toISOString(),
		};
		const workerRun = vi.fn(async (task) => ({
			artifactType: task.requiredOutputType,
			content: { commands: ["run"], runs: ["seed-1"], metrics: {}, logs: ["run.log"], failures: [] },
			refs: ["pi-session:test-run"],
		}));
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: workerRun },
			reviewer: {
				review: async (evidence, currentJob) =>
					reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				planStage: async () => plan,
				decideEvidence: async (evidence) =>
					decision(job, "evidence", { evidenceId: evidence.id, decision: "accept" }),
				decideAdoption: async (evidence) => decision(job, "adoption", { evidenceId: evidence.id, adopt: true }),
				decideSearch: async (batch, evaluations) =>
					decision(job, "search-selection", {
						searchBatchId: batch.id,
						selectedCandidateId: evaluations[0]?.candidateId,
					}),
				decideRoute: async () =>
					decision(job, "route", {
						stageId: "run",
						routeAction: "continue",
						evidenceRefs: Object.values(job.state.canonicalRoute.stageArtifactIds),
					}),
			},
		});

		await supervisor.tick();

		expect(workerRun).toHaveBeenCalledOnce();
		expect(workerRun.mock.calls[0]?.[0].budget).toEqual({
			maxTurns: 24,
			maxToolCalls: 64,
			maxRuntimeMs: 3_600_000,
		});
	});
});
