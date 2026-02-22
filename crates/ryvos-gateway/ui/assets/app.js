(function () {
  'use strict';

  // --- State ---
  let apiKey = localStorage.getItem('ryvos_api_key') || '';
  let ws = null;
  let currentSessionId = '';
  let sessions = [];
  let streaming = false;
  let streamingEl = null;
  let streamingText = '';

  // --- DOM refs ---
  const loginOverlay = document.getElementById('login-overlay');
  const apiKeyInput = document.getElementById('api-key-input');
  const loginBtn = document.getElementById('login-btn');
  const logoutBtn = document.getElementById('logout-btn');
  const sessionList = document.getElementById('session-list');
  const newSessionBtn = document.getElementById('new-session-btn');
  const messagesEl = document.getElementById('messages');
  const messageInput = document.getElementById('message-input');
  const sendBtn = document.getElementById('send-btn');

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
    newSessionBtn.addEventListener('click', newSession);
    sendBtn.addEventListener('click', sendMessage);
    messageInput.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        sendMessage();
      }
    });
    messageInput.addEventListener('input', autoResize);
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
  }

  function handleLogout() {
    apiKey = '';
    localStorage.removeItem('ryvos_api_key');
    if (ws) ws.close();
    ws = null;
    sessions = [];
    currentSessionId = '';
    sessionList.innerHTML = '';
    messagesEl.innerHTML = '';
    showLogin();
  }

  function authHeaders() {
    var h = { 'Content-Type': 'application/json' };
    if (apiKey) h['Authorization'] = 'Bearer ' + apiKey;
    return h;
  }

  // --- API helpers ---
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
      } else if (data.type === 'response') {
        handleResponse(data);
      }
    };

    ws.onclose = function () {
      // Reconnect after delay
      setTimeout(function () {
        if (apiKey) connect();
      }, 3000);
    };

    ws.onerror = function () {
      // Let onclose handle reconnect
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

    if (data.session_id !== currentSessionId) return;

    switch (evt.kind) {
      case 'text_delta':
        if (!streaming) {
          streaming = true;
          streamingText = '';
          streamingEl = addMessage('assistant', '');
        }
        streamingText += evt.text || '';
        updateMessageBody(streamingEl, streamingText, true);
        scrollToBottom();
        break;
      case 'run_complete':
        if (streaming) {
          updateMessageBody(streamingEl, streamingText, false);
          streaming = false;
          streamingEl = null;
          streamingText = '';
        }
        sendBtn.disabled = false;
        messageInput.disabled = false;
        messageInput.focus();
        break;
      case 'run_error':
        if (streaming) {
          updateMessageBody(streamingEl, streamingText, false);
          streaming = false;
          streamingEl = null;
          streamingText = '';
        }
        var errText = (evt.data && evt.data.error) || 'Unknown error';
        addMessage('assistant', 'Error: ' + errText);
        sendBtn.disabled = false;
        messageInput.disabled = false;
        break;
      case 'tool_start':
        // Could show tool usage indicator
        break;
      case 'tool_end':
        break;
    }
  }

  function handleResponse(_data) {
    // Responses to WS RPC calls are handled implicitly via events
  }

  // --- Sessions ---
  function fetchSessions() {
    apiFetch('/api/sessions').then(function (data) {
      sessions = data.sessions || [];
      renderSessions();
      if (sessions.length > 0 && !currentSessionId) {
        selectSession(sessions[0]);
      }
    }).catch(function () {
      // If health check passes but sessions fail, we might need auth
      apiFetch('/api/health').catch(function () {
        showLogin();
      });
    });
  }

  function renderSessions() {
    sessionList.innerHTML = '';
    sessions.forEach(function (s) {
      var el = document.createElement('div');
      el.className = 'session-item' + (s === currentSessionId ? ' active' : '');
      el.textContent = truncate(s, 28);
      el.title = s;
      el.addEventListener('click', function () { selectSession(s); });
      sessionList.appendChild(el);
    });
  }

  function selectSession(sid) {
    currentSessionId = sid;
    renderSessions();
    loadHistory(sid);
  }

  function newSession() {
    var sid = 'web-' + Date.now().toString(36);
    currentSessionId = sid;
    if (sessions.indexOf(sid) === -1) {
      sessions.unshift(sid);
    }
    renderSessions();
    messagesEl.innerHTML = '';
    messageInput.focus();
  }

  function loadHistory(sid) {
    messagesEl.innerHTML = '';
    apiFetch('/api/sessions/' + encodeURIComponent(sid) + '/history?limit=100').then(function (data) {
      var msgs = data.messages || [];
      msgs.forEach(function (m) {
        // Preserve the raw role from the API (user, assistant, tool, system)
        var role = m.role || 'assistant';
        addMessage(role, m.text || '');
      });
      scrollToBottom();
    }).catch(function () {
      // Session may not exist yet
    });
  }

  // --- Messages ---
  function sendMessage() {
    var text = messageInput.value.trim();
    if (!text || streaming) return;

    addMessage('user', text);
    messageInput.value = '';
    autoResize();
    scrollToBottom();

    sendBtn.disabled = true;
    messageInput.disabled = true;

    // Send via WebSocket only — streaming events handle the response.
    // The REST endpoint remains available for external API clients.
    wsSend('agent.send', { session_id: currentSessionId, message: text });
  }

  function addMessage(role, text) {
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
    messagesEl.appendChild(wrapper);
    scrollToBottom();
    return wrapper;
  }

  function updateMessageBody(el, text, isStreaming) {
    if (!el) return;
    var body = el.querySelector('.message-body');
    if (!body) return;
    body.innerHTML = renderMarkdown(text) + (isStreaming ? '<span class="streaming-cursor"></span>' : '');
  }

  // --- Markdown rendering (basic) ---
  function renderMarkdown(text) {
    if (!text) return '';

    // Escape HTML
    text = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

    // Code blocks
    text = text.replace(/```(\w*)\n([\s\S]*?)```/g, function (_, lang, code) {
      return '<pre><code>' + code.trim() + '</code></pre>';
    });

    // Inline code
    text = text.replace(/`([^`]+)`/g, '<code>$1</code>');

    // Bold
    text = text.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');

    // Italic
    text = text.replace(/\*(.+?)\*/g, '<em>$1</em>');

    // Line breaks
    text = text.replace(/\n/g, '<br>');

    return text;
  }

  // --- Utilities ---
  function scrollToBottom() {
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  function autoResize() {
    messageInput.style.height = 'auto';
    messageInput.style.height = Math.min(messageInput.scrollHeight, 120) + 'px';
  }

  function truncate(s, max) {
    return s.length > max ? s.substring(0, max) + '...' : s;
  }

  // --- Boot ---
  document.addEventListener('DOMContentLoaded', init);
})();
