import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { requestResearchPause } from "../src/pause-control.ts";
import { ResearchJob } from "../src/research.ts";
import { decodeResearchControl, parseResearchControlArgs, runResearchControl } from "../src/research-control.ts";
import { JsonlAstraStore } from "../src/store.ts";

interface CompletableJob {
	state: { frame: { activeStageId: string; automation: string; status: "running" | "completed"; jobId: string } };
	commit(event: {
		type: "route_decided";
		decision: {
			id: string;
			stageId: string;
			action: "complete";
			evidenceRefs: string[];
			newQuestions: string[];
			rationale: string;
			sessionRef: string;
			createdAt: string;
		};
	}): Promise<void>;
	requireUserGate(input: { kind: "research"; stageId: string; question: string; reason: string }): Promise<void>;
}

const supervisorState = vi.hoisted(() => ({
	ticks: 0,
	tickHook: undefined as ((jobId: string) => Promise<void>) | undefined,
}));

vi.mock("../src/supervisor.ts", () => ({
	ResearchSupervisor: class {
		private readonly job: CompletableJob;

		constructor(job: CompletableJob) {
			this.job = job;
		}

		async tick() {
			supervisorState.ticks++;
			await supervisorState.tickHook?.(this.job.state.frame.jobId);
			const stageId = this.job.state.frame.activeStageId;
			if (this.job.state.frame.automation === "collaborative") {
				await this.job.requireUserGate({
					kind: "research",
					stageId,
					question: "Which route should dominate?",
					reason: `The route from ${stageId} depends on user preference`,
				});
				return { stageId, dispatchedTaskIds: [], completed: false, recovered: false, paused: true };
			}
			await this.job.commit({
				type: "route_decided",
				decision: {
					id: "test-complete",
					stageId,
					action: "complete",
					evidenceRefs: [],
					newQuestions: [],
					rationale: "test driver completion",
					sessionRef: "test:main",
					createdAt: new Date().toISOString(),
				},
			});
			return {
				stageId,
				dispatchedTaskIds: [],
				completed: true,
				routeChanged: false,
				recovered: false,
				paused: false,
			};
		}
	},
}));

afterEach(() => {
	supervisorState.ticks = 0;
	supervisorState.tickHook = undefined;
	vi.unstubAllEnvs();
});

describe("research control recovery", () => {
	it("holds and publishes execution identity for run, resume and public tick", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-control-owner-"));
		try {
			const store = new JsonlAstraStore(root);
			const published: string[] = [];
			supervisorState.tickHook = async (jobId) => {
				expect(published.at(-1)).toBe(jobId);
				expect(await store.readExecutionOwner(jobId)).toMatchObject({ jobId, pid: process.pid });
				// The mock tick has no supervisor lock, exercising an actual control-driver gap.
				await expect(runResearchControl({ action: "resume", jobId, automation: "full" }, root)).rejects.toThrow(
					"execution lock held",
				);
			};
			const publish = (jobId: string) => {
				published.push(jobId);
			};
			await runResearchControl({ action: "run", objective: "owner during run", automation: "full" }, root, publish);
			for (const action of ["resume", "tick"] as const) {
				const job = await ResearchJob.create(store, {
					objective: `owner during ${action}`,
					workspaceRoot: root,
					automation: "full",
				});
				if (action === "resume") await job.pause("resume fixture");
				await runResearchControl({ action, jobId: job.state.frame.jobId }, root, publish);
				expect(await store.readExecutionOwner(job.state.frame.jobId)).toBeUndefined();
			}
			expect(published).toHaveLength(3);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it.each([undefined, "keep the completed result"])(
		"keeps completed resume inert with guidance %s",
		async (guidance) => {
			const root = await mkdtemp(join(tmpdir(), "astra-control-completed-"));
			try {
				const initial = await runResearchControl(
					{ action: "run", objective: "completed fixture", automation: "full" },
					root,
				);
				const store = new JsonlAstraStore(root);
				const before = await store.readEvents(initial.jobId!);
				await requestResearchPause(root, initial.jobId!, "pending pause after completion");
				const snapshot = await readFile(join(root, ".astra", "jobs", initial.jobId!, "job.json"), "utf8");
				const result = await runResearchControl(
					{
						action: "resume",
						jobId: initial.jobId,
						guidance,
						maxTasks: 999,
						maxTurns: 999,
						unlimitedCost: true,
						automation: "collaborative",
					},
					root,
				);
				expect(result.status?.status).toBe("completed");
				expect(await store.readEvents(initial.jobId!)).toEqual(before);
				expect(await readFile(join(root, ".astra", "jobs", initial.jobId!, "job.json"), "utf8")).toEqual(snapshot);
				expect(supervisorState.ticks).toBe(1);
			} finally {
				await rm(root, { recursive: true, force: true });
			}
		},
	);
	it("parses automation and global budget flags without folding them into the objective", () => {
		expect(
			parseResearchControlArgs([
				"run",
				"--automation",
				"full",
				"--max-tasks=80",
				"--max-turns",
				"300",
				"--max-cost-usd",
				"12.5",
				"target",
				"question",
			]),
		).toEqual({
			action: "run",
			objective: "target question",
			automation: "full",
			maxTasks: 80,
			maxTurns: 300,
			maxCostUsd: 12.5,
		});
		expect(() => decodeResearchControl('{"action":"run","automation":"unsafe"}')).toThrow(
			"unknown Astra automation level",
		);
		expect(parseResearchControlArgs(["resume", "--max-cost-usd", "unlimited"])).toEqual({
			action: "resume",
			unlimitedCost: true,
		});
		expect(parseResearchControlArgs(["run", "--require-paper", "embodied VLM adaptation"])).toEqual({
			action: "run",
			requirePaper: true,
			objective: "embodied VLM adaptation",
		});
		expect(parseResearchControlArgs(["resume", "--guidance", "install the required simulator"])).toEqual({
			action: "resume",
			guidance: "install the required simulator",
		});
		expect(() => parseResearchControlArgs(["run", "--guidance", "invalid", "objective"])).toThrow(
			"only valid when resuming",
		);
	});

	it("returns a collaborative user gate instead of exhausting the driver tick limit", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-control-gate-"));
		try {
			const result = await runResearchControl(
				{ action: "run", objective: "collaborative fixture", automation: "collaborative" },
				root,
			);

			expect(supervisorState.ticks).toBe(1);
			expect(result.status?.paused).toBe(true);
			expect(result.status?.userGate).toMatchObject({ kind: "research", stageId: "validation" });
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("drives a paused active job to completion on resume", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-control-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, { objective: "resume fixture", workspaceRoot: root });
			await job.pause("restart boundary");
			await mkdir(join(root, ".astra"), { recursive: true });
			await writeFile(
				join(root, ".astra", "active-job.json"),
				`${JSON.stringify({ jobId: job.state.frame.jobId })}\n`,
			);

			const result = await runResearchControl({ action: "resume" }, root);

			expect(supervisorState.ticks).toBe(1);
			expect(result.status?.paused).toBe(false);
			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			expect(reopened?.state.frame.status).toBe("completed");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("removes an existing cost limit when resume requests unlimited cost", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-control-unlimited-cost-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				objective: "resume without a cost ceiling",
				workspaceRoot: root,
				maxCostUsd: 1,
			});
			await job.recordCost(1.25);
			await job.requireUserGate({
				kind: "budget",
				stageId: "validation",
				limit: "maxCostUsd",
				reason: "cost budget exhausted",
			});
			await mkdir(join(root, ".astra"), { recursive: true });
			await writeFile(
				join(root, ".astra", "active-job.json"),
				`${JSON.stringify({ jobId: job.state.frame.jobId })}\n`,
			);

			const result = await runResearchControl({ action: "resume", unlimitedCost: true }, root);

			expect(result.status?.budget.maxCostUsd).toBeUndefined();
			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			expect(reopened?.state.frame.budget.maxCostUsd).toBeUndefined();
			expect(reopened?.state.frame.status).toBe("completed");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
});
