import { setTimeout as delay } from "node:timers/promises";

if (!process.argv.includes("app-server")) {
	process.stdin.setEncoding("utf8");
	for await (const _chunk of process.stdin) {}
}
const mode = process.env.ASTRA_PRODUCER_MODE;
const channel = process.env.ASTRA_PRODUCER_CHANNEL === "stdout" ? process.stdout : process.stderr;
const limit = Number(process.env.ASTRA_PRODUCER_LIMIT ?? "10");
if (mode === "failure") {
	process.stdout.write("😀" + "x".repeat(4095));
	process.stderr.write("😀" + "x".repeat(4095));
} else if (mode === "eof") channel.write(Buffer.from([0xe4, 0xb8]));
else {
	const value = mode === "ascii" ? "ASCII diagnostic" : mode === "cut" || mode === "default"
		? "😀" + "x".repeat(limit - 1) : mode === "pair" ? "😀" + "x".repeat(limit - 2) : "中文😀🚀";
	const bytes = Buffer.from(value);
	if (mode === "split") for (let offset = 0; offset < bytes.length; offset++) {
		channel.write(bytes.subarray(offset, offset + 1));
		await delay(10);
	} else channel.write(bytes);
}
process.exitCode = 1;
