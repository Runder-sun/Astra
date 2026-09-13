import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { TaskPacket } from "../src/types.ts";
import { validateWorkerSubmission } from "../src/worker-submission.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

function packet(root: string): TaskPacket {
	return {
		schemaVersion: "astra.task_packet.v1",
		id: "task_submission",
		jobId: "job_submission",
		agentId: "worker_submission",
		stageId: "implement-solution",
		stageExecutionId: "stage_exec_implementation",
		role: "worker",
		runnerKind: "pi-session",
		objective: "implement and test the method",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "implement-solution",
		requiredOutputFields: ["implementation", "files", "tests", "commands", "limitations"],
		acceptanceChecks: ["implementation is runnable"],
		failureSignals: ["missing test evidence"],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read", "write", "edit"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
		outputManifestRequired: true,
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["implementation is runnable"],
		replayKey: "submission",
		attempt: 1,
		status: "running",
		createdAt: new Date().toISOString(),
	};
}

describe("worker submission validation", () => {
	it.each(["result-to-claim", "research-review"])(
		"rejects unparseable outcome labels before reviewing %s",
		async (artifactType) => {
			const root = await mkdtemp(join(tmpdir(), "astra-submission-outcomes-"));
			tempRoots.push(root);
			const task = {
				...packet(root),
				stageId: artifactType,
				requiredOutputType: artifactType,
				requiredOutputFields: ["scientificOutcome", "missionCoverage", "conclusion"],
			};
			const options = { executionRoot: root, sessionRef: "codex-session:test" };
			for (const labels of [
				{ scientificOutcome: "supported: observations from fixed experiments", missionCoverage: "sufficient" },
				{ scientificOutcome: "supported", missionCoverage: "sufficient: experiments complete" },
				{ scientificOutcome: { status: "supported" }, missionCoverage: "sufficient" },
				{ scientificOutcome: "pending", missionCoverage: "sufficient" },
			]) {
				await expect(
					validateWorkerSubmission(
						task,
						{
							artifactType,
							content: { ...labels, conclusion: "Evidence and scope are explained here" },
							refs: [],
						},
						options,
					),
				).rejects.toThrow("machine-readable outcome labels");
			}
			for (const scientificOutcome of [
				"supported",
				"partially-supported",
				"refuted",
				"inconclusive",
				"insufficient-evidence",
			]) {
				const content = {
					scientificOutcome,
					missionCoverage: "insufficient",
					conclusion: "Preserve the observed outcome and uncovered requirements",
				};
				await expect(
					validateWorkerSubmission(task, { artifactType, content, refs: [] }, options),
				).resolves.toMatchObject({ content });
			}
		},
	);
	it("counts distinct literature sources rather than repeated citations", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-duplicate-"));
		tempRoots.push(root);
		await expect(
			validateWorkerSubmission(
				{ ...packet(root), requiredOutputFields: ["content"] },
				{
					artifactType: "implement-solution",
					content: { content: "same source repeated" },
					refs: Array.from({ length: 3 }, () => ({ kind: "source", ref: "openalex:W1", summary: "one paper" })),
				},
				{ executionRoot: root, sessionRef: "codex-session:test", minSourceRefs: 3 },
			),
		).rejects.toThrow("received 1");
	});

	it.skipIf(process.platform === "win32")("rejects files reached through an escaping parent symlink", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-parent-link-"));
		tempRoots.push(root);
		const workspace = join(root, "workspace");
		await mkdir(workspace);
		await writeFile(join(root, "secret.txt"), "private");
		await symlink(root, join(workspace, "escape"));
		await expect(
			validateWorkerSubmission(
				{ ...packet(workspace), requiredOutputFields: ["content"] },
				{
					artifactType: "implement-solution",
					content: { content: "outside file" },
					refs: [{ kind: "artifact", ref: "escape/secret.txt", summary: "invalid" }],
				},
				{ executionRoot: workspace, sessionRef: "codex-session:test" },
			),
		).rejects.toThrow(/outside|symbolic/);
	});
	it("rejects the wrong output type and missing required fields", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-"));
		tempRoots.push(root);
		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "literature",
					content: {},
					refs: [],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("required output type");
		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "implement-solution",
					content: { implementation: "partial" },
					refs: [],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("missing required output fields");
	});

	it("normalizes local artifact refs and computes their checksum", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-files-"));
		tempRoots.push(root);
		await writeFile(join(root, "implementation.ts"), "export const answer = 42;\n", "utf8");
		const validated = await validateWorkerSubmission(
			packet(root),
			{
				artifactType: "implement-solution",
				content: {
					implementation: "implemented",
					files: ["implementation.ts"],
					tests: ["static check"],
					commands: ["node implementation.ts"],
					limitations: [],
				},
				refs: [{ kind: "artifact", ref: "implementation.ts", summary: "implementation source" }],
			},
			{ executionRoot: root, sessionRef: "pi-session:test" },
		);

		expect(validated.outputRefs[0]).toMatchObject({ kind: "artifact", ref: "implementation.ts" });
		expect(validated.outputRefs[0]?.sha256).toMatch(/^[a-f0-9]{64}$/);
		expect(validated.outputRefs.at(-1)).toMatchObject({ kind: "session", ref: "pi-session:test" });
	});

	it("resolves declared canonical IDs to their materialized snapshots without accepting unknown IDs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-canonical-"));
		tempRoots.push(root);
		await mkdir(join(root, "canonical"));
		await writeFile(join(root, "canonical", "artifact_input.json"), '{"content":{"result":42}}\n');
		await writeFile(join(root, "canonical", "artifact_unrelated.json"), '{"content":{"result":0}}\n');
		const task = {
			...packet(root),
			inputArtifactRefs: ["artifact_input", "artifact_missing"],
			requiredCanonicalArtifacts: ["artifact_input", "artifact_missing", "artifact_unrelated"],
			requiredOutputFields: ["content"],
		};
		const submit = (ref: string) =>
			validateWorkerSubmission(
				task,
				{
					artifactType: task.requiredOutputType,
					content: { content: "Uses the declared upstream result" },
					refs: [{ kind: "artifact", ref, summary: "upstream evidence" }],
				},
				{ executionRoot: root, sessionRef: "codex-session:test" },
			);
		const byId = await submit("artifact_input");
		const byPath = await submit("canonical/artifact_input.json");
		expect(byId.outputRefs).toEqual(byPath.outputRefs);
		expect(byId.outputRefs[0]).toMatchObject({ ref: "canonical/artifact_input.json", sha256: expect.any(String) });
		await expect(submit("artifact_unrelated")).rejects.toThrow("ENOENT");
		await expect(submit("artifact_missing")).rejects.toThrow("ENOENT");
	});

	it("requires paper manuscript paths to have a matching artifact ref", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-manuscript-"));
		tempRoots.push(root);
		await writeFile(join(root, "paper-manuscript.md"), "# Evidence-bound paper\n", "utf8");
		const paperPacket: TaskPacket = {
			...packet(root),
			stageId: "paper-write",
			requiredOutputType: "paper-write",
			requiredOutputFields: ["manuscript", "sections", "citations", "claimBindings", "limitations"],
		};
		const submission = {
			artifactType: "paper-write",
			content: {
				manuscript: "paper-manuscript.md",
				sections: [],
				citations: [],
				claimBindings: [],
				limitations: [],
			},
			refs: [],
		};

		await expect(
			validateWorkerSubmission(paperPacket, submission, {
				executionRoot: root,
				sessionRef: "pi-session:test",
			}),
		).rejects.toThrow("manuscript path must have a matching artifact ref");

		await expect(
			validateWorkerSubmission(
				paperPacket,
				{
					...submission,
					refs: [{ kind: "artifact", ref: "paper-manuscript.md", summary: "complete manuscript" }],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).resolves.toMatchObject({
			outputRefs: [
				{ kind: "artifact", ref: "paper-manuscript.md" },
				{ kind: "session", ref: "pi-session:test" },
			],
		});
	});

	it("requires compiled paper paths to have a matching artifact ref", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-compiled-paper-"));
		tempRoots.push(root);
		await writeFile(join(root, "paper-manuscript.md"), "# Compiled paper\n", "utf8");
		await expect(
			validateWorkerSubmission(
				{
					...packet(root),
					stageId: "paper-compile",
					requiredOutputType: "paper-compile",
					requiredOutputFields: ["artifact", "command", "buildLog", "validation", "remainingWarnings"],
				},
				{
					artifactType: "paper-compile",
					content: {
						artifact: "paper-manuscript.md",
						command: "compile",
						buildLog: [],
						validation: { status: "passed" },
						remainingWarnings: [],
					},
					refs: [],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("paper-compile artifact path must have a matching artifact ref");
	});

	it("adds the authoritative session ref when structured output has no external refs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-session-only-"));
		tempRoots.push(root);
		const validated = await validateWorkerSubmission(
			packet(root),
			{
				artifactType: "implement-solution",
				content: {
					implementation: "structured result",
					files: [],
					tests: [],
					commands: [],
					limitations: ["no files required by this test"],
				},
				refs: [],
			},
			{ executionRoot: root, sessionRef: "pi-session:test" },
		);

		expect(validated.outputRefs).toEqual([{ kind: "session", ref: "pi-session:test", summary: "Pi worker session" }]);
	});

	it("rejects artifact paths that escape the isolated task workspace", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-escape-"));
		tempRoots.push(root);
		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "implement-solution",
					content: {
						implementation: "invalid",
						files: ["../secret"],
						tests: [],
						commands: [],
						limitations: [],
					},
					refs: [{ kind: "artifact", ref: "../secret", summary: "escaped path" }],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("outside the task workspace");
	});

	it.skipIf(process.platform === "win32")("rejects artifact symlinks that escape the workspace", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-symlink-"));
		tempRoots.push(root);
		const outside = join(root, "..", `${root.split("/").at(-1)}-secret.txt`);
		await writeFile(outside, "secret\n", "utf8");
		tempRoots.push(outside);
		await symlink(outside, join(root, "result.txt"));

		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "implement-solution",
					content: {
						implementation: "invalid",
						files: ["result.txt"],
						tests: [],
						commands: [],
						limitations: [],
					},
					refs: [{ kind: "artifact", ref: "result.txt", summary: "symlinked output" }],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("symbolic links");
	});
});
