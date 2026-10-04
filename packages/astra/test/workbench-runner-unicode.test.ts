import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";

const cases = (["run", "resume"] as const).flatMap((action) =>
	(
		[
			["中", 1],
			["中", 2],
			["😀", 1],
			["😀", 2],
			["😀", 3],
			["ASCII", 2],
		] as const
	).map(([marker, offset]) => ({ action, marker, offset })),
);
it.each(cases)(
	"actual runner $action preserves stdin split at $marker/$offset without model calls",
	async ({ action, marker, offset }) => {
		const root = await mkdtemp(join(tmpdir(), "astra-runner-unicode-"));
		try {
			const store = new JsonlAstraStore(root);
			const text = "ASCII中文研究目标😀评估任务因果后果";
			let jobId: string | undefined;
			if (action === "resume") {
				const seed = await ResearchJob.create(store, {
					workspaceRoot: root,
					objective: "offline seed",
					automation: "full",
				});
				await seed.pause("fixture");
				jobId = seed.state.frame.jobId;
			}
			const bytes = Buffer.from(
				JSON.stringify(
					action === "run"
						? { action, backend: "pi", objective: text, maxTasks: 4 }
						: { action, backend: "pi", jobId, guidance: text },
				),
			);
			const child = spawn(
				process.execPath,
				[
					"--import",
					new URL("./fixtures/stdin-observer.mjs", import.meta.url).href,
					fileURLToPath(new URL("../src/workbench-runner.ts", import.meta.url)),
				],
				{
					cwd: root,
					env: { ...process.env, ASTRA_MAX_TICKS: "0", ASTRA_FIXTURE_PROVIDER: "1" },
					stdio: ["pipe", "pipe", "pipe", "ipc"],
				},
			);
			const consumed = new Promise<void>((resolve) =>
				child.on("message", (message: unknown) => {
					if (message && typeof message === "object" && "type" in message && message.type === "stdin-chunk")
						resolve();
				}),
			);
			const closed = new Promise<number | null>((resolve, reject) => {
				child.on("close", resolve);
				child.on("error", reject);
			});
			child.stdout!.resume();
			child.stderr!.setEncoding("utf8");
			let errors = "";
			child.stderr!.on("data", (chunk: string) => {
				errors += chunk;
			});
			const split = bytes.indexOf(Buffer.from(marker)) + offset;
			child.stdin!.write(bytes.subarray(0, split));
			await consumed;
			child.stdin!.end(bytes.subarray(split));
			expect(await closed).toBe(1);
			expect(errors).toContain("research run exceeded 0 ticks");
			jobId ??= (JSON.parse(await readFile(join(root, ".astra/active-job.json"), "utf8")) as { jobId: string })
				.jobId;
			const job = (await ResearchJob.open(store, jobId))!;
			expect(
				action === "run"
					? job.state.frame.objective
					: Object.values(job.state.graph.nodes).find((node) => node.actor === "user")?.statement,
			).toBe(action === "run" ? text : `User guidance: ${text}`);
			expect(job.status().budget).toMatchObject({ tasksUsed: 0, turnsUsed: 0 });
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	},
);
