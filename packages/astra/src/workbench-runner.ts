import { decodeResearchControl, runResearchControl } from "./research-control.ts";

let input = "";
for await (const chunk of process.stdin) input += chunk;
try {
	console.log(JSON.stringify(await runResearchControl(decodeResearchControl(input), process.cwd())));
} catch (error) {
	console.error(error instanceof Error ? error.message : String(error));
	process.exitCode = 1;
}
