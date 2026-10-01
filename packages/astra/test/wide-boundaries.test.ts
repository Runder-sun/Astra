import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createContext, runInContext } from "node:vm";
import { afterEach, expect, it } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { taskWorkspacePath } from "../src/task-workspace.ts";
import { startWorkbench } from "../src/workbench.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

for (const [ref, declared, expected] of [
	["report.txt", true, 1],
	[".astra/jobs/test/workspaces/worker/report.txt", true, 1],
	["report.txt", false, 1],
	[".astra/jobs/test/workspaces/worker/report.txt", false, 1],
	["https://example.invalid/report.txt", true, 0],
	["pi-session:worker", true, 0],
	["source:https://example.invalid", true, 0],
	["/etc/passwd", true, 0],
	["../other/report.txt", true, 0],
	["file:///etc/passwd", false, 0],
] as const) {
	it(`renders a declared download link for ${ref}`, async () => {
		const elements = new Map<string, ReturnType<typeof element>>();
		function element(tag: string) {
			return {
				tag,
				children: [] as unknown[],
				append(...children: unknown[]) {
					this.children.push(...children);
				},
				replaceChildren(...children: unknown[]) {
					this.children = children;
				},
				focus() {},
			};
		}
		const context = createContext({
			document: {
				documentElement: { dataset: {} },
				getElementById: (id: string) => {
					if (!elements.has(id)) elements.set(id, element("div"));
					return elements.get(id);
				},
				createElement: element,
			},
			fetch: () => new Promise(() => {}),
			setInterval: () => {},
			URLSearchParams,
		});
		const source = await readFile(new URL("../web/app.js", import.meta.url), "utf8");
		runInContext(source, context);
		runInContext(
			`stageId = 'validation'; selected = 'job'; stages = [{id:'validation',acceptanceChecks:[]}]; current = ${JSON.stringify({ snapshot: { evidence: { e: { id: "e", stageId: "validation", type: "validation", taskId: "t", createdAt: "now", refs: [ref], ...(declared ? { files: [{ sourceRef: ref, sha256: "frozen" }] } : {}) } }, tasks: { t: { acceptanceChecks: [], successCriteria: [] } }, reviews: {}, obligations: {}, canonicalRoute: { stageArtifactIds: { validation: "a" } }, canonical: { a: { id: "a", evidenceId: "e", content: {} } } }, milestones: [] })}; renderDetails();`,
			context,
		);
		const descendants = (item: { tag?: string; children?: unknown[] }): unknown[] => [
			item,
			...(item.children ?? []).flatMap((child) =>
				typeof child === "object" && child ? descendants(child as { children?: unknown[] }) : [],
			),
		];
		const links = descendants(elements.get("stage-detail")!).filter((item) => (item as { tag?: string }).tag === "a");
		expect(links).toHaveLength(expected);
	});
}

it("backend serves declared task-relative evidence refs", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-relative-download-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "download declared relative evidence",
	});
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "download",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["write"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["verified"],
	});
	const workspace = taskWorkspacePath(root, task.jobId, task.id);
	await mkdir(workspace, { recursive: true });
	await writeFile(join(workspace, "report.txt"), "reviewed bytes");
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: "validation",
		refs: ["report.txt"],
		content: { content: "verified" },
	});
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
	const artifact = await job.adoptEvidence(evidence.id);
	const crossRef = `.astra/jobs/${task.jobId}/workspaces/another-task/report.txt`;
	const snapshot = job.state;
	snapshot.evidence[evidence.id].refs.push(crossRef, "linked.txt");
	snapshot.evidence[evidence.id].files!.push({ ...evidence.files![0], sourceRef: crossRef });
	await symlink(join(root, "outside.txt"), join(workspace, "linked.txt"));
	await writeFile(join(root, "outside.txt"), "private");
	await store.writeSnapshot(snapshot);
	await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: task.jobId }));
	const app = await startWorkbench({ root: join(root, "workbench"), watch: [root], port: 0 });
	try {
		const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { jobs: Array<{ id: string }> };
		const response = await fetch(
			`${app.url}/api/file?${new URLSearchParams({ id: listing.jobs[0].id, artifact: artifact.id, ref: "report.txt" })}`,
		);
		expect(response.status).toBe(200);
		expect(await response.text()).toBe("reviewed bytes");
		for (const ref of [
			"https://example.invalid/report.txt",
			"pi-session:worker",
			"source:https://example.invalid",
			"file:///etc/passwd",
			"/etc/passwd",
			"../report.txt",
			crossRef,
			"linked.txt",
		])
			expect(
				(
					await fetch(
						`${app.url}/api/file?${new URLSearchParams({ id: listing.jobs[0].id, artifact: artifact.id, ref })}`,
					)
				).status,
			).toBe(400);
	} finally {
		await new Promise<void>((resolve, reject) => app.server.close((error) => (error ? reject(error) : resolve())));
	}
});

it("serialized independent job writers retain earlier committed budget usage", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-stale-write-"));
	roots.push(root);
	const first = await ResearchJob.create(new JsonlAstraStore(root), {
		jobId: "job_stale",
		workspaceRoot: root,
		objective: "count durable usage",
	});
	const second = (await ResearchJob.open(new JsonlAstraStore(root), "job_stale"))!;
	await first.consumeTurns(1);
	await expect(second.consumeTurns(1)).rejects.toThrow(/stale/i);
	await second.reload();
	await second.consumeTurns(1);
	const reopened = (await ResearchJob.open(new JsonlAstraStore(root), "job_stale"))!;
	expect(reopened.state.budgetUsage?.turnsUsed).toBe(2);
});
