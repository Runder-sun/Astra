import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import type { Static, TSchema } from "typebox";
import { Value } from "typebox/value";
import { vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner, type CodexRunOptions } from "../src/codex-app-server.ts";
import { writeReviewerOutputManifest, writeStagePlanManifest, writeWorkerOutputManifest } from "../src/contracts.ts";
import {
	PiChildSessionRunner,
	PiMainAgentAdapter,
	PiReviewerAdapter,
	PiWorkerAdapter,
} from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ProviderCapacityError } from "../src/supervisor.ts";
import type { StageDefinition, TaskPacket } from "../src/types.ts";
import { validateWorkerSubmission } from "../src/worker-submission.ts";

export const lifecycleDefinition: StageDefinition = {
	id: "validation",
	label: "Offline validation",
	suggestedInputArtifactTypes: [],
	outputArtifactType: "validation",
	requiredOutputFields: ["content"],
	acceptanceChecks: ["verify declared content"],
	failureSignals: ["missing content"],
	workerTaskFamily: "validation",
	workerTools: ["read"],
	workspaceWrite: false,
	minSourceRefs: 0,
	gate: "main-agent",
	qualityPolicy: { minPassingReviews: 1, minScore: 0.8, requireResolvableArtifacts: false },
};

/** Real adapters and fixed manifests; only the model runner is replaced. */
export async function lifecycleScenario(backend: "pi" | "codex", definition = lifecycleDefinition) {
	const root = await mkdtemp(join(tmpdir(), "astra-lifecycle-"));
	const store = new JsonlAstraStore(root);
	let job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "offline lifecycle regression",
		definitions: [definition],
		automation: "full",
		maxTurns: 32,
	});
	const calls: Array<{ role: string; taskId: string }> = [];
	const control: {
		capacityFailures: number;
		reviewerCapacityFailures: number;
		allowPlanning: boolean;
		sources: string[];
		beforeMainReturn?: () => Promise<void>;
	} = { capacityFailures: 0, reviewerCapacityFailures: 0, allowPlanning: false, sources: [] };
	const planned = () => ({
		key: "current",
		objective: "execute current reviewed task",
		deliveryKind: "stage" as const,
		inputArtifactRefs: [],
		requiredOutputFields: definition.requiredOutputFields,
		acceptanceChecks: definition.acceptanceChecks,
		successCriteria: definition.acceptanceChecks,
		failureSignals: definition.failureSignals,
		responsibilityBindings: [],
		responsibilityTransfers: [],
		hypothesis: "offline contract execution",
	});
	const content = (task: TaskPacket) => ({
		...Object.fromEntries(
			task.requiredOutputFields.map((field) => [field, field === "content" ? "offline content" : []]),
		),
		...(["result-to-claim", "research-review"].includes(task.requiredOutputType)
			? { scientificOutcome: "insufficient-evidence", missionCoverage: "insufficient" }
			: {}),
	});
	const assessment = (evidenceId: string, criteria: string[]) => ({
		verdict: "pass" as const,
		score: 1,
		findings: [],
		verifiedRefs: [`evidence:${evidenceId}`],
		criteria: criteria.map((criterion) => ({
			criterion,
			passed: true,
			score: 1,
			evidenceRefs: [`evidence:${evidenceId}`],
			rationale: "offline explicit assessment",
		})),
	});
	const piRunner = new PiChildSessionRunner();
	vi.spyOn(piRunner, "run").mockImplementation(async (cwd, jobId, taskId, _attempt, role, _prompt, env = {}) => {
		calls.push({ role, taskId });
		if (
			(role === "worker" && control.capacityFailures-- > 0) ||
			(role === "reviewer" && control.reviewerCapacityFailures-- > 0) ||
			(role === "main-agent" && !control.allowPlanning)
		)
			return {
				exitCode: 1,
				stdout: "",
				stderr: "offline capacity",
				jsonEvents: [],
				costUsd: 0,
				providerError: { kind: "capacity", message: "offline capacity" },
			};
		if (role === "worker") {
			const task = job.state.tasks[taskId];
			const validated = await validateWorkerSubmission(
				task,
				{
					artifactType: task.requiredOutputType,
					content: content(task),
					refs: control.sources.map((ref) => ({ kind: "source", ref, summary: "offline recorded source" })),
				},
				{ executionRoot: cwd, sessionRef: `pi-session:${taskId}`, job },
			);
			await writeWorkerOutputManifest(
				{
					schemaVersion: "astra.worker_output_manifest.v1",
					manifestId: `manifest_${taskId}`,
					jobId,
					taskId,
					agentId: task.agentId,
					status: "completed",
					artifactType: task.requiredOutputType,
					content: validated.content,
					outputRefs: validated.outputRefs,
					validationStatus: "passed",
					validationErrors: [],
					sessionRef: `pi-session:${taskId}`,
					createdAt: new Date().toISOString(),
				},
				root,
			);
		} else if (role === "reviewer") {
			await writeReviewerOutputManifest(
				{
					schemaVersion: "astra.reviewer_output_manifest.v1",
					manifestId: `review_${taskId}`,
					jobId,
					taskId,
					evidenceId: env.ASTRA_EVIDENCE_ID!,
					...assessment(env.ASTRA_EVIDENCE_ID!, JSON.parse(env.ASTRA_REVIEW_CRITERIA!)),
					sessionRef: `pi-session:${taskId}`,
					createdAt: new Date().toISOString(),
				},
				root,
			);
		} else {
			await writeStagePlanManifest(
				{
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: env.ASTRA_STAGE_PLAN_ID!,
					jobId,
					stageId: definition.id,
					decisionRef: env.ASTRA_DECISION_REF!,
					mode: "decompose",
					tasks: [planned()],
					rationale: "current source contract",
					sessionRef: "pi-session:planner",
					createdAt: new Date().toISOString(),
				},
				root,
			);
		}
		if (role === "main-agent") await control.beforeMainReturn?.();
		return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
	});
	const codexRunner = new CodexAppServerRunner();
	vi.spyOn(codexRunner, "run").mockImplementation(async <S extends TSchema>(options: CodexRunOptions<S>) => {
		const fields = Object.keys((options.schema as { properties?: Record<string, unknown> }).properties ?? {});
		const role = fields.includes("artifactType") ? "worker" : fields.includes("criteria") ? "reviewer" : "main-agent";
		const taskId =
			role === "main-agent" ? basename(options.logPath).replace(/-\d+\.jsonl$/, "") : basename(options.cwd);
		calls.push({ role, taskId });
		if (
			(role === "worker" && control.capacityFailures-- > 0) ||
			(role === "reviewer" && control.reviewerCapacityFailures-- > 0) ||
			(role === "main-agent" && !control.allowPlanning)
		)
			throw new ProviderCapacityError("offline capacity");
		let output: unknown;
		if (role === "worker") {
			const task = job.state.tasks[taskId];
			output = {
				artifactType: task.requiredOutputType,
				contentJson: JSON.stringify(content(task)),
				refs: control.sources.map((ref) => ({ kind: "source", ref, summary: "offline recorded source" })),
			};
		} else if (role === "reviewer") {
			const task = job.state.tasks[taskId];
			output = assessment(
				task.inputArtifactRefs[0],
				JSON.parse(await readFile(join(options.cwd, "review-criteria.json"), "utf8")),
			);
			if (fields.includes("astraValidatedReview")) output = { ...(output as object), astraValidatedReview: null };
		} else output = { tasks: [planned()], rationale: "current source contract" };
		if (!Value.Check(options.schema, output)) throw new Error("offline runner output violates actual adapter schema");
		const threadId = `offline_${calls.length}`;
		await options.onThread(threadId);
		await mkdir(dirname(options.logPath), { recursive: true });
		await writeFile(options.logPath, "");
		if (role === "main-agent") await control.beforeMainReturn?.();
		return { output: output as Static<S>, model: "offline", threadId, sessionFile: options.logPath };
	});
	const codex = new CodexResearchAdapters(codexRunner);
	return {
		root,
		store,
		get job() {
			return job;
		},
		set job(next: ResearchJob) {
			job = next;
		},
		calls,
		control,
		worker: backend === "pi" ? new PiWorkerAdapter(piRunner) : codex,
		reviewer: backend === "pi" ? new PiReviewerAdapter(piRunner) : codex,
		mainAgent: backend === "pi" ? new PiMainAgentAdapter(piRunner, root) : codex,
		async task(objective: string, refs: string[] = []) {
			return job.dispatchTask({
				stageId: definition.id,
				stageExecutionId: definition.id,
				role: "worker",
				objective,
				inputArtifactRefs: refs,
				requiredCanonicalArtifacts: refs.filter((ref) => job.state.canonical[ref]),
				requiredOutputType: definition.outputArtifactType,
				requiredOutputFields: definition.requiredOutputFields,
				acceptanceChecks: definition.acceptanceChecks,
				successCriteria: definition.acceptanceChecks,
				failureSignals: definition.failureSignals,
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: definition.workerTools,
				writeAuthority: definition.workspaceWrite ? "workspace-write" : "none",
				budget: { maxTurns: 4, maxToolCalls: 16, maxRuntimeMs: 10000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
			});
		},
	};
}
