use std::fs;
use std::path::PathBuf;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

static ISOLATE_CLI_CONFIG_HOME: Once = Once::new();

fn ensure_isolated_cli_config_home() {
    ISOLATE_CLI_CONFIG_HOME.call_once(|| {
        let root = std::env::temp_dir().join(format!(
            "research_cli_test_config_home_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test config home should be created");
        fs::write(root.join("settings.json"), "{}\n").expect("isolated config should write");
        std::env::set_var("RESEARCH_CLI_CONFIG_HOME", &root);
        std::env::set_var("XDG_CONFIG_HOME", &root);
    });
}

pub fn unique_temp_dir(label: &str) -> PathBuf {
    ensure_isolated_cli_config_home();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    let dir = std::env::temp_dir().join(format!("research_cli_{label}_{nonce}"));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

pub fn init_git_workspace(label: &str) -> PathBuf {
    ensure_isolated_cli_config_home();
    let root = unique_temp_dir(label);
    fs::create_dir_all(root.join(".git")).expect("git marker should be created");
    root
}

pub fn write_text_file(path: impl Into<PathBuf>, contents: &str) {
    let path = path.into();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dirs should be created");
    }
    fs::write(path, contents).expect("file should be written");
}

pub fn isolate_cli_env(command: &mut std::process::Command) {
    command
        .env_remove("RESEARCH_CLI_CONFIG_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RESEARCH_CLI_PROVIDER")
        .env_remove("RESEARCH_CLI_MODEL")
        .env_remove("RESEARCH_CLI_PROVIDER_FAILOVER")
        .env_remove("RESEARCH_CLI_PERMISSION_MODE");
}
