import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionCommandContext, ToolDefinition } from "@earendil-works/pi-coding-agent";
import type { TSchema } from "typebox";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as contractFiles from "../src/contracts.ts";
import { previousExecutedSearchBatch } from "../src/effective-contract.ts";
import { createAstraExtension } from "../src/extension.ts";
import { applyPendingGuidance } from "../src/guidance-control.ts";
import { PiChildSessionRunner, PiMainAgentAdapter } from "../src/pi-child-session.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { runResearchControl } from "../src/research-control.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore, ResearchJobBusyError } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

function extension(root: string, signal?: AbortSignal) {
	const commands = new Map<string, (text: string, context: ExtensionCommandContext) => Promise<void> | void>();
	const tools = new Map<string, ToolDefinition<TSchema, unknown, unknown>>();
	const notify = vi.fn();
	createAstraExtension()({
		getFlag: vi.fn(),
		on: vi.fn(),
		registerFlag: vi.fn(),
		appendEntry: vi.fn(),
		sendMessage: vi.fn(),
		registerCommand: (
			name: string,
			command: { handler: (text: string, context: ExtensionCommandContext) => Promise<void> | void },
		) => {
			commands.set(name, command.handler);
		},
		registerTool: (tool: ToolDefinition<TSchema, unknown, unknown>) => {
			tools.set(tool.name, tool);
		},
	} as unknown as ExtensionAPI);
	const context = {
		cwd: root,
		signal,
		ui: { notify, setStatus: vi.fn(), setWidget: vi.fn() },
	} as unknown as ExtensionCommandContext;
	return {
		tools,
		context,
		notify,
		guide: () => Promise.resolve(commands.get("research-guide")!("retain tail instruction", context)),
	};
}

function plan(job: ResearchJob, id: string): StagePlanManifest {
	return {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id,
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: id,
		mode: "search",
		tasks: [1, 2].map((index) => ({
			key: `${id}_${index}`,
			objective: `${id} hypothesis ${index}`,
			hypothesis: `${id} hypothesis ${index}`,
			inputArtifactRefs: [],
			requiredOutputFields: job.definitions.validation.requiredOutputFields,
			acceptanceChecks: ["Evaluate candidate"],
			failureSignals: ["No result"],
			successCriteria: ["Evaluate candidate"],
		})),
		rationale: "offline independent candidates",
		sessionRef: "fixture:main",
		createdAt: new Date().toISOString(),
	};
}

async function executeSearch(job: ResearchJob, store: JsonlAstraStore, id: string) {
	const manifest = await job.recordStagePlan(plan(job, id));
	const evidence = await preparePlanEvidence(job, manifest);
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	const stop = async () => {
		throw new Error("offline deferred review");
	};
	await new ResearchSupervisor(job, store, {
		worker: {
			run: async () => ({
				artifactType: "validation",
				refs: [],
				content: Object.fromEntries(
					job.definitions.validation.requiredOutputFields.map((field) => [field, "offline output"]),
				),
			}),
		},
		reviewer: { review: stop },
		mainAgent: { planStage: stop, decideEvidence: stop, decideAdoption: stop, decideSearch: stop, decideRoute: stop },
	}).tick();
	await job.resumeWithGuidance(`replace executed ${id}`);
	return previousExecutedSearchBatch(job.state, "validation")!;
}

async function complete(job: ResearchJob) {
	for (const stageId of ["result-to-claim", "research-review"]) {
		const definition = job.definitions[stageId];
		const inputs = Object.values(job.state.canonicalRoute.stageArtifactIds);
		const task = await job.dispatchTask({
			stageId,
			stageExecutionId: `stage_exec_${stageId}`,
			role: "worker",
			objective: `produce ${stageId}`,
			inputArtifactRefs: inputs,
			requiredCanonicalArtifacts: inputs,
			requiredOutputType: definition.outputArtifactType,
			requiredOutputFields: definition.requiredOutputFields,
			acceptanceChecks: definition.acceptanceChecks,
			failureSignals: definition.failureSignals,
			dependencies: [],
			scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
			allowedTools: definition.workerTools,
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: definition.acceptanceChecks,
		});
		await job.setTaskStatus(task.id, "succeeded");
		const content =
			stageId === "result-to-claim"
				? {
						scientificOutcome: "inconclusive",
						missionCoverage: "insufficient",
						claims: [],
						supportingResults: [],
						unsupportedClaims: [],
						missingEvidence: ["No experiment performed"],
						conclusion: "Insufficient evidence",
					}
				: {
						scientificOutcome: "inconclusive",
						missionCoverage: "insufficient",
						verdict: "pass",
						strengths: ["Limitations explicit"],
						weaknesses: ["No experiment"],
						claimAudit: [],
						requiredRepairs: [],
					};
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId,
			type: definition.outputArtifactType,
			content,
			refs: [`pi-session:${task.id}`],
		});
		for (let index = 0; index < (definition.qualityPolicy?.minPassingReviews ?? 1); index++)
			await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(evidence.id, true);
		await job.adoptEvidence(evidence.id);
		await job.applyRouteDecision({
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: `manifest_${stageId}`,
			jobId: job.state.frame.jobId,
			stageId,
			decisionType: "route",
			decisionRef: `route_${stageId}`,
			routeAction: stageId === "result-to-claim" ? "advance" : "complete",
			targetStageId: stageId === "result-to-claim" ? "research-review" : undefined,
			rationale: "offline verified route",
			sessionRef: "fixture:main",
			createdAt: new Date().toISOString(),
		});
	}
	expect(job.completionBlockers()).toEqual([]);
}

afterEach(() => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
});

describe("guidance and search entry coordination", () => {
	it.each([false, true])(
		"binds application confirmation to the accepted identity when empty-pending fault=%s",
		async (emptyPending) => {
			const root = await mkdtemp(join(tmpdir(), "astra-guide-identity-"));
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, { objective: "original accepted job", workspaceRoot: root });
				const other = await ResearchJob.create(store, { objective: "new active job", workspaceRoot: root });
				await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
				const fixture = extension(root);
				let guidance: Promise<unknown> | undefined;
				let requestId = "";
				await store.withJobLock(job.state.frame.jobId, "original holder", async () => {
					guidance = fixture.guide().catch((error: unknown) => error);
					await vi.waitFor(() => expect(fixture.notify).toHaveBeenCalledWith(expect.stringContaining("accepted")));
					const path = join(root, ".astra", "jobs", job.state.frame.jobId, "guidance-inbox.json");
					const inbox = JSON.parse(await readFile(path, "utf8"));
					requestId = inbox.receipts[0].requestId;
					if (emptyPending) {
						// Explicit receipt/queue inconsistency injection must not fake successful application.
						inbox.pending = [];
						await writeFile(path, JSON.stringify(inbox));
					}
					await writeFile(
						join(root, ".astra", "active-job.json"),
						JSON.stringify({ jobId: other.state.frame.jobId }),
					);
				});
				const result = await guidance;
				const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
				const applied = Object.values(reopened.state.graph.nodes).filter(
					(node) => node.domainRef === `guidance-request:${requestId}`,
				);
				if (emptyPending) {
					expect(result).toMatchObject({ message: "research guidance application has not been confirmed" });
					expect(applied).toHaveLength(0);
				} else {
					expect(result).toBeUndefined();
					expect(applied).toHaveLength(1);
				}
				expect(
					Object.values((await ResearchJob.open(store, other.state.frame.jobId))!.state.graph.nodes).filter(
						(node) => node.actor === "user",
					),
				).toHaveLength(0);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);
	it.each(["not-started", "old-revision", "older-batch", "max-round"] as const)(
		"rejects the %s predecessor at the registered submission guard",
		async (boundary) => {
			const root = await mkdtemp(join(tmpdir(), "astra-pi-predecessor-"));
			try {
				const store = new JsonlAstraStore(root);
				const definition = structuredClone(DEFAULT_STAGES.find((stage) => stage.id === "validation")!);
				definition.searchPolicy = {
					strategy: "diverse-candidates",
					minCandidates: 2,
					maxCandidates: 4,
					maxRounds: boundary === "max-round" ? 1 : 3,
					criteria: definition.acceptanceChecks,
				};
				const job = await ResearchJob.create(store, {
					objective: "strict previous identity",
					workspaceRoot: root,
					automation: "full",
					definitions: [definition],
				});
				let claimed: string;
				if (boundary === "not-started") {
					await job.recordStagePlan(plan(job, "ready"));
					claimed = Object.values(job.state.searchBatches)[0].id;
				} else {
					claimed = (await executeSearch(job, store, "first_executed")).id;
					if (boundary === "older-batch") {
						const latest = await executeSearch(job, store, "second_executed");
						expect(latest.id).not.toBe(claimed);
						expect(latest.round).toBe(2);
					}
					if (boundary === "old-revision")
						await job.applyRouteDecision({
							schemaVersion: "astra.main_agent_decision_manifest.v1",
							manifestId: "backtrack_manifest",
							jobId: job.state.frame.jobId,
							stageId: "validation",
							decisionType: "route",
							decisionRef: "backtrack_revision",
							routeAction: "backtrack",
							targetStageId: "validation",
							rationale: "restart stage revision",
							sessionRef: "fixture:main",
							createdAt: new Date().toISOString(),
						});
				}
				for (const [key, value] of Object.entries({
					ASTRA_PROJECT_ROOT: root,
					ASTRA_JOB_ID: job.state.frame.jobId,
					ASTRA_ROLE: "main-agent",
					ASTRA_STAGE_ID: "validation",
					ASTRA_STAGE_PLAN_ID: "replacement_guard",
					ASTRA_DECISION_REF: "replacement_guard",
					ASTRA_PLAN_MODE: "search",
					ASTRA_SEARCH_PREVIOUS_BATCH_ID: claimed,
				}))
					vi.stubEnv(key, value);
				const fixture = extension(root);
				const result = await fixture.tools.get("astra_submit_stage_plan")!.execute(
					"offline",
					{
						mode: "search",
						tasks: plan(job, "new_guard").tasks.map((task) => ({
							...task,
							responsibilityBindings: [],
							responsibilityTransfers: [],
						})),
						rationale: "new independent hypotheses",
					},
					undefined,
					undefined,
					fixture.context,
				);
				expect(result.content).toEqual(
					expect.arrayContaining([
						expect.objectContaining({ text: expect.stringContaining("previous search batch does not match") }),
					]),
				);
				if (boundary === "max-round") {
					const runner = new PiChildSessionRunner();
					const run = vi.spyOn(runner, "run");
					await expect(new PiMainAgentAdapter(runner, root).planStage(job, undefined, "search")).rejects.toThrow(
						"budget is exhausted",
					);
					expect(run).not.toHaveBeenCalled();
				}
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);
	it("cancels only waiting and retains the accepted identity", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-guide-cancel-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, { objective: "cancel waiting", workspaceRoot: root });
			await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
			const controller = new AbortController();
			const fixture = extension(root, controller.signal);
			await store.withJobLock(job.state.frame.jobId, "worker still active", async () => {
				const guidance = fixture.guide().catch((error: unknown) => error);
				await vi.waitFor(() => expect(fixture.notify).toHaveBeenCalledWith(expect.stringContaining("accepted")));
				controller.abort();
				expect(await guidance).toMatchObject({ message: "research guidance waiting was cancelled" });
				expect(fixture.notify).toHaveBeenCalledWith(expect.stringContaining("unconfirmed"));
			});
			const inbox = JSON.parse(
				await readFile(join(root, ".astra", "jobs", job.state.frame.jobId, "guidance-inbox.json"), "utf8"),
			);
			expect(inbox.pending).toEqual([inbox.receipts[0].requestId]);
			await store.withJobLock(job.state.frame.jobId, "later safe consumer", () => applyPendingGuidance(job));
			expect(
				Object.values(job.state.graph.nodes).filter(
					(node) => node.domainRef === `guidance-request:${inbox.receipts[0].requestId}`,
				),
			).toHaveLength(1);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it.each(["unknown", "wrong-job", "wrong-legacy-job", "invalid-path"])(
		"rejects %s supervisor ownership instead of waiting forever",
		async (fault) => {
			const root = await mkdtemp(join(tmpdir(), "astra-guide-owner-"));
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, { objective: "reject invalid owner", workspaceRoot: root });
				await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
				const lock = join(root, ".astra", "jobs", job.state.frame.jobId, "supervisor.lock.d");
				const token = "00000000-0000-4000-8000-000000000010";
				// Explicit invalid lock injection; not a newly counted production root cause.
				if (fault === "invalid-path") await writeFile(lock, "not a directory");
				else if (fault === "wrong-legacy-job")
					await writeFile(
						join(root, ".astra", "jobs", job.state.frame.jobId, "supervisor.lock"),
						JSON.stringify({
							jobId: "job_other",
							token,
							owner: "legacy wrong job",
							pid: process.pid,
							createdAt: new Date().toISOString(),
						}),
					);
				else {
					await mkdir(lock);
					await writeFile(
						join(lock, fault === "unknown" ? "unknown.json" : `owner-${token}.json`),
						JSON.stringify({
							jobId: "job_other",
							token,
							owner: "wrong job",
							pid: process.pid,
							createdAt: new Date().toISOString(),
						}),
					);
				}
				const fixture = extension(root);
				await expect(fixture.guide()).rejects.toThrow();
				expect(fixture.notify).toHaveBeenCalledWith(expect.stringContaining("unconfirmed"));
				const inbox = JSON.parse(
					await readFile(join(root, ".astra", "jobs", job.state.frame.jobId, "guidance-inbox.json"), "utf8"),
				);
				expect(inbox.pending).toEqual([inbox.receipts[0].requestId]);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);

	it.each(["recovery", "snapshot", "ack"])(
		"exposes entered %s failure without retrying a Busy error",
		async (fault) => {
			const root = await mkdtemp(join(tmpdir(), "astra-guide-entered-"));
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, { objective: "application failure", workspaceRoot: root });
				await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
				let injected = 0;
				const originalLock = JsonlAstraStore.prototype.withJobLock;
				let commandEntries = 0;
				vi.spyOn(JsonlAstraStore.prototype, "withJobLock").mockImplementation(function (
					this: JsonlAstraStore,
					id,
					owner,
					operation,
				) {
					return originalLock.call(this, id, owner, async () => {
						if (owner === `guidance_${process.pid}`) commandEntries++;
						return operation();
					});
				});
				if (fault === "recovery")
					vi.spyOn(ResearchJob.prototype, "recoverMainAgentDeliveries").mockImplementation(async () => {
						injected++;
						throw new ResearchJobBusyError("injected entered recovery failure");
					});
				if (fault === "snapshot") {
					const original = JsonlAstraStore.prototype.writeSnapshot;
					vi.spyOn(JsonlAstraStore.prototype, "writeSnapshot").mockImplementation(async function (
						this: JsonlAstraStore,
						snapshot,
					) {
						if (
							Object.values(snapshot.graph.nodes).some((node) => node.domainRef?.startsWith("guidance-request:"))
						) {
							injected++;
							throw new ResearchJobBusyError("injected entered snapshot failure");
						}
						await original.call(this, snapshot);
					});
				}
				if (fault === "ack") {
					const original = contractFiles.atomicWriteJson;
					vi.spyOn(contractFiles, "atomicWriteJson").mockImplementation(async (path, value) => {
						if (path.endsWith("guidance-inbox.json") && (value as { pending: string[] }).pending.length === 0) {
							injected++;
							throw new ResearchJobBusyError("injected entered ack failure");
						}
						await original(path, value);
					});
				}
				const fixture = extension(root);
				await expect(fixture.guide()).rejects.toThrow(`entered ${fault} failure`);
				expect(commandEntries).toBe(1);
				// Existing inbox helper retries LockedError up to 100 times; command entry must still remain one.
				expect(injected).toBe(fault === "ack" ? 100 : 1);
				expect(fixture.notify).toHaveBeenCalledWith(expect.stringContaining("unconfirmed"));
				const inbox = JSON.parse(
					await readFile(join(root, ".astra", "jobs", job.state.frame.jobId, "guidance-inbox.json"), "utf8"),
				);
				expect(inbox.pending).toHaveLength(1);
				vi.restoreAllMocks();
				const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
				await store.withJobLock(job.state.frame.jobId, "later recovery", () => applyPendingGuidance(reopened));
				expect(
					Object.values(reopened.state.graph.nodes).filter(
						(node) => node.domainRef === `guidance-request:${inbox.receipts[0].requestId}`,
					),
				).toHaveLength(1);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);
	it.each(["completed", "paused", "stage", "budget", "research", "interactive", "maxTicks"] as const)(
		"applies accepted guidance after the %s holder stops",
		async (boundary) => {
			const root = await mkdtemp(join(tmpdir(), "astra-guide-tail-"));
			let release!: () => void;
			const hold = new Promise<void>((resolve) => {
				release = resolve;
			});
			let entered!: () => void;
			const atTail = new Promise<void>((resolve) => {
				entered = resolve;
			});
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, {
					objective: "tail instruction",
					workspaceRoot: root,
					...(boundary === "completed"
						? {
								definitions: DEFAULT_STAGES.filter((stage) =>
									["result-to-claim", "research-review"].includes(stage.id),
								),
							}
						: {}),
				});
				if (boundary === "completed") await complete(job);
				if (boundary === "paused") await job.pause("operator pause");
				if (boundary === "stage")
					await job.requireUserGate({
						kind: "stage",
						stageId: "validation",
						phase: "route",
						reason: "stage permission",
					});
				if (boundary === "budget")
					await job.requireUserGate({
						kind: "budget",
						stageId: "validation",
						limit: "maxTurns",
						requiredMinimum: 9999,
						reason: "budget permission",
					});
				if (boundary === "research")
					await job.requireUserGate({
						kind: "research",
						stageId: "validation",
						question: "Which method?",
						reason: "research preference",
					});
				await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
				const before = job.state.frame;
				let driver: Promise<unknown>;
				if (boundary === "completed") {
					const original = ResearchJob.prototype.releaseLease;
					// Fault timing injection only: delay the real final lease release after its last drain.
					vi.spyOn(ResearchJob.prototype, "releaseLease").mockImplementation(async function (
						this: ResearchJob,
						owner,
					) {
						entered();
						await hold;
						await original.call(this, owner);
					});
					driver = runResearchControl({ action: "tick", jobId: job.state.frame.jobId }, root);
				} else if (boundary === "maxTicks") {
					vi.stubEnv("ASTRA_MAX_TICKS", "0");
					const original = JsonlAstraStore.prototype.withJobLock;
					let calls = 0;
					vi.spyOn(JsonlAstraStore.prototype, "withJobLock").mockImplementation(function (
						this: JsonlAstraStore,
						id,
						owner,
						operation,
					) {
						return original.call(this, id, owner, async () => {
							const value = await operation();
							if (owner === `control_${process.pid}` && ++calls === 3) {
								entered();
								await hold;
							}
							return value;
						});
					});
					driver = runResearchControl({ action: "resume", jobId: job.state.frame.jobId }, root).catch(
						(error: unknown) => error,
					);
				} else {
					driver = store.withJobLock(job.state.frame.jobId, "interactive tail", async () => {
						await applyPendingGuidance(job);
						entered();
						await hold;
					});
				}
				await atTail;
				const fixture = extension(root);
				let ended = false;
				const guide = fixture.guide().then(() => {
					ended = true;
				});
				await vi.waitFor(() =>
					expect(fixture.notify.mock.calls.some(([message]) => String(message).includes("accepted"))).toBe(true),
				);
				const returnedBeforeUnlock = ended;
				release();
				const result = await driver;
				await guide;
				const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
				const inbox = JSON.parse(
					await readFile(join(root, ".astra", "jobs", job.state.frame.jobId, "guidance-inbox.json"), "utf8"),
				) as { pending: string[]; receipts: Array<{ requestId: string }> };
				expect(returnedBeforeUnlock).toBe(false);
				expect(inbox.receipts).toHaveLength(1);
				expect(inbox.pending).toEqual([]);
				expect(
					Object.values(reopened.state.graph.nodes).filter(
						(node) => node.domainRef === `guidance-request:${inbox.receipts[0].requestId}`,
					),
				).toHaveLength(1);
				if (boundary === "completed") {
					expect(reopened.state.frame.completedAt).toBe(before.completedAt);
					expect(reopened.state.frame.status).toBe("completed");
					expect(reopened.state.lease).toBeUndefined();
				}
				if (boundary === "paused") expect(reopened.state.paused).toBe(true);
				if (boundary === "stage" || boundary === "budget") {
					expect(reopened.state.paused).toBe(true);
					expect(reopened.state.frame.userGate).toEqual(before.userGate);
				}
				if (boundary === "research") {
					expect(reopened.state.paused).toBe(false);
					expect(reopened.state.frame.userGate).toBeUndefined();
				}
				if (boundary === "maxTicks") expect(result).toMatchObject({ message: "research run exceeded 0 ticks" });
			} finally {
				release();
				await rm(root, { recursive: true, force: true });
			}
		},
	);

	it.each(["superseded", "exhausted"] as const)(
		"submits a real Pi replacement after an executed %s batch",
		async (status) => {
			const root = await mkdtemp(join(tmpdir(), "astra-pi-search-"));
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, {
					objective: "Pi replacement",
					workspaceRoot: root,
					automation: "full",
				});
				const initial = await job.recordStagePlan(plan(job, "first"));
				const evidence = await preparePlanEvidence(job, initial);
				await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
				const stop = async () => {
					throw new Error("offline deferred decision");
				};
				await new ResearchSupervisor(job, store, {
					worker: {
						run: async () => ({
							artifactType: "validation",
							refs: [],
							content: Object.fromEntries(
								job.definitions.validation.requiredOutputFields.map((field) => [field, "offline output"]),
							),
						}),
					},
					reviewer: { review: stop },
					mainAgent: {
						planStage: stop,
						decideEvidence: stop,
						decideAdoption: stop,
						decideSearch: stop,
						decideRoute: stop,
					},
				}).tick();
				if (status === "superseded") await job.resumeWithGuidance("replace the candidate hypotheses");
				else {
					for (const output of Object.values(job.state.evidence).filter((entry) => entry.type !== "stage-plan")) {
						const review = await job.recordReview(
							reviewFixture(job, {
								evidenceId: output.id,
								verdict: "fail",
								findings: ["No qualified candidate"],
								blocking: false,
							}),
						);
						await job.recordCandidateEvaluationFromReview(review.id);
					}
					await job.continueSearchBatch(
						Object.values(job.state.searchBatches)[0].id,
						"continue_fixture",
						"use orthogonal hypotheses",
					);
				}
				const previous = previousExecutedSearchBatch(job.state, "validation")!;
				expect(previous.status).toBe(status);
				expect(previous.executionStartedAt).toBeDefined();
				const runner = new PiChildSessionRunner();
				let submitted = "";
				vi.spyOn(runner, "run").mockImplementation(async (_cwd, id, _taskId, _attempt, role, _prompt, env) => {
					for (const [key, value] of Object.entries(env ?? {})) vi.stubEnv(key, value);
					vi.stubEnv("ASTRA_JOB_ID", id);
					vi.stubEnv("ASTRA_ROLE", role);
					const fixture = extension(root);
					const submit = (tasks: StagePlanManifest["tasks"]) =>
						fixture.tools.get("astra_submit_stage_plan")!.execute(
							"offline",
							{
								mode: "search",
								tasks: tasks.map((task) => ({
									...task,
									responsibilityBindings: [],
									responsibilityTransfers: [],
								})),
								rationale: "new independent hypotheses",
							},
							undefined,
							undefined,
							fixture.context,
						);
					const text = (result: Awaited<ReturnType<typeof submit>>) =>
						result.content
							.filter((item) => item.type === "text")
							.map((item) => item.text)
							.join("\n");
					// Explicit invalid context inputs must never select a different historical predecessor.
					vi.stubEnv("ASTRA_SEARCH_PREVIOUS_BATCH_ID", "search_unknown");
					expect(text(await submit(plan(job, "replacement").tasks))).toContain(
						"previous search batch does not match",
					);
					vi.stubEnv("ASTRA_SEARCH_PREVIOUS_BATCH_ID", undefined);
					expect(text(await submit(plan(job, "replacement").tasks))).toContain(
						"previous search batch does not match",
					);
					vi.stubEnv("ASTRA_SEARCH_PREVIOUS_BATCH_ID", previous.id);
					vi.stubEnv("ASTRA_STAGE_ID", "literature");
					expect(text(await submit(plan(job, "replacement").tasks))).toContain("environment does not match");
					vi.stubEnv("ASTRA_STAGE_ID", "validation");
					const repeated = initial.tasks.map((task) => ({
						...task,
						hypothesis: `  ${task.hypothesis!.toUpperCase()}   `,
					}));
					expect(text(await submit(repeated))).toContain("repeats a hypothesis");
					const result = await submit(plan(job, "replacement").tasks);
					submitted = result.content
						.filter((item) => item.type === "text")
						.map((item) => item.text)
						.join("\n");
					expect(submitted).toContain("Stage plan manifest written");
					return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
				});
				const replacement = await new PiMainAgentAdapter(runner, root).planStage(job, undefined, "search");
				expect(submitted).toContain("Stage plan manifest written");
				await job.reload();
				await job.recordStagePlan(replacement);
				const batch = Object.values(job.state.searchBatches).find((entry) => entry.planId === replacement.id)!;
				expect(batch).toMatchObject({ round: 2, previousBatchId: previous.id, maxRounds: previous.maxRounds });
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);
});
