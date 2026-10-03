import { readFile, realpath, stat, writeFile } from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import type { AgentMessage, AgentToolResult } from "@earendil-works/pi-agent-core";
import type { ExtensionAPI, ExtensionContext, ExtensionFactory, ToolCallEvent } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import {
	readTaskPacket,
	taskStageContract,
	writeMainDecisionManifest,
	writeReviewerOutputManifest,
	writeStagePlanManifest,
	writeWorkerOutputManifest,
} from "./contracts.ts";
import { compactLiteratureSearch, type Fetcher } from "./literature.ts";
import { searchLiterature } from "./literature-search.ts";
import { appendJobMemory, loadStageSkills, readJobMemory } from "./memory.ts";
import { migratePmcli } from "./migration.ts";
import { ResearchJob } from "./research.ts";
import { buildResearchBoard, formatResearchBoard } from "./research-board.ts";
import {
	decodeResearchControl,
	parseResearchControlArgs,
	type ResearchControlRequest,
	type ResearchControlResult,
	runResearchControl,
} from "./research-control.ts";
import { JsonlAstraStore } from "./store.ts";
import { readVersionedFile, taskRecoveryMaterials } from "./task-workspace.ts";
import type {
	AutomationLevel,
	MainAgentDecisionManifest,
	PlannedTask,
	ReviewVerdict,
	Role,
	StagePlanManifest,
	TaskPacket,
} from "./types.ts";
import { validateWorkerSubmission } from "./worker-submission.ts";

const ACTIVE_JOB_FILE = [".astra", "active-job.json"];
const MAX_WORKER_DIRECT_READ_BYTES = 16 * 1024;
const reviewAssessmentFields = {
	verdict: Type.Union([Type.Literal("pass"), Type.Literal("fail"), Type.Literal("partial"), Type.Literal("blocked")]),
	findings: Type.Array(Type.String()),
	score: Type.Number({ minimum: 0, maximum: 1 }),
	criteria: Type.Array(
		Type.Object({
			criterion: Type.String(),
			passed: Type.Boolean(),
			score: Type.Number({ minimum: 0, maximum: 1 }),
			evidenceRefs: Type.Array(Type.String()),
			rationale: Type.String(),
		}),
		{ minItems: 1 },
	),
	verifiedRefs: Type.Array(Type.String(), { minItems: 1 }),
};

interface AstraExtensionOptions {
	role?: Exclude<Role, "supervisor">;
	defaultAutomation?: AutomationLevel;
	jobId?: string;
	literatureFetcher?: Fetcher;
}

interface AstraContext {
	store: JsonlAstraStore;
	job?: ResearchJob;
	role: Exclude<Role, "supervisor">;
	jobIdOverride?: string;
	taskPacket?: TaskPacket;
}

function result(text: string, details: unknown = {}): { content: [{ type: "text"; text: string }]; details: unknown } {
	return { content: [{ type: "text", text }], details };
}

function terminalResult(text: string, details: unknown): AgentToolResult<unknown> {
	return { ...result(text, details), terminate: true };
}

async function readActiveJobId(cwd: string): Promise<string | undefined> {
	try {
		const data = JSON.parse(await readFile(join(cwd, ...ACTIVE_JOB_FILE), "utf8")) as { jobId?: string };
		return data.jobId;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
		throw error;
	}
}

async function setActiveJobId(cwd: string, jobId: string): Promise<void> {
	await writeFile(join(cwd, ...ACTIVE_JOB_FILE), `${JSON.stringify({ jobId }, null, 2)}\n`, "utf8");
}

function projectRoot(ctx: ExtensionContext): string {
	return resolve(process.env.ASTRA_PROJECT_ROOT ?? ctx.cwd);
}

function isInside(root: string, path: string): boolean {
	const value = relative(root, path);
	return value === "" || (!value.startsWith("..") && !isAbsolute(value));
}

function absoluteShellPaths(command: string): string[] {
	return [...command.matchAll(/(?:^|[\s"'=<>])(\/(?!\/)[A-Za-z0-9._$~*?+-][^\s"'|;&<>()]*)/gu)].map(
		(match) => match[1],
	);
}

async function loadJob(ctx: AstraContext, cwd: string): Promise<ResearchJob | undefined> {
	if (ctx.job) return ctx.job;
	const jobId = ctx.jobIdOverride ?? (await readActiveJobId(cwd));
	if (!jobId) return undefined;
	ctx.job = await ResearchJob.open(ctx.store, jobId);
	return ctx.job;
}

export function createAstraExtension(options: AstraExtensionOptions = {}): ExtensionFactory {
	return (pi: ExtensionAPI): void => {
		let state: AstraContext | undefined;
		let compactionRequested = false;
		let researchControlStarted = false;
		let taskToolCallCount = 0;
		let literatureSearchCallCount = 0;
		let terminalSubmissionCompleted = false;
		pi.registerFlag("astra-research-control", {
			description: "Run an Astra research control action through the Pi runtime",
			type: "string",
		});
		const getState = (ctx: ExtensionContext): AstraContext => {
			const root = projectRoot(ctx);
			if (!state || state.store.root !== join(root, ".astra")) {
				const role =
					(process.env.ASTRA_ROLE as Exclude<Role, "supervisor"> | undefined) ?? options.role ?? "main-agent";
				state = { store: new JsonlAstraStore(root), role };
				state.jobIdOverride = options.jobId ?? process.env.ASTRA_JOB_ID;
			}
			return state;
		};

		const loadTaskPacket = async (ctx: ExtensionContext): Promise<TaskPacket | undefined> => {
			const current = getState(ctx);
			if (current.taskPacket) return current.taskPacket;
			const packetPath = process.env.ASTRA_TASK_PACKET;
			if (!packetPath) return undefined;
			current.taskPacket = await readTaskPacket(packetPath);
			return current.taskPacket;
		};
		const projectStatus = (status: ReturnType<ResearchJob["status"]>, ctx: ExtensionContext): void => {
			ctx.ui.setStatus(
				"astra-research",
				`${status.status}:${status.activeStageId} | ${status.unresolvedObjections} objections | ${status.activeSearches} searches`,
			);
			const lines = [
				`Astra: ${status.activeStageId}`,
				`Mode: ${status.automation}`,
				`State: ${status.status}`,
				`Event: ${status.eventSeq}`,
				`Obligations: ${status.openObligations}`,
				`Open: ${status.openQuestions} questions; ${status.unresolvedObjections} objections`,
				`Search: ${status.activeSearches} active; ${status.activeHypotheses} hypotheses`,
				`Budget: ${status.budget.tasksUsed}/${status.budget.maxTasks} tasks; ${status.budget.turnsUsed}/${status.budget.maxTurns} turns; $${status.budget.costUsdUsed.toFixed(4)}${status.budget.maxCostUsd === undefined ? "" : `/$${status.budget.maxCostUsd.toFixed(4)}`}`,
				`Next: ${status.nextAction}`,
			];
			if (status.userGate) lines.push(`Gate: ${status.userGate.reason}`);
			ctx.ui.setWidget("astra-research", lines);
		};
		const recordResearchControlResult = async (
			controlResult: ResearchControlResult,
			ctx: ExtensionContext,
			notify: boolean,
		): Promise<void> => {
			const current = getState(ctx);
			if (controlResult.jobId) current.job = await ResearchJob.open(current.store, controlResult.jobId);
			pi.appendEntry("astra_research_result", controlResult);
			pi.sendMessage({
				customType: "astra_research_result",
				content: [{ type: "text", text: JSON.stringify(controlResult, null, 2) }],
				display: true,
				details: controlResult,
			});
			if (controlResult.status) projectStatus(controlResult.status, ctx);
			if (notify) ctx.ui.notify(JSON.stringify(controlResult, null, 2));
		};
		const executeResearchControl = async (request: ResearchControlRequest, ctx: ExtensionContext): Promise<void> => {
			const controlResult = await runResearchControl(request, projectRoot(ctx));
			await recordResearchControlResult(controlResult, ctx, true);
		};

		pi.on("session_start", async (_event, ctx) => {
			const current = getState(ctx);
			await loadTaskPacket(ctx);
			await migratePmcli(projectRoot(ctx));
			const controlValue = pi.getFlag("astra-research-control");
			if (!researchControlStarted && typeof controlValue === "string") {
				researchControlStarted = true;
				const controlResult = await runResearchControl(decodeResearchControl(controlValue), projectRoot(ctx));
				await recordResearchControlResult(controlResult satisfies ResearchControlResult, ctx, false);
				return;
			}
			const job = await loadJob(current, projectRoot(ctx));
			if (job) {
				const status = job.status();
				projectStatus(status, ctx);
				pi.appendEntry("astra_status", status);
			}
		});

		pi.on("before_agent_start", async (event, ctx) => {
			const current = getState(ctx);
			const job = await loadJob(current, projectRoot(ctx));
			if (!job) return;
			const status = job.status();
			const skills = await loadStageSkills(projectRoot(ctx), status.activeStageId, current.role);
			const memory = await readJobMemory(projectRoot(ctx), status.jobId, 8);
			const skillContext = skills.length > 0 ? `\n\nAstra stage/role skills:\n${skills.join("\n\n")}` : "";
			const memoryContext =
				memory.length > 0
					? `\n\nRecent durable research memory:\n${memory.map((entry) => `- ${entry.kind}: ${entry.content}`).join("\n")}`
					: "";
			return {
				systemPrompt: `${event.systemPrompt}\n\nAstra mission context: job ${status.jobId}; objective: ${status.objective}; active stage: ${status.activeStageId}; open obligations: ${status.openObligations}; next action: ${status.nextAction ?? `dispatch ${status.activeStageId}`}. Treat .astra as canonical research state.${skillContext}${memoryContext}`,
			};
		});

		pi.on("context", async (event, ctx) => {
			const current = getState(ctx);
			const job = await loadJob(current, projectRoot(ctx));
			if (!job) return;
			const status = job.status();
			const memory = await readJobMemory(projectRoot(ctx), status.jobId, 8);
			const missionContext: AgentMessage = {
				role: "custom",
				customType: "astra_context",
				content: `Astra status: ${status.activeStageId}; ${status.openObligations} open obligations; ${status.readyTasks} ready tasks; ${memory.length} durable memory entries.`,
				display: false,
				details: status,
				timestamp: Date.now(),
			};
			return { messages: [...event.messages, missionContext] };
		});

		pi.on("tool_call", async (event: ToolCallEvent, ctx) => {
			const current = getState(ctx);
			const job = await loadJob(current, projectRoot(ctx));
			if (!job) return;
			const input = event.input as Record<string, unknown>;
			const packet =
				current.role === "worker" || current.role === "reviewer" ? await loadTaskPacket(ctx) : undefined;
			const terminalTool =
				current.role === "worker"
					? "astra_submit_worker_output"
					: current.role === "reviewer"
						? "astra_submit_review"
						: undefined;
			if (packet && event.toolName !== terminalTool) {
				taskToolCallCount++;
				if (taskToolCallCount > packet.budget.maxToolCalls) {
					return {
						block: true,
						reason: `TaskPacket maxToolCalls exceeded: ${taskToolCallCount}/${packet.budget.maxToolCalls}`,
						terminate: true,
					};
				}
			}
			if (packet && terminalSubmissionCompleted) {
				return { block: true, reason: "TaskPacket terminal submission already completed", terminate: true };
			}
			if (packet && event.toolName !== terminalTool && !packet.allowedTools.includes(event.toolName)) {
				return { block: true, reason: `${current.role} TaskPacket does not allow tool ${event.toolName}` };
			}
			if (
				current.role === "reviewer" &&
				[
					"write",
					"edit",
					"bash",
					"research_adopt",
					"research_dispatch",
					"research_decide_evidence",
					"research_record_evidence",
				].includes(event.toolName)
			) {
				return { block: true, reason: "reviewer role is read-only" };
			}
			if (
				current.role === "worker" &&
				["research_dispatch", "research_review", "research_decide_evidence", "research_adopt"].includes(
					event.toolName,
				)
			) {
				return { block: true, reason: "worker role cannot dispatch, review, decide, or adopt research state" };
			}
			if (
				current.role !== "main-agent" &&
				["research_start", "research_pause", "research_resume"].includes(event.toolName)
			) {
				return { block: true, reason: "only the main-agent role can control the research job" };
			}
			if (packet && ["read", "write", "edit", "grep", "find", "ls"].includes(event.toolName)) {
				const requestedPath = String(input.path ?? ".").replace(/^@/, "");
				const target = resolve(ctx.cwd, requestedPath);
				const readOnlyTool = ["read", "grep", "find", "ls"].includes(event.toolName);
				let allowedRoots = [resolve(ctx.cwd)];
				if (current.role === "worker" && readOnlyTool && !isInside(resolve(ctx.cwd), target)) {
					try {
						const declared: unknown = JSON.parse(process.env.ASTRA_RECOVERY_READ_ROOTS ?? "[]");
						const recovery = await taskRecoveryMaterials(packet, job);
						if (
							!Array.isArray(declared) ||
							declared.some((root) => typeof root !== "string" || !recovery?.readRoots.includes(root))
						)
							return { block: true, reason: "retry recovery roots are not host-validated" };
						allowedRoots = [...allowedRoots, ...declared];
					} catch (error) {
						return { block: true, reason: error instanceof Error ? error.message : String(error) };
					}
				}
				if (!allowedRoots.some((root) => isInside(root, target)))
					return {
						block: true,
						reason:
							"worker file tools must stay inside the isolated workspace or declared read-only recovery roots",
					};
				let ancestor = target;
				while (true) {
					try {
						const resolvedTarget = await realpath(ancestor);
						if (!allowedRoots.some((root) => isInside(root, resolvedTarget)))
							return {
								block: true,
								reason: "worker file path escapes its allowed root through a symbolic link",
							};
						break;
					} catch (error) {
						if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
						const parent = dirname(ancestor);
						if (parent === ancestor) throw error;
						ancestor = parent;
					}
				}
				if (current.role === "worker" && event.toolName === "read" && requestedPath !== "ASTRA_TASK_CONTEXT.json") {
					try {
						const metadata = await stat(target);
						if (metadata.isFile() && metadata.size > MAX_WORKER_DIRECT_READ_BYTES) {
							const recovery = !isInside(resolve(ctx.cwd), target)
								? "Use grep with an exact pattern and a small limit to inspect this read-only recovery file."
								: packet.allowedTools.includes("bash")
									? "Use a bounded bash command such as jq, sed, head, tail, or wc and keep the output concise."
									: "Use inputs.canonicalArtifacts and smaller summary files, then submit the required output.";
							return {
								block: true,
								reason: `Direct read blocked for ${requestedPath} (${metadata.size} bytes). Do not retry this file with a different limit or offset. ${recovery}`,
							};
						}
					} catch (error) {
						if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
					}
				}
				if (["write", "edit"].includes(event.toolName) && packet.writeAuthority !== "workspace-write") {
					return { block: true, reason: "worker TaskPacket is read-only" };
				}
			}
			if (event.toolName === "bash") {
				const command = String(input.command ?? "");
				if (/\b(rm|git\s+(reset|clean)|sudo|mkfs|shutdown|mount|chroot)\b/.test(command)) {
					return { block: true, reason: "Astra destructive command gate" };
				}
				if (packet && /(?:\.\.\/|\$\{?HOME\}?|~\/|\bcd\s)/.test(command)) {
					return { block: true, reason: "worker shell command may not leave the isolated task workspace" };
				}
				if (packet) {
					const inputResourceRoots = (() => {
						try {
							const parsed = JSON.parse(process.env.ASTRA_INPUT_RESOURCE_ROOTS ?? "[]") as unknown;
							return Array.isArray(parsed)
								? parsed.filter((value): value is string => typeof value === "string")
								: [];
						} catch {
							return [];
						}
					})();
					const allowedRoots = [ctx.cwd, process.env.ASTRA_RESOURCE_ROOT, ...inputResourceRoots]
						.filter((root): root is string => typeof root === "string" && root.length > 0)
						.map((root) => resolve(root));
					const outsidePath = absoluteShellPaths(command).find((path) => {
						if (path === "/dev/null") return false;
						const target = resolve(path);
						return !allowedRoots.some((root) => isInside(root, target));
					});
					if (outsidePath) {
						return {
							block: true,
							reason: `worker shell absolute path is outside declared resource roots: ${outsidePath}`,
						};
					}
				}
			}
		});

		pi.on("turn_end", async (event, ctx) => {
			const current = getState(ctx);
			const job = await loadJob(current, projectRoot(ctx));
			if (job) {
				const status = job.status();
				projectStatus(status, ctx);
				pi.appendEntry("astra_status", status);
			}
			if (process.env.ASTRA_FORCE_COMPACTION === "1" && !compactionRequested && ctx.isIdle()) {
				compactionRequested = true;
				ctx.compact();
			}
			const packet =
				current.role === "worker" || current.role === "reviewer" ? await loadTaskPacket(ctx) : undefined;
			if (packet && !terminalSubmissionCompleted && event.turnIndex + 1 >= packet.budget.maxTurns) {
				pi.appendEntry("astra_task_budget_exhausted", {
					taskId: packet.id,
					limit: "maxTurns",
					used: event.turnIndex + 1,
					max: packet.budget.maxTurns,
				});
				ctx.abort();
			}
		});

		pi.on("session_before_compact", async (_event, ctx) => {
			const current = getState(ctx);
			const job = await loadJob(current, projectRoot(ctx));
			if (!job) return;
			const status = job.status();
			await appendJobMemory(projectRoot(ctx), status.jobId, {
				stageId: status.activeStageId,
				role: current.role,
				kind: "checkpoint",
				content: `eventSeq=${status.eventSeq}; next=${status.nextAction}`,
				sourceRefs: [`job:${status.jobId}`, `event:${status.eventSeq}`],
			});
			pi.appendEntry("astra_checkpoint", {
				jobId: status.jobId,
				activeStageId: status.activeStageId,
				openObligations: status.openObligations,
				readyTasks: status.readyTasks,
				eventSeq: status.eventSeq,
				nextAction: status.nextAction,
			});
			return {
				customInstructions: `Preserve Astra checkpoint: job ${status.jobId}, objective ${status.objective}, active stage ${status.activeStageId}, open obligations ${status.openObligations}, ready tasks ${status.readyTasks}, next action ${status.nextAction}, and canonical artifact refs.`,
			};
		});

		pi.on("session_shutdown", async (_event, ctx) => {
			const current = getState(ctx);
			const job = await loadJob(current, projectRoot(ctx));
			if (job) {
				const status = job.status();
				await appendJobMemory(projectRoot(ctx), status.jobId, {
					stageId: status.activeStageId,
					role: current.role,
					kind: "checkpoint",
					content: `shutdown eventSeq=${status.eventSeq}; next=${status.nextAction}`,
					sourceRefs: [`job:${status.jobId}`, `event:${status.eventSeq}`],
				});
				pi.appendEntry("astra_checkpoint", job.status());
				pi.appendEntry("astra_shutdown", job.status());
			}
		});

		const workerOutputType = process.env.ASTRA_REQUIRED_OUTPUT_TYPE ?? process.env.ASTRA_STAGE_ID;
		const workerContentParameters =
			workerOutputType === "result-to-claim"
				? Type.Object(
						{
							scientificOutcome: Type.Optional(Type.String({ maxLength: 40 })),
							missionCoverage: Type.Optional(Type.String({ maxLength: 40 })),
							claims: Type.Optional(Type.Array(Type.Unknown(), { maxItems: 20 })),
							supportingResults: Type.Optional(Type.Array(Type.Unknown(), { maxItems: 40 })),
							unsupportedClaims: Type.Optional(Type.Array(Type.Unknown(), { maxItems: 20 })),
							missingEvidence: Type.Optional(Type.Array(Type.Unknown(), { maxItems: 20 })),
							conclusion: Type.Optional(Type.String({ maxLength: 4_000 })),
						},
						{ additionalProperties: false },
					)
				: workerOutputType === "research-review"
					? Type.Object(
							{
								verdict: Type.Optional(Type.String({ maxLength: 40 })),
								scientificOutcome: Type.Optional(Type.String({ maxLength: 40 })),
								missionCoverage: Type.Optional(Type.String({ maxLength: 40 })),
								strengths: Type.Optional(Type.Array(Type.String())),
								weaknesses: Type.Optional(Type.Array(Type.String())),
								claimAudit: Type.Optional(Type.Array(Type.String())),
								requiredRepairs: Type.Optional(Type.Array(Type.String())),
							},
							{ additionalProperties: false },
						)
					: Type.Record(Type.String(), Type.Unknown(), {
							description: "Structured object; Astra validates the merged full candidate",
						});

		pi.registerTool({
			name: "astra_submit_worker_output",
			label: "Submit worker output",
			description:
				"Persist one validated worker output manifest. Use refs=[] when there is no real file or retrieved source; Astra adds the Pi session ref automatically.",
			executionMode: "sequential",
			parameters: Type.Object({
				artifactType: Type.String({ description: "Exact requiredOutputType from ASTRA_TASK_CONTEXT.json" }),
				content: workerContentParameters,
				incrementalRevision: Type.Optional(
					Type.Object(
						{
							baseEvidenceId: Type.String(),
							baseHash: Type.String({ pattern: "^[a-f0-9]{64}$" }),
							operations: Type.Array(
								Type.Object(
									{
										op: Type.Union([Type.Literal("set"), Type.Literal("delete")]),
										path: Type.Array(Type.String()),
										value: Type.Optional(Type.Unknown()),
										issueId: Type.Optional(Type.String()),
										sourceRefs: Type.Array(Type.String()),
										reason: Type.String({ minLength: 1 }),
									},
									{ additionalProperties: false },
								),
								{ minItems: 1 },
							),
							affectedCriteria: Type.Array(Type.String(), { minItems: 1 }),
							rationale: Type.String({ minLength: 1 }),
						},
						{ additionalProperties: false },
					),
				),
				refs: Type.Array(
					Type.Object({
						kind: Type.Union([
							Type.Literal("artifact"),
							Type.Literal("source"),
							Type.Literal("log"),
							Type.Literal("session"),
						]),
						ref: Type.String({
							description:
								"Existing relative file path for artifact/log, or a real https/openalex/doi/arxiv identifier for source",
						}),
						summary: Type.String({ description: "What this existing file or retrieved source proves" }),
						sha256: Type.Optional(Type.String()),
					}),
					{
						description:
							"Traceable outputs only. Never invent artifact/log paths or source identifiers. Pass [] for structured/session-only work.",
					},
				),
				validationErrors: Type.Optional(Type.Array(Type.String())),
			}),
			async execute(_id, params, _signal, _update, ctx): Promise<AgentToolResult<unknown>> {
				const current = getState(ctx);
				if (current.role !== "worker")
					return result("Only worker sessions may submit worker output") as AgentToolResult<unknown>;
				const packet = await loadTaskPacket(ctx);
				if (!packet) return result("No Astra TaskPacket is bound to this session") as AgentToolResult<unknown>;
				if ((params.validationErrors?.length ?? 0) > 0) {
					return result(`Worker submission rejected: ${params.validationErrors?.join("; ")}`, {
						validationErrors: params.validationErrors,
					}) as AgentToolResult<unknown>;
				}
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job") as AgentToolResult<unknown>;
				let validated: Awaited<ReturnType<typeof validateWorkerSubmission>>;
				try {
					validated = await validateWorkerSubmission(
						packet,
						{
							artifactType: params.artifactType,
							content: params.content,
							refs: params.refs,
							incrementalRevision: params.incrementalRevision,
						},
						{
							executionRoot: ctx.cwd,
							sessionRef: `pi-session:${process.env.ASTRA_SESSION_ID ?? `${packet.jobId}:${packet.id}:${packet.attempt}`}`,
							minSourceRefs: taskStageContract(job.definitions[packet.stageId], packet).minSourceRefs ?? 0,
							job,
						},
					);
				} catch (error) {
					return result(`Worker submission rejected: ${error instanceof Error ? error.message : String(error)}`, {
						error: error instanceof Error ? error.message : String(error),
					}) as AgentToolResult<unknown>;
				}
				const root = projectRoot(ctx);
				const executionPrefix = relative(root, resolve(ctx.cwd)).split("\\").join("/");
				const outputRefs = validated.outputRefs.map((ref) =>
					ref.kind === "artifact" || ref.kind === "log"
						? { ...ref, ref: [executionPrefix, ref.ref].filter(Boolean).join("/") }
						: ref,
				);
				const manifest = {
					schemaVersion: "astra.worker_output_manifest.v1" as const,
					manifestId: `manifest_${packet.id}_${packet.attempt}`,
					jobId: packet.jobId,
					taskId: packet.id,
					agentId: packet.agentId,
					status: "completed" as const,
					artifactType: params.artifactType,
					content: validated.content,
					...(validated.incrementalRevision ? { incrementalRevision: validated.incrementalRevision } : {}),
					outputRefs,
					validationStatus: "passed" as const,
					validationErrors: [],
					sessionRef: `pi-session:${process.env.ASTRA_SESSION_ID ?? `${packet.jobId}:${packet.id}:${packet.attempt}`}`,
					createdAt: new Date().toISOString(),
				};
				const path = await writeWorkerOutputManifest(manifest, packet.scope.workspaceRoot);
				terminalSubmissionCompleted = true;
				return terminalResult(`Worker output manifest written: ${path}`, manifest);
			},
		});

		pi.registerTool({
			name: "astra_search_papers",
			label: "Search papers",
			description:
				"Search cached literature, OpenAlex, Crossref and arXiv with stable source references and channel diagnostics.",
			parameters: Type.Object({
				query: Type.String({ minLength: 3, maxLength: 500 }),
				limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 10 })),
			}),
			async execute(_id, params, _signal, _update, ctx): Promise<AgentToolResult<unknown>> {
				const current = getState(ctx);
				if (current.role !== "worker") {
					return result("Only scoped worker sessions may search papers") as AgentToolResult<unknown>;
				}
				const packet = await loadTaskPacket(ctx);
				if (!packet?.allowedTools.includes("astra_search_papers")) {
					return result("The current TaskPacket does not allow paper search") as AgentToolResult<unknown>;
				}
				if (literatureSearchCallCount >= 4) {
					return result(
						"Literature search budget exhausted for this task. Use the collected source refs and submit the worker output.",
						{ searchCalls: literatureSearchCallCount, exhausted: true },
					) as AgentToolResult<unknown>;
				}
				literatureSearchCallCount += 1;
				try {
					const search = await searchLiterature({
						query: params.query,
						limit: params.limit ?? 5,
						workspaceRoot: projectRoot(ctx),
						jobId: packet.jobId,
						fetcher: options.literatureFetcher,
					});
					const compact = compactLiteratureSearch(search);
					return result(JSON.stringify(compact, null, 2), compact) as AgentToolResult<unknown>;
				} catch (error) {
					return result(`Paper search failed: ${error instanceof Error ? error.message : String(error)}`, {
						error: error instanceof Error ? error.message : String(error),
					}) as AgentToolResult<unknown>;
				}
			},
		});

		pi.registerTool({
			name: "astra_submit_review",
			label: "Submit review",
			description: "Persist a reviewer verdict for the assigned evidence snapshot.",
			executionMode: "sequential",
			parameters: Type.Object({
				evidenceId: Type.String(),
				...reviewAssessmentFields,
			}),
			async execute(_id, params, _signal, _update, ctx): Promise<AgentToolResult<unknown>> {
				const current = getState(ctx);
				if (current.role !== "reviewer")
					return result("Only reviewer sessions may submit reviews") as AgentToolResult<unknown>;
				const packet = await loadTaskPacket(ctx);
				if (!packet)
					return result("No Astra reviewer TaskPacket is bound to this session") as AgentToolResult<unknown>;
				const expectedEvidenceId = packet.inputArtifactRefs[0];
				if (!expectedEvidenceId || params.evidenceId !== expectedEvidenceId) {
					return result(`Reviewer submission rejected: expected evidenceId ${expectedEvidenceId ?? "missing"}`, {
						expectedEvidenceId,
						receivedEvidenceId: params.evidenceId,
					}) as AgentToolResult<unknown>;
				}
				let expectedCriteria: string[];
				try {
					const parsed = JSON.parse(process.env.ASTRA_REVIEW_CRITERIA ?? "null") as unknown;
					if (
						!Array.isArray(parsed) ||
						parsed.length === 0 ||
						parsed.some((criterion) => typeof criterion !== "string")
					) {
						throw new Error("expected a non-empty string array");
					}
					expectedCriteria = parsed;
				} catch (error) {
					return result(
						`Reviewer submission rejected: frozen review criteria are unavailable (${error instanceof Error ? error.message : String(error)})`,
					) as AgentToolResult<unknown>;
				}
				const missingCriteria = expectedCriteria.filter(
					(expected) => !params.criteria.some((criterion) => criterion.criterion === expected),
				);
				const failedCriteria =
					params.verdict === "pass"
						? expectedCriteria.filter(
								(expected) =>
									!params.criteria.some((criterion) => criterion.criterion === expected && criterion.passed),
							)
						: [];
				if (missingCriteria.length > 0 || failedCriteria.length > 0) {
					return result(
						`Reviewer submission rejected: use every frozen criterion exactly as written${params.verdict === "pass" ? " and mark each one passed" : ""}: ${expectedCriteria.join("; ")}`,
						{ expectedCriteria, missingCriteria, failedCriteria },
					) as AgentToolResult<unknown>;
				}
				const manifest = {
					schemaVersion: "astra.reviewer_output_manifest.v1" as const,
					manifestId: `review_${packet.id}_${packet.attempt}`,
					jobId: packet.jobId,
					taskId: packet.id,
					evidenceId: expectedEvidenceId,
					verdict: params.verdict as ReviewVerdict,
					findings: params.findings,
					score: params.score,
					criteria: params.criteria,
					verifiedRefs: params.verifiedRefs,
					sessionRef: `pi-session:${process.env.ASTRA_SESSION_ID ?? `${packet.jobId}:${packet.id}:${packet.attempt}`}`,
					createdAt: new Date().toISOString(),
				};
				const path = await writeReviewerOutputManifest(manifest, packet.scope.workspaceRoot);
				terminalSubmissionCompleted = true;
				return terminalResult(`Reviewer manifest written: ${path}`, manifest);
			},
		});

		pi.registerTool({
			name: "astra_read_research_object",
			label: "Read research object",
			description:
				"Read a research object by job-owned ID, or its declared frozen UTF-8 file using fileRef. Results are paginated; arbitrary filesystem paths are not accepted.",
			parameters: Type.Object({
				id: Type.String({ minLength: 1 }),
				fileRef: Type.Optional(Type.String({ minLength: 1 })),
				offset: Type.Optional(Type.Integer({ minimum: 0 })),
				limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 16000 })),
			}),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				if (current.role !== "main-agent") return result("Only the main-agent session may read research objects");
				const jobId = current.jobIdOverride ?? (await readActiveJobId(projectRoot(ctx)));
				if (!jobId) return result("No active Astra research job");
				const job = await ResearchJob.open(current.store, jobId);
				if (!job) return result("No active Astra research job");
				const collections = [
					job.state.canonical,
					job.state.evidence,
					job.state.tasks,
					job.state.reviews,
					job.state.obligations,
					job.state.stagePlans,
					job.state.graph.nodes,
					job.state.searchBatches,
					job.state.candidateEvaluations,
					job.state.routeDecisions,
				];
				const object = collections
					.flatMap((collection) => Object.values(collection))
					.find((value) => value.id === params.id);
				if (!object) return result(`Unknown research object: ${params.id}`);
				let serialized = JSON.stringify(object);
				let sha256: string | undefined;
				if (params.fileRef !== undefined) {
					const evidence = job.state.evidence[job.state.canonical[params.id]?.evidenceId ?? params.id];
					const file = evidence?.files?.find((entry) => entry.sourceRef === params.fileRef);
					if (!evidence || !file || !evidence.refs.includes(file.sourceRef))
						throw new Error("File must be a declared frozen evidence ref");
					const task = job.state.tasks[evidence.taskId];
					if (!task) throw new Error("Frozen evidence task is missing");
					const bytes = await readVersionedFile(task, evidence, file.sourceRef, "", "");
					if (!bytes) throw new Error("Frozen evidence file is missing");
					serialized = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
					if (serialized.includes("\u0000"))
						throw new Error("Read a text extraction or page preview instead of a binary file");
					sha256 = file.sha256;
				}
				const offset = params.offset ?? 0;
				const limit = params.limit ?? 16000;
				const end = Math.min(serialized.length, offset + limit);
				const page = {
					id: params.id,
					...(params.fileRef !== undefined ? { fileRef: params.fileRef, sha256 } : {}),
					offset,
					totalCharacters: serialized.length,
					text: serialized.slice(offset, end),
					...(end < serialized.length ? { nextOffset: end } : {}),
				};
				return result(JSON.stringify(page), page);
			},
		});

		pi.registerTool({
			name: "astra_submit_stage_plan",
			label: "Submit stage plan",
			description: "Persist a main-agent-authored plan containing scoped worker tasks.",
			executionMode: "sequential",
			parameters: Type.Object({
				mode: Type.Union([Type.Literal("decompose"), Type.Literal("search"), Type.Literal("repair")]),
				tasks: Type.Array(
					Type.Object({
						key: Type.String({ minLength: 1, maxLength: 80, pattern: "^[A-Za-z0-9][A-Za-z0-9._-]*$" }),
						objective: Type.String({ minLength: 10 }),
						deliveryKind: Type.Optional(
							Type.Union([Type.Literal("stage"), Type.Literal("local"), Type.Literal("synthesis")]),
						),
						hypothesis: Type.Optional(Type.String({ minLength: 5 })),
						inputArtifactRefs: Type.Array(Type.String()),
						requiredOutputFields: Type.Array(Type.String(), { minItems: 1 }),
						acceptanceChecks: Type.Array(Type.String(), { minItems: 1 }),
						failureSignals: Type.Array(Type.String(), { minItems: 1 }),
						successCriteria: Type.Array(Type.String(), { minItems: 1 }),
						responsibilityTransfers: Type.Array(
							Type.Object(
								{
									sourceTaskId: Type.String({ minLength: 1 }),
									sourceContractHash: Type.String({ pattern: "^[a-f0-9]{64}$" }),
									sourceField: Type.Literal("acceptanceChecks"),
									sourceIndex: Type.Integer({ minimum: 0 }),
									exactCriterion: Type.String({ minLength: 1 }),
									nodeId: Type.Union([Type.String({ minLength: 1 }), Type.Null()]),
									issueId: Type.Union([Type.String({ minLength: 1 }), Type.Null()]),
									destinationStageId: Type.String({ minLength: 1 }),
									destinationPhase: Type.Literal("synthesis"),
									rationale: Type.String({ minLength: 1 }),
								},
								{ additionalProperties: false },
							),
						),
						responsibilityBindings: Type.Array(
							Type.Object({
								nodeId: Type.String({ minLength: 1 }),
								stageId: Type.String({ minLength: 1 }),
								phase: Type.Union([Type.Literal("stage"), Type.Literal("synthesis")]),
							}),
						),
					}),
					{ minItems: 1, maxItems: process.env.ASTRA_PLAN_MODE === "search" ? 4 : 2 },
				),
				rationale: Type.String({ minLength: 10 }),
			}),
			async execute(_id, params, _signal, _update, ctx): Promise<AgentToolResult<unknown>> {
				const current = getState(ctx);
				if (current.role !== "main-agent") {
					return result("Only the main-agent session may submit stage plans") as AgentToolResult<unknown>;
				}
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job") as AgentToolResult<unknown>;
				const stageId = process.env.ASTRA_STAGE_ID;
				const planId = process.env.ASTRA_STAGE_PLAN_ID;
				const decisionRef = process.env.ASTRA_DECISION_REF;
				if (!stageId || !planId || !decisionRef || stageId !== job.state.frame.activeStageId) {
					return result("Stage planning environment does not match the active job") as AgentToolResult<unknown>;
				}
				if (process.env.ASTRA_OBLIGATION_ID && params.tasks.length !== 1) {
					return result(
						"Stage repair plan rejected: one obligation requires exactly one complete repair task",
					) as AgentToolResult<unknown>;
				}
				const expectedMode =
					process.env.ASTRA_PLAN_MODE ?? (process.env.ASTRA_OBLIGATION_ID ? "repair" : "decompose");
				if (params.mode !== expectedMode) {
					return result(`Stage plan rejected: expected mode ${expectedMode}`) as AgentToolResult<unknown>;
				}
				const definition = job.definitions[stageId];
				if (params.mode === "search") {
					const minimum = definition.searchPolicy?.minCandidates ?? 2;
					if (params.tasks.length < minimum || params.tasks.some((task) => !task.hypothesis)) {
						return result(
							`Search plan rejected: at least ${minimum} candidates with distinct hypotheses are required`,
						) as AgentToolResult<unknown>;
					}
					const previousBatchId = process.env.ASTRA_SEARCH_PREVIOUS_BATCH_ID;
					if (previousBatchId) {
						const previousBatch = job.state.searchBatches[previousBatchId];
						if (!previousBatch || previousBatch.stageId !== stageId || previousBatch.status !== "exhausted") {
							return result(
								"Search plan rejected: previous search batch does not match this round",
							) as AgentToolResult<unknown>;
						}
						const previousHypotheses = new Set(
							Object.values(previousBatch.candidates).map((candidate) =>
								candidate.hypothesis.trim().toLocaleLowerCase().replace(/\s+/g, " "),
							),
						);
						const repeated = params.tasks.find((task) =>
							previousHypotheses.has(
								(task.hypothesis ?? task.objective).trim().toLocaleLowerCase().replace(/\s+/g, " "),
							),
						);
						if (repeated) {
							return result(
								`Search plan rejected: task ${repeated.key} repeats a hypothesis from ${previousBatchId}`,
							) as AgentToolResult<unknown>;
						}
					}
				}
				const available = new Set<string>(JSON.parse(process.env.ASTRA_AVAILABLE_INPUT_REFS ?? "[]"));
				for (const task of params.tasks) {
					const unknown = task.inputArtifactRefs.filter((ref) => !available.has(ref));
					if (unknown.length > 0) {
						return result(
							`Stage plan rejected: unknown input refs ${unknown.join(", ")}`,
						) as AgentToolResult<unknown>;
					}
					const missing = definition.requiredOutputFields.filter(
						(field) => !task.requiredOutputFields.includes(field),
					);
					if (task.deliveryKind !== "local" && missing.length > 0) {
						return result(
							`Stage plan rejected: task ${task.key} omits fields ${missing.join(", ")}`,
						) as AgentToolResult<unknown>;
					}
				}
				const manifest: StagePlanManifest = {
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: planId,
					jobId: job.state.frame.jobId,
					stageId,
					decisionRef,
					mode: params.mode,
					tasks: params.tasks as PlannedTask[],
					rationale: params.rationale,
					sessionRef: `pi-session:${process.env.ASTRA_SESSION_ID ?? `${job.state.frame.jobId}:main`}`,
					...(process.env.ASTRA_OBLIGATION_ID ? { obligationId: process.env.ASTRA_OBLIGATION_ID } : {}),
					createdAt: new Date().toISOString(),
				};
				const path = await writeStagePlanManifest(manifest, projectRoot(ctx));
				terminalSubmissionCompleted = true;
				return terminalResult(`Stage plan manifest written: ${path}`, manifest);
			},
		});

		const mainDecisionParameters =
			process.env.ASTRA_DECISION_TYPE === "evidence"
				? Type.Object({
						decisionType: Type.Literal("evidence"),
						decisionRef: Type.String(),
						evidenceId: Type.String(),
						decision: Type.Union([Type.Literal("accept"), Type.Literal("reject"), Type.Literal("defer")]),
						rationale: Type.String(),
					})
				: process.env.ASTRA_DECISION_TYPE === "adoption"
					? Type.Object({
							decisionType: Type.Literal("adoption"),
							decisionRef: Type.String(),
							evidenceId: Type.String(),
							adopt: Type.Boolean(),
							replacementOf: Type.Optional(Type.String()),
							rationale: Type.String(),
						})
					: process.env.ASTRA_DECISION_TYPE === "search-selection"
						? Type.Object({
								decisionType: Type.Literal("search-selection"),
								decisionRef: Type.String(),
								searchBatchId: Type.String(),
								selectedCandidateId: Type.Optional(Type.String()),
								continueSearch: Type.Optional(Type.Boolean()),
								rationale: Type.String(),
							})
						: Type.Object({
								decisionType: Type.Literal("route"),
								decisionRef: Type.String(),
								stageId: Type.String(),
								routeAction: Type.Union([
									Type.Literal("continue"),
									Type.Literal("search"),
									Type.Literal("advance"),
									Type.Literal("backtrack"),
									Type.Literal("ask-user"),
									Type.Literal("complete"),
								]),
								targetStageId: Type.Optional(Type.String()),
								evidenceRefs: Type.Array(Type.String()),
								question: Type.Optional(Type.String()),
								newQuestions: Type.Optional(Type.Array(Type.String())),
								rationale: Type.String(),
							});

		pi.registerTool({
			name: "astra_submit_main_decision",
			label: "Submit main-agent decision",
			description: "Persist an explicit main-agent evidence, adoption, search-selection, or route decision.",
			executionMode: "sequential",
			parameters: mainDecisionParameters,
			async execute(_id, params, _signal, _update, ctx): Promise<AgentToolResult<unknown>> {
				const current = getState(ctx);
				if (current.role !== "main-agent")
					return result("Only the main-agent session may submit main decisions") as AgentToolResult<unknown>;
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job") as AgentToolResult<unknown>;
				const expectedDecisionType = process.env.ASTRA_DECISION_TYPE;
				const expectedDecisionRef = process.env.ASTRA_DECISION_REF;
				if (params.decisionType !== expectedDecisionType || params.decisionRef !== expectedDecisionRef) {
					return result("Main-agent decision rejected: decision type or reference does not match this session", {
						expectedDecisionType,
						expectedDecisionRef,
					}) as AgentToolResult<unknown>;
				}
				if (
					(params.decisionType === "evidence" || params.decisionType === "adoption") &&
					params.evidenceId !== process.env.ASTRA_EVIDENCE_ID
				) {
					return result(
						`Main-agent decision rejected: expected evidenceId ${process.env.ASTRA_EVIDENCE_ID ?? "missing"}`,
						{
							expectedEvidenceId: process.env.ASTRA_EVIDENCE_ID,
						},
					) as AgentToolResult<unknown>;
				}
				if (
					params.decisionType === "evidence" &&
					(typeof params.decision !== "string" || !["accept", "reject", "defer"].includes(params.decision))
				) {
					return result("Main-agent evidence decision rejected: decision is required") as AgentToolResult<unknown>;
				}
				if (params.decisionType === "adoption" && typeof params.adopt !== "boolean") {
					return result("Main-agent adoption decision rejected: adopt is required") as AgentToolResult<unknown>;
				}
				if (params.decisionType === "adoption" && params.replacementOf) {
					const replacement = job.state.canonical[params.replacementOf];
					if (!replacement) {
						return result(
							`Main-agent adoption decision rejected: unknown replacement artifact ${params.replacementOf}`,
						) as AgentToolResult<unknown>;
					}
					if (replacement.status !== "active") {
						return result(
							`Main-agent adoption decision rejected: replacement artifact ${params.replacementOf} is not active`,
						) as AgentToolResult<unknown>;
					}
				}
				if (
					params.decisionType === "search-selection" &&
					params.searchBatchId !== process.env.ASTRA_SEARCH_BATCH_ID
				) {
					return result(
						`Main-agent search decision rejected: expected batch ${process.env.ASTRA_SEARCH_BATCH_ID ?? "missing"}`,
					) as AgentToolResult<unknown>;
				}
				if (
					params.decisionType === "search-selection" &&
					Boolean(params.selectedCandidateId) === (params.continueSearch === true)
				) {
					return result(
						"Main-agent search decision rejected: choose exactly one of candidate selection or continued search",
					) as AgentToolResult<unknown>;
				}
				if (
					params.decisionType === "search-selection" &&
					params.continueSearch === true &&
					process.env.ASTRA_SEARCH_ALLOW_CONTINUE !== "1"
				) {
					return result(
						"Main-agent search decision rejected: final search round requires a candidate selection",
					) as AgentToolResult<unknown>;
				}
				if (params.decisionType === "search-selection" && params.selectedCandidateId) {
					let candidateIds: unknown;
					try {
						candidateIds = JSON.parse(process.env.ASTRA_CANDIDATE_IDS ?? "null");
					} catch {
						candidateIds = undefined;
					}
					if (!Array.isArray(candidateIds) || !candidateIds.includes(params.selectedCandidateId)) {
						return result(
							`Main-agent search decision rejected: unknown candidate ${params.selectedCandidateId}`,
						) as AgentToolResult<unknown>;
					}
				}
				if (params.decisionType === "route" && params.stageId !== job.state.frame.activeStageId) {
					return result(`Main-agent decision rejected: expected stageId ${job.state.frame.activeStageId}`, {
						expectedStageId: job.state.frame.activeStageId,
					}) as AgentToolResult<unknown>;
				}
				if (params.decisionType === "route") {
					const unknownEvidenceRefs = params.evidenceRefs.filter(
						(ref) => !job.state.canonical[ref] && !job.state.evidence[ref] && !job.state.graph.nodes[ref],
					);
					if (unknownEvidenceRefs.length > 0) {
						let allowedEvidenceRefs: unknown;
						try {
							allowedEvidenceRefs = JSON.parse(process.env.ASTRA_ROUTE_EVIDENCE_REFS ?? "[]");
						} catch {
							allowedEvidenceRefs = [];
						}
						return result(
							`Main-agent route decision rejected: unknown evidence refs ${unknownEvidenceRefs.join(", ")}`,
							{ unknownEvidenceRefs, allowedEvidenceRefs },
						) as AgentToolResult<unknown>;
					}
				}
				if (
					params.decisionType === "route" &&
					["advance", "backtrack"].includes(params.routeAction) &&
					(!params.targetStageId || !job.definitions[params.targetStageId])
				) {
					return result(
						"Main-agent route decision rejected: a valid targetStageId is required",
					) as AgentToolResult<unknown>;
				}
				if (params.decisionType === "route" && params.routeAction === "ask-user" && !params.question?.trim()) {
					return result(
						"Main-agent route decision rejected: ask-user requires a question",
					) as AgentToolResult<unknown>;
				}
				const manifest: MainAgentDecisionManifest = {
					schemaVersion: "astra.main_agent_decision_manifest.v1" as const,
					manifestId: `decision_${params.decisionRef}`,
					jobId: job.state.frame.jobId,
					decisionType: params.decisionType,
					decisionRef: params.decisionRef,
					...(params.decisionType === "evidence"
						? { evidenceId: params.evidenceId, decision: params.decision }
						: params.decisionType === "adoption"
							? { evidenceId: params.evidenceId, adopt: params.adopt, replacementOf: params.replacementOf }
							: params.decisionType === "search-selection"
								? {
										searchBatchId: params.searchBatchId,
										selectedCandidateId: params.selectedCandidateId,
										continueSearch: params.continueSearch,
									}
								: {
										stageId: params.stageId,
										routeAction: params.routeAction,
										targetStageId: params.targetStageId,
										evidenceRefs: params.evidenceRefs,
										question: params.question,
										newQuestions: params.newQuestions,
									}),
					rationale: params.rationale,
					sessionRef: `pi-session:${process.env.ASTRA_SESSION_ID ?? `${job.state.frame.jobId}:main`}`,
					createdAt: new Date().toISOString(),
				};
				const path = await writeMainDecisionManifest(manifest, projectRoot(ctx));
				terminalSubmissionCompleted = true;
				return terminalResult(`Main-agent decision manifest written: ${path}`, manifest);
			},
		});

		pi.registerTool({
			name: "research_start",
			label: "Start research",
			description: "Create a durable Astra research job and bind it to this project.",
			parameters: Type.Object({
				objective: Type.String(),
				automation: Type.Optional(
					Type.Union([Type.Literal("collaborative"), Type.Literal("autonomous"), Type.Literal("full")]),
				),
				boundaries: Type.Optional(Type.Array(Type.String())),
				maxTasks: Type.Optional(Type.Integer({ minimum: 1 })),
				maxTurns: Type.Optional(Type.Integer({ minimum: 1 })),
				maxCostUsd: Type.Optional(Type.Number({ minimum: 0 })),
				requirePaper: Type.Optional(Type.Boolean()),
			}),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await ResearchJob.create(current.store, {
					objective: params.objective,
					workspaceRoot: projectRoot(ctx),
					automation: params.automation ?? options.defaultAutomation,
					boundaries: params.boundaries,
					maxTasks: params.maxTasks,
					maxTurns: params.maxTurns,
					maxCostUsd: params.maxCostUsd,
					requiredArtifactTypes: params.requirePaper ? ["paper-write", "paper-compile"] : undefined,
				});
				current.job = job;
				await setActiveJobId(projectRoot(ctx), job.state.frame.jobId);
				return result(
					`Started Astra research job ${job.state.frame.jobId} at stage ${job.state.frame.activeStageId}`,
					job.status(),
				);
			},
		});

		pi.registerTool({
			name: "research_status",
			label: "Research status",
			description: "Read the current durable Astra research status.",
			parameters: Type.Object({}),
			async execute(_id, _params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await loadJob(current, projectRoot(ctx));
				return job
					? result(JSON.stringify(job.status(), null, 2), job.status())
					: result("No active Astra research job");
			},
		});

		pi.registerTool({
			name: "research_dispatch",
			label: "Dispatch research task",
			description: "Dispatch a structured worker or reviewer TaskPacket for the active stage.",
			parameters: Type.Object({
				objective: Type.String(),
				role: Type.Union([Type.Literal("worker"), Type.Literal("reviewer")]),
				requiredOutputType: Type.String(),
				inputArtifactRefs: Type.Optional(Type.Array(Type.String())),
			}),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job");
				const stageId = job.state.frame.activeStageId;
				const task = await job.dispatchTask({
					stageId,
					stageExecutionId: job.state.stages[stageId].executionId ?? `stage_exec_${stageId}`,
					agentId: `${params.role}_${Date.now()}`,
					role: params.role,
					objective: params.objective,
					inputArtifactRefs: params.inputArtifactRefs ?? [],
					requiredCanonicalArtifacts: [],
					requiredOutputType: params.requiredOutputType,
					requiredOutputFields: ["content", "outputRefs"],
					acceptanceChecks: job.definitions[stageId]?.acceptanceChecks ?? [],
					failureSignals: ["missing output manifest", "empty evidence refs"],
					dependencies: [],
					scope: { workspaceRoot: projectRoot(ctx), allowedPaths: ["."] },
					allowedTools: job.state.frame.permissions.allowedTools,
					writeAuthority: params.role === "worker" ? "workspace-write" : "none",
					budget: { maxTurns: 8, maxToolCalls: 16, maxRuntimeMs: 300_000 },
					reviewGateRequired: true,
					resumePolicy: "resume-session",
					successCriteria: job.definitions[stageId]?.acceptanceChecks ?? [],
				});
				return result(JSON.stringify(task, null, 2), task);
			},
		});

		pi.registerTool({
			name: "research_record_evidence",
			label: "Record evidence",
			description: "Record a worker output as a stage-local evidence candidate.",
			parameters: Type.Object({
				taskId: Type.String(),
				type: Type.String(),
				content: Type.Unknown(),
				refs: Type.Array(Type.String()),
			}),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job");
				const evidence = await job.recordEvidence({
					taskId: params.taskId,
					stageId: job.state.frame.activeStageId,
					type: params.type,
					content: params.content,
					refs: params.refs,
				});
				return result(`Recorded evidence candidate ${evidence.id}`, evidence);
			},
		});

		pi.registerTool({
			name: "research_decide_evidence",
			label: "Decide evidence",
			description: "Record the main-agent decision that promotes or rejects a worker evidence candidate.",
			parameters: Type.Object({
				evidenceId: Type.String(),
				accepted: Type.Boolean(),
				decisionRef: Type.Optional(Type.String()),
			}),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job");
				await job.decideEvidence(params.evidenceId, params.accepted, params.decisionRef);
				return result(
					`${params.accepted ? "Accepted" : "Rejected"} evidence ${params.evidenceId}`,
					job.state.evidence[params.evidenceId],
				);
			},
		});

		pi.registerTool({
			name: "research_review",
			label: "Review evidence",
			description: "Record an independent structured review; failed reviews become blocking obligations.",
			parameters: Type.Object({
				evidenceId: Type.String(),
				...reviewAssessmentFields,
				targetVersionHash: Type.Optional(Type.String()),
			}),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job");
				const review = await job.recordReview({
					evidenceId: params.evidenceId,
					verdict: params.verdict as ReviewVerdict,
					findings: params.findings,
					score: params.score,
					criteria: params.criteria,
					verifiedRefs: params.verifiedRefs,
					targetVersionHash: params.targetVersionHash,
				});
				return result(`Recorded ${review.verdict} review ${review.id}`, review);
			},
		});

		pi.registerTool({
			name: "research_adopt",
			label: "Adopt evidence",
			description:
				"Explicitly adopt passing reviewed evidence as canonical, optionally replacing an existing artifact.",
			parameters: Type.Object({ evidenceId: Type.String(), replacementOf: Type.Optional(Type.String()) }),
			async execute(_id, params, _signal, _update, ctx) {
				const current = getState(ctx);
				const job = await loadJob(current, projectRoot(ctx));
				if (!job) return result("No active Astra research job");
				const artifact = await job.adoptEvidence(params.evidenceId, params.replacementOf);
				return result(`Adopted canonical artifact ${artifact.id}`, artifact);
			},
		});

		const researchCommand = async (args: string, ctx: ExtensionContext) => {
			const tokens = args.trim() ? args.trim().split(/\s+/) : [];
			await executeResearchControl(parseResearchControlArgs(tokens), ctx);
		};
		pi.registerCommand("research-status", {
			description: "Show durable Astra research status",
			handler: async (_args, ctx) => executeResearchControl({ action: "status" }, ctx),
		});
		pi.registerCommand("research-board", {
			description: "Show the shared Astra research board",
			handler: async (_args, ctx) => {
				const job = await loadJob(getState(ctx), projectRoot(ctx));
				ctx.ui.notify(job ? formatResearchBoard(buildResearchBoard(job.state)) : "No active Astra research job");
			},
		});
		pi.registerCommand("research-guide", {
			description: "Add user guidance to the canonical research graph",
			handler: async (guidance, ctx) => {
				const job = await loadJob(getState(ctx), projectRoot(ctx));
				if (!job) {
					ctx.ui.notify("No active Astra research job");
					return;
				}
				await job.recordUserGuidance(guidance);
				ctx.ui.notify(formatResearchBoard(buildResearchBoard(job.state)));
			},
		});
		pi.registerCommand("research-route", {
			description: "Show canonical route decisions and candidate comparisons",
			handler: async (_args, ctx) => {
				const job = await loadJob(getState(ctx), projectRoot(ctx));
				ctx.ui.notify(
					job
						? JSON.stringify(
								{
									canonicalRoute: job.state.canonicalRoute,
									routeDecisions: job.state.routeDecisions,
									searchBatches: job.state.searchBatches,
									candidateEvaluations: job.state.candidateEvaluations,
									objections: job.state.graph.unresolvedObjectionIds.map((id) => job.state.graph.nodes[id]),
								},
								null,
								2,
							)
						: "No active Astra research job",
				);
			},
		});
		pi.registerCommand("research", {
			description: "Control Astra research: run, status, tick, pause, resume, migrate",
			handler: researchCommand,
		});
		pi.registerCommand("research-tasks", {
			description: "Show current Astra task pool",
			handler: async (_args, ctx) => {
				const job = await loadJob(getState(ctx), projectRoot(ctx));
				ctx.ui.notify(
					job ? JSON.stringify(Object.values(job.state.tasks), null, 2) : "No active Astra research job",
				);
			},
		});
		pi.registerCommand("research-review", {
			description: "Show current Astra reviews and obligations",
			handler: async (_args, ctx) => {
				const job = await loadJob(getState(ctx), projectRoot(ctx));
				ctx.ui.notify(
					job
						? JSON.stringify({ reviews: job.state.reviews, obligations: job.state.obligations }, null, 2)
						: "No active Astra research job",
				);
			},
		});
		pi.registerCommand("research-continue", {
			description: "Resume Astra research",
			handler: async (_args, ctx) => executeResearchControl({ action: "resume" }, ctx),
		});
		pi.registerCommand("research-pause", {
			description: "Pause Astra research",
			handler: async (reason, ctx) =>
				executeResearchControl({ action: "pause", ...(reason.trim() ? { reason: reason.trim() } : {}) }, ctx),
		});
		pi.registerCommand("research-resume", {
			description: "Resume Astra research",
			handler: async (_args, ctx) => executeResearchControl({ action: "resume" }, ctx),
		});
	};
}

export default createAstraExtension;
