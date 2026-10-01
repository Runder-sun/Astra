import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { FIXTURE_PDF_SOURCE } from "../../src/fixture-pdf.ts";

// Protocol fixture only. The parseable PDF does not establish scientific results or publication quality.
export const stages = ["validation", "literature", "idea", "novelty", "refine", "experiment-plan", "implement-solution", "run", "monitor", "result-to-claim", "paper-plan", "paper-write", "paper-compile", "research-review"];

export function fullResearchOutput(schema) {
	const fields = schema.properties;
	if (fields.contentJson) {
		const context = JSON.parse(readFileSync("ASTRA_TASK_CONTEXT.json", "utf8"));
		const { task } = context;
		const content = Object.fromEntries(task.requiredOutputFields.map(field => [field, `fixture ${field}`]));
		const refs = [];
		const emit = (path, text, kind = "artifact") => {
			mkdirSync(dirname(path), { recursive: true });
			writeFileSync(path, text);
			refs.push({ kind, ref: path, summary: "Offline protocol fixture" });
		};
		if (task.stageId === "implement-solution") {
			const repaired = context.inputs.evidence.length > 0;
			if (repaired) {
				const input = context.inputs.files.find(file => file.path.endsWith("src/main.mjs"));
				if (!input || execFileSync(process.execPath, [input.path], { encoding: "utf8" }).trim() !== "41") throw new Error("Repair input code was not preserved");
			}
			emit("src/main.mjs", 'import { value } from "./lib/value.mjs"; console.log(value);');
			emit("src/lib/value.mjs", `export const value = ${repaired ? 42 : 41};`);
			content.tests = repaired ? "repaired" : "needs-repair";
		}
		if (task.stageId === "run") {
			const input = context.inputs.files.find(file => file.path.endsWith("src/main.mjs"));
			const result = execFileSync(process.execPath, [input.path], { encoding: "utf8" }).trim();
			if (result !== "42") throw new Error("Downstream code did not execute the repaired implementation");
			emit("results/run.log", `${result}\n`, "log");
			content.metrics = { value: Number(result) };
		}
		if (task.stageId === "monitor") {
			const input = context.inputs.files.find(file => file.path.endsWith("results/run.log"));
			if (readFileSync(input.path, "utf8").trim() !== "42") throw new Error("Monitor did not receive the run log");
		}
		if (task.stageId === "result-to-claim") Object.assign(content, {
			scientificOutcome: "supported", missionCoverage: "sufficient", claims: [{ statement: "Fixture execution returns 42", assessment: "supported" }], supportingResults: ["fixture run log"], unsupportedClaims: [], missingEvidence: [], conclusion: "supported",
		});
		if (task.stageId === "research-review") Object.assign(content, {
			verdict: "pass", scientificOutcome: "supported", missionCoverage: "sufficient", strengths: ["fixture evidence chain complete"], weaknesses: [], claimAudit: ["fixture only"], requiredRepairs: [],
		});
		if (task.stageId === "paper-plan") {
			for (const artifact of context.inputs.canonicalArtifacts) refs.push({ kind: "artifact", ref: artifact.id, summary: "Declared upstream canonical snapshot" });
		}
		if (task.stageId === "paper-write") {
			const snapshot = context.inputs.files.find(file => file.path.includes("/canonical/artifact_"));
			if (!snapshot || !JSON.parse(readFileSync(snapshot.path, "utf8")).content) throw new Error("Canonical ID citation did not propagate as a readable snapshot");
			emit("paper/manuscript.md", "# Offline fixture\n\nThis exercises delivery, not research validity.\n");
			content.manuscript = "paper/manuscript.md";
		}
		if (task.stageId === "paper-compile") {
			const input = context.inputs.files.find(file => file.path.endsWith("paper/manuscript.md"));
			if (!readFileSync(input.path, "utf8").includes("Offline fixture")) throw new Error("Manuscript not received");
			emit("paper/build.mjs", FIXTURE_PDF_SOURCE);
			const log = execFileSync(process.execPath, ["paper/build.mjs", "paper/paper.pdf"], { encoding: "utf8" });
			refs.push({ kind: "artifact", ref: "paper/paper.pdf", summary: "Offline fixture PDF" });
			emit("paper/build.log", log, "log");
			content.artifact = "paper/paper.pdf";
			content.buildLog = "paper/build.log";
			content.source = "paper/build.mjs";
			content.buildInputs = ["paper/build.mjs"];
			content.command = "node paper/build.mjs paper/paper.pdf";
		}
		return { artifactType: task.requiredOutputType, contentJson: JSON.stringify(content), refs };
	}
	if (fields.verdict) {
		const packet = JSON.parse(readFileSync("review-packet.json", "utf8"));
		const snapshot = JSON.parse(readFileSync("review-target-snapshot.json", "utf8"));
		for (const ref of packet.resolvedEvidenceRefs) readFileSync(ref.path);
		for (const ref of snapshot.evidence.refs.filter(ref => /^(?:openalex:|doi:|arxiv:|https:\/\/)/.test(ref))) {
			if (!packet.resolvedEvidenceRefs.some(item => item.sourceRef === ref)) throw new Error("Reviewer missing source receipt");
		}
		const passed = snapshot.evidence.content.tests !== "needs-repair";
		const refs = ["review-target-snapshot.json", ...packet.resolvedEvidenceRefs.map(ref => ref.path)];
		return { verdict: passed ? "pass" : "fail", score: passed ? 1 : 0.2, findings: [passed ? "Fixture verified" : "Implementation returns 41; repair it to return 42"], verifiedRefs: refs, criteria: [...new Set([...packet.workerContract.acceptanceChecks, ...packet.workerContract.successCriteria])].map(criterion => ({ criterion, passed, score: passed ? 1 : 0.2, evidenceRefs: refs, rationale: "Offline fixture contract" })) };
	}
	const context = JSON.parse(readFileSync("research-context.json", "utf8"));
	const { state } = context;
	const stageId = state.frame.activeStageId;
	const stage = context.capabilities[stageId];
	if (fields.tasks) {
		const obligation = Object.values(state.obligations).find(item => item.status === "open" && state.evidence[state.reviews[item.sourceReviewId].evidenceId].stageId === stageId);
		const inputs = Object.values(state.canonical).filter(artifact => stageId === "research-review" || stage.suggestedInputArtifactTypes.includes(artifact.type)).map(artifact => artifact.id);
		if (obligation) inputs.push(state.reviews[obligation.sourceReviewId].evidenceId);
		const count = obligation ? 1 : stage.searchPolicy?.minCandidates ?? 1;
		const round = Object.values(state.searchBatches).filter(batch => batch.stageId === stageId).length + 1;
		return { tasks: Array.from({ length: count }, (_, index) => ({ key: `candidate-${index}`, deliveryKind: "stage", objective: `Produce ${stageId} fixture ${index}`, inputArtifactRefs: inputs, requiredOutputFields: stage.requiredOutputFields, acceptanceChecks: stage.acceptanceChecks, failureSignals: stage.failureSignals, successCriteria: stage.acceptanceChecks, responsibilityBindings: [], responsibilityTransfers: [], hypothesis: `Fixture round ${round} alternative ${index}` })), rationale: "Exercise the full research contract" };
	}
	if (fields.decision) return { decision: "accept", rationale: "Reviewed fixture" };
	if (fields.adopt) return { adopt: true, rationale: "Adopt fixture" };
	if (fields.continueSearch) {
		const batch = Object.values(state.searchBatches).find(batch => batch.stageId === stageId && ["planning", "running", "evaluating"].includes(batch.status));
		const continueSearch = stageId === "idea" && batch.round === 1;
		return { continueSearch, selectedCandidateId: continueSearch ? null : Object.keys(batch.candidates)[0], rationale: "Exercise continuation and selection" };
	}
	if (fields.routeAction && Object.values(state.obligations).some(issue => issue.status === "open" && issue.stageId === stageId)) return { routeAction: "continue", targetStageId: null, question: null, newQuestions: [], evidenceRefs: [], rationale: "Repair the open fixture findings" };
	if (fields.routeAction) return { routeAction: stageId === "research-review" ? "complete" : "advance", targetStageId: stages[stages.indexOf(stageId) + 1] ?? null, question: null, newQuestions: [], evidenceRefs: Object.keys(state.canonical), rationale: "Advance the offline fixture" };
	throw new Error("Unknown fixture schema");
}
