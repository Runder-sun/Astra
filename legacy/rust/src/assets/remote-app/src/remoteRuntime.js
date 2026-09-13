// ── Astra Remote — Mobile Frontend (Page Navigation Architecture) ──

// ── Central State ──
const state = {
  page: 'HOME',
  sessionId: null,
  serverOrigin: '',
  controlToken: '',
  cwd: '',
  clientId: 'phone-alpha',
  interactionMode: 'watch',
  eventsCursor: 0,
  eventsActive: false,
  reconnectAttempts: 0,
  projection: {},
  sessions: [],
  terminalBridge: null,
  streamingDrafts: new Map(),
  selectedResearchCard: null,
  agentBusy: false,
  pendingMessage: null,
  availableSkills: [],
};

const MAX_RECONNECT_ATTEMPTS = 3;
const serverStorageKey = 'research-remote-servers-v1';
const CONTROL_TOKEN_KEY = 'research-remote-control-token-v1';
let serverDirectory = [];
let latestHostSurfaceProjection = null;
let latestSessionTabs = [];
let voiceRecognition = null;
const terminalDecoder = new TextDecoder();

const SERVER_SETUP_GUIDE = {
  exampleOrigin: 'http://100.x.y.z:8787',
  daemonCommand: 'research-cli remote daemon --host 0.0.0.0 --port 8787',
  tailscaleCommand: 'remote tailscale serve-plan --daemon-port 8787',
  authority: 'host-surface command model',
  paletteState: 'palette action staged',
  conversationContract: 'conversation_primary_surface',
  terminalContract: 'terminal_compatibility_lane',
};

function registerInstallableAppShell() {
  if (!('serviceWorker' in navigator)) return;
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js', { scope: '/' }).catch((err) => {
      console.debug('Astra Remote service worker registration skipped', err);
    });
  }, { once: true });
}

function initialPageFromLocation() {
  const page = new URL(window.location.href).searchParams.get('page');
  return ['HOME', 'CHAT', 'KANBAN', 'TERMINAL', 'NEWSESSION'].includes(page) ? page : 'HOME';
}

const terminalViewport = {
  lines: [''],
  cursorRow: 0,
  cursorCol: 0,
  pendingEscape: '',
  maxLines: 2000,
};

// ── i18n ──
const I18N = {
  zh: {
    // App
    app_name: 'Astra Remote',
    connecting: '连接中...',
    connected: '已连接',
    disconnected: '已断开',
    // Tabs
    tab_home: '首页',
    tab_chat: '对话',
    tab_approvals: '审批',
    tab_terminal: '终端',
    // Chat
    thinking: '思考中...',
    send: '发送',
    message_placeholder: '给 Astra 发送消息，或输入 / 命令',
    waiting_for_agent: '等待 agent 完成后发送...',
    message_queued: '消息已排队，等待发送...',
    commands: '命令',
    skills: '技能',
    no_commands: '没有匹配的命令',
    no_skills: '没有匹配的技能',
    you: '你',
    astra: 'Astra',
    // Permission modes
    perm_read_only: '只读',
    perm_workspace_write: '工作区写入',
    perm_danger_full: '完全访问',
    perm_read_only_desc: '仅读取工具，修改需审批',
    perm_workspace_write_desc: '工作区内修改自动允许',
    perm_danger_full_desc: '全部允许，无需审批',
    perm_mode_label: '权限模式',
    perm_change: '切换权限模式',
    perm_changed: '权限模式已切换',
    more_lines: '更多行',
    // Permission approval
    approve: '批准',
    deny: '拒绝',
    permission_required: '需要权限',
    // Session
    new_session: '新建会话',
    session_title: '会话标题',
    directory: '工作目录',
    directory_placeholder: '/path/to/project',
    model: '模型',
    create_session: '创建会话',
    start_session: '开始',
    // Status
    running: '运行中',
    passed: '通过',
    failed: '失败',
    idle: '空闲',
    error: '错误',
    // Tool types
    tool: '工具',
    result: '结果',
    diff: '差异',
    code: '代码',
    test: '测试',
    status: '状态',
    research: '研究',
    terminal: '终端',
    // Settings
    settings: '设置',
    theme: '主题',
    theme_light: '日间',
    theme_dark: '夜间',
    language: '语言',
    lang_zh: '中文',
    lang_en: 'English',
    server: '服务器',
    server_url: '服务器地址',
    control_token: '控制令牌',
    connect: '连接',
    about: '关于',
    project: '项目',
    session: '会话',
    daemon: '守护进程',
    // Misc
    no_sessions: '暂无会话',
    no_claims: '暂无结论',
    no_findings: '暂无发现',
    no_servers: '暂无服务器',
    connected_to: '已连接到',
    pair_device: '配对设备',
    token_required: '请输入控制令牌',
    voice_idle: '语音输入就绪',
    voice_listening: '正在聆听...',
    images_selected: '张图片已选择',
    images_uploaded: '张图片已上传',
    image_attachments_cleared: '图片已清除',
    command_palette_opened: '命令面板已打开',
    sessions_opened: '会话列表已打开',
    approval_inbox_opened: '审批列表已打开',
    connected_ok: '已连接',
    reconnecting: '重新连接中...',
    terminal_attached: '终端已连接',
    terminal_bridge_connected: '终端通道已连接',
    terminal_closed: '终端已断开',
    terminal_error: '终端错误',
    remote_paired: '远程配对成功',
    server_saved: '服务器已保存',
    server_url_required: '请输入服务器地址',
    sessions: '会话',
    logs: '日志',
    pairing: '配对',
    lease_aware: '租约感知',
    workspace: '工作区',
    client_id: '客户端 ID',
    pair_ticket: '配对票据',
    remember_token: '在此设备记住令牌',
    session_actions: '会话操作',
    session_id: '会话 ID',
    target_client: '目标客户端',
    notification_kind: '通知类型',
    message: '消息',
    context: '上下文',
    context_subtitle: '记忆、会话、研究',
    fleet: '设备组',
    unchecked: '未检查',
    alias: '别名',
    active: '当前',
    save: '保存',
    current: '当前',
    probe: '探测',
    replay: '重放',
    commands: '命令',
    workbench: '工作台',
    branches: '分支',
    event_log: '事件日志',
    clear: '清空',
    owner: '所有者',
    lease: '租约',
    transport: '传输',
    cancel: '取消',
    confirm: '确认',
    close: '关闭',
    session_title_placeholder: '移动端开始',
    session_created: '会话已创建',
    // Semantic cards
    chat_label: '对话',
    memory_label: '记忆',
    reviews_label: '评审',
    changes_label: '变更',
    artifacts_label: '制品',
    terminal_lane: '终端',
    approval_label: '审批',
    security_label: '安全',
    research_brief: '研究概要',
    // Kanban
    kanban: '看板',
    hermes_board: 'Hermes 看板',
    research_inbox: '研究收件箱',
    board_columns: '看板',
    progress: '进度',
    stages: '阶段',
    claims: '主张',
    findings: '发现',
    chat: '对话',
    back: '返回',
    add_server: '+ 添加服务器',
    // Loading
    connecting_to_astra: '正在连接 Astra...',
    // Error
    connection_lost: '连接断开',
    retry: '重试',
    dismiss: '关闭',
    remote_request_failed: '远程请求失败',
  },
  en: {
    app_name: 'Astra Remote',
    connecting: 'Connecting...',
    connected: 'Connected',
    disconnected: 'Disconnected',
    tab_home: 'Home',
    tab_chat: 'Chat',
    tab_approvals: 'Approvals',
    tab_terminal: 'Terminal',
    thinking: 'Thinking...',
    send: 'Send',
    message_placeholder: 'Message Astra, or type / for commands',
    waiting_for_agent: 'Waiting for agent to finish...',
    message_queued: 'Message queued, waiting to send...',
    commands: 'Commands',
    skills: 'Skills',
    no_commands: 'No matching commands',
    no_skills: 'No matching skills',
    you: 'You',
    astra: 'Astra',
    perm_read_only: 'Read Only',
    perm_workspace_write: 'Workspace Write',
    perm_danger_full: 'Full Access',
    perm_read_only_desc: 'Only read tools, mutations need approval',
    perm_workspace_write_desc: 'Workspace mutations allowed',
    perm_danger_full_desc: 'All tools allowed, no approval',
    perm_mode_label: 'Permission Mode',
    perm_change: 'Change Permission Mode',
    perm_changed: 'Permission mode changed',
    more_lines: 'more lines',
    approve: 'Approve',
    deny: 'Deny',
    permission_required: 'Permission Required',
    new_session: 'New Session',
    session_title: 'Session Title',
    directory: 'Directory',
    directory_placeholder: '/path/to/project',
    model: 'Model',
    create_session: 'Create Session',
    start_session: 'Start',
    running: 'Running',
    passed: 'Passed',
    failed: 'Failed',
    idle: 'Idle',
    error: 'Error',
    tool: 'Tool',
    result: 'Result',
    diff: 'Diff',
    code: 'Code',
    test: 'Test',
    status: 'Status',
    research: 'Research',
    terminal: 'Terminal',
    settings: 'Settings',
    theme: 'Theme',
    theme_light: 'Light',
    theme_dark: 'Dark',
    language: 'Language',
    lang_zh: 'Chinese',
    lang_en: 'English',
    server: 'Server',
    server_url: 'Server URL',
    control_token: 'Control Token',
    connect: 'Connect',
    about: 'About',
    project: 'Project',
    session: 'Session',
    daemon: 'Daemon',
    no_sessions: 'No sessions',
    no_claims: 'No claims yet',
    no_findings: 'No findings yet',
    no_servers: 'No saved servers',
    connected_to: 'Connected to',
    pair_device: 'Pair Device',
    token_required: 'Control token required',
    voice_idle: 'Voice dictation idle',
    voice_listening: 'Listening...',
    images_selected: 'image(s) selected',
    images_uploaded: 'image(s) uploaded',
    image_attachments_cleared: 'Image attachments cleared',
    command_palette_opened: 'Command palette opened',
    sessions_opened: 'Sessions opened',
    approval_inbox_opened: 'Approval inbox opened',
    connected_ok: 'Connected',
    reconnecting: 'Reconnecting...',
    terminal_attached: 'Terminal attached',
    terminal_bridge_connected: 'Terminal bridge connected',
    terminal_closed: 'Terminal bridge closed',
    terminal_error: 'Terminal bridge error',
    remote_paired: 'Remote paired',
    server_saved: 'Server saved',
    server_url_required: 'Server URL is required',
    sessions: 'Sessions',
    logs: 'Logs',
    pairing: 'Pairing',
    lease_aware: 'lease-aware',
    workspace: 'Workspace',
    client_id: 'Client ID',
    pair_ticket: 'Pair Ticket',
    remember_token: 'Remember token on this device',
    session_actions: 'Session Actions',
    session_id: 'Session ID',
    target_client: 'Target Client',
    notification_kind: 'Notification Kind',
    message: 'Message',
    context: 'Context',
    context_subtitle: 'memory, sessions, research',
    fleet: 'Fleet',
    unchecked: 'unchecked',
    alias: 'Alias',
    active: 'Active',
    save: 'Save',
    current: 'Current',
    probe: 'Probe',
    replay: 'Replay',
    commands: 'Commands',
    workbench: 'Workbench',
    branches: 'Branches',
    event_log: 'Event Log',
    clear: 'Clear',
    owner: 'Owner',
    lease: 'Lease',
    transport: 'Transport',
    cancel: 'Cancel',
    confirm: 'Confirm',
    close: 'Close',
    session_title_placeholder: 'Mobile start',
    session_created: 'Session created',
    chat_label: 'Chat',
    memory_label: 'Memory',
    reviews_label: 'Reviews',
    changes_label: 'Changes',
    artifacts_label: 'Artifacts',
    terminal_lane: 'Terminal',
    approval_label: 'Approval',
    security_label: 'Security',
    research_brief: 'Research Brief',
    kanban: 'Kanban',
    hermes_board: 'Hermes Board',
    research_inbox: 'Research Inbox',
    board_columns: 'Board',
    progress: 'Progress',
    stages: 'Stages',
    claims: 'Claims',
    findings: 'Findings',
    chat: 'Chat',
    back: 'Back',
    add_server: '+ Add Server',
    connecting_to_astra: 'Connecting to Astra...',
    connection_lost: 'Connection lost',
    retry: 'Retry',
    dismiss: 'Dismiss',
    remote_request_failed: 'Remote request failed',
  }
};

const LANG_KEY = 'astra-remote-lang';

function currentLang() {
  const stored = localStorage.getItem(LANG_KEY);
  if (stored === 'zh' || stored === 'en') return stored;
  const nav = (navigator.language || '').toLowerCase();
  return nav.startsWith('zh') ? 'zh' : 'en';
}

function t(key) {
  return (I18N[currentLang()] && I18N[currentLang()][key]) || (I18N.en[key]) || key;
}

function setLanguage(lang) {
  const next = lang === 'en' ? 'en' : 'zh';
  localStorage.setItem(LANG_KEY, next);
  document.documentElement.lang = next;
  applyAllTranslations();
  document.querySelectorAll('[data-lang-choice]').forEach((button) => {
    button.setAttribute('aria-pressed', button.dataset.langChoice === next ? 'true' : 'false');
  });
}

function applyAllTranslations() {
  document.querySelectorAll('[data-i18n]').forEach(el => {
    const key = el.dataset.i18n;
    if (key) el.textContent = t(key);
  });
  document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
    const key = el.dataset.i18nPlaceholder;
    if (key) el.placeholder = t(key);
  });
  document.querySelectorAll('[data-i18n-title]').forEach(el => {
    const key = el.dataset.i18nTitle;
    if (key) el.title = t(key);
  });
}

// ── Theme ──
const THEME_KEY = 'astra-remote-theme';

function currentTheme() {
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === 'dark' || stored === 'light') return stored;
  if (window.matchMedia('(prefers-color-scheme: dark)').matches) return 'dark';
  return 'light';
}

function setTheme(theme) {
  const next = theme === 'dark' ? 'dark' : 'light';
  localStorage.setItem(THEME_KEY, next);
  document.documentElement.dataset.theme = next;
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.content = next === 'dark' ? '#0e0d0c' : '#fafaf9';
  document.querySelectorAll('[data-theme-choice]').forEach((button) => {
    button.setAttribute('aria-pressed', button.dataset.themeChoice === next ? 'true' : 'false');
  });
}

// ── Utility ──
function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

function log(label, value) {
  console.log(`[${new Date().toLocaleTimeString()}] ${label}`, value);
  const panel = document.getElementById('debug-panel');
  if (panel) {
    const ts = new Date().toLocaleTimeString();
    const text = typeof value === 'object' ? JSON.stringify(value) : String(value || '');
    panel.textContent += `[${ts}] ${label}: ${text.slice(0, 300)}\n`;
    panel.scrollTop = panel.scrollHeight;
  }
}

function normalizedError(err) {
  if (typeof err === 'string') {
    try {
      return JSON.parse(err);
    } catch (_parseErr) {
      return { error: { message: err } };
    }
  }
  return err || {};
}

function errorMessage(err) {
  const normalized = normalizedError(err);
  if (normalized?.error?.code === 'remote_daemon_control_token_required') {
    return 'Remote setup requires a control token. Restart the daemon with --control-token or set RESEARCH_CLI_REMOTE_DAEMON_TOKEN.';
  }
  return (
    normalized?.error?.hint ||
    normalized?.error?.message ||
    normalized?.data?.reason ||
    normalized?.message ||
    t('remote_request_failed')
  );
}

function isControlTokenSetupRequired(err) {
  return normalizedError(err)?.error?.code === 'remote_daemon_control_token_required';
}

function localControlTokenSetupError() {
  return {
    error: {
      code: 'remote_daemon_control_token_required',
      message: 'remote daemon API requires a configured control token',
    },
  };
}

function handleError(err) {
  log('error', err);
  if (isControlTokenSetupRequired(err)) {
    state.eventsActive = false;
    showMobileTokenPrompt();
    return;
  }
  updateConnectionStatus('error');
  showErrorBanner(errorMessage(err));
  if (state.reconnectAttempts < MAX_RECONNECT_ATTEMPTS && state.controlToken) {
    state.reconnectAttempts++;
    const delay = state.reconnectAttempts * 3000;
    setTimeout(() => { quickPairAndConnect().catch(handleError); }, delay);
  }
}

function showActionToast(message, kind = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;
  const toast = document.createElement('div');
  toast.className = 'action-toast';
  toast.dataset.kind = kind;
  toast.textContent = message;
  container.appendChild(toast);
  window.setTimeout(() => {
    if (toast.parentElement) toast.remove();
  }, 4000);
}

// ── Server Directory ──
function normalizeServerUrl(value) {
  const raw = String(value || '').trim();
  if (!raw) return '';
  try {
    const url = new URL(raw);
    return url.origin;
  } catch (_err) {
    return '';
  }
}

function loadServerDirectory() {
  const saved = JSON.parse(localStorage.getItem(serverStorageKey) || '{}');
  serverDirectory = (Array.isArray(saved.servers) ? saved.servers : [])
    .map((server) => ({
      alias: server.alias || server.origin || '',
      origin: normalizeServerUrl(server.origin),
    }))
    .filter((server) => server.origin);
  state.serverOrigin = normalizeServerUrl(saved.activeServerOrigin);
  if (saved.rememberControlToken) {
    const remembered = localStorage.getItem(CONTROL_TOKEN_KEY) || '';
    if (remembered) {
      state.controlToken = remembered;
    }
  }
}

function saveServerDirectory() {
  localStorage.setItem(
    serverStorageKey,
    JSON.stringify({
      activeServerOrigin: state.serverOrigin,
      rememberControlToken: Boolean(document.getElementById('remember-control-token')?.checked),
      servers: serverDirectory,
    }),
  );
}

function actionInputSchema(action) {
  const schema = {
    action_id: { type: 'string', required: true },
  };
  if (action?.requires_session) {
    schema.session_id = { type: 'string', required: true };
  }
  if (action?.requires_message) {
    schema.message = { type: 'string', required: true };
  }
  if (action?.requires_request_id) {
    schema.request_id = { type: 'string', required: true };
  }
  if (action?.requires_trigger_id) {
    schema.trigger_id = { type: 'string', required: true };
  }
  return schema;
}

function validateActionInput(action, payload = {}) {
  const schema = actionInputSchema(action);
  for (const [key, rule] of Object.entries(schema)) {
    if (rule.required && !String(payload[key] || '').trim()) {
      return { ok: false, error: `${key} is required` };
    }
  }
  return { ok: true, value: payload };
}

function renderSecuritySetupState(message) {
  const status = document.getElementById('server-health-status');
  if (status) status.textContent = message || '';
  const remoteMessage = document.getElementById('remote-message');
  if (remoteMessage && !message) {
    remoteMessage.textContent = `${SERVER_SETUP_GUIDE.exampleOrigin} · ${SERVER_SETUP_GUIDE.authority}`;
  }
}

function remoteRootElement() {
  return document.getElementById('remote-root') || document.body;
}

async function selectServer(origin) {
  const nextOrigin = normalizeServerUrl(origin);
  if (nextOrigin !== state.serverOrigin) {
    resetActiveServerRuntime();
  }
  state.serverOrigin = nextOrigin;
  saveServerDirectory();
  await probeServerHealth(state.serverOrigin);
  await refresh();
}

async function probeServerHealth(origin) {
  const base = normalizeServerUrl(origin) || activeApiBase();
  const response = await fetch(new URL('/api/health', base).toString(), {
    headers: apiHeaders(),
  });
  const payload = await response.json();
  if (!response.ok || payload.ok === false) {
    throw payload;
  }
  log('server health', payload);
  const label = document.getElementById('server-health-status');
  if (label) label.textContent = payload.data?.daemon_state || payload.data?.status || t('connected');
  renderSecuritySetupState(payload.data?.status || payload.data?.daemon_state || '');
  return payload;
}

function resetActiveServerRuntime() {
  state.eventsCursor = 0;
  if (state.terminalBridge && state.terminalBridge.readyState <= WebSocket.OPEN) {
    state.terminalBridge.close(1000, 'active server changed');
  }
  state.terminalBridge = null;
}

function reconnectRemote() {
  resetActiveServerRuntime();
  if (!state.controlToken) {
    showMobileTokenPrompt();
    return Promise.resolve();
  }
  return post('/api/reconnect', requestBody({
    client_id: state.clientId,
    after_seq: state.eventsCursor,
  }))
    .then((result) => {
      log('remote reconnect', result);
      const ready = result.data?.status?.remote_ready || result.data?.remote_ready;
      if (!ready) {
        return quickPairAndConnect();
      }
      updateConnectionStatus('connected');
      if (!state.eventsActive) connectEvents();
      return refresh();
    })
    .catch((err) => {
      log('reconnect fallback', err);
      return quickPairAndConnect();
    });
}

function rememberControlToken() {
  const checkbox = document.getElementById('remember-control-token');
  if (checkbox?.checked && state.controlToken) {
    localStorage.setItem(CONTROL_TOKEN_KEY, state.controlToken);
  } else {
    localStorage.removeItem(CONTROL_TOKEN_KEY);
  }
  saveServerDirectory();
}

// ── API Layer ──
function activeApiBase() {
  return state.serverOrigin || window.location.origin;
}

function apiUrl(path) {
  return new URL(path, activeApiBase()).toString();
}

function apiHeaders() {
  const headers = { 'Content-Type': 'application/json' };
  if (state.controlToken) {
    headers.Authorization = `Bearer ${state.controlToken}`;
  }
  return headers;
}

function requestBody(extra = {}) {
  return {
    cwd: state.cwd || undefined,
    ...extra,
  };
}

function hostSurfaceUrl() {
  const query = state.cwd ? `?cwd=${encodeURIComponent(state.cwd)}` : '';
  return `/api/host-surface${query}`;
}

function eventStreamUrl() {
  const url = new URL('/api/events', activeApiBase());
  if (state.cwd) url.searchParams.set('cwd', state.cwd);
  url.searchParams.set('after_seq', String(state.eventsCursor));
  url.searchParams.set('wait_ms', '25000');
  return url.toString();
}

async function api(path, options = {}) {
  const response = await fetch(apiUrl(path), {
    headers: apiHeaders(),
    ...options,
  });
  const text = await response.text();
  const json = text ? JSON.parse(text) : {};
  if (!response.ok || json.ok === false) {
    throw json;
  }
  return json;
}

async function post(path, body) {
  return api(path, {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

// ── Connection ──
async function startRemoteRuntime() {
  updateConnectionStatus('connecting');
  ensureCurrentServer();
  if (state.controlToken) {
    await quickPairAndConnect();
    return;
  }
  await bootstrapRemoteRuntimeWithoutToken();
}

async function bootstrapRemoteRuntimeWithoutToken() {
  try {
    await probeServerHealth(activeApiBase());
    if (await refresh() === false) return;
    if (!state.eventsActive) connectEvents();
    hideErrorBanner();
  } catch (err) {
    handleError(err);
    hideLoading();
  }
}

async function quickPairAndConnect() {
  updateConnectionStatus('connecting');
  try {
    await post('/api/quick-pair', requestBody({ client_id: state.clientId }));
    updateConnectionStatus('connected');
    ensureCurrentServer();
    if (!state.eventsActive) connectEvents();
    await refresh();
    hideErrorBanner();
  } catch (err) {
    log('quick-pair attempt', err);
    ensureCurrentServer();
    if (!state.eventsActive) connectEvents();
    try {
      await refresh();
      hideErrorBanner();
    } catch (refreshErr) {
      log('refresh after pair failed', refreshErr);
      hideLoading();
    }
  }
}

function ensureCurrentServer() {
  const origin = state.serverOrigin || window.location.origin;
  if (!origin) return;
  state.serverOrigin = origin;
  const exists = serverDirectory.some(s => s.origin === origin);
  if (!exists) {
    serverDirectory.push({ alias: origin.replace(/^https?:\/\//, ''), origin });
    saveServerDirectory();
  }
  if (state.availableSkills.length === 0) loadSkills();
}

function showMobileTokenPrompt() {
  let overlay = document.getElementById('mobile-token-prompt');
  if (overlay) return;
  overlay = document.createElement('div');
  overlay.id = 'mobile-token-prompt';
  overlay.innerHTML = `
    <div class="token-prompt-card">
      <div class="brand-mark" aria-hidden="true">A</div>
      <h2>Astra Remote</h2>
      <p>${t('token_required')}</p>
      <input id="mobile-token-input" type="password" placeholder="${t('control_token')}" autocomplete="off">
      <button type="button" id="mobile-token-submit">${t('connect')}</button>
    </div>
  `;
  document.body.appendChild(overlay);
  document.getElementById('mobile-token-submit').addEventListener('click', () => {
    const token = document.getElementById('mobile-token-input').value.trim();
    if (!token) return;
    state.controlToken = token;
    overlay.remove();
    showLoading();
    quickPairAndConnect();
  });
  document.getElementById('mobile-token-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') document.getElementById('mobile-token-submit').click();
  });
}

function setInteractionMode(mode) {
  state.interactionMode = mode === 'control' ? 'control' : 'watch';
  document.body.dataset.interactionMode = state.interactionMode;
  document.querySelectorAll('[data-interaction-mode]').forEach((button) => {
    button.setAttribute('aria-pressed', button.dataset.interactionMode === state.interactionMode ? 'true' : 'false');
  });
}

function requireControlMode() {
  if (state.interactionMode !== 'control') {
    showActionToast('Control mode required', 'error');
    return false;
  }
  return true;
}

// ── Events ──
async function pollEvents() {
  if (!state.eventsActive) return;
  let sawRemoteEvents = false;
  try {
    const response = await fetch(eventStreamUrl(), { headers: apiHeaders() });
    const body = await response.text();
    if (!response.ok) throw body;
    for (const match of body.matchAll(/^id: (\d+)$/gm)) {
      state.eventsCursor = Math.max(state.eventsCursor, Number.parseInt(match[1], 10) || state.eventsCursor);
    }
    const remoteEvents = parseRemoteEventStream(body);
    sawRemoteEvents = remoteEvents.length > 0;
    let needsRefresh = false;
    for (const event of remoteEvents) {
      needsRefresh = handleRemoteEvent(event) || needsRefresh;
    }
    if (needsRefresh || (body.includes('event: remote-event') && remoteEvents.length === 0)) {
      await refresh();
    }
  } catch (err) {
    log('poll error', err);
    if (isControlTokenSetupRequired(err)) {
      state.eventsActive = false;
      showMobileTokenPrompt();
      return;
    }
  } finally {
    if (state.eventsActive) {
      window.setTimeout(pollEvents, sawRemoteEvents ? 30 : 250);
    }
  }
}

function parseRemoteEventStream(body) {
  const events = [];
  for (const block of body.split(/\n\n+/)) {
    let eventName = '';
    const data = [];
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith('event:')) {
        eventName = line.slice('event:'.length).trim();
      } else if (line.startsWith('data:')) {
        data.push(line.slice('data:'.length).trimStart());
      }
    }
    if (eventName !== 'remote-event' || data.length === 0) continue;
    try {
      events.push(JSON.parse(data.join('\n')));
    } catch (_err) {
      log('event parse skipped', { block });
    }
  }
  return events;
}

function handleRemoteEvent(event) {
  if (event.event_name === 'remote_message_delta') {
    state.agentBusy = true;
    updateComposerState();
    renderStreamingAssistantDraft(event.payload || {});
    return false;
  }
  if (event.event_name === 'remote_message') {
    state.agentBusy = false;
    clearStreamingAssistantDraft(event.payload?.session_id);
    flushPendingMessage();
    updateComposerState();
    return true;
  }
  if (event.event_name === 'ownership_change') {
    log('ownership changed', event.payload);
    return false;
  }
  return true;
}

function renderSessionTabs(sessions, sessionId) {
  const container = document.getElementById('session-tabs');
  if (!container) return;
  container.textContent = '';
  for (const session of sessions || []) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = session.title || session.session_id || t('session');
    button.className = session.session_id === sessionId ? 'active' : '';
    button.addEventListener('click', () => {
      state.sessionId = session.session_id;
      navigate('CHAT', { sessionId: session.session_id });
    });
    container.append(button);
  }
}

function renderTranscriptTimeline(transcript) {
  const timeline = document.getElementById('structured-timeline') || document.getElementById('chat-timeline');
  if (!timeline) return;
  timeline.textContent = '';
  for (const line of transcript || []) {
    timeline.append(transcriptLineElement(line));
  }
}

function semanticCardElement(title, summary) {
  const el = document.createElement('article');
  el.className = 'research-brief';
  el.innerHTML = `<div><strong>${escapeHtml(title)}</strong><p>${escapeHtml(summary || '')}</p></div>`;
  return el;
}

function renderSemanticCards(projection) {
  const rail = document.getElementById('semantic-card-rail');
  if (!rail) return;
  rail.textContent = '';
  const research = projection?.research || {};
  const brief = semanticCardElement(
    research.active_thread_title || research.active_thread_id || t('research_brief'),
    research.next_recommended_action || '',
  );
  rail.append(brief);
}

function renderStructuredConversation(latestHostSurfaceProjection, transcript) {
  const sessionId = latestHostSurfaceProjection?.sessions?.active_session_id || state.sessionId;
  renderSessionTabs(latestSessionTabs, sessionId);
  renderSemanticCards(latestHostSurfaceProjection);
  renderTranscriptTimeline(transcript);
}

function renderActionForm(action) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'projected-action-button';
  button.dataset.actionId = action.action_id;
  button.dataset.disabledReason = action.disabled_reason || '';
  button.dataset.actionSchema = JSON.stringify(actionInputSchema(action));
  button.textContent = action.label || action.action_id;
  button.disabled = action.enabled === false;
  return button;
}

function boardEntryId(entry, index = 0) {
  return String(entry?.entry_id || entry?.id || `${entry?.source?.source_kind || 'entry'}:${entry?.source?.source_id || index}`);
}

function entryTitle(entry) {
  return String(entry?.title || entry?.text || entry?.summary || entry?.entry_id || 'Research item');
}

function entrySummary(entry) {
  return String(entry?.summary || entry?.detail || entry?.intent || entry?.status || '');
}

function entryBucket(entry) {
  return String(entry?.bucket_id || entry?.bucket || entry?.status || 'projected');
}

function entrySourceLine(entry) {
  const source = entry?.source || {};
  const parts = [
    source.source_kind || source.kind || entry?.source_kind,
    source.source_id || source.id || entry?.source_id,
  ].filter(Boolean);
  return parts.join(' / ') || entry?.write_authority || 'host projection';
}

function entryActions(entry) {
  const refs = Array.isArray(entry?.action_refs) ? entry.action_refs : [];
  if (refs.length > 0) return refs;
  const bucket = entryBucket(entry);
  const actionPolicy = entry?.action_policy || '';
  if (bucket === 'needs_approval' || actionPolicy === 'requires_approval') {
    return [{ action_id: 'decide_research_stage', label: 'Decide', intent: 'Resolve projected research gate' }];
  }
  if (bucket === 'needs_review') {
    return [{ action_id: 'open_research', label: 'Open', intent: 'Inspect review context' }];
  }
  if (bucket === 'repair_or_pivot') {
    return [{ action_id: 'inspect_recovery_governance', label: 'Inspect', intent: 'Inspect recovery context' }];
  }
  if (bucket === 'blocked') {
    return [{ action_id: 'open_research', label: 'Open', intent: 'Inspect blocking context' }];
  }
  if (bucket === 'ready_to_run') {
    return [{ action_id: 'advance_research_loop', label: 'Advance', intent: 'Advance the goal loop' }];
  }
  return [];
}

function boardBucketLabel(bucketId, buckets = []) {
  const bucket = buckets.find((candidate) => candidate.bucket_id === bucketId);
  return bucket?.label || String(bucketId || 'Projected');
}

function normalizeResearchCard(entry, index = 0, kind = 'board') {
  return {
    ...entry,
    entry_id: boardEntryId(entry, index),
    bucket_id: entryBucket(entry),
    title: entryTitle(entry),
    summary: entrySummary(entry),
    source_line: entrySourceLine(entry),
    surface_kind: kind,
    action_refs: entryActions(entry),
  };
}

function collectResearchInboxItems(research, boardEntries, taskPoolEntries, recoveryEntries) {
  const inbox = [];
  const pushEntries = (entries, kind, predicate) => {
    entries.filter(predicate).forEach((entry, index) => {
      inbox.push(normalizeResearchCard(entry, inbox.length + index, kind));
    });
  };

  pushEntries(taskPoolEntries, 'task_pool', (entry) => (
    ['needs_approval', 'needs_review', 'blocked', 'repair_or_pivot', 'running', 'ready_to_run'].includes(entryBucket(entry))
  ));
  pushEntries(boardEntries, 'board', (entry) => (
    ['needs_approval', 'needs_review', 'blocked', 'repair_or_pivot', 'running', 'ready_to_run', 'evidence_needed'].includes(entryBucket(entry))
  ));
  pushEntries(recoveryEntries, 'recovery', (entry) => {
    const retryBudget = Number(entry.retry_budget_remaining || 0);
    return retryBudget > 0 || ['retry', 'escalate', 'manual'].some((needle) => String(entry.decision || '').includes(needle));
  });

  const stagePolicy = research.stage_decision_policy || {};
  const decisions = Array.isArray(stagePolicy.decisions) ? stagePolicy.decisions : [];
  decisions.forEach((decision, index) => {
    inbox.unshift(normalizeResearchCard({
      entry_id: `stage-decision:${decision.stage_execution_id || index}`,
      bucket_id: 'needs_approval',
      title: decision.operation || 'Research stage decision',
      summary: [
        decision.stage_execution_id,
        decision.agent_id,
        decision.reason,
      ].filter(Boolean).join(' · '),
      status: stagePolicy.status || 'decision_needed',
      source: {
        source_kind: 'stage_decision_policy',
        source_id: decision.stage_execution_id || String(index),
      },
      action_refs: [{ action_id: 'decide_research_stage', label: 'Decide', intent: 'Approve projected stage decision' }],
    }, index, 'stage_policy'));
  });

  const seen = new Set();
  return inbox
    .filter((entry) => {
      const id = `${entry.surface_kind}:${entry.entry_id}`;
      if (seen.has(id)) return false;
      seen.add(id);
      return true;
    })
    .slice(0, 8);
}

function renderActionRefs(actions = []) {
  return actions.slice(0, 3).map((entryAction) => {
    const actionId = entryAction.action_id || '';
    const label = entryAction.label || actionId || 'Action';
    const inputKey = entryAction.input_key || '';
    const inputValue = entryAction.input_value || '';
    return `<button type="button" class="secondary compact" data-action-id="${escapeHtml(actionId)}" data-input-key="${escapeHtml(inputKey)}" data-input-value="${escapeHtml(inputValue)}">${escapeHtml(label)}</button>`;
  }).join('');
}

function bindResearchCardActions(root) {
  root?.querySelectorAll('[data-action-id]').forEach((button) => {
    button.addEventListener('click', (event) => {
      event.stopPropagation();
      if (button.dataset.inputKey === 'trigger_id' && button.dataset.inputValue) {
        const field = document.getElementById('recovery-trigger-id');
        if (field) field.value = button.dataset.inputValue;
      }
      if (button.dataset.triggerId) {
        const field = document.getElementById('recovery-trigger-id');
        if (field) field.value = button.dataset.triggerId;
      }
      executeProjectedAction(button).catch(handleError);
    });
  });
}

function renderResearchInbox(research, taskPoolEntries, boardEntries, recoveryEntries) {
  const summary = document.getElementById('research-inbox-summary');
  const list = document.getElementById('research-inbox-list');
  if (!list) return;

  const inbox = collectResearchInboxItems(research, boardEntries, taskPoolEntries, recoveryEntries);
  if (summary) {
    const gated = inbox.filter((entry) => ['needs_approval', 'needs_review', 'blocked'].includes(entry.bucket_id)).length;
    summary.textContent = `${inbox.length} / ${gated}`;
    summary.title = `${inbox.length} inbox items, ${gated} gated`;
  }

  list.textContent = '';
  if (inbox.length === 0) {
    list.innerHTML = `<p class="empty">No inbox items</p>`;
    return;
  }

  inbox.forEach((entry, index) => {
    const card = document.createElement('button');
    card.type = 'button';
    card.className = 'research-inbox-card';
    card.dataset.entryId = entry.entry_id;
    card.dataset.bucket = entry.bucket_id;
    card.innerHTML = `
      <span class="inbox-rank">${index + 1}</span>
      <span class="inbox-main">
        <strong>${escapeHtml(entry.title)}</strong>
        <small>${escapeHtml([entry.summary, entry.source_line].filter(Boolean).join(' · '))}</small>
      </span>
      <span class="inbox-bucket">${escapeHtml(entry.bucket_id)}</span>
    `;
    card.addEventListener('click', () => openResearchCardDrawer(entry));
    list.append(card);
  });
}

function renderHermesBoardColumns(boardEntries, buckets = []) {
  const container = document.getElementById('hermes-board-columns');
  const count = document.getElementById('hermes-board-count');
  if (!container) return;

  const normalized = boardEntries.map((entry, index) => normalizeResearchCard(entry, index, 'board'));
  if (count) count.textContent = String(normalized.length);
  container.textContent = '';
  const activeBuckets = buckets.length > 0
    ? buckets
    : [...new Set(normalized.map((entry) => entry.bucket_id))].map((bucketId) => ({ bucket_id: bucketId, label: bucketId, description: '' }));

  activeBuckets.forEach((bucket) => {
    const entries = normalized.filter((entry) => entry.bucket_id === bucket.bucket_id);
    if (entries.length === 0) return;
    const column = document.createElement('section');
    column.className = 'hermes-board-column';
    column.dataset.bucket = bucket.bucket_id;
    column.innerHTML = `
      <header class="hermes-column-header">
        <div>
          <h4>${escapeHtml(bucket.label || bucket.bucket_id)}</h4>
          <p>${escapeHtml(bucket.description || bucket.bucket_id)}</p>
        </div>
        <span>${entries.length}</span>
      </header>
      <div class="hermes-column-cards"></div>
    `;
    const cards = column.querySelector('.hermes-column-cards');
    entries.forEach((entry) => {
      const card = document.createElement('button');
      card.type = 'button';
      card.className = 'hermes-board-card';
      card.dataset.entryId = entry.entry_id;
      card.innerHTML = `
        <span class="board-card-source">${escapeHtml(entry.source_line)}</span>
        <strong>${escapeHtml(entry.title)}</strong>
        <small>${escapeHtml(entry.summary || entry.status || '')}</small>
      `;
      card.addEventListener('click', () => openResearchCardDrawer(entry));
      cards.append(card);
    });
    container.append(column);
  });

  if (!container.childElementCount) {
    container.innerHTML = `<p class="empty">No board entries</p>`;
  }
}

function openResearchCardDrawer(entry) {
  state.selectedResearchCard = entry;
  const drawer = document.getElementById('research-card-drawer');
  const title = document.getElementById('research-card-drawer-title');
  const bucket = document.getElementById('research-card-drawer-bucket');
  const body = document.getElementById('research-card-drawer-body');
  const actions = document.getElementById('research-card-drawer-actions');
  if (!drawer || !title || !bucket || !body || !actions) return;

  const research = state.projection?.research || {};
  const board = research.board || {};
  const buckets = Array.isArray(board.buckets) ? board.buckets : [];
  title.textContent = entry.title || entryTitle(entry);
  bucket.textContent = boardBucketLabel(entry.bucket_id, buckets);
  body.innerHTML = `
    <dl class="drawer-facts">
      <div><dt>Status</dt><dd>${escapeHtml(entry.status || 'projected')}</dd></div>
      <div><dt>Source</dt><dd>${escapeHtml(entry.source_line || entrySourceLine(entry))}</dd></div>
      <div><dt>Authority</dt><dd>${escapeHtml(entry.write_authority || entry.surface_kind || 'host projection')}</dd></div>
      <div><dt>Policy</dt><dd>${escapeHtml(entry.action_policy || 'read_only_projection')}</dd></div>
    </dl>
    <p class="drawer-summary">${escapeHtml(entry.summary || entrySummary(entry) || 'No summary')}</p>
  `;
  const actionHtml = renderActionRefs(entry.action_refs || entryActions(entry));
  actions.innerHTML = actionHtml || `<button type="button" class="secondary compact" data-action-id="open_research">Open Research</button>`;
  bindResearchCardActions(actions);
  drawer.hidden = false;
  drawer.setAttribute('aria-hidden', 'false');
}

function closeResearchCardDrawer() {
  const drawer = document.getElementById('research-card-drawer');
  if (!drawer) return;
  state.selectedResearchCard = null;
  drawer.hidden = true;
  drawer.setAttribute('aria-hidden', 'true');
}

function executeProjectedAction(action) {
  const actionId = action?.dataset?.actionId || action?.action_id || action;
  const actionModel = action?.action_id ? action : state.projection?.actions?.find((candidate) => candidate.action_id === actionId) || { action_id: actionId };
  const payload = {
    action_id: actionId,
    session_id: state.sessionId || undefined,
  };
  if (actionId === 'approve_permission' || actionId === 'deny_permission') {
    payload.request_id = document.getElementById('permission-request-id')?.value.trim();
  }
  if (actionId === 'retry_routine_trigger') {
    payload.trigger_id = action?.dataset?.triggerId || action?.dataset?.inputValue || document.getElementById('recovery-trigger-id')?.value.trim();
  }
  if (actionId === 'decide_research_stage') {
    const research = state.projection?.research || {};
    const decision = research.stage_decision_policy?.decisions?.[0] || {};
    payload.thread_id = research.active_thread_id || undefined;
    payload.stage_execution_id = decision.stage_execution_id || undefined;
    payload.operation = decision.operation || 'advance';
    payload.decision = 'approve';
    payload.message = 'operator approved projected research stage decision from mobile kanban';
  }
  if (actionId === 'interrupt_turn') {
    payload.message = 'interrupt requested from mobile control mode';
  }
  const validation = validateActionInput(actionModel, payload);
  if (!validation.ok) {
    showActionToast(validation.error, 'error');
    return Promise.resolve();
  }
  return api('/api/tui/action', {
    method: 'POST',
    body: JSON.stringify(requestBody(validation.value)),
  });
}

function handleCommandButton(button) {
  const command = button?.dataset?.command || '';
  const input = document.getElementById('chat-input') || document.getElementById('kanban-input');
  if (input) {
    input.value = command;
    input.focus();
  }
}

function executeProductCommand(command) {
  if (command === '/terminal') {
    navigate('TERMINAL');
    return;
  }
  if (command === '/artifacts') {
    fetchArtifactIndex().then(renderArtifactPane).catch(handleError);
    return;
  }
  if (command === '/status') {
    renderSecuritySetupState(formatTerminalProjection(state.projection?.terminal));
    return;
  }
  handleCommandButton({ dataset: { command } });
}

function executeSkillCommand(command) {
  handleCommandButton({ dataset: { command } });
}

function shortcutIntentForButton(button) {
  return button?.textContent || button?.dataset?.command || button?.dataset?.skillCommand || '';
}

function shortcutCommandSpan(text) {
  const span = document.createElement('span');
  span.className = "shortcut-command";
  span.textContent = text || '';
  return span;
}

function shortcutIntentSpan(text) {
  const span = document.createElement('span');
  span.className = "shortcut-intent";
  span.textContent = text || '';
  return span;
}

function refreshCommandShortcuts() {
  document.querySelectorAll('[data-command],[data-skill-command]').forEach((button) => {
    button.classList.add('shortcut-command');
    if (!button.querySelector('.shortcut-intent')) {
      button.prepend(shortcutIntentSpan(shortcutIntentForButton(button)));
    }
    if (!button.querySelector('.shortcut-command')) {
      button.append(shortcutCommandSpan(button.dataset.command || button.dataset.skillCommand || ''));
    }
  });
}

function enhanceFleetServerGuide() {
  const guide = document.getElementById('server-directory');
  if (guide) guide.dataset.enhanced = 'true';
}

function enhanceTerminalExtraKeys() {
  const keys = document.getElementById('terminal-extra-keys');
  if (keys) keys.dataset.contract = 'terminal-signal';
}

function parseSkillCommand(command) {
  return String(command || '').trim();
}

function formatTerminalProjection(terminal) {
  return terminal?.lane_state || terminal?.xterm_bridge_state || 'terminal';
}

function renderTerminalHttpResult(result) {
  const screen = document.getElementById('terminal-screen');
  if (!screen) return;
  screen.textContent = JSON.stringify(result, null, 2);
}

function fetchArtifactIndex() {
  const result = { data: { remote_ready: Boolean(state.projection?.surfaces?.remote?.ready) } };
  if (result.data?.remote_ready === false) {
    log('skip unpaired artifact/result fetch', result);
    return Promise.resolve(result);
  }
  return api('/api/artifacts');
}

function renderArtifactPane(result) {
  const target = document.getElementById('artifact-preview');
  if (target) target.textContent = JSON.stringify(result, null, 2);
}

function inspectArtifact(path) {
  const url = `/api/artifact/inspect?cwd=${encodeURIComponent(state.cwd || '')}&target=${encodeURIComponent(path)}`;
  return api(url);
}

function openArtifactPath() {
  const input = document.getElementById('artifact-path-input');
  const path = input?.value.trim();
  if (!path) return Promise.resolve();
  return inspectArtifact(path).then((result) => {
    renderArtifactPane(result);
    return result;
  });
}

function fetchResultPanel() {
  const result = { data: { remote_ready: Boolean(state.projection?.surfaces?.remote?.ready) } };
  if (result.data?.remote_ready === false) {
    log('skip unpaired artifact/result fetch', result);
    return Promise.resolve(result);
  }
  return api('/api/results');
}

function renderResultPanel(result) {
  const target = document.getElementById('result-preview');
  if (target) target.textContent = JSON.stringify(result, null, 2);
}

function uploadConversationAttachments() {
  return api('/api/session/attachments', {
    method: 'POST',
    body: JSON.stringify(requestBody({ attachments: [] })),
  });
}

function renderAttachmentPreview(payload) {
  const target = document.getElementById('attachment-preview');
  if (target) target.textContent = JSON.stringify(payload, null, 2);
}

function clearConversationAttachments() {
  const target = document.getElementById('attachment-preview');
  if (target) target.textContent = '';
}

function startVoiceDictation() {
  const SpeechRecognitionCtor = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SpeechRecognitionCtor) {
    showActionToast('SpeechRecognition unavailable', 'error');
    return;
  }
  if (!voiceRecognition) {
    voiceRecognition = new SpeechRecognitionCtor();
    voiceRecognition.continuous = false;
    voiceRecognition.interimResults = false;
    voiceRecognition.addEventListener('result', (event) => {
      const transcript = Array.from(event.results || [])
        .map((result) => result[0]?.transcript || '')
        .join('');
      appendDictationTranscript(transcript);
    });
    voiceRecognition.addEventListener('end', stopVoiceDictation);
  }
  const status = document.getElementById('voice-status');
  if (status) status.textContent = t('voice_listening');
  voiceRecognition.start();
}

function stopVoiceDictation() {
  const status = document.getElementById('voice-status');
  if (status) status.textContent = t('voice_idle');
  if (voiceRecognition) {
    try { voiceRecognition.stop(); } catch (_err) {}
  }
}

function appendDictationTranscript(text) {
  const input = document.getElementById('chat-input');
  if (input) input.value = `${input.value}${text || ''}`;
}

async function renderServerDirectory() {
  const container = document.getElementById('server-list');
  const origin = document.getElementById('server-origin');
  const current = document.getElementById('active-server-origin');
  if (origin) origin.textContent = state.serverOrigin || '';
  if (current) current.textContent = state.serverOrigin || '';
  if (!container) return;
  container.textContent = '';
  for (const server of serverDirectory) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = server.alias || server.origin;
    button.addEventListener('click', () => selectServer(server.origin).catch(handleError));
    container.append(button);
  }
}

function connectEvents() {
  state.eventsActive = true;
  pollEvents();
}

// ── Streaming Drafts ──
function renderStreamingAssistantDraft(payload) {
  const sessionId = payload.session_id || 'active';
  const content = payload.accumulated_content || payload.delta || '';
  if (!content) return;

  const timeline = document.getElementById('structured-timeline') || document.getElementById('chat-timeline');
  if (!timeline) return;
  timeline.querySelector('.empty')?.remove();

  let item = state.streamingDrafts.get(sessionId);
  if (!item || !timeline.contains(item)) {
    item = document.createElement('article');
    item.className = 'turn turn--streaming';
    item.dataset.type = 'assistant';
    item.dataset.streamingSession = sessionId;
    item.innerHTML = `
      <div class="thinking-indicator" id="thinking-${sessionId}">
        <span class="thinking-spinner">◔</span>
        <span class="thinking-text">${t('thinking')}</span>
      </div>
      <div class="turn-body" style="display:none"></div>
    `;
    timeline.append(item);
    state.streamingDrafts.set(sessionId, item);
  }
  const thinking = item.querySelector('.thinking-indicator');
  const body = item.querySelector('.turn-body');
  if (thinking && content.trim()) {
    thinking.style.display = 'none';
    body.style.display = 'block';
  }
  body.textContent = content;
  timeline.scrollTop = timeline.scrollHeight;
}

function clearStreamingAssistantDraft(sessionId) {
  const ids = sessionId ? [sessionId] : Array.from(state.streamingDrafts.keys());
  for (const id of ids) {
    const item = state.streamingDrafts.get(id);
    if (item?.parentElement) item.remove();
    state.streamingDrafts.delete(id);
  }
}

// ── Slash Commands & Skills ──
const SLASH_COMMANDS = [
  { typed: '/help',        label: 'Help',           summary: 'Show available commands' },
  { typed: '/prompt',      label: 'Prompt',         summary: 'Run an explicit prompt turn' },
  { typed: '/exc',         label: 'Interrupt',      summary: 'Interrupt the active turn' },
  { typed: '/exit',        label: 'Exit',           summary: 'Close session' },
  { typed: '/sessions',    label: 'Sessions',       summary: 'Browse and switch sessions' },
  { typed: '/model',       label: 'Model',          summary: 'Change AI model' },
  { typed: '/reasoning',   label: 'Reasoning',      summary: 'Change reasoning effort' },
  { typed: '/language',    label: 'Language',       summary: 'Switch language (zh/en)' },
  { typed: '/theme',       label: 'Theme',          summary: 'Switch light/dark theme' },
  { typed: '/permissions', label: 'Permissions',    summary: 'Inspect pending approvals' },
  { typed: '/approve',     label: 'Approve',        summary: 'Approve a permission request' },
  { typed: '/deny',        label: 'Deny',           summary: 'Deny a permission request' },
  { typed: '/terminal',    label: 'Terminal',       summary: 'Open terminal view' },
  { typed: '/fold',        label: 'Fold Output',    summary: 'Collapse long output blocks' },
  { typed: '/expand',      label: 'Expand Output',  summary: 'Expand collapsed output' },
  { typed: '/output',      label: 'Output',         summary: 'Show structured output blocks' },
  { typed: '/logs',        label: 'Logs',           summary: 'Show recent log blocks' },
  { typed: '/artifacts',   label: 'Artifacts',      summary: 'Open artifact previews' },
  { typed: '/memory',      label: 'Memory',         summary: 'Inspect project memory' },
  { typed: '/research',    label: 'Research',       summary: 'Show research brief' },
  { typed: '/status',      label: 'Status',         summary: 'Show session status' },
  { typed: '/continue',    label: 'Continue',       summary: 'Continue latest session' },
  { typed: '/diff',        label: 'Diff',           summary: 'Show workspace changes' },
  { typed: '/commit',      label: 'Commit',         summary: 'Stage and commit workflow' },
];

function showCommandPalette(prefix) {
  hideCommandPalette();
  const isSkill = prefix.startsWith('$');
  const query = prefix.slice(1).toLowerCase();

  const palette = document.createElement('div');
  palette.id = 'command-palette';
  palette.className = 'command-palette';
  palette.innerHTML = '<div class="command-palette-title">' +
    (isSkill ? t('skills') : t('commands')) + '</div>';

  let items;
  if (isSkill) {
    items = state.availableSkills
      .filter(s => {
        const description = String(s.description || '').toLowerCase();
        return !query || s.skill_id.toLowerCase().includes(query) || description.includes(query);
      })
      .map(s => ({ typed: '$' + s.skill_id, label: s.skill_id, summary: s.description || '' }));
  } else {
    items = SLASH_COMMANDS
      .filter(c => !query || c.typed.toLowerCase().includes(query) || c.label.toLowerCase().includes(query))
      .map(c => ({ typed: c.typed, label: c.label, summary: c.summary }));
  }

  for (const item of items) {
    const row = document.createElement('div');
    row.className = 'command-palette-item';
    row.dataset.typed = item.typed;
    row.innerHTML = `<span class="cp-typed">${escapeHtml(item.typed)}</span>` +
      (item.summary ? `<span class="cp-summary">${escapeHtml(item.summary)}</span>` : '');
    row.addEventListener('click', () => {
      const input = document.getElementById('chat-input') || document.getElementById('kanban-input');
      if (input) { input.value = item.typed + ' '; input.focus(); }
      hideCommandPalette();
    });
    palette.append(row);
  }

  if (items.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'command-palette-empty';
    empty.textContent = isSkill ? t('no_skills') : t('no_commands');
    palette.append(empty);
  }

  const bottombar = document.querySelector('#page-CHAT .page-bottombar, #page-KANBAN .page-bottombar');
  if (bottombar) bottombar.prepend(palette);
}

function hideCommandPalette() {
  document.getElementById('command-palette')?.remove();
}

async function loadSkills() {
  try {
    const result = await api('/api/skills');
    state.availableSkills = result.data?.skills || [];
  } catch (_) {}
}

// ── Permission Mode ──
function updatePermBadge(mode) {
  const badge = document.getElementById('perm-mode-badge') || document.getElementById('chat-perm-badge');
  if (!badge) return;
  badge.dataset.mode = mode || 'read-only';
  const text = badge.querySelector('.perm-mode-text');
  if (text) {
    const labels = {
      'read-only': 'RO',
      'workspace-write': 'WW',
      'danger-full-access': 'FA',
    };
    text.textContent = labels[mode] || 'RO';
  }
}

function showPermModeSheet() {
  const sheet = document.getElementById('perm-mode-sheet');
  if (!sheet) return;
  sheet.style.display = 'flex';
  const backdrop = document.createElement('div');
  backdrop.className = 'bottom-sheet-backdrop';
  backdrop.id = 'perm-backdrop';
  backdrop.addEventListener('click', hidePermModeSheet);
  sheet.parentNode.insertBefore(backdrop, sheet);
  const current = (document.getElementById('perm-mode-badge') || document.getElementById('chat-perm-badge'))?.dataset.mode || 'read-only';
  sheet.querySelectorAll('.perm-mode-option').forEach(opt => {
    opt.classList.toggle('active', opt.dataset.permMode === current);
  });
}

function hidePermModeSheet() {
  const sheet = document.getElementById('perm-mode-sheet');
  if (sheet) sheet.style.display = 'none';
  document.getElementById('perm-backdrop')?.remove();
}

async function changePermissionMode(mode) {
  hidePermModeSheet();
  const result = await api('/api/session/permission-mode', {
    method: 'PATCH',
    body: JSON.stringify({
      permission_mode: mode,
      cwd: state.cwd || undefined,
    }),
  });
  updatePermBadge(mode);
  showActionToast(t('perm_changed'), 'ok');
  log('permission mode change', result);
  await refresh();
}

// ── Session ──
async function createSession() {
  const titleInput = document.getElementById('new-session-title');
  const title = titleInput?.value.trim() || undefined;
  const cwdInput = document.getElementById('new-cwd');
  const cwd = cwdInput?.value.trim() || undefined;
  const selectedModel = document.querySelector('#new-model-group .button-group-option[aria-pressed="true"]');
  const model = selectedModel?.dataset.model || undefined;
  const permissionMode = document.querySelector('input[name="session-permission-mode"]:checked')?.value || 'workspace-write';
  const result = await post('/api/session/create', requestBody({
    title,
    cwd,
    model,
    permission_mode: permissionMode,
  }));
  const session = result.data?.session || {};
  const sessionId = result.session_id || session.session_id;
  if (sessionId) {
    state.sessionId = sessionId;
  }
  updatePermBadge(permissionMode);
  if (titleInput) titleInput.value = '';
  showActionToast(t('session_created'), 'ok');
  log('session create', result);
  navigate('CHAT', { sessionId });
}

async function resumeSession(sessionId) {
  if (!sessionId) return;
  const result = await post('/api/session/resume', requestBody({ session_id: sessionId }));
  log('session resume', result);
  state.sessionId = sessionId;
  await refresh();
  navigate('CHAT', { sessionId });
}

// ── Transcript Rendering ──
function extractToolPath(args) {
  try {
    const parsed = JSON.parse(args);
    return parsed.path || parsed.file_path || parsed.target_path || parsed.directory || '';
  } catch { return ''; }
}

function truncatePath(path) {
  if (!path) return '';
  const parts = path.split('/');
  return parts.length > 3 ? '.../' + parts.slice(-2).join('/') : path;
}

function classifyOutputLine(line) {
  if (!line) return '';
  if (line.startsWith('+++') || line.startsWith('---')) return '';
  if (line.startsWith('+')) return 'diff-line-add';
  if (line.startsWith('-')) return 'diff-line-del';
  if (line.startsWith('@@')) return 'diff-line-hunk';
  if (line.startsWith('$') || line.startsWith('>')) return 'output-cmd';
  return '';
}

function transcriptLineElement(line) {
  const item = document.createElement('article');
  if (line.line_type === 'message') {
    item.className = `turn ${line.line_type}`;
    item.dataset.type = line.role || 'assistant';
    const label = line.role === 'user' ? t('you') : t('astra');
    item.innerHTML = `
      <div class="turn-header">
        <span class="turn-label">${escapeHtml(label)}</span>
      </div>
      <div class="turn-body">${escapeHtml(line.content || '')}</div>
    `;
    return item;
  }
  if (line.line_type === 'tool_call') {
    item.className = 'turn tool';
    item.dataset.type = 'tool';
    const path = extractToolPath(line.arguments || '');
    const shortPath = truncatePath(path);
    item.innerHTML = `
      <div class="tool-row" role="button" tabindex="0">
        <span class="turn-icon" data-type="tool">▸</span>
        <span class="tool-name">${escapeHtml(line.tool_name || 'tool')}</span>
        ${shortPath ? `<span class="tool-path">${escapeHtml(shortPath)}</span>` : ''}
        <span class="turn-status" data-status="running">${t('running')}</span>
      </div>
      <div class="tool-detail" style="display:none">
        <div class="turn-body">${escapeHtml(line.arguments || '')}</div>
      </div>
    `;
    const row = item.querySelector('.tool-row');
    const detail = item.querySelector('.tool-detail');
    const icon = item.querySelector('.turn-icon');
    row.addEventListener('click', () => {
      const expanded = detail.style.display !== 'none';
      detail.style.display = expanded ? 'none' : 'block';
      icon.textContent = expanded ? '▸' : '▾';
    });
    return item;
  }
  if (line.line_type === 'tool_result') {
    const outputStr = line.output || '';
    const passed = /\b(ok|success|passed|done|complete)\s*$/im.test(outputStr.split('\n').filter(l => l.trim()).pop() || '');
    item.className = 'turn tool';
    item.dataset.type = 'tool';
    const outputLines = (line.output || '').split('\n');
    const shouldFold = outputLines.length > 12;
    const visibleLines = shouldFold ? outputLines.slice(0, 5) : outputLines;
    item.innerHTML = `
      <div class="tool-row" role="button" tabindex="0">
        <span class="turn-icon" data-type="tool">▸</span>
        <span class="tool-name">${t('result')}</span>
        <span class="turn-status" data-status="${passed ? 'passed' : 'failed'}">${passed ? t('passed') : t('failed')}</span>
      </div>
      <div class="tool-detail" style="display:none">
        ${visibleLines.map(l => `<div class="output-line ${classifyOutputLine(l)}">${escapeHtml(l)}</div>`).join('')}
        ${shouldFold ? `<button type="button" class="fold-hint" data-remaining="${outputLines.length - 5}">▸ ${outputLines.length - 5} ${t('more_lines')}</button>` : ''}
      </div>
    `;
    const row = item.querySelector('.tool-row');
    const detail = item.querySelector('.tool-detail');
    const icon = item.querySelector('.turn-icon');
    row.addEventListener('click', () => {
      const expanded = detail.style.display !== 'none';
      detail.style.display = expanded ? 'none' : 'block';
      icon.textContent = expanded ? '▸' : '▾';
    });
    const foldBtn = item.querySelector('.fold-hint');
    if (foldBtn) {
      foldBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        const remaining = outputLines.slice(5);
        const container = foldBtn.parentElement;
        remaining.forEach(l => {
          const div = document.createElement('div');
          div.className = `output-line ${classifyOutputLine(l)}`;
          div.textContent = l;
          container.insertBefore(div, foldBtn);
        });
        foldBtn.remove();
      });
    }
    return item;
  }
  item.className = 'turn status';
  item.dataset.type = 'status';
  item.innerHTML = `
    <div class="turn-header">
      <span class="turn-icon" data-type="status">○</span>
      <span class="turn-label">${escapeHtml(line.line_type || 'event')}</span>
    </div>
    <div class="turn-body">${escapeHtml(line.event || line.summary_ref || JSON.stringify(line))}</div>
  `;
  return item;
}

// ── Terminal Subsystem ──
async function attachTerminal() {
  const result = await post('/api/terminal/attach', requestBody());
  const terminal = result.data?.terminal || {};
  log('terminal attach', terminal);
  await replayTerminal();
  await connectTerminalBridge();
}

async function replayTerminal() {
  const query = state.cwd ? `?cwd=${encodeURIComponent(state.cwd)}` : '';
  const result = await api(`/api/terminal/replay${query}`);
  const replay = result.data?.replay || result.data;
  if (replay && (replay.preview || replay.scrollback || replay.text)) {
    const screen = document.getElementById('terminal-screen');
    if (screen) {
      appendTerminalText(replay.preview || replay.scrollback || replay.text);
    }
  }
  log('terminal replay', result);
}

async function terminalBridgeUrl() {
  const ticketResult = await post('/api/terminal/ws-ticket', requestBody());
  const ticket = ticketResult.data?.ticket;
  if (!ticket) {
    throw new Error('terminal websocket ticket was not returned');
  }
  const params = new URLSearchParams();
  if (state.cwd) params.set('cwd', state.cwd);
  params.set('ticket', ticket);
  const query = params.toString() ? `?${params.toString()}` : '';
  const base = new URL(activeApiBase());
  const scheme = base.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${scheme}//${base.host}/api/terminal/ws${query}`;
}

async function connectTerminalBridge() {
  if (state.terminalBridge && state.terminalBridge.readyState <= WebSocket.OPEN) {
    return state.terminalBridge;
  }
  const bridgeUrl = await terminalBridgeUrl();
  state.terminalBridge = new WebSocket(bridgeUrl);
  state.terminalBridge.addEventListener('open', () => {
    resetTerminalViewport();
    log('terminal bridge', { state: 'connected', url: bridgeUrl.replace(/ticket=[^&]+/, 'ticket=redacted') });
    showActionToast(t('terminal_bridge_connected'), 'ok');
  });
  state.terminalBridge.addEventListener('message', (event) => {
    try {
      const payload = JSON.parse(event.data);
      if (payload.type === 'pty_output') {
        appendPtyBytes(payload.data || {});
      } else {
        log('terminal bridge ack', payload);
      }
    } catch (_err) {
      appendTerminalText(event.data);
    }
  });
  state.terminalBridge.addEventListener('close', () => {
    log('terminal bridge closed');
  });
  state.terminalBridge.addEventListener('error', () => {
    showActionToast(t('terminal_error'), 'error');
  });
  return state.terminalBridge;
}

function decodePtyChunk(chunkBase64) {
  const raw = window.atob(chunkBase64 || '');
  const bytes = new Uint8Array(raw.length);
  for (let index = 0; index < raw.length; index += 1) {
    bytes[index] = raw.charCodeAt(index);
  }
  return bytes;
}

function appendPtyBytes(payload) {
  const bytes = decodePtyChunk(payload.chunk_base64);
  const text = terminalDecoder.decode(bytes, { stream: true });
  appendTerminalText(text || payload.text || '');
}

function appendTerminalText(text) {
  renderTerminalBytes(text || '');
  const screen = document.getElementById('terminal-screen');
  if (screen) {
    screen.textContent = terminalViewport.lines.join('\n');
    screen.scrollTop = screen.scrollHeight;
  }
}

function resetTerminalViewport() {
  terminalViewport.lines = [''];
  terminalViewport.cursorRow = 0;
  terminalViewport.cursorCol = 0;
  terminalViewport.pendingEscape = '';
  const screen = document.getElementById('terminal-screen');
  if (screen) screen.textContent = '';
}

function renderTerminalBytes(text) {
  let input = `${terminalViewport.pendingEscape}${text}`;
  terminalViewport.pendingEscape = '';
  let index = 0;
  while (index < input.length) {
    const char = input[index];
    if (char === '') {
      const parsed = parseAnsiSequence(input, index);
      if (!parsed) {
        terminalViewport.pendingEscape = input.slice(index);
        break;
      }
      applyAnsiSequence(parsed.sequence);
      index = parsed.nextIndex;
      continue;
    }
    writeTerminalChar(char);
    index += 1;
  }
  trimTerminalViewport();
}

function parseAnsiSequence(input, startIndex) {
  if (startIndex + 1 >= input.length) return null;
  if (input[startIndex + 1] !== '[') {
    return { sequence: input.slice(startIndex, startIndex + 2), nextIndex: startIndex + 2 };
  }
  for (let index = startIndex + 2; index < input.length; index += 1) {
    const code = input.charCodeAt(index);
    if (code >= 0x40 && code <= 0x7e) {
      return { sequence: input.slice(startIndex, index + 1), nextIndex: index + 1 };
    }
  }
  return null;
}

function applyAnsiSequence(sequence) {
  if (!sequence.startsWith('[')) return;
  const command = sequence.slice(-1);
  const body = sequence.slice(2, -1).replace(/^\?/, '');
  const parts = body
    .split(';')
    .filter((part) => part.length > 0)
    .map((part) => Number.parseInt(part, 10) || 0);
  if (command === 'J') {
    if ((parts[0] || 0) === 2 || (parts[0] || 0) === 0) {
      terminalViewport.lines = [''];
      terminalViewport.cursorRow = 0;
      terminalViewport.cursorCol = 0;
    }
    return;
  }
  if (command === 'K') {
    ensureTerminalLine();
    const line = terminalViewport.lines[terminalViewport.cursorRow] || '';
    terminalViewport.lines[terminalViewport.cursorRow] = line.slice(0, terminalViewport.cursorCol);
    return;
  }
  if (command === 'H' || command === 'f') {
    terminalViewport.cursorRow = Math.max((parts[0] || 1) - 1, 0);
    terminalViewport.cursorCol = Math.max((parts[1] || 1) - 1, 0);
    ensureTerminalLine();
    return;
  }
  if (command === 'A') {
    terminalViewport.cursorRow = Math.max(terminalViewport.cursorRow - (parts[0] || 1), 0);
    return;
  }
  if (command === 'B') {
    terminalViewport.cursorRow += parts[0] || 1;
    ensureTerminalLine();
    return;
  }
  if (command === 'C') {
    terminalViewport.cursorCol += parts[0] || 1;
    return;
  }
  if (command === 'D') {
    terminalViewport.cursorCol = Math.max(terminalViewport.cursorCol - (parts[0] || 1), 0);
    return;
  }
  if (command === 'h' && body === '1049') {
    resetTerminalViewport();
  }
}

function writeTerminalChar(char) {
  if (char === '\r') {
    terminalViewport.cursorCol = 0;
    return;
  }
  if (char === '\n') {
    terminalViewport.cursorRow += 1;
    terminalViewport.cursorCol = 0;
    ensureTerminalLine();
    return;
  }
  if (char === '\b') {
    terminalViewport.cursorCol = Math.max(terminalViewport.cursorCol - 1, 0);
    return;
  }
  if (char === '\t') {
    const spaces = 4 - (terminalViewport.cursorCol % 4);
    for (let index = 0; index < spaces; index += 1) {
      writeTerminalChar(' ');
    }
    return;
  }
  if (char < ' ' && char !== ' ') return;
  ensureTerminalLine();
  const line = terminalViewport.lines[terminalViewport.cursorRow] || '';
  const padded = line.padEnd(terminalViewport.cursorCol, ' ');
  terminalViewport.lines[terminalViewport.cursorRow] =
    padded.slice(0, terminalViewport.cursorCol) + char + padded.slice(terminalViewport.cursorCol + 1);
  terminalViewport.cursorCol += 1;
}

function ensureTerminalLine() {
  while (terminalViewport.lines.length <= terminalViewport.cursorRow) {
    terminalViewport.lines.push('');
  }
}

function trimTerminalViewport() {
  if (terminalViewport.lines.length <= terminalViewport.maxLines) return;
  const extra = terminalViewport.lines.length - terminalViewport.maxLines;
  terminalViewport.lines.splice(0, extra);
  terminalViewport.cursorRow = Math.max(terminalViewport.cursorRow - extra, 0);
}

function sendTerminalBridgeMessage(payload) {
  if (state.terminalBridge && state.terminalBridge.readyState === WebSocket.OPEN) {
    state.terminalBridge.send(JSON.stringify(payload));
    return true;
  }
  return false;
}

async function sendTerminalInput(data) {
  if (!data) return;
  if (sendTerminalBridgeMessage({ type: 'input', data })) return;
  await post('/api/terminal/input', requestBody({ data }));
}

async function sendTerminalPreset(preset) {
  if (preset === 'ctrl_c') {
    sendTerminalSignal('interrupt').catch(handleError);
  }
  const inputs = {
    ctrl_c: '',
    escape: '',
    tab: '\t',
    enter: '\n',
    ctrl_d: '',
  };
  const data = inputs[preset];
  if (!data) return;
  if (sendTerminalBridgeMessage({ type: 'input', data })) return;
  await post('/api/terminal/input', requestBody({ data }));
}

async function resizeTerminal(cols, rows) {
  if (sendTerminalBridgeMessage({ type: 'resize', cols, rows })) return;
  await post('/api/terminal/resize', requestBody({ cols, rows }));
}

async function sendTerminalSignal(signal) {
  if (sendTerminalBridgeMessage({ type: 'terminal-signal', signal })) return;
  await post('/api/terminal/signal', requestBody({ signal }));
}

// ── Permission Approval ──
async function submitPermissionDecision(request_id, decision) {
  if (!request_id) {
    showActionToast(t('permission_required'), 'error');
    return;
  }
  const result = await post('/api/permission', requestBody({ request_id, decision }));
  log('permission', result);
  showActionToast(`${decision === 'approve' ? t('approve') : t('deny')} ${t('permission_required')}`, 'ok');
  await refresh();
}

// ── UI Helpers ──
function updateConnectionStatus(st) {
  const serverName = document.getElementById('server-origin-label') || document.getElementById('home-active-server-origin');
  if (serverName) {
    const label = st === 'connected' ? `● ${t('connected')}` : st === 'error' ? `✕ ${t('disconnected')}` : `○ ${t('connecting')}`;
    serverName.textContent = label;
    serverName.dataset.state = st;
  }
}

function showLoading() {
  const splash = document.getElementById('loading-splash');
  if (splash) splash.style.display = 'flex';
}

function hideLoading() {
  const splash = document.getElementById('loading-splash');
  if (splash) splash.style.display = 'none';
}

function showErrorBanner(message) {
  const banner = document.getElementById('error-banner');
  const msg = document.getElementById('error-banner-message');
  if (banner && msg) {
    msg.textContent = message;
    banner.style.display = 'flex';
  }
  const panel = document.getElementById('debug-panel');
  if (panel) panel.style.display = 'block';
}

function hideErrorBanner() {
  const banner = document.getElementById('error-banner');
  if (banner) banner.style.display = 'none';
  const panel = document.getElementById('debug-panel');
  if (panel) panel.style.display = 'none';
}

function promptAddServer() {
  const existing = document.getElementById('add-server-sheet');
  if (existing) return;
  const backdrop = document.createElement('div');
  backdrop.className = 'bottom-sheet-backdrop';
  backdrop.id = 'add-server-backdrop';
  const sheet = document.createElement('div');
  sheet.className = 'bottom-sheet';
  sheet.id = 'add-server-sheet';
  sheet.innerHTML = `
    <div class="bottom-sheet-handle"></div>
    <div style="font-weight:600;font-size:15px;">${t('server_url')}</div>
    <input id="add-server-input" type="url" placeholder="https://..." class="form-input">
    <div style="display:flex;gap:8px;">
      <button id="add-server-cancel" class="bottom-sheet-cancel">${t('cancel')}</button>
      <button id="add-server-confirm" style="flex:1;padding:10px;border:none;border-radius:var(--radius_sm);background:var(--accent);color:#fff;font-size:14px;font-weight:600;">${t('confirm')}</button>
    </div>`;
  document.body.appendChild(backdrop);
  document.body.appendChild(sheet);
  const input = document.getElementById('add-server-input');
  input.focus();
  const close = () => { backdrop.remove(); sheet.remove(); };
  document.getElementById('add-server-cancel').addEventListener('click', close);
  backdrop.addEventListener('click', close);
  const confirm = () => {
    const origin = input.value.trim();
    close();
    if (!origin) return;
    const normalized = normalizeServerUrl(origin);
    if (!normalized) {
      showActionToast(t('server_url_required'), 'error');
      return;
    }
    const dup = serverDirectory.find(s => s.origin === normalized);
    if (!dup) {
      serverDirectory.push({ alias: normalized, origin: normalized });
    }
    state.serverOrigin = normalized;
    saveServerDirectory();
    showActionToast(t('server_saved'), 'ok');
    refresh().catch(handleError);
  };
  document.getElementById('add-server-confirm').addEventListener('click', confirm);
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') confirm(); });
}

function showSettingsSheet() {
  const sheet = document.getElementById('mobile-settings-sheet');
  if (!sheet) return;
  setTheme(currentTheme());
  setLanguage(currentLang());
  sheet.style.display = 'flex';
  const backdrop = document.createElement('div');
  backdrop.className = 'bottom-sheet-backdrop';
  backdrop.id = 'settings-backdrop';
  backdrop.addEventListener('click', hideSettingsSheet);
  sheet.parentNode.insertBefore(backdrop, sheet);
}

function hideSettingsSheet() {
  const sheet = document.getElementById('mobile-settings-sheet');
  if (sheet) sheet.style.display = 'none';
  document.getElementById('settings-backdrop')?.remove();
}

// ── Page Router ──
function navigate(page, params = {}) {
  document.querySelectorAll('[data-page]').forEach(p => { p.hidden = true; });
  const el = document.getElementById('page-' + page);
  if (el) el.hidden = false;
  state.page = page;
  if (params.sessionId) state.sessionId = params.sessionId;
  renderCurrentPage();
}

function renderCurrentPage() {
  const renderers = { HOME: renderHomePage, CHAT: renderChatPage, KANBAN: renderKanbanPage, TERMINAL: renderTerminalPage, NEWSESSION: renderNewSessionPage };
  const fn = renderers[state.page];
  if (fn) Promise.resolve(fn()).catch(handleError);
}

// ── Page Renderers ──

async function renderHomePage() {
  const container = document.getElementById('server-list');
  if (!container) return;

  const serverName = document.getElementById('home-active-server-origin');
  if (serverName && state.cwd) {
    serverName.textContent = state.cwd.split('/').pop() || t('connected');
  }

  container.textContent = '';

  if (serverDirectory.length === 0) {
    const empty = document.createElement('p');
    empty.className = 'empty';
    empty.textContent = t('no_servers');
    container.append(empty);
    return;
  }

  for (const server of serverDirectory) {
    const card = document.createElement('div');
    card.className = 'server-card';
    const isActive = server.origin === state.serverOrigin;
    card.innerHTML = `
      <div class="server-card-header">
        <span class="server-card-dot" data-connected="${isActive ? 'true' : 'false'}"></span>
        <span class="server-card-alias">${escapeHtml(server.alias || server.origin)}</span>
        <span class="server-card-ip">${escapeHtml(server.origin)}</span>
      </div>
    `;

    // Render sessions for this server (only for the active connection)
    if (!isActive) continue;
    const serverSessions = state.sessions;

    if (serverSessions.length > 0) {
      for (const session of serverSessions) {
        const sessionCard = document.createElement('div');
        sessionCard.className = 'session-card';
        const title = session.title || session.session_id || 'Session';
        const model = session.model || '';
        const branch = session.branch || '';
        const progressPct = session.progress || 0;

        sessionCard.innerHTML = `
          <div class="session-card-title" data-session-id="${escapeHtml(session.session_id || '')}">${escapeHtml(title)}</div>
          <div class="session-card-meta">${escapeHtml(model)}${branch ? ' · ' + escapeHtml(branch) : ''}</div>
          <div class="session-card-progress">
            <div class="progress-bar"><div class="progress-fill${progressPct >= 100 ? ' complete' : ''}" style="width:${progressPct}%"></div></div>
            <button type="button" class="session-card-progress-arrow" data-session-id="${escapeHtml(session.session_id || '')}" data-navigate-kanban>&#x2192;</button>
          </div>
        `;

        // Title click -> CHAT
        sessionCard.querySelector('.session-card-title').addEventListener('click', () => {
          state.sessionId = session.session_id;
          navigate('CHAT', { sessionId: session.session_id });
        });

        // Progress arrow -> KANBAN
        sessionCard.querySelector('[data-navigate-kanban]').addEventListener('click', () => {
          state.sessionId = session.session_id;
          navigate('KANBAN', { sessionId: session.session_id });
        });

        card.append(sessionCard);
      }
    }

    if (serverSessions.length === 0) {
      const noSessions = document.createElement('p');
      noSessions.className = 'empty';
      noSessions.style.marginLeft = '16px';
      noSessions.textContent = t('no_sessions');
      card.append(noSessions);
    }

    container.append(card);
  }
  renderServerDirectory().catch(() => {});
}

async function renderChatPage() {
  const titleEl = document.getElementById('chat-title');
  const subtitleEl = document.getElementById('chat-subtitle');
  const timeline = document.getElementById('structured-timeline') || document.getElementById('chat-timeline');

  if (!timeline) return;

  // Set title
  const activeSession = state.sessions.find(s => s.session_id === state.sessionId);
  if (titleEl) {
    titleEl.textContent = activeSession?.title || state.sessionId || t('chat');
  }
  if (subtitleEl) {
    const parts = [];
    if (activeSession?.model) parts.push(activeSession.model);
    if (activeSession?.branch) parts.push(activeSession.branch);
    if (activeSession?.status) parts.push(activeSession.status);
    subtitleEl.textContent = parts.join(' · ');
  }

  // Update perm badge
  const permMode = state.projection?.runtime_descriptor?.permission_mode || state.projection?.permission_mode || '';
  if (permMode) updatePermBadge(permMode);

  // Fetch and render transcript
  if (state.sessionId) {
    try {
      const params = new URLSearchParams();
      if (state.cwd) params.set('cwd', state.cwd);
      params.set('session_id', state.sessionId);
      const transcript = await api(`/api/session/transcript?${params.toString()}`);
      const lines = transcript.data?.lines || [];
      // Preserve streaming drafts while clearing
      const drafts = [];
      state.streamingDrafts.forEach((el) => drafts.push(el));
      timeline.textContent = '';
      if (lines.length === 0) {
        const empty = document.createElement('p');
        empty.className = 'empty';
        empty.textContent = t('no_sessions');
        timeline.append(empty);
      } else {
        for (const line of lines.slice(-500)) {
          timeline.append(transcriptLineElement(line));
        }
      }
      renderStructuredConversation(latestHostSurfaceProjection, lines);
      // Re-attach streaming drafts after transcript render
      for (const draft of drafts) {
        timeline.append(draft);
      }
    } catch (err) {
      log('transcript unavailable', err);
    }
  } else {
    timeline.textContent = '';
    const empty = document.createElement('p');
    empty.className = 'empty';
    empty.textContent = t('no_sessions');
    timeline.append(empty);
  }

  // Render pending permission requests inline
  const permissions = state.projection?.workbench?.permissions || state.projection?.permissions || {};
  const pending = permissions.pending || [];
  for (const request of pending) {
    const card = document.createElement('article');
    card.className = 'approval-card';
    card.innerHTML = `
      <div class="approval-icon">${t('permission_required')}</div>
      <div class="approval-title">${escapeHtml(request.tool_name || t('tool'))}</div>
      <p>${escapeHtml(request.target_path || request.request_id || '')}</p>
      <div class="approval-actions">
        <button type="button" class="btn-approve" data-decision="approve">${t('approve')}</button>
        <button type="button" class="btn-deny" data-decision="deny">${t('deny')}</button>
      </div>
    `;
    card.querySelectorAll('[data-decision]').forEach((button) => {
      button.addEventListener('click', (event) => {
        event.stopPropagation();
        submitPermissionDecision(request.request_id, button.dataset.decision).catch(handleError);
      });
    });
    timeline.append(card);
  }

  const actionContainer = document.getElementById('surface-actions');
  if (actionContainer) {
    actionContainer.textContent = '';
    const actions = state.projection?.actions || [];
    for (const action of actions) {
      actionContainer.append(renderActionForm(action));
    }
  }

  timeline.scrollTop = timeline.scrollHeight;
}

function renderKanbanPage() {
  const progressContainer = document.getElementById('kanban-progress');
  const stagesContainer = document.getElementById('kanban-stages');
  const claimsContainer = document.getElementById('kanban-claims');
  const findingsContainer = document.getElementById('kanban-findings');
  const recoveryStatusContainer = document.getElementById('kanban-recovery-status');
  const recoveryActionsContainer = document.getElementById('kanban-recovery-actions');
  const loopHealthContainer = document.getElementById('kanban-loop-health');
  const stagePolicyContainer = document.getElementById('kanban-stage-policy');

  const research = state.projection?.research || {};
  const board = research.board || {};
  const boardEntries = Array.isArray(board.entries) ? board.entries : [];
  const taskPool = research.task_pool || {};
  const taskPoolEntries = Array.isArray(taskPool.entries) ? taskPool.entries : [];
  const hasBoardEntries = boardEntries.length > 0;
  const recovery = research.recovery_governance || {};
  const recoveryEntries = Array.isArray(recovery.entries) ? recovery.entries : [];
  const watch = research.goal_watch || {};
  const watchPlan = research.watch_plan || {};
  const stageDecisionPolicy = research.stage_decision_policy || {};
  const maintenance = research.task_pool_maintenance || {};
  const activeThread = research.active_thread_title || research.active_thread_id || '';
  const stage = research.active_stage_id || 'none';

  renderResearchInbox(research, taskPoolEntries, boardEntries, recoveryEntries);
  renderHermesBoardColumns(boardEntries, Array.isArray(board.buckets) ? board.buckets : []);

  // Progress bar
  if (progressContainer) {
    const stages = ['survey', 'idea', 'refine', 'experiment', 'implement', 'paper', 'document'];
    const stageIdx = stages.findIndex(s => stage.includes(s));
    const pct = stageIdx >= 0 ? Math.round(((stageIdx + 1) / stages.length) * 100) : (research.status === 'no_active_thread' ? 0 : 10);
    const fill = progressContainer.querySelector('.kanban-progress-fill');
    const label = progressContainer.querySelector('.kanban-progress-label');
    if (fill) {
      fill.style.width = pct + '%';
      fill.className = `kanban-progress-fill${pct >= 100 ? ' complete' : ''}`;
    }
    if (label) label.textContent = pct + '%';
  }

  if (loopHealthContainer) {
    const summary = maintenance.task_pool_summary || {};
    const loopClosure = research.loop_closure || watch.loop_closure || {};
    loopHealthContainer.innerHTML = `
      <div class="loop-health-row">
        <span>watch</span>
        <strong>${escapeHtml(watch.status || 'idle')}</strong>
        <span>${escapeHtml(watch.stop_reason || watchPlan.recommended_runner || 'not_started')}</span>
      </div>
      <div class="loop-health-row">
        <span>loop</span>
        <strong>${escapeHtml(loopClosure.status || maintenance.status || 'waiting')}</strong>
        <span>${escapeHtml(loopClosure.next_recommended_action || maintenance.next_recommended_action || 'advance_research_loop')}</span>
      </div>
      <div class="loop-health-row">
        <span>pool</span>
        <strong>${Number(summary.ready_to_run || maintenance.ready_to_run_count || 0)} ready</strong>
        <span>${Number(summary.running || maintenance.running_count || 0)} running · ${Number(summary.blocked || maintenance.blocked_count || 0)} blocked</span>
      </div>
    `;
  }

  if (stagePolicyContainer) {
    const command = Array.isArray(watchPlan.command) ? watchPlan.command.join(' ') : '';
    stagePolicyContainer.innerHTML = `
      <div class="loop-health-row">
        <span>stage</span>
        <strong>${escapeHtml(stageDecisionPolicy.status || 'clear')}</strong>
        <span>${Number(stageDecisionPolicy.operator_gate_count || 0)} gated · ${Number(stageDecisionPolicy.auto_approvable_count || 0)} auto</span>
      </div>
      <div class="loop-health-row">
        <span>plan</span>
        <strong>${escapeHtml(watchPlan.install_mode || 'dry_run')}</strong>
        <span>${escapeHtml(command || watchPlan.cron_expression || 'goals watch plan')}</span>
      </div>
    `;
  }

  if (recoveryStatusContainer) {
    const status = [
      recovery.status || research.recovery_governance_status || 'healthy',
      `failed ${recovery.failed_trigger_count || 0}`,
      `retryable ${recovery.retryable_trigger_count || 0}`,
      `backoff ${recovery.backoff_active_count || 0}`,
    ].join(' · ');
    recoveryStatusContainer.textContent = status;
  }

  if (recoveryActionsContainer) {
    recoveryActionsContainer.textContent = '';
    for (const entry of recoveryEntries.slice(0, 4)) {
      const card = document.createElement('div');
      card.className = 'kanban-finding recovery-entry';
      const canRetry = entry.object_kind === 'trigger' && Number(entry.retry_budget_remaining || 0) > 0;
      card.innerHTML = `
        <div class="recovery-entry-top">
          <span class="recovery-entry-kind">${escapeHtml(entry.object_kind || 'trigger')}</span>
          <span class="recovery-entry-id">${escapeHtml(entry.object_id || '')}</span>
        </div>
        <div class="recovery-entry-meta">${escapeHtml(entry.decision || '')}</div>
        <div class="button-row">
          <button type="button" class="secondary compact" data-action-id="inspect_recovery_governance">${t('research_brief')}</button>
          ${canRetry ? `<button type="button" class="secondary compact" data-action-id="retry_routine_trigger" data-trigger-id="${escapeHtml(entry.object_id || '')}">Retry</button>` : ''}
        </div>
      `;
      card.querySelectorAll('[data-action-id="retry_routine_trigger"]').forEach((button) => {
        button.addEventListener('click', () => {
          const field = document.getElementById('recovery-trigger-id');
          if (field) field.value = button.dataset.triggerId || '';
        });
      });
      recoveryActionsContainer.append(card);
    }
  }

  // Stages
  if (stagesContainer) {
    stagesContainer.textContent = '';
    const stages = [
      { id: 'survey', label: 'Survey', icon: '○' },
      { id: 'idea', label: 'Idea', icon: '○' },
      { id: 'refine', label: 'Refine', icon: '○' },
      { id: 'experiment', label: 'Experiment', icon: '○' },
      { id: 'implement', label: 'Implement', icon: '○' },
      { id: 'paper', label: 'Paper', icon: '○' },
    ];
    const allStageIds = stages.map(s => s.id);
    const currentIdx = allStageIds.findIndex(s => stage.includes(s));

    for (let i = 0; i < stages.length; i++) {
      const isDone = currentIdx > i;
      const isActive = currentIdx === i;
      const status = isDone ? 'done' : isActive ? 'active' : 'pending';
      const icon = isDone ? '✓' : isActive ? '⟳' : '○';

      const stageCard = document.createElement('div');
      stageCard.className = 'kanban-stage';
      stageCard.innerHTML = `
        <div class="kanban-stage-header">
          <span class="kanban-stage-icon" data-status="${status}">${icon}</span>
          <span class="kanban-stage-name">${stages[i].label}</span>
        </div>
      `;
      stagesContainer.append(stageCard);
    }
  }

  // Claims
  if (claimsContainer) {
    claimsContainer.textContent = '';
    const claims = taskPoolEntries.length > 0
      ? taskPoolEntries.filter((entry) => ['running', 'needs_review', 'needs_approval'].includes(entry.bucket_id))
      : hasBoardEntries
      ? boardEntries.filter((entry) => ['hypotheses', 'needs_review', 'promoted_memory'].includes(entry.bucket_id))
      : (Array.isArray(research.claims) ? research.claims : []);
    if (claims.length === 0) {
      claimsContainer.innerHTML = `<p class="empty">${t('no_claims')}</p>`;
    } else {
      for (const claim of claims) {
        const el = document.createElement('div');
        el.className = 'kanban-claim';
        const claimStatus = claim.status || 'projected';
        const icon = claimStatus === 'done' ? '✓' : claimStatus === 'active' || claimStatus === 'projected' ? '⟳' : '○';
        const actions = Array.isArray(claim.action_refs) ? claim.action_refs : [];
        const actionButtons = actions.slice(0, 2).map((entryAction) => (
          `<button type="button" class="secondary compact" data-action-id="${escapeHtml(entryAction.action_id || '')}">${escapeHtml(entryAction.label || entryAction.action_id || 'Action')}</button>`
        )).join('');
        el.innerHTML = `
          <span class="kanban-claim-icon" data-status="${claimStatus}">${icon}</span>
          <span class="kanban-claim-text">${escapeHtml(claim.title || claim.text || claim.summary || '')}</span>
          ${actionButtons ? `<div class="button-row">${actionButtons}</div>` : ''}
        `;
        claimsContainer.append(el);
      }
    }
  }

  // Findings
  if (findingsContainer) {
    findingsContainer.textContent = '';
    const findings = taskPoolEntries.length > 0
      ? taskPoolEntries.filter((entry) => ['ready_to_run', 'repair_or_pivot', 'blocked'].includes(entry.bucket_id))
      : hasBoardEntries
      ? boardEntries.filter((entry) => ['questions', 'evidence_needed', 'ready_to_run', 'running', 'needs_approval', 'repair_or_pivot', 'blocked'].includes(entry.bucket_id))
      : (Array.isArray(research.findings) ? research.findings : []);
    if (findings.length === 0) {
      findingsContainer.innerHTML = `<p class="empty">${t('no_findings')}</p>`;
    } else {
      for (const finding of findings) {
        const el = document.createElement('div');
        el.className = 'kanban-finding';
        const actions = Array.isArray(finding.action_refs) ? finding.action_refs : [];
        const actionButtons = actions.slice(0, 2).map((entryAction) => (
          `<button type="button" class="secondary compact" data-action-id="${escapeHtml(entryAction.action_id || '')}">${escapeHtml(entryAction.label || entryAction.action_id || 'Action')}</button>`
        )).join('');
        el.innerHTML = `
          <div>${escapeHtml([finding.title || finding.text, finding.summary].filter(Boolean).join(' · '))}</div>
          ${actionButtons ? `<div class="button-row">${actionButtons}</div>` : ''}
        `;
        findingsContainer.append(el);
      }
    }
  }
}

async function renderTerminalPage() {
  const screen = document.getElementById('terminal-screen');
  if (!screen) return;

  // Connect terminal bridge if not already connected
  if (!state.terminalBridge || state.terminalBridge.readyState > WebSocket.OPEN) {
    try {
      await attachTerminal();
    } catch (err) {
      log('terminal attach failed', err);
    }
  }

  // Auto-resize terminal
  const cols = Math.floor((screen.clientWidth - 16) / 7.8) || 80;
  const rows = Math.floor((screen.clientHeight - 16) / 19.5) || 24;
  try {
    await resizeTerminal(cols, rows);
  } catch (_) {}
}

function renderNewSessionPage() {
  const serverSelector = document.getElementById('new-server');
  if (serverSelector) {
    serverSelector.textContent = '';
    for (const server of serverDirectory) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'button-group-option';
      btn.dataset.serverOrigin = server.origin;
      btn.textContent = server.alias || server.origin;
      if (server.origin === state.serverOrigin) {
        btn.style.borderColor = 'var(--accent)';
        btn.style.color = 'var(--accent)';
      }
      btn.addEventListener('click', () => {
        state.serverOrigin = server.origin;
        serverSelector.querySelectorAll('button').forEach(b => {
          b.style.borderColor = '';
          b.style.color = '';
        });
        btn.style.borderColor = 'var(--accent)';
        btn.style.color = 'var(--accent)';
        saveServerDirectory();
      });
      serverSelector.append(btn);
    }
  }

  // Model buttons
  const modelGroup = document.getElementById('new-model-group');
  if (modelGroup) {
    modelGroup.querySelectorAll('button').forEach(btn => {
      btn.addEventListener('click', () => {
        modelGroup.querySelectorAll('button').forEach(b => b.setAttribute('aria-pressed', 'false'));
        btn.setAttribute('aria-pressed', 'true');
      });
    });
  }
}

// ── Data Refresh ──
async function refresh() {
  try {
    const query = state.cwd ? `?cwd=${encodeURIComponent(state.cwd)}` : '';
    log('refresh: fetching host-surface', state.serverOrigin || location.origin);
    const surface = await api(`/api/host-surface${query}`);
    const projection = surface.data?.projection || {};
    state.projection = projection;
    latestHostSurfaceProjection = projection;

    // Extract sessions from projection
    const sessionsData = projection.sessions || {};
    const activeSessionId = sessionsData.active_session_id;
    if (!state.sessionId && activeSessionId) {
      state.sessionId = activeSessionId;
    }

    // Fetch sessions list
    try {
      const sessionsResult = await api(`/api/sessions${query}`);
      state.sessions = sessionsResult.data?.sessions || [];
      log('refresh: sessions loaded', state.sessions.length);
    } catch (sessionsErr) {
      log('refresh: sessions fetch failed', sessionsErr);
      state.sessions = sessionsData.recent || [];
    }

    // Extract useful info from projection
    const rd = projection.runtime_descriptor || {};
    if (rd.workspace_root) state.cwd = rd.workspace_root;
    if (rd.model) state._model = rd.model;
    if (rd.branch) state._branch = rd.branch;
    const maybeMode = rd.permission_mode || projection.permission_mode;
    if (maybeMode) updatePermBadge(maybeMode);

    // Update connection status and server name
    updateConnectionStatus('connected');
  const serverName = document.getElementById('server-origin-label') || document.getElementById('home-active-server-origin');
    if (serverName && state.cwd) {
      serverName.textContent = state.cwd.split('/').pop() || t('connected');
    }
    const origin = document.getElementById('server-origin-label');
    if (origin) origin.textContent = state.serverOrigin || window.location.origin;
    const activeOrigin = document.getElementById('home-active-server-origin');
    if (activeOrigin) activeOrigin.textContent = state.serverOrigin || window.location.origin;

    state.reconnectAttempts = 0;
    hideLoading();
    hideErrorBanner();

    // Re-render current page
    latestSessionTabs = state.sessions.slice();
    renderServerDirectory().catch(() => {});
    renderCurrentPage();
    return true;
  } catch (err) {
    handleError(err);
    hideLoading();
    return false;
  }
}

// ── Composer State ──
function updateComposerState() {
  const chatInput = document.getElementById('chat-input');
  const kanbanInput = document.getElementById('kanban-input');
  const inputs = [chatInput, kanbanInput].filter(Boolean);
  for (const input of inputs) {
    input.placeholder = state.agentBusy
      ? t('waiting_for_agent')
      : state.pendingMessage
        ? t('message_queued')
        : t('message_placeholder');
  }
}

async function flushPendingMessage() {
  if (!state.pendingMessage || state.agentBusy) return;
  const { message, sessionId } = state.pendingMessage;
  state.pendingMessage = null;
  try {
    await post('/api/message', requestBody({
      session_id: sessionId,
      message,
      client_id: state.clientId,
    }));
    await refresh();
  } catch (err) {
    log('pending message send failed', err);
    handleError(err);
  }
}

// ── Composer Message Senders ──
function getActiveTimeline() {
  if (state.page === 'KANBAN') return document.getElementById('kanban-timeline');
  return document.getElementById('structured-timeline') || document.getElementById('chat-timeline');
}

function appendUserMessageToTimeline(message) {
  const timeline = getActiveTimeline();
  if (!timeline) return;
  timeline.querySelector('.empty')?.remove();
  const item = document.createElement('article');
  item.className = 'turn';
  item.dataset.type = 'user';
  item.innerHTML = `<div class="turn-body">${escapeHtml(message)}</div>`;
  timeline.append(item);
  timeline.scrollTop = timeline.scrollHeight;
}

function appendThinkingToTimeline() {
  const timeline = getActiveTimeline();
  if (!timeline) return;
  if (timeline.querySelector('.thinking-indicator')) return;
  const item = document.createElement('article');
  item.className = 'turn';
  item.dataset.type = 'assistant';
  item.innerHTML = `<div class="thinking-indicator"><span class="thinking-spinner">◔</span><span class="thinking-text">${t('thinking')}</span></div>`;
  timeline.append(item);
  timeline.scrollTop = timeline.scrollHeight;
}

async function sendChatMessage(event) {
  event.preventDefault();
  const input = document.getElementById('chat-input');
  const message = (input?.value || '').trim();
  if (!message) return;
  input.value = '';
  if (state.agentBusy) {
    state.pendingMessage = { message, sessionId: state.sessionId };
    updateComposerState();
    return;
  }
  state.agentBusy = true;
  appendUserMessageToTimeline(message);
  appendThinkingToTimeline();
  updateComposerState();
  try {
    await post('/api/message', requestBody({
      session_id: state.sessionId,
      message,
      client_id: state.clientId,
    }));
  } catch (err) {
    state.agentBusy = false;
    updateComposerState();
    handleError(err);
  }
}

async function sendKanbanMessage(event) {
  event.preventDefault();
  const input = document.getElementById('kanban-input');
  const message = (input?.value || '').trim();
  if (!message) return;
  input.value = '';
  if (state.agentBusy) {
    state.pendingMessage = { message, sessionId: state.sessionId };
    updateComposerState();
    return;
  }
  state.agentBusy = true;
  appendUserMessageToTimeline(message);
  appendThinkingToTimeline();
  updateComposerState();
  try {
    await post('/api/message', requestBody({
      session_id: state.sessionId,
      message,
      client_id: state.clientId,
    }));
  } catch (err) {
    state.agentBusy = false;
    updateComposerState();
    handleError(err);
  }
}

// ── DOMContentLoaded Init ──
export function initializeRemoteRuntime() {
  registerInstallableAppShell();
  // Theme & Language
  setTheme(currentTheme());
  setLanguage(currentLang());

  // Load persisted server directory
  loadServerDirectory();
  refreshCommandShortcuts();
  enhanceFleetServerGuide();
  enhanceTerminalExtraKeys();
  remoteRootElement().dataset.mobileRuntime = 'ready';
  renderSecuritySetupState('');

  // Wire [data-navigate] buttons (delegated)
  document.addEventListener('click', (event) => {
    const navBtn = event.target.closest('[data-navigate]');
    if (navBtn) {
      const target = navBtn.dataset.navigate;
      if (target === 'CHAT') {
        navigate('CHAT', { sessionId: state.sessionId });
      } else if (target === 'KANBAN') {
        navigate('KANBAN', { sessionId: state.sessionId });
      } else if (target === 'TERMINAL') {
        navigate('TERMINAL');
      } else if (target === 'NEWSESSION') {
        navigate('NEWSESSION');
      } else {
        navigate(target);
      }
      return;
    }

    // Terminal shortcut keys
    const termBtn = event.target.closest('[data-terminal-input]');
    if (termBtn) {
      sendTerminalPreset(termBtn.dataset.terminalInput).catch(handleError);
      return;
    }

    // Command / skill palette buttons
    const cmdBtn = event.target.closest('[data-cmd-palette]');
    if (cmdBtn) {
      const prefix = cmdBtn.dataset.cmdPalette;
      const existing = document.getElementById('command-palette');
      if (existing) { hideCommandPalette(); return; }
      const form = cmdBtn.closest('form');
      const input = form?.querySelector('textarea');
      if (input) { input.value = prefix; input.focus(); }
      showCommandPalette(prefix);
      return;
    }

    // Permission mode options
    const permOpt = event.target.closest('.perm-mode-option');
    if (permOpt) {
      changePermissionMode(permOpt.dataset.permMode).catch(handleError);
      return;
    }

    // Theme choice
    const themeBtn = event.target.closest('[data-theme-choice]');
    if (themeBtn) {
      setTheme(themeBtn.dataset.themeChoice);
      return;
    }

    // Language choice
    const langBtn = event.target.closest('[data-lang-choice]');
    if (langBtn) {
      setLanguage(langBtn.dataset.langChoice);
      return;
    }

    const mobileTarget = event.target.closest('[data-mobile-target]');
    if (mobileTarget) {
      const target = mobileTarget.dataset.mobileTarget;
      const element = document.getElementById(target);
      if (element) {
        element.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
      return;
    }

    const drawerClose = event.target.closest('[data-drawer-close]');
    if (drawerClose) {
      closeResearchCardDrawer();
      return;
    }

    const actionBtn = event.target.closest('[data-action-id]');
    if (actionBtn) {
      executeProjectedAction(actionBtn).catch(handleError);
      return;
    }

    const commandBtn = event.target.closest('[data-command]');
    if (commandBtn) {
      executeProductCommand(commandBtn.dataset.command || '');
      return;
    }

    const skillBtn = event.target.closest('[data-skill-command]');
    if (skillBtn) {
      executeSkillCommand(parseSkillCommand(skillBtn.dataset.skillCommand || ''));
      return;
    }
  });

  // Settings sheet
  document.getElementById('home-settings-btn')?.addEventListener('click', showSettingsSheet);
  document.getElementById('mobile-settings-btn')?.addEventListener('click', showSettingsSheet);
  document.getElementById('settings-sheet-close')?.addEventListener('click', hideSettingsSheet);

  // Permission mode sheet
  document.getElementById('chat-perm-badge')?.addEventListener('click', showPermModeSheet);
  document.getElementById('perm-mode-badge')?.addEventListener('click', showPermModeSheet);
  document.getElementById('perm-sheet-cancel')?.addEventListener('click', hidePermModeSheet);
  document.querySelectorAll('[data-interaction-mode]').forEach((button) => {
    button.addEventListener('click', () => setInteractionMode(button.dataset.interactionMode));
  });

  // Add server
  document.getElementById('add-server-btn')?.addEventListener('click', promptAddServer);
  document.getElementById('home-reconnect')?.addEventListener('click', () => reconnectRemote().catch(handleError));
  document.getElementById('server-reconnect')?.addEventListener('click', () => reconnectRemote().catch(handleError));
  document.getElementById('server-health')?.addEventListener('click', () => probeServerHealth(state.serverOrigin).catch(handleError));

  // Composer forms
  document.getElementById('chat-composer')?.addEventListener('submit', (event) => {
    sendChatMessage(event).catch(handleError);
  });
  document.getElementById('conversation-composer')?.addEventListener('submit', (event) => {
    sendChatMessage(event).catch(handleError);
  });

  document.getElementById('kanban-composer')?.addEventListener('submit', (event) => {
    sendKanbanMessage(event).catch(handleError);
  });

  // Terminal input
  document.getElementById('terminal-composer')?.addEventListener('submit', (event) => {
    event.preventDefault();
    const input = document.getElementById('terminal-input');
    if (!input) return;
    const cmd = input.value;
    if (!cmd) return;
    sendTerminalInput(cmd + '\n').catch(handleError);
    input.value = '';
  });

  // Slash command / skill palette on input + auto-resize
  for (const inputId of ['chat-input', 'kanban-input']) {
    const input = document.getElementById(inputId);
    if (!input) continue;
    input.addEventListener('input', () => {
      input.style.height = 'auto';
      input.style.height = Math.min(input.scrollHeight, 100) + 'px';
      const val = input.value.trimStart();
      if (val.startsWith('/') && !val.includes(' ')) {
        showCommandPalette(val);
      } else if (val.startsWith('$') && !val.includes(' ')) {
        showCommandPalette(val);
      } else {
        hideCommandPalette();
      }
    });
    input.addEventListener('blur', () => setTimeout(hideCommandPalette, 200));
  }

  // New session form
  document.getElementById('new-session-start-btn')?.addEventListener('click', () => {
    createSession().catch(handleError);
  });
  document.getElementById('attachment-clear')?.addEventListener('click', clearConversationAttachments);
  document.getElementById('artifact-open-path')?.addEventListener('click', () => openArtifactPath().catch(handleError));

  // Error banner
  document.getElementById('error-banner-retry')?.addEventListener('click', () => {
    hideErrorBanner();
    refresh().catch(handleError);
  });
  document.getElementById('error-banner-dismiss')?.addEventListener('click', hideErrorBanner);

  // Terminal screen: auto-resize on page show
  const terminalScreen = document.getElementById('terminal-screen');
  if (terminalScreen) {
    terminalScreen.addEventListener('focus', () => {
      const cols = Math.floor((terminalScreen.clientWidth - 16) / 7.8) || 80;
      const rows = Math.floor((terminalScreen.clientHeight - 16) / 19.5) || 24;
      resizeTerminal(cols, rows).catch(() => {});
    });
  }

  document.getElementById('remember-control-token')?.addEventListener('change', rememberControlToken);
  document.getElementById('server-origin')?.addEventListener('change', () => {
    const input = document.getElementById('server-origin');
    if (!input) return;
    const origin = normalizeServerUrl(input.value);
    if (origin) {
      state.serverOrigin = origin;
      saveServerDirectory();
      refresh().catch(handleError);
    }
  });

  // Auto-reconnect on visibility change
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible' && state.controlToken && !state.eventsActive) {
      state.reconnectAttempts = 0;
      quickPairAndConnect();
    }
  });

  // Start connection
  navigate(initialPageFromLocation());
  showLoading();
  startRemoteRuntime();

}
