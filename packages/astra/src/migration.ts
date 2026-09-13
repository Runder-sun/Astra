import { createHash } from "node:crypto";
import { access, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

export interface AstraMigrationReport {
	schemaVersion: "astra.migration_report.v1";
	status: "not-needed" | "imported" | "already-imported";
	source: string;
	target: string;
	readonlySource: true;
	importedFiles: string[];
	sourceInventory: Array<{ path: string; sha256: string; bytes: number }>;
	createdAt: string;
}

async function inventory(root: string, current = ""): Promise<Array<{ path: string; sha256: string; bytes: number }>> {
	const result: Array<{ path: string; sha256: string; bytes: number }> = [];
	for (const entry of await readdir(join(root, current), { withFileTypes: true })) {
		const relative = join(current, entry.name);
		if (entry.isDirectory()) {
			result.push(...(await inventory(root, relative)));
			continue;
		}
		const content = await readFile(join(root, relative));
		result.push({
			path: relative,
			sha256: createHash("sha256").update(content).digest("hex"),
			bytes: content.byteLength,
		});
	}
	return result;
}

export async function migratePmcli(workspaceRoot: string): Promise<AstraMigrationReport> {
	const sourceRoot = join(workspaceRoot, ".pmcli");
	const targetRoot = join(workspaceRoot, ".astra", "migrations");
	const reportPath = join(targetRoot, "pmcli-import-report.json");
	try {
		await access(reportPath);
		return JSON.parse(await readFile(reportPath, "utf8")) as AstraMigrationReport;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
	}
	let sourceExists = true;
	try {
		await access(sourceRoot);
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") sourceExists = false;
		else throw error;
	}
	const sourceInventory = sourceExists ? await inventory(sourceRoot) : [];
	const report: AstraMigrationReport = {
		schemaVersion: "astra.migration_report.v1",
		status: sourceExists ? "imported" : "not-needed",
		source: ".pmcli",
		target: ".astra",
		readonlySource: true,
		importedFiles: sourceExists ? sourceInventory.map((entry) => entry.path) : [],
		sourceInventory,
		createdAt: new Date().toISOString(),
	};
	await mkdir(targetRoot, { recursive: true });
	await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
	return report;
}
