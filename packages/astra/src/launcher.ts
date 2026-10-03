#!/usr/bin/env node

import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { main } from "@earendil-works/pi-coding-agent";
import { createAstraExtension } from "./extension.ts";
import { createAstraFixtureProvider, fetchAstraFixtureOpenAlex } from "./fixture-provider.ts";
import {
	encodeResearchControl,
	parseResearchControlArgs,
	researchBackend,
	resolveResearchControlTarget,
	runResearchControl,
} from "./research-control.ts";

/** Pi owns interactive modes; Codex research control does not start a Pi model session. */
export async function runAstra(args: string[] = process.argv.slice(2)): Promise<void> {
	let translated: string[] | undefined;
	if (args[0] === "research") {
		const cwd = resolve(process.env.ASTRA_PROJECT_ROOT ?? process.cwd());
		const request = await resolveResearchControlTarget(parseResearchControlArgs(args.slice(1)), cwd);
		if ((await researchBackend(request, cwd)) === "codex") {
			console.log(JSON.stringify(await runResearchControl(request, cwd)));
			return;
		}
		translated = ["--mode", "json", "--astra-research-control", encodeResearchControl(request)];
	}
	const fixtureProvider = process.env.ASTRA_FIXTURE_PROVIDER === "1";
	await main(translated ?? args, {
		extensionFactories: [
			createAstraExtension({ literatureFetcher: fixtureProvider ? fetchAstraFixtureOpenAlex : undefined }),
			...(fixtureProvider ? [createAstraFixtureProvider()] : []),
		],
	});
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
	await runAstra();
}
