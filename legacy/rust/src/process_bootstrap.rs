use std::env;
use std::path::{Path, PathBuf};

const CLI_BINARY_ENV: &str = "RESEARCH_CLI_BIN";

pub fn current_cli_executable() -> PathBuf {
    if let Some(path) = env::var_os(CLI_BINARY_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return path;
    }

    let raw = env::current_exe().unwrap_or_else(|_| PathBuf::from("research-cli"));
    normalize_deleted_executable_path(raw)
}

pub fn propagate_current_cli_executable(command: &mut std::process::Command, exe: &Path) {
    command.env(CLI_BINARY_ENV, exe);
}

fn normalize_deleted_executable_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    let Some(stripped) = text.strip_suffix(" (deleted)") else {
        return path;
    };
    let candidate = PathBuf::from(stripped);
    if candidate.exists() {
        return candidate;
    }
    path
}
