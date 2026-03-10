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

    // Route handling
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

    ws = new WebSocket(wsUrl);

    ws.onopen = function () {
      fetchSessions();
    };

    ws.onmessage = function (e) {
      var data;
      try { data = JSON.parse(e.data); } catch (_) { return; }

      if (data.type === 'event') {
        handleEvent(data);
      }
    };

    ws.onclose = function () {
      setTimeout(function () { if (apiKey) connect(); }, 3000);
    };
  }

  function wsSend(method, params) {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    var frame = {
      type: 'request',
      id: String(Date.now()),
      method: method,
      params: params || {},
    };
    ws.send(JSON.stringify(frame));
  }

  // --- Events ---
  function handleEvent(data) {
    var evt = data.event;
    if (!evt) return;

    // Feed activity to dashboard
    addActivityItem(evt, data.session_id);

    // Handle chat events for session view
    if (currentRoute === 'chat' && data.session_id === currentSessionId) {
      handleChatEvent(evt);
    }
  }

  function addActivityItem(evt, sessionId) {
    var item = {
      time: new Date().toLocaleTimeString(),
      kind: evt.kind,
      session: sessionId ? sessionId.substring(0, 12) : '',
      detail: '',
    };
    switch (evt.kind) {
      case 'text_delta': return; // Too noisy
      case 'run_started': item.detail = 'Run started'; break;
      case 'run_complete':
        item.detail = 'Run complete (' + (evt.data && evt.data.total_turns || '?') + ' turns)';
        break;
      case 'run_error': item.detail = 'Error: ' + (evt.data && evt.data.error || '?'); break;
      case 'tool_start': item.detail = 'Tool: ' + (evt.tool || '?'); break;
      case 'tool_end': item.detail = 'Tool done: ' + (evt.tool || '?'); break;
      case 'usage_update':
        item.detail = 'Tokens: +' + (evt.data && evt.data.input_tokens || 0) + '/' + (evt.data && evt.data.output_tokens || 0);
        break;
      case 'budget_warning':
        item.detail = 'Budget warning: ' + (evt.data && evt.data.utilization_pct || '?') + '%';
        break;
      case 'budget_exceeded':
        item.detail = 'Budget exceeded!';
        break;
      default: item.detail = evt.kind; break;
    }
    activityFeed.unshift(item);
    if (activityFeed.length > 50) activityFeed.pop();

    // Update dashboard feed if visible
    var feedEl = document.getElementById('activity-feed');
    if (feedEl) {
      renderActivityFeed(feedEl);
    }
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
          streaming = false;
          streamingEl = null;
          streamingText = '';
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

    // Update nav
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
      '<h2>Dashboard</h2>' +
      '<div class="metric-cards" id="metric-cards">Loading...</div>' +
      '<div class="dashboard-grid">' +
      '<div class="card"><h3>Activity Feed</h3><div id="activity-feed" class="activity-feed"></div></div>' +
      '<div class="card"><h3>Recent Runs (7d)</h3><div id="run-chart" class="run-chart">Loading...</div></div>' +
      '</div></div>';

    apiFetch('/api/metrics').then(function (data) {
      var cards = document.getElementById('metric-cards');
      if (!cards) return;
      cards.innerHTML =
        metricCard('Runs', data.total_runs) +
        metricCard('Sessions', data.active_sessions) +
        metricCard('Spend', '$' + (data.total_cost_cents / 100).toFixed(2)) +
        metricCard('Budget', data.monthly_budget_cents > 0
          ? data.budget_utilization_pct + '%'
          : 'Unlimited') +
        metricCard('Uptime', formatDuration(data.uptime_secs));
    }).catch(function () {
      var cards = document.getElementById('metric-cards');
      if (cards) cards.innerHTML = '<p class="text-muted">Failed to load metrics</p>';
    });

    var feedEl = document.getElementById('activity-feed');
    if (feedEl) renderActivityFeed(feedEl);

    // Load recent runs for chart
    apiFetch('/api/runs?limit=100').then(function (data) {
      var chartEl = document.getElementById('run-chart');
      if (!chartEl) return;
      renderRunChart(chartEl, data.runs || []);
    }).catch(function () {
      var chartEl = document.getElementById('run-chart');
      if (chartEl) chartEl.innerHTML = '<p class="text-muted">No data</p>';
    });
  }

  function metricCard(label, value) {
    return '<div class="metric-card"><div class="metric-value">' + value +
      '</div><div class="metric-label">' + label + '</div></div>';
  }

  function renderActivityFeed(el) {
    if (activityFeed.length === 0) {
      el.innerHTML = '<p class="text-muted">Waiting for events...</p>';
      return;
    }
    var html = '';
    activityFeed.forEach(function (item) {
      var cls = item.kind === 'run_error' || item.kind === 'budget_exceeded'
        ? ' feed-error' : (item.kind === 'budget_warning' ? ' feed-warn' : '');
      html += '<div class="feed-item' + cls + '">' +
        '<span class="feed-time">' + item.time + '</span>' +
        '<span class="feed-session">' + item.session + '</span>' +
        '<span class="feed-detail">' + item.detail + '</span></div>';
    });
    el.innerHTML = html;
  }

  function renderRunChart(el, runs) {
    // Group runs by day (last 7 days)
    var days = {};
    var now = new Date();
    for (var i = 6; i >= 0; i--) {
      var d = new Date(now); d.setDate(d.getDate() - i);
      var key = d.toISOString().split('T')[0];
      days[key] = 0;
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

    var barWidth = Math.floor(280 / labels.length) - 4;
    var svg = '<svg width="100%" height="160" viewBox="0 0 300 160">';
    labels.forEach(function (label, i) {
      var h = (values[i] / max) * 120;
      var x = i * (barWidth + 4) + 10;
      var y = 130 - h;
      svg += '<rect x="' + x + '" y="' + y + '" width="' + barWidth +
        '" height="' + h + '" fill="var(--accent)" rx="2"/>';
      svg += '<text x="' + (x + barWidth / 2) + '" y="148" text-anchor="middle" ' +
        'fill="var(--text-muted)" font-size="9">' + label.substring(5) + '</text>';
      if (values[i] > 0) {
        svg += '<text x="' + (x + barWidth / 2) + '" y="' + (y - 4) + '" text-anchor="middle" ' +
          'fill="var(--text)" font-size="10">' + values[i] + '</text>';
      }
    });
    svg += '</svg>';
    el.innerHTML = svg;
  }

  // --- Sessions ---
  function renderSessions() {
    content.innerHTML = '<div class="page"><h2>Sessions</h2><div id="session-list">Loading...</div></div>';
    fetchSessions();
  }

  function fetchSessions() {
    apiFetch('/api/sessions').then(function (data) {
      sessions = data.sessions || [];
      var listEl = document.getElementById('session-list');
      if (!listEl) return;

      if (sessions.length === 0) {
        listEl.innerHTML = '<p class="text-muted">No active sessions</p>';
        return;
      }

      var html = '<div class="table-wrap"><table><thead><tr><th>Session</th><th>Actions</th></tr></thead><tbody>';
      sessions.forEach(function (s) {
        html += '<tr><td>' + escapeHtml(truncate(s, 50)) + '</td>' +
          '<td><a href="#/chat/' + encodeURIComponent(s) + '" class="btn-sm">Chat</a></td></tr>';
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
      '<div class="chat-header"><a href="#/sessions" class="back-link">← Sessions</a>' +
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
      var sid = 'web-' + Date.now().toString(36);
      window.location.hash = '#/chat/' + sid;
    });

    // Load history
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
    content.innerHTML = '<div class="page"><h2>Run History</h2><div id="runs-table">Loading...</div>' +
      '<div id="runs-pagination" class="pagination"></div></div>';

    loadRuns(0);
  }

  function loadRuns(offset) {
    apiFetch('/api/runs?limit=20&offset=' + offset).then(function (data) {
      var el = document.getElementById('runs-table');
      if (!el) return;
      var runs = data.runs || [];
      if (runs.length === 0) {
        el.innerHTML = '<p class="text-muted">No runs recorded yet</p>';
        return;
      }
      var html = '<div class="table-wrap"><table><thead><tr>' +
        '<th>Time</th><th>Session</th><th>Model</th><th>Turns</th>' +
        '<th>Tokens</th><th>Cost</th><th>Type</th><th>Status</th></tr></thead><tbody>';
      runs.forEach(function (r) {
        var tokens = (r.input_tokens || 0) + (r.output_tokens || 0);
        var cost = '$' + ((r.cost_cents || 0) / 100).toFixed(3);
        var time = r.start_time ? new Date(r.start_time).toLocaleString() : '-';
        html += '<tr>' +
          '<td class="text-muted">' + time + '</td>' +
          '<td>' + truncate(r.session_id || '', 12) + '</td>' +
          '<td>' + escapeHtml(r.model || '-') + '</td>' +
          '<td>' + (r.total_turns || 0) + '</td>' +
          '<td>' + tokens.toLocaleString() + '</td>' +
          '<td>' + cost + '</td>' +
          '<td><span class="badge badge-' + (r.billing_type || 'api') + '">' +
          (r.billing_type || 'api') + '</span></td>' +
          '<td><span class="status-' + (r.status || 'unknown') + '">' +
          (r.status || '-') + '</span></td></tr>';
      });
      html += '</tbody></table></div>';
      el.innerHTML = html;

      // Pagination
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
      if (el) el.innerHTML = '<p class="text-muted">Failed to load runs (cost tracking may not be configured)</p>';
    });
  }

  // --- Costs ---
  function renderCosts() {
    var now = new Date();
    var thirtyDaysAgo = new Date(now.getTime() - 30 * 86400000);

    content.innerHTML =
      '<div class="page"><h2>Cost Analysis</h2>' +
      '<div class="cost-controls">' +
      '<label>From: <input type="date" id="cost-from" value="' + thirtyDaysAgo.toISOString().split('T')[0] + '"></label>' +
      '<label>To: <input type="date" id="cost-to" value="' + now.toISOString().split('T')[0] + '"></label>' +
      '<label>Group by: <select id="cost-group">' +
      '<option value="model">Model</option><option value="provider">Provider</option>' +
      '<option value="day">Day</option></select></label>' +
      '<button id="cost-refresh" class="btn-sm">Refresh</button></div>' +
      '<div id="cost-summary" class="cost-summary">Loading...</div>' +
      '<div id="cost-table">Loading...</div></div>';

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
          metricCard('Total Cost', '$' + ((s.total_cost_cents || 0) / 100).toFixed(2)) +
          metricCard('Input Tokens', (s.total_input_tokens || 0).toLocaleString()) +
          metricCard('Output Tokens', (s.total_output_tokens || 0).toLocaleString()) +
          metricCard('Events', (s.total_events || 0).toLocaleString()) +
          '</div>';
      }

      var tableEl = document.getElementById('cost-table');
      if (!tableEl) return;
      var breakdown = data.breakdown || [];
      if (breakdown.length === 0) {
        tableEl.innerHTML = '<p class="text-muted">No cost data for this period</p>';
        return;
      }
      var html = '<div class="table-wrap"><table><thead><tr><th>' +
        capitalizeFirst(groupBy) + '</th><th>Cost</th><th>Input Tokens</th><th>Output Tokens</th></tr></thead><tbody>';
      breakdown.forEach(function (row) {
        html += '<tr><td>' + escapeHtml(row.key) + '</td>' +
          '<td>$' + ((row.cost_cents || 0) / 100).toFixed(3) + '</td>' +
          '<td>' + (row.input_tokens || 0).toLocaleString() + '</td>' +
          '<td>' + (row.output_tokens || 0).toLocaleString() + '</td></tr>';
      });
      html += '</tbody></table></div>';
      tableEl.innerHTML = html;
    }).catch(function () {
      var el = document.getElementById('cost-table');
      if (el) el.innerHTML = '<p class="text-muted">Cost tracking not configured</p>';
    });
  }

  // --- Settings ---
  function renderSettings() {
    content.innerHTML = '<div class="page"><h2>Settings</h2><div id="settings-content">Loading...</div></div>';
    apiFetch('/api/health').then(function (data) {
      var el = document.getElementById('settings-content');
      if (!el) return;
      el.innerHTML =
        '<div class="card"><h3>System</h3>' +
        '<p><strong>Version:</strong> ' + (data.version || 'unknown') + '</p>' +
        '<p><strong>Status:</strong> ' + (data.status || 'unknown') + '</p></div>' +
        '<div class="card"><h3>Budget</h3><div id="budget-info">Loading...</div></div>';

      apiFetch('/api/metrics').then(function (m) {
        var budgetEl = document.getElementById('budget-info');
        if (!budgetEl) return;
        if (m.monthly_budget_cents > 0) {
          budgetEl.innerHTML =
            '<p><strong>Monthly Budget:</strong> $' + (m.monthly_budget_cents / 100).toFixed(2) + '</p>' +
            '<p><strong>Spent:</strong> $' + (m.total_cost_cents / 100).toFixed(2) + '</p>' +
            '<p><strong>Utilization:</strong> ' + m.budget_utilization_pct + '%</p>' +
            '<div class="budget-bar"><div class="budget-fill" style="width:' +
            Math.min(m.budget_utilization_pct, 100) + '%"></div></div>';
        } else {
          budgetEl.innerHTML = '<p class="text-muted">No budget configured</p>';
        }
      }).catch(function () {});
    }).catch(function () {
      var el = document.getElementById('settings-content');
      if (el) el.innerHTML = '<p class="text-muted">Failed to load settings</p>';
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
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
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
