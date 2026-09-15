const copyButton = document.getElementById("copy-command");
copyButton?.addEventListener("click", async () => {
	const code = document.getElementById("commands");
	const status = document.getElementById("copy-status");
	if (!code || !status) return;
	try {
		await navigator.clipboard.writeText(code.textContent ?? "");
		status.textContent = "命令已复制。";
	} catch {
		status.textContent = "无法访问剪贴板，请选中上方命令手动复制。";
	}
});
