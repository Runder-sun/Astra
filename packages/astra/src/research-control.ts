import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { CodexResearchAdapters } from "./codex-adapters.ts";
import { CodexAppServerRunner } from "./codex-app-server.ts";
import { assertAstraId, atomicWriteJson } from "./contracts.ts";
import { migratePmcli } from "./migration.ts";
import { applyPendingPauses, requestResearchPause } from "./pause-control.ts";
import { PiChildSessionRunner, PiMainAgentAdapter, PiReviewerAdapter, PiWorkerAdapter } from "./pi-child-session.ts";
import { ResearchJob } from "./research.ts";
import { JsonlAstraStore, ResearchJobLockedError } from "./store.ts";
import { ResearchSupervisor } from "./supervisor.ts";
import type { AutomationLevel, MissionFrame } from "./types.ts";

export type ResearchControlAction = "run" | "status" | "tick" | "pause" | "resume" | "migrate";

const RESEARCH_CONTROL_ACTIONS: readonly ResearchControlAction[] = [
	"run",
	"status",
	"tick",
	"pause",
	"resume",
	"migrate",
];

export interface ResearchControlRequest {
	action: ResearchControlAction;
	backend?: "pi" | "codex";
	objective?: string;
	reason?: string;
	automation?: AutomationLevel;
	maxTasks?: number;
	maxTurns?: number;
	maxCostUsd?: number;
	unlimitedCost?: boolean;
	requirePaper?: boolean;
	guidance?: string;
}

export interface ResearchControlResult {
	action: ResearchControlAction;
	pauseRequested?: boolean;
	backend?: "pi" | "codex";
	costAccounting?: "provider-estimate" | "subscription-unavailable";
	jobId?: string;
	status?: ReturnType<ResearchJob["status"]>;
	control?: Awaited<ReturnType<ResearchSupervisor["tick"]>>;
	migration?: Awaited<ReturnType<typeof migratePmcli>>;
}

interface ActiveJobRef {
	jobId?: string;
}

export function encodeResearchControl(request: ResearchControlRequest): string {
	return JSON.stringify(request);
}

function isResearchControlAction(value: string): value is ResearchControlAction {
	return RESEARCH_CONTROL_ACTIONS.includes(value as ResearchControlAction);
}

function isAutomationLevel(value: string): value is AutomationLevel {
	return ["collaborative", "autonomous", "full"].includes(value);
}

function parseNumberFlag(name: string, value: string, integer: boolean): number {
	const parsed = Number(value);
	const valid = Number.isFinite(parsed) && parsed >= 0 && (!integer || (Number.isInteger(parsed) && parsed > 0));
	if (!valid) throw new Error(`invalid Astra ${name} value: ${value}`);
	return parsed;
}

function parseRunOptions(args: string[]): Omit<ResearchControlRequest, "action" | "objective" | "reason"> & {
	positional: string[];
} {
	const options: Omit<ResearchControlRequest, "action" | "objective" | "reason"> & { positional: string[] } = {
		positional: [],
	};
	for (let index = 0; index < args.length; index++) {
		const token = args[index];
		if (token === "--") {
			options.positional.push(...args.slice(index + 1));
			break;
		}
		if (!token.startsWith("--")) {
			options.positional.push(token);
			continue;
		}
		const separator = token.indexOf("=");
		const name = separator === -1 ? token : token.slice(0, separator);
		const inlineValue = separator === -1 ? undefined : token.slice(separator + 1);
		if (name === "--require-paper") {
			if (inlineValue !== undefined) throw new Error("Astra --require-paper does not accept a value");
			options.requirePaper = true;
			continue;
		}
		const value = inlineValue ?? args[++index];
		if (value === undefined || value.startsWith("--")) throw new Error(`missing value for Astra ${name}`);
		switch (name) {
			case "--backend":
				if (value !== "pi" && value !== "codex") throw new Error(`unknown Astra backend: ${value}`);
				options.backend = value;
				break;
			case "--guidance":
				options.guidance = value;
				break;
			case "--automation":
				if (!isAutomationLevel(value)) throw new Error(`unknown Astra automation level: ${value}`);
				options.automation = value;
				break;
			case "--max-tasks":
				options.maxTasks = parseNumberFlag(name, value, true);
				break;
			case "--max-turns":
				options.maxTurns = parseNumberFlag(name, value, true);
				break;
			case "--max-cost-usd":
				if (value === "unlimited") options.unlimitedCost = true;
				else options.maxCostUsd = parseNumberFlag(name, value, false);
				break;
			default:
				throw new Error(`unknown Astra research option: ${name}`);
		}
	}
	return options;
}

export function parseResearchControlArgs(args: string[]): ResearchControlRequest {
	const requestedAction = args[0] ?? "status";
	const action = requestedAction === "continue" ? "resume" : requestedAction;
	if (!isResearchControlAction(action)) throw new Error(`unknown Astra research action: ${requestedAction}`);
	const remainder = args.slice(1);
	if (action === "pause") {
		const reason = remainder.join(" ").trim();
		return { action, ...(reason ? { reason } : {}) };
	}
	if (action !== "run" && action !== "resume") {
		if (remainder.length > 0) throw new Error(`Astra research ${action} does not accept arguments`);
		return { action };
	}
	const { positional, ...options } = parseRunOptions(remainder);
	if (action === "resume" && positional.length > 0)
		throw new Error("astra research resume does not accept an objective");
	if (action === "resume" && options.requirePaper) {
		throw new Error("astra research resume cannot add a paper deliverable");
	}
	if (action !== "resume" && options.guidance !== undefined) {
		throw new Error("Astra guidance is only valid when resuming research");
	}
	const objective = positional.join(" ").trim();
	return {
		action,
		...options,
		...(action === "run" && objective ? { objective } : {}),
	};
}

export function decodeResearchControl(value: string): ResearchControlRequest {
	const request = JSON.parse(value) as Partial<ResearchControlRequest>;
	if (request.backend !== undefined && request.backend !== "pi" && request.backend !== "codex") {
		throw new Error(`unknown Astra backend: ${String(request.backend)}`);
	}
	if (!request.action || !isResearchControlAction(request.action)) {
		throw new Error(`unknown Astra research action: ${String(request.action)}`);
	}
	if (request.automation !== undefined && !isAutomationLevel(request.automation)) {
		throw new Error(`unknown Astra automation level: ${String(request.automation)}`);
	}
	if (request.maxTasks !== undefined) parseNumberFlag("--max-tasks", String(request.maxTasks), true);
	if (request.maxTurns !== undefined) parseNumberFlag("--max-turns", String(request.maxTurns), true);
	if (request.maxCostUsd !== undefined) parseNumberFlag("--max-cost-usd", String(request.maxCostUsd), false);
	if (request.unlimitedCost !== undefined && typeof request.unlimitedCost !== "boolean") {
		throw new Error("Astra unlimitedCost must be a boolean");
	}
	if (request.requirePaper !== undefined && typeof request.requirePaper !== "boolean") {
		throw new Error("Astra requirePaper must be a boolean");
	}
	if (request.guidance !== undefined && (typeof request.guidance !== "string" || !request.guidance.trim())) {
		throw new Error("Astra guidance must be a non-empty string");
	}
	if (request.action !== "resume" && request.guidance !== undefined) {
		throw new Error("Astra guidance is only valid when resuming research");
	}
	if (request.unlimitedCost && request.maxCostUsd !== undefined) {
		throw new Error("Astra cost budget cannot be both unlimited and finite");
	}
	return request as ResearchControlRequest;
}

async function activeJobId(cwd: string): Promise<string | undefined> {
	try {
		return (JSON.parse(await readFile(join(cwd, ".astra", "active-job.json"), "utf8")) as ActiveJobRef).jobId;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
		throw error;
	}
}

async function bindActiveJob(cwd: string, jobId: string): Promise<void> {
	await mkdir(join(cwd, ".astra"), { recursive: true });
	await writeFile(join(cwd, ".astra", "active-job.json"), `${JSON.stringify({ jobId }, null, 2)}\n`, "utf8");
}

export async function researchBackend(request: ResearchControlRequest, cwd: string): Promise<"pi" | "codex"> {
	if (request.action === "run") return request.backend ?? "pi";
	const jobId = await activeJobId(cwd);
	if (!jobId) return request.backend ?? "pi";
	assertAstraId(jobId, "job id");
	let backend: "pi" | "codex" = "pi";
	try {
		const stored = JSON.parse(await readFile(join(cwd, ".astra", "jobs", jobId, "backend.json"), "utf8")) as {
			backend?: unknown;
		};
		if (stored.backend !== "pi" && stored.backend !== "codex") throw new Error("invalid persisted Astra backend");
		backend = stored.backend;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
	}
	if (request.backend && request.backend !== backend)
		throw new Error("Cannot change an existing research job's backend; start a new job");
	return backend;
}

function createSupervisor(
	job: ResearchJob,
	store: JsonlAstraStore,
	cwd: string,
	backend: "pi" | "codex",
): ResearchSupervisor {
	if (backend === "codex") {
		const adapters = new CodexResearchAdapters(new CodexAppServerRunner());
		return new ResearchSupervisor(job, store, {
			worker: adapters,
			reviewer: adapters,
			mainAgent: adapters,
			maxParallel: 1,
		});
	}
	const runner = new PiChildSessionRunner({ fixtureProvider: process.env.ASTRA_FIXTURE_PROVIDER === "1" });
	return new ResearchSupervisor(job, store, {
		worker: new PiWorkerAdapter(runner),
		reviewer: new PiReviewerAdapter(runner),
		mainAgent: new PiMainAgentAdapter(runner, cwd),
	});
}

async function driveToCompletion(
	job: ResearchJob,
	store: JsonlAstraStore,
	cwd: string,
	backend: "pi" | "codex",
): Promise<void> {
	const supervisor = createSupervisor(job, store, cwd, backend);
	const maxTicks = Number(process.env.ASTRA_MAX_TICKS ?? "64");
	let ticks = 0;
	while (job.state.frame.status !== "completed") {
		await store.withJobLock(job.state.frame.jobId, `control_${process.pid}`, async () => {
			await job.reload();
			await applyPendingPauses(job);
		});
		if (job.state.paused) return;
		const retryAt = job.state.providerBackoff?.retryAt;
		const waitMs = retryAt ? Date.parse(retryAt) - Date.now() : 0;
		if (waitMs > 0) {
			await new Promise((resolvePromise) => setTimeout(resolvePromise, Math.min(waitMs, 250)));
			continue;
		}
		if (ticks >= maxTicks) throw new Error(`research run exceeded ${maxTicks} ticks`);
		const result = await supervisor.tick();
		if (!job.state.providerBackoff) ticks++;
		if (result.paused || job.state.paused) return;
	}
}

async function mutateLatestJob(
	store: JsonlAstraStore,
	jobId: string,
	operation: (job: ResearchJob) => Promise<void>,
): Promise<ResearchJob> {
	return store.withJobLock(jobId, `control_${process.pid}`, async () => {
		const job = await ResearchJob.open(store, jobId);
		if (!job) throw new Error(`active Astra research job not found: ${jobId}`);
		await operation(job);
		return job;
	});
}

export async function runResearchControl(request: ResearchControlRequest, cwd: string): Promise<ResearchControlResult> {
	if (request.action === "migrate") {
		return { action: request.action, migration: await migratePmcli(cwd) };
	}

	const store = new JsonlAstraStore(cwd);
	const backend = await researchBackend(request, cwd);
	const accounting = {
		backend,
		costAccounting: backend === "codex" ? ("subscription-unavailable" as const) : ("provider-estimate" as const),
	};
	if (backend === "codex" && request.maxCostUsd !== undefined) {
		throw new Error("Codex subscription does not report a USD bill; use --max-tasks and --max-turns instead");
	}
	if (backend === "codex" && process.env.ASTRA_FIXTURE_PROVIDER === "1") {
		throw new Error("ASTRA_FIXTURE_PROVIDER is a Pi fixture; it cannot authenticate the official Codex backend");
	}
	if (request.action === "run") {
		const objective = request.objective?.trim();
		if (!objective) throw new Error("astra research run requires an objective");
		const job = await ResearchJob.create(store, {
			objective,
			workspaceRoot: cwd,
			automation: request.automation,
			maxTasks: request.maxTasks,
			maxTurns: request.maxTurns,
			maxCostUsd: request.unlimitedCost ? undefined : request.maxCostUsd,
			requiredArtifactTypes: request.requirePaper ? ["paper-write", "paper-compile"] : undefined,
		});
		await atomicWriteJson(join(cwd, ".astra", "jobs", job.state.frame.jobId, "backend.json"), { backend });
		await bindActiveJob(cwd, job.state.frame.jobId);
		await driveToCompletion(job, store, cwd, backend);
		return { action: request.action, ...accounting, jobId: job.state.frame.jobId, status: job.status() };
	}

	const jobId = await activeJobId(cwd);
	if (!jobId) throw new Error("no active Astra research job");
	let job = await ResearchJob.open(store, jobId);
	if (!job) throw new Error(`active Astra research job not found: ${jobId}`);
	if (request.action === "status") return { action: request.action, ...accounting, jobId, status: job.status() };
	if (request.action === "pause") {
		await requestResearchPause(cwd, jobId, request.reason?.trim() || "paused by operator");
		try {
			job = await mutateLatestJob(store, jobId, applyPendingPauses);
		} catch (error) {
			if (!(error instanceof ResearchJobLockedError)) throw error;
			return { action: request.action, ...accounting, jobId, status: job.status(), pauseRequested: true };
		}
		return { action: request.action, ...accounting, jobId, status: job.status() };
	}
	if (request.action === "resume") {
		job = await mutateLatestJob(store, jobId, async (latest) => {
			await applyPendingPauses(latest);
			if (request.automation !== undefined && request.automation !== latest.state.frame.automation) {
				await latest.setAutomation(request.automation);
			}
			const budget: Partial<MissionFrame["budget"]> = {};
			if (request.maxTasks !== undefined) budget.maxTasks = request.maxTasks;
			if (request.maxTurns !== undefined) budget.maxTurns = request.maxTurns;
			if (request.maxCostUsd !== undefined) budget.maxCostUsd = request.maxCostUsd;
			if (request.unlimitedCost) budget.maxCostUsd = undefined;
			if (Object.keys(budget).length > 0) await latest.updateBudget(budget);
			if (request.guidance) await latest.resumeWithGuidance(request.guidance);
			else await latest.resume();
		});
		await driveToCompletion(job, store, cwd, backend);
		return { action: request.action, ...accounting, jobId, status: job.status() };
	}
	const control = await createSupervisor(job, store, cwd, backend).tick();
	return { action: request.action, ...accounting, jobId, status: job.status(), control };
}
