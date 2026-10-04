import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { ResearchJob } from "../../src/research.ts";
import { JsonlAstraStore } from "../../src/store.ts";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const request = JSON.parse(Buffer.concat(chunks).toString("utf8"));
const root = process.cwd();
const store = new JsonlAstraStore(root);
const job = request.action === "run"
	? await ResearchJob.create(store, { workspaceRoot: root, objective: request.objective, maxTasks: request.maxTasks })
	: await ResearchJob.open(store, request.jobId);
await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
await store.withExecutionLock(job.state.frame.jobId, "unicode-fixture", async () => {
	if (request.guidance) await job.resumeWithGuidance(request.guidance);
	await job.pause("offline fixture");
	await writeFile(join(root, "captured.json"), JSON.stringify(request));
	if (process.send) await new Promise((resolve, reject) => process.send(
		{ type: "astra/job-published", jobId: job.state.frame.jobId }, (error) => error ? reject(error) : resolve(),
	));
	if (request.objective?.startsWith("output-fragments")) {
		const stdout = Buffer.from("中😀");
		const stderr = Buffer.from("文🚀");
		for (let offset = 0; offset < stdout.length; offset++) {
			process.stdout.write(stdout.subarray(offset, offset + 1));
			await delay(30);
			process.stderr.write(stderr.subarray(offset, offset + 1));
			await delay(30);
		}
	} else if (request.objective?.startsWith("output-tail")) process.stdout.write("😀" + "x".repeat(15999));
	else if (request.objective?.startsWith("output-incomplete")) process.stdout.write(Buffer.from([0xe4, 0xb8]));
});
if (process.connected) process.disconnect();
