use crate::mcp;
use crate::plugins;
use crate::skills;
use crate::workspace::resolve::resolve_workspace_from;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct SchemaVersion {
    pub schema: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataDirectoryStatus {
    pub kind: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationStatus {
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallRoutesReport {
    pub platform: String,
    pub state_home: String,
    pub registry_root: String,
    pub pmcli_root: String,
    pub schema_root: String,
    pub providers: Vec<String>,
    pub skills: Vec<String>,
    pub mcp: Vec<String>,
    pub plugins: Vec<String>,
    pub hooks: Vec<String>,
    pub remote: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusReport {
    pub overall_status: String,
    pub state_home: String,
    pub install_routes: InstallRoutesReport,
    pub schema_versions: Vec<SchemaVersion>,
    pub data_directories: Vec<DataDirectoryStatus>,
    pub last_migration: MigrationStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrateCheckReport {
    pub overall_status: String,
    pub checked_roots: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pending_migrations: Vec<String>,
    pub incompatible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairHintComponent {
    pub component: String,
    pub status: String,
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairHintsReport {
    pub overall_status: String,
    pub components: Vec<RepairHintComponent>,
    pub total_count: usize,
}

pub fn install_routes(state_home: &Path, cwd: &Path) -> InstallRoutesReport {
    let workspace_root = resolve_workspace_from(cwd)
        .map(|binding| binding.workspace_root)
        .unwrap_or_else(|_| cwd.to_path_buf());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    InstallRoutesReport {
        platform: std::env::consts::OS.to_string(),
        state_home: state_home.display().to_string(),
        registry_root: state_home.join("registry").display().to_string(),
        pmcli_root: workspace_root.join(".pmcli").display().to_string(),
        schema_root: repo_root.join("schemas").display().to_string(),
        providers: vec![
            "OPENAI_API_KEY".to_string(),
            "ANTHROPIC_API_KEY".to_string(),
            state_home.join("providers").display().to_string(),
        ],
        skills: skills::search_paths(state_home, cwd),
        mcp: mcp::search_paths(state_home, cwd),
        plugins: plugins::plugin_search_paths_for_display(state_home, cwd),
        hooks: plugins::plugin_search_paths_for_display(state_home, cwd)
            .into_iter()
            .map(|path| {
                PathBuf::from(path)
                    .join("*")
                    .join("hooks")
                    .display()
                    .to_string()
            })
            .collect(),
        remote: vec![
            state_home.join("remote").display().to_string(),
            workspace_root
                .join(".pmcli")
                .join("remote")
                .display()
                .to_string(),
            workspace_root.join(".happy").display().to_string(),
        ],
    }
}

pub fn status(state_home: &Path, cwd: &Path) -> SetupStatusReport {
    let routes = install_routes(state_home, cwd);
    let migration_path = state_home.join("setup").join("last_migration.json");
    let migration_status = if migration_path.exists() {
        MigrationStatus {
            status: "recorded".to_string(),
            detail: migration_path.display().to_string(),
            updated_at: String::new(),
        }
    } else {
        MigrationStatus {
            status: "not_started".to_string(),
            detail: "no migration record found".to_string(),
            updated_at: String::new(),
        }
    };

    let data_directories = vec![
        DataDirectoryStatus {
            kind: "state_home".to_string(),
            path: state_home.display().to_string(),
            exists: state_home.exists(),
        },
        DataDirectoryStatus {
            kind: "registry".to_string(),
            path: state_home.join("registry").display().to_string(),
            exists: state_home.join("registry").exists(),
        },
        DataDirectoryStatus {
            kind: "pmcli".to_string(),
            path: routes.pmcli_root.clone(),
            exists: Path::new(&routes.pmcli_root).exists(),
        },
        DataDirectoryStatus {
            kind: "schemas".to_string(),
            path: routes.schema_root.clone(),
            exists: Path::new(&routes.schema_root).exists(),
        },
    ];

    SetupStatusReport {
        overall_status: if data_directories.iter().all(|entry| entry.exists) {
            "ready".to_string()
        } else {
            "degraded".to_string()
        },
        state_home: state_home.display().to_string(),
        install_routes: routes,
        schema_versions: schema_versions(),
        data_directories,
        last_migration: migration_status,
    }
}

pub fn migrate_check(state_home: &Path, cwd: &Path) -> MigrateCheckReport {
    let checked_roots = vec![
        state_home.display().to_string(),
        resolve_workspace_from(cwd)
            .map(|binding| binding.data_dir.display().to_string())
            .unwrap_or_else(|_| cwd.join(".pmcli").display().to_string()),
    ];
    MigrateCheckReport {
        overall_status: "ready".to_string(),
        checked_roots,
        pending_migrations: Vec::new(),
        incompatible: false,
    }
}

pub fn repair_hints(components: Vec<RepairHintComponent>) -> RepairHintsReport {
    let total_count = components
        .iter()
        .map(|component| component.hints.len())
        .sum();
    let overall_status = if total_count == 0 {
        "ready".to_string()
    } else {
        "degraded".to_string()
    };
    RepairHintsReport {
        overall_status,
        components,
        total_count,
    }
}

fn schema_versions() -> Vec<SchemaVersion> {
    let schema_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    let mut versions = std::fs::read_dir(schema_root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return None;
            }
            Some(SchemaVersion {
                schema: path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string(),
                version: "v1alpha1".to_string(),
            })
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| left.schema.cmp(&right.schema));
    versions
}
