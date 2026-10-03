import type { JobSnapshot, TaskPacket } from "./types.ts";

export function taskBindingIsCurrent(snapshot: JobSnapshot, task: TaskPacket): boolean {
	if (
		!task ||
		task.jobId !== snapshot.frame.jobId ||
		!snapshot.stages[task.stageId] ||
		task.status === "blocked" ||
		(task.stageRevision ?? 1) !== (snapshot.stages[task.stageId].revision ?? 1) ||
		Object.values(snapshot.tasks).some(
			(next) =>
				next.supersedesTaskId === task.id || (next.replayKey === task.replayKey && next.attempt > task.attempt),
		)
	)
		return false;
	return true;
}

/** Retained evidence and current unfinished consumers own source files; historical task rows do not. */
export function taskHasRetainedOwner(
	snapshot: JobSnapshot,
	taskId: string,
	excludedEvidenceIds = new Set<string>(),
): boolean {
	const retained = Object.values(snapshot.evidence).filter(
		(evidence) =>
			!excludedEvidenceIds.has(evidence.id) &&
			taskBindingIsCurrent(snapshot, snapshot.tasks[evidence.taskId]) &&
			!snapshot.discardedEvidence[evidence.id] &&
			!Object.values(snapshot.retiredArtifacts).some((receipt) => receipt.evidenceId === evidence.id) &&
			!Object.values(snapshot.discardedCandidates).some((receipt) => receipt.evidenceId === evidence.id),
	);
	if (retained.some((evidence) => evidence.taskId === taskId)) return true;
	return Object.values(snapshot.tasks).some((consumer) => {
		if (!["ready", "running"].includes(consumer.status) || !taskBindingIsCurrent(snapshot, consumer)) return false;
		if (consumer.id === taskId) return true;
		if (consumer.supersedesTaskId === taskId) return true;
		return consumer.inputArtifactRefs.some((ref) => {
			const evidence = retained.find((value) => value.id === (snapshot.canonical[ref]?.evidenceId ?? ref));
			return evidence?.taskId === taskId;
		});
	});
}
