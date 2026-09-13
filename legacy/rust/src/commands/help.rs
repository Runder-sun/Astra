use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandSpec {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub slash_aliases: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub category: String,
    pub summary: String,
    pub supports_json_help: bool,
    pub canonical_json_invocation: Vec<String>,
    pub graduation_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HelpSurfaceReport {
    pub scope: String,
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaletteEntry {
    pub command: String,
    pub summary: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub category: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shortcut_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaletteSurfaceReport {
    pub scope: String,
    pub entries: Vec<PaletteEntry>,
}

pub fn command_registry() -> Vec<CommandSpec> {
    vec![
        command(
            "help",
            &["help"],
            "core",
            "Show command and slash help",
            &["help", "--json"],
            "graduated",
        ),
        command(
            "palette",
            &["help"],
            "core",
            "Export the canonical command-palette surface",
            &["palette", "--json"],
            "graduated",
        ),
        command(
            "slash",
            &["help"],
            "core",
            "Inspect the canonical slash-command help surface",
            &["slash", "help", "--json"],
            "graduated",
        ),
        command(
            "chat",
            &["help", "status"],
            "core",
            "Open the interactive chat lane",
            &["chat", "--json"],
            "graduated",
        ),
        command(
            "prompt",
            &[],
            "core",
            "Run one bounded prompt turn in non-interactive mode",
            &["prompt", "status", "--json"],
            "graduated",
        ),
        command(
            "model",
            &["model"],
            "provider",
            "Inspect and set default model selection",
            &["model", "current", "--json"],
            "graduated",
        ),
        command(
            "resume",
            &["session"],
            "session",
            "Resume a specific saved session",
            &["resume", "latest", "--json"],
            "graduated",
        ),
        command(
            "continue",
            &["status"],
            "session",
            "Resume the latest session in the current project",
            &["continue", "--json"],
            "graduated",
        ),
        command(
            "inspect",
            &["status", "session", "projects"],
            "session",
            "Inspect session or project runtime state",
            &["inspect", "--json"],
            "graduated",
        ),
        command(
            "compact",
            &["compact"],
            "session",
            "Compact a session into summary and recap artifacts",
            &["compact", "latest", "--json"],
            "graduated",
        ),
        command(
            "sessions",
            &["session"],
            "session",
            "Create, browse, rename, delete, prune, export, and inspect saved sessions",
            &["sessions", "list", "--json"],
            "graduated",
        ),
        command(
            "projects",
            &["projects"],
            "project",
            "Register, resolve, and inspect project scope",
            &["projects", "status", "--json"],
            "graduated",
        ),
        command(
            "permissions",
            &["permissions"],
            "ops",
            "Inspect pending/history permission state",
            &["permissions", "pending", "--json"],
            "graduated",
        ),
        command(
            "goals",
            &["goals"],
            "project",
            "Inspect, update, advance, tick, and watch project MissionFrame goal runs",
            &["goals", "status", "--json"],
            "graduated",
        ),
        command(
            "tools",
            &["tools"],
            "ops",
            "Run local tools through the permission gate",
            &["tools", "run", "read_file", "--path", "README.md", "--json"],
            "graduated",
        ),
        command(
            "providers",
            &["doctor"],
            "provider",
            "Inspect provider routing, auth, and catalog status",
            &["providers", "list", "--json"],
            "graduated",
        ),
        command(
            "config",
            &["permissions"],
            "config",
            "Inspect and mutate runtime configuration",
            &["config", "effective", "--json"],
            "graduated",
        ),
        command(
            "usage",
            &["usage"],
            "ops",
            "Inspect token and cost accounting",
            &["usage", "--json"],
            "graduated",
        ),
        command(
            "cost",
            &["cost"],
            "ops",
            "Inspect monetary usage estimate",
            &["cost", "--json"],
            "graduated",
        ),
        command(
            "stats",
            &["stats"],
            "ops",
            "Inspect top-level runtime stats",
            &["stats", "--json"],
            "graduated",
        ),
        command(
            "doctor",
            &["doctor"],
            "ops",
            "Run preflight diagnostics for the current project",
            &["doctor", "--json"],
            "graduated",
        ),
        command(
            "setup",
            &["doctor"],
            "ops",
            "Inspect install routes, migrations, and repair hints",
            &["setup", "status", "--json"],
            "graduated",
        ),
        command(
            "smoke",
            &["doctor"],
            "ops",
            "Run mutation-safe smoke validation for the current project",
            &["smoke", "--json"],
            "graduated",
        ),
        command(
            "conformance",
            &["doctor"],
            "ops",
            "Inspect local conformance fixture family status",
            &["conformance", "--json"],
            "graduated",
        ),
        command(
            "mcp",
            &["mcp"],
            "ops",
            "Inspect MCP registry, health, and refresh state",
            &["mcp", "list", "--json"],
            "graduated",
        ),
        command(
            "skills",
            &["skills"],
            "ops",
            "Inspect, run, validate, evolve, and publish governed skill outputs",
            &["skills", "list", "--json"],
            "graduated",
        ),
        command(
            "plugins",
            &["plugin", "plugins"],
            "ops",
            "Inspect plugin registry, manifests, and validation",
            &["plugins", "list", "--json"],
            "graduated",
        ),
        command(
            "hooks",
            &["hooks"],
            "ops",
            "Inspect hook inventory and dry-run validation",
            &["hooks", "list", "--json"],
            "graduated",
        ),
        command(
            "memory",
            &["memory"],
            "advanced",
            "Inspect project memory surfaces and status",
            &["memory", "status", "--json"],
            "graduated",
        ),
        command(
            "feedback",
            &["feedback"],
            "advanced",
            "Collect advisory learning signals and calibrate runtime recommendations",
            &["feedback", "status", "--json"],
            "graduated",
        ),
        command(
            "agents",
            &["agents"],
            "advanced",
            "Run packet-bound agent lifecycle proof harnesses",
            &["agents", "list", "--json"],
            "graduated",
        ),
        command(
            "branches",
            &["branches"],
            "advanced",
            "Run Git-native branch mutation, evaluation, debate, verifier tournament, and winner merge gates",
            &[
                "branches",
                "verify",
                "branch_a",
                "--against",
                "branch_b",
                "--json",
            ],
            "graduated",
        ),
        command(
            "artifacts",
            &["artifacts"],
            "advanced",
            "Inspect artifact families and canonical pointers",
            &["artifacts", "list", "--json"],
            "graduated",
        ),
        command(
            "repo",
            &["cleanup"],
            "advanced",
            "Plan, apply, and restore reversible repository cleanup",
            &["repo", "cleanup-plan", "--json"],
            "graduated",
        ),
        command(
            "projectops",
            &["projectops"],
            "advanced",
            "Inspect and drive experiment supervision leases and wake transitions",
            &["projectops", "status", "--json"],
            "graduated",
        ),
        command(
            "routines",
            &["routines"],
            "advanced",
            "Define, ingest, run, and retry background routines through agents and ProjectOps supervision",
            &["routines", "list", "--json"],
            "graduated",
        ),
        command(
            "host",
            &["host"],
            "advanced",
            "Inspect shared host-surface projections for local TUI, web, and mobile clients",
            &["host", "surface", "status", "--json"],
            "graduated",
        ),
        command(
            "tui",
            &["tui"],
            "advanced",
            "Launch and inspect projection-first local terminal UI surfaces",
            &["tui", "snapshot", "--json"],
            "graduated",
        ),
        command(
            "remote",
            &["remote"],
            "advanced",
            "Pair, inspect, control, serve, and project mobile remote access",
            &["remote", "status", "--json"],
            "graduated",
        ),
        command(
            "research",
            &["research"],
            "advanced",
            "Run autonomous research from a prompt and inspect threads, decisions, and stage graph state",
            &[
                "research",
                "run",
                "--prompt",
                "...",
                "--automation-mode",
                "full_auto",
                "--json",
            ],
            "graduated",
        ),
        command(
            "reviews",
            &["review"],
            "advanced",
            "Open, list, and inspect review packets and traces",
            &["reviews", "list", "--json"],
            "graduated",
        ),
        command(
            "docs",
            &["docs"],
            "advanced",
            "Index, inspect, and refresh document DocFrame blocks",
            &["docs", "index", "--json"],
            "graduated",
        ),
    ]
}

pub fn help_surface_report(scope: &str) -> HelpSurfaceReport {
    HelpSurfaceReport {
        scope: scope.to_string(),
        commands: command_registry(),
    }
}

pub fn palette_surface_report(scope: &str) -> PaletteSurfaceReport {
    PaletteSurfaceReport {
        scope: scope.to_string(),
        entries: command_registry()
            .into_iter()
            .map(|command| PaletteEntry {
                command: command.name,
                summary: command.summary,
                category: command.category,
                shortcut_hint: String::new(),
            })
            .collect(),
    }
}

pub fn find_command(name: &str) -> Option<CommandSpec> {
    command_registry()
        .into_iter()
        .find(|spec| spec.name == name)
}

pub fn render_help_text(scope: &str) -> String {
    let report = help_surface_report(scope);
    let mut lines = vec![
        "astra".to_string(),
        String::new(),
        "Available commands:".to_string(),
    ];

    for command in report.commands {
        lines.push(format!("  {:<10} {}", command.name, command.summary));
    }

    lines.push(String::new());
    lines.push("Use `astra help --json` for machine-readable help.".to_string());
    lines.join("\n")
}

fn command(
    name: &str,
    slash_aliases: &[&str],
    category: &str,
    summary: &str,
    canonical_json_invocation: &[&str],
    graduation_status: &str,
) -> CommandSpec {
    CommandSpec {
        name: name.to_string(),
        slash_aliases: slash_aliases
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        category: category.to_string(),
        summary: summary.to_string(),
        supports_json_help: true,
        canonical_json_invocation: canonical_json_invocation
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        graduation_status: graduation_status.to_string(),
    }
}
