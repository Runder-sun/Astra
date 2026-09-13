import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { ENV_AGENT_DIR } from "../src/config.ts";

const cliPath = resolve(__dirname, "../src/cli.ts");
const tempDirs: string[] = [];
const providerEnvKeys = [
	"ANTHROPIC_AUTH_TOKEN",
	"ANTHROPIC_OAUTH_TOKEN",
	"ANTHROPIC_API_KEY",
	"ANT_LING_API_KEY",
	"QWEN_TOKEN_PLAN_API_KEY",
	"QWEN_TOKEN_PLAN_CN_API_KEY",
	"OPENAI_API_KEY",
	"AZURE_OPENAI_API_KEY",
	"NVIDIA_API_KEY",
	"DEEPSEEK_API_KEY",
	"GEMINI_API_KEY",
	"GOOGLE_CLOUD_API_KEY",
	"GOOGLE_APPLICATION_CREDENTIALS",
	"GOOGLE_CLOUD_PROJECT",
	"GCLOUD_PROJECT",
	"GOOGLE_CLOUD_LOCATION",
	"GROQ_API_KEY",
	"CEREBRAS_API_KEY",
	"XAI_API_KEY",
	"RADIUS_API_KEY",
	"OPENROUTER_API_KEY",
	"AI_GATEWAY_API_KEY",
	"ZAI_API_KEY",
	"ZAI_CODING_CN_API_KEY",
	"MISTRAL_API_KEY",
	"MINIMAX_API_KEY",
	"MINIMAX_CN_API_KEY",
	"MOONSHOT_API_KEY",
	"HF_TOKEN",
	"FIREWORKS_API_KEY",
	"TOGETHER_API_KEY",
	"BASETEN_API_KEY",
	"OPENCODE_API_KEY",
	"KIMI_API_KEY",
	"CLOUDFLARE_API_KEY",
	"XIAOMI_API_KEY",
	"XIAOMI_TOKEN_PLAN_CN_API_KEY",
	"XIAOMI_TOKEN_PLAN_AMS_API_KEY",
	"XIAOMI_TOKEN_PLAN_SGP_API_KEY",
	"COPILOT_GITHUB_TOKEN",
	"AWS_PROFILE",
	"AWS_ACCESS_KEY_ID",
	"AWS_SECRET_ACCESS_KEY",
	"AWS_BEARER_TOKEN_BEDROCK",
	"AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
	"AWS_CONTAINER_CREDENTIALS_FULL_URI",
	"AWS_WEB_IDENTITY_TOKEN_FILE",
];

interface CliResult {
	stdout: string;
	stderr: string;
	code: number | null;
}

function createTempDir(): string {
	const dir = mkdtempSync(join(tmpdir(), "pi-model-less-control-"));
	tempDirs.push(dir);
	return dir;
}

function cleanProviderEnvironment(root: string): NodeJS.ProcessEnv {
	const env = { ...process.env };
	for (const key of providerEnvKeys) delete env[key];
	env[ENV_AGENT_DIR] = join(root, "agent");
	env.HOME = root;
	env.PI_OFFLINE = "1";
	env.PI_SKIP_VERSION_CHECK = "1";
	return env;
}

async function runControlSession(messages: string[]): Promise<CliResult> {
	const root = createTempDir();
	const projectDir = join(root, "project");
	const sessionDir = join(root, "sessions");
	const extensionPath = join(root, "control-extension.mjs");
	mkdirSync(projectDir, { recursive: true });
	writeFileSync(
		extensionPath,
		[
			"export default function controlExtension(pi) {",
			'  pi.on("session_start", () => {',
			'    pi.appendEntry("control_result", { ok: true });',
			"  });",
			"}",
		].join("\n"),
		"utf8",
	);

	return await new Promise((resolvePromise, reject) => {
		const child = spawn(
			process.execPath,
			[
				cliPath,
				"--offline",
				"--approve",
				"--mode",
				"json",
				"--session-dir",
				sessionDir,
				"--extension",
				extensionPath,
				...messages,
			],
			{
				cwd: projectDir,
				env: cleanProviderEnvironment(root),
				stdio: ["ignore", "pipe", "pipe"],
			},
		);
		let stdout = "";
		let stderr = "";
		child.stdout.on("data", (chunk) => {
			stdout += chunk.toString();
		});
		child.stderr.on("data", (chunk) => {
			stderr += chunk.toString();
		});
		child.on("error", reject);
		child.on("close", (code) => resolvePromise({ stdout, stderr, code }));
	});
}

afterEach(() => {
	for (const dir of tempDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

describe("model-less control sessions", () => {
	it("runs extension-only JSON control and exposes session_start entries", async () => {
		const result = await runControlSession([]);
		const events = result.stdout
			.trim()
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line) as unknown);

		expect(result.code).toBe(0);
		expect(result.stderr).not.toContain("No models available");
		expect(events).toContainEqual(
			expect.objectContaining({
				type: "entry_appended",
				entry: expect.objectContaining({ type: "custom", customType: "control_result", data: { ok: true } }),
			}),
		);
	});

	it("still requires an authenticated model when a JSON prompt is present", async () => {
		const result = await runControlSession(["call the model"]);

		expect(result.code).toBe(1);
		expect(result.stderr).toMatch(/No (models available|API key found)/);
	});
});
