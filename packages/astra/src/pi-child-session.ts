import { type ChildProcess, type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { access, readdir, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
	mainDecisionManifestPath,
	readJson,
	readReviewerOutputManifest,
	readStagePlanManifest,
	readWorkerOutputManifest,
	stagePlanManifestPath,
	taskDir,
	writeReviewPacket,
	writeReviewSnapshot,
	writeReviewTrace,
	writeTaskPacket,
} from "./contracts.ts";
import { classifyProviderErrorMessage } from "./provider-errors.ts";
import type { ResearchJob } from "./research.ts";
import { checksum } from "./research.ts";
import {
	NonRetryableResearchError,
	ProviderCapacityError,
	type ResearchMainAgentAdapter,
	type ResearchReviewerAdapter,
	type ResearchWorkerAdapter,
	type ReviewerRunResult,
	type WorkerRunResult,
} from "./supervisor.ts";
import {
	prepareReviewEvidenceBundle,
	prepareTaskWorkspace,
	taskInputResources,
	taskResourcePath,
} from "./task-workspace.ts";
import type {
	CandidateEvaluation,
	CanonicalArtifact,
	Evidence,
	JobSnapshot,
	MainAgentDecisionManifest,
	Obligation,
	ResearchNode,
	ReviewerOutputManifest,
	SearchBatch,
	StagePlanManifest,
	TaskPacket,
	WorkerOutputManifest,
} from "./types.ts";

export interface PiChildSessionOptions {
	launcherPath?: string;
	sessionDir?: string;
	model?: string;
	fixtureProvider?: boolean;
	maxOutputBytes?: number;
}

interface ProcessResult {
	exitCode: number;
	stdout: string;
	stderr: string;
	jsonEvents: unknown[];
}

const MANIFEST_WAIT_TIMEOUT_MS = 30_000;

function compactDecisionValue(value: unknown, depth = 0): unknown {
	if (typeof value === "string") return value.length <= 240 ? value : `${value.slice(0, 240)}...`;
	if (value === null || typeof value !== "object") return value;
	if (depth >= 3) {
		return Array.isArray(value)
			? { itemCount: value.length }
			: {
					fieldCount: Object.keys(value as Record<string, unknown>).length,
					fieldNames: Object.keys(value as Record<string, unknown>).slice(0, 12),
				};
	}
	if (Array.isArray(value)) {
		return {
			itemCount: value.length,
			sample: value.slice(0, 2).map((entry) => compactDecisionValue(entry, depth + 1)),
		};
	}
	const entries = Object.entries(value as Record<string, unknown>);
	return {
		...Object.fromEntries(entries.slice(0, 12).map(([key, entry]) => [key, compactDecisionValue(entry, depth + 1)])),
		...(entries.length > 12 ? { omittedFieldCount: entries.length - 12 } : {}),
	};
}

function projectNode(node: ResearchNode): Record<string, unknown> {
	return {
		id: node.id,
		kind: node.kind,
		status: node.status,
		statement: compactDecisionValue(node.statement),
		stageId: node.stageId,
		domainRef: node.domainRef,
		claimAssessment: node.claimAssessment,
		actor: node.actor,
		sourceRefs: node.sourceRefs.slice(0, 8),
		...(node.sourceRefs.length > 8 ? { omittedSourceRefCount: node.sourceRefs.length - 8 } : {}),
	};
}

function projectNodeSet(ids: string[], snapshot: JobSnapshot): Record<string, unknown> {
	const nodes = ids.flatMap((id) => {
		const node = snapshot.graph.nodes[id];
		return node ? [projectNode(node)] : [];
	});
	return {
		total: ids.length,
		items: nodes.slice(-12),
		...(nodes.length > 12 ? { omittedNodeCount: nodes.length - 12 } : {}),
	};
}

export function projectResearchGraph(snapshot: JobSnapshot): Record<string, unknown> {
	const root = snapshot.graph.nodes[snapshot.graph.rootQuestionId];
	const userGuidance = Object.values(snapshot.graph.nodes)
		.filter((node) => node.actor === "user")
		.sort((left, right) => left.updatedAt.localeCompare(right.updatedAt));
	return {
		version: snapshot.graph.version,
		revision: snapshot.graph.revision,
		sourceRef: `job:${snapshot.frame.jobId}:graph:${snapshot.graph.revision}`,
		rootQuestion: root ? projectNode(root) : { id: snapshot.graph.rootQuestionId },
		openQuestions: projectNodeSet(snapshot.graph.openQuestionIds, snapshot),
		activeHypotheses: projectNodeSet(snapshot.graph.activeHypothesisIds, snapshot),
		acceptedClaims: projectNodeSet(snapshot.graph.acceptedClaimIds, snapshot),
		unresolvedObjections: projectNodeSet(snapshot.graph.unresolvedObjectionIds, snapshot),
		userGuidance: {
			total: userGuidance.length,
			items: userGuidance.slice(-12).map(projectNode),
		},
	};
}

function projectCanonicalArtifact(
	artifact: CanonicalArtifact,
	snapshot: JobSnapshot,
	includeContent: boolean,
): Record<string, unknown> {
	const evidence = snapshot.evidence[artifact.evidenceId];
	return {
		id: artifact.id,
		type: artifact.type,
		stageId: evidence?.stageId,
		evidenceId: artifact.evidenceId,
		checksum: artifact.checksum,
		status: artifact.status,
		sourceSha256: artifact.sourceSha256,
		targetSha256: artifact.targetSha256,
		materializationRef: artifact.materializationRef,
		...(includeContent ? { contentProjection: compactDecisionValue(artifact.content) } : {}),
	};
}

const activeChildren = new Set<ChildProcess>();
let childShutdownHandler: (() => void) | undefined;

function trackChild(child: ChildProcess): void {
	activeChildren.add(child);
	if (childShutdownHandler) return;
	childShutdownHandler = () => {
		for (const activeChild of activeChildren) activeChild.kill("SIGTERM");
		setTimeout(() => {
			for (const activeChild of activeChildren) activeChild.kill("SIGKILL");
			process.exit(143);
		}, 2_000).unref();
	};
	process.on("SIGTERM", childShutdownHandler);
}

function untrackChild(child: ChildProcess): void {
	activeChildren.delete(child);
	if (activeChildren.size === 0 && childShutdownHandler) {
		process.off("SIGTERM", childShutdownHandler);
		childShutdownHandler = undefined;
	}
}

function defaultLauncherPath(): string {
	const directory = dirname(fileURLToPath(import.meta.url));
	const built = resolve(directory, "launcher.js");
	return existsSync(built) ? built : resolve(directory, "launcher.ts");
}

function safeSessionId(jobId: string, taskId: string, attempt: number): string {
	return `astra-${jobId}-${taskId}-${attempt}`.replace(/[^A-Za-z0-9._-]/g, "-");
}

function persistentMainSessionId(jobId: string): string {
	return `astra-${jobId}-main`.replace(/[^A-Za-z0-9._-]/g, "-");
}

function resumableMainSessionId(job: ResearchJob): string {
	const sessionId = job.state.mainAgentSessionId || persistentMainSessionId(job.state.frame.jobId);
	const session = job.state.sessions[sessionId];
	if (!session || session.status === "completed") return sessionId;
	const recoveryId = checksum({ sessionId, taskId: session.taskId, eventSeq: job.state.eventSeq }).slice(0, 12);
	return `${persistentMainSessionId(job.state.frame.jobId)}-recovery-${recoveryId}`;
}

async function findSessionFile(sessionDir: string | undefined, sessionId: string): Promise<string | undefined> {
	if (!sessionDir) return undefined;
	try {
		const files = await readdir(sessionDir);
		const match = files.find((file) => file.endsWith(`_${sessionId}.jsonl`));
		return match ? join(sessionDir, match) : undefined;
	} catch {
		return undefined;
	}
}

function parseJsonLines(stdout: string): unknown[] {
	return stdout
		.split("\n")
		.filter((line) => line.trim().startsWith("{"))
		.flatMap((line) => {
			try {
				return [JSON.parse(line)];
			} catch {
				return [];
			}
		});
}

export interface ProviderErrorEvent {
	kind: "configuration" | "capacity";
	message: string;
}

export function providerErrorFromJsonEvents(events: unknown[]): ProviderErrorEvent | undefined {
	for (const event of events) {
		if (event === null || typeof event !== "object") continue;
		const record = event as Record<string, unknown>;
		if (record.type !== "message_end" || record.message === null || typeof record.message !== "object") continue;
		const message = record.message as Record<string, unknown>;
		if (message.role !== "assistant" || message.stopReason !== "error" || typeof message.errorMessage !== "string") {
			continue;
		}
		const errorMessage = message.errorMessage;
		const kind = classifyProviderErrorMessage(errorMessage);
		if (kind) return { kind, message: errorMessage };
	}
	return undefined;
}

function researchErrorFromProvider(error: ProviderErrorEvent): Error {
	return error.kind === "capacity"
		? new ProviderCapacityError(error.message)
		: new NonRetryableResearchError(error.message);
}

export function costUsdFromJsonEvents(events: unknown[]): number {
	let total = 0;
	for (const event of events) {
		if (event === null || typeof event !== "object") continue;
		const record = event as Record<string, unknown>;
		if (record.type !== "message_end" || record.message === null || typeof record.message !== "object") continue;
		const message = record.message as Record<string, unknown>;
		if (message.role !== "assistant" || message.usage === null || typeof message.usage !== "object") continue;
		const usage = message.usage as Record<string, unknown>;
		if (usage.cost === null || typeof usage.cost !== "object") continue;
		const cost = (usage.cost as Record<string, unknown>).total;
		if (typeof cost === "number" && Number.isFinite(cost) && cost >= 0) total += cost;
	}
	return total;
}

export class PiChildSessionRunner {
	private readonly launcherPath: string;
	private readonly maxOutputBytes: number;
	private readonly options: PiChildSessionOptions;

	constructor(options: PiChildSessionOptions = {}) {
		this.options = options;
		this.launcherPath = options.launcherPath ?? process.env.ASTRA_LAUNCHER ?? defaultLauncherPath();
		this.maxOutputBytes = options.maxOutputBytes ?? 2 * 1024 * 1024;
	}

	get sessionDir(): string | undefined {
		return this.options.sessionDir;
	}

	async run(
		cwd: string,
		jobId: string,
		taskId: string,
		attempt: number,
		role: "worker" | "reviewer" | "main-agent",
		prompt: string,
		env: Record<string, string | undefined> = {},
		timeoutMs?: number,
	): Promise<ProcessResult> {
		const projectRoot = env.ASTRA_PROJECT_ROOT ?? cwd;
		const sessionDir = this.options.sessionDir ?? join(projectRoot, ".astra", "jobs", jobId, "sessions");
		const sessionId =
			role === "main-agent"
				? (env.ASTRA_SESSION_ID ?? persistentMainSessionId(jobId))
				: safeSessionId(jobId, taskId, attempt);
		const allowedWorkerTools = (env.ASTRA_ALLOWED_TOOLS ?? "read,grep,find,ls")
			.split(",")
			.map((tool) => tool.trim())
			.filter(Boolean);
		const tools =
			role === "worker"
				? [...new Set([...allowedWorkerTools, "astra_submit_worker_output"])]
				: role === "reviewer"
					? ["read", "grep", "find", "ls", "astra_submit_review"]
					: env.ASTRA_STAGE_PLAN_ID
						? ["astra_submit_stage_plan"]
						: env.ASTRA_DECISION_TYPE
							? ["astra_submit_main_decision"]
							: [
									"read",
									"grep",
									"find",
									"ls",
									"astra_submit_stage_plan",
									"astra_submit_main_decision",
									"research_status",
								];
		const args = [
			"--mode",
			"json",
			"--print",
			"--session-id",
			sessionId,
			"--session-dir",
			sessionDir,
			"--no-extensions",
			"--no-context-files",
			"--no-themes",
			"--no-prompt-templates",
			"--no-skills",
			"--tools",
			tools.join(","),
		];
		const skillsRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "skills", "astra");
		for (const name of new Set([env.ASTRA_STAGE_ID, role])) {
			if (!name) continue;
			const skillPath = join(skillsRoot, `${name}.md`);
			try {
				await access(skillPath);
				args.push("--skill", skillPath);
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			}
		}
		const model =
			this.options.model ??
			process.env.ASTRA_MODEL ??
			(this.options.fixtureProvider ? "astra-fixture-1" : undefined);
		if (model) args.push("--model", model);
		const childEnv = {
			...process.env,
			ASTRA_JOB_ID: jobId,
			ASTRA_ROLE: role,
			ASTRA_SESSION_ID: sessionId,
			ASTRA_LAUNCHER: this.launcherPath,
			...(this.options.fixtureProvider ? { ASTRA_FIXTURE_PROVIDER: "1" } : {}),
			...env,
		};
		return new Promise((resolveResult) => {
			let settled = false;
			let timer: NodeJS.Timeout | undefined;
			let providerEventBuffer = "";
			const finish = (value: ProcessResult): void => {
				if (settled) return;
				settled = true;
				if (timer) clearTimeout(timer);
				resolveResult(value);
			};
			let child: ChildProcessWithoutNullStreams;
			try {
				const runtimeArgs = this.launcherPath.endsWith(".ts")
					? [...new Set([...process.execArgv, "--experimental-strip-types"]), this.launcherPath, ...args]
					: [this.launcherPath, ...args];
				child = spawn(process.execPath, runtimeArgs, {
					cwd,
					env: childEnv,
					stdio: ["pipe", "pipe", "pipe"],
					shell: false,
				});
			} catch (error) {
				finish({
					exitCode: 1,
					stdout: "",
					stderr: error instanceof Error ? error.message : String(error),
					jsonEvents: [],
				});
				return;
			}
			trackChild(child);
			let stdout = "";
			let stderr = "";
			const append = (target: "stdout" | "stderr", chunk: Buffer): void => {
				const value = chunk.toString("utf8");
				if (target === "stderr") {
					stderr = `${stderr}${value}`.slice(-this.maxOutputBytes);
					return;
				}
				stdout = `${stdout}${value}`.slice(-this.maxOutputBytes);
				providerEventBuffer += value;
				const lines = providerEventBuffer.split("\n");
				providerEventBuffer = lines.pop() ?? "";
				const providerError = providerErrorFromJsonEvents(parseJsonLines(lines.join("\n")));
				if (!providerError) return;
				child.kill("SIGTERM");
				finish({
					exitCode: 1,
					stdout,
					stderr: `${stderr}Pi child session stopped after provider failure: ${providerError.message}`,
					jsonEvents: parseJsonLines(stdout),
				});
			};
			child.stdout.on("data", (chunk: Buffer) => append("stdout", chunk));
			child.stderr.on("data", (chunk: Buffer) => append("stderr", chunk));
			child.stdin.on("error", (error) => {
				append("stderr", Buffer.from(`Pi child stdin error: ${error.message}`));
			});
			child.stdin.end(prompt);
			child.on("error", (error) =>
				finish({ exitCode: 1, stdout, stderr: `${stderr}${error.message}`, jsonEvents: parseJsonLines(stdout) }),
			);
			child.on("close", (exitCode) => {
				untrackChild(child);
				finish({ exitCode: exitCode ?? 1, stdout, stderr, jsonEvents: parseJsonLines(stdout) });
			});
			timer =
				timeoutMs && timeoutMs > 0
					? setTimeout(() => {
							child.kill("SIGTERM");
							finish({
								exitCode: 124,
								stdout,
								stderr: `${stderr}Pi child session timed out after ${timeoutMs}ms`,
								jsonEvents: parseJsonLines(stdout),
							});
						}, timeoutMs)
					: undefined;
		});
	}

	async runTask(
		task: TaskPacket,
		role: "worker" | "reviewer",
		prompt: string,
		env: Record<string, string | undefined> = {},
	): Promise<ProcessResult> {
		await writeTaskPacket(task);
		await rm(
			join(
				task.scope.workspaceRoot,
				".astra",
				"jobs",
				task.jobId,
				"tasks",
				task.id,
				role === "worker" ? "output-manifest.json" : "review-manifest.json",
			),
			{ force: true },
		);
		return this.run(
			env.ASTRA_EXECUTION_ROOT ?? task.scope.workspaceRoot,
			task.jobId,
			task.id,
			task.attempt,
			role,
			prompt,
			{
				ASTRA_PROJECT_ROOT: task.scope.workspaceRoot,
				ASTRA_STAGE_ID: task.stageId,
				ASTRA_TASK_PACKET: join(
					task.scope.workspaceRoot,
					".astra",
					"jobs",
					task.jobId,
					"tasks",
					task.id,
					"task-packet.json",
				),
				ASTRA_TASK_ID: task.id,
				ASTRA_ALLOWED_TOOLS: task.allowedTools.join(","),
				ASTRA_REQUIRED_OUTPUT_FIELDS: JSON.stringify(task.requiredOutputFields),
				ASTRA_REQUIRED_OUTPUT_TYPE: task.requiredOutputType,
				...env,
			},
			task.budget.maxRuntimeMs,
		);
	}

	async waitForManifest<T>(path: string, reader: (path: string) => Promise<T>, timeoutMs = 2_000): Promise<T> {
		const deadline = Date.now() + timeoutMs;
		let lastError: unknown;
		while (Date.now() < deadline) {
			try {
				await access(path);
				return await reader(path);
			} catch (error) {
				lastError = error;
				await new Promise((resolvePromise) => setTimeout(resolvePromise, 20));
			}
		}
		throw lastError instanceof Error ? lastError : new Error(`manifest not available: ${path}`);
	}
}

export class PiWorkerAdapter implements ResearchWorkerAdapter {
	private readonly runner: PiChildSessionRunner;

	constructor(runner: PiChildSessionRunner) {
		this.runner = runner;
	}

	async run(task: TaskPacket, job: ResearchJob): Promise<WorkerRunResult> {
		const sessionId = safeSessionId(task.jobId, task.id, task.attempt);
		await job.recordChildSession({
			sessionId,
			role: "worker",
			taskId: task.id,
			status: "starting",
			attempt: task.attempt,
			updatedAt: new Date().toISOString(),
		});
		const executionRoot = await prepareTaskWorkspace(task, job);
		const inputResources = await taskInputResources(task, job);
		const resourceRoot =
			task.writeAuthority === "workspace-write"
				? taskResourcePath(task.scope.workspaceRoot, task.jobId, task.id)
				: undefined;
		const result = await this.runner.runTask(
			task,
			"worker",
			`You are the Astra worker for TaskPacket ${task.id}. Read ASTRA_TASK_CONTEXT.json in the current isolated task workspace before acting. Execute only the authored objective. Upstream structured content is inline under inputs.canonicalArtifacts unless an entry provides contentPath; read that relative JSON file when present.${task.stageId === "research-review" ? " For research-review, read inputs.reviewSummaryPath first and then drill into at most three contentPath files needed to verify the highest-risk claims; do not read every canonical digest. Keep the submitted content under 3,500 characters with at most 3 strengths, at most 5 weaknesses, at most 5 claim-audit entries, and at most 5 required repairs. Independently report scientificOutcome and missionCoverage for the primary objective, then submit strengths, weaknesses, claimAudit, and requiredRepairs as concise string arrays matching the tool schema." : ""} Upstream files are available only at the exact relative paths in inputs.files[].path. Never assume an upstream basename exists in the workspace root. Do not load raw datasets or long logs into model context; use bounded commands or small summaries. Treat the current directory as the workspace root and never access a parent .astra directory or construct host absolute paths.${resourceRoot ? ' The only writable path outside the task workspace is the predeclared "$ASTRA_RESOURCE_ROOT". For resource-producing work, create environments, datasets, checkpoints, caches, and other reusable runtime assets under "$ASTRA_RESOURCE_ROOT" by referencing that environment variable literally in commands. Do not create a resources/ directory in the task workspace or derive a resource path from the task id. Keep source code, compact resource manifests, exact command records, and reviewable logs in the task workspace.' : ""} Produce every required output field and call astra_submit_worker_output exactly once. Only cite artifact/log refs for files you actually created and source refs returned by a retrieval tool; otherwise pass refs: [] and Astra will add the session ref. Never invent a ref. Do not decide adoption, route, or stage closure.`,
			{
				ASTRA_EXECUTION_ROOT: executionRoot,
				ASTRA_RESOURCE_ROOT: resourceRoot,
				ASTRA_INPUT_RESOURCE_ROOTS: JSON.stringify(inputResources.map((resource) => resource.root)),
				PIP_CACHE_DIR: resourceRoot ? join(resourceRoot, "cache", "pip") : undefined,
				HF_HOME: resourceRoot ? join(resourceRoot, "cache", "huggingface") : undefined,
				TORCH_HOME: resourceRoot ? join(resourceRoot, "cache", "torch") : undefined,
				XDG_CACHE_HOME: resourceRoot ? join(resourceRoot, "cache") : undefined,
				...Object.fromEntries(inputResources.map((resource) => [resource.envVar, resource.root])),
			},
		);
		await job.recordCost(costUsdFromJsonEvents(result.jsonEvents));
		const providerError = providerErrorFromJsonEvents(result.jsonEvents);
		if (providerError) {
			await job.recordChildSession({
				sessionId,
				role: "worker",
				taskId: task.id,
				status: providerError.kind === "capacity" ? "interrupted" : "failed",
				attempt: task.attempt,
				error: providerError.message,
				updatedAt: new Date().toISOString(),
			});
			throw researchErrorFromProvider(providerError);
		}
		const manifestPath = join(
			task.scope.workspaceRoot,
			".astra",
			"jobs",
			task.jobId,
			"tasks",
			task.id,
			"output-manifest.json",
		);
		if (result.exitCode !== 0) {
			await job.recordChildSession({
				sessionId,
				role: "worker",
				taskId: task.id,
				status: "failed",
				attempt: task.attempt,
				error: result.stderr || `exit ${result.exitCode}`,
				updatedAt: new Date().toISOString(),
			});
			throw new Error(`Pi worker exited with ${result.exitCode}: ${result.stderr}`);
		}
		let manifest: WorkerOutputManifest;
		try {
			manifest = await this.runner.waitForManifest(
				manifestPath,
				readWorkerOutputManifest,
				Math.min(task.budget.maxRuntimeMs, MANIFEST_WAIT_TIMEOUT_MS),
			);
			if (
				manifest.jobId !== task.jobId ||
				manifest.taskId !== task.id ||
				manifest.agentId !== task.agentId ||
				manifest.artifactType !== task.requiredOutputType
			)
				throw new Error("worker manifest identity does not match TaskPacket");
		} catch (error) {
			await job.recordChildSession({
				sessionId,
				role: "worker",
				taskId: task.id,
				status: "failed",
				attempt: task.attempt,
				error: error instanceof Error ? error.message : String(error),
				updatedAt: new Date().toISOString(),
			});
			throw error;
		}
		await job.recordChildSession({
			sessionId,
			role: "worker",
			taskId: task.id,
			status: "completed",
			attempt: task.attempt,
			sessionFile: await findSessionFile(
				this.runner.sessionDir ?? join(task.scope.workspaceRoot, ".astra", "jobs", task.jobId, "sessions"),
				safeSessionId(task.jobId, task.id, task.attempt),
			),
			manifestRef: manifestPath,
			updatedAt: new Date().toISOString(),
		});
		return {
			content: manifest.content,
			refs: manifest.outputRefs.map((ref) => ref.ref),
			artifactType: manifest.artifactType,
		};
	}
}

export class PiReviewerAdapter implements ResearchReviewerAdapter {
	private readonly runner: PiChildSessionRunner;

	constructor(runner: PiChildSessionRunner) {
		this.runner = runner;
	}

	async review(evidence: Evidence, job: ResearchJob): Promise<ReviewerRunResult> {
		const definition = job.definitions[evidence.stageId];
		const workerTask = job.state.tasks[evidence.taskId];
		if (!definition) throw new Error(`stage definition not found for review: ${evidence.stageId}`);
		if (!workerTask) throw new Error(`source worker task not found for evidence: ${evidence.id}`);
		const reviewOrdinal =
			Object.values(job.state.reviews).filter((review) => review.evidenceId === evidence.id).length + 1;
		const priorReviewer = Object.values(job.state.tasks).find(
			(candidate) =>
				candidate.role === "reviewer" &&
				candidate.inputArtifactRefs.includes(evidence.id) &&
				candidate.status !== "failed" &&
				!Object.values(job.state.reviews).some((review) => review.reviewerTaskId === candidate.id),
		);
		if (
			priorReviewer &&
			(priorReviewer.status === "succeeded" ||
				Object.values(job.state.sessions).some(
					(session) =>
						session.taskId === priorReviewer.id &&
						["starting", "running", "completed", "failed"].includes(session.status),
				))
		) {
			await job.failUncommittedReviewerTask(priorReviewer.id);
		}
		const task = await job.dispatchTask({
			stageId: evidence.stageId,
			stageExecutionId: job.state.stages[evidence.stageId].executionId ?? `stage_exec_${evidence.stageId}`,
			agentId: `reviewer_${evidence.id}_${reviewOrdinal}`,
			role: "reviewer",
			objective: `Review evidence ${evidence.id} against the current ${evidence.stageId} stage contract as independent reviewer ${reviewOrdinal}`,
			inputArtifactRefs: [evidence.id],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "review",
			requiredOutputFields: ["verdict", "findings"],
			acceptanceChecks: ["current-stage contract is applied independently", "findings are explicit"],
			failureSignals: ["future-stage deliverables are treated as current requirements", "missing review manifest"],
			dependencies: [],
			scope: {
				workspaceRoot: job.state.frame.permissions.workspaceRoot,
				allowedPaths: [`.astra/jobs/${job.state.frame.jobId}/tasks`],
			},
			allowedTools: ["read", "grep", "find", "ls"],
			writeAuthority: "none",
			budget: definition.reviewerBudget
				? { ...definition.reviewerBudget }
				: { maxTurns: 8, maxToolCalls: 32, maxRuntimeMs: 180_000 },
			reviewGateRequired: false,
			resumePolicy: "resume-session",
			successCriteria: ["review manifest written"],
			replayKey: `review:${evidence.id}:${reviewOrdinal}`,
		});
		const resolvedEvidenceRefs = await prepareReviewEvidenceBundle(task, evidence, job);
		const snapshotRef = await writeReviewSnapshot(
			{ schemaVersion: "astra.review_target_snapshot.v1", evidence, resolvedEvidenceRefs },
			task.scope.workspaceRoot,
			task.jobId,
			task.id,
		);
		await writeReviewPacket(
			{
				schemaVersion: "astra.review_packet.v1",
				id: `review_packet_${task.id}`,
				jobId: task.jobId,
				taskId: task.id,
				evidenceId: evidence.id,
				targetSnapshotHash: checksum({
					evidenceId: evidence.id,
					content: evidence.content,
					refs: evidence.refs,
					checksum: evidence.checksum,
				}),
				targetSnapshotRef: snapshotRef,
				inputRefs: task.inputArtifactRefs,
				resolvedEvidenceRefs,
				objective: task.objective,
				stageContract: {
					stageId: definition.id,
					label: definition.label,
					outputArtifactType: definition.outputArtifactType,
					requiredOutputFields: definition.requiredOutputFields,
					acceptanceChecks: definition.acceptanceChecks,
					failureSignals: definition.failureSignals,
				},
				workerContract: {
					objective: workerTask.objective,
					requiredOutputFields: workerTask.requiredOutputFields,
					acceptanceChecks: workerTask.acceptanceChecks,
					failureSignals: workerTask.failureSignals,
					successCriteria: workerTask.successCriteria,
				},
				reviewerRole: "reviewer",
				freshThread: true,
				blinded: true,
				bannedContext: ["executor interpretations", "subjective conclusions", "pre-ranked findings"],
				createdAt: new Date().toISOString(),
			},
			task.scope.workspaceRoot,
		);
		const frozenCriteria = [...new Set([...workerTask.acceptanceChecks, ...workerTask.successCriteria])];
		const sessionId = safeSessionId(task.jobId, task.id, task.attempt);
		await job.recordChildSession({
			sessionId,
			role: "reviewer",
			taskId: task.id,
			status: "starting",
			attempt: task.attempt,
			updatedAt: new Date().toISOString(),
		});
		await job.setTaskStatus(task.id, "running");
		const result = await this.runner.runTask(
			task,
			"reviewer",
			`You are an independent Astra reviewer. Read review-packet.json and review-target-snapshot.json from the current directory; do not search parent directories, job logs, or session transcripts. Judge only the current ${evidence.stageId} stage artifact against stageContract and workerContract. Review evidence ${evidence.id} from the immutable snapshot and inspect every relevant file listed in resolvedEvidenceRefs before passing a criterion that depends on it. If the worker claims a required file but resolvedEvidenceRefs does not contain it, fail the corresponding criterion. Source identifiers and Pi session refs remain provenance pointers and are not local files. Do not evaluate overall mission completion: future experiment, result, paper, or final mission deliverables are out of scope unless the current stage contract explicitly requires them. In astra_submit_review, include every workerContract.acceptanceChecks and workerContract.successCriteria string as a separate criterion, deduplicated only when the strings are identical, and copy each string exactly, including case and punctuation; do not paraphrase or normalize them. A pass verdict requires every frozen criterion to have passed=true. Call astra_submit_review exactly once with criterion-level scores, verified refs, pass, fail, partial, or blocked, and concrete findings. Do not modify canonical artifacts.`,
			{
				ASTRA_EVIDENCE_ID: evidence.id,
				ASTRA_REVIEW_CRITERIA: JSON.stringify(frozenCriteria),
				ASTRA_EXECUTION_ROOT: taskDir(task.scope.workspaceRoot, task.jobId, task.id),
			},
		);
		await job.recordCost(costUsdFromJsonEvents(result.jsonEvents));
		const providerError = providerErrorFromJsonEvents(result.jsonEvents);
		if (providerError) {
			await job.setTaskStatus(task.id, providerError.kind === "capacity" ? "ready" : "failed");
			await job.recordChildSession({
				sessionId,
				role: "reviewer",
				taskId: task.id,
				status: providerError.kind === "capacity" ? "interrupted" : "failed",
				attempt: task.attempt,
				error: providerError.message,
				updatedAt: new Date().toISOString(),
			});
			throw researchErrorFromProvider(providerError);
		}
		if (result.exitCode !== 0) {
			await job.setTaskStatus(task.id, "failed");
			await job.recordChildSession({
				sessionId,
				role: "reviewer",
				taskId: task.id,
				status: "failed",
				attempt: task.attempt,
				error: result.stderr || `exit ${result.exitCode}`,
				updatedAt: new Date().toISOString(),
			});
			throw new Error(`Pi reviewer exited with ${result.exitCode}: ${result.stderr}`);
		}
		const path = join(
			task.scope.workspaceRoot,
			".astra",
			"jobs",
			task.jobId,
			"tasks",
			task.id,
			"review-manifest.json",
		);
		let manifest: ReviewerOutputManifest;
		try {
			manifest = await this.runner.waitForManifest(
				path,
				readReviewerOutputManifest,
				Math.min(task.budget.maxRuntimeMs, MANIFEST_WAIT_TIMEOUT_MS),
			);
			if (manifest.jobId !== task.jobId || manifest.taskId !== task.id || manifest.evidenceId !== evidence.id)
				throw new Error("review manifest identity does not match TaskPacket");
		} catch (error) {
			await job.setTaskStatus(task.id, "failed");
			await job.recordChildSession({
				sessionId,
				role: "reviewer",
				taskId: task.id,
				status: "failed",
				attempt: task.attempt,
				error: error instanceof Error ? error.message : String(error),
				updatedAt: new Date().toISOString(),
			});
			throw error;
		}
		await job.recordChildSession({
			sessionId,
			role: "reviewer",
			taskId: task.id,
			status: "completed",
			attempt: task.attempt,
			sessionFile: await findSessionFile(
				this.runner.sessionDir ?? join(task.scope.workspaceRoot, ".astra", "jobs", task.jobId, "sessions"),
				sessionId,
			),
			manifestRef: path,
			updatedAt: new Date().toISOString(),
		});
		await writeReviewTrace(
			{
				schemaVersion: "astra.review_trace.v1",
				id: `review_trace_${task.id}`,
				packetId: `review_packet_${task.id}`,
				jobId: task.jobId,
				taskId: task.id,
				evidenceId: evidence.id,
				sessionId,
				verdict: manifest.verdict,
				findings: manifest.findings,
				createdAt: new Date().toISOString(),
			},
			task.scope.workspaceRoot,
		);
		return {
			verdict: manifest.verdict,
			findings: manifest.findings,
			reviewerTaskId: task.id,
			score: manifest.score,
			criteria: manifest.criteria,
			verifiedRefs: manifest.verifiedRefs,
		};
	}
}

export class PiMainAgentAdapter implements ResearchMainAgentAdapter {
	private readonly runner: PiChildSessionRunner;
	private readonly cwd: string;

	constructor(runner: PiChildSessionRunner, cwd: string) {
		this.runner = runner;
		this.cwd = cwd;
	}

	async planStage(
		job: ResearchJob,
		obligation?: Obligation,
		requestedMode: "decompose" | "search" | "repair" = obligation ? "repair" : "decompose",
	): Promise<StagePlanManifest> {
		const stageId = job.state.frame.activeStageId;
		const definition = job.definitions[stageId];
		const decisionRef = `stage-plan-${stageId}-${job.state.eventSeq + 1}-${Date.now()}`;
		const planId = `plan_${decisionRef}`;
		const sessionId = resumableMainSessionId(job);
		const activeCanonical = Object.values(job.state.canonical)
			.filter((artifact) => artifact.status === "active")
			.map((artifact) => ({ id: artifact.id, type: artifact.type, evidenceId: artifact.evidenceId }));
		const repairEvidence = obligation
			? Object.values(job.state.evidence)
					.filter((evidence) => evidence.stageId === stageId)
					.map((evidence) => ({ id: evidence.id, type: evidence.type, status: evidence.status }))
			: [];
		const latestSearchBatch = Object.values(job.state.searchBatches)
			.filter((batch) => batch.stageId === stageId && batch.status !== "superseded")
			.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
		const continuationBatch =
			requestedMode === "search" &&
			latestSearchBatch?.status === "exhausted" &&
			latestSearchBatch.round < latestSearchBatch.maxRounds
				? latestSearchBatch
				: undefined;
		const continuationContext = continuationBatch
			? {
					batch: continuationBatch,
					evaluations: Object.values(job.state.candidateEvaluations).filter(
						(evaluation) => evaluation.batchId === continuationBatch.id,
					),
				}
			: undefined;
		const availableInputRefs = [
			...activeCanonical.map((artifact) => artifact.id),
			...repairEvidence.map((evidence) => evidence.id),
		];
		await job.recordChildSession({
			sessionId,
			role: "main-agent",
			taskId: decisionRef,
			status: "running",
			attempt: 1,
			updatedAt: new Date().toISOString(),
		});
		const result = await this.runner.run(
			this.cwd,
			job.state.frame.jobId,
			decisionRef,
			1,
			"main-agent",
			`You are Astra's persistent main research agent and the only authority that may define subagent work. The active capability is ${stageId}; capabilities are not a fixed pipeline. The frozen capability contract is ${JSON.stringify(definition)}. The canonical research graph projection is ${JSON.stringify(projectResearchGraph(job.state))}. Available canonical and repair inputs are ${JSON.stringify({ activeCanonical, repairEvidence })}. The requested plan mode is ${requestedMode}. ${obligation ? `This repair plan must resolve obligation ${obligation.id}: ${obligation.description}. Submit exactly one complete repair task.` : requestedMode === "search" ? `Author ${definition.searchPolicy?.minCandidates ?? 2} to ${definition.searchPolicy?.maxCandidates ?? 4} genuinely diverse candidate tasks. Give each a distinct hypothesis and use mode search; candidates run independently and will be compared by frozen criteria ${JSON.stringify(definition.searchPolicy?.criteria ?? definition.acceptanceChecks)}.${continuationContext ? ` This is bounded search round ${continuationBatch?.round ? continuationBatch.round + 1 : 1}/${continuationBatch?.maxRounds ?? definition.searchPolicy?.maxRounds ?? 2}. The previous round and independent evaluations are ${JSON.stringify(continuationContext)}. Its tie rationale was ${JSON.stringify(continuationBatch?.continuationRationale)}. Create orthogonal discriminators that can break that exact tie; do not repeat any previous hypothesis.` : " This is the first bounded search round."}` : "Author one focused execution task, or two non-overlapping tasks only when their outputs are independently useful."} Call astra_submit_stage_plan exactly once. Tasks in the same plan run concurrently in isolated workspaces: a task cannot consume another task's output from the same plan. If work has an ordering dependency, plan only the prerequisite task and let a later main-agent round plan its consumer. Every task must include every authored canonical ref it needs; full immutable contents will be materialized in its workspace. Do not perform worker work or choose the route yourself.`,
			{
				ASTRA_SESSION_ID: sessionId,
				ASTRA_PROJECT_ROOT: this.cwd,
				ASTRA_DECISION_REF: decisionRef,
				ASTRA_STAGE_PLAN_ID: planId,
				ASTRA_STAGE_ID: stageId,
				ASTRA_STAGE_REQUIRED_FIELDS: JSON.stringify(definition.requiredOutputFields),
				ASTRA_AVAILABLE_INPUT_REFS: JSON.stringify(availableInputRefs),
				ASTRA_OBLIGATION_ID: obligation?.id,
				ASTRA_PLAN_MODE: requestedMode,
				ASTRA_SEARCH_MIN_CANDIDATES: String(definition.searchPolicy?.minCandidates ?? 2),
				ASTRA_SEARCH_PREVIOUS_BATCH_ID: continuationBatch?.id,
			},
			180_000,
		);
		await job.recordCost(costUsdFromJsonEvents(result.jsonEvents));
		const providerError = providerErrorFromJsonEvents(result.jsonEvents);
		if (providerError) {
			await job.recordChildSession({
				sessionId,
				role: "main-agent",
				taskId: decisionRef,
				status: providerError.kind === "capacity" ? "interrupted" : "failed",
				attempt: 1,
				error: providerError.message,
				updatedAt: new Date().toISOString(),
			});
			throw researchErrorFromProvider(providerError);
		}
		if (result.exitCode !== 0) {
			await job.recordChildSession({
				sessionId,
				role: "main-agent",
				taskId: decisionRef,
				status: "failed",
				attempt: 1,
				error: result.stderr || `exit ${result.exitCode}`,
				updatedAt: new Date().toISOString(),
			});
			throw new Error(`Pi main-agent stage planner exited with ${result.exitCode}: ${result.stderr}`);
		}
		const path = stagePlanManifestPath(this.cwd, job.state.frame.jobId, planId);
		let manifest: StagePlanManifest;
		try {
			manifest = await this.runner.waitForManifest(path, readStagePlanManifest, MANIFEST_WAIT_TIMEOUT_MS);
			if (
				manifest.jobId !== job.state.frame.jobId ||
				manifest.stageId !== stageId ||
				manifest.decisionRef !== decisionRef ||
				manifest.obligationId !== obligation?.id
			) {
				throw new Error("stage plan manifest identity does not match the planning round");
			}
		} catch (error) {
			await job.recordChildSession({
				sessionId,
				role: "main-agent",
				taskId: decisionRef,
				status: "failed",
				attempt: 1,
				error: error instanceof Error ? error.message : String(error),
				updatedAt: new Date().toISOString(),
			});
			throw error;
		}
		await job.recordChildSession({
			sessionId,
			role: "main-agent",
			taskId: decisionRef,
			status: "completed",
			attempt: 1,
			sessionFile: await findSessionFile(
				this.runner.sessionDir ?? join(this.cwd, ".astra", "jobs", job.state.frame.jobId, "sessions"),
				sessionId,
			),
			manifestRef: path,
			updatedAt: new Date().toISOString(),
		});
		return manifest;
	}

	private async decide(
		job: ResearchJob,
		type: MainAgentDecisionManifest["decisionType"],
		prompt: string,
		env: Record<string, string> = {},
	): Promise<MainAgentDecisionManifest> {
		const decisionRef = `decision-${type}-${job.state.eventSeq + 1}-${Date.now()}`;
		const sessionId = resumableMainSessionId(job);
		await job.recordChildSession({
			sessionId,
			role: "main-agent",
			taskId: decisionRef,
			status: "running",
			attempt: 1,
			updatedAt: new Date().toISOString(),
		});
		const result = await this.runner.run(
			this.cwd,
			job.state.frame.jobId,
			decisionRef,
			1,
			"main-agent",
			`${prompt} Use decisionRef ${decisionRef}.`,
			{
				ASTRA_SESSION_ID: sessionId,
				ASTRA_DECISION_TYPE: type,
				ASTRA_DECISION_REF: decisionRef,
				ASTRA_EVIDENCE_ID: env.ASTRA_EVIDENCE_ID,
				ASTRA_STAGE_ID: job.state.frame.activeStageId,
				...env,
			},
			180_000,
		);
		await job.recordCost(costUsdFromJsonEvents(result.jsonEvents));
		const providerError = providerErrorFromJsonEvents(result.jsonEvents);
		if (providerError) {
			await job.recordChildSession({
				sessionId,
				role: "main-agent",
				taskId: decisionRef,
				status: providerError.kind === "capacity" ? "interrupted" : "failed",
				attempt: 1,
				error: providerError.message,
				updatedAt: new Date().toISOString(),
			});
			throw researchErrorFromProvider(providerError);
		}
		if (result.exitCode !== 0) {
			await job.recordChildSession({
				sessionId,
				role: "main-agent",
				taskId: decisionRef,
				status: "failed",
				attempt: 1,
				error: result.stderr || `exit ${result.exitCode}`,
				updatedAt: new Date().toISOString(),
			});
			throw new Error(`Pi main-agent exited with ${result.exitCode}: ${result.stderr}`);
		}
		const path = mainDecisionManifestPath(this.cwd, job.state.frame.jobId, type, decisionRef);
		let manifest: MainAgentDecisionManifest;
		try {
			manifest = await this.runner.waitForManifest(
				path,
				async (manifestPath) => readJson<MainAgentDecisionManifest>(manifestPath),
				MANIFEST_WAIT_TIMEOUT_MS,
			);
		} catch (error) {
			await job.recordChildSession({
				sessionId,
				role: "main-agent",
				taskId: decisionRef,
				status: "failed",
				attempt: 1,
				error: error instanceof Error ? error.message : String(error),
				updatedAt: new Date().toISOString(),
			});
			throw error;
		}
		await job.recordChildSession({
			sessionId,
			role: "main-agent",
			taskId: decisionRef,
			status: "completed",
			attempt: 1,
			sessionFile: await findSessionFile(
				this.runner.sessionDir ?? join(this.cwd, ".astra", "jobs", job.state.frame.jobId, "sessions"),
				sessionId,
			),
			manifestRef: path,
			updatedAt: new Date().toISOString(),
		});
		return manifest;
	}

	decideEvidence(evidence: Evidence, job: ResearchJob): Promise<MainAgentDecisionManifest> {
		const reviewEvidenceGuidance =
			evidence.stageId === "research-review"
				? " A FAIL verdict is valid negative evidence when the review is complete, grounded, and explicit about required repairs; do not reject it merely because the assessment is negative. Evidence acceptance validates the review artifact, not the reviewed research."
				: "";
		return this.decide(
			job,
			"evidence",
			`You are Astra's persistent main research agent. The complete evidence snapshot is ${JSON.stringify(evidence)} and its independent reviews are ${JSON.stringify(Object.values(job.state.reviews).filter((review) => review.evidenceId === evidence.id))}. Decide whether to accept, reject, or defer it.${reviewEvidenceGuidance} You MUST call astra_submit_main_decision with decisionType evidence, evidenceId ${evidence.id}, and a concrete rationale.`,
			{ ASTRA_EVIDENCE_ID: evidence.id },
		);
	}

	decideAdoption(evidence: Evidence, job: ResearchJob): Promise<MainAgentDecisionManifest> {
		const replaceableArtifactIds = Object.values(job.state.canonical)
			.filter((artifact) => artifact.status === "active" && artifact.type === evidence.type)
			.map((artifact) => artifact.id);
		return this.decide(
			job,
			"adoption",
			`You are Astra's main research agent. This complete evidence snapshot has passed independent review: ${JSON.stringify(evidence)}. Decide whether to adopt it as canonical without inspecting the filesystem. Valid active canonical artifact ids for replacementOf are ${JSON.stringify(replaceableArtifactIds)}. Use replacementOf only when replacing one of those exact ids; omit it when the list is empty. Call astra_submit_main_decision with decisionType adoption, evidenceId ${evidence.id}, adopt true or false, and rationale.`,
			{ ASTRA_EVIDENCE_ID: evidence.id },
		);
	}

	decideSearch(
		batch: SearchBatch,
		evaluations: CandidateEvaluation[],
		job: ResearchJob,
	): Promise<MainAgentDecisionManifest> {
		const allowContinue = batch.round < batch.maxRounds;
		return this.decide(
			job,
			"search-selection",
			`You are Astra's persistent main research agent. Compare this durable search batch ${JSON.stringify(batch)} using only these independent criterion-level evaluations ${JSON.stringify(evaluations)}. ${allowContinue ? "Select one passing candidate when it dominates on the frozen criteria. Request continued search only when the evaluations expose a concrete discriminator that a new orthogonal round can test." : "This is the final bounded search round: you must select one passing candidate and may not continue search. If candidates remain tied, rank them deterministically by criterion scores in the frozen criteria order, then overall score, then candidateId ascending."} Call astra_submit_main_decision with decisionType search-selection, searchBatchId ${batch.id}, ${allowContinue ? "selectedCandidateId or continueSearch" : "selectedCandidateId"}, and a concrete rationale.`,
			{
				ASTRA_SEARCH_BATCH_ID: batch.id,
				ASTRA_CANDIDATE_IDS: JSON.stringify(Object.keys(batch.candidates)),
				ASTRA_SEARCH_ALLOW_CONTINUE: allowContinue ? "1" : "0",
			},
		);
	}

	decideRoute(job: ResearchJob): Promise<MainAgentDecisionManifest> {
		const snapshot = job.state;
		const currentArtifactId = snapshot.canonicalRoute.stageArtifactIds[snapshot.frame.activeStageId];
		const currentArtifact = currentArtifactId ? snapshot.canonical[currentArtifactId] : undefined;
		const currentEvidence = currentArtifact ? snapshot.evidence[currentArtifact.evidenceId] : undefined;
		const currentSearchBatches = Object.values(snapshot.searchBatches).filter(
			(batch) => batch.stageId === snapshot.frame.activeStageId && batch.status !== "superseded",
		);
		const currentSearchBatchIds = new Set(currentSearchBatches.map((batch) => batch.id));
		const activeArtifacts = Object.values(snapshot.canonical).filter((artifact) => artifact.status === "active");
		const recentRouteDecisions = Object.values(snapshot.routeDecisions)
			.sort((left, right) => left.createdAt.localeCompare(right.createdAt))
			.slice(-6);
		const routeContext = {
			projection: {
				kind: "astra.route_decision_context.v1",
				sourceRef: `job:${snapshot.frame.jobId}:event:${snapshot.eventSeq}`,
				note: "Canonical content and provenance remain durable under the listed ids and refs; only the current decision surface is projected inline.",
			},
			status: job.status(),
			graph: projectResearchGraph(snapshot),
			canonicalRoute: snapshot.canonicalRoute,
			canonicalArtifacts: activeArtifacts.map((artifact) => projectCanonicalArtifact(artifact, snapshot, false)),
			currentArtifact: currentArtifact ? projectCanonicalArtifact(currentArtifact, snapshot, true) : null,
			currentEvidence: currentEvidence
				? {
						id: currentEvidence.id,
						taskId: currentEvidence.taskId,
						stageId: currentEvidence.stageId,
						type: currentEvidence.type,
						status: currentEvidence.status,
						checksum: currentEvidence.checksum,
						refs: currentEvidence.refs.slice(0, 12),
					}
				: null,
			currentReviews: currentEvidence
				? compactDecisionValue(
						Object.values(snapshot.reviews).filter((review) => review.evidenceId === currentEvidence.id),
					)
				: [],
			currentSearchBatches: compactDecisionValue(currentSearchBatches),
			currentCandidateEvaluations: compactDecisionValue(
				Object.values(snapshot.candidateEvaluations).filter((evaluation) =>
					currentSearchBatchIds.has(evaluation.batchId),
				),
			),
			recentRouteDecisions: compactDecisionValue(recentRouteDecisions),
			completionBlockers: job.completionBlockers(),
			capabilities: Object.values(job.definitions).map((definition) => ({
				id: definition.id,
				label: definition.label,
				suggestedInputArtifactTypes: definition.suggestedInputArtifactTypes,
			})),
		};
		return this.decide(
			job,
			"route",
			`You are Astra's persistent main research agent and control the canonical route. Inspect this bounded, lossless-by-reference route context ${JSON.stringify(routeContext)}. Choose exactly one action: continue the current evidence loop, search alternatives, advance to the highest-value capability, backtrack to invalidate a flawed upstream conclusion, ask the user one concrete blocking question, or complete. Completion is forbidden while completionBlockers is non-empty. Process completion does not mean the hypothesis was supported: preserve the recorded scientificOutcome and missionCoverage, and continue gathering evidence only when an in-scope action can materially improve coverage. A negative whole-research review requires backtrack or continue, never complete. In collaborative mode, use ask-user only for a decision where the user's scientific preference, boundary, risk tolerance, or external knowledge can materially change the route; do not ask for routine task approval. Call astra_submit_main_decision with decisionType route, stageId ${snapshot.frame.activeStageId}, routeAction, targetStageId when advancing/backtracking, evidenceRefs, any newQuestions, and rationale.`,
			{
				ASTRA_COMPLETION_BLOCKERS: JSON.stringify(routeContext.completionBlockers),
				ASTRA_ROUTE_EVIDENCE_REFS: JSON.stringify(routeContext.canonicalArtifacts.map((artifact) => artifact.id)),
			},
		);
	}
}
