import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import type { OutputRef } from "./types.ts";

/** Validate declared files after shared path, size and checksum validation. */
export async function validatePaperDelivery(
	content: Record<string, unknown>,
	refs: OutputRef[],
	root: string,
): Promise<void> {
	const requireRef = (value: unknown, field: string, kind: "artifact" | "log"): OutputRef => {
		if (typeof value !== "string" || !value.trim() || isAbsolute(value))
			throw new Error(`paper-compile ${field} must be a relative file path`);
		const path = relative(root, resolve(root, value)).split("\\").join("/");
		const ref = refs.find((ref) => ref.kind === kind && ref.ref === path);
		if (!ref) throw new Error(`paper-compile ${field} requires a matching ${kind} ref: ${value}`);
		return ref;
	};
	const pdf = requireRef(content.artifact, "artifact", "artifact");
	const log = requireRef(content.buildLog, "buildLog", "log");
	const source = requireRef(content.source, "source", "artifact");
	if (source.ref === pdf.ref || source.ref === log.ref || /\.pdf$/i.test(source.ref))
		throw new Error("paper-compile source must be an editable compilation entry point");
	if (typeof content.command !== "string" || !content.command.trim())
		throw new Error("paper-compile command is required");
	if (!Array.isArray(content.buildInputs) || !content.buildInputs.length)
		throw new Error("paper-compile buildInputs must list required local inputs including source");
	const inputs = content.buildInputs.map((value) => requireRef(value, "buildInputs", "artifact"));
	if (!inputs.some((ref) => ref.ref === source.ref)) throw new Error("paper-compile buildInputs omits source");
	let pdfBytes: Buffer | undefined;
	for (const ref of [pdf, log, source]) {
		const bytes = await readFile(resolve(root, ref.ref));
		if (!bytes.length) throw new Error(`paper-compile file is empty: ${ref.ref}`);
		if (createHash("sha256").update(bytes).digest("hex") !== ref.sha256)
			throw new Error(`paper-compile file changed during validation: ${ref.ref}`);
		if (ref === pdf) pdfBytes = bytes;
	}
	await new Promise<void>((ok, reject) => {
		const child = execFile(
			"pdfinfo",
			["-"],
			{ env: { ...process.env, LC_ALL: "C" }, timeout: 30000, maxBuffer: 1024 * 1024 },
			(error, stdout, stderr) => {
				if ((error as NodeJS.ErrnoException | null)?.code === "ENOENT")
					return reject(
						new Error("PDF validation requires pdfinfo (Poppler); install it before paper compilation"),
					);
				if (error || stderr.trim() || !/^Pages:\s+[1-9]\d*\s*$/m.test(stdout))
					return reject(
						new Error(`PDF parsing failed: ${stderr.trim() || error?.message || "no readable pages"}`),
					);
				ok();
			},
		);
		child.stdin?.on("error", (error: NodeJS.ErrnoException) => {
			if (error.code !== "EPIPE") reject(error);
		});
		child.stdin?.end(pdfBytes);
	});
}
