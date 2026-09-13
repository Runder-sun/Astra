import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { type Static, type TSchema, Type } from "typebox";
import type { CodexAppServerRunner, CodexRunOptions, CodexRunResult, CodexTool } from "./codex-app-server.ts";
import {
	codexAdoptionSchema,
	codexEvidenceDecisionSchema,
	codexPlanSchema,
	codexReviewSchema,
	codexRouteSchema,
	codexSearchSchema,
	codexWorkerSchema,
} from "./codex-schemas.ts";
import { recordCodexWebSources } from "./codex-web-sources.ts";
import {
	atomicWriteJson,
	taskDir,
	writeMainDecisionManifest,
	writeReviewerOutputManifest,
	writeReviewPacket,
	writeReviewSnapshot,
	writeReviewTrace,
	writeStagePlanManifest,
	writeTaskPacket,
	writeWorkerOutputManifest,
} from "./contracts.ts";
import { readSourceRecord } from "./literature.ts";
import { searchLiterature } from "./literature-search.ts";
import { loadStageSkills } from "./memory.ts";
import { checksum, type ResearchJob } from "./research.ts";
import { validateReviewAssessment } from "./review-validation.ts";
import {
	NonRetryableResearchError,
	ProviderCapacityError,
	type ResearchMainAgentAdapter,
	type ResearchReviewerAdapter,
	type ResearchWorkerAdapter,
	type ReviewerRunResult,
	type WorkerRunResult,
} from "./supervisor.ts";
import {
	prepareReviewEvidenceBundle,
	prepareTaskWorkspace,
	taskInputResources,
	taskResourcePath,
} from "./task-workspace.ts";
import type {
	CandidateEvaluation,
	ChildSessionRecord,
	Evidence,
	MainAgentDecisionManifest,
	Obligation,
	SearchBatch,
	StagePlanManifest,
	TaskPacket,
} from "./types.ts";
import { validateWorkerSubmission } from "./worker-submission.ts";

const submissionInstructions =
	"Astra owns research state and dispatch. Do not launch additional agents or alter parent directories. The full stage and role skill text is included above; do not search for separate skill files. Packaged skills describe the research contract; their astra_submit_* instructions mean return the requested final structured JSON in this backend. The host validates and records the submission. Do not claim success without observable evidence.";

/** The three roles share session bookkeeping, not conversation history. */
export class CodexResearchAdapters implements ResearchWorkerAdapter, ResearchReviewerAdapter, ResearchMainAgentAdapter {
	private readonly runner: CodexAppServerRunner;

	constructor(runner: CodexAppServerRunner) {
		this.runner = runner;
	}

	private async execute<S extends TSchema, T>(
		job: ResearchJob,
		role: ChildSessionRecord["role"],
		taskId: string,
		attempt: number,
		options: Omit<CodexRunOptions<S>, "onThread" | "logPath" | "threadId">,
		consume: (result: CodexRunResult<Static<S>>) => Promise<{ value: T; manifestRef: string }>,
	): Promise<T> {
		const previous = Object.values(job.state.sessions).find(
			(session) => session.role === role && session.taskId === taskId && session.status === "interrupted",
		);
		let sessionId =
			role === "main-agent" ? job.state.sessions[job.state.mainAgentSessionId]?.sessionId : previous?.sessionId;
		if (sessionId?.startsWith("codex-pending-")) sessionId = undefined;
		const logPath = join(
			job.state.frame.permissions.workspaceRoot,
			".astra",
			"jobs",
			job.state.frame.jobId,
			"codex-events",
			`${taskId}-${attempt}.jsonl`,
		);
		try {
			const result = await this.runner.run({
				...options,
				threadId: sessionId,
				logPath,
				model: process.env.ASTRA_CODEX_MODEL,
				onThread: async (id) => {
					sessionId = id;
					await job.recordChildSession({
						sessionId: id,
						role,
						taskId,
						attempt,
						status: "running",
						sessionFile: logPath,
						updatedAt: new Date().toISOString(),
					});
				},
			});
			const submitted = await consume(result);
			await job.recordChildSession({
				sessionId: result.threadId,
				role,
				taskId,
				attempt,
				status: "completed",
				sessionFile: logPath,
				manifestRef: submitted.manifestRef,
				updatedAt: new Date().toISOString(),
			});
			return submitted.value;
		} catch (error) {
			await job.recordChildSession({
				sessionId: sessionId ?? `codex-pending-${taskId}`,
				role,
				taskId,
				attempt,
				status: error instanceof ProviderCapacityError ? "interrupted" : "failed",
				sessionFile: logPath,
				error: error instanceof Error ? error.message : String(error),
				updatedAt: new Date().toISOString(),
			});
			throw error instanceof NonRetryableResearchError || error instanceof ProviderCapacityError
				? error
				: new NonRetryableResearchError(error instanceof Error ? error.message : String(error));
		}
	}

	async run(task: TaskPacket, job: ResearchJob): Promise<WorkerRunResult> {
		await writeTaskPacket(task);
		const cwd = await prepareTaskWorkspace(task, job);
		const resources = await taskInputResources(task, job);
		const root =
			task.writeAuthority === "workspace-write"
				? taskResourcePath(task.scope.workspaceRoot, task.jobId, task.id)
				: undefined;
		const sourceLedger = join(taskDir(task.scope.workspaceRoot, task.jobId, task.id), "codex-sources.json");
		const sources = new Set<string>();
		try {
			const saved: string[] = JSON.parse(await readFile(sourceLedger, "utf8"));
			for (const ref of saved) sources.add(ref);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
		for (const inputRef of task.inputArtifactRefs) {
			const artifact = job.state.canonical[inputRef];
			const evidence = job.state.evidence[artifact?.evidenceId ?? inputRef];
			for (const ref of evidence?.refs ?? []) {
				if (await readSourceRecord(task.scope.workspaceRoot, task.jobId, ref)) sources.add(ref);
			}
		}
		const searchSchema = Type.Object(
			{ query: Type.String({ minLength: 3, maxLength: 500 }), limit: Type.Integer({ minimum: 1, maximum: 10 }) },
			{ additionalProperties: false },
		);
		const tools: CodexTool[] = task.allowedTools.includes("astra_search_papers")
			? [
					{
						name: "astra_search_literature",
						description:
							"Retrieve cached papers or search OpenAlex, Crossref and arXiv with verifiable receipts. If insufficient, use native web search, then astra_list_sources for observed sourceRefs. Metadata and snippets do not prove full-text claims.",
						inputSchema: searchSchema,
						execute: async (input) => {
							const args = input as Static<typeof searchSchema>;
							const result = await searchLiterature({
								...args,
								workspaceRoot: task.scope.workspaceRoot,
								jobId: task.jobId,
							});
							for (const source of result.results) sources.add(source.sourceRef);
							await atomicWriteJson(sourceLedger, [...sources]);
							return result;
						},
					},
					{
						name: "astra_list_sources",
						description:
							"List receipted sources available to this task from search, previous execution, or declared upstream evidence. Use these exact sourceRefs in the final output.",
						inputSchema: Type.Object({}, { additionalProperties: false }),
						execute: async () => {
							const records = await Promise.all(
								[...sources].map((ref) => readSourceRecord(task.scope.workspaceRoot, task.jobId, ref)),
							);
							return { results: records.filter((record) => record !== undefined) };
						},
					},
				]
			: [];
		const skills = await loadStageSkills(task.scope.workspaceRoot, task.stageId, "worker");
		const minSourceRefs = job.definitions[task.stageId].minSourceRefs ?? 0;
		const validateSubmission = async (output: Static<ReturnType<typeof codexWorkerSchema>>, sessionRef: string) => {
			for (const ref of output.refs) {
				if (
					ref.kind === "source" &&
					(!sources.has(ref.ref) || !(await readSourceRecord(task.scope.workspaceRoot, task.jobId, ref.ref)))
				) {
					throw new NonRetryableResearchError(`Worker cited a source not retrieved in this task: ${ref.ref}`);
				}
			}
			return validateWorkerSubmission(
				task,
				{ ...output, content: JSON.parse(output.contentJson) },
				{
					executionRoot: cwd,
					sessionRef,
					minSourceRefs,
				},
			);
		};
		tools.push({
			name: "astra_validate_submission",
			description:
				"Check a draft submission against the final file, field and source requirements. Fix reported errors before returning final JSON. This does not submit evidence or replace independent review.",
			inputSchema: codexWorkerSchema(),
			execute: async (input) => {
				await validateSubmission(input as Static<ReturnType<typeof codexWorkerSchema>>, "preflight-only");
				return { valid: true };
			},
		});
		return this.execute(
			job,
			"worker",
			task.id,
			task.attempt,
			{
				cwd,
				schema: codexWorkerSchema(minSourceRefs),
				tools,
				webSearch: task.allowedTools.includes("astra_search_papers"),
				onWebSearch: async (item) => {
					const records = await recordCodexWebSources(
						{ workspaceRoot: task.scope.workspaceRoot, jobId: task.jobId, query: task.objective, limit: 10 },
						item,
					);
					for (const record of records) sources.add(record.sourceRef);
					await atomicWriteJson(sourceLedger, [...sources]);
				},
				instructions: `${skills.join("\n\n")}\n\n${submissionInstructions} Before returning final JSON, call astra_validate_submission with the exact draft. Correct any reported errors within the existing task budget, then return the validated draft. Validation is not scientific acceptance.`,
				prompt: `Execute this task: ${JSON.stringify(task)}. Read ASTRA_TASK_CONTEXT.json and the referenced inputs. Preserve source directories in produced files. Use ASTRA_RESOURCE_ROOT for environments, models, datasets and caches. Return artifactType, contentJson (a JSON-encoded object with every required output field), and refs for actual files. This stage requires at least ${minSourceRefs} distinct receipted source entries with kind=source in refs, including repair submissions; citing sources only in contentJson does not satisfy this requirement. Reuse declared input source receipts. For new papers use astra_search_literature; if unavailable or insufficient, use native web search targeting original paper pages, then astra_list_sources for host-observed sourceRefs. Do not cite an attempted URL absent from returned results. Distinguish metadata and search snippets from verified full text; restrict claims to available evidence. A negative scientific review is valid evidence; never manufacture positive findings.`,
				readRoots: [
					...resources.map((resource) => resource.root),
					...(process.platform === "linux" && task.stageId === "paper-compile"
						? ["/etc/texmf", "/var/lib/texmf"].filter(existsSync)
						: []),
				],
				writeRoots: root ? [cwd, root] : [],
				network: Boolean(root),
				env: {
					...(root
						? {
								ASTRA_RESOURCE_ROOT: root,
								PIP_CACHE_DIR: join(root, "cache", "pip"),
								HF_HOME: join(root, "cache", "huggingface"),
								TORCH_HOME: join(root, "cache", "torch"),
								XDG_CACHE_HOME: join(root, "cache"),
							}
						: {}),
					...Object.fromEntries(resources.map((resource) => [resource.envVar, resource.root])),
				},
				maxToolCalls: task.budget.maxToolCalls,
				timeoutMs: task.budget.maxRuntimeMs,
			},
			async (result) => {
				const sessionRef = `codex-session:${result.threadId}`;
				const validated = await validateSubmission(result.output, sessionRef);
				// Match the Pi evidence convention: durable refs are relative to the research root.
				const prefix = `.astra/jobs/${task.jobId}/workspaces/${task.id}/`;
				const refs = validated.outputRefs.map((ref) =>
					["artifact", "log"].includes(ref.kind) ? { ...ref, ref: `${prefix}${ref.ref}` } : ref,
				);
				const manifestRef = await writeWorkerOutputManifest(
					{
						schemaVersion: "astra.worker_output_manifest.v1",
						manifestId: `worker_${randomUUID()}`,
						jobId: task.jobId,
						taskId: task.id,
						agentId: task.agentId,
						status: "completed",
						artifactType: result.output.artifactType,
						content: validated.content,
						outputRefs: refs,
						validationStatus: "passed",
						validationErrors: [],
						sessionRef,
						createdAt: new Date().toISOString(),
					},
					task.scope.workspaceRoot,
				);
				return {
					manifestRef,
					value: {
						content: validated.content,
						refs: refs.map((ref) => ref.ref),
						artifactType: result.output.artifactType,
					},
				};
			},
		);
	}

	async review(evidence: Evidence, job: ResearchJob): Promise<ReviewerRunResult> {
		const sourceTask = job.state.tasks[evidence.taskId];
		const required = [...new Set([...sourceTask.acceptanceChecks, ...sourceTask.successCriteria])];
		const definition = job.definitions[evidence.stageId];
		const reviewTasks = Object.values(job.state.tasks).filter(
			(task) => task.role === "reviewer" && task.inputArtifactRefs.includes(evidence.id),
		);
		const interrupted = reviewTasks.find(
			(task) =>
				task.status === "ready" &&
				Object.values(job.state.sessions).some(
					(session) =>
						session.taskId === task.id && session.role === "reviewer" && session.status === "interrupted",
				),
		);
		const ordinal = reviewTasks.length + 1;
		const task =
			interrupted ??
			(await job.dispatchTask({
				stageId: evidence.stageId,
				stageExecutionId: sourceTask.stageExecutionId,
				role: "reviewer",
				objective: `Independently review ${evidence.id}, review ${ordinal}`,
				inputArtifactRefs: [evidence.id],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "review",
				requiredOutputFields: ["verdict", "findings", "criteria", "verifiedRefs"],
				acceptanceChecks: ["apply the frozen evidence contract independently"],
				failureSignals: ["missing verified evidence"],
				dependencies: [],
				scope: { workspaceRoot: sourceTask.scope.workspaceRoot, allowedPaths: ["."] },
				allowedTools: ["read", "grep", "find", "ls"],
				writeAuthority: "none",
				budget: definition.reviewerBudget ?? { maxTurns: 8, maxToolCalls: 32, maxRuntimeMs: 180_000 },
				reviewGateRequired: false,
				resumePolicy: "resume-session",
				successCriteria: ["criterion-level review submitted"],
				replayKey: `codex-review:${evidence.id}:${ordinal}`,
			}));
		try {
			await writeTaskPacket(task);
			const cwd = taskDir(task.scope.workspaceRoot, task.jobId, task.id);
			const resolvedEvidenceRefs = await prepareReviewEvidenceBundle(task, evidence, job);
			const resources = [
				...(await taskInputResources(task, job)),
				...(await taskInputResources(sourceTask, job)),
			].map(({ artifactId, artifactType, taskId, root }) => ({ artifactId, artifactType, taskId, root }));
			const targetSnapshotRef = await writeReviewSnapshot(
				{ evidence, resolvedEvidenceRefs, resources },
				task.scope.workspaceRoot,
				task.jobId,
				task.id,
			);
			const packetId = `review_packet_${task.id}`;
			await writeReviewPacket(
				{
					schemaVersion: "astra.review_packet.v1",
					id: packetId,
					jobId: task.jobId,
					taskId: task.id,
					evidenceId: evidence.id,
					targetSnapshotHash: checksum({
						evidenceId: evidence.id,
						content: evidence.content,
						refs: evidence.refs,
						checksum: evidence.checksum,
					}),
					targetSnapshotRef,
					inputRefs: [evidence.id],
					resolvedEvidenceRefs,
					objective: task.objective,
					stageContract: {
						stageId: definition.id,
						label: definition.label,
						outputArtifactType: definition.outputArtifactType,
						requiredOutputFields: definition.requiredOutputFields,
						acceptanceChecks: definition.acceptanceChecks,
						failureSignals: definition.failureSignals,
					},
					workerContract: {
						objective: sourceTask.objective,
						requiredOutputFields: sourceTask.requiredOutputFields,
						acceptanceChecks: sourceTask.acceptanceChecks,
						failureSignals: sourceTask.failureSignals,
						successCriteria: sourceTask.successCriteria,
					},
					reviewerRole: "reviewer",
					freshThread: true,
					blinded: true,
					bannedContext: ["other reviews", "worker session history"],
					createdAt: new Date().toISOString(),
				},
				task.scope.workspaceRoot,
			);
			await job.setTaskStatus(task.id, "running");
			const allowedRefs = new Set([
				"review-packet.json",
				"review-target-snapshot.json",
				`evidence:${evidence.id}`,
				evidence.id,
				...evidence.refs,
				...sourceTask.inputArtifactRefs,
				...resources.map((resource) => resource.artifactId),
				...resolvedEvidenceRefs.flatMap((ref) => [ref.sourceRef, ref.path]),
			]);
			const skills = await loadStageSkills(task.scope.workspaceRoot, task.stageId, "reviewer");
			const reviewSchema = codexReviewSchema(required, [...allowedRefs]);
			const validateReview = (review: Static<typeof reviewSchema>) => {
				try {
					validateReviewAssessment(review, required);
				} catch (error) {
					throw new NonRetryableResearchError(error instanceof Error ? error.message : String(error));
				}
				if (
					[...review.verifiedRefs, ...review.criteria.flatMap((item) => item.evidenceRefs)].some(
						(ref) => !allowedRefs.has(ref),
					)
				)
					throw new NonRetryableResearchError("Codex review cited evidence outside its packet");
			};
			return await this.execute(
				job,
				"reviewer",
				task.id,
				task.attempt,
				{
					cwd,
					schema: reviewSchema,
					tools: [
						{
							name: "astra_validate_review",
							description:
								"Check that a draft review covers its frozen criteria and that its verdict agrees with the individual assessments. This does not record or approve the review.",
							inputSchema: reviewSchema,
							execute: async (input) => {
								validateReview(input as Static<typeof reviewSchema>);
								return { valid: true };
							},
						},
					],
					instructions: `${skills.join("\n\n")}\n\n${submissionInstructions} Before final submission, call astra_validate_review and correct inconsistent verdicts or omitted criteria. Review validity is separate from the research outcome: identify a failed frozen criterion when rejecting a report, and preserve negative scientific findings.`,
					prompt: `You are an independent reviewer with no worker or prior reviewer conversation. Your review directory is ${cwd}; use absolute paths or an explicit working directory when switching between this packet and resource directories. You have at most ${task.budget.maxToolCalls} tool calls: batch related file reads and numerical checks into a few commands. Read review-packet.json, review-target-snapshot.json, and all relevant evidence files. The snapshot's resources list gives read-only access to the original runtime results of this task and its declared inputs. Inspect actual result files and logs there, not just manifest claims; cite the containing resource's exact artifactId and describe the inspected filenames in your rationale. Judge the current artifact against the frozen worker contract; do not demand future-stage results. Include each workerContract.acceptanceChecks and workerContract.successCriteria string from review-packet.json exactly once in criteria; do not add the reviewer task's own checks. Passing requires every criterion to pass with actual evidenceRefs. Cite the exact evidence ID, refs listed in the packet, resource artifactIds, or the two review JSON files you read. Copy reference strings exactly. Submit the structured review only after verification; avoid preliminary review JSON while still inspecting files. A complete negative research assessment can be a valid artifact.`,
					readRoots: resources.map((resource) => resource.root),
					maxToolCalls: task.budget.maxToolCalls,
					timeoutMs: task.budget.maxRuntimeMs,
				},
				async (result) => {
					const review = result.output;
					validateReview(review);
					const manifestRef = await writeReviewerOutputManifest(
						{
							...review,
							schemaVersion: "astra.reviewer_output_manifest.v1",
							manifestId: `review_${randomUUID()}`,
							jobId: task.jobId,
							taskId: task.id,
							evidenceId: evidence.id,
							sessionRef: `codex-session:${result.threadId}`,
							createdAt: new Date().toISOString(),
						},
						task.scope.workspaceRoot,
					);
					await writeReviewTrace(
						{
							schemaVersion: "astra.review_trace.v1",
							id: `trace_${task.id}`,
							packetId,
							jobId: task.jobId,
							taskId: task.id,
							evidenceId: evidence.id,
							sessionId: result.threadId,
							verdict: review.verdict,
							findings: review.findings,
							createdAt: new Date().toISOString(),
						},
						task.scope.workspaceRoot,
					);
					return { manifestRef, value: { ...review, reviewerTaskId: task.id } };
				},
			);
		} catch (error) {
			await job.setTaskStatus(task.id, error instanceof ProviderCapacityError ? "ready" : "failed");
			throw error instanceof NonRetryableResearchError || error instanceof ProviderCapacityError
				? error
				: new NonRetryableResearchError(error instanceof Error ? error.message : String(error));
		}
	}

	private async main<S extends TSchema, T>(
		job: ResearchJob,
		taskId: string,
		schema: S,
		prompt: string,
		consume: (result: CodexRunResult<Static<S>>) => Promise<{ value: T; manifestRef: string }>,
	): Promise<T> {
		const state = job.state;
		const root = state.frame.permissions.workspaceRoot;
		const stageId = state.frame.activeStageId;
		const cwd = join(root, ".astra", "jobs", state.frame.jobId, "main-agent", "codex-context");
		await mkdir(cwd, { recursive: true });
		const completionBlockers = job.completionBlockers();
		await atomicWriteJson(join(cwd, "research-context.json"), {
			state,
			capabilities: job.definitions,
			completionBlockers,
		});
		const searches = Object.values(state.searchBatches).filter((batch) => batch.stageId === stageId);
		await atomicWriteJson(join(cwd, "research-summary.json"), {
			fullStatePath: "research-context.json",
			frame: state.frame,
			budgetUsage: state.budgetUsage,
			activeCapability: job.definitions[stageId],
			capabilityCatalog: Object.values(job.definitions).map(
				({ id, label, suggestedInputArtifactTypes, outputArtifactType }) => ({
					id,
					label,
					suggestedInputArtifactTypes,
					outputArtifactType,
				}),
			),
			canonicalRoute: state.canonicalRoute,
			canonicalArtifacts: Object.values(state.canonical).map(({ id, type, status, evidenceId }) => ({
				id,
				type,
				status,
				evidenceId,
			})),
			evidence: Object.values(state.evidence).map(({ id, stageId, taskId, type, status }) => ({
				id,
				stageId,
				taskId,
				type,
				status,
			})),
			reviews: Object.values(state.reviews).map(({ id, evidenceId, verdict, score }) => ({
				id,
				evidenceId,
				verdict,
				score,
			})),
			tasks: Object.values(state.tasks).map(({ id, stageId, role, status, attempt, inputArtifactRefs }) => ({
				id,
				stageId,
				role,
				status,
				attempt,
				inputArtifactRefs,
			})),
			openObligations: state.frame.openObligationIds.map((id) => state.obligations[id]),
			openQuestions: state.graph.openQuestionIds.map((id) => state.graph.nodes[id]),
			unresolvedObjections: state.graph.unresolvedObjectionIds.map((id) => state.graph.nodes[id]),
			userGuidance: Object.values(state.graph.nodes)
				.filter((node) => node.actor === "user")
				.sort((left, right) => left.updatedAt.localeCompare(right.updatedAt)),
			searches,
			candidateEvaluations: Object.values(state.candidateEvaluations).filter((evaluation) =>
				searches.some((batch) => batch.id === evaluation.batchId),
			),
			completionBlockers,
		});
		const skills = await loadStageSkills(root, stageId, "main-agent");
		return this.execute(
			job,
			"main-agent",
			taskId,
			1,
			{
				cwd,
				schema,
				instructions: `${skills.join("\n\n")}\n\n${submissionInstructions}`,
				prompt: `You are the persistent research main agent. Read research-summary.json first for the fresh capability contract, evidence index, open issues and completion blockers; prior conversation may be stale. Full original state and graph remain in research-context.json. Inspect the specific evidence content, review findings or historical nodes needed for this decision using targeted queries (for example jq by evidence ID), and the original files in the read-only job directory. The summary is an index, not proof of evidence quality. Avoid repeatedly dumping the entire state file. ${prompt}`,
				readRoots: [join(root, ".astra", "jobs", state.frame.jobId)],
				maxToolCalls: 32,
				timeoutMs: 600_000,
			},
			consume,
		);
	}

	planStage(
		job: ResearchJob,
		obligation?: Obligation,
		requestedMode: "decompose" | "search" | "repair" = obligation ? "repair" : "decompose",
	): Promise<StagePlanManifest> {
		const stageId = job.state.frame.activeStageId;
		const id = `plan_${randomUUID()}`;
		return this.main(
			job,
			id,
			codexPlanSchema,
			`Design actionable worker assignments for the active capability ${stageId} in mode ${requestedMode}. ${obligation ? `Resolve obligation ${JSON.stringify(obligation)} with one complete repair task.` : requestedMode === "search" ? "Create diverse independent candidates within searchPolicy bounds. Inspect previous batches and evaluations before continuing a search." : "Create one focused task, or two independently useful tasks."} Every task must include every requiredOutputFields field from the capability contract. Use exact existing artifact/evidence IDs as inputs. Concurrent tasks cannot depend on one another. Each task.objective must instruct its worker to perform this capability and deliver its outputs, not to plan dispatch, enter a stage or declare pipeline completion. Success criteria must assess delivered results, not readiness to start. Only you are planning: do not execute these assignments yourself, and do not copy that restriction into worker objectives. Workers must perform the work and verification permitted by their capability contract.`,
			async (result) => {
				const value: StagePlanManifest = {
					...result.output,
					schemaVersion: "astra.stage_plan_manifest.v1",
					id,
					jobId: job.state.frame.jobId,
					stageId,
					decisionRef: id,
					mode: requestedMode,
					obligationId: obligation?.id,
					sessionRef: `codex-session:${result.threadId}`,
					createdAt: new Date().toISOString(),
				};
				const manifestRef = await writeStagePlanManifest(value, job.state.frame.permissions.workspaceRoot);
				return { value, manifestRef };
			},
		);
	}

	private decision<S extends TSchema>(
		job: ResearchJob,
		type: MainAgentDecisionManifest["decisionType"],
		schema: S,
		prompt: string,
		fields: (output: Static<S>) => Partial<MainAgentDecisionManifest>,
	): Promise<MainAgentDecisionManifest> {
		const id = `decision_${randomUUID()}`;
		return this.main(job, id, schema, prompt, async (result) => {
			const value: MainAgentDecisionManifest = {
				schemaVersion: "astra.main_agent_decision_manifest.v1",
				manifestId: id,
				jobId: job.state.frame.jobId,
				decisionType: type,
				decisionRef: id,
				stageId: job.state.frame.activeStageId,
				rationale: "",
				...fields(result.output),
				sessionRef: `codex-session:${result.threadId}`,
				createdAt: new Date().toISOString(),
			};
			const manifestRef = await writeMainDecisionManifest(value, job.state.frame.permissions.workspaceRoot);
			return { value, manifestRef };
		});
	}

	decideEvidence(evidence: Evidence, job: ResearchJob): Promise<MainAgentDecisionManifest> {
		return this.decision(
			job,
			"evidence",
			codexEvidenceDecisionSchema,
			`Accept, reject, or defer evidence ${evidence.id} using its independent reviews. A rigorous negative whole-research review is valid evidence; acceptance does not endorse the research hypothesis.`,
			(output) => ({ ...output, evidenceId: evidence.id }),
		);
	}

	decideAdoption(evidence: Evidence, job: ResearchJob): Promise<MainAgentDecisionManifest> {
		return this.decision(
			job,
			"adoption",
			codexAdoptionSchema,
			`Decide whether reviewed evidence ${evidence.id} should become the canonical version of this stage. Adoption automatically replaces its previous canonical version.`,
			(output) => ({ ...output, evidenceId: evidence.id }),
		);
	}

	decideSearch(
		batch: SearchBatch,
		evaluations: CandidateEvaluation[],
		job: ResearchJob,
	): Promise<MainAgentDecisionManifest> {
		return this.decision(
			job,
			"search-selection",
			codexSearchSchema,
			`Compare batch ${JSON.stringify(batch)} using independent evaluations ${JSON.stringify(evaluations)}. Select one passing candidate or continue search only if round < maxRounds. Set selectedCandidateId to null when continuing.`,
			(output) => {
				if (
					output.continueSearch === Boolean(output.selectedCandidateId) ||
					(output.continueSearch && batch.round >= batch.maxRounds)
				)
					throw new NonRetryableResearchError(
						"Codex search decision must select one candidate or continue within the round budget",
					);
				return { ...output, selectedCandidateId: output.selectedCandidateId ?? undefined, searchBatchId: batch.id };
			},
		);
	}

	decideRoute(job: ResearchJob): Promise<MainAgentDecisionManifest> {
		return this.decision(
			job,
			"route",
			codexRouteSchema,
			"Choose continue, search, advance, backtrack, ask-user, or complete. Capabilities are not a fixed pipeline. Completion requires zero completionBlockers and a valid whole-research review. Preserve scientificOutcome and missionCoverage; never force a positive scientific result. Ask the user only for a material scientific decision. Cite exact canonical artifact IDs as evidenceRefs. Set targetStageId and question to null when inapplicable.",
			(output) => ({
				...output,
				targetStageId: output.targetStageId ?? undefined,
				question: output.question ?? undefined,
			}),
		);
	}
}
