import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { NonRetryableResearchError } from "../src/supervisor.ts";
import type { JobSnapshot, TaskPacket } from "../src/types.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it.each([10, 30, 60])(
	"prepares %i repair requirements with a constant number of full snapshot copies",
	async (count) => {
		const root = await mkdtemp(join(tmpdir(), "astra-plan-copies-"));
		roots.push(root);
		const store = new MemoryAstraStore();
		const initial = await ResearchJob.create(store, { workspaceRoot: root, objective: "bounded copy cost" });
		const snapshot = initial.state;
		snapshot.tasks.source = {
			id: "source",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verify"],
			successCriteria: [],
			failureSignals: [],
		} as unknown as TaskPacket;
		for (let index = 0; index < count; index++) {
			const id = `obligation_${index}`;
			snapshot.obligations[id] = {
				id,
				sourceReviewId: `review_${index}`,
				status: "open",
				description: "verify",
				createdAt: "now",
				items: [{ id: `issue_${index}`, criterion: "verify", status: "open" }],
			};
			snapshot.reviews[`review_${index}`] = {
				id: `review_${index}`,
				evidenceId: `evidence_${index}`,
				verdict: "fail",
				findings: ["verify"],
				createdAt: "now",
			};
			snapshot.evidence[`evidence_${index}`] = {
				id: `evidence_${index}`,
				taskId: "source",
				currentEvidenceSetId: "lineage",
				stageId: "validation",
				type: "validation",
				status: "rejected",
				refs: [],
				content: { payload: "x".repeat(16000) },
				checksum: "fixture",
				createdAt: "now",
			};
			snapshot.frame.openObligationIds.push(id);
		}
		await store.writeSnapshot(snapshot);
		const job = (await ResearchJob.open(store, snapshot.frame.jobId))!;
		let copies = 0;
		const readState = Object.getOwnPropertyDescriptor(ResearchJob.prototype, "state")!.get as (
			this: ResearchJob,
		) => JobSnapshot;
		Object.defineProperty(job, "state", {
			get: () => {
				copies++;
				return readState.call(job);
			},
		});
		const runner = new CodexAppServerRunner();
		vi.spyOn(runner, "run").mockRejectedValue(new NonRetryableResearchError("fixture stops before model call"));
		const started = performance.now();
		await expect(new CodexResearchAdapters(runner).planStage(job)).rejects.toThrow("fixture stops");
		const elapsedMs = performance.now() - started;
		const requirements = JSON.parse(
			await readFile(
				join(
					root,
					".astra",
					"jobs",
					snapshot.frame.jobId,
					"main-agent",
					"codex-context",
					"repair-requirements.json",
				),
				"utf8",
			),
		);
		expect(Object.keys(requirements)).toHaveLength(count);
		expect(requirements.obligation_0.openRepairChecks).toHaveLength(count);
		expect(requirements.obligation_0.inheritedTask.acceptanceChecks).toEqual([
			"Verify that the reported issue is resolved: verify",
		]);
		console.log(JSON.stringify({ obligations: count, copies, elapsedMs: Math.round(elapsedMs) }));
		expect(copies).toBeLessThanOrEqual(15);
	},
);
