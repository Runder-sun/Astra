import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { assertAstraId, atomicWriteJson } from "./contracts.ts";
import type { ResearchJob } from "./research.ts";
import { JsonlAstraStore, ResearchJobLockedError } from "./store.ts";

export interface GuidanceRequest {
	jobId: string;
	requestId: string;
	sequence: number;
	guidance: string;
}

interface GuidanceInbox {
	jobId: string;
	nextSequence: number;
	pending: string[];
	/** Keep identity receipts after acknowledging pending requests. */
	receipts: GuidanceRequest[];
}

function inboxPath(root: string, jobId: string): string {
	return join(root, ".astra", "jobs", jobId, "guidance-inbox.json");
}

async function readInbox(root: string, jobId: string): Promise<GuidanceInbox> {
	try {
		const value = JSON.parse(await readFile(inboxPath(root, jobId), "utf8")) as GuidanceInbox;
		if (
			value.jobId !== jobId ||
			!Number.isInteger(value.nextSequence) ||
			value.nextSequence < 1 ||
			!Array.isArray(value.pending) ||
			!Array.isArray(value.receipts) ||
			value.receipts.some(
				(request) =>
					request.jobId !== jobId ||
					typeof request.requestId !== "string" ||
					typeof request.guidance !== "string" ||
					!Number.isInteger(request.sequence),
			)
		) {
			throw new Error("research guidance inbox identity is invalid");
		}
		return value;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT")
			return { jobId, nextSequence: 1, pending: [], receipts: [] };
		throw error;
	}
}

async function withInboxLock<T>(root: string, jobId: string, operation: () => Promise<T>): Promise<T> {
	const store = new JsonlAstraStore(root);
	for (let attempt = 0; ; attempt++) {
		try {
			return await store.withGuidanceLock(jobId, operation);
		} catch (error) {
			if (!(error instanceof ResearchJobLockedError) || attempt >= 99) throw error;
			await new Promise((resolve) => setTimeout(resolve, 10));
		}
	}
}

export async function requestResearchGuidance(
	root: string,
	jobId: string,
	guidance: string,
	requestId: string = randomUUID(),
): Promise<GuidanceRequest> {
	assertAstraId(jobId, "job id");
	const statement = guidance.trim();
	if (!statement || !requestId.trim()) throw new Error("research guidance and request identity cannot be empty");
	return withInboxLock(root, jobId, async () => {
		const inbox = await readInbox(root, jobId);
		const prior = inbox.receipts.find((request) => request.requestId === requestId);
		if (prior) {
			if (prior.guidance !== statement)
				throw new Error("research guidance request identity already has different content");
			return prior;
		}
		const request = { jobId, requestId, sequence: inbox.nextSequence++, guidance: statement };
		inbox.receipts.push(request);
		inbox.pending.push(requestId);
		await atomicWriteJson(inboxPath(root, jobId), inbox);
		return request;
	});
}

/** Caller owns the supervisor lock and has preserved all in-flight outputs. */
export async function applyPendingGuidance(job: ResearchJob, confirmRequestId?: string): Promise<void> {
	const root = job.state.frame.permissions.workspaceRoot;
	const jobId = job.state.frame.jobId;
	const inbox = await readInbox(root, jobId);
	if (inbox.pending.length && job.state.frame.status !== "completed") {
		await job.recoverMainAgentDeliveries();
		await job.recoverPendingOperations();
	}
	for (const requestId of inbox.pending) {
		const request = inbox.receipts.find((receipt) => receipt.requestId === requestId);
		if (!request) throw new Error("research guidance inbox request receipt is missing");
		try {
			await job.recordUserGuidance(request.guidance, false, request.requestId);
		} catch (error) {
			// append may be durable even when snapshot persistence fails.
			await job.reload();
			throw error;
		}
		await withInboxLock(root, jobId, async () => {
			const current = await readInbox(root, jobId);
			current.pending = current.pending.filter((id) => id !== requestId);
			await atomicWriteJson(inboxPath(root, jobId), current);
		});
	}
	if (confirmRequestId) {
		const current = await readInbox(root, jobId);
		const request = current.receipts.find((receipt) => receipt.requestId === confirmRequestId);
		const applied = Object.values(job.state.graph.nodes).find(
			(node) => node.domainRef === `guidance-request:${confirmRequestId}`,
		);
		if (
			!request ||
			current.pending.includes(confirmRequestId) ||
			applied?.statement !== `User guidance: ${request.guidance}`
		)
			throw new Error("research guidance application has not been confirmed");
	}
}
