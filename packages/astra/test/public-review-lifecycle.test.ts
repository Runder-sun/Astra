import { rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext, ExtensionEvent, ToolDefinition } from "@earendil-works/pi-coding-agent";
import { Value } from "typebox/value";
import { afterEach, expect, it, vi } from "vitest";
import { createAstraExtension } from "../src/extension.ts";
import { ResearchJob } from "../src/research.ts";
import { lifecycleScenario } from "./lifecycle-fixture.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function scenario() {
	const f = await lifecycleScenario("pi");
	roots.push(f.root);
	const task = await f.task("public complete assessment");
	await f.job.setTaskStatus(task.id, "succeeded");
	const evidence = await f.job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: "declared public assessment target" },
		refs: [],
	});
	const tools = new Map<string, ToolDefinition>();
	const handlers = new Map<string, (event: ExtensionEvent, ctx: ExtensionContext) => Promise<unknown>>();
	const api = {
		appendEntry: vi.fn(),
		registerCommand: vi.fn(),
		registerFlag: vi.fn(),
		sendMessage: vi.fn(),
		getFlag: vi.fn(() => undefined),
		registerTool(tool: ToolDefinition) {
			tools.set(tool.name, tool);
		},
		on(name: string, handler: (event: ExtensionEvent, ctx: ExtensionContext) => Promise<unknown>) {
			handlers.set(name, handler);
		},
	} as unknown as ExtensionAPI;
	const ctx = {
		cwd: f.root,
		hasUI: false,
		ui: { notify: vi.fn(), setStatus: vi.fn(), setWidget: vi.fn() },
	} as unknown as ExtensionContext;
	vi.stubEnv("ASTRA_ROLE", "main-agent");
	vi.stubEnv("ASTRA_PROJECT_ROOT", f.root);
	createAstraExtension({ role: "main-agent", jobId: task.jobId })(api);
	return { ...f, task, evidence, tools, handlers, ctx };
}

it.each(["pass", "fail", "partial", "blocked"] as const)(
	"U01/U03 public %s assessment follows real schema/tool permission/execute and persists exact fields",
	async (verdict) => {
		const f = await scenario();
		const review = reviewFixture(f.job, {
			evidenceId: f.evidence.id,
			verdict,
			findings: verdict === "pass" ? [] : ["declared requirement not met"],
		});
		const params = {
			evidenceId: review.evidenceId,
			verdict: review.verdict,
			findings: review.findings,
			score: review.score,
			criteria: review.criteria,
			verifiedRefs: review.verifiedRefs,
			targetVersionHash: f.evidence.versionHash,
		};
		const tool = f.tools.get("research_review")!;
		expect(Value.Check(tool.parameters, params)).toBe(true);
		expect(
			await f.handlers.get("tool_call")!(
				{ type: "tool_call", toolCallId: "public", toolName: tool.name, input: params },
				f.ctx,
			),
		).toBeUndefined();
		const response = await tool.execute("public", params, undefined, undefined, f.ctx);
		expect(response.details).toMatchObject(params);
		const reopened = (await ResearchJob.open(f.store, f.task.jobId))!;
		expect(Object.values(reopened.state.reviews)).toHaveLength(1);
		expect(Object.values(reopened.state.reviews)[0]).toMatchObject(params);
		expect(Object.values(reopened.state.reviews)[0].reviewerTaskId).toBeUndefined();
	},
);

it.each(["verified", "criterion", "unbound-packet"])(
	"U02 first raw complete assessment rejects %s foreign refs with zero events",
	async (field) => {
		const f = await scenario();
		const review = reviewFixture(f.job, { evidenceId: f.evidence.id, verdict: "pass", findings: [] });
		const foreign = field === "unbound-packet" ? "review-packet.json" : "evidence:foreign_job_evidence";
		if (field === "criterion") review.criteria[0].evidenceRefs = [foreign];
		else review.verifiedRefs = [foreign];
		const before = f.job.state;
		await expect(f.job.recordReview(review)).rejects.toThrow(/reference|refs|declared|packet/);
		expect(f.job.state.eventSeq).toBe(before.eventSeq);
		expect(Object.values(f.job.state.reviews)).toHaveLength(0);
	},
);

it("U02 incomplete and wrong-version public assessments reject without registration", async () => {
	const f = await scenario();
	const tool = f.tools.get("research_review")!;
	const complete = reviewFixture(f.job, { evidenceId: f.evidence.id, verdict: "pass", findings: [] });
	for (const key of ["criteria", "verifiedRefs"]) {
		const input = { ...complete } as Record<string, unknown>;
		delete input[key];
		expect(Value.Check(tool.parameters, input)).toBe(false);
		await expect(f.job.recordReview(input as typeof complete)).rejects.toThrow(/explicit criteria/);
	}
	await expect(
		tool.execute("wrong", { ...complete, targetVersionHash: "f".repeat(64) }, undefined, undefined, f.ctx),
	).rejects.toThrow(/version/);
	expect(Object.values((await ResearchJob.open(f.store, f.task.jobId))!.state.reviews)).toHaveLength(0);
});

it.each(["pi", "codex"] as const)(
	"U02 %s authorizes only generated and target-bound auxiliary review files",
	async (backend) => {
		for (const ref of ["review-criteria.json", "review-target-evidence.json"]) {
			const f = await lifecycleScenario(backend);
			roots.push(f.root);
			const task = await f.task("bound review materials");
			await f.job.setTaskStatus(task.id, "succeeded");
			const evidence = await f.job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: task.requiredOutputType,
				content: { content: "bound target" },
				refs: [],
			});
			const result = await f.reviewer.review(evidence, f.job);
			const input = reviewFixture(f.job, { ...result, evidenceId: evidence.id, verifiedRefs: [ref] });
			input.criteria[0].evidenceRefs = [ref];
			const before = f.job.state.eventSeq;
			if (backend === "pi") {
				await expect(f.job.recordReview(input)).rejects.toThrow(/reference|refs|declared|packet/);
				expect(f.job.state.eventSeq).toBe(before);
			} else {
				await f.job.recordReview(input);
				expect(Object.values((await ResearchJob.open(f.store, task.jobId))!.state.reviews)).toHaveLength(1);
			}
		}
	},
);

it.each(["review-criteria.json", "review-target-evidence.json"] as const)(
	"U02 rejects an existing but foreign bound %s",
	async (ref) => {
		const f = await lifecycleScenario("codex");
		roots.push(f.root);
		const task = await f.task("foreign auxiliary binding control");
		await f.job.setTaskStatus(task.id, "succeeded");
		const evidence = await f.job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { content: "original bound target" },
			refs: [],
		});
		const result = await f.reviewer.review(evidence, f.job);
		await writeFile(
			join(f.root, ".astra", "jobs", task.jobId, "tasks", result.reviewerTaskId!, ref),
			JSON.stringify(ref === "review-criteria.json" ? ["foreign criterion"] : { id: "foreign_evidence" }),
		);
		const input = reviewFixture(f.job, { ...result, evidenceId: evidence.id, verifiedRefs: [ref] });
		input.criteria[0].evidenceRefs = [ref];
		const seq = f.job.state.eventSeq;
		await expect(f.job.recordReview(input)).rejects.toThrow(/reference|refs|declared|packet/);
		expect(f.job.state.eventSeq).toBe(seq);
		expect(Object.values(f.job.state.reviews)).toHaveLength(0);
	},
);
