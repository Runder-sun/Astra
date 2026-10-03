import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { ResearchJob } from "../../src/research.ts";
import { JsonlAstraStore } from "../../src/store.ts";

let input = "";
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
const root = process.cwd();
const store = new JsonlAstraStore(root);
const mode = process.env.ASTRA_FAKE_WORKBENCH_MODE;
if (mode === "delayed-publication") process.on("SIGINT", () => {});
async function publish(jobId) {
	if (process.send) await new Promise((resolve, reject) => process.send({ type: "astra/job-published", jobId }, error => error ? reject(error) : resolve()));
}
await writeFile(join(root, "fixture-request.json"), JSON.stringify({ request, model: process.env.ASTRA_CODEX_MODEL }));
if (request.action === "run") {
	if (mode === "missing-publication") { if (process.connected) process.disconnect(); process.exit(1); }
	if (mode === "delayed-publication") await new Promise(resolve => setTimeout(resolve, 250));
	const job = await ResearchJob.create(store, { objective: request.objective, workspaceRoot: root, maxTasks: request.maxTasks });
	await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
	await publish(job.state.frame.jobId);
	if (mode === "failed-after-publication") { console.error(`fixture-job-error:${job.state.frame.jobId}`); if (process.connected) process.disconnect(); process.exit(1); }
	await job.pause("模拟任务等待用户输入；不调用模型");
} else {
	const jobId = request.jobId ?? JSON.parse(await readFile(join(root, ".astra/active-job.json"), "utf8")).jobId;
	const job = await ResearchJob.open(store, jobId);
	if (request.action === "pause") await job.pause(request.reason);
	else {
		await job.resume();
		const timer = setInterval(() => {}, 1000);
		process.once("SIGINT", () => { clearInterval(timer); });
	}
}
if (process.connected) process.disconnect();
