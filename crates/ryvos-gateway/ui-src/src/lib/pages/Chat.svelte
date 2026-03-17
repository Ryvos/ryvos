<script>
  import { onMount, onDestroy } from 'svelte';
  import { apiFetch } from '../api.js';
  import { sendWs, streamingText, resetStreamingText, activityFeed } from '../ws.js';

  export let sessionId = '';

  let messages = [];
  let messageText = '';
  let sending = false;
  let messagesEl;
  let currentStreamText = '';
  let streaming = false;
  let sessionList = [];

  // Subscribe to streaming text
  const unsubStream = streamingText.subscribe(v => {
    currentStreamText = v;
    if (v && !streaming) streaming = true;
  });

  // Subscribe to activity feed for run_complete/run_error events
  const unsubActivity = activityFeed.subscribe(feed => {
    if (feed.length > 0 && streaming) {
      const latest = feed[0];
      if (latest.kind === 'run_complete' || latest.kind === 'run_error') {
        if (currentStreamText) {
          messages = [...messages, { role: 'assistant', text: currentStreamText }];
        }
        if (latest.kind === 'run_error') {
          messages = [...messages, { role: 'assistant', text: 'Error: ' + (latest.data?.error || 'Unknown') }];
        }
        streaming = false;
        sending = false;
        resetStreamingText();
      }
    }
  });

  function escapeHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  function renderMarkdown(text) {
    if (!text) return '';
    let html = escapeHtml(text);
    html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_, lang, code) => `<pre><code>${code.trim()}</code></pre>`);
    html = html.replace(/`([^`]+)`/g, '<code class="bg-gray-800 px-1.5 py-0.5 rounded text-sm font-mono">$1</code>');
    html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');
    html = html.replace(/\n/g, '<br>');
    return html;
  }

  function scrollToBottom() {
    if (messagesEl) {
      setTimeout(() => { messagesEl.scrollTop = messagesEl.scrollHeight; }, 10);
    }
  }

  async function loadSessions() {
    try {
      const data = await apiFetch('/api/sessions');
      sessionList = data.sessions || [];
    } catch {}
  }

  async function loadHistory() {
    if (!sessionId) return;
    try {
      const data = await apiFetch(`/api/sessions/${encodeURIComponent(sessionId)}/history?limit=100`);
      messages = (data.messages || []).map(m => ({ role: m.role || 'assistant', text: m.text || '' }));
      scrollToBottom();
    } catch {}
  }

  function handleSend() {
    const text = messageText.trim();
    if (!text || sending) return;

    messages = [...messages, { role: 'user', text }];
    messageText = '';
    sending = true;
    resetStreamingText();
    scrollToBottom();

    sendWs('agent.send', { session_id: sessionId, message: text });
  }

  function handleKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function newSession() {
    window.location.hash = '#/chat/web-' + Date.now().toString(36);
  }

  onMount(() => {
    loadSessions();
    if (sessionId) {
      loadHistory();
    }
  });

  onDestroy(() => {
    unsubStream();
    unsubActivity();
  });

  $: if (sessionId) loadHistory();
  $: scrollToBottom(), currentStreamText;
</script>

<div class="flex flex-col h-[calc(100vh-4rem)]">
  <!-- Header -->
  <div class="flex items-center gap-4 pb-4 border-b border-gray-800 mb-4">
    <a href="#/sessions" class="text-indigo-400 text-sm font-medium hover:opacity-80 transition-opacity flex items-center gap-1">
      <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
      Sessions
    </a>
    {#if sessionId}
      <span class="flex-1 text-xs text-gray-500 font-mono truncate">{sessionId}</span>
    {:else}
      <span class="flex-1 text-xs text-gray-500">Select or create a session</span>
    {/if}
    <button
      on:click={newSession}
      class="px-3 py-1.5 bg-gray-800 border border-gray-700 rounded-md text-xs text-gray-300 font-medium
             hover:bg-gray-700 hover:text-indigo-400 hover:border-indigo-400 transition-all duration-200"
    >
      + New
    </button>
  </div>

  {#if !sessionId}
    <!-- Session list -->
    <div class="flex-1 overflow-y-auto">
      {#if sessionList.length === 0}
        <div class="text-center py-16">
          <p class="text-gray-500 text-sm mb-4">No active sessions</p>
          <button on:click={newSession}
            class="px-4 py-2 bg-gradient-to-br from-indigo-400 to-indigo-600 text-white rounded-lg text-sm font-semibold
                   hover:shadow-lg hover:shadow-indigo-500/30 transition-all duration-200">
            Start New Session
          </button>
        </div>
      {:else}
        <div class="space-y-1">
          {#each sessionList as sid}
            <a href="#/chat/{encodeURIComponent(sid)}"
              class="block px-4 py-3 bg-gray-900 border border-gray-800 rounded-lg text-sm font-mono text-gray-300
                     hover:bg-gray-800 hover:border-gray-700 transition-all duration-200 truncate">
              {sid}
            </a>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <!-- Messages -->
    <div bind:this={messagesEl} class="flex-1 overflow-y-auto py-2 space-y-4">
      {#each messages as msg}
        <div class="max-w-[75%] {msg.role === 'user' ? 'ml-auto' : 'mr-auto'}">
          <div class="text-[0.65rem] uppercase tracking-widest text-gray-500 font-semibold mb-1">
            {msg.role}
          </div>
          <div class="px-4 py-3 rounded-xl text-sm leading-relaxed break-words
            {msg.role === 'user'
              ? 'bg-gradient-to-br from-indigo-400 to-indigo-600 text-white rounded-br-sm'
              : 'bg-gray-900 border border-gray-800 rounded-bl-sm text-gray-200'}">
            {@html renderMarkdown(msg.text)}
          </div>
        </div>
      {/each}

      <!-- Streaming message -->
      {#if streaming && currentStreamText}
        <div class="max-w-[75%] mr-auto">
          <div class="text-[0.65rem] uppercase tracking-widest text-gray-500 font-semibold mb-1">assistant</div>
          <div class="px-4 py-3 rounded-xl rounded-bl-sm bg-gray-900 border border-gray-800 text-gray-200 text-sm leading-relaxed break-words">
            {@html renderMarkdown(currentStreamText)}
            <span class="streaming-cursor"></span>
          </div>
        </div>
      {/if}
    </div>

    <!-- Input bar -->
    <div class="flex gap-2 pt-4 border-t border-gray-800 mt-2">
      <textarea
        bind:value={messageText}
        on:keydown={handleKeydown}
        disabled={sending}
        placeholder="Type a message..."
        rows="1"
        class="flex-1 px-4 py-2.5 bg-gray-900 border border-gray-800 rounded-lg text-gray-100 text-sm
               font-sans resize-none outline-none max-h-28 leading-relaxed
               transition-all duration-200 focus:border-indigo-400 focus:ring-2 focus:ring-indigo-400/20
               disabled:opacity-50 disabled:cursor-not-allowed"
      ></textarea>
      <button
        on:click={handleSend}
        disabled={sending}
        class="px-6 py-2.5 bg-gradient-to-br from-indigo-400 to-indigo-600 text-white rounded-lg
               font-semibold text-sm self-end transition-all duration-200
               hover:shadow-lg hover:shadow-indigo-500/30 disabled:opacity-40 disabled:cursor-not-allowed"
      >
        Send
      </button>
    </div>
  {/if}
</div>
