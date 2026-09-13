use crate::support::{init_git_workspace, unique_temp_dir, write_text_file};
use research_cli::projects::current::resolve_current_project;
use research_cli::projects::registry::{ProjectRegistry, ProjectRegistryEntry};
use research_cli::workspace::resolve::{ensure_project_data_dir, resolve_workspace_from};

#[test]
fn workspace_resolution_detects_root_and_bootstraps_pmcli() {
    let workspace_root = init_git_workspace("workspace_resolution");
    let nested = workspace_root.join("src").join("nested");
    std::fs::create_dir_all(&nested).expect("nested dir should exist");

    let binding = resolve_workspace_from(&nested).expect("workspace should resolve");
    ensure_project_data_dir(&binding).expect("data dir should bootstrap");

    assert_eq!(binding.workspace_root, workspace_root);
    assert!(!binding.workspace_hash.is_empty());
    assert_eq!(binding.data_dir, workspace_root.join(".pmcli"));
    assert!(binding.data_dir.is_dir());
}

#[test]
fn project_registry_round_trip_and_pointer_fallback_are_deterministic() {
    let registry_root = unique_temp_dir("registry_root");
    let workspace_root = init_git_workspace("project_a");
    let registry = ProjectRegistry::new(registry_root.join("registry"));
    let entry = ProjectRegistryEntry::new("project_a".to_string(), workspace_root.clone())
        .expect("entry should build");

    registry
        .register(entry.clone())
        .expect("register should succeed");
    registry
        .set_current_project(entry.to_pointer())
        .expect("pointer should save");

    let resolved = resolve_current_project(
        &registry,
        None,
        None,
        &unique_temp_dir("outside_registered_workspace"),
    )
    .expect("pointer fallback should resolve");

    assert_eq!(resolved.project_id, "project_a");
    assert_eq!(resolved.workspace_root, workspace_root);
    assert_eq!(resolved.resolution_source, "current_project_pointer");
}

#[test]
fn stale_current_project_pointer_fails_closed() {
    let registry_root = unique_temp_dir("registry_root_stale");
    let workspace_root = init_git_workspace("project_b");
    let registry = ProjectRegistry::new(registry_root.join("registry"));
    let entry = ProjectRegistryEntry::new("project_b".to_string(), workspace_root.clone())
        .expect("entry should build");

    registry
        .register(entry.clone())
        .expect("register should succeed");
    registry
        .set_current_project(entry.to_pointer())
        .expect("pointer should save");

    write_text_file(workspace_root.join("README.md"), "placeholder");
    std::fs::remove_dir_all(&workspace_root).expect("workspace should be removed");

    let err = resolve_current_project(
        &registry,
        None,
        None,
        &unique_temp_dir("outside_registered_workspace_again"),
    )
    .expect_err("stale pointer should fail");

    assert!(
        err.to_string().contains("stale current project pointer"),
        "unexpected error: {err}"
    );
}
