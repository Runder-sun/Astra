import { mkdtemp, readFile, rm } from "node:fs/promises";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it } from "vitest";
import { startWorkbench } from "../src/workbench.ts";

const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => {
	for (const close of cleanup.splice(0).reverse()) await close();
});

async function setup() {
	const root = await mkdtemp(join(tmpdir(), "astra-unicode-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	const app = await startWorkbench({
		root,
		port: 0,
		runnerPath: fileURLToPath(new URL("./fixtures/workbench-unicode.mjs", import.meta.url)),
	});
	cleanup.push(async () => {
		for (const state of app.running.values()) state.child?.kill("SIGINT");
		await new Promise<void>((resolve) => app.server.close(() => resolve()));
	});
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { token: string };
	const headers = { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" };
	async function post(path: string, body: Buffer, split: number) {
		return new Promise<{ status: number; body: string }>((resolve, reject) => {
			const req = request(app.url + path, { method: "POST", headers }, (res) => {
				res.setEncoding("utf8");
				let output = "";
				res.on("data", (chunk: string) => {
					output += chunk;
				});
				res.on("end", () => resolve({ status: res.statusCode!, body: output }));
			});
			req.on("error", reject);
			void (async () => {
				req.write(body.subarray(0, split));
				await delay(30);
				req.end(body.subarray(split));
			})();
		});
	}
	async function get(id: string) {
		return (await (await fetch(`${app.url}/api/job?id=${id}`)).json()) as {
			root: string;
			output: string;
			snapshot: { frame: { jobId: string; objective: string } };
		};
	}
	return { app, headers, post, get };
}

it.each([1, 2, 3])("HTTP run and resume preserve split Chinese and emoji (offset=%s)", async (offset) => {
	const f = await setup();
	const objective = "研究目标中文😀及范围";
	const body = Buffer.from(JSON.stringify({ objective, maxTasks: 4, requirePaper: false }));
	const response = await f.post("/api/run", body, body.indexOf(Buffer.from("😀")) + offset);
	expect(response.status).toBe(202);
	const { id } = JSON.parse(response.body) as { id: string };
	await expect.poll(() => Boolean(f.app.running.get(id)?.child)).toBe(false);
	const created = await f.get(id);
	expect(created.snapshot.frame.objective).toBe(objective);
	const guidance = "继续指导中文😀";
	const resume = Buffer.from(JSON.stringify({ jobId: created.snapshot.frame.jobId, guidance }));
	expect((await f.post(`/api/resume?id=${id}`, resume, resume.indexOf(Buffer.from("中")) + 1)).status).toBe(202);
	await expect.poll(() => Boolean(f.app.running.get(id)?.child)).toBe(false);
	expect(JSON.parse(await readFile(join(created.root, "captured.json"), "utf8"))).toMatchObject({
		guidance,
		jobId: created.snapshot.frame.jobId,
	});
});

it.each((["run", "resume"] as const).flatMap((action) => [20000, 20001].map((bytes) => ({ action, bytes }))))(
	"HTTP $action limits raw bytes at $bytes despite a split character",
	async ({ action, bytes }) => {
		const f = await setup();
		let path = "/api/run";
		let input: Record<string, unknown> = { objective: "中".repeat(6000), maxTasks: 4, requirePaper: false };
		if (action === "resume") {
			const seed = await f.post(
				path,
				Buffer.from(JSON.stringify({ objective: "ASCII offline seed", maxTasks: 4, requirePaper: false })),
				5,
			);
			const { id } = JSON.parse(seed.body) as { id: string };
			await expect.poll(() => Boolean(f.app.running.get(id)?.child)).toBe(false);
			path = `/api/resume?id=${id}`;
			input = { jobId: (await f.get(id)).snapshot.frame.jobId, guidance: "中".repeat(6000) };
		}
		const base = JSON.stringify(input);
		const body = Buffer.from(base + " ".repeat(bytes - Buffer.byteLength(base)));
		expect(body.length).toBe(bytes);
		expect((await f.post(path, body, body.indexOf(Buffer.from("中")) + 1)).status).toBe(bytes === 20000 ? 202 : 400);
		if (bytes === 20000)
			await expect.poll(() => [...f.app.running.values()].some((state) => Boolean(state.child))).toBe(false);
	},
);

it.each(["fragments", "tail", "incomplete"])(
	"decodes independent pipes and persists a safe bounded tail (%s)",
	async (mode) => {
		const f = await setup();
		const response = await fetch(`${f.app.url}/api/run`, {
			method: "POST",
			headers: f.headers,
			body: JSON.stringify({ objective: `output-${mode} offline test`, maxTasks: 4, requirePaper: false }),
		});
		const { id } = (await response.json()) as { id: string };
		await expect.poll(() => Boolean(f.app.running.get(id)?.child)).toBe(false);
		const live = await f.get(id);
		if (mode === "fragments") {
			expect(live.output).toContain("中");
			expect(live.output).toContain("文");
			expect(live.output).toContain("😀");
			expect(live.output).toContain("🚀");
			expect(live.output).not.toContain("�");
		} else expect(live.output).toBe(mode === "tail" ? "x".repeat(15999) : "�");
		expect(live.output.length).toBeLessThanOrEqual(16000);
		expect(
			await readFile(join(live.root, ".astra/jobs", live.snapshot.frame.jobId, "workbench-output.log"), "utf8"),
		).toBe(live.output);
		f.app.running.delete(id);
		expect((await f.get(id)).output).toBe(live.output);
	},
);
