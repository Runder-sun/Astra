use crate::support::{unique_temp_dir, write_text_file};
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn release_packaging_scripts_emit_cli_and_remote_app_archives() {
    let root = repo_root();
    let package_root = unique_temp_dir("release_packaging");
    let dist_dir = package_root.join("dist");
    let target_dir = package_root.join("target");
    let release_dir = target_dir.join("release");
    write_text_file(
        release_dir.join("research-cli"),
        "#!/usr/bin/env bash\nprintf 'research-cli test package\\n'\n",
    );
    write_text_file(
        release_dir.join("astra"),
        "#!/usr/bin/env bash\nprintf 'astra test package\\n'\n",
    );

    let remote_output = Command::new(root.join("scripts/package_remote_app.sh"))
        .current_dir(&root)
        .env("DIST_DIR", &dist_dir)
        .env("REMOTE_APP_SKIP_BUILD", "1")
        .output()
        .expect("remote app package script should execute");
    assert!(
        remote_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&remote_output.stderr)
    );
    let remote_archive = String::from_utf8(remote_output.stdout)
        .expect("remote package stdout should be utf8")
        .trim()
        .to_string();
    assert!(PathBuf::from(&remote_archive).exists());
    assert!(PathBuf::from(format!("{remote_archive}.sha256")).exists());
    assert!(dist_dir
        .join("astra-remote-app-0.1.0")
        .join("sw.js")
        .exists());
    assert!(dist_dir
        .join("astra-remote-app-0.1.0")
        .join("icons/icon.svg")
        .exists());
    assert!(dist_dir
        .join("astra-remote-app-0.1.0")
        .join("favicon.ico")
        .exists());

    let cli_output = Command::new(root.join("scripts/package_cli_release.sh"))
        .current_dir(&root)
        .env("DIST_DIR", &dist_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("PACKAGE_SKIP_BUILD", "1")
        .env("REMOTE_APP_SKIP_BUILD", "1")
        .output()
        .expect("cli package script should execute");
    assert!(
        cli_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cli_output.stderr)
    );
    let cli_archive = String::from_utf8(cli_output.stdout)
        .expect("cli package stdout should be utf8")
        .trim()
        .to_string();
    assert!(PathBuf::from(&cli_archive).exists());
    assert!(PathBuf::from(format!("{cli_archive}.sha256")).exists());
    let cli_package_dir = std::fs::read_dir(&dist_dir)
        .expect("dist dir should read")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("research-cli-0.1.0-"))
        })
        .expect("cli package directory should exist");
    assert!(cli_package_dir.join("bin/research-cli").exists());
    assert!(cli_package_dir.join("bin/astra").exists());
    assert!(cli_package_dir.join("share/astra-remote/sw.js").exists());
    assert!(cli_package_dir
        .join("share/astra-remote/favicon.ico")
        .exists());
    assert!(cli_package_dir
        .join("share/astra-remote/manifest.webmanifest")
        .exists());
}
