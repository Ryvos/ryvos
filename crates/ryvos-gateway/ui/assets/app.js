(function () {
  'use strict';

  // --- State ---
  var apiKey = localStorage.getItem('ryvos_api_key') || '';
  var ws = null;
  var currentRoute = 'dashboard';
  var currentSessionId = '';
  var sessions = [];
  var streaming = false;
  var streamingEl = null;
  var streamingText = '';
  var activityFeed = [];

  // --- DOM refs ---
  var loginOverlay = document.getElementById('login-overlay');
  var apiKeyInput = document.getElementById('api-key-input');
  var loginBtn = document.getElementById('login-btn');
  var logoutBtn = document.getElementById('logout-btn');
  var content = document.getElementById('content');

  // --- Init ---
  function init() {
    if (apiKey) {
      connect();
    } else {
      showLogin();
    }

    loginBtn.addEventListener('click', handleLogin);
    apiKeyInput.addEventListener('keydown', function (e) {
      if (e.key === 'Enter') handleLogin();
    });
    logoutBtn.addEventListener('click', handleLogout);

    window.addEventListener('hashchange', handleRoute);
    handleRoute();
  }

  // --- Auth ---
  function showLogin() {
    loginOverlay.classList.remove('hidden');
    apiKeyInput.value = '';
    apiKeyInput.focus();
  }

  function handleLogin() {
    apiKey = apiKeyInput.value.trim();
    localStorage.setItem('ryvos_api_key', apiKey);
    loginOverlay.classList.add('hidden');
    connect();
    handleRoute();
  }

  function handleLogout() {
    apiKey = '';
    localStorage.removeItem('ryvos_api_key');
    if (ws) ws.close();
    ws = null;
    sessions = [];
    currentSessionId = '';
    content.innerHTML = '';
    showLogin();
  }

  function authHeaders() {
    var h = { 'Content-Type': 'application/json' };
    if (apiKey) h['Authorization'] = 'Bearer ' + apiKey;
    return h;
  }

  function apiFetch(path, opts) {
    opts = opts || {};
    opts.headers = authHeaders();
    return fetch(path, opts).then(function (resp) {
      if (resp.status === 401) {
        handleLogout();
        throw new Error('Unauthorized');
      }
      return resp.json();
    });
  }

  // --- WebSocket ---
  function connect() {
    var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    var wsUrl = proto + '//' + location.host + '/ws';
    if (apiKey) wsUrl += '?token=' + encodeURIComponent(apiKey);

    try {
      ws = new WebSocket(wsUrl);
    } catch (_) {
      return;
    }

    ws.onopen = function () {
      updateConnectionStatus(true);
      fetchSessions();
    };

    ws.onmessage = function (e) {
      var data;
      try { data = JSON.parse(e.data); } catch (_) { return; }
      if (data.type === 'event') handleEvent(data);
    };

    ws.onerror = function () {
      updateConnectionStatus(false);
    };

    ws.onclose = function () {
      updateConnectionStatus(false);
      setTimeout(function () { if (apiKey) connect(); }, 3000);
    };
  }

  function updateConnectionStatus(connected) {
    var el = document.getElementById('conn-status');
    if (!el) return;
    var dot = el.querySelector('.status-dot');
    var text = el.querySelector('span:last-child');
    if (connected) {
      if (dot) dot.style.background = 'var(--success)';
      if (dot) dot.style.boxShadow = '0 0 6px var(--success)';
      if (text) text.textContent = 'Connected';
    } else {
      if (dot) dot.style.background = 'var(--text-muted)';
      if (dot) dot.style.boxShadow = 'none';
      if (text) text.textContent = 'Disconnected';
    }
  }

  function wsSend(method, params) {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({
      type: 'request',
      id: String(Date.now()),
      method: method,
      params: params || {},
    }));
  }

  // --- Events ---
  function handleEvent(data) {
    var evt = data.event;
    if (!evt) return;
    addActivityItem(evt, data.session_id);
    if (currentRoute === 'chat' && data.session_id === currentSessionId) {
      handleChatEvent(evt);
    }
  }

  function addActivityItem(evt, sessionId) {
    var item = {
      time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
      kind: evt.kind,
      session: sessionId ? sessionId.substring(0, 10) : '',
      detail: '',
    };
    switch (evt.kind) {
      case 'text_delta': return;
      case 'run_started': item.detail = 'Run started'; break;
      case 'run_complete':
        item.detail = 'Completed in ' + (evt.data && evt.data.total_turns || '?') + ' turns';
        break;
      case 'run_error': item.detail = 'Error: ' + (evt.data && evt.data.error || '?'); break;
      case 'tool_start': item.detail = 'Tool: ' + (evt.tool || '?'); break;
      case 'tool_end': item.detail = 'Tool done: ' + (evt.tool || '?'); break;
      case 'usage_update':
        item.detail = '+' + (evt.data && evt.data.input_tokens || 0) + ' in / +' + (evt.data && evt.data.output_tokens || 0) + ' out';
        break;
      case 'budget_warning':
        item.detail = 'Budget at ' + (evt.data && evt.data.utilization_pct || '?') + '%';
        break;
      case 'budget_exceeded':
        item.detail = 'Budget exceeded!';
        break;
      case 'heartbeat_fired':
        item.detail = 'Heartbeat fired';
        item.session = 'system';
        break;
      case 'heartbeat_ok':
        item.detail = 'Heartbeat OK (' + (evt.data && evt.data.response_chars || 0) + ' chars)';
        break;
      case 'heartbeat_alert':
        item.detail = 'Heartbeat ALERT: ' + truncate(evt.data && evt.data.message || '', 60);
        break;
      case 'cron_fired':
        item.detail = 'Cron: ' + (evt.data && evt.data.job_name || '?');
        item.session = 'system';
        break;
      case 'cron_complete':
        item.detail = 'Cron done: ' + (evt.data && evt.data.job_name || '?');
        break;
      case 'guardian_stall':
        item.detail = 'Guardian: stall detected';
        break;
      case 'guardian_doom_loop':
        item.detail = 'Guardian: doom loop detected';
        break;
      case 'guardian_budget_alert':
        item.detail = 'Guardian: budget alert';
        break;
      case 'approval_requested':
        item.detail = 'Approval needed: ' + (evt.data && evt.data.tool_name || '?');
        break;
      case 'tool_blocked':
        item.detail = 'Blocked: ' + (evt.tool || '?');
        break;
      default: item.detail = evt.kind; break;
    }
    activityFeed.unshift(item);
    if (activityFeed.length > 50) activityFeed.pop();

    var feedEl = document.getElementById('activity-feed');
    if (feedEl) renderActivityFeed(feedEl);
  }

  function handleChatEvent(evt) {
    var messagesEl = document.getElementById('messages');
    if (!messagesEl) return;

    switch (evt.kind) {
      case 'text_delta':
        if (!streaming) {
          streaming = true;
          streamingText = '';
          streamingEl = addMessage(messagesEl, 'assistant', '');
        }
        streamingText += evt.text || '';
        updateMessageBody(streamingEl, streamingText, true);
        messagesEl.scrollTop = messagesEl.scrollHeight;
        break;
      case 'run_complete':
        if (streaming) {
          updateMessageBody(streamingEl, streamingText, false);
          streaming = false; streamingEl = null; streamingText = '';
        }
        var sendBtn = document.getElementById('send-btn');
        var msgInput = document.getElementById('message-input');
        if (sendBtn) sendBtn.disabled = false;
        if (msgInput) { msgInput.disabled = false; msgInput.focus(); }
        break;
      case 'run_error':
        if (streaming) {
          updateMessageBody(streamingEl, streamingText, false);
          streaming = false; streamingEl = null; streamingText = '';
        }
        addMessage(messagesEl, 'assistant', 'Error: ' + (evt.data && evt.data.error || 'Unknown'));
        break;
    }
  }

  // --- Router ---
  function handleRoute() {
    var hash = window.location.hash || '#/dashboard';
    var parts = hash.replace('#/', '').split('/');
    var route = parts[0] || 'dashboard';
    var param = parts[1] || '';

    currentRoute = route;

    document.querySelectorAll('.nav-item').forEach(function (el) {
      el.classList.toggle('active', el.dataset.route === route ||
        (route === 'chat' && el.dataset.route === 'sessions'));
    });

    switch (route) {
      case 'dashboard': renderDashboard(); break;
      case 'sessions': renderSessions(); break;
      case 'chat': currentSessionId = param; renderChat(param); break;
      case 'runs': renderRuns(); break;
      case 'costs': renderCosts(); break;
      case 'settings': renderSettings(); break;
      default: renderDashboard(); break;
    }
  }

  // --- Dashboard ---
  function renderDashboard() {
    content.innerHTML =
      '<div class="dashboard">' +
      '<div class="page-header"><h2>Dashboard</h2><p>Overview of your Ryvos instance</p></div>' +
      '<div class="metric-cards" id="metric-cards">' + metricCardSkeleton(5) + '</div>' +
      '<div class="dashboard-grid">' +
      '<div class="card"><div class="card-header"><h3>Activity Feed</h3><span class="card-badge">Live</span></div>' +
      '<div id="activity-feed" class="activity-feed"></div></div>' +
      '<div class="card"><div class="card-header"><h3>Runs</h3><span class="card-badge">7 days</span></div>' +
      '<div id="run-chart" class="run-chart"></div></div>' +
      '</div></div>';

    apiFetch('/api/metrics').then(function (data) {
      var cards = document.getElementById('metric-cards');
      if (!cards) return;
      cards.innerHTML =
        metricCard('Runs', data.total_runs, 'runs', SVG_ACTIVITY) +
        metricCard('Sessions', data.active_sessions, 'sessions', SVG_USERS) +
        metricCard('Spend', '$' + (data.total_cost_cents / 100).toFixed(2), 'spend', SVG_DOLLAR) +
        metricCard('Budget', data.monthly_budget_cents > 0 ? data.budget_utilization_pct + '%' : 'Unlimited', 'budget', SVG_SHIELD) +
        metricCard('Uptime', formatDuration(data.uptime_secs), 'uptime', SVG_CLOCK);
    }).catch(function () {
      var cards = document.getElementById('metric-cards');
      if (cards) cards.innerHTML = '<p class="text-muted">Failed to load metrics</p>';
    });

    var feedEl = document.getElementById('activity-feed');
    if (feedEl) renderActivityFeed(feedEl);

    apiFetch('/api/runs?limit=100').then(function (data) {
      var chartEl = document.getElementById('run-chart');
      if (!chartEl) return;
      renderRunChart(chartEl, data.runs || []);
    }).catch(function () {
      var chartEl = document.getElementById('run-chart');
      if (chartEl) chartEl.innerHTML = '<p class="feed-empty">No run data yet</p>';
    });
  }

  // SVG icon constants
  var SVG_ACTIVITY = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>';
  var SVG_USERS = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/></svg>';
  var SVG_DOLLAR = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="1" x2="12" y2="23"/><path d="M17 5H9.5a3.5 3.5 0 000 7h5a3.5 3.5 0 010 7H6"/></svg>';
  var SVG_SHIELD = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>';
  var SVG_CLOCK = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>';

  function metricCard(label, value, type, icon) {
    return '<div class="metric-card">' +
      '<div class="metric-icon ' + type + '">' + icon + '</div>' +
      '<div class="metric-value">' + value + '</div>' +
      '<div class="metric-label">' + label + '</div></div>';
  }

  function metricCardSkeleton(count) {
    var html = '';
    for (var i = 0; i < count; i++) {
      html += '<div class="metric-card" style="min-height:100px"><div class="metric-label" style="margin-top:2rem">Loading...</div></div>';
    }
    return html;
  }

  function renderActivityFeed(el) {
    if (activityFeed.length === 0) {
      el.innerHTML = '<p class="feed-empty">Waiting for events...</p>';
      return;
    }
    var html = '';
    activityFeed.forEach(function (item) {
      var cls = item.kind === 'run_error' || item.kind === 'budget_exceeded'
        ? ' feed-error' : (item.kind === 'budget_warning' ? ' feed-warn' : '');
      html += '<div class="feed-item' + cls + '">' +
        '<span class="feed-dot"></span>' +
        '<span class="feed-time">' + item.time + '</span>' +
        '<span class="feed-detail">' + item.detail + '</span>' +
        '<span class="feed-session">' + item.session + '</span></div>';
    });
    el.innerHTML = html;
  }

  function renderRunChart(el, runs) {
    var days = {};
    var now = new Date();
    for (var i = 6; i >= 0; i--) {
      var d = new Date(now); d.setDate(d.getDate() - i);
      days[d.toISOString().split('T')[0]] = 0;
    }
    runs.forEach(function (r) {
      if (r.start_time) {
        var day = r.start_time.split('T')[0];
        if (days[day] !== undefined) days[day]++;
      }
    });

    var labels = Object.keys(days);
    var values = Object.values(days);
    var max = Math.max.apply(null, values) || 1;
    var total = values.reduce(function (a, b) { return a + b; }, 0);

    if (total === 0) {
      el.innerHTML = '<p class="feed-empty">No runs in the last 7 days</p>';
      return;
    }

    var w = 380, h = 160, pad = 30;
    var barW = Math.floor((w - pad * 2) / labels.length) - 6;
    var svg = '<svg width="100%" height="' + h + '" viewBox="0 0 ' + w + ' ' + h + '">';

    // Grid lines
    for (var g = 0; g <= 3; g++) {
      var gy = pad + (h - pad * 2) * (1 - g / 3);
      svg += '<line x1="' + pad + '" y1="' + gy + '" x2="' + (w - pad) + '" y2="' + gy + '" stroke="rgba(255,255,255,0.04)" stroke-width="1"/>';
    }

    labels.forEach(function (label, i) {
      var barH = (values[i] / max) * (h - pad * 2 - 10);
      var x = pad + i * ((w - pad * 2) / labels.length) + 3;
      var y = h - pad - barH;

      // Bar with gradient
      svg += '<defs><linearGradient id="bg' + i + '" x1="0" y1="0" x2="0" y2="1">' +
        '<stop offset="0%" stop-color="#818cf8"/><stop offset="100%" stop-color="#6366f1" stop-opacity="0.6"/>' +
        '</linearGradient></defs>';
      svg += '<rect x="' + x + '" y="' + y + '" width="' + barW + '" height="' + barH +
        '" fill="url(#bg' + i + ')" rx="4"/>';

      // Day label
      var dayName = new Date(label + 'T12:00:00').toLocaleDateString([], { weekday: 'short' });
      svg += '<text x="' + (x + barW / 2) + '" y="' + (h - 8) + '" text-anchor="middle" ' +
        'fill="var(--text-muted)" font-size="10" font-family="Inter, sans-serif">' + dayName + '</text>';

      // Value on top
      if (values[i] > 0) {
        svg += '<text x="' + (x + barW / 2) + '" y="' + (y - 6) + '" text-anchor="middle" ' +
          'fill="var(--text-secondary)" font-size="11" font-weight="600" font-family="Inter, sans-serif">' + values[i] + '</text>';
      }
    });
    svg += '</svg>';
    el.innerHTML = svg;
  }

  // --- Sessions ---
  function renderSessions() {
    content.innerHTML = '<div class="page"><div class="page-header"><h2>Sessions</h2><p>Active conversation sessions</p></div>' +
      '<div id="session-list"></div></div>';
    fetchSessions();
  }

  function fetchSessions() {
    apiFetch('/api/sessions').then(function (data) {
      sessions = data.sessions || [];
      var listEl = document.getElementById('session-list');
      if (!listEl) return;

      if (sessions.length === 0) {
        listEl.innerHTML = '<p class="feed-empty" style="padding:3rem 0">No active sessions</p>';
        return;
      }

      var html = '<div class="table-wrap"><table><thead><tr><th>Session ID</th><th>Actions</th></tr></thead><tbody>';
      sessions.forEach(function (s) {
        html += '<tr><td style="font-family:var(--font-mono);font-size:0.8rem">' + escapeHtml(truncate(s, 50)) + '</td>' +
          '<td><a href="#/chat/' + encodeURIComponent(s) + '" class="btn-sm">Open Chat</a></td></tr>';
      });
      html += '</tbody></table></div>';
      listEl.innerHTML = html;
    }).catch(function () {
      var el = document.getElementById('session-list');
      if (el) el.innerHTML = '<p class="text-muted">Failed to load sessions</p>';
    });
  }

  // --- Chat ---
  function renderChat(sessionId) {
    if (!sessionId) { window.location.hash = '#/sessions'; return; }

    content.innerHTML =
      '<div class="chat-view">' +
      '<div class="chat-header"><a href="#/sessions" class="back-link">&larr; Sessions</a>' +
      '<span class="chat-title">' + escapeHtml(truncate(sessionId, 40)) + '</span>' +
      '<button id="new-session-btn" class="btn-sm">+ New</button></div>' +
      '<div id="messages" class="messages"></div>' +
      '<div class="input-bar">' +
      '<textarea id="message-input" placeholder="Type a message..." rows="1"></textarea>' +
      '<button id="send-btn">Send</button></div></div>';

    var msgInput = document.getElementById('message-input');
    var sendBtn = document.getElementById('send-btn');
    var newBtn = document.getElementById('new-session-btn');

    sendBtn.addEventListener('click', function () { sendMessage(sessionId); });
    msgInput.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(sessionId); }
    });
    msgInput.addEventListener('input', autoResize);
    newBtn.addEventListener('click', function () {
      window.location.hash = '#/chat/web-' + Date.now().toString(36);
    });

    var messagesEl = document.getElementById('messages');
    apiFetch('/api/sessions/' + encodeURIComponent(sessionId) + '/history?limit=100').then(function (data) {
      var msgs = data.messages || [];
      msgs.forEach(function (m) {
        addMessage(messagesEl, m.role || 'assistant', m.text || '');
      });
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }).catch(function () {});
  }

  function sendMessage(sessionId) {
    var msgInput = document.getElementById('message-input');
    var sendBtn = document.getElementById('send-btn');
    var messagesEl = document.getElementById('messages');
    var text = msgInput.value.trim();
    if (!text || streaming) return;

    addMessage(messagesEl, 'user', text);
    msgInput.value = '';
    autoResize();
    messagesEl.scrollTop = messagesEl.scrollHeight;
    sendBtn.disabled = true;
    msgInput.disabled = true;
    wsSend('agent.send', { session_id: sessionId, message: text });
  }

  // --- Runs ---
  function renderRuns() {
    content.innerHTML = '<div class="page"><div class="page-header"><h2>Run History</h2><p>All recorded agent runs</p></div>' +
      '<div id="runs-table"></div><div id="runs-pagination" class="pagination"></div></div>';
    loadRuns(0);
  }

  function loadRuns(offset) {
    apiFetch('/api/runs?limit=20&offset=' + offset).then(function (data) {
      var el = document.getElementById('runs-table');
      if (!el) return;
      var runs = data.runs || [];
      if (runs.length === 0) {
        el.innerHTML = '<p class="feed-empty" style="padding:3rem 0">No runs recorded yet</p>';
        return;
      }
      var html = '<div class="table-wrap"><table><thead><tr>' +
        '<th>Time</th><th>Session</th><th>Model</th><th>Turns</th>' +
        '<th>Tokens</th><th>Cost</th><th>Type</th><th>Status</th></tr></thead><tbody>';
      runs.forEach(function (r) {
        var tokens = (r.input_tokens || 0) + (r.output_tokens || 0);
        var cost = '$' + ((r.cost_cents || 0) / 100).toFixed(3);
        var time = r.start_time ? new Date(r.start_time).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '-';
        html += '<tr>' +
          '<td style="font-family:var(--font-mono);font-size:0.8rem">' + time + '</td>' +
          '<td style="font-family:var(--font-mono);font-size:0.75rem">' + truncate(r.session_id || '', 12) + '</td>' +
          '<td>' + escapeHtml(r.model || '-') + '</td>' +
          '<td>' + (r.total_turns || 0) + '</td>' +
          '<td>' + tokens.toLocaleString() + '</td>' +
          '<td style="font-family:var(--font-mono)">' + cost + '</td>' +
          '<td><span class="badge badge-' + (r.billing_type || 'api') + '">' +
          (r.billing_type || 'api') + '</span></td>' +
          '<td><span class="status-' + (r.status || 'unknown') + '">' +
          (r.status || '-') + '</span></td></tr>';
      });
      html += '</tbody></table></div>';
      el.innerHTML = html;

      var pagEl = document.getElementById('runs-pagination');
      if (pagEl && data.total > 20) {
        var pages = Math.ceil(data.total / 20);
        var currentPage = Math.floor(offset / 20);
        var pagHtml = '';
        for (var i = 0; i < pages && i < 10; i++) {
          pagHtml += '<button class="page-btn' + (i === currentPage ? ' active' : '') +
            '" data-offset="' + (i * 20) + '">' + (i + 1) + '</button>';
        }
        pagEl.innerHTML = pagHtml;
        pagEl.querySelectorAll('.page-btn').forEach(function (btn) {
          btn.addEventListener('click', function () { loadRuns(parseInt(btn.dataset.offset)); });
        });
      }
    }).catch(function () {
      var el = document.getElementById('runs-table');
      if (el) el.innerHTML = '<p class="feed-empty" style="padding:3rem 0">Cost tracking not configured</p>';
    });
  }

  // --- Costs ---
  function renderCosts() {
    var now = new Date();
    var thirtyDaysAgo = new Date(now.getTime() - 30 * 86400000);

    content.innerHTML =
      '<div class="page"><div class="page-header"><h2>Cost Analysis</h2><p>Token usage and spending breakdown</p></div>' +
      '<div class="cost-controls">' +
      '<label>From <input type="date" id="cost-from" value="' + thirtyDaysAgo.toISOString().split('T')[0] + '"></label>' +
      '<label>To <input type="date" id="cost-to" value="' + now.toISOString().split('T')[0] + '"></label>' +
      '<label>Group by <select id="cost-group">' +
      '<option value="model">Model</option><option value="provider">Provider</option>' +
      '<option value="day">Day</option></select></label>' +
      '<button id="cost-refresh" class="btn-primary">Refresh</button></div>' +
      '<div id="cost-summary" class="cost-summary"></div>' +
      '<div id="cost-table"></div></div>';

    document.getElementById('cost-refresh').addEventListener('click', loadCosts);
    loadCosts();
  }

  function loadCosts() {
    var from = document.getElementById('cost-from').value + 'T00:00:00Z';
    var to = document.getElementById('cost-to').value + 'T23:59:59Z';
    var groupBy = document.getElementById('cost-group').value;

    apiFetch('/api/costs?from=' + encodeURIComponent(from) + '&to=' + encodeURIComponent(to) +
      '&group_by=' + groupBy).then(function (data) {
      var summEl = document.getElementById('cost-summary');
      if (summEl && data.summary) {
        var s = data.summary;
        summEl.innerHTML =
          '<div class="metric-cards">' +
          metricCard('Total Cost', '$' + ((s.total_cost_cents || 0) / 100).toFixed(2), 'spend', SVG_DOLLAR) +
          metricCard('Input Tokens', (s.total_input_tokens || 0).toLocaleString(), 'runs', SVG_ACTIVITY) +
          metricCard('Output Tokens', (s.total_output_tokens || 0).toLocaleString(), 'sessions', SVG_ACTIVITY) +
          metricCard('Events', (s.total_events || 0).toLocaleString(), 'uptime', SVG_CLOCK) +
          '</div>';
      }

      var tableEl = document.getElementById('cost-table');
      if (!tableEl) return;
      var breakdown = data.breakdown || [];
      if (breakdown.length === 0) {
        tableEl.innerHTML = '<p class="feed-empty" style="padding:2rem 0">No cost data for this period</p>';
        return;
      }
      var html = '<div class="table-wrap"><table><thead><tr><th>' +
        capitalizeFirst(groupBy) + '</th><th>Cost</th><th>Input Tokens</th><th>Output Tokens</th></tr></thead><tbody>';
      breakdown.forEach(function (row) {
        html += '<tr><td style="font-weight:500">' + escapeHtml(row.key) + '</td>' +
          '<td style="font-family:var(--font-mono)">$' + ((row.cost_cents || 0) / 100).toFixed(3) + '</td>' +
          '<td>' + (row.input_tokens || 0).toLocaleString() + '</td>' +
          '<td>' + (row.output_tokens || 0).toLocaleString() + '</td></tr>';
      });
      html += '</tbody></table></div>';
      tableEl.innerHTML = html;
    }).catch(function () {
      var el = document.getElementById('cost-table');
      if (el) el.innerHTML = '<p class="feed-empty" style="padding:2rem 0">Cost tracking not configured</p>';
    });
  }

  // --- Settings ---
  function renderSettings() {
    content.innerHTML = '<div class="page"><div class="page-header"><h2>Settings</h2><p>System info and budget configuration</p></div>' +
      '<div class="settings-grid" id="settings-content"></div></div>';

    var settingsEl = document.getElementById('settings-content');

    apiFetch('/api/health').then(function (data) {
      var html =
        '<div class="card"><div class="card-header"><h3>System</h3></div>' +
        '<p style="margin-bottom:0.5rem"><strong style="color:var(--text-muted);font-size:0.8rem">VERSION</strong><br>' +
        '<span style="font-size:1.1rem;font-weight:600">' + (data.version || 'unknown') + '</span></p>' +
        '<p><strong style="color:var(--text-muted);font-size:0.8rem">STATUS</strong><br>' +
        '<span style="color:var(--success);font-weight:600">' + (data.status || 'unknown') + '</span></p></div>';

      settingsEl.innerHTML = html + '<div class="card"><div class="card-header"><h3>Budget</h3></div><div id="budget-info"></div></div>';

      apiFetch('/api/metrics').then(function (m) {
        var budgetEl = document.getElementById('budget-info');
        if (!budgetEl) return;
        if (m.monthly_budget_cents > 0) {
          var pct = Math.min(m.budget_utilization_pct, 100);
          var color = pct > 90 ? 'var(--error)' : pct > 70 ? 'var(--warning)' : 'var(--accent)';
          budgetEl.innerHTML =
            '<p style="margin-bottom:0.5rem"><strong style="color:var(--text-muted);font-size:0.8rem">MONTHLY BUDGET</strong><br>' +
            '<span style="font-size:1.1rem;font-weight:600">$' + (m.monthly_budget_cents / 100).toFixed(2) + '</span></p>' +
            '<p style="margin-bottom:0.5rem"><strong style="color:var(--text-muted);font-size:0.8rem">SPENT</strong><br>' +
            '<span style="font-size:1.1rem;font-weight:600">$' + (m.total_cost_cents / 100).toFixed(2) + '</span></p>' +
            '<p style="margin-bottom:0.5rem"><strong style="color:var(--text-muted);font-size:0.8rem">UTILIZATION</strong><br>' +
            '<span style="font-size:1.1rem;font-weight:600;color:' + color + '">' + m.budget_utilization_pct + '%</span></p>' +
            '<div class="budget-bar"><div class="budget-fill" style="width:' + pct + '%;background:' + color + '"></div></div>';
        } else {
          budgetEl.innerHTML = '<p class="text-muted" style="padding:1rem 0">No budget configured. Add <code style="font-family:var(--font-mono);background:var(--bg-base);padding:0.15rem 0.4rem;border-radius:4px">[budget]</code> to your config.toml.</p>';
        }
      }).catch(function () {});
    }).catch(function () {
      settingsEl.innerHTML = '<p class="text-muted">Failed to load settings</p>';
    });
  }

  // --- Message rendering ---
  function addMessage(container, role, text) {
    var wrapper = document.createElement('div');
    wrapper.className = 'message ' + role;
    var roleLabel = document.createElement('div');
    roleLabel.className = 'message-role';
    roleLabel.textContent = role;
    var body = document.createElement('div');
    body.className = 'message-body';
    body.innerHTML = renderMarkdown(text);
    wrapper.appendChild(roleLabel);
    wrapper.appendChild(body);
    container.appendChild(wrapper);
    return wrapper;
  }

  function updateMessageBody(el, text, isStreaming) {
    if (!el) return;
    var body = el.querySelector('.message-body');
    if (!body) return;
    body.innerHTML = renderMarkdown(text) + (isStreaming ? '<span class="streaming-cursor"></span>' : '');
  }

  function renderMarkdown(text) {
    if (!text) return '';
    text = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    text = text.replace(/```(\w*)\n([\s\S]*?)```/g, function (_, lang, code) {
      return '<pre><code>' + code.trim() + '</code></pre>';
    });
    text = text.replace(/`([^`]+)`/g, '<code>$1</code>');
    text = text.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    text = text.replace(/\*(.+?)\*/g, '<em>$1</em>');
    text = text.replace(/\n/g, '<br>');
    return text;
  }

  // --- Utilities ---
  function autoResize() {
    var el = document.getElementById('message-input');
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 120) + 'px';
  }

  function truncate(s, max) {
    return s.length > max ? s.substring(0, max) + '...' : s;
  }

  function escapeHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function capitalizeFirst(s) {
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  function formatDuration(secs) {
    if (secs < 60) return secs + 's';
    if (secs < 3600) return Math.floor(secs / 60) + 'm';
    if (secs < 86400) return Math.floor(secs / 3600) + 'h ' + Math.floor((secs % 3600) / 60) + 'm';
    return Math.floor(secs / 86400) + 'd ' + Math.floor((secs % 86400) / 3600) + 'h';
  }

  // --- Boot ---
  document.addEventListener('DOMContentLoaded', init);
})();
