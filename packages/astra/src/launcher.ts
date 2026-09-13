#!/usr/bin/env node

import { realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { main } from "@earendil-works/pi-coding-agent";
import { createAstraExtension } from "./extension.ts";
import { createAstraFixtureProvider, fetchAstraFixtureOpenAlex } from "./fixture-provider.ts";
import {
	encodeResearchControl,
	parseResearchControlArgs,
	researchBackend,
	runResearchControl,
} from "./research-control.ts";

function translateResearchInvocation(args: string[]): string[] | undefined {
	if (args[0] !== "research") return undefined;
	const request = parseResearchControlArgs(args.slice(1));
	return ["--mode", "json", "--astra-research-control", encodeResearchControl(request)];
}

/** Pi owns interactive modes; Codex research control does not start a Pi model session. */
export async function runAstra(args: string[] = process.argv.slice(2)): Promise<void> {
	if (args[0] === "research") {
		const request = parseResearchControlArgs(args.slice(1));
		if ((await researchBackend(request, process.cwd())) === "codex") {
			console.log(JSON.stringify(await runResearchControl(request, process.cwd())));
			return;
		}
	}
	const translated = translateResearchInvocation(args);
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
