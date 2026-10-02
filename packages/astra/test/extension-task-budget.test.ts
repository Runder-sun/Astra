import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
	createGrepTool,
	type ExtensionAPI,
	type ExtensionContext,
	type ExtensionEvent,
	type ExtensionFactory,
	type ToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { Value } from "typebox/value";
import { afterEach, describe, expect, it, vi } from "vitest";
import { taskPacketPath, writeTaskPacket } from "../src/contracts.ts";
import { createAstraExtension } from "../src/extension.ts";
import { writeSourceReceipt } from "../src/literature.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { prepareTaskWorkspace, taskWorkspacePath } from "../src/task-workspace.ts";
import type { TaskPacket } from "../src/types.ts";
import { incrementalContentHash } from "../src/worker-submission.ts";
import { reviewFixture } from "./review-fixture.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

interface ExtensionFixture {
	context: ExtensionContext;
	handlers: Map<string, (event: ExtensionEvent, ctx: ExtensionContext) => Promise<unknown>>;
	tools: Map<string, ToolDefinition>;
}

function createFixture(factory: ExtensionFactory, root: string): ExtensionFixture {
	const handlers = new Map<string, (event: ExtensionEvent, ctx: ExtensionContext) => Promise<unknown>>();
	const tools = new Map<string, ToolDefinition>();
	const api = {
		appendEntry: vi.fn(),
		getFlag: vi.fn(() => undefined),
		on(event: string, handler: (value: ExtensionEvent, ctx: ExtensionContext) => Promise<unknown>) {
			handlers.set(event, handler);
		},
		registerCommand: vi.fn(),
		registerFlag: vi.fn(),
		registerTool(tool: ToolDefinition) {
			tools.set(tool.name, tool);
		},
		sendMessage: vi.fn(),
	} as unknown as ExtensionAPI;
	const context = {
		abort: vi.fn(),
		cwd: root,
		hasUI: false,
		isIdle: () => false,
		mode: "json",
		ui: {
			notify: vi.fn(),
			setStatus: vi.fn(),
			setWidget: vi.fn(),
		},
	} as unknown as ExtensionContext;
	void factory(api);
	return { context, handlers, tools };
}

async function createTask(root: string, role: "worker" | "reviewer" = "worker"): Promise<TaskPacket> {
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		jobId: "job_task_budget",
		objective: "validate a bounded question",
		workspaceRoot: root,
		automation: "full",
	});
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "stage_exec_validation",
		agentId: `${role}_task_budget`,
		role,
		objective: "define a bounded and falsifiable research question",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["researchQuestion", "scope", "nonGoals", "acceptanceCriteria", "falsifiableNextStep"],
		acceptanceChecks: ["question is bounded"],
		failureSignals: ["question is vague"],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 30_000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["question is falsifiable"],
	});
	await writeTaskPacket(task);
	return task;
}

async function setup(role: "worker" | "reviewer" = "worker"): Promise<{ fixture: ExtensionFixture; task: TaskPacket }> {
	const root = await mkdtemp(join(tmpdir(), "astra-extension-budget-"));
	tempRoots.push(root);
	const task = await createTask(root, role);
	vi.stubEnv("ASTRA_PROJECT_ROOT", root);
	vi.stubEnv("ASTRA_JOB_ID", task.jobId);
	vi.stubEnv("ASTRA_ROLE", role);
	vi.stubEnv("ASTRA_SESSION_ID", "worker-budget-session");
	vi.stubEnv("ASTRA_TASK_PACKET", taskPacketPath(root, task.jobId, task.id));
	return { fixture: createFixture(createAstraExtension({ jobId: task.jobId, role }), root), task };
}

describe("Astra TaskPacket inner-loop controls", () => {
	it.each([false, true])(
		"M4 valid Pi adoption submission enforces same capability at the source (cross=%s)",
		async (cross) => {
			const root = await mkdtemp(join(tmpdir(), "astra-pi-replacement-source-"));
			tempRoots.push(root);
			const seed = await createTask(root);
			const store = new JsonlAstraStore(root);
			let job = (await ResearchJob.open(store, seed.jobId))!;
			const accepted = [];
			for (const [index, stageId] of [cross ? "literature" : "validation", "validation"].entries()) {
				const task = await job.dispatchTask({
					...seed,
					id: `replacement_${index}`,
					replayKey: `replacement_${index}`,
					stageId,
					stageExecutionId: job.state.stages[stageId].executionId ?? `stage_exec_${stageId}`,
					requiredOutputType: stageId,
					requiredOutputFields: ["content"],
				});
				await job.setTaskStatus(task.id, "succeeded");
				const evidence = await job.recordEvidence({
					taskId: task.id,
					stageId,
					type: stageId,
					content: { content: index },
					refs: [],
				});
				await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
				await job.decideEvidence(evidence.id, true);
				accepted.push(evidence);
			}
			const old = await job.adoptEvidence(accepted[0].id);
			vi.stubEnv("ASTRA_PROJECT_ROOT", root);
			vi.stubEnv("ASTRA_JOB_ID", job.state.frame.jobId);
			vi.stubEnv("ASTRA_ROLE", "main-agent");
			vi.stubEnv("ASTRA_SESSION_ID", "real-pi-submission");
			vi.stubEnv("ASTRA_DECISION_TYPE", "adoption");
			vi.stubEnv("ASTRA_DECISION_REF", "replacement_submission");
			vi.stubEnv("ASTRA_EVIDENCE_ID", accepted[1].id);
			const fixture = createFixture(
				createAstraExtension({ jobId: job.state.frame.jobId, role: "main-agent" }),
				root,
			);
			const submit = fixture.tools.get("astra_submit_main_decision")!;
			const params = {
				decisionType: "adoption",
				decisionRef: "replacement_submission",
				evidenceId: accepted[1].id,
				adopt: true,
				replacementOf: old.id,
				rationale: "Replace the reviewed original",
			};
			expect(Value.Check(submit.parameters, params)).toBe(true);
			const response = await submit.execute("replacement", params, undefined, undefined, fixture.context);
			expect(response.terminate).toBe(true);
			const before = job.state;
			if (cross) {
				await expect(job.adoptEvidence(accepted[1].id, old.id)).rejects.toThrow(/same|replacement/);
				expect(job.state).toEqual(before);
				return;
			}
			const append = store.append.bind(store);
			let hit = false;
			const spy = vi.spyOn(store, "append").mockImplementation(async (id, event) => {
				if (!hit && event.type === "canonical_artifact_status" && event.status === "active") {
					hit = true;
					vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("Active snapshot failed"));
				}
				return append(id, event);
			});
			await expect(job.adoptEvidence(accepted[1].id, old.id)).rejects.toThrow(/snapshot/);
			spy.mockRestore();
			job = (await ResearchJob.open(store, job.state.frame.jobId))!;
			expect(job.state.retiredArtifacts[old.id]).toBeDefined();
			const adopted = await job.adoptEvidence(accepted[1].id, old.id);
			expect(adopted.status).toBe("active");
			const completed = job.state;
			await job.adoptEvidence(accepted[1].id, old.id);
			expect(job.state).toEqual(completed);
		},
	);
	it("submits a literature increment through the extension and records the merged evidence", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-extension-incremental-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Incremental literature",
			workspaceRoot: root,
			automation: "full",
		});
		const refs = ["https://example.org/a", "https://example.org/b", "https://example.org/c"];
		for (const sourceRef of refs)
			await writeSourceReceipt(
				{ workspaceRoot: root, jobId: job.state.frame.jobId, query: "fixture", limit: 3 },
				{ sourceRef, title: sourceRef, authors: [] },
				"fixture",
				new Date().toISOString(),
			);
		const sourceTask = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			objective: "create base",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature:local",
			requiredOutputFields: ["queryStrategy", "sources"],
			acceptanceChecks: ["record limitations"],
			failureSignals: ["limitations remain undocumented"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		await job.setTaskStatus(sourceTask.id, "succeeded");
		const base = await job.recordEvidence({
			taskId: sourceTask.id,
			stageId: "literature",
			type: "literature:local",
			content: { queryStrategy: { limitations: "search-only" }, sources: refs.map((sourceRef) => ({ sourceRef })) },
			refs,
		});
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			deliveryKind: "local",
			objective: "repair limitation",
			repairOfEvidenceId: base.id,
			inputArtifactRefs: [base.id],
			requiredOutputType: "literature:local",
			requiredOutputFields: ["queryStrategy", "sources"],
			acceptanceChecks: ["record limitations"],
			repairChecks: [{ issueId: "issue_lit", criterion: "record limitations" }],
			requiredCanonicalArtifacts: [],
			failureSignals: ["limitations remain undocumented"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		await writeTaskPacket(task);
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", task.jobId);
		vi.stubEnv("ASTRA_ROLE", "worker");
		vi.stubEnv("ASTRA_TASK_PACKET", taskPacketPath(root, task.jobId, task.id));
		vi.stubEnv("ASTRA_SESSION_ID", "incremental-session");
		const fixture = createFixture(createAstraExtension({ jobId: task.jobId, role: "worker" }), root);
		const submit = fixture.tools.get("astra_submit_worker_output");
		if (!submit) throw new Error("worker submission tool was not registered");
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set" as const,
					path: ["queryStrategy", "limitations"],
					value: "capture unavailable",
					issueId: "issue_lit",
					sourceRefs: [],
					reason: "Clarify the recorded limitation.",
				},
			],
			affectedCriteria: task.acceptanceChecks,
			rationale: "Retain the full base while correcting the limitation.",
		};
		const response = await submit.execute(
			"incremental-output",
			{ artifactType: task.requiredOutputType, content: {}, incrementalRevision: revision, refs: [] },
			undefined,
			undefined,
			fixture.context,
		);
		expect(response.terminate).toBe(true);
		const manifest = response.details as {
			content: unknown;
			incrementalRevision: { resultHash: string };
			outputRefs: Array<{ ref: string }>;
		};
		expect(manifest.content).toMatchObject({
			queryStrategy: { limitations: "capture unavailable" },
			sources: refs.map((sourceRef) => ({ sourceRef })),
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: manifest.content,
			refs: manifest.outputRefs.map((ref) => ref.ref),
			currentEvidenceSetId: base.currentEvidenceSetId,
			incrementalRevision: { ...revision, resultHash: manifest.incrementalRevision.resultHash },
		});
		expect(evidence.incrementalRevision?.resultHash).toBe(manifest.incrementalRevision.resultHash);
	});
	it("lets the main agent page through full evidence by owned ID without accepting file paths", async () => {
		const { task, fixture: workerFixture } = await setup();
		const root = workerFixture.context.cwd;
		const job = await ResearchJob.open(new JsonlAstraStore(root), task.jobId);
		if (!job) throw new Error("research job missing");
		const sourceRoot = join(root, ".astra", "jobs", task.jobId, "workspaces", task.id);
		await mkdir(sourceRoot, { recursive: true });
		await writeFile(join(sourceRoot, "result.csv"), "value\n42\n");
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { raw: `${"x".repeat(32000)}exact-evidence-tail` },
			refs: ["result.csv"],
		});
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		const fixture = createFixture(createAstraExtension({ jobId: task.jobId, role: "main-agent" }), root);
		const read = fixture.tools.get("astra_read_research_object");
		if (!read) throw new Error("controlled research reader missing");
		let combined = "";
		let offset = 0;
		do {
			const response = await read.execute(
				"read-object",
				{ id: evidence.id, offset, limit: 16000 },
				undefined,
				undefined,
				fixture.context,
			);
			const page = response.details as { text: string; nextOffset?: number; totalCharacters: number };
			expect(page.text.length).toBeLessThanOrEqual(16000);
			combined += page.text;
			offset = page.nextOffset ?? -1;
		} while (offset >= 0);
		expect(JSON.parse(combined)).toEqual(evidence);
		await writeFile(join(sourceRoot, "result.csv"), "value\n999\n");
		const filePage = await read.execute(
			"read-file",
			{ id: evidence.id, fileRef: "result.csv" },
			undefined,
			undefined,
			fixture.context,
		);
		expect(filePage.details).toMatchObject({ text: "value\n42\n", sha256: evidence.files?.[0].sha256 });
		await expect(
			read.execute(
				"read-unknown-file",
				{ id: evidence.id, fileRef: "../../job.json" },
				undefined,
				undefined,
				fixture.context,
			),
		).rejects.toThrow(/frozen/);
		const frozen = join(root, ".astra", "jobs", task.jobId, "versions", "files", evidence.files![0].sha256);
		await rm(frozen);
		await writeFile(frozen, "corrupt");
		await expect(
			read.execute(
				"read-corrupt-file",
				{ id: evidence.id, fileRef: "result.csv" },
				undefined,
				undefined,
				fixture.context,
			),
		).rejects.toThrow(/integrity/);
		const missing = await read.execute("read-path", { id: "../../job.json" }, undefined, undefined, fixture.context);
		expect(missing.content[0]).toMatchObject({ text: expect.stringContaining("Unknown research object") });
		const workerRead = workerFixture.tools.get("astra_read_research_object");
		if (!workerRead) throw new Error("controlled research reader missing");
		vi.stubEnv("ASTRA_ROLE", "worker");
		const denied = await workerRead.execute(
			"worker-read",
			{ id: evidence.id },
			undefined,
			undefined,
			workerFixture.context,
		);
		expect(denied.content[0]).toMatchObject({ text: expect.stringContaining("Only the main-agent") });
	});

	it("rejects incomplete main-agent decisions before writing a terminal manifest", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-main-decision-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			jobId: "job_main_decision",
			objective: "validate main decision contracts",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", job.state.frame.jobId);
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_DECISION_TYPE", "evidence");
		vi.stubEnv("ASTRA_DECISION_REF", "decision_test");
		vi.stubEnv("ASTRA_EVIDENCE_ID", "evidence_test");
		const fixture = createFixture(createAstraExtension({ jobId: job.state.frame.jobId, role: "main-agent" }), root);
		const submit = fixture.tools.get("astra_submit_main_decision");
		if (!submit) throw new Error("main decision tool was not registered");

		const rejected = await submit.execute(
			"main-decision-incomplete",
			{
				decisionType: "evidence",
				decisionRef: "decision_test",
				evidenceId: "evidence_test",
				rationale: "accept the evidence",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(rejected.terminate).toBeUndefined();
		expect(rejected.content[0]).toMatchObject({ text: expect.stringContaining("decision is required") });
	});

	it("rejects unknown route evidence refs before writing a terminal manifest", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-main-route-refs-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			jobId: "job_main_route_refs",
			objective: "validate route evidence references",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", job.state.frame.jobId);
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_DECISION_TYPE", "route");
		vi.stubEnv("ASTRA_DECISION_REF", "route_refs_test");
		const fixture = createFixture(createAstraExtension({ jobId: job.state.frame.jobId, role: "main-agent" }), root);
		const submit = fixture.tools.get("astra_submit_main_decision");
		if (!submit) throw new Error("main decision tool was not registered");

		const rejected = await submit.execute(
			"route-unknown-evidence-ref",
			{
				decisionType: "route",
				decisionRef: "route_refs_test",
				stageId: "validation",
				routeAction: "continue",
				evidenceRefs: ["evidence_typo"],
				rationale: "continue the current stage",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(rejected.terminate).toBeUndefined();
		expect(rejected.content[0]).toMatchObject({ text: expect.stringContaining("unknown evidence") });

		const corrected = await submit.execute(
			"route-corrected-evidence-ref",
			{
				decisionType: "route",
				decisionRef: "route_refs_test",
				stageId: "validation",
				routeAction: "continue",
				evidenceRefs: [job.state.graph.rootQuestionId],
				rationale: "continue with a known research-graph reference",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(corrected.terminate).toBe(true);
	});

	it("rejects an adoption replacement that is not an active canonical artifact", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-main-adoption-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			jobId: "job_main_adoption",
			objective: "validate adoption decisions",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", job.state.frame.jobId);
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_DECISION_TYPE", "adoption");
		vi.stubEnv("ASTRA_DECISION_REF", "adoption_test");
		vi.stubEnv("ASTRA_EVIDENCE_ID", "evidence_test");
		const fixture = createFixture(createAstraExtension({ jobId: job.state.frame.jobId, role: "main-agent" }), root);
		const submit = fixture.tools.get("astra_submit_main_decision");
		if (!submit) throw new Error("main decision tool was not registered");

		const rejected = await submit.execute(
			"main-adoption-invalid-replacement",
			{
				decisionType: "adoption",
				decisionRef: "adoption_test",
				evidenceId: "evidence_test",
				adopt: true,
				replacementOf: "paper-manuscript.md",
				rationale: "replace the previous artifact",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(rejected.terminate).toBeUndefined();
		expect(rejected.content[0]).toMatchObject({ text: expect.stringContaining("unknown replacement artifact") });
	});

	it("rejects continued search in the final round and accepts an exact candidate selection", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-main-final-search-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			jobId: "job_main_final_search",
			objective: "validate final search decisions",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", job.state.frame.jobId);
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_DECISION_TYPE", "search-selection");
		vi.stubEnv("ASTRA_DECISION_REF", "search_final_test");
		vi.stubEnv("ASTRA_SEARCH_BATCH_ID", "search_final_batch");
		vi.stubEnv("ASTRA_CANDIDATE_IDS", JSON.stringify(["candidate_exact"]));
		vi.stubEnv("ASTRA_SEARCH_ALLOW_CONTINUE", "0");
		const fixture = createFixture(createAstraExtension({ jobId: job.state.frame.jobId, role: "main-agent" }), root);
		const submit = fixture.tools.get("astra_submit_main_decision");
		if (!submit) throw new Error("main decision tool was not registered");

		const rejected = await submit.execute(
			"search-final-continue",
			{
				decisionType: "search-selection",
				decisionRef: "search_final_test",
				searchBatchId: "search_final_batch",
				continueSearch: true,
				rationale: "the candidates remain tied",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(rejected.terminate).toBeUndefined();
		expect(rejected.content[0]).toMatchObject({ text: expect.stringContaining("final search round") });

		const accepted = await submit.execute(
			"search-final-select",
			{
				decisionType: "search-selection",
				decisionRef: "search_final_test",
				searchBatchId: "search_final_batch",
				selectedCandidateId: "candidate_exact",
				rationale: "deterministic frozen-criteria tie break",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(accepted.terminate).toBe(true);
	});

	it("rejects repair plans that split one obligation across sibling tasks", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-main-repair-plan-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			jobId: "job_main_repair_plan",
			objective: "validate repair planning",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", job.state.frame.jobId);
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_STAGE_ID", "validation");
		vi.stubEnv("ASTRA_STAGE_PLAN_ID", "repair_plan_test");
		vi.stubEnv("ASTRA_DECISION_REF", "repair_plan_decision_test");
		vi.stubEnv("ASTRA_OBLIGATION_ID", "obligation_test");
		vi.stubEnv("ASTRA_AVAILABLE_INPUT_REFS", "[]");
		const fixture = createFixture(createAstraExtension({ jobId: job.state.frame.jobId, role: "main-agent" }), root);
		const submit = fixture.tools.get("astra_submit_stage_plan");
		if (!submit) throw new Error("stage plan tool was not registered");
		const plannedTask = {
			key: "repair-a",
			objective: "repair the complete validation artifact",
			inputArtifactRefs: [],
			requiredOutputFields: job.definitions.validation.requiredOutputFields,
			acceptanceChecks: ["all obligation findings are resolved"],
			failureSignals: ["a finding remains unresolved"],
			successCriteria: ["the complete artifact passes review"],
			responsibilityBindings: [],
		};

		const rejected = await submit.execute(
			"repair-plan-split",
			{
				tasks: [plannedTask, { ...plannedTask, key: "repair-b" }],
				rationale: "split the repair across two workers",
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(rejected.terminate).toBeUndefined();
		expect(rejected.content[0]).toMatchObject({ text: expect.stringContaining("exactly one complete repair task") });
	});

	it("allows research-review findings beyond the former item limits", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-review-output-schema-"));
		tempRoots.push(root);
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_ROLE", "worker");
		vi.stubEnv("ASTRA_STAGE_ID", "research-review");
		const fixture = createFixture(createAstraExtension({ role: "worker" }), root);
		const submit = fixture.tools.get("astra_submit_worker_output");
		if (!submit) throw new Error("worker submission tool was not registered");
		const schema = submit.parameters as unknown as {
			properties: {
				content: { properties: Record<string, { maxItems?: number; items?: { maxLength?: number } }> };
			};
		};

		for (const field of ["strengths", "weaknesses", "claimAudit", "requiredRepairs"]) {
			expect(schema.properties.content.properties[field]?.maxItems).toBeUndefined();
			expect(schema.properties.content.properties[field]?.items?.maxLength).toBeUndefined();
		}
	});

	it("marks a successful worker submission as terminal for the Pi loop", async () => {
		const { fixture } = await setup();
		const submit = fixture.tools.get("astra_submit_worker_output");
		if (!submit) throw new Error("worker submission tool was not registered");

		const output = await submit.execute(
			"submit-1",
			{
				artifactType: "validation",
				content: {
					researchQuestion: "When does A outperform B?",
					scope: ["local benchmark"],
					nonGoals: ["memory use"],
					acceptanceCriteria: ["measure latency"],
					falsifiableNextStep: "run the benchmark",
				},
				refs: [],
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(output.terminate).toBe(true);
	});

	it("terminates a tool batch that exceeds maxToolCalls", async () => {
		const { fixture } = await setup();
		const handler = fixture.handlers.get("tool_call");
		if (!handler) throw new Error("tool_call handler was not registered");
		const event = { type: "tool_call", toolCallId: "read", toolName: "read", input: { path: "." } } as ExtensionEvent;

		expect(await handler(event, fixture.context)).toBeUndefined();
		expect(await handler(event, fixture.context)).toBeUndefined();
		expect(await handler(event, fixture.context)).toMatchObject({ block: true, terminate: true });
	});

	it("reserves the terminal submission after the work-tool budget is exhausted", async () => {
		const { fixture } = await setup();
		const handler = fixture.handlers.get("tool_call");
		if (!handler) throw new Error("tool_call handler was not registered");
		const readEvent = {
			type: "tool_call",
			toolCallId: "read",
			toolName: "read",
			input: { path: "." },
		} as ExtensionEvent;

		expect(await handler(readEvent, fixture.context)).toBeUndefined();
		expect(await handler(readEvent, fixture.context)).toBeUndefined();
		await expect(
			handler(
				{
					type: "tool_call",
					toolCallId: "submit-after-budget",
					toolName: "astra_submit_worker_output",
					input: {},
				} as ExtensionEvent,
				fixture.context,
			),
		).resolves.toBeUndefined();
	});

	it("blocks direct worker reads of large files before they enter model context", async () => {
		const { fixture } = await setup();
		const handler = fixture.handlers.get("tool_call");
		if (!handler) throw new Error("tool_call handler was not registered");
		await mkdir(join(fixture.context.cwd, "inputs"), { recursive: true });
		await writeFile(join(fixture.context.cwd, "inputs", "raw.json"), "x".repeat(16 * 1024 + 1));

		await expect(
			handler(
				{
					type: "tool_call",
					toolCallId: "read-large",
					toolName: "read",
					input: { path: "inputs/raw.json" },
				} as ExtensionEvent,
				fixture.context,
			),
		).resolves.toMatchObject({ block: true, reason: expect.stringContaining("Do not retry") });
	});

	it("confines worker shell paths to the task workspace and declared resource roots", async () => {
		const { fixture, task } = await setup();
		const handler = fixture.handlers.get("tool_call");
		if (!handler) throw new Error("tool_call handler was not registered");
		const resourceRoot = join(fixture.context.cwd, ".resources", task.id);
		const inputRoot = join(fixture.context.cwd, ".resources", "upstream");
		vi.stubEnv("ASTRA_RESOURCE_ROOT", resourceRoot);
		vi.stubEnv("ASTRA_INPUT_RESOURCE_ROOTS", JSON.stringify([inputRoot]));
		await writeTaskPacket({
			...task,
			allowedTools: ["bash"],
			writeAuthority: "workspace-write",
			budget: { ...task.budget, maxToolCalls: 8 },
		});
		const call = (id: string, command: string) =>
			handler(
				{ type: "tool_call", toolCallId: id, toolName: "bash", input: { command } } as ExtensionEvent,
				fixture.context,
			);

		await expect(call("resource-env", 'python -m venv "$ASTRA_RESOURCE_ROOT/env"')).resolves.toBeUndefined();
		await expect(call("resource-literal", `touch "${resourceRoot}/marker"`)).resolves.toBeUndefined();
		await expect(
			call("resource-download", 'curl https://example.com/model -o "$ASTRA_RESOURCE_ROOT/model"'),
		).resolves.toBeUndefined();
		await expect(call("resource-input", `sha256sum "${inputRoot}/checkpoint"`)).resolves.toBeUndefined();
		await expect(call("shell-division", 'python -c "print(1 / 2)"')).resolves.toBeUndefined();
		await expect(call("outside", "touch /tmp/astra-outside")).resolves.toMatchObject({
			block: true,
			reason: expect.stringContaining("outside declared resource roots"),
		});
	});

	it("permits validated retry roots only for read, ls, find and grep", async () => {
		const { task: seed, fixture: firstFixture } = await setup();
		const root = firstFixture.context.cwd;
		const store = new JsonlAstraStore(root);
		const job = (await ResearchJob.open(store, seed.jobId))!;
		const first = await job.dispatchTask({
			...seed,
			id: "source_read_permissions",
			replayKey: "retry-permissions",
			allowedTools: ["read", "ls", "find", "grep", "write", "edit", "bash"],
			writeAuthority: "workspace-write",
			budget: { ...seed.budget, maxToolCalls: 20 },
		});
		await prepareTaskWorkspace(first, job);
		const previous = taskWorkspacePath(root, first.jobId, first.id);
		await writeFile(join(previous, "partial.json"), '{"result":42}');
		await symlink(root, join(previous, "escape"));
		await job.setTaskStatus(first.id, "failed");
		const retry = await job.dispatchTask({
			...first,
			id: "retry_read_permissions",
			attempt: 2,
			supersedesTaskId: first.id,
			allowedTools: ["read", "ls", "find", "grep", "write", "edit", "bash"],
			writeAuthority: "workspace-write",
			budget: { ...first.budget, maxToolCalls: 20 },
		});
		const cwd = await prepareTaskWorkspace(retry, job);
		vi.stubEnv("ASTRA_TASK_PACKET", taskPacketPath(root, retry.jobId, retry.id));
		vi.stubEnv("ASTRA_RECOVERY_READ_ROOTS", JSON.stringify([previous]));
		const fixture = createFixture(createAstraExtension({ jobId: retry.jobId, role: "worker" }), cwd);
		const handler = fixture.handlers.get("tool_call")!;
		const call = (toolName: string, path: string) =>
			handler(
				{ type: "tool_call", toolCallId: toolName, toolName, input: { path } } as ExtensionEvent,
				fixture.context,
			);
		for (const name of ["read", "ls", "find", "grep"]) {
			await expect(call(name, name === "read" ? join(previous, "partial.json") : previous)).resolves.toBeUndefined();
		}
		for (const name of ["write", "edit"])
			await expect(call(name, join(previous, "partial.json"))).resolves.toMatchObject({ block: true });
		await expect(call("read", join(previous, "escape", "AGENTS.md"))).resolves.toMatchObject({ block: true });
		await expect(
			handler(
				{
					type: "tool_call",
					toolCallId: "old-bash",
					toolName: "bash",
					input: { command: `touch "${previous}/marker"` },
				} as ExtensionEvent,
				fixture.context,
			),
		).resolves.toMatchObject({ block: true });
		const largeLog = join(previous, "large-log.jsonl");
		const logPrefix = `${JSON.stringify({ detail: "x".repeat(90) })}\n`.repeat(200);
		await writeFile(largeLog, `${logPrefix}{"failure":"RECOVERY_MARKER"}\n`);
		await expect(call("read", largeLog)).resolves.toMatchObject({
			block: true,
			reason: expect.stringContaining("grep"),
		});
		await expect(call("read", largeLog)).resolves.toMatchObject({ reason: expect.not.stringContaining("bash") });
		await expect(
			handler(
				{
					type: "tool_call",
					toolCallId: "old-log-bash",
					toolName: "bash",
					input: { command: `tail -n 1 "${largeLog}"` },
				} as ExtensionEvent,
				fixture.context,
			),
		).resolves.toMatchObject({ block: true });
		await expect(call("grep", largeLog)).resolves.toBeUndefined();
		const result = await createGrepTool(cwd).execute("recovery-log-grep", {
			path: largeLog,
			pattern: "RECOVERY_MARKER",
			limit: 1,
		});
		expect(JSON.stringify(result)).toContain("RECOVERY_MARKER");
		await expect(call("grep", join(previous, "escape", "AGENTS.md"))).resolves.toMatchObject({ block: true });
		const largeCurrent = join(cwd, "large-current.log");
		await writeFile(largeCurrent, "x".repeat(16 * 1024 + 1));
		await expect(call("read", largeCurrent)).resolves.toMatchObject({
			block: true,
			reason: expect.stringContaining("bash"),
		});
		vi.stubEnv("ASTRA_RECOVERY_READ_ROOTS", JSON.stringify([root]));
		await expect(call("read", join(root, ".astra", "jobs", retry.jobId, "job.json"))).resolves.toMatchObject({
			block: true,
		});
	});

	it("allows all canonical digests within the task budget", async () => {
		const { fixture, task } = await setup();
		const handler = fixture.handlers.get("tool_call");
		if (!handler) throw new Error("tool_call handler was not registered");
		await writeTaskPacket({
			...task,
			stageId: "research-review",
			allowedTools: ["read", "bash"],
			budget: { ...task.budget, maxToolCalls: 8 },
		});
		await mkdir(join(fixture.context.cwd, "canonical"), { recursive: true });
		for (const name of ["a", "b", "c", "d", "e", "f", "g", "h"]) {
			await writeFile(join(fixture.context.cwd, "canonical", `${name}.json`), `{"id":"${name}"}\n`);
		}

		for (const name of ["a", "b", "c", "d", "e", "f", "g", "h"]) {
			await expect(
				handler(
					{
						type: "tool_call",
						toolCallId: `read-${name}`,
						toolName: "read",
						input: { path: `canonical/${name}.json` },
					} as ExtensionEvent,
					fixture.context,
				),
			).resolves.toBeUndefined();
		}
		await expect(
			handler(
				{
					type: "tool_call",
					toolCallId: "unbounded-cat",
					toolName: "bash",
					input: { command: "cat canonical/a.json" },
				} as ExtensionEvent,
				fixture.context,
			),
		).resolves.toMatchObject({ block: true, terminate: true });
	});

	it("allows a reviewer to call its terminal submission tool", async () => {
		const { fixture, task } = await setup("reviewer");
		const handler = fixture.handlers.get("tool_call");
		if (!handler) throw new Error("tool_call handler was not registered");
		await writeTaskPacket({ ...task, inputArtifactRefs: ["evidence_expected"] });

		await expect(
			handler(
				{
					type: "tool_call",
					toolCallId: "review-1",
					toolName: "astra_submit_review",
					input: { evidenceId: "evidence-1", verdict: "pass", findings: [] },
				} as ExtensionEvent,
				fixture.context,
			),
		).resolves.toBeUndefined();

		const submit = fixture.tools.get("astra_submit_review");
		if (!submit) throw new Error("review submission tool was not registered");

		const rejected = await submit.execute(
			"review-submit-wrong-id",
			{ evidenceId: "evidence_typo", verdict: "pass", findings: [] },
			undefined,
			undefined,
			fixture.context,
		);

		expect(rejected.terminate).toBeUndefined();
		expect(rejected.content[0]).toMatchObject({ text: expect.stringContaining("evidence_expected") });
	});

	it("requires reviewer criteria to preserve every frozen criterion exactly", async () => {
		const { fixture, task } = await setup("reviewer");
		const frozenCriteria = ["research question is bounded and falsifiable", "acceptance criteria are measurable"];
		await writeTaskPacket({ ...task, inputArtifactRefs: ["evidence_expected"] });
		vi.stubEnv("ASTRA_REVIEW_CRITERIA", JSON.stringify(frozenCriteria));
		const submit = fixture.tools.get("astra_submit_review");
		if (!submit) throw new Error("review submission tool was not registered");
		const baseSubmission = {
			evidenceId: "evidence_expected",
			verdict: "pass",
			findings: [],
			score: 1,
			verifiedRefs: ["review-target-snapshot.json"],
		};

		const paraphrased = await submit.execute(
			"review-submit-paraphrased-criteria",
			{
				...baseSubmission,
				criteria: frozenCriteria.map((criterion) => ({
					criterion: criterion[0].toUpperCase() + criterion.slice(1),
					passed: true,
					score: 1,
					evidenceRefs: ["review-target-snapshot.json"],
					rationale: "verified",
				})),
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(paraphrased.terminate).toBeUndefined();
		expect(paraphrased.content[0]).toMatchObject({ text: expect.stringContaining(frozenCriteria[0]) });

		const exact = await submit.execute(
			"review-submit-exact-criteria",
			{
				...baseSubmission,
				criteria: frozenCriteria.map((criterion) => ({
					criterion,
					passed: true,
					score: 1,
					evidenceRefs: ["review-target-snapshot.json"],
					rationale: "verified",
				})),
			},
			undefined,
			undefined,
			fixture.context,
		);

		expect(exact.terminate).toBe(true);
	});

	it("aborts after maxTurns when no terminal manifest was submitted", async () => {
		const { fixture } = await setup();
		const handler = fixture.handlers.get("turn_end");
		if (!handler) throw new Error("turn_end handler was not registered");

		await handler(
			{
				type: "turn_end",
				turnIndex: 0,
				message: { role: "user", content: [{ type: "text", text: "first" }], timestamp: 1 },
				toolResults: [],
			},
			fixture.context,
		);
		await handler(
			{
				type: "turn_end",
				turnIndex: 1,
				message: { role: "user", content: [{ type: "text", text: "second" }], timestamp: 2 },
				toolResults: [],
			},
			fixture.context,
		);

		expect(fixture.context.abort).toHaveBeenCalledOnce();
	});
});
