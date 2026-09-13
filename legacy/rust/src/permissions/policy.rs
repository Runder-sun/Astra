use serde::Serialize;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PermissionMode {
    #[serde(rename = "read-only")]
    ReadOnly,
    #[serde(rename = "workspace-write")]
    WorkspaceWrite,
    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
}

impl PermissionMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "read-only" => Some(Self::ReadOnly),
            "workspace-write" => Some(Self::WorkspaceWrite),
            "danger-full-access" => Some(Self::DangerFullAccess),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

impl fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionPolicy {
    pub mode: PermissionMode,
    pub workspace_root: String,
    pub blocked_patterns: Vec<String>,
}

impl PermissionPolicy {
    pub fn new<P>(mode: PermissionMode, workspace_root: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            mode,
            workspace_root: workspace_root.as_ref().display().to_string(),
            blocked_patterns: default_blocked_patterns(),
        }
    }

    pub fn evaluate(&self, tool: &ToolSpec, target_path: Option<&str>) -> PermissionCheckResult {
        if let Some(path) = target_path {
            if self.mode != PermissionMode::DangerFullAccess {
                if let Some(blocked) = self.blocked_patterns.iter().find(|pat| path.contains(*pat))
                {
                    return PermissionCheckResult {
                        allowed: false,
                        reason: format!("path matches blocked pattern: {blocked}"),
                        requires_approval: false,
                        mode: self.mode,
                        reason_code: "blocked_pattern".to_string(),
                        workspace_boundary_ok: false,
                    };
                }
            } else if let Some(blocked) = self
                .blocked_patterns
                .iter()
                .find(|pat| path_accesses_blocked_system_path(path, pat))
            {
                return PermissionCheckResult {
                    allowed: false,
                    reason: format!("path matches blocked pattern: {blocked}"),
                    requires_approval: false,
                    mode: self.mode,
                    reason_code: "blocked_pattern".to_string(),
                    workspace_boundary_ok: false,
                };
            }
            if self.mode != PermissionMode::DangerFullAccess {
                if let Err(reason) = self.check_workspace_boundary(path) {
                    return PermissionCheckResult {
                        allowed: false,
                        reason,
                        requires_approval: false,
                        mode: self.mode,
                        reason_code: "workspace_scope_violation".to_string(),
                        workspace_boundary_ok: false,
                    };
                }
            }
        }

        match self.mode {
            PermissionMode::ReadOnly => {
                if tool.classification == ToolClassification::ReadOnly {
                    PermissionCheckResult {
                        allowed: true,
                        reason: "read-only tool permitted under read-only mode".to_string(),
                        requires_approval: false,
                        mode: self.mode,
                        reason_code: "allowed_read_only".to_string(),
                        workspace_boundary_ok: true,
                    }
                } else {
                    PermissionCheckResult {
                        allowed: false,
                        reason: format!(
                            "mutating tool '{}' blocked under read-only mode",
                            tool.name
                        ),
                        requires_approval: true,
                        mode: self.mode,
                        reason_code: "approval_required".to_string(),
                        workspace_boundary_ok: true,
                    }
                }
            }
            PermissionMode::WorkspaceWrite => {
                if tool.classification == ToolClassification::Destructive {
                    PermissionCheckResult {
                        allowed: false,
                        reason: format!(
                            "destructive tool '{}' requires approval under workspace-write mode",
                            tool.name
                        ),
                        requires_approval: true,
                        mode: self.mode,
                        reason_code: "destructive_approval_required".to_string(),
                        workspace_boundary_ok: true,
                    }
                } else {
                    PermissionCheckResult {
                        allowed: true,
                        reason: format!("tool '{}' permitted inside workspace", tool.name),
                        requires_approval: false,
                        mode: self.mode,
                        reason_code: "allowed_workspace".to_string(),
                        workspace_boundary_ok: true,
                    }
                }
            }
            PermissionMode::DangerFullAccess => PermissionCheckResult {
                allowed: true,
                reason: format!("tool '{}' permitted under danger-full-access", tool.name),
                requires_approval: false,
                mode: self.mode,
                reason_code: "allowed_danger_full_access".to_string(),
                workspace_boundary_ok: true,
            },
        }
    }

    pub fn check_action(
        &self,
        tool_name: &str,
        classification: ToolClassification,
        target_path: Option<&str>,
    ) -> PermissionCheckResult {
        self.evaluate(&ToolSpec::new(tool_name, classification), target_path)
    }

    fn check_workspace_boundary(&self, target_path: &str) -> Result<(), String> {
        let target = Path::new(target_path);
        if target.is_absolute() {
            return Err(format!(
                "absolute target path is outside workspace: {}",
                target.display()
            ));
        }

        let workspace_root = PathBuf::from(&self.workspace_root);
        for component in target.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err(format!(
                    "target path escapes workspace boundary: {}",
                    workspace_root.join(target).display()
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ToolClassification {
    ReadOnly,
    Mutating,
    Destructive,
}

impl ToolClassification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Mutating => "mutating",
            Self::Destructive => "destructive",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub classification: ToolClassification,
}

impl ToolSpec {
    pub fn new(name: &str, classification: ToolClassification) -> Self {
        Self {
            name: name.to_string(),
            classification,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionCheckResult {
    pub allowed: bool,
    pub reason: String,
    pub requires_approval: bool,
    pub mode: PermissionMode,
    pub reason_code: String,
    pub workspace_boundary_ok: bool,
}

fn default_blocked_patterns() -> Vec<String> {
    vec![
        "/etc/".to_string(),
        "/proc/".to_string(),
        "/sys/".to_string(),
        "/dev/".to_string(),
        "/root/".to_string(),
    ]
}

fn path_accesses_blocked_system_path(path: &str, pattern: &str) -> bool {
    for token in path.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '\'' | '"'
                    | '`'
                    | '<'
                    | '>'
                    | '|'
                    | '&'
                    | ';'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | ','
            )
    }) {
        let cleaned = token.trim_start_matches(['-', '=']);
        if cleaned == "/dev/null" {
            continue;
        }
        if cleaned.starts_with(pattern) {
            return true;
        }
    }
    false
}
