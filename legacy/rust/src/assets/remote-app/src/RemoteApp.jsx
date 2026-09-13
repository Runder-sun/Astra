import React, { useEffect } from 'react';
import './styles.css';
import { initializeRemoteRuntime } from './remoteRuntime.js';

export function RemoteApp() {
  useEffect(() => {
    initializeRemoteRuntime();
  }, []);

  return (
    <div id="remote-root" className="workspace-shell">
    {/* ========== LOADING SPLASH ========== */}
    <div id="loading-splash" className="loading-splash">
      <div className="loading-spinner"></div>
      <p data-i18n="connecting_to_astra">Connecting to Astra...</p>
    </div>

    {/* ========== PAGE: HOME ========== */}
    <div id="page-HOME" className="page" data-page="true">
      <header className="page-topbar mobile-brand-bar">
        <span className="mobile-brand-bar" hidden={true}></span>
        <span className="brand-mark-small" aria-hidden="true">A</span>
        <div className="topbar-title-group">
          <p className="eyebrow">Astra Code</p>
          <span id="home-active-server-origin" className="topbar-subtitle" data-i18n="connecting">connecting...</span>
        </div>
        <div className="topbar-actions">
          <button type="button" id="new-session-btn" className="icon-button" data-navigate="NEWSESSION" aria-label="New session" title="New session" data-i18n-title="new_session">+</button>
          <button type="button" id="mobile-settings-btn" className="icon-button" aria-label="Settings" title="Settings" data-i18n-title="settings">&#9881;</button>
        </div>
      </header>
      <div className="page-body">
        <section id="server-directory" className="panel">
          <div className="panel-heading">
            <h2 data-i18n="server">Server</h2>
            <span id="server-origin-label" className="subtle"></span>
          </div>
          <div id="server-list" className="server-list"></div>
          <div id="server-health-status" className="subtle"></div>
          <div className="button-row">
            <button type="button" id="home-reconnect" className="secondary" data-i18n="replay">Reconnect</button>
          </div>
        </section>
      </div>
      <div className="page-bottombar">
        <button type="button" id="add-server-btn" className="primary full-width" aria-label="Add server" data-i18n="add_server">+ Add Server</button>
      </div>
    </div>

    {/* ========== PAGE: CHAT ========== */}
    <div id="page-CHAT" className="page" data-page="true" hidden={true}>
      <header className="page-topbar">
        <button type="button" className="icon-button back-button" data-navigate="HOME" aria-label="Back" title="Back" data-i18n-title="back">&larr;</button>
        <span id="chat-title" className="topbar-title" data-i18n="chat">Chat</span>
        <div className="topbar-actions">
          <button type="button" id="perm-mode-badge" className="perm-mode-badge" data-mode="read-only" aria-label="Permission mode" title="Permission mode" data-i18n-title="perm_mode_label">
            <span className="perm-mode-dot"></span>
            <span className="perm-mode-text">RO</span>
          </button>
          <div id="interaction-mode" className="button-group" role="group" aria-label="Interaction mode">
            <button type="button" id="mode-watch" className="button-group-option" data-interaction-mode="watch" aria-pressed="true">Watch</button>
            <button type="button" id="mode-control" className="button-group-option" data-interaction-mode="control" aria-pressed="false">Control</button>
          </div>
          <button type="button" id="chat-kanban-btn" className="icon-button" data-navigate="KANBAN" aria-label="Kanban board" title="Kanban board" data-i18n-title="kanban">&#x1F4CA;</button>
          <button type="button" id="chat-terminal-btn" className="icon-button" data-navigate="TERMINAL" aria-label="Terminal" title="Terminal" data-i18n-title="terminal">&#x2328;</button>
        </div>
      </header>
      <div className="topbar-subtitle-row" aria-live="polite">
        <span id="chat-subtitle" className="subtitle-text"></span>
      </div>
      <div className="page-body">
        <section id="code-agent-conversation" className="agent-surface conversation-shell">
          <div className="agent-surface" hidden={true}></div>
          <div id="session-tabs" className="session-tabs"></div>
          <section id="semantic-card-rail" className="semantic-card-rail">
            <article id="research-brief" className="research-brief"></article>
          </section>
          <div id="structured-timeline" className="timeline"></div>
          <section id="mobile-inbox" className="panel">
            <div className="panel-heading">
              <h2 data-i18n="workbench">Workbench</h2>
              <span id="remote-message" className="subtle"></span>
            </div>
            <div id="surface-actions" className="button-row"></div>
            <div id="workbench-permissions" className="list">
              <label className="form-group">
                <span className="form-label" data-i18n="permission_required">Permission</span>
                <input id="permission-request-id" type="text" className="form-input" placeholder="request_123" autoComplete="off" />
              </label>
            </div>
          </section>
          <div id="mobile-command-palette" hidden={true}></div>
          <div id="slash-command-bar" className="button-row">
            <button type="button" className="shortcut-command" data-command="/help"><span className="shortcut-intent">Help</span><span className="shortcut-command">/help</span></button>
            <button type="button" className="shortcut-command" data-command="/permissions"><span className="shortcut-intent">Permissions</span><span className="shortcut-command">/permissions</span></button>
          </div>
          <div id="skill-command-bar" className="button-row">
            <button type="button" data-skill-command="$list"><span className="shortcut-intent">Skills</span><span className="shortcut-command">$list</span></button>
            <button type="button" data-skill-command="$inspect web-design-engineer"><span className="shortcut-intent">Inspect</span><span className="shortcut-command">$inspect web-design-engineer</span></button>
            <button type="button" data-skill-command="$web-design-engineer"><span className="shortcut-intent">Run</span><span className="shortcut-command">$web-design-engineer</span></button>
          </div>
          <form id="conversation-composer" className="composer-bar">
            <button type="button" className="icon-button composer-cmd-btn" data-cmd-palette="/" aria-label="Commands" title="Commands">/</button>
            <button type="button" className="icon-button composer-skill-btn" data-cmd-palette="$" aria-label="Skills" title="Skills">$</button>
            <textarea id="chat-input" rows="1" autoComplete="off" data-i18n-placeholder="message_placeholder" placeholder="Message Astra..." aria-label="Message input"></textarea>
            <button type="submit" className="primary send-button" aria-label="Send" data-i18n-title="send">Send</button>
          </form>
          <section id="agent-attachments" className="list">
            <div id="attachment-preview" className="artifact-preview"></div>
            <div className="button-row">
              <button type="button" id="attachment-clear" className="secondary" data-i18n="clear">Clear</button>
            </div>
            <div id="voice-status" className="subtle" data-i18n="voice_idle">Voice dictation idle</div>
          </section>
          <section id="artifact-list" className="panel">
            <div className="panel-heading">
              <h2 data-i18n="artifacts_label">Artifacts</h2>
              <span className="subtle" data-i18n="artifacts_label">Artifacts</span>
            </div>
            <div className="form-group">
              <input id="artifact-path-input" type="text" className="form-input" placeholder="reports/demo.diff" autoComplete="off" />
            </div>
            <div className="button-row">
              <button type="button" id="artifact-open-path" className="secondary">Open Artifact</button>
            </div>
            <div id="artifact-preview" className="artifact-preview"></div>
          </section>
          <section id="result-list" className="panel">
            <div className="panel-heading">
              <h2 data-i18n="result">Result</h2>
              <span className="subtle" data-i18n="result">Result</span>
            </div>
            <div id="result-preview" className="artifact-preview"></div>
          </section>
        </section>
      </div>
    </div>

    {/* ========== PAGE: KANBAN ========== */}
    <div id="page-KANBAN" className="page" data-page="true" hidden={true}>
      <header className="page-topbar">
        <button type="button" className="icon-button back-button" data-navigate="HOME" aria-label="Back" title="Back" data-i18n-title="back">&larr;</button>
        <span id="kanban-title" className="topbar-title" data-i18n="hermes_board">Hermes Board</span>
        <div className="topbar-actions">
          <button type="button" id="kanban-chat-btn" className="icon-button" data-navigate="CHAT" aria-label="Chat" title="Chat" data-i18n-title="chat">&#x1F4AC;</button>
        </div>
      </header>
      <div className="page-body">
        <div id="kanban-timeline" className="timeline"></div>
        <div id="hermes-board-shell" className="hermes-board-shell">
          <div className="hermes-board-layout">
            <section className="kanban-section hermes-inbox-panel" aria-labelledby="research-inbox-title">
              <div className="section-heading-row">
                <h3 id="research-inbox-title" className="kanban-section-title" data-i18n="research_inbox">Research Inbox</h3>
                <span id="research-inbox-summary" className="section-chip">0</span>
              </div>
              <div id="research-inbox-list" className="research-inbox-list"></div>
            </section>
            <section className="kanban-section hermes-loop-panel">
              <h3 className="kanban-section-title" data-i18n="progress">Progress</h3>
              <div id="kanban-progress" className="kanban-progress-bar">
                <div className="kanban-progress-fill" style={{ width: "0%" }}></div>
                <span className="kanban-progress-label">0%</span>
              </div>
              <h3 className="kanban-section-title">Loop</h3>
              <div id="kanban-loop-health" className="kanban-loop-health"></div>
              <div id="kanban-stage-policy" className="kanban-loop-health"></div>
              <div className="button-row hermes-action-row">
                <button type="button" className="primary compact" data-action-id="advance_research_loop">Advance</button>
                <button type="button" className="secondary compact" data-action-id="decide_research_stage">Decide</button>
                <button type="button" className="secondary compact" data-action-id="open_research">Open</button>
              </div>
            </section>
            <section className="kanban-section hermes-columns-panel" aria-labelledby="hermes-board-title">
              <div className="section-heading-row">
                <h3 id="hermes-board-title" className="kanban-section-title" data-i18n="board_columns">Board</h3>
                <span id="hermes-board-count" className="section-chip">0</span>
              </div>
              <div id="hermes-board-columns" className="hermes-board-columns"></div>
            </section>
          </div>
          <div className="kanban-support-grid">
            <section className="kanban-section">
              <h3 className="kanban-section-title" data-i18n="stages">Stages</h3>
              <div id="kanban-stages" className="kanban-stages-list"></div>
            </section>
            <section className="kanban-section">
              <h3 className="kanban-section-title" data-i18n="claims">Claims</h3>
              <div id="kanban-claims" className="kanban-claims-list"></div>
            </section>
            <section className="kanban-section">
              <h3 className="kanban-section-title">Recovery</h3>
              <div id="kanban-recovery-status" className="kanban-recovery-status"></div>
              <div className="form-group">
                <input id="recovery-trigger-id" type="text" className="form-input" placeholder="trigger_123" autoComplete="off" />
              </div>
              <div className="button-row">
                <button type="button" className="secondary compact" data-action-id="inspect_recovery_governance">Inspect</button>
                <button type="button" className="primary compact" data-action-id="retry_routine_trigger">Retry</button>
              </div>
              <div id="kanban-recovery-actions" className="kanban-recovery-actions"></div>
            </section>
            <section className="kanban-section">
              <h3 className="kanban-section-title" data-i18n="findings">Findings</h3>
              <div id="kanban-findings" className="kanban-findings-list"></div>
            </section>
          </div>
        </div>
        <div id="research-card-drawer" className="research-card-drawer" hidden aria-hidden="true">
          <button type="button" className="drawer-backdrop" data-drawer-close aria-label="Close"></button>
          <aside className="drawer-panel" role="dialog" aria-modal="true" aria-labelledby="research-card-drawer-title">
            <header className="drawer-header">
              <div>
                <div id="research-card-drawer-bucket" className="drawer-kicker">bucket</div>
                <h3 id="research-card-drawer-title">Research item</h3>
              </div>
              <button type="button" className="icon-button" data-drawer-close aria-label="Close" title="Close">x</button>
            </header>
            <div id="research-card-drawer-body" className="drawer-body"></div>
            <div id="research-card-drawer-actions" className="drawer-actions"></div>
          </aside>
        </div>
      </div>
      <div className="page-bottombar">
        <form id="kanban-composer" className="composer-bar">
          <button type="button" className="icon-button composer-cmd-btn" data-cmd-palette="/" aria-label="Commands" title="Commands">/</button>
          <button type="button" className="icon-button composer-skill-btn" data-cmd-palette="$" aria-label="Skills" title="Skills">$</button>
          <textarea id="kanban-input" rows="1" autoComplete="off" data-i18n-placeholder="message_placeholder" placeholder="Message Astra..." aria-label="Message input"></textarea>
          <button type="submit" className="primary send-button" aria-label="Send" data-i18n-title="send">Send</button>
        </form>
      </div>
    </div>

    {/* ========== PAGE: TERMINAL ========== */}
    <div id="page-TERMINAL" className="page" data-page="true" hidden={true}>
      <header className="page-topbar">
        <button type="button" className="icon-button back-button" data-navigate="HOME" aria-label="Back" title="Back" data-i18n-title="back">&larr;</button>
        <span className="topbar-title" data-i18n="terminal">Terminal</span>
      </header>
      <div id="terminal-panel" className="page-body page-body-terminal">
        <div id="terminal-screen" className="terminal-screen" tabIndex="0" role="textbox" aria-label="Terminal output"></div>
      </div>
      <div className="page-bottombar">
        <form id="terminal-composer" className="terminal-composer">
          <input id="terminal-input" type="text" placeholder="$" autoComplete="off" autoCorrect="off" autoCapitalize="off" spellCheck="false" />
        </form>
        <div id="terminal-extra-keys" className="shortcut-keys-row" aria-label="Terminal shortcut keys">
          <button type="button" className="shortcut-key" data-terminal-input="ctrl_c" aria-label="Ctrl-C">Ctrl-C</button>
          <button type="button" className="shortcut-key" data-terminal-input="tab" aria-label="Tab">Tab</button>
          <button type="button" className="shortcut-key" data-terminal-input="enter" aria-label="Enter">Enter</button>
          <button type="button" className="shortcut-key" data-terminal-input="ctrl_d" aria-label="Ctrl-D">Ctrl-D</button>
          <button type="button" className="shortcut-key" data-terminal-input="escape" aria-label="Esc">Esc</button>
        </div>
      </div>
    </div>

    {/* ========== PAGE: NEW SESSION ========== */}
    <div id="page-NEWSESSION" className="page" data-page="true" hidden={true}>
      <header className="page-topbar">
        <button type="button" className="icon-button back-button" data-navigate="HOME" aria-label="Back" title="Back" data-i18n-title="back">&larr;</button>
        <span className="topbar-title" data-i18n="new_session">New Session</span>
      </header>
      <div id="session-create-sheet" className="page-body session-create-sheet">
        <div className="form-group">
          <label className="form-label" htmlFor="new-session-title" data-i18n="session_title">Session Title</label>
          <input id="new-session-title" type="text" className="form-input" autoComplete="off" data-i18n-placeholder="session_title_placeholder" placeholder="Optional session name" />
        </div>
        <div className="form-group">
          <label className="form-label" data-i18n="server">Server</label>
          <div id="new-server" className="server-selector"></div>
        </div>
        <div className="form-group">
          <label className="form-label" htmlFor="new-cwd" data-i18n="directory">Directory</label>
          <input id="new-cwd" type="text" className="form-input" autoComplete="off" data-i18n-placeholder="directory_placeholder" placeholder="/path/to/project" />
        </div>
        <div className="form-group">
          <label className="form-label" data-i18n="model">Model</label>
          <div id="new-model-group" className="button-group" role="group" aria-label="Model selection">
            <button type="button" className="button-group-option" data-model="gpt-5.4" aria-pressed="true">gpt-5.4</button>
            <button type="button" className="button-group-option" data-model="5.5" aria-pressed="false">5.5</button>
            <button type="button" className="button-group-option" data-model="opus" aria-pressed="false">opus</button>
            <button type="button" className="button-group-option" data-model="sonnet" aria-pressed="false">sonnet</button>
          </div>
        </div>
        <div className="form-group">
          <label className="form-label" data-i18n="perm_mode_label">Permission Mode</label>
          <div id="new-perm-group" className="radio-card-group">
            <label className="radio-card">
              <input type="radio" name="session-permission-mode" value="read-only" />
              <span className="radio-card-content">
                <strong data-i18n="perm_read_only">Read Only</strong>
                <small data-i18n="perm_read_only_desc">Only read tools, mutations need approval</small>
              </span>
            </label>
            <label className="radio-card">
              <input type="radio" name="session-permission-mode" value="workspace-write" defaultChecked />
              <span className="radio-card-content">
                <strong data-i18n="perm_workspace_write">Workspace Write</strong>
                <small data-i18n="perm_workspace_write_desc">Workspace mutations allowed</small>
              </span>
            </label>
            <label className="radio-card">
              <input type="radio" name="session-permission-mode" value="danger-full-access" />
              <span className="radio-card-content">
                <strong data-i18n="perm_danger_full">Full Access</strong>
                <small data-i18n="perm_danger_full_desc">All tools allowed, no approval</small>
              </span>
            </label>
          </div>
        </div>
      </div>
      <div className="page-bottombar">
        <button type="button" id="new-session-start-btn" className="primary full-width" data-i18n="start_session">Start Session</button>
      </div>
    </div>

    {/* ========== OVERLAYS ========== */}

    {/* Permission Mode Bottom Sheet */}
    <div id="perm-mode-sheet" className="bottom-sheet" style={{ display: "none" }}>
      <div className="bottom-sheet-handle"></div>
      <div className="bottom-sheet-header">
        <h3 data-i18n="perm_change">Change Permission Mode</h3>
      </div>
      <div className="perm-mode-list">
        <button type="button" className="perm-mode-option" data-perm-mode="read-only">
          <span className="perm-mode-indicator perm-mode-ro"></span>
          <div className="perm-mode-info">
            <strong data-i18n="perm_read_only">Read Only</strong>
            <span data-i18n="perm_read_only_desc">Only read tools, mutations need approval</span>
          </div>
        </button>
        <button type="button" className="perm-mode-option" data-perm-mode="workspace-write">
          <span className="perm-mode-indicator perm-mode-ww"></span>
          <div className="perm-mode-info">
            <strong data-i18n="perm_workspace_write">Workspace Write</strong>
            <span data-i18n="perm_workspace_write_desc">Workspace mutations allowed</span>
          </div>
        </button>
        <button type="button" className="perm-mode-option" data-perm-mode="danger-full-access">
          <span className="perm-mode-indicator perm-mode-fa"></span>
          <div className="perm-mode-info">
            <strong data-i18n="perm_danger_full">Full Access</strong>
            <span data-i18n="perm_danger_full_desc">All tools allowed, no approval</span>
          </div>
        </button>
      </div>
      <button type="button" id="perm-sheet-cancel" className="bottom-sheet-cancel" data-i18n="cancel">Cancel</button>
    </div>

    {/* Settings Bottom Sheet */}
    <div id="mobile-settings-sheet" className="bottom-sheet settings-sheet" style={{ display: "none" }}>
      <div className="bottom-sheet-handle"></div>
      <div className="bottom-sheet-header">
        <h3 data-i18n="settings">Settings</h3>
      </div>
      <section id="server-settings" className="settings-group">
        <h4 data-i18n="server">Server</h4>
        <label className="form-label" htmlFor="server-origin" data-i18n="server_url">Server URL</label>
        <input id="server-origin" type="url" className="form-input" autoComplete="off" placeholder="https://..." />
        <div className="button-row">
          <button type="button" id="server-health" className="secondary" data-i18n="probe">Probe</button>
          <button type="button" id="server-reconnect" className="secondary" data-i18n="replay">Reconnect</button>
        </div>
        <div id="active-server-origin" className="subtle"></div>
        <label className="radio-card" htmlFor="remember-control-token" style={{ marginTop: "8px" }}>
          <input type="checkbox" id="remember-control-token" />
          <span className="radio-card-content">
            <strong data-i18n="remember_token">Remember token on this device</strong>
            <small data-i18n="token_required">Control token required</small>
          </span>
        </label>
      </section>
      <section className="settings-group" aria-label="Theme">
        <h4 data-i18n="theme">Theme</h4>
        <div className="choice-row" role="group" aria-label="Theme">
          <button type="button" id="theme-toggle-btn" data-theme-choice="light" aria-pressed="true" data-i18n="theme_light">Light</button>
          <button type="button" data-theme-choice="dark" aria-pressed="false" data-i18n="theme_dark">Dark</button>
        </div>
      </section>
      <section className="settings-group" aria-label="Language">
        <h4 data-i18n="language">Language</h4>
        <div className="choice-row" role="group" aria-label="Language">
          <button type="button" data-lang-choice="zh" aria-pressed="true" data-i18n="lang_zh">中文</button>
          <button type="button" data-lang-choice="en" aria-pressed="false" data-i18n="lang_en">English</button>
        </div>
      </section>
      <button type="button" id="settings-sheet-close" className="bottom-sheet-cancel" data-i18n="close">Close</button>
    </div>

    {/* Error Banner */}
    <div id="error-banner" className="error-banner" role="alert" style={{ display: "none" }}>
      <span id="error-banner-message" data-i18n="connection_lost">Connection lost</span>
      <button type="button" id="error-banner-retry" data-i18n="retry">Retry</button>
      <button type="button" id="error-banner-dismiss" data-i18n="dismiss">Dismiss</button>
    </div>

    <nav id="mobile-tabbar" className="mobile-tabbar" aria-label="Mobile navigation">
      <button type="button" data-mobile-target="code-agent-conversation" data-i18n="chat">Chat</button>
      <button type="button" data-mobile-target="mobile-inbox" data-i18n="workbench">Inbox</button>
      <button type="button" data-mobile-target="terminal-panel" data-i18n="terminal">Terminal</button>
    </nav>

    <template id="projected-action-buttons">
      <button type="button" className="projected-action-button" data-action-id="switch_session">switch_session</button>
      <button type="button" className="projected-action-button" data-action-id="terminal_attach">terminal_attach</button>
      <button type="button" className="projected-action-button" data-action-id="inspect_permissions">inspect_permissions</button>
      <button type="button" className="projected-action-button" data-action-id="interrupt_turn">interrupt_turn</button>
      <button type="button" className="projected-action-button" data-action-id="open_artifact">open_artifact</button>
      <button type="button" className="projected-action-button" data-action-id="approve_permission">approve_permission</button>
      <button type="button" className="projected-action-button" data-action-id="deny_permission">deny_permission</button>
    </template>

    {/* Debug Log (visible on mobile for troubleshooting) */}
    <div id="debug-panel" style={{ display: "none", position: "fixed", bottom: 0, left: 0, right: 0, maxHeight: "40vh", overflowY: "auto", background: "var(--surface)", borderTop: "2px solid var(--danger)", zIndex: 9000, fontFamily: "var(--mono)", fontSize: "11px", padding: "8px", color: "var(--text)", whiteSpace: "pre-wrap", wordBreak: "break-all" }}></div>

    {/* Toast Container */}
    <div id="toast-container" className="toast-container" aria-live="polite"></div>

    </div>
  );
}
