import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionCommandContext, ToolDefinition } from "@earendil-works/pi-coding-agent";
import type { TSchema } from "typebox";
import { describe, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import * as contractFiles from "../src/contracts.ts";
import { buildEffectiveTaskContract, searchBatchForPlan, semanticContractHash } from "../src/effective-contract.ts";
import { createAstraExtension } from "../src/extension.ts";
import { applyPendingGuidance, requestResearchGuidance } from "../src/guidance-control.ts";
import { PiChildSessionRunner, PiMainAgentAdapter } from "../src/pi-child-session.ts";
import { planReviewStatus, preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor, type WorkerRunResult } from "../src/supervisor.ts";
import type { ReviewVerdict, StagePlanManifest, TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

function searchPlan(job: ResearchJob, id: string): StagePlanManifest {
	return {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id,
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: `decision_${id}`,
		mode: "search",
		tasks: [1, 2].map((index) => ({
			key: `candidate_${index}`,
			objective: `${id} hypothesis ${index}`,
			inputArtifactRefs: [],
			requiredOutputFields: job.definitions.validation.requiredOutputFields,
			acceptanceChecks: ["Evaluate candidate"],
			failureSignals: ["No result"],
			successCriteria: ["Evaluate candidate"],
		})),
		rationale: "offline corrected scope",
		sessionRef: "fixture:main",
		createdAt: new Date().toISOString(),
	};
}

describe("runtime coordination", () => {
	it("retains the pending search plan after ordinary-error and starts it only after further approval", async () => {
		const store = new MemoryAstraStore();
		const definition = structuredClone(DEFAULT_STAGES.find((stage) => stage.id === "validation")!);
		const job = await ResearchJob.create(store, {
			objective: "retry pending search approval",
			workspaceRoot: "/workspace",
			automation: "full",
			definitions: [definition],
		});
		for (let index = 0; index < 3; index++) {
			const rejected = await job.recordStagePlan(searchPlan(job, `pending_rejected_${index}`));
			const evidence = await preparePlanEvidence(job, rejected);
			await job.recordReview(
				reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: "fail",
					findings: ["Clarify scope"],
					blocking: false,
				}),
			);
		}
		const plan = await job.recordStagePlan(searchPlan(job, "pending_retry"));
		await preparePlanEvidence(job, plan);
		const batch = Object.values(job.state.searchBatches).find((search) => search.planId === plan.id)!;
		let reviews = 0;
		const worker = vi.fn(async () => {
			throw new NonRetryableResearchError("offline stop after approved startup");
		});
		const stop = async () => {
			throw new NonRetryableResearchError("unexpected planning or selection");
		};
		const planStage = vi.fn(stop);
		const decideSearch = vi.fn(stop);
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: worker },
			reviewer: {
				review: async (evidence) => {
					reviews++;
					// Explicit ordinary adapter failure; canonical state is created through public job operations.
					if (reviews === 1) throw new Error("temporary reviewer failure");
					return reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] });
				},
			},
			mainAgent: { planStage, decideEvidence: stop, decideAdoption: stop, decideSearch, decideRoute: stop },
		});
		const first = await supervisor.tick();
		expect(first.recovered).toBe(true);
		expect(planReviewStatus(job, plan.id)).toBe("pending");
		expect(() => job.searchQualification(batch.id)).toThrow("stale, superseded or not current");
		expect(worker).not.toHaveBeenCalled();
		expect(decideSearch).not.toHaveBeenCalled();
		expect(Object.values(job.state.tasks).filter((task) => task.role === "worker")).toEqual([]);
		expect(Object.values(job.state.searchBatches).map((search) => search.round)).toEqual([1, 1, 1, 1]);
		await supervisor.tick();
		expect(planReviewStatus(job, plan.id)).toBe("passed");
		expect(worker).toHaveBeenCalledTimes(2);
		expect(job.state.searchBatches[batch.id].executionStartedAt).toBeDefined();
		expect(planStage).not.toHaveBeenCalled();
		expect(decideSearch).not.toHaveBeenCalled();
		expect(Object.values(job.state.stagePlans)).toHaveLength(4);
	});
	it("keeps candidate review quorum separate from single-pass plan approval", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-candidate-quorum-"));
		try {
			const definition = structuredClone(DEFAULT_STAGES.find((stage) => stage.id === "validation")!);
			definition.qualityPolicy!.minPassingReviews = 2;
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				objective: "candidate review quorum",
				workspaceRoot: root,
				automation: "full",
				definitions: [definition],
			});
			const plan = await job.recordStagePlan(searchPlan(job, "candidate_quorum"));
			const approval = await preparePlanEvidence(job, plan);
			await job.recordReview(reviewFixture(job, { evidenceId: approval.id, verdict: "pass", findings: [] }));
			expect(planReviewStatus(job, plan.id)).toBe("passed");
			const worker = vi.fn(async () => ({
				artifactType: "validation",
				refs: [],
				content: Object.fromEntries(
					definition.requiredOutputFields.map((field) => [field, "offline candidate output"]),
				),
			}));
			const stop = async () => {
				throw new Error("offline candidate review deferred");
			};
			const decideSearch = vi.fn(stop);
			await new ResearchSupervisor(job, store, {
				worker: { run: worker },
				reviewer: { review: stop },
				mainAgent: { planStage: stop, decideEvidence: stop, decideAdoption: stop, decideSearch, decideRoute: stop },
			}).tick();
			expect(worker).toHaveBeenCalledTimes(2);
			expect(decideSearch).not.toHaveBeenCalled();
			const batch = Object.values(job.state.searchBatches)[0];
			const evidence = Object.values(job.state.evidence).filter((entry) => entry.type !== "stage-plan");
			expect(evidence).toHaveLength(2);
			for (let round = 0; round < 2; round++) {
				for (const entry of evidence) {
					const review = await job.recordReview(
						reviewFixture(job, { evidenceId: entry.id, verdict: "pass", findings: [] }),
					);
					await job.recordCandidateEvaluationFromReview(review.id);
				}
				const qualification = job.searchQualification(batch.id);
				expect(qualification.ready).toBe(round === 1);
				expect(qualification.eligibleIds).toHaveLength(round === 1 ? 2 : 0);
			}
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it("replays a succeeded third attempt with its exact predecessor and keeps failed retries linked", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			objective: "retry identity replay",
			workspaceRoot: "/workspace",
			automation: "full",
		});
		const plan = await job.recordStagePlan(searchPlan(job, "retry"));
		const evidence = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		const stop = async () => {
			throw new NonRetryableResearchError("no adapter expected in identity fixture");
		};
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: stop },
			reviewer: { review: stop },
			mainAgent: {
				planStage: stop,
				decideEvidence: stop,
				decideAdoption: stop,
				decideSearch: stop,
				decideRoute: stop,
			},
		});
		const dispatcher = supervisor as unknown as { dispatchPlan(plan: StagePlanManifest): Promise<TaskPacket[]> };
		const first = (await dispatcher.dispatchPlan(plan))[0];
		await job.setTaskStatus(first.id, "failed");
		const second = (await dispatcher.dispatchPlan(plan))[0];
		expect(second).toMatchObject({ attempt: 2, supersedesTaskId: first.id });
		await job.setTaskStatus(second.id, "failed");
		const third = (await dispatcher.dispatchPlan(plan))[0];
		expect(third).toMatchObject({ attempt: 3, supersedesTaskId: second.id });
		await job.setTaskStatus(third.id, "succeeded");
		const before = job.state.eventSeq;
		const replayed = (await dispatcher.dispatchPlan(plan))[0];
		expect(replayed).toMatchObject({ id: third.id, attempt: 3, supersedesTaskId: second.id, status: "succeeded" });
		expect(job.state.eventSeq).toBe(before);
		await expect(job.dispatchTask({ ...replayed, supersedesTaskId: first.id })).rejects.toThrow(
			"identity declaration conflicts",
		);
		expect(job.state.eventSeq).toBe(before);
	});
	it("retains a guidance request when acknowledgment fails after its canonical event", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-guidance-ack-"));
		const persist = contractFiles.atomicWriteJson;
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, { objective: "ack recovery", workspaceRoot: root });
			const id = job.state.frame.jobId;
			await requestResearchGuidance(root, id, "one instruction", "ack-failure");
			let fail = true;
			vi.spyOn(contractFiles, "atomicWriteJson").mockImplementation(async (path, value) => {
				if (fail && path.endsWith("guidance-inbox.json") && (value as { pending: string[] }).pending.length === 0) {
					fail = false;
					throw new Error("injected guidance acknowledgment persistence failure");
				}
				await persist(path, value);
			});
			await expect(store.withJobLock(id, "consume", () => applyPendingGuidance(job))).rejects.toThrow(
				"acknowledgment persistence failure",
			);
			const restarted = (await ResearchJob.open(new JsonlAstraStore(root), id))!;
			await store.withJobLock(id, "restart", () => applyPendingGuidance(restarted));
			expect(
				(await store.readEvents(id)).filter((event) => event.event.type === "user_guidance_recorded"),
			).toHaveLength(1);
			expect(
				JSON.parse(await readFile(join(root, ".astra", "jobs", id, "guidance-inbox.json"), "utf8")).pending,
			).toEqual([]);
		} finally {
			vi.restoreAllMocks();
			await rm(root, { recursive: true, force: true });
		}
	});

	it("keeps concurrent submissions in persistent acceptance order across confirmation and job identities", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-guidance-fifo-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, { objective: "concurrent guidance", workspaceRoot: root });
			const other = await ResearchJob.create(store, { objective: "other guidance job", workspaceRoot: root });
			const id = job.state.frame.jobId;
			const requests = await Promise.all([
				requestResearchGuidance(root, id, "first", "first"),
				requestResearchGuidance(root, id, "second", "second"),
			]);
			await requestResearchGuidance(root, other.state.frame.jobId, "another job", "first");
			await store.withJobLock(id, "consume", () => applyPendingGuidance(job));
			await requestResearchGuidance(root, id, "third", "third");
			await store.withJobLock(id, "consume again", () => applyPendingGuidance(job));
			expect(
				(await store.readEvents(id))
					.filter((event) => event.event.type === "user_guidance_recorded")
					.map((event) => (event.event.type === "user_guidance_recorded" ? event.event.node.domainRef : "")),
			).toEqual([
				...requests
					.sort((a, b) => a.sequence - b.sequence)
					.map((request) => `guidance-request:${request.requestId}`),
				"guidance-request:third",
			]);
			expect(Object.values(other.state.graph.nodes).filter((node) => node.actor === "user")).toEqual([]);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it.each(["research", "stage", "budget", "operator"] as const)(
		"consumes ordinary guidance while preserving the %s pause contract",
		async (kind) => {
			const root = await mkdtemp(join(tmpdir(), "astra-guidance-gate-"));
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, { objective: "answer the correct gate", workspaceRoot: root });
				if (kind === "operator") await job.pause("operator pause");
				else
					await job.requireUserGate(
						kind === "research"
							? { kind, stageId: "validation", question: "Which method?", reason: "research preference" }
							: kind === "stage"
								? { kind, stageId: "validation", phase: "route", reason: "stage permission" }
								: {
										kind,
										stageId: "validation",
										limit: "maxTurns",
										requiredMinimum: 9999,
										reason: "budget permission",
									},
					);
				const gate = job.state.frame.userGate;
				await requestResearchGuidance(root, job.state.frame.jobId, "use the declared method", "gate-answer");
				await store.withJobLock(job.state.frame.jobId, "consume guidance", () => applyPendingGuidance(job));
				expect(job.state.paused).toBe(kind !== "research");
				expect(job.state.frame.userGate).toEqual(kind === "research" ? undefined : gate);
				expect(
					(await store.readEvents(job.state.frame.jobId)).filter(
						(event) => event.event.type === "user_guidance_recorded",
					),
				).toHaveLength(1);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);

	it("does not charge approved ready tasks that never reached a worker callback, and conservatively charges an ambiguous legacy batch", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, { objective: "ready is not executed", workspaceRoot: "/workspace" });
		const plan = await job.recordStagePlan(searchPlan(job, "ready"));
		const evidence = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		const contract = buildEffectiveTaskContract(job, plan, plan.tasks[0]);
		await job.dispatchTask({
			...contract,
			effectiveContractHash: semanticContractHash(contract),
			replayKey: `stage-plan:${plan.id}:${plan.tasks[0].key}`,
		});
		await job.pause("operator pause before worker launch");
		await job.resumeWithGuidance("replace the approved but unexecuted scope");
		const first = Object.values(job.state.searchBatches)[0];
		expect(first.status).toBe("superseded");
		expect(first.executionStartedAt).toBeUndefined();
		const next = searchPlan(job, "replacement");
		expect(searchBatchForPlan(job.state, job.definitions.validation, next).round).toBe(1);
		// Explicit legacy fixture: startup tracking did not exist, so a candidate task is ambiguous.
		const legacy = structuredClone(job.state);
		delete legacy.searchBatches[first.id].executionTracking;
		expect(searchBatchForPlan(legacy, job.definitions.validation, next).round).toBe(2);
		expect(job.state.searchBatches[first.id].round).toBe(1);
	});

	it("recovers a dead matching execution owner and refuses both legacy and current live supervisor locks", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-owner-recovery-"));
		try {
			const store = new JsonlAstraStore(root);
			const jobId = "job_owner";
			const jobRoot = join(root, ".astra", "jobs", jobId);
			const token = "00000000-0000-4000-8000-000000000002";
			await mkdir(join(jobRoot, "execution.lock"), { recursive: true });
			await writeFile(
				join(jobRoot, "execution.lock", `owner-${token}.json`),
				JSON.stringify({ jobId, owner: "dead driver", token, pid: 99999999, createdAt: new Date().toISOString() }),
			);
			expect(await store.readExecutionOwner(jobId)).toBeUndefined();
			await store.withExecutionLock(jobId, "replacement", async () => {
				expect((await store.readExecutionOwner(jobId))?.owner).toBe("replacement");
			});
			await store.withJobLock(jobId, "old current supervisor", async () => {
				await expect(store.readExecutionOwner(jobId)).rejects.toThrow("supervisor lock held");
				await expect(store.withExecutionLock(jobId, "new driver", async () => {})).rejects.toThrow(
					"supervisor lock held",
				);
			});
			await writeFile(
				join(jobRoot, "supervisor.lock"),
				JSON.stringify({
					owner: "legacy supervisor",
					token,
					pid: process.pid,
					createdAt: new Date().toISOString(),
				}),
			);
			await expect(store.readExecutionOwner(jobId)).rejects.toThrow("supervisor lock held");
			await expect(store.withExecutionLock(jobId, "new driver", async () => {})).rejects.toThrow(
				"supervisor lock held",
			);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it.each(["pi", "codex"])("shows the charged superseded search round in the %s planner prompt", async (backend) => {
		const root = await mkdtemp(join(tmpdir(), "astra-search-prompt-"));
		try {
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				objective: "actual execution stays charged",
				workspaceRoot: root,
				automation: "full",
			});
			const plan = await job.recordStagePlan(searchPlan(job, "executed"));
			const evidence = await preparePlanEvidence(job, plan);
			await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
			for (const specification of plan.tasks) {
				const contract = buildEffectiveTaskContract(job, plan, specification);
				await job.dispatchTask({
					...contract,
					effectiveContractHash: semanticContractHash(contract),
					replayKey: `stage-plan:${plan.id}:${specification.key}`,
				});
			}
			let calls = 0;
			const stop = async () => {
				throw new NonRetryableResearchError("offline stop");
			};
			await new ResearchSupervisor(job, store, {
				worker: {
					run: async (task) => {
						calls++;
						expect(job.state.searchBatches[task.searchBatchId!].executionStartedAt).toBeDefined();
						throw new NonRetryableResearchError("offline worker stopped after launch");
					},
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
			expect(calls).toBe(2);
			await job.resumeWithGuidance("replace the executed search");
			if (backend === "pi") {
				const runner = new PiChildSessionRunner();
				const run = vi.spyOn(runner, "run").mockResolvedValue({
					exitCode: 1,
					stdout: "",
					stderr: "offline prompt capture",
					jsonEvents: [],
					costUsd: 0,
				});
				await expect(new PiMainAgentAdapter(runner, root).planStage(job, undefined, "search")).rejects.toThrow(
					"offline prompt capture",
				);
				expect(run.mock.calls[0][5]).toContain("bounded search round 2/2");
			} else {
				const runner = new CodexAppServerRunner();
				const run = vi.spyOn(runner, "run").mockRejectedValue(new Error("offline prompt capture"));
				await expect(new CodexResearchAdapters(runner).planStage(job, undefined, "search")).rejects.toThrow(
					"offline prompt capture",
				);
				expect(run.mock.calls[0][0].prompt).toContain("bounded search round 2/2");
			}
			const next = await job.recordStagePlan(searchPlan(job, "next"));
			expect(Object.values(job.state.searchBatches).find((batch) => batch.planId === next.id)?.round).toBe(2);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it.each([process.pid, 99999999])(
		"rejects an execution owner with a different job identity and pid %s",
		async (pid) => {
			const root = await mkdtemp(join(tmpdir(), "astra-corrupt-owner-"));
			try {
				const store = new JsonlAstraStore(root);
				const directory = join(root, ".astra", "jobs", "job_target", "execution.lock");
				await mkdir(directory, { recursive: true });
				const token = "00000000-0000-4000-8000-000000000001";
				// Explicit corrupted-lock injection tests rejection, not an ordinary research path.
				await writeFile(
					join(directory, `owner-${token}.json`),
					JSON.stringify({
						jobId: "job_other",
						owner: "foreign owner",
						token,
						pid,
						createdAt: new Date().toISOString(),
					}),
				);
				await expect(store.readExecutionOwner("job_target")).rejects.toThrow("job identity");
				await expect(store.withExecutionLock("job_target", "driver", async () => {})).rejects.toThrow(
					"job identity",
				);
				expect(await readdir(directory)).toEqual([`owner-${token}.json`]);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);
	it.each([true, false])(
		"preserves two staggered worker outputs with public guidance=%s, including a snapshot failure",
		async (withGuidance) => {
			const root = await mkdtemp(join(tmpdir(), "astra-guidance-wave-"));
			try {
				const store = new JsonlAstraStore(root);
				const job = await ResearchJob.create(store, {
					objective: "two independent workers",
					workspaceRoot: root,
					automation: "full",
				});
				const plan = await job.recordStagePlan(searchPlan(job, "wave"));
				const approval = await preparePlanEvidence(job, plan);
				await job.recordReview(reviewFixture(job, { evidenceId: approval.id, verdict: "pass", findings: [] }));
				const tasks: TaskPacket[] = [];
				for (const specification of plan.tasks) {
					const contract = buildEffectiveTaskContract(job, plan, specification);
					tasks.push(
						await job.dispatchTask({
							...contract,
							effectiveContractHash: semanticContractHash(contract),
							replayKey: `stage-plan:${plan.id}:${specification.key}`,
						}),
					);
				}
				const releases = new Map<string, (result: WorkerRunResult) => void>();
				let workersStarted!: () => void;
				const started = new Promise<void>((resolve) => {
					workersStarted = resolve;
				});
				let faultInjected = false;
				const persist = store.writeSnapshot.bind(store);
				vi.spyOn(store, "writeSnapshot").mockImplementation(async (snapshot) => {
					if (
						!faultInjected &&
						Object.values(snapshot.evidence).some((evidence) => evidence.taskId === tasks[0].id)
					) {
						faultInjected = true;
						throw new Error("injected worker snapshot failure after durable output event");
					}
					await persist(snapshot);
				});
				const stop = async () => {
					throw new NonRetryableResearchError("offline fixture stops after worker wave");
				};
				const supervisor = new ResearchSupervisor(job, store, {
					worker: {
						run: async (task) =>
							new Promise((resolve) => {
								releases.set(task.id, resolve);
								if (releases.size === 2) workersStarted();
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
				});
				const tick = supervisor.tick();
				await started;
				await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
				const commands = new Map<string, (args: string, ctx: ExtensionCommandContext) => Promise<void> | void>();
				const tools = new Map<string, ToolDefinition<TSchema, unknown, unknown>>();
				const api = {
					getFlag: vi.fn(),
					on: vi.fn(),
					registerFlag: vi.fn(),
					appendEntry: vi.fn(),
					sendMessage: vi.fn(),
					registerCommand(
						name: string,
						command: { handler: (args: string, ctx: ExtensionCommandContext) => Promise<void> | void },
					) {
						commands.set(name, command.handler);
					},
					registerTool(tool: ToolDefinition<TSchema, unknown, unknown>) {
						tools.set(tool.name, tool);
					},
				} as unknown as ExtensionAPI;
				void createAstraExtension()(api);
				const context = {
					cwd: root,
					ui: { notify: vi.fn(), setStatus: vi.fn(), setWidget: vi.fn() },
				} as unknown as ExtensionCommandContext;
				const before = job.state.eventSeq;
				await expect(
					tools
						.get("research_dispatch")!
						.execute(
							"busy",
							{ objective: "in-flight dispatch", role: "worker", requiredOutputType: "validation" },
							undefined,
							undefined,
							context,
						),
				).rejects.toThrow("supervisor lock held");
				const guidancePromise = withGuidance
					? Promise.resolve(commands.get("research-guide")!("use both outputs", context))
					: undefined;
				if (withGuidance)
					await vi.waitFor(() =>
						expect(context.ui.notify).toHaveBeenCalledWith(expect.stringContaining("accepted")),
					);
				expect(job.state.eventSeq).toBe(before);
				const queued = withGuidance
					? (JSON.parse(
							await readFile(join(root, ".astra", "jobs", job.state.frame.jobId, "guidance-inbox.json"), "utf8"),
						) as { pending: string[] })
					: { pending: [] };
				const domainRef = `guidance-request:${queued.pending[0] ?? "none"}`;
				releases.get(tasks[0].id)!({ artifactType: "validation", refs: [], content: { content: "first output" } });
				for (let attempt = 0; attempt < 100 && job.state.tasks[tasks[0].id].status !== "succeeded"; attempt++) {
					await new Promise((resolve) => setTimeout(resolve, 10));
				}
				expect(Object.values(job.state.graph.nodes).some((node) => node.domainRef === domainRef)).toBe(false);
				releases.get(tasks[1].id)!({ artifactType: "validation", refs: [], content: { content: "second output" } });
				await tick;
				await guidancePromise;
				expect(faultInjected).toBe(true);
				for (const task of tasks) {
					expect(job.state.tasks[task.id].status).toBe("succeeded");
					expect(Object.values(job.state.evidence).some((evidence) => evidence.taskId === task.id)).toBe(true);
				}
				expect(Object.values(job.state.graph.nodes).filter((node) => node.domainRef === domainRef)).toHaveLength(
					withGuidance ? 1 : 0,
				);
				expect(job.state.lease).toBeUndefined();
				const events = await store.readEvents(job.state.frame.jobId);
				const guidance = events.findIndex((event) => event.event.type === "user_guidance_recorded");
				if (withGuidance)
					expect(
						events
							.filter(
								(event) =>
									event.event.type === "evidence_recorded" &&
									tasks.some(
										(task) =>
											event.event.type === "evidence_recorded" && task.id === event.event.evidence.taskId,
									),
							)
							.every((event) => event.seq < events[guidance].seq),
					).toBe(true);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);

	it("replays FIFO guidance once after an event persisted but its snapshot failed", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-guidance-restart-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, { objective: "guidance replay", workspaceRoot: root });
			const id = job.state.frame.jobId;
			await requestResearchGuidance(root, id, "first instruction", "first");
			await requestResearchGuidance(root, id, "second instruction", "second");
			const persist = store.writeSnapshot.bind(store);
			let fail = true;
			vi.spyOn(store, "writeSnapshot").mockImplementation(async (snapshot) => {
				if (
					fail &&
					Object.values(snapshot.graph.nodes).some((node) => node.domainRef === "guidance-request:first")
				) {
					fail = false;
					throw new Error("injected guidance snapshot failure");
				}
				await persist(snapshot);
			});
			await expect(store.withJobLock(id, "consume", () => applyPendingGuidance(job))).rejects.toThrow(
				"snapshot failure",
			);
			const restarted = (await ResearchJob.open(new JsonlAstraStore(root), id))!;
			await store.withJobLock(id, "restart", () => applyPendingGuidance(restarted));
			await expect(requestResearchGuidance(root, id, "changed instruction", "first")).rejects.toThrow(
				"different content",
			);
			expect(
				(await store.readEvents(id))
					.filter((event) => event.event.type === "user_guidance_recorded")
					.map((event) => (event.event.type === "user_guidance_recorded" ? event.event.node.domainRef : "")),
			).toEqual(["guidance-request:first", "guidance-request:second"]);
			const path = join(root, ".astra", "jobs", id, "guidance-inbox.json");
			const inbox = JSON.parse(await readFile(path, "utf8"));
			expect(inbox.pending).toEqual([]);
			// Explicit fault injection: restore the unacknowledged request after its event is durable.
			inbox.pending = ["first"];
			await writeFile(path, JSON.stringify(inbox));
			await store.withJobLock(id, "ack recovery", () => applyPendingGuidance(restarted));
			expect(
				(await store.readEvents(id)).filter((event) => event.event.type === "user_guidance_recorded"),
			).toHaveLength(2);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it.each(["fail", "partial", "blocked", "low-score", "reject"])(
		"does not charge search rounds for repeatedly %s plans",
		async (outcome) => {
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				objective: "correct search plans",
				workspaceRoot: "/workspace",
			});
			for (let index = 0; index < 3; index++) {
				const plan = await job.recordStagePlan(searchPlan(job, `rejected_${index}`));
				const evidence = await preparePlanEvidence(job, plan);
				if (outcome === "reject") await job.decideEvidence(evidence.id, false);
				else
					await job.recordReview(
						reviewFixture(job, {
							evidenceId: evidence.id,
							verdict: outcome === "low-score" ? "pass" : (outcome as ReviewVerdict),
							score: outcome === "low-score" ? 0.2 : undefined,
							findings: ["Clarify scope"],
							blocking: false,
						}),
					);
			}
			expect(Object.values(job.state.searchBatches).map((batch) => batch.round)).toEqual([1, 1, 1]);
			expect(Object.values(job.state.tasks).filter((task) => task.role === "worker")).toEqual([]);
		},
	);

	it("holds one live execution identity across supervisor lock gaps", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-execution-owner-"));
		try {
			const store = new JsonlAstraStore(root);
			const other = new JsonlAstraStore(root);
			await store.withExecutionLock("job_execution", "first driver", async () => {
				expect(await other.readExecutionOwner("job_execution")).toMatchObject({
					jobId: "job_execution",
					owner: "first driver",
					pid: process.pid,
				});
				await store.withJobLock("job_execution", "tick", async () => {});
				await expect(other.withExecutionLock("job_execution", "second driver", async () => {})).rejects.toThrow(
					"execution lock held",
				);
			});
			expect(await other.readExecutionOwner("job_execution")).toBeUndefined();
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
});
