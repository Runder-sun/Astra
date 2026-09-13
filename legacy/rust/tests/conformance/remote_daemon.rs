use crate::support::{init_git_workspace, unique_temp_dir};
use research_cli::remote::daemon::{handle_request, DaemonContext, DaemonError, DaemonRequest};
use research_cli::session::store::SessionStore;
use research_cli::session::transcript::TranscriptLine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

static NEXT_DAEMON_TEST_PORT: AtomicU16 = AtomicU16::new(29000);

fn cargo_bin() -> String {
    std::env::var("CARGO_BIN_EXE_research-cli").expect("cargo should expose built binary path")
}

fn output_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid json")
}

fn read_json_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .expect("jsonl path should be readable")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("line should contain valid json"))
        .collect()
}

fn init_real_git_workspace(label: &str) -> std::path::PathBuf {
    let root = unique_temp_dir(label);
    let init = Command::new("git")
        .current_dir(&root)
        .args(["init", "-q"])
        .status()
        .expect("git init should execute");
    assert!(init.success());
    let config_name = Command::new("git")
        .current_dir(&root)
        .args(["config", "user.name", "Research CLI Test"])
        .status()
        .expect("git config user.name should execute");
    assert!(config_name.success());
    let config_email = Command::new("git")
        .current_dir(&root)
        .args(["config", "user.email", "research-cli@example.test"])
        .status()
        .expect("git config user.email should execute");
    assert!(config_email.success());
    std::fs::write(root.join("README.md"), "# test workspace\n").expect("README should write");
    let add = Command::new("git")
        .current_dir(&root)
        .args(["add", "README.md"])
        .status()
        .expect("git add should execute");
    assert!(add.success());
    let commit = Command::new("git")
        .current_dir(&root)
        .args(["commit", "-q", "-m", "initial"])
        .status()
        .expect("git commit should execute");
    assert!(commit.success());
    root
}

#[test]
fn remote_daemon_serves_installable_mobile_web_app() {
    let workspace_root = init_git_workspace("remote_daemon_web_app");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root);

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("daemon request should be handled");

    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "text/html; charset=utf-8");
    let html = String::from_utf8(response.body).expect("html should be utf8");
    assert!(html.contains("<html lang=\"zh\" data-theme=\"light\">"));
    assert!(html.contains("<title>Astra Remote</title>"));
    assert!(html.contains("/app.js"));
    assert!(html.contains("/manifest.webmanifest"));
    assert!(html.contains("mobile-web-app-capable"));
    assert!(html.contains("/icons/icon.svg"));
    assert!(html.contains("<div id=\"app\"></div>"));
    assert!(html.contains("Astra Remote requires JavaScript."));
    assert!(!html.contains("id=\"page-HOME\""));
    assert!(!html.contains("id=\"remote-root\""));
    assert!(!html.contains("Agent workspace</h1>"));

    let app_js = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/app.js".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("app js request should be handled");
    assert_eq!(app_js.status, 200);
    let js = String::from_utf8(app_js.body).expect("app js should be utf8");
    assert!(js.contains("createRoot"));
    assert!(js.contains("document.getElementById(\"app\")"));
    assert!(js.contains("function RemoteApp"));
    assert!(js.contains("id: \"remote-root\""));
    assert!(js.contains("id: \"page-HOME\""));
    assert!(js.contains("id: \"page-CHAT\""));
    assert!(js.contains("id: \"page-KANBAN\""));
    assert!(js.contains("id: \"page-TERMINAL\""));
    assert!(js.contains("id: \"page-NEWSESSION\""));
    assert!(js.contains("id: \"hermes-board-shell\""));
    assert!(js.contains("id: \"research-inbox-summary\""));
    assert!(js.contains("id: \"research-inbox-list\""));
    assert!(js.contains("id: \"hermes-board-columns\""));
    assert!(js.contains("id: \"research-card-drawer\""));
    assert!(js.contains("id: \"new-session-btn\""));
    assert!(js.contains("id: \"mobile-settings-btn\""));
    assert!(js.contains("id: \"add-server-btn\""));
    assert!(js.contains("id: \"conversation-composer\""));
    assert!(js.contains("id: \"kanban-composer\""));
    assert!(js.contains("id: \"perm-mode-badge\""));
    assert!(js.contains("id: \"perm-mode-sheet\""));
    assert!(js.contains("id: \"mobile-settings-sheet\""));
    assert!(js.contains("id: \"terminal-screen\""));
    assert!(js.contains("id: \"terminal-input\""));
    assert!(js.contains("data-terminal-input"));
    assert!(js.contains("ctrl_c"));
    assert!(js.contains("escape"));
    assert!(js.contains("id: \"new-session-start-btn\""));
    assert!(js.contains("name: \"session-permission-mode\""));
    assert!(js.contains("data-theme-choice"));
    assert!(js.contains("data-lang-choice"));
    assert!(js.contains("function createSession"));
    assert!(js.contains("function setTheme"));
    assert!(js.contains("function setLanguage"));
    assert!(js.contains("function changePermissionMode"));
    assert!(js.contains("function submitPermissionDecision"));
    assert!(js.contains("function selectServer"));
    assert!(js.contains("function probeServerHealth"));
    assert!(js.contains("function quickPairAndConnect"));
    assert!(js.contains("function parseRemoteEventStream"));
    assert!(js.contains("function handleRemoteEvent"));
    assert!(js.contains("function registerInstallableAppShell"));
    assert!(js.contains("function initialPageFromLocation"));
    assert!(js.contains("serviceWorker"));
    assert!(js.contains("register(\"/sw.js\"") || js.contains("register('/sw.js'"));
    assert!(js.contains("function renderResearchInbox"));
    assert!(js.contains("function renderHermesBoardColumns"));
    assert!(js.contains("function openResearchCardDrawer"));
    assert!(js.contains("function showCommandPalette"));
    assert!(js.contains("function loadSkills"));
    assert!(js.contains("function transcriptLineElement"));
    assert!(js.contains("function attachTerminal"));
    assert!(js.contains("/api/skills"));
    assert!(js.contains("/api/terminal/replay"));
    assert!(js.contains("await refresh();"));
    assert!(js.contains("/api/session/create"));
    assert!(js.contains("/api/session/permission-mode"));
    assert!(js.contains("/api/session/transcript"));
    assert!(js.contains("/api/quick-pair"));
    assert!(js.contains("/api/events"));
    assert!(js.contains("isControlTokenSetupRequired"));
    assert!(js.contains("remote_daemon_control_token_required"));
    assert!(js.contains("research-remote-control-token-v1"));
    assert!(js.contains("remember-control-token"));
    assert!(js.contains("startRemoteRuntime"));
    assert!(js.contains("showActionToast"));
    assert!(js.contains("connectTerminalBridge"));
    assert!(js.contains("new WebSocket"));
    assert!(js.contains("/api/terminal/ws"));
    assert!(js.contains("/api/terminal/ws-ticket"));
    assert!(js.contains("pty_output"));
    assert!(js.contains("appendPtyBytes"));
    assert!(js.contains("chunk_base64"));
    assert!(js.contains("TextDecoder"));
    assert!(js.contains("sendTerminalInput"));
    assert!(js.contains("sendTerminalPreset"));
    assert!(js.contains("resizeTerminal"));
    assert!(js.contains("sendTerminalSignal"));
    assert!(js.contains("/api/terminal/signal"));
    assert!(js.contains("remote_message_delta"));
    assert!(js.contains("renderStreamingAssistantDraft"));
    assert!(js.contains("sawRemoteEvents ? 30 : 250"));
    assert!(js.contains("research-remote-servers-v1"));
    assert!(js.contains("activeApiBase"));
    assert!(js.contains("normalizeServerUrl"));
    assert!(js.contains("selectServer"));
    assert!(js.contains("probeServerHealth"));
    assert!(js.contains("function resetActiveServerRuntime"));
    assert!(js.contains("terminalBridge.close("));
    assert!(js.contains("active server changed"));
    assert!(js.contains("eventsCursor = 0"));

    let styles = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/styles.css".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("styles request should be handled");
    assert_eq!(styles.status, 200);
    let css = String::from_utf8(styles.body).expect("styles should be utf8");
    assert!(css.contains("--surface: #fafaf9"));
    assert!(css.contains("[data-theme=\"dark\"]"));
    assert!(css.contains(".page-topbar"));
    assert!(css.contains(".page-bottombar"));
    assert!(css.contains(".hermes-board-shell"));
    assert!(css.contains(".research-inbox-card"));
    assert!(css.contains(".hermes-board-columns"));
    assert!(css.contains(".research-card-drawer"));
    assert!(css.contains("[data-page][hidden]"));
    assert!(css.contains(".bottom-sheet"));
    assert!(css.contains(".settings-group"));
    assert!(css.contains(".choice-row"));
    assert!(css.contains(".composer-bar"));
    assert!(css.contains(".command-palette"));
    assert!(css.contains(".command-palette-item"));
    assert!(css.contains(".shortcut-keys-row"));
    assert!(css.contains(".terminal-screen"));
    assert!(css.contains(".tool-row"));
    assert!(css.contains(".thinking-indicator"));
    assert!(css.contains(".approval-card"));
    assert!(css.contains("position: fixed"));
    assert!(css.contains(".pill[data-ready=\"false\"]"));

    let manifest = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/manifest.webmanifest".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("manifest request should be handled");
    assert_eq!(manifest.status, 200);
    assert_eq!(
        manifest.content_type,
        "application/manifest+json; charset=utf-8"
    );
    let manifest_json: serde_json::Value =
        serde_json::from_slice(&manifest.body).expect("manifest should parse");
    assert_eq!(manifest_json["display"], "standalone");
    assert_eq!(manifest_json["icons"][0]["src"], "/icons/icon.svg");
    assert_eq!(manifest_json["shortcuts"][0]["url"], "/?page=KANBAN");

    let sw = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/sw.js".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("service worker request should be handled");
    assert_eq!(sw.status, 200);
    let sw_js = String::from_utf8(sw.body).expect("service worker should be utf8");
    assert!(sw_js.contains("CACHE_NAME"));
    assert!(sw_js.contains("APP_SHELL"));
    assert!(sw_js.contains("url.pathname.startsWith('/api/')"));

    let icon = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/icons/icon.svg".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("app icon request should be handled");
    assert_eq!(icon.status, 200);
    assert_eq!(icon.content_type, "image/svg+xml");
    let icon_svg = String::from_utf8(icon.body).expect("icon should be utf8");
    assert!(icon_svg.contains("viewBox=\"0 0 512 512\""));
}

#[test]
fn remote_mobile_web_app_assets_expose_theme_language_and_session_controls() {
    let workspace_root = init_git_workspace("remote_mobile_controls");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root);

    let html_response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("mobile web app should be served");
    assert_eq!(html_response.status, 200);
    let html = String::from_utf8(html_response.body).expect("html should be utf8");
    assert!(html.contains("<div id=\"app\"></div>"));
    assert!(html.contains("/app.js"));
    assert!(!html.contains("id=\"theme-toggle-btn\""));
    assert!(!html.contains("id=\"page-NEWSESSION\""));

    let js_response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/app.js".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("mobile app js should be served");
    assert_eq!(js_response.status, 200);
    let js = String::from_utf8(js_response.body).expect("js should be utf8");
    assert!(js.contains("id: \"theme-toggle-btn\""));
    assert!(js.contains("id: \"mobile-settings-sheet\""));
    assert!(js.contains("\"data-theme-choice\": \"light\""));
    assert!(js.contains("\"data-lang-choice\": \"zh\""));
    assert!(js.contains("id: \"new-session-btn\""));
    assert!(js.contains("id: \"page-NEWSESSION\""));
    assert!(js.contains("id: \"new-session-start-btn\""));
    assert!(js.contains("name: \"session-permission-mode\""));
    assert!(js.contains("id: \"hermes-board-shell\""));
    assert!(js.contains("id: \"research-card-drawer\""));
    assert!(js.contains("function createSession"));
    assert!(js.contains("/api/session/create"));
    assert!(js.contains("/api/session/permission-mode"));
    assert!(js.contains("more_lines"));
    assert!(js.contains("setLanguage(currentLang())"));
    assert!(js.contains("setTheme(currentTheme())"));
    assert!(js.contains("renderResearchInbox"));
    assert!(js.contains("prefers-color-scheme: dark"));
}

#[test]
fn remote_mobile_runtime_bootstrap_probes_before_requiring_control_token() {
    let workspace_root = init_git_workspace("remote_mobile_no_token_bootstrap");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root);

    let js_response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/app.js".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("mobile app js should be served");
    assert_eq!(js_response.status, 200);
    let js = String::from_utf8(js_response.body).expect("js should be utf8");
    assert!(js.contains("async function startRemoteRuntime()"));
    assert!(js.contains("async function bootstrapRemoteRuntimeWithoutToken()"));
    assert!(js.contains("await probeServerHealth(activeApiBase());"));
    assert!(js.contains("if (await refresh() === false) return;"));
    let start_remote_runtime = js
        .split("async function startRemoteRuntime()")
        .nth(1)
        .and_then(|body| {
            body.split("async function bootstrapRemoteRuntimeWithoutToken()")
                .next()
        })
        .expect("startRemoteRuntime should precede no-token bootstrap");
    assert!(
        !start_remote_runtime.contains("showMobileTokenPrompt();"),
        "startup should not prompt for a token before probing whether the daemon requires one"
    );

    let css_response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/styles.css".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("styles should be served");
    assert_eq!(css_response.status, 200);
    let css = String::from_utf8(css_response.body).expect("styles should be utf8");
    assert!(css.contains("#mobile-token-prompt"));
    assert!(css.contains("overflow-x: hidden"));
    assert!(css.contains("width: min(100%, 360px)"));
    assert!(css.contains("max-width: calc(100vw - 32px)"));
    assert!(css.contains("min-width: 0"));
    assert!(css.contains("@media (max-width: 600px)"));
    assert!(css.contains("justify-content: flex-start"));
}

#[test]
fn remote_daemon_creates_mobile_web_app_session_with_permission_mode() {
    let workspace_root = init_git_workspace("remote_daemon_session_create");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let created = daemon_json(
        &context,
        "POST",
        "/api/session/create",
        json!({
            "cwd": workspace_root,
            "title": "Mobile start",
            "permission_mode": "workspace-write"
        }),
    );
    assert_eq!(created["ok"], true);
    assert_eq!(created["command"], "remote session create");
    let session_id = created["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();
    assert_eq!(created["session_id"], session_id);
    assert_eq!(created["data"]["session"]["title"], "Mobile start");
    assert_eq!(created["data"]["permission_mode"], "workspace-write");

    let status = daemon_json(
        &context,
        "GET",
        &format!("/api/status?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(
        status["data"]["projection"]["runtime_descriptor"]["active_session_id"],
        session_id
    );
    assert_eq!(
        status["data"]["projection"]["runtime_descriptor"]["permission_mode"],
        "workspace-write"
    );

    let changed = daemon_json(
        &context,
        "PATCH",
        "/api/session/permission-mode",
        json!({
            "cwd": workspace_root,
            "permission_mode": "danger-full-access"
        }),
    );
    assert_eq!(changed["ok"], true);
    assert_eq!(changed["data"]["permission_mode"], "danger-full-access");

    let changed_status = daemon_json(
        &context,
        "GET",
        &format!("/api/status?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(
        changed_status["data"]["projection"]["runtime_descriptor"]["permission_mode"],
        "danger-full-access"
    );

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    assert!(events.iter().any(|event| {
        event["event_name"] == "session_open" && event["object_id"] == session_id
    }));
    assert!(events.iter().any(|event| {
        event["event_name"] == "permission_mode"
            && event["payload"]["permission_mode"] == "workspace-write"
    }));
}

#[test]
fn remote_daemon_exposes_artifact_code_viewer_projection() {
    let workspace_root = init_git_workspace("remote_daemon_artifact_viewer");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let reports_dir = workspace_root.join("reports");
    std::fs::create_dir_all(&reports_dir).expect("reports dir should be created");
    std::fs::write(
        reports_dir.join("demo.diff"),
        "diff --git a/a.rs b/a.rs\n+structured artifact viewer\n",
    )
    .expect("artifact diff should be writable");

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let artifacts = daemon_json(
        &context,
        "GET",
        &format!("/api/artifacts?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(artifacts["ok"], true);
    assert_eq!(artifacts["command"], "remote artifacts");
    assert_eq!(
        artifacts["data"]["viewer_contract"],
        "artifact_code_diff_pane"
    );
    assert_eq!(
        artifacts["data"]["authority"],
        "artifact_registry_read_only"
    );
    assert!(
        artifacts["data"]["list"]["total_count"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );

    let inspected = daemon_json(
        &context,
        "GET",
        &format!(
            "/api/artifact/inspect?cwd={}&target=reports%2Fdemo.diff",
            workspace_root.display()
        ),
        json!({}),
    );
    assert_eq!(inspected["ok"], true);
    assert_eq!(inspected["command"], "remote artifact inspect");
    assert_eq!(
        inspected["data"]["viewer_contract"],
        "artifact_code_diff_pane"
    );
    assert_eq!(inspected["data"]["preview"]["syntax"], "unified_diff");
    assert_eq!(inspected["data"]["preview"]["truncated"], false);
    assert!(inspected["data"]["preview"]["content"]
        .as_str()
        .unwrap_or("")
        .contains("structured artifact viewer"));

    let empty_family = daemon_json(
        &context,
        "GET",
        &format!(
            "/api/artifact/inspect?cwd={}&target=archive",
            workspace_root.display()
        ),
        json!({}),
    );
    assert_eq!(empty_family["ok"], true);
    assert_eq!(empty_family["data"]["preview"]["preview_kind"], "missing");
}

#[test]
fn remote_daemon_exposes_skills_for_product_surfaces() {
    let workspace_root = init_git_workspace("remote_daemon_skills_surface");
    let state_home = unique_temp_dir("remote_daemon_skills_state");
    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    let skill_dir = workspace_root.join(".codex/skills/web-design-engineer");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "# web-design-engineer\n\nUse this skill for product UI implementation.\n",
    )
    .expect("skill manifest should write");
    let context =
        DaemonContext::new(state_home, workspace_root.clone()).with_control_token("test-token");

    let list = daemon_json(
        &context,
        "GET",
        &format!("/api/skills?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(list["ok"], true);
    assert_eq!(list["command"], "remote skills list");
    assert_eq!(list["data"]["schema_version"], "remote_skill_list.v1");
    assert!(list["data"]["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .any(|skill| skill["skill_id"] == "web-design-engineer"));

    let inspect = daemon_json(
        &context,
        "GET",
        &format!(
            "/api/skills?cwd={}&kind=inspect&skill_id=web-design-engineer",
            workspace_root.display()
        ),
        json!({}),
    );
    assert_eq!(inspect["ok"], true);
    assert_eq!(inspect["command"], "remote skills inspect");
    assert_eq!(inspect["data"]["skill_id"], "web-design-engineer");
}

#[test]
fn remote_daemon_executes_projected_tui_action_ids() {
    let workspace_root = init_real_git_workspace("remote_daemon_tui_action");
    let state_home = unique_temp_dir("remote_daemon_tui_action_state");
    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    let context =
        DaemonContext::new(state_home, workspace_root.clone()).with_control_token("test-token");

    let result = daemon_json(
        &context,
        "POST",
        "/api/tui/action",
        json!({
            "cwd": workspace_root,
            "action_id": "open_research"
        }),
    );
    assert_eq!(result["ok"], true);
    assert_eq!(result["command"], "remote tui action");
    assert_eq!(
        result["data"]["schema_version"],
        "remote_tui_action_result.v1"
    );
    assert_eq!(result["data"]["action_id"], "open_research");
    assert!(result["data"]["result"]["next_recommended_action"].is_string());

    let set_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &context.state_home)
        .args([
            "goals",
            "set",
            "--project-max-goal",
            "Advance research loop from mobile action",
            "--milestone-goal",
            "Expose one projected control",
            "--current-implementation-goal",
            "Execute goal tick through daemon tui action",
            "--automation-mode",
            "full_auto",
            "--json",
        ])
        .output()
        .expect("goals set should execute");
    assert!(
        set_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );
    let record_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &context.state_home)
        .args([
            "research",
            "record",
            "--kind",
            "goal",
            "--title",
            "Remote projected goal tick",
            "--stage",
            "implement-solution",
            "--mode",
            "ready_to_execute",
            "--json",
        ])
        .output()
        .expect("research record should execute");
    assert!(
        record_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&record_output.stderr)
    );
    let advance = daemon_json(
        &context,
        "POST",
        "/api/tui/action",
        json!({
            "cwd": workspace_root,
            "action_id": "advance_research_loop"
        }),
    );
    assert_eq!(advance["ok"], true);
    assert_eq!(advance["command"], "remote tui action");
    assert_eq!(advance["data"]["action_id"], "advance_research_loop");
    assert_eq!(advance["data"]["control_event_kind"], "goal_tick");
    assert_eq!(
        advance["data"]["contract"],
        "goal_tick_existing_authorities"
    );
    assert_eq!(
        advance["data"]["result"]["schema_version"],
        "goal_tick_result.v1"
    );
    assert_eq!(advance["data"]["result"]["dispatch_count"], 1);

    let stage_policy = daemon_json(
        &context,
        "POST",
        "/api/tui/action",
        json!({
            "cwd": workspace_root,
            "action_id": "decide_research_stage"
        }),
    );
    assert_eq!(stage_policy["ok"], true);
    assert_eq!(stage_policy["command"], "remote tui action");
    assert_eq!(stage_policy["data"]["action_id"], "decide_research_stage");
    assert_eq!(
        stage_policy["data"]["result"]["schema_version"],
        "goal_stage_decision_policy_projection.v1"
    );

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &context.state_home)
        .args(["sessions", "create", "--title", "Interrupt Me", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/session/resume",
        json!({
            "cwd": workspace_root,
            "session_id": session_id
        }),
    );

    let interrupt = daemon_json(
        &context,
        "POST",
        "/api/tui/action",
        json!({
            "cwd": workspace_root,
            "action_id": "interrupt_turn",
            "message": "stop current turn"
        }),
    );
    assert_eq!(interrupt["ok"], true);
    assert_eq!(interrupt["command"], "remote tui action");
    assert_eq!(interrupt["data"]["action_id"], "interrupt_turn");
    assert_eq!(
        interrupt["data"]["control_event_kind"],
        "interrupt_requested"
    );
    assert_eq!(
        interrupt["data"]["contract"],
        "notification_backed_interrupt_request"
    );
    assert_eq!(
        interrupt["data"]["result"]["notification"]["kind"],
        "interrupt_requested"
    );

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    assert!(
        events
            .iter()
            .any(|event| event["event_name"] == "remote_interrupt_requested"
                && event["phase"] == "control"
                && event["payload"]["control_event_kind"] == "interrupt_requested"
                && event["payload"]["contract"] == "notification_backed_interrupt_request"),
        "interrupt should be recorded as an explicit control event: {events:?}"
    );
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "goal_advance"
            && event["payload"]["schema_version"] == "goal_advance_result.v1"));
    assert!(events
        .iter()
        .any(|event| event["event_name"] == "goal_task_dispatch"));
}

#[test]
fn remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly() {
    let workspace_root = init_git_workspace("remote_daemon_direct_interrupt");
    let state_home = unique_temp_dir("remote_daemon_state");
    let daemon_port = free_port();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let create_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "sessions",
            "create",
            "--title",
            "Direct Interrupt",
            "--json",
        ])
        .output()
        .expect("sessions create should execute");
    assert!(create_output.status.success());
    let session_id = output_json(&create_output)["data"]["session"]["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("secret-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/session/resume",
        json!({
            "cwd": workspace_root,
            "session_id": session_id
        }),
    );

    let (provider_port, provider_started, provider_done) =
        spawn_slow_openai_stream_fixture("direct interrupt fixture");
    let mut child = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .env("RESEARCH_CLI_MODEL", "gpt-5.5")
        .env("OPENAI_API_KEY", "sk-test")
        .env(
            "OPENAI_BASE_URL",
            format!("http://127.0.0.1:{provider_port}/v1"),
        )
        .args([
            "remote",
            "daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &daemon_port.to_string(),
            "--control-token",
            "secret-token",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("remote daemon should start");

    wait_for_http(daemon_port, "/api/health");

    let message_path = "/api/message".to_string();
    let message_body = json!({
        "cwd": workspace_root,
        "session_id": session_id,
        "message": "stream until interrupted"
    });
    let message_thread = thread::spawn(move || {
        http_post_json_with_header(
            daemon_port,
            &message_path,
            &message_body,
            "Authorization: Bearer secret-token",
        )
    });
    provider_started
        .recv_timeout(Duration::from_secs(5))
        .expect("provider fixture should receive the running prompt request");

    let interrupt_response = http_post_json_with_header(
        daemon_port,
        "/api/tui/action",
        &json!({
            "cwd": workspace_root,
            "action_id": "interrupt_turn",
            "message": "stop active turn"
        }),
        "Authorization: Bearer secret-token",
    );
    let interrupt_json = parse_http_json_body(&interrupt_response);
    assert_eq!(interrupt_json["ok"], true);
    assert_eq!(interrupt_json["data"]["action_id"], "interrupt_turn");
    assert_eq!(
        interrupt_json["data"]["contract"],
        "direct_runtime_cancellation"
    );
    assert_eq!(
        interrupt_json["data"]["control_event_kind"],
        "runtime_cancellation_requested"
    );
    assert_eq!(interrupt_json["data"]["result"]["cancelled"], true);

    let message_response = message_thread
        .join()
        .expect("message request thread should join");
    let message_json = parse_http_json_body(&message_response);
    assert_eq!(message_json["ok"], true);
    assert_eq!(message_json["command"], "remote message");
    assert_eq!(
        message_json["data"]["turn_result"]["outcome"], "cancelled",
        "active remote prompt should be cancelled by host interrupt: {message_json}"
    );
    provider_done
        .join()
        .expect("provider fixture thread should finish");
    terminate(&mut child);

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    assert!(
        events
            .iter()
            .any(|event| event["event_name"] == "remote_interrupt_requested"
                && event["phase"] == "control"
                && event["payload"]["contract"] == "direct_runtime_cancellation"
                && event["payload"]["control_event_kind"] == "runtime_cancellation_requested"),
        "direct interrupt should be recorded as a control event: {events:?}"
    );
    assert!(
        events.iter().any(|event| event["event_name"] == "turn"
            && event["phase"] == "terminal"
            && event["terminal_outcome"] == "cancelled"
            && event["payload"]["outcome"] == "cancelled"),
        "cancelled active turn should be recorded as a terminal event: {events:?}"
    );
}

#[test]
fn remote_daemon_exposes_result_panel_projection_without_new_truth() {
    let workspace_root = init_git_workspace("remote_daemon_result_panel");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let reports_dir = workspace_root.join("reports");
    std::fs::create_dir_all(&reports_dir).expect("reports dir should be created");
    std::fs::write(
        reports_dir.join("test-results.json"),
        r#"{"status":"passed","command":"cargo test remote_daemon_","passed":18}"#,
    )
    .expect("test result artifact should be writable");

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let panel = daemon_json(
        &context,
        "GET",
        &format!("/api/results?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(panel["ok"], true);
    assert_eq!(panel["command"], "remote results");
    assert_eq!(panel["data"]["result_contract"], "test_result_panel");
    assert_eq!(panel["data"]["authority"], "artifact_registry_read_only");
    assert_eq!(panel["data"]["targets"][0]["target"], "reports");
    assert_eq!(panel["data"]["targets"][0]["status"], "available");
    assert_eq!(
        panel["data"]["targets"][0]["preview"]["syntax"],
        "plain_text"
    );
    assert!(panel["data"]["targets"][0]["preview"]["content"]
        .as_str()
        .unwrap_or("")
        .contains("test-results.json"));
}

#[test]
fn remote_daemon_persists_governed_conversation_image_attachments() {
    let workspace_root = init_git_workspace("remote_daemon_attachments");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    let session_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["sessions", "create", "--title", "Attachments", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(session_output.status.success());
    let session_json = output_json(&session_output);
    let session_id = session_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let uploaded = daemon_json(
        &context,
        "POST",
        "/api/session/attachments",
        json!({
            "cwd": workspace_root,
            "session_id": session_id,
            "attachments": [{
                "name": "screen shot.png",
                "mime_type": "image/png",
                "data_base64": "aW1hZ2U="
            }]
        }),
    );
    assert_eq!(uploaded["ok"], true);
    assert_eq!(uploaded["command"], "remote session attachments");
    assert_eq!(
        uploaded["data"]["attachment_contract"],
        "conversation_image_attachment"
    );
    assert_eq!(uploaded["data"]["attachments"][0]["mime_type"], "image/png");
    let saved_path = uploaded["data"]["attachments"][0]["artifact_ref"]
        .as_str()
        .expect("attachment ref should be present");
    assert!(saved_path.contains(".pmcli/remote/attachments/"));
    assert!(std::path::Path::new(saved_path).exists());

    let event_log = std::fs::read_to_string(workspace_root.join(".pmcli/events/events.jsonl"))
        .expect("events log should be readable");
    assert!(event_log.contains("remote_attachment"));
    assert!(event_log.contains("conversation_image_attachment"));
}

#[test]
fn remote_daemon_serves_favicon_without_console_404() {
    let workspace_root = init_git_workspace("remote_daemon_favicon");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root);

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/favicon.ico".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("favicon request should be handled");

    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "image/svg+xml");
    assert!(!response.body.is_empty());
    let icon = String::from_utf8(response.body).expect("favicon should be utf8 svg");
    assert!(icon.contains("<svg"));
    assert!(icon.contains("Astra Remote"));
}

#[test]
fn remote_daemon_status_bootstraps_default_workspace_without_manual_cwd() {
    let workspace_root = init_git_workspace("remote_daemon_default_cwd");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root).with_control_token("test-token");

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/api/status".to_string(),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("daemon status should be handled");

    assert_eq!(response.status, 200);
    let json: Value = serde_json::from_slice(&response.body).expect("status should be json");
    assert_eq!(json["ok"], true);
    assert_eq!(json["command"], "remote status");
    assert_eq!(json["data"]["projection"]["projection_state"], "derived");
}

#[test]
fn remote_daemon_requires_control_token_for_api_when_configured() {
    let workspace_root = init_git_workspace("remote_daemon_token_auth");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root).with_control_token("secret-token");

    let health = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/api/health".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        },
    )
    .expect("health request should be handled");
    assert_eq!(health.status, 200);

    let rejected = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/api/status".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        },
    )
    .expect("unauthorized request should return an envelope");
    assert_eq!(rejected.status, 401);
    let body: Value =
        serde_json::from_slice(&rejected.body).expect("unauthorized body should be json");
    assert_eq!(body["error"]["code"], "remote_daemon_unauthorized");

    let authorized = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/api/status".to_string(),
            headers: vec![(
                "authorization".to_string(),
                "Bearer secret-token".to_string(),
            )],
            body: Vec::new(),
        },
    )
    .expect("authorized request should be handled");
    assert_eq!(authorized.status, 200);
}

#[test]
fn remote_daemon_rejects_api_without_control_token() {
    let workspace_root = init_git_workspace("remote_daemon_no_token_api");
    let state_home = unique_temp_dir("remote_daemon_state");
    let context = DaemonContext::new(state_home, workspace_root.clone());

    let health = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: "/api/health".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        },
    )
    .expect("health request should be handled");
    assert_eq!(health.status, 200);

    let rejected = handle_request(
        &context,
        DaemonRequest {
            method: "POST".to_string(),
            path: "/api/pair".to_string(),
            headers: Vec::new(),
            body: serde_json::to_vec(&json!({
                "cwd": workspace_root,
                "client_id": "phone-alpha",
                "ticket_id": "caller-invented-ticket"
            }))
            .expect("pair body should serialize"),
        },
    )
    .expect("unauthorized pair should return response");
    assert_eq!(rejected.status, 401);
    let body: Value = serde_json::from_slice(&rejected.body).expect("body should be json");
    assert_eq!(
        body["error"]["code"],
        "remote_daemon_control_token_required"
    );
}

#[test]
fn remote_daemon_pair_requires_preissued_ticket() {
    let workspace_root = init_git_workspace("remote_daemon_preissued_ticket");
    let state_home = unique_temp_dir("remote_daemon_state");
    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    let context =
        DaemonContext::new(state_home, workspace_root.clone()).with_control_token("secret-token");

    let rejected = match handle_request(
        &context,
        DaemonRequest {
            method: "POST".to_string(),
            path: "/api/pair".to_string(),
            headers: vec![(
                "authorization".to_string(),
                "Bearer secret-token".to_string(),
            )],
            body: serde_json::to_vec(&json!({
                "cwd": workspace_root,
                "client_id": "phone-alpha",
                "ticket_id": "caller-invented-ticket"
            }))
            .expect("pair body should serialize"),
        },
    ) {
        Ok(response) => {
            let body: Value = serde_json::from_slice(&response.body).expect("body should be json");
            (response.status, body)
        }
        Err(DaemonError::Envelope { status, envelope }) => (status, envelope),
        Err(err) => panic!("unissued pair ticket should return governed response: {err:?}"),
    };
    assert_eq!(rejected.0, 409);
    let body = rejected.1;
    assert_eq!(body["error"]["code"], "remote_action_rejected");
    assert_eq!(
        body["data"]["rejection_code"],
        "remote_pair_ticket_not_found"
    );

    preissue_pair_ticket_for_test(
        &context,
        "/api/pair",
        &json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "expired-ticket",
            "ticket_expires_at": "1"
        }),
    );
    let expired = match handle_request(
        &context,
        DaemonRequest {
            method: "POST".to_string(),
            path: "/api/pair".to_string(),
            headers: auth_headers(&context),
            body: serde_json::to_vec(&json!({
                "cwd": workspace_root,
                "client_id": "phone-alpha",
                "ticket_id": "expired-ticket"
            }))
            .expect("pair body should serialize"),
        },
    ) {
        Ok(response) => {
            let body: Value = serde_json::from_slice(&response.body).expect("body should be json");
            (response.status, body)
        }
        Err(DaemonError::Envelope { status, envelope }) => (status, envelope),
        Err(err) => panic!("expired pair ticket should return governed response: {err:?}"),
    };
    assert_eq!(expired.0, 409);
    assert_eq!(
        expired.1["data"]["rejection_code"],
        "remote_pair_ticket_expired"
    );
}

#[test]
fn remote_daemon_api_pairs_controls_and_writes_canonical_events() {
    let workspace_root = init_git_workspace("remote_daemon_api");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let session_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "sessions",
            "create",
            "--title",
            "Phone controlled",
            "--json",
        ])
        .output()
        .expect("sessions create should execute");
    assert!(session_output.status.success());
    let session_json = output_json(&session_output);
    let session_id = session_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");

    let pair = daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    assert_eq!(pair["ok"], true);
    assert_eq!(pair["command"], "remote pair");
    assert_eq!(pair["data"]["remote_ready"], true);
    assert_eq!(pair["data"]["client_identity"]["client_id"], "phone-alpha");

    let resume = daemon_json(
        &context,
        "POST",
        "/api/session/resume",
        json!({
            "cwd": workspace_root,
            "session_id": session_id
        }),
    );
    assert_eq!(resume["ok"], true);
    assert_eq!(resume["command"], "remote session resume");
    assert_eq!(resume["session_id"], session_id);

    let attach = daemon_json(
        &context,
        "POST",
        "/api/attach",
        json!({
            "cwd": workspace_root,
            "session_id": session_id,
            "strategy": "terminal_host",
            "execute": true
        }),
    );
    assert_eq!(attach["ok"], true);
    assert_eq!(attach["command"], "remote attach");
    assert_eq!(attach["session_id"], session_id);
    assert_eq!(attach["data"]["execution_mode"], "execute");
    assert_eq!(attach["data"]["control_owner"]["owner_id"], "phone-alpha");

    let notify = daemon_json(
        &context,
        "POST",
        "/api/notify",
        json!({
            "cwd": workspace_root,
            "kind": "task_finished",
            "message": "phone acknowledged"
        }),
    );
    assert_eq!(notify["ok"], true);
    assert_eq!(notify["command"], "remote notify");
    assert_eq!(notify["data"]["notification"]["kind"], "task_finished");

    let status = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: format!("/api/status?cwd={}", workspace_root.display()),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("status request should be handled");
    assert_eq!(status.status, 200);
    let status_json: Value =
        serde_json::from_slice(&status.body).expect("status body should be json");
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["command"], "remote status");
    assert_eq!(
        status_json["data"]["control_owner"]["owner_id"],
        "phone-alpha"
    );

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    for expected in ["remote_attach", "remote_notify"] {
        assert!(
            events.iter().any(|event| event["event_name"] == expected),
            "missing daemon event {expected}: {events:?}"
        );
    }
}

#[test]
fn remote_daemon_events_streams_canonical_events_after_cursor() {
    let workspace_root = init_git_workspace("remote_daemon_events");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let session_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["sessions", "create", "--title", "Streamed", "--json"])
        .output()
        .expect("sessions create should execute");
    assert!(session_output.status.success());
    let session_json = output_json(&session_output);
    let session_id = session_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/message",
        json!({
            "cwd": workspace_root,
            "session_id": session_id,
            "message": "stream this"
        }),
    );

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: format!("/api/events?cwd={}&after_seq=0", workspace_root.display()),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("events request should be handled");
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "text/event-stream; charset=utf-8");
    let body = String::from_utf8(response.body).expect("events stream should be utf8");
    assert!(body.contains("event: remote-event"));
    assert!(body.contains("\"event_name\":\"remote_message_delta\""));
    assert!(body.contains("\"delta\":"));
    assert!(body.contains("\"event_name\":\"remote_message\""));
    assert!(body.contains("\"seq\":"));
}

#[test]
fn remote_daemon_events_returns_heartbeat_before_event_log_exists() {
    let workspace_root = init_git_workspace("remote_daemon_events_empty");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    let event_log_path = workspace_root.join(".pmcli/events/events.jsonl");
    std::fs::remove_file(&event_log_path).expect("event log should be removable");

    let context =
        DaemonContext::new(state_home, workspace_root.clone()).with_control_token("test-token");

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: format!("/api/events?cwd={}&after_seq=0", workspace_root.display()),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("events request should be handled before events exist");
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "text/event-stream; charset=utf-8");
    let body = String::from_utf8(response.body).expect("events stream should be utf8");
    assert!(body.contains("event: remote-heartbeat"));
}

#[test]
fn remote_daemon_events_exposes_long_poll_cursor_heartbeat() {
    let workspace_root = init_git_workspace("remote_daemon_events_wait");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());
    let event_log_path = workspace_root.join(".pmcli/events/events.jsonl");
    std::fs::remove_file(&event_log_path).expect("event log should be removable");

    let context =
        DaemonContext::new(state_home, workspace_root.clone()).with_control_token("test-token");

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: format!(
                "/api/events?cwd={}&after_seq=7&wait_ms=1",
                workspace_root.display()
            ),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("events long-poll request should be handled");
    assert_eq!(response.status, 200);
    let body = String::from_utf8(response.body).expect("events stream should be utf8");
    assert!(body.contains("event: remote-heartbeat"));
    assert!(body.contains("\"after_seq\":7"));
    assert!(body.contains("\"wait_ms\":1"));
}

#[test]
fn remote_daemon_exposes_host_surface_and_governed_terminal_projection() {
    let workspace_root = init_git_workspace("remote_daemon_host_surface");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let surface = daemon_json(
        &context,
        "GET",
        &format!("/api/host-surface?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(surface["ok"], true);
    assert_eq!(surface["command"], "host surface status");
    assert_eq!(
        surface["data"]["projection"]["surfaces"]["mobile"]["transport"],
        "tailscale_private_overlay"
    );
    assert!(surface["data"]["projection"]["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .any(|action| action["action_id"] == "terminal_attach"));
    let interrupt_action = surface["data"]["projection"]["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .find(|action| action["action_id"] == "interrupt_turn")
        .expect("interrupt action should be projected");
    assert_eq!(
        interrupt_action["control_contract"], "direct_runtime_cancellation",
        "host surface should expose the direct runtime cancel path"
    );
    assert_eq!(
        interrupt_action["fallback_control_contract"], "notification_backed_interrupt_request",
        "host surface should preserve notification fallback"
    );

    let actions = daemon_json(
        &context,
        "GET",
        &format!("/api/tui/actions?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(actions["ok"], true);
    assert_eq!(actions["command"], "tui actions");
    assert!(actions["data"]["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .any(|action| action["action_id"] == "switch_session"
            && action["command"].as_array().unwrap()[1] == "resume"));
    let projected_interrupt = actions["data"]["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .find(|action| action["action_id"] == "interrupt_turn")
        .expect("interrupt action should be listed");
    assert_eq!(
        projected_interrupt["control_contract"],
        "direct_runtime_cancellation"
    );
    assert_eq!(
        projected_interrupt["fallback_control_contract"],
        "notification_backed_interrupt_request"
    );

    let terminal = daemon_json(
        &context,
        "POST",
        "/api/terminal/attach",
        json!({
            "cwd": workspace_root
        }),
    );
    assert_eq!(terminal["ok"], true);
    assert_eq!(terminal["command"], "remote terminal attach");
    assert_eq!(
        terminal["data"]["terminal"]["xterm_bridge_state"],
        "host_pty_byte_stream"
    );

    let events_before_replay =
        read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl")).len();
    let replay = daemon_json(
        &context,
        "GET",
        &format!("/api/terminal/replay?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(replay["ok"], true);
    assert_eq!(replay["command"], "remote terminal replay");
    assert_eq!(
        replay["data"]["replay"]["events"][0]["event_kind"],
        "projection_cursor"
    );
    let events_after_replay =
        read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl")).len();
    assert_eq!(
        events_after_replay, events_before_replay,
        "GET terminal replay must be a read-only projection"
    );
}

#[test]
fn remote_daemon_terminal_bridge_records_governed_input_resize_and_signal() {
    let workspace_root = init_git_workspace("remote_daemon_terminal_bridge");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/terminal/attach",
        json!({
            "cwd": workspace_root
        }),
    );
    let ticket = daemon_json(
        &context,
        "POST",
        "/api/terminal/ws-ticket",
        json!({
            "cwd": workspace_root
        }),
    );
    let ticket_value = ticket["data"]["ticket"].as_str().expect("ticket");

    let token_query_rejected = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: format!(
                "/api/terminal/ws?cwd={}&token=test-token",
                workspace_root.display()
            ),
            headers: Vec::new(),
            body: Vec::new(),
        },
    )
    .expect("terminal websocket token query should return json envelope");
    assert_eq!(token_query_rejected.status, 401);

    let bridge_without_upgrade = handle_request(
        &context,
        DaemonRequest {
            method: "GET".to_string(),
            path: format!(
                "/api/terminal/ws?cwd={}&ticket={ticket_value}",
                workspace_root.display()
            ),
            headers: auth_headers(&context),
            body: Vec::new(),
        },
    )
    .expect("terminal websocket route should return a contract envelope");
    assert_eq!(bridge_without_upgrade.status, 426);
    let bridge_body: Value = serde_json::from_slice(&bridge_without_upgrade.body)
        .expect("bridge rejection should be json");
    assert_eq!(
        bridge_body["error"]["code"],
        "terminal_websocket_upgrade_required"
    );

    let input = daemon_json(
        &context,
        "POST",
        "/api/terminal/input",
        json!({
            "cwd": workspace_root,
            "data": "printf ready\n"
        }),
    );
    assert_eq!(input["ok"], true);
    assert_eq!(input["command"], "remote terminal input");
    assert_eq!(input["data"]["bridge_state"], "queued_for_governed_bridge");
    assert_eq!(input["data"]["bytes"], 13);

    let resize = daemon_json(
        &context,
        "POST",
        "/api/terminal/resize",
        json!({
            "cwd": workspace_root,
            "cols": 120,
            "rows": 36
        }),
    );
    assert_eq!(resize["ok"], true);
    assert_eq!(resize["command"], "remote terminal resize");
    assert_eq!(resize["data"]["cols"], 120);
    assert_eq!(resize["data"]["rows"], 36);

    let signal = daemon_json(
        &context,
        "POST",
        "/api/terminal/signal",
        json!({
            "cwd": workspace_root,
            "signal": "interrupt"
        }),
    );
    assert_eq!(signal["ok"], true);
    assert_eq!(signal["command"], "remote terminal signal");
    assert_eq!(signal["data"]["signal"], "interrupt");

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    for expected in [
        "remote_terminal_attach",
        "remote_terminal_input",
        "remote_terminal_resize",
        "remote_terminal_signal",
    ] {
        assert!(
            events.iter().any(|event| event["event_name"] == expected),
            "missing terminal bridge event {expected}: {events:?}"
        );
    }
}

#[test]
fn remote_daemon_reconnect_replays_cursor_and_rejects_expired_lease() {
    let workspace_root = init_git_workspace("remote_daemon_reconnect_harness");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/terminal/attach",
        json!({
            "cwd": workspace_root
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/terminal/input",
        json!({
            "cwd": workspace_root,
            "data": "printf reconnect\n"
        }),
    );

    let restarted_context = DaemonContext::new(state_home.clone(), workspace_root.clone());
    let reconnect = daemon_json(
        &restarted_context,
        "POST",
        "/api/reconnect",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "after_seq": 0
        }),
    );
    assert_eq!(reconnect["ok"], true);
    assert_eq!(reconnect["command"], "remote reconnect");
    assert_eq!(reconnect["data"]["status"]["remote_ready"], true);
    assert_eq!(
        reconnect["data"]["status"]["cursor"]["replay_policy"],
        "replay_from_cursor"
    );
    assert_eq!(reconnect["data"]["replay"]["after_seq"], 0);
    assert!(
        reconnect["data"]["replay"]["event_count"]
            .as_u64()
            .expect("event_count should be numeric")
            >= 2
    );
    let replay_events = reconnect["data"]["replay"]["events"]
        .as_array()
        .expect("replay events should be an array");
    for expected in ["remote_terminal_attach", "remote_terminal_input"] {
        assert!(
            replay_events
                .iter()
                .any(|event| event["event_name"] == expected),
            "replay should include {expected}: {replay_events:?}"
        );
    }
    assert_eq!(
        reconnect["data"]["projection"]["projection_state"],
        "derived"
    );

    let reconnect_events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    assert!(reconnect_events
        .iter()
        .any(|event| event["event_name"] == "remote_reconnect"));

    let mismatched_reconnect = daemon_json_response(
        &restarted_context,
        "POST",
        "/api/reconnect",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-beta",
            "after_seq": 0
        }),
    );
    assert_eq!(mismatched_reconnect.0, 409);
    assert_eq!(
        mismatched_reconnect.1["error"]["code"],
        "remote_action_rejected"
    );
    assert_eq!(
        mismatched_reconnect.1["data"]["rejection_code"],
        "remote_client_mismatch"
    );

    daemon_json(
        &restarted_context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha-expired",
            "lease_expires_at": "1"
        }),
    );
    let expired_reconnect = daemon_json_response(
        &restarted_context,
        "POST",
        "/api/reconnect",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "after_seq": 0
        }),
    );
    assert_eq!(expired_reconnect.0, 409);
    assert_eq!(
        expired_reconnect.1["error"]["code"],
        "remote_action_rejected"
    );
    assert_eq!(
        expired_reconnect.1["data"]["rejection_code"],
        "remote_lease_expired"
    );
}

#[test]
fn remote_daemon_exposes_workbench_message_and_permission_response() {
    let workspace_root = init_git_workspace("remote_daemon_workbench");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let permission_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "remote.txt",
            "--content",
            "from phone",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("permission request should execute");
    assert_eq!(permission_output.status.code(), Some(5));
    let permission_json = output_json(&permission_output);
    let request_id = permission_json["data"]["request_id"]
        .as_str()
        .expect("permission request id should exist")
        .to_string();

    let session_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "sessions",
            "create",
            "--title",
            "Remote workbench",
            "--json",
        ])
        .output()
        .expect("sessions create should execute");
    assert!(session_output.status.success());
    let session_json = output_json(&session_output);
    let session_id = session_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone());
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let workbench = daemon_json(
        &context,
        "GET",
        &format!("/api/workbench?cwd={}", workspace_root.display()),
        json!({}),
    );
    assert_eq!(workbench["ok"], true);
    assert_eq!(workbench["command"], "remote workbench");
    assert_eq!(workbench["data"]["permissions"]["pending_count"], 1);
    assert_eq!(
        workbench["data"]["permissions"]["pending"][0]["request_id"],
        request_id
    );
    assert_eq!(workbench["data"]["sections"]["reviews"], "available");
    assert_eq!(
        workbench["data"]["sections"]["memory"],
        "durable_memory_available"
    );
    assert_eq!(
        workbench["data"]["memory"]["status"],
        "durable_memory_available"
    );
    assert_eq!(
        workbench["data"]["transport"]["mode"],
        "tailscale_private_overlay"
    );

    let message = daemon_json(
        &context,
        "POST",
        "/api/message",
        json!({
            "cwd": workspace_root,
            "session_id": session_id,
            "message": "continue from phone"
        }),
    );
    assert_eq!(message["ok"], true);
    assert_eq!(message["command"], "remote message");
    assert_eq!(message["data"]["message_state"], "completed_by_kernel");
    assert_eq!(message["data"]["turn_result"]["outcome"], "completed");
    let store = SessionStore::new(workspace_root.join(".pmcli"));
    let transcript = store
        .read_transcript(&session_id)
        .expect("remote message should be readable from transcript");
    assert!(
        transcript.iter().any(|line| {
            matches!(
                line,
                TranscriptLine::Message { role, content }
                    if role == "user" && content == "continue from phone"
            )
        }),
        "remote message should append a user transcript line: {transcript:?}"
    );
    assert!(
        transcript.iter().any(|line| {
            matches!(
                line,
                TranscriptLine::Message { role, content }
                    if role == "assistant" && !content.trim().is_empty()
            )
        }),
        "remote message should append a non-empty assistant transcript line: {transcript:?}"
    );

    let decision = daemon_json(
        &context,
        "POST",
        "/api/permission",
        json!({
            "cwd": workspace_root,
            "request_id": request_id,
            "decision": "approve"
        }),
    );
    assert_eq!(decision["ok"], true);
    assert_eq!(decision["command"], "remote permission");
    assert_eq!(decision["data"]["decision"], "approved");

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    for expected in ["remote_message", "remote_permission_response", "permission"] {
        assert!(
            events.iter().any(|event| event["event_name"] == expected),
            "missing daemon event {expected}: {events:?}"
        );
    }
}

#[test]
fn remote_daemon_exposes_structured_session_transcript_for_conversation_ui() {
    let workspace_root = init_git_workspace("remote_daemon_structured_transcript");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let session_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "sessions",
            "create",
            "--title",
            "Structured agent",
            "--json",
        ])
        .output()
        .expect("sessions create should execute");
    assert!(session_output.status.success());
    let session_json = output_json(&session_output);
    let session_id = session_json["data"]["session"]["session_id"]
        .as_str()
        .expect("session id should be present")
        .to_string();

    let store = SessionStore::new(workspace_root.join(".pmcli"));
    store
        .append_line(
            &session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "summarize the failing test".to_string(),
            },
        )
        .expect("user transcript should append");
    store
        .append_line(
            &session_id,
            TranscriptLine::ToolCall {
                tool_name: "cargo test".to_string(),
                arguments: "remote_daemon_".to_string(),
                call_id: Some("call_remote_daemon_1".to_string()),
            },
        )
        .expect("tool call should append");
    store
        .append_line(
            &session_id,
            TranscriptLine::ToolResult {
                tool_name: "cargo test".to_string(),
                output: "14 passed; 0 failed".to_string(),
                call_id: Some("call_remote_daemon_1".to_string()),
            },
        )
        .expect("tool result should append");
    store
        .append_line(
            &session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: "The remote daemon tests pass.".to_string(),
            },
        )
        .expect("assistant transcript should append");

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone());
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let transcript = daemon_json(
        &context,
        "GET",
        &format!(
            "/api/session/transcript?cwd={}&session_id={}",
            workspace_root.display(),
            session_id
        ),
        json!({}),
    );
    assert_eq!(transcript["ok"], true);
    assert_eq!(transcript["command"], "remote session transcript");
    assert_eq!(transcript["session_id"], session_id);
    assert_eq!(
        transcript["data"]["surface_contract"],
        "conversation_primary_surface"
    );
    assert_eq!(
        transcript["data"]["terminal_lane"],
        "terminal_compatibility_lane"
    );
    assert_eq!(transcript["data"]["lines"].as_array().unwrap().len(), 4);
    assert_eq!(transcript["data"]["lines"][0]["line_type"], "message");
    assert_eq!(transcript["data"]["lines"][1]["line_type"], "tool_call");
    assert_eq!(
        transcript["data"]["lines"][2]["output"],
        "14 passed; 0 failed"
    );
}

#[test]
fn remote_daemon_rejects_expired_permission_response() {
    let workspace_root = init_git_workspace("remote_daemon_permission_ttl");
    let state_home = unique_temp_dir("remote_daemon_state");

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let permission_output = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .env("RESEARCH_CLI_PERMISSION_REQUEST_TTL_MS", "0")
        .args([
            "tools",
            "run",
            "write_file",
            "--path",
            "expired.txt",
            "--content",
            "expired",
            "--permission-mode",
            "read-only",
            "--json",
        ])
        .output()
        .expect("expired permission request should execute");
    assert_eq!(permission_output.status.code(), Some(5));
    let permission_json = output_json(&permission_output);
    let request_id = permission_json["data"]["request_id"]
        .as_str()
        .expect("permission request id should exist")
        .to_string();

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("test-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );

    let response = handle_request(
        &context,
        DaemonRequest {
            method: "POST".to_string(),
            path: "/api/permission".to_string(),
            headers: auth_headers(&context),
            body: serde_json::to_vec(&json!({
                "cwd": workspace_root,
                "request_id": request_id,
                "decision": "approve"
            }))
            .expect("request body should serialize"),
        },
    )
    .expect("daemon request should be handled");
    assert_eq!(response.status, 400);
    let body: Value =
        serde_json::from_slice(&response.body).expect("daemon response should be json");
    assert_eq!(body["error"]["code"], "permission_request_expired");
}

#[test]
fn remote_daemon_cli_serves_health_and_mobile_web_app_over_http() {
    let workspace_root = init_git_workspace("remote_daemon_cli");
    let state_home = unique_temp_dir("remote_daemon_state");
    let port = free_port();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let mut child = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "remote",
            "daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("remote daemon should start");

    wait_for_http(port, "/api/health");

    let health = http_get(port, "/api/health");
    assert!(health.contains("\"command\":\"remote daemon health\""));
    assert!(health.contains("\"daemon_state\":\"running\""));

    let app_html = http_get(port, "/");
    assert!(app_html.contains("<title>Astra Remote</title>"));
    assert!(app_html.contains("<div id=\"app\"></div>"));
    assert!(!app_html.contains("Astra Code"));

    terminate(&mut child);
}

#[test]
fn remote_daemon_cli_enforces_control_token_over_http() {
    let workspace_root = init_git_workspace("remote_daemon_cli_token");
    let state_home = unique_temp_dir("remote_daemon_state");
    let port = free_port();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let mut child = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "remote",
            "daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--control-token",
            "secret-token",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("remote daemon should start");

    wait_for_http(port, "/api/health");

    let rejected = http_get(port, "/api/status");
    assert!(rejected.contains("401 Unauthorized"));
    assert!(rejected.contains("\"code\":\"remote_daemon_unauthorized\""));

    let rejected_query_token = http_get(port, "/api/status?token=secret-token");
    assert!(rejected_query_token.contains("401 Unauthorized"));
    assert!(rejected_query_token.contains("\"code\":\"remote_daemon_unauthorized\""));

    let accepted = http_get_with_header(port, "/api/status", "Authorization: Bearer secret-token");
    assert!(accepted.contains("200 OK"));
    assert!(accepted.contains("\"projection_state\":\"derived\""));

    terminate(&mut child);
}

#[test]
fn remote_daemon_cli_accepts_terminal_websocket_frames_over_tailscale_route() {
    let workspace_root = init_git_workspace("remote_daemon_cli_terminal_ws");
    let state_home = unique_temp_dir("remote_daemon_state");
    let port = free_port();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let context = DaemonContext::new(state_home.clone(), workspace_root.clone())
        .with_control_token("secret-token");
    daemon_json(
        &context,
        "POST",
        "/api/pair",
        json!({
            "cwd": workspace_root,
            "client_id": "phone-alpha",
            "ticket_id": "ticket-phone-alpha"
        }),
    );
    daemon_json(
        &context,
        "POST",
        "/api/terminal/attach",
        json!({
            "cwd": workspace_root
        }),
    );

    let mut child = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "remote",
            "daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--json",
            "--control-token",
            "secret-token",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("remote daemon should start");

    wait_for_http(port, "/api/health");
    let ticket_response = http_post_json_with_header(
        port,
        "/api/terminal/ws-ticket",
        &json!({ "cwd": workspace_root }),
        "Authorization: Bearer secret-token",
    );
    let ticket_json: Value = serde_json::from_str(
        ticket_response
            .split("\r\n\r\n")
            .nth(1)
            .expect("ticket response body"),
    )
    .expect("ticket response should be json");
    let ticket = ticket_json["data"]["ticket"]
        .as_str()
        .expect("ticket should be present")
        .to_string();

    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("websocket connection should open");
    write!(
        stream,
        "GET /api/terminal/ws?cwd={}&ticket={} HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        workspace_root.display(),
        ticket
    )
    .expect("websocket handshake should write");
    let handshake = read_http_headers(&mut stream);
    assert!(handshake.contains("101 Switching Protocols"));
    assert!(handshake.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("websocket test stream should accept read timeout");

    stream
        .write_all(&masked_ws_text_frame(
            r#"{"type":"input","data":"touch pty_bypass\n"}"#,
        ))
        .expect("websocket input frame should write");
    stream
        .write_all(&masked_ws_text_frame(r#"{"type":"input","data":"\u0004"}"#))
        .expect("websocket quit frame should write");
    let mut saw_ack = false;
    let mut saw_pty_output = false;
    let mut frames = String::new();
    for _ in 0..20 {
        let Some(frame) = read_server_ws_text_frame_timeout(&mut stream) else {
            break;
        };
        frames.push_str(&frame);
        saw_ack |= frame.contains("\"type\":\"input_ack\"")
            && frame.contains("\"remote_terminal_input.v1\"");
        saw_pty_output |= frame.contains("\"type\":\"pty_output\"")
            && frame.contains("\"remote_terminal_pty_output.v1\"")
            && frame.contains("\"chunk_base64\"")
            && frame.contains("Astra Code full-screen TUI");
        if saw_ack && saw_pty_output {
            break;
        }
    }
    assert!(saw_ack, "websocket input should return input_ack");

    terminate(&mut child);

    let events = read_json_lines(&workspace_root.join(".pmcli/events/events.jsonl"));
    assert!(
        events
            .iter()
            .any(|event| event["event_name"] == "remote_terminal_input"),
        "websocket input should be recorded as a canonical event: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["event_name"] == "remote_terminal_output"
                && event["payload"]["byte_bridge_state"] == "host_pty_byte_stream"),
        "websocket PTY output should be recorded as canonical byte-stream event: {events:?}"
    );
    saw_pty_output |= events.iter().any(|event| {
        event["event_name"] == "remote_terminal_output"
            && event["payload"]["byte_bridge_state"] == "host_pty_byte_stream"
            && event["payload"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("Astra Code full-screen TUI"))
    });
    assert!(
        saw_pty_output,
        "websocket should stream or persist real host PTY output bytes; frames={frames}; events={events:?}"
    );
    assert!(
        !workspace_root.join("pty_bypass").exists(),
        "PTY must run the governed research-cli surface, not an unrestricted shell"
    );
}

#[test]
fn remote_daemon_cli_handles_browser_preconnect_without_blocking_assets() {
    let workspace_root = init_git_workspace("remote_daemon_preconnect");
    let state_home = unique_temp_dir("remote_daemon_state");
    let port = free_port();

    let init_status = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args(["projects", "init", "--json"])
        .status()
        .expect("projects init should execute");
    assert!(init_status.success());

    let mut child = Command::new(cargo_bin())
        .current_dir(&workspace_root)
        .env("RESEARCH_CLI_STATE_HOME", &state_home)
        .args([
            "remote",
            "daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("remote daemon should start");

    wait_for_http(port, "/api/health");

    let idle_connection =
        TcpStream::connect(("127.0.0.1", port)).expect("idle browser preconnect should open");
    let started = Instant::now();
    let app_js = http_get(port, "/app.js");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "second request was blocked by idle preconnect"
    );
    assert!(app_js.contains("renderHomePage") || app_js.contains("/api/quick-pair"));

    drop(idle_connection);
    terminate(&mut child);
}

fn daemon_json(context: &DaemonContext, method: &str, path: &str, body: Value) -> Value {
    let request_context = test_authenticated_context(context);
    preissue_pair_ticket_for_test(&request_context, path, &body);
    let response = handle_request(
        &request_context,
        DaemonRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers: auth_headers(&request_context),
            body: serde_json::to_vec(&body).expect("request body should serialize"),
        },
    )
    .expect("daemon request should be handled");
    assert_eq!(
        response.status,
        200,
        "daemon returned {}: {}",
        response.status,
        String::from_utf8_lossy(&response.body)
    );
    serde_json::from_slice(&response.body).expect("daemon response should be json")
}

fn daemon_json_response(
    context: &DaemonContext,
    method: &str,
    path: &str,
    body: Value,
) -> (u16, Value) {
    let request_context = test_authenticated_context(context);
    preissue_pair_ticket_for_test(&request_context, path, &body);
    let response = match handle_request(
        &request_context,
        DaemonRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers: auth_headers(&request_context),
            body: serde_json::to_vec(&body).expect("request body should serialize"),
        },
    ) {
        Ok(response) => response,
        Err(DaemonError::Envelope { status, envelope }) => return (status, envelope),
        Err(err) => panic!("daemon request should be handled: {err:?}"),
    };
    let value = serde_json::from_slice(&response.body).expect("daemon response should be json");
    (response.status, value)
}

fn test_authenticated_context(context: &DaemonContext) -> DaemonContext {
    if context.control_token.is_some() {
        context.clone()
    } else {
        context.clone().with_control_token("test-token")
    }
}

fn auth_headers(context: &DaemonContext) -> Vec<(String, String)> {
    context
        .control_token
        .as_ref()
        .map(|token| vec![("authorization".to_string(), format!("Bearer {token}"))])
        .unwrap_or_default()
}

fn preissue_pair_ticket_for_test(context: &DaemonContext, path: &str, body: &Value) {
    if path != "/api/pair" {
        return;
    }
    let Some(ticket_id) = body.get("ticket_id").and_then(Value::as_str) else {
        return;
    };
    let Some(client_id) = body.get("client_id").and_then(Value::as_str) else {
        return;
    };
    let ticket_dir = context.state_home.join("remote").join("pair_tickets");
    std::fs::create_dir_all(&ticket_dir).expect("pair ticket dir should create");
    let ticket_path = ticket_dir.join(format!("{ticket_id}.json"));
    if ticket_path.exists() {
        return;
    }
    let ticket = json!({
        "schema_version": "1",
        "ticket_id": ticket_id,
        "status": "issued",
        "client_id": client_id,
        "machine_id": "machine_test",
        "issued_at": "1",
        "consumed_at": "",
        "expires_at": body.get("ticket_expires_at").and_then(Value::as_str).unwrap_or("")
    });
    std::fs::write(
        ticket_path,
        serde_json::to_vec_pretty(&ticket).expect("pair ticket should serialize"),
    )
    .expect("pair ticket should write");
}

fn free_port() -> u16 {
    for _ in 0..1000 {
        let port = NEXT_DAEMON_TEST_PORT.fetch_add(1, Ordering::SeqCst);
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    TcpListener::bind(("127.0.0.1", 0))
        .expect("free port probe should bind")
        .local_addr()
        .expect("free port probe should have local addr")
        .port()
}

fn wait_for_http(port: u16, path: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match try_http_get(port, path) {
            Ok(_) => return,
            Err(err) if Instant::now() < deadline => {
                let _ = err;
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => panic!("daemon did not become ready: {err}"),
        }
    }
}

fn http_get(port: u16, path: &str) -> String {
    try_http_get(port, path).expect("http get should succeed")
}

fn http_get_with_header(port: u16, path: &str, header: &str) -> String {
    let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("http connection should open");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{header}\r\nConnection: close\r\n\r\n"
    )
    .expect("http request should write");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("http response should read");
    response
}

fn http_post_json_with_header(port: u16, path: &str, body: &Value, header: &str) -> String {
    let body = serde_json::to_string(body).expect("http body should serialize");
    let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("http connection should open");
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{header}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("http request should write");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("http response should read");
    response
}

fn parse_http_json_body(response: &str) -> Value {
    let body = response.split("\r\n\r\n").nth(1).unwrap_or(response).trim();
    serde_json::from_str(body).unwrap_or_else(|err| {
        panic!("http response body should be json: {err}; response={response}")
    })
}

fn spawn_slow_openai_stream_fixture(
    first_delta: &'static str,
) -> (u16, mpsc::Receiver<()>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("provider listener should bind");
    listener
        .set_nonblocking(true)
        .expect("provider listener should become nonblocking");
    let port = listener.local_addr().expect("provider local addr").port();
    let (started_tx, started_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    read_http_request(&mut stream);
                    let body_prefix = format!(
                        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{first_delta}\"}}}}]}}\n\n"
                    );
                    let response_header = concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Content-Type: text/event-stream; charset=utf-8\r\n",
                        "Connection: close\r\n",
                        "\r\n"
                    );
                    stream
                        .write_all(response_header.as_bytes())
                        .expect("provider response header should write");
                    stream
                        .write_all(body_prefix.as_bytes())
                        .expect("provider first delta should write");
                    stream.flush().expect("provider first delta should flush");
                    let _ = started_tx.send(());
                    thread::sleep(Duration::from_millis(400));
                    for index in 0..20 {
                        let delta = format!(
                            "data: {{\"choices\":[{{\"delta\":{{\"content\":\" tail-{index}\"}}}}]}}\n\n"
                        );
                        if stream.write_all(delta.as_bytes()).is_err() {
                            return;
                        }
                        let _ = stream.flush();
                        thread::sleep(Duration::from_millis(40));
                    }
                    let _ = stream.write_all(b"data: [DONE]\n\n");
                    return;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("provider fixture did not receive a prompt request");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("provider accept failed: {err}"),
            }
        }
    });
    (port, started_rx, handle)
}

fn read_http_request(stream: &mut TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("request should read");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_text = String::from_utf8_lossy(&request[..header_end]);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        if request.len() >= header_end + 4 + content_length {
            break;
        }
    }
}

fn read_http_headers(stream: &mut TcpStream) -> String {
    let mut response = Vec::new();
    let mut one = [0u8; 1];
    while stream.read_exact(&mut one).is_ok() {
        response.push(one[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(response).expect("http headers should be utf8")
}

fn masked_ws_text_frame(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    assert!(bytes.len() < 126, "test helper only writes small frames");
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![0x81, 0x80 | bytes.len() as u8];
    frame.extend_from_slice(&mask);
    for (index, byte) in bytes.iter().enumerate() {
        frame.push(byte ^ mask[index % mask.len()]);
    }
    frame
}

fn read_server_ws_text_frame_timeout(stream: &mut TcpStream) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut header = [0u8; 2];
    if !read_exact_until_optional(stream, &mut header, deadline, "websocket frame header") {
        return None;
    }
    assert_eq!(header[0] & 0x0f, 1, "server frame should be text");
    let mut len = (header[1] & 0x7f) as usize;
    if len == 126 {
        let mut extended = [0u8; 2];
        read_exact_until(stream, &mut extended, deadline, "extended websocket len");
        len = u16::from_be_bytes(extended) as usize;
    }
    let mut payload = vec![0u8; len];
    read_exact_until(stream, &mut payload, deadline, "websocket payload");
    Some(String::from_utf8(payload).expect("server websocket text should be utf8"))
}

fn read_exact_until_optional(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
    label: &str,
) -> bool {
    loop {
        match stream.read_exact(buffer) {
            Ok(()) => return true,
            Err(err) if is_retryable_read_timeout(&err) && Instant::now() < deadline => {
                continue;
            }
            Err(err) if is_retryable_read_timeout(&err) => {
                return false;
            }
            Err(err) => panic!("{label} should read: {err}"),
        }
    }
}

fn read_exact_until(stream: &mut TcpStream, buffer: &mut [u8], deadline: Instant, label: &str) {
    loop {
        match stream.read_exact(buffer) {
            Ok(()) => return,
            Err(err) if is_retryable_read_timeout(&err) && Instant::now() < deadline => {
                continue;
            }
            Err(err) => panic!("{label} should read: {err}"),
        }
    }
}

fn is_retryable_read_timeout(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ) || err.raw_os_error() == Some(11)
}

fn try_http_get(port: u16, path: &str) -> Result<String, std::io::Error> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
