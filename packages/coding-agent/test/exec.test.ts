import * as childProcess from "node:child_process";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { execCommand } from "../src/core/exec.ts";

vi.mock("node:child_process", async (importOriginal) => {
	const original = await importOriginal<typeof childProcess>();
	return { ...original, spawn: vi.fn(original.spawn) };
});

function fakeChild() {
	const child = Object.assign(new EventEmitter(), {
		stdin: null,
		stdout: new PassThrough(),
		stderr: new PassThrough(),
		exitCode: null as number | null,
		signalCode: null as NodeJS.Signals | null,
		killed: false,
		kill: vi.fn((_signal: NodeJS.Signals) => true),
	});
	const exit = (code: number | null, signal: NodeJS.Signals | null, close = true) => {
		child.exitCode = code;
		child.signalCode = signal;
		child.emit("exit", code, signal);
		if (close) child.emit("close", code, signal);
	};
	child.kill.mockImplementation((signal) => {
		child.killed = true;
		if (signal === "SIGKILL") exit(null, signal);
		return true;
	});
	vi.mocked(childProcess.spawn).mockReturnValue(child as unknown as ReturnType<typeof childProcess.spawn>);
	return { child, exit };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
	vi.mocked(childProcess.spawn).mockReset();
	vi.restoreAllMocks();
	vi.useRealTimers();
});

it.each(["timeout", "abort", "pre-abort", "simultaneous"])(
	"F05-01/03 escalates %s once when SIGTERM was sent but the process is still alive",
	async (kind) => {
		const { child } = fakeChild();
		const controller = new AbortController();
		if (kind === "pre-abort") controller.abort();
		const running = execCommand("offline", [], "/tmp", { timeout: 25, signal: controller.signal });
		if (kind === "abort" || kind === "simultaneous") controller.abort();
		await vi.advanceTimersByTimeAsync(25);
		expect(child.kill.mock.calls).toEqual([["SIGTERM"]]);
		await vi.advanceTimersByTimeAsync(kind === "timeout" ? 4999 : 4974);
		expect(child.kill.mock.calls).toEqual([["SIGTERM"]]);
		await vi.advanceTimersByTimeAsync(1);
		expect(child.kill.mock.calls).toEqual([["SIGTERM"], ["SIGKILL"]]);
		expect(await running).toMatchObject({ code: 0, killed: true });
		expect(vi.getTimerCount()).toBe(0);
		expect(child.listenerCount("exit")).toBe(0);
		expect(child.listenerCount("error")).toBe(0);
	},
);

it.each(["code", "signal"])(
	"F05-02 clears both timers when the child exits by %s during SIGTERM grace",
	async (kind) => {
		const { child, exit } = fakeChild();
		const controller = new AbortController();
		const running = execCommand("offline", [], "/tmp", { timeout: 1000, signal: controller.signal });
		controller.abort();
		exit(kind === "code" ? 7 : null, kind === "signal" ? "SIGTERM" : null);
		expect(await running).toMatchObject({ code: kind === "code" ? 7 : 0, killed: true });
		expect(vi.getTimerCount()).toBe(0);
		await vi.advanceTimersByTimeAsync(6000);
		expect(child.kill.mock.calls).toEqual([["SIGTERM"]]);
	},
);

it("F05-03 removes timers and cancellation listeners on spawn failure", async () => {
	const { child } = fakeChild();
	const controller = new AbortController();
	const remove = vi.spyOn(controller.signal, "removeEventListener");
	const running = execCommand("missing", [], "/tmp", { timeout: 1000, signal: controller.signal });
	child.emit("error", new Error("spawn ENOENT"));
	expect(await running).toEqual({ stdout: "", stderr: "", code: 1, killed: false });
	expect(vi.getTimerCount()).toBe(0);
	expect(child.listenerCount("exit")).toBe(0);
	expect(child.listenerCount("error")).toBe(0);
	expect(remove).toHaveBeenCalledWith("abort", expect.any(Function));
});

it("F05-02/04 keeps collecting descendant output after a real exit without later cancellation signals", async () => {
	const { child, exit } = fakeChild();
	const controller = new AbortController();
	const running = execCommand("offline", [], "/tmp", { timeout: 50, signal: controller.signal });
	child.stdout.write("head\n");
	child.stderr.write("diagnostic\n");
	exit(3, null, false);
	for (let index = 0; index < 6; index++) {
		await vi.advanceTimersByTimeAsync(50);
		child.stdout.write(`tail-${index}\n`);
	}
	controller.abort();
	await vi.advanceTimersByTimeAsync(100);
	expect(await running).toEqual({
		stdout: `head\n${Array.from({ length: 6 }, (_, index) => `tail-${index}\n`).join("")}`,
		stderr: "diagnostic\n",
		code: 3,
		killed: false,
	});
	expect(child.kill).not.toHaveBeenCalled();
	expect(vi.getTimerCount()).toBe(0);
});

it.runIf(process.platform !== "win32")(
	"F05-01 terminates a real local process whose SIGTERM cleanup outlasts the grace",
	async () => {
		vi.useRealTimers();
		const controller = new AbortController();
		const running = execCommand(
			process.execPath,
			["-e", 'process.on("SIGTERM", () => {}); process.stdout.write("ready\\n"); setInterval(() => {}, 1000);'],
			"/tmp",
			{ signal: controller.signal, timeout: 300 },
		);
		const result = await running;
		expect(result.stdout).toBe("ready\n");
		expect(result.killed).toBe(true);
		expect(result.code).toBe(0);
	},
	10000,
);
