import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const auditScript = fileURLToPath(new URL("./audit-astra-run.mjs", import.meta.url));

test("counts Pi assistant terminal errors as failed child sessions", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-"));
	const jobId = "job_fixture";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	try {
		await mkdir(join(jobRoot, "sessions"), { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { activeStageId: "research-lit" },
				stages: { "research-lit": { status: "completed" } },
				eventSeq: 1,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {},
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		await writeFile(
			join(jobRoot, "sessions", "child.jsonl"),
			`${JSON.stringify({
				type: "message",
				message: { role: "assistant", stopReason: "error", errorMessage: "fixture failure" },
			})}\n`,
		);

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		assert.equal(JSON.parse(result.stdout).sessions.failedFiles, 1);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("recognizes resumed completion checkpoints and optional compaction in parent Pi sessions", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-parent-"));
	const jobId = "job_resumed";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const parentSessions = join(workspace, "parent-sessions");
	try {
		await mkdir(join(jobRoot, "sessions"), { recursive: true });
		await mkdir(parentSessions, { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { activeStageId: "research-lit" },
				stages: { "research-lit": { status: "completed" } },
				eventSeq: 0,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {},
			}),
		);
		await writeFile(
			join(jobRoot, "events.jsonl"),
			`${JSON.stringify({ seq: 1, event: { type: "job_resumed" } })}\n`,
		);
		await writeFile(
			join(parentSessions, "resume.jsonl"),
			[
				{ type: "custom", customType: "astra_research_result", data: { action: "resume", jobId } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n"),
		);

		const missingCheckpoint = spawnSync(process.execPath, [auditScript, workspace, jobId, parentSessions], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(missingCheckpoint.status, 0, missingCheckpoint.stderr);
		assert.equal(JSON.parse(missingCheckpoint.stdout).contractChecks.recoveryCheckpointed, false);

		await writeFile(
			join(parentSessions, "resume.jsonl"),
			[
				{ type: "custom", customType: "astra_checkpoint", data: { jobId } },
				{ type: "custom", customType: "astra_research_result", data: { action: "resume", jobId } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n"),
		);
		const checkpointed = spawnSync(process.execPath, [auditScript, workspace, jobId, parentSessions], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(checkpointed.status, 0, checkpointed.stderr);
		const checkpointReport = JSON.parse(checkpointed.stdout);
		assert.equal(checkpointReport.contractChecks.recoveryCheckpointed, true);
		assert.equal(checkpointReport.parentSessions.checkpointEntries, 1);
		assert.equal(checkpointReport.parentSessions.compactionEntries, 0);

		await writeFile(
			join(parentSessions, "resume.jsonl"),
			[
				{ type: "custom", customType: "astra_checkpoint", data: { jobId } },
				{ type: "compaction", summary: "durable checkpoint" },
				{ type: "custom", customType: "astra_research_result", data: { action: "resume", jobId } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n"),
		);
		const result = spawnSync(process.execPath, [auditScript, workspace, jobId, parentSessions], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.equal(report.contractChecks.recoveryCheckpointed, true);
		assert.equal(report.policy.automation, null);
		assert.deepEqual(report.canonical.duplicateEvidenceIds, []);
		assert.equal(report.parentSessions.matchingFiles, 1);
		assert.deepEqual(report.parentSessions.actions, ["resume"]);
		assert.equal(report.parentSessions.compactionFiles, 1);
		assert.equal(report.parentSessions.compactionEntries, 1);
		assert.equal(report.parentSessions.checkpointEntries, 1);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("audits stage plans, task context, upstream artifacts, and source receipts", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-contracts-"));
	const jobId = "job_contracts";
	const taskId = "task_literature";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const sourceRefs = ["openalex:W1", "openalex:W2", "openalex:W3"];
	try {
		await mkdir(join(jobRoot, "tasks", taskId), { recursive: true });
		await mkdir(join(jobRoot, "workspaces", taskId), { recursive: true });
		await mkdir(join(jobRoot, "sources"), { recursive: true });
		const plan = {
			id: "plan_literature",
			stageId: "literature",
			tasks: [{ key: "primary", objective: "Survey the selected literature" }],
		};
		const task = {
			id: taskId,
			role: "worker",
			stageId: "literature",
			objective: "Survey the selected literature",
			replayKey: "stage-plan:plan_literature:primary",
			inputArtifactRefs: ["artifact_validation"],
		};
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "literature" },
				stages: { literature: { status: "completed" } },
				stagePlans: { [plan.id]: plan },
				eventSeq: 0,
				lease: null,
				tasks: { [task.id]: task },
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: { artifact_validation: { id: "artifact_validation", status: "retired" } },
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		await writeFile(
			join(jobRoot, "workspaces", taskId, "ASTRA_TASK_CONTEXT.json"),
			JSON.stringify({
				schemaVersion: "astra.task_context.v1",
				mission: { jobId },
				stage: { minSourceRefs: 3 },
				task: { id: taskId },
				inputs: { canonicalArtifacts: [{ id: "artifact_validation" }] },
			}),
		);
		await writeFile(
			join(jobRoot, "tasks", taskId, "output-manifest.json"),
			JSON.stringify({ outputRefs: sourceRefs.map((ref) => ({ kind: "source", ref })) }),
		);
		for (const [index, sourceRef] of sourceRefs.entries()) {
			await writeFile(join(jobRoot, "sources", `openalex-W${index + 1}.json`), JSON.stringify({ sourceRef }));
		}

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.deepEqual(report.planning.missingStageIds, []);
		assert.deepEqual(report.planning.unplannedWorkerTaskIds, []);
		assert.deepEqual(report.planning.objectiveMismatchTaskIds, []);
		assert.deepEqual(report.planning.forbiddenGenericTaskIds, []);
		assert.deepEqual(report.workspaces.missingTaskIds, []);
		assert.deepEqual(report.workspaces.missingUpstreamTaskIds, []);
		assert.deepEqual(report.sources.insufficientTaskIds, []);
		assert.deepEqual(report.sources.missingReceiptRefs, []);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("audits the accepted chain without treating recovered attempts as active failures", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-recovered-"));
	const jobId = "job_recovered";
	const succeededTaskId = "task_succeeded";
	const failedTaskId = "task_failed";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const failedSessionPath = join(jobRoot, "sessions", "failed.jsonl");
	const mainSessionPath = join(jobRoot, "sessions", "main.jsonl");
	try {
		await mkdir(join(jobRoot, "tasks", succeededTaskId), { recursive: true });
		await mkdir(join(jobRoot, "workspaces", succeededTaskId), { recursive: true });
		await mkdir(join(jobRoot, "workspaces", failedTaskId), { recursive: true });
		await mkdir(join(jobRoot, "sources"), { recursive: true });
		await mkdir(join(jobRoot, "sessions"), { recursive: true });
		const plan = {
			id: "plan_review",
			stageId: "research-review",
			tasks: [
				{ key: "accepted", objective: "Audit the accepted chain" },
				{ key: "recovered", objective: "Historical failed attempt" },
			],
		};
		const succeededTask = {
			id: succeededTaskId,
			role: "worker",
			status: "succeeded",
			stageId: "research-review",
			objective: plan.tasks[0].objective,
			replayKey: `stage-plan:${plan.id}:accepted`,
			inputArtifactRefs: ["artifact_expanded", "artifact_indexed"],
		};
		const failedTask = {
			id: failedTaskId,
			role: "worker",
			status: "failed",
			stageId: "research-review",
			objective: plan.tasks[1].objective,
			replayKey: `stage-plan:${plan.id}:recovered`,
			inputArtifactRefs: ["artifact_expanded"],
		};
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "research-review" },
				stages: { "research-review": { status: "completed" } },
				stagePlans: { [plan.id]: plan },
				eventSeq: 0,
				lease: null,
				tasks: { [succeededTaskId]: succeededTask, [failedTaskId]: failedTask },
				sessions: {
					failed: {
						sessionId: "failed",
						role: "worker",
						taskId: failedTaskId,
						status: "starting",
					},
					main: {
						sessionId: "main",
						role: "main-agent",
						status: "completed",
					},
				},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {
					artifact_expanded: { id: "artifact_expanded", status: "active" },
					artifact_indexed: { id: "artifact_indexed", status: "retired" },
				},
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		for (const task of [succeededTask, failedTask]) {
			await writeFile(
				join(jobRoot, "workspaces", task.id, "ASTRA_TASK_CONTEXT.json"),
				JSON.stringify({
					schemaVersion: "astra.task_context.v1",
					mission: { jobId },
					stage: { minSourceRefs: 1 },
					task: { id: task.id },
					inputs: {
						canonicalArtifacts: [{ id: "artifact_expanded" }],
						omittedRefs: task.id === succeededTaskId ? ["artifact_indexed"] : [],
					},
				}),
			);
		}
		await writeFile(
			join(jobRoot, "tasks", succeededTaskId, "output-manifest.json"),
			JSON.stringify({ outputRefs: [{ kind: "source", ref: "https://doi.org/10.1000/astra" }] }),
		);
		await writeFile(
			join(jobRoot, "sources", "openalex-W1.json"),
			JSON.stringify({
				sourceRef: "openalex:W1",
				record: { doi: "10.1000/astra", landingPageUrl: "https://doi.org/10.1000/astra" },
			}),
		);
		await writeFile(
			failedSessionPath,
			`${[
				{ type: "session", id: "failed" },
				{
					type: "message",
					message: { role: "assistant", stopReason: "error", errorMessage: "recovered fixture failure" },
				},
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n")}\n`,
		);
		await writeFile(
			mainSessionPath,
			`${[
				{ type: "session", id: "main" },
				{ type: "message", message: { role: "assistant", stopReason: "error", errorMessage: "transient 502" } },
				{ type: "message", message: { role: "assistant", stopReason: "toolUse" } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n")}\n`,
		);

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.equal(report.contractChecks.upstreamArtifactsBound, true);
		assert.equal(report.contractChecks.sourceMinimumsSatisfied, true);
		assert.equal(report.contractChecks.sourceReceiptsPresent, true);
		assert.equal(report.contractChecks.acceptedChildSessionsValid, true);
		assert.equal(report.sessions.failedFiles, 2);
		assert.equal(report.sessions.recoveredFailedFiles, 2);
		assert.equal(report.sessions.unrecoveredFailedFiles, 0);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("accepts a whole-research review that passes with nonblocking caveats", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-review-caveats-"));
	const jobId = "job_review_caveats";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	try {
		await mkdir(jobRoot, { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "research-review" },
				stages: { "research-review": { status: "completed" } },
				eventSeq: 0,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {
					artifact_review: {
						id: "artifact_review",
						type: "research-review",
						status: "active",
						content: { verdict: "pass_with_nonblocking_caveats", requiredRepairs: [] },
					},
				},
				graph: { acceptedClaimIds: [], unresolvedObjectionIds: [], openQuestionIds: [] },
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		assert.equal(JSON.parse(result.stdout).researchQuality.wholeResearchReviewPassed, true);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("reports process completion separately from insufficient scientific evidence", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-scientific-outcome-"));
	const jobId = "job_scientific_outcome";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	try {
		await mkdir(jobRoot, { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: {
					jobId,
					status: "completed",
					activeStageId: "research-review",
					scientificOutcome: "inconclusive",
					missionCoverage: "insufficient",
					finalDecisionRef: "complete_inconclusive",
				},
				stages: { "research-review": { status: "completed" } },
				eventSeq: 0,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {
					artifact_review: {
						id: "artifact_review",
						type: "research-review",
						status: "active",
						content: {
							verdict: "pass_with_nonblocking_caveats",
							scientificOutcome: "inconclusive",
							missionCoverage: "insufficient",
							requiredRepairs: [],
						},
					},
				},
				graph: { nodes: {}, acceptedClaimIds: [], unresolvedObjectionIds: [], openQuestionIds: [] },
				routeDecisions: {
					complete_inconclusive: { id: "complete_inconclusive", action: "complete" },
				},
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.equal(report.researchQuality.researchCompletedByDecision, true);
		assert.equal(report.researchQuality.scientificOutcomeRecorded, true);
		assert.equal(report.researchQuality.claimOutcomeConsistent, true);
		assert.deepEqual(report.scientificResult, {
			outcome: "inconclusive",
			missionCoverage: "insufficient",
			acceptedClaimCount: 0,
			processCompleted: true,
		});
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("does not require a pruned workspace for a worker recorded in a retired artifact receipt", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-retired-worker-"));
	const jobId = "job_retired_worker";
	const taskId = "task_retired_worker";
	const artifactId = "artifact_retired_review";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const receiptPath = join(jobRoot, "canonical", `${artifactId}.json.receipt.json`);
	try {
		await mkdir(join(jobRoot, "canonical"), { recursive: true });
		const plan = {
			id: "plan_retired_review",
			stageId: "research-review",
			tasks: [{ key: "retired", objective: "Produce the superseded whole-research review" }],
		};
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "research-review" },
				stages: { "research-review": { status: "completed" } },
				stagePlans: { [plan.id]: plan },
				eventSeq: 0,
				lease: null,
				tasks: {
					[taskId]: {
						id: taskId,
						role: "worker",
						status: "succeeded",
						stageId: "research-review",
						objective: plan.tasks[0].objective,
						replayKey: `stage-plan:${plan.id}:retired`,
						inputArtifactRefs: ["artifact_upstream"],
					},
				},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {},
				retiredArtifacts: {
					[artifactId]: {
						artifactId,
						taskId,
						materializationReceiptRef: receiptPath,
					},
				},
				graph: { acceptedClaimIds: [], unresolvedObjectionIds: [], openQuestionIds: [] },
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		await writeFile(receiptPath, JSON.stringify({ schemaVersion: "astra.retired_artifact_receipt.v1", artifactId }));

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.deepEqual(report.workspaces.missingTaskIds, []);
		assert.deepEqual(report.workspaces.missingUpstreamTaskIds, []);
		assert.equal(report.contractChecks.workerContextsValid, true);
		assert.equal(report.contractChecks.upstreamArtifactsBound, true);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});
