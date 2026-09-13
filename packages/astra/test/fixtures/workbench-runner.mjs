import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { ResearchJob } from "../../src/research.ts";
import { JsonlAstraStore } from "../../src/store.ts";

let input = "";
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
const root = process.cwd();
const store = new JsonlAstraStore(root);
await writeFile(join(root, "fixture-request.json"), JSON.stringify({ request, model: process.env.ASTRA_CODEX_MODEL }));
if (request.action === "run") {
	const job = await ResearchJob.create(store, { objective: request.objective, workspaceRoot: root, maxTasks: request.maxTasks });
	await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
	await job.pause("模拟任务等待用户输入；不调用模型");
} else {
	const { jobId } = JSON.parse(await readFile(join(root, ".astra/active-job.json"), "utf8"));
	const job = await ResearchJob.open(store, jobId);
	if (request.action === "pause") await job.pause(request.reason);
	else {
		await job.resume();
		const timer = setInterval(() => {}, 1000);
		process.once("SIGINT", async () => { await job.pause("模拟运行已中断"); clearInterval(timer); });
	}
}
