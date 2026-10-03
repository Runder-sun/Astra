import { decodeResearchControl, runResearchControl } from "./research-control.ts";

let input = "";
for await (const chunk of process.stdin) input += chunk;
let publication = Promise.resolve();
try {
	console.log(
		JSON.stringify(
			await runResearchControl(decodeResearchControl(input), process.cwd(), (jobId) => {
				if (process.send)
					publication = new Promise<void>((resolve, reject) =>
						process.send!({ type: "astra/job-published", jobId }, (error) => (error ? reject(error) : resolve())),
					);
				void publication.catch(() => {});
			}),
		),
	);
} catch (error) {
	console.error(error instanceof Error ? error.message : String(error));
	process.exitCode = 1;
} finally {
	try {
		await publication;
	} catch (error) {
		console.error(error instanceof Error ? error.message : String(error));
		process.exitCode = 1;
	}
	if (process.connected) process.disconnect();
}
