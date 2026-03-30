<script>
  import { onMount, onDestroy, tick } from 'svelte';
  import { apiFetch } from '../api.js';
  import { sendWs, streamingText, resetStreamingText, activityFeed } from '../ws.js';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  export let sessionId = '';

  let messages = [];
  let messageText = '';
  let sending = false;
  let messagesEl;
  let currentStreamText = '';
  let streaming = false;
  let sessionList = [];
  let showSessionDropdown = false;

  // Configure marked
  marked.setOptions({ breaks: true, gfm: true });

  function renderMarkdown(text) {
    if (!text) return '';
    return DOMPurify.sanitize(marked.parse(text));
  }

  function timeNow() {
    return new Date().toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  // Subscribe to streaming text
  const unsubStream = streamingText.subscribe(v => {
    currentStreamText = v;
    if (v && !streaming) streaming = true;
  });

  // Subscribe to activity feed for run_complete/run_error/tool events
  const unsubActivity = activityFeed.subscribe(feed => {
    if (feed.length > 0) {
      const latest = feed[0];

      // Tool call events — insert inline
      if (latest.kind === 'tool_start' && streaming) {
        messages = [...messages, {
          type: 'tool_call',
          tool: latest.detail?.replace('Tool: ', '') || 'unknown',
          status: 'running',
          expanded: false,
          input: latest.data?.input || '',
          output: null,
          startTime: Date.now(),
          elapsed: null,
        }];
      }

      if (latest.kind === 'tool_end' && streaming) {
        // Update matching tool_call message with output + elapsed (match by tool name for parallel calls)
        const endedTool = (latest.text || latest.detail?.replace('Tool done: ', '') || '').trim();
        const lastTool = [...messages].reverse().find(
          m => m.type === 'tool_call' && m.status === 'running' && (!endedTool || m.tool === endedTool)
        );
        if (lastTool) {
          lastTool.status = 'done';
          lastTool.output = latest.data?.output || latest.detail || '';
          lastTool.elapsed = Date.now() - lastTool.startTime;
          messages = [...messages]; // trigger reactivity
        }
      }

      // Approval requested
      if (latest.kind === 'approval_requested') {
        messages = [...messages, {
          type: 'approval',
          id: latest.data?.request_id || '',
          tool_name: latest.data?.tool_name || 'unknown',
          input_summary: latest.data?.input_summary || '',
          resolved: false,
          approved: false,
        }];
      }

      if (streaming && (latest.kind === 'run_complete' || latest.kind === 'run_error')) {
        if (currentStreamText) {
          messages = [...messages, {
            role: 'assistant',
            text: currentStreamText,
            timestamp: timeNow(),
            tokens: latest.data ? {
              input: latest.data.input_tokens || 0,
              output: latest.data.output_tokens || 0,
            } : null,
          }];
        }
        if (latest.kind === 'run_error') {
          messages = [...messages, { role: 'assistant', text: 'Error: ' + (latest.data?.error || 'Unknown'), timestamp: timeNow() }];
        }
        streaming = false;
        sending = false;
        resetStreamingText();
      }
    }
  });

  async function scrollToBottom() {
    await tick();
    if (messagesEl) {
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }
  }

  async function loadSessions() {
    try {
      const data = await apiFetch('/api/sessions');
      const raw = data.sessions || [];
      // Handle both formats: array of strings OR array of objects with .id
      sessionList = raw.map(s => typeof s === 'string' ? s : (s.id || s.session_key || String(s)));
    } catch (e) {
      console.error('Failed to load sessions:', e);
    }
  }

  async function loadHistory() {
    if (!sessionId) return;
    try {
      const data = await apiFetch(`/api/sessions/${encodeURIComponent(sessionId)}/history?limit=100`);
      const raw = data.messages || [];
      messages = raw
        .filter(m => m.text && m.text.trim())
        .map(m => ({
          role: m.role || 'assistant',
          text: m.text || '',
          timestamp: m.timestamp ? new Date(m.timestamp).toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit' }) : '',
        }));
      if (messages.length === 0 && raw.length > 0) {
        messages = [{ role: 'system', text: `${raw.length} messages in this session contain only tool calls or system data.`, timestamp: '' }];
      }
      scrollToBottom();
    } catch (e) {
      console.error('Failed to load history:', e);
      messages = [{ role: 'system', text: 'Failed to load chat history. Check your connection.', timestamp: '' }];
    }
  }

  function handleSend() {
    const text = messageText.trim();
    if (!text || sending) return;

    messages = [...messages, { role: 'user', text, timestamp: timeNow() }];
    messageText = '';
    sending = true;
    resetStreamingText();
    scrollToBottom();

    if (!sessionId) {
      sessionId = 'web-' + Date.now().toString(36);
    }
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

  async function handleApproval(id, approved) {
    try {
      await apiFetch(`/api/approvals/${id}/${approved ? 'approve' : 'deny'}`, { method: 'POST' });
      const msg = messages.find(m => m.type === 'approval' && m.id === id);
      if (msg) {
        msg.resolved = true;
        msg.approved = approved;
        messages = [...messages];
      }
    } catch {}
  }

  onMount(() => {
    loadSessions();
    if (sessionId) loadHistory();
  });

  onDestroy(() => {
    unsubStream();
    unsubActivity();
  });

  $: if (sessionId) {
    messages = [];
    loadHistory();
  }
  $: scrollToBottom(), currentStreamText;
</script>

<div class="flex flex-col h-[calc(100vh-4rem)]">
  <!-- Header -->
  <div class="flex items-center gap-4 pb-4 border-b-2 border-[#1A1A1A] mb-4">
    <!-- Session selector dropdown -->
    <div class="relative">
      <button
        on:click={() => { showSessionDropdown = !showSessionDropdown; loadSessions(); }}
        class="flex items-center gap-2 px-3 py-1.5 bg-white border-2 border-[#1A1A1A] text-xs text-[#6B6560] hover:text-[#1A1A1A] hover:bg-[#F7F4F0] transition-colors"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
        {sessionId ? (sessionId.length > 20 ? sessionId.substring(0, 20) + '...' : sessionId) : 'Select session'}
        <svg class="w-3 h-3 transition-transform {showSessionDropdown ? 'rotate-180' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if showSessionDropdown}
        <div class="absolute top-full left-0 mt-1 w-72 bg-white border-2 border-[#1A1A1A] shadow-brutal z-30 max-h-60 overflow-y-auto">
          {#each sessionList as sid}
            <a
              href="#/chat/{encodeURIComponent(sid)}"
              on:click={() => showSessionDropdown = false}
              class="block px-3 py-2 text-xs font-mono text-[#6B6560] hover:bg-[#F7F4F0] hover:text-[#1A1A1A] transition-colors truncate
                {sid === sessionId ? 'bg-[#FEF3EC] text-[#F07030]' : ''}"
            >
              {sid}
            </a>
          {/each}
          {#if sessionList.length === 0}
            <p class="px-3 py-3 text-xs text-[#9B9590]">No sessions yet</p>
          {/if}
        </div>
      {/if}
    </div>

    <div class="flex-1"></div>

    <button
      on:click={newSession}
      class="px-3 py-1.5 bg-white border-2 border-[#1A1A1A] text-xs text-[#6B6560] font-medium
             hover:bg-[#F7F4F0] hover:text-[#F07030] hover:border-[#F07030] transition-all duration-200 shadow-brutal-sm brutal-shift"
    >
      + New
    </button>
  </div>

  {#if !sessionId && sessionList.length > 0}
    <!-- Session list when no session selected -->
    <div class="flex-1 overflow-y-auto">
      <div class="space-y-1">
        {#each sessionList as sid}
          <a href="#/chat/{encodeURIComponent(sid)}"
            class="block px-4 py-3 bg-white border-2 border-[#1A1A1A] text-sm font-mono text-[#6B6560]
                   hover:bg-[#F7F4F0] hover:text-[#1A1A1A] transition-all duration-200 truncate">
            {sid}
          </a>
        {/each}
      </div>
    </div>
  {:else if !sessionId}
    <div class="flex-1 flex items-center justify-center">
      <div class="text-center">
        <p class="text-[#9B9590] text-sm mb-4">No active sessions</p>
        <button on:click={newSession}
          class="px-4 py-2 bg-[#F07030] text-white border-2 border-[#1A1A1A] text-sm font-semibold
                 shadow-brutal brutal-shift transition-all duration-200">
          Start New Session
        </button>
      </div>
    </div>
  {:else}
    <!-- Messages -->
    <div bind:this={messagesEl} class="flex-1 overflow-y-auto py-2 space-y-4">
      {#each messages as msg}
        {#if msg.type === 'tool_call'}
          <!-- Tool call inline block -->
          <div class="max-w-[80%] mr-auto">
            <button on:click={() => { msg.expanded = !msg.expanded; messages = [...messages]; }}
              class="flex items-center gap-2 px-3 py-2 bg-[#F7F4F0] border-2 border-[#E8E4E0] text-xs text-[#6B6560] hover:text-[#1A1A1A] w-full text-left transition-colors">
              <svg class="w-3.5 h-3.5 shrink-0 {msg.status === 'running' ? 'text-[#F07030]' : 'text-[#16A34A]'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14.7 6.3a1 1 0 000 1.4l1.6 1.6a1 1 0 001.4 0l3.77-3.77a6 6 0 01-7.94 7.94l-6.91 6.91a2.12 2.12 0 01-3-3l6.91-6.91a6 6 0 017.94-7.94l-3.76 3.76z"/></svg>
              <span class="font-mono">{msg.tool}</span>
              <span class="ml-auto text-[0.65rem] {msg.status === 'running' ? 'text-[#F07030]' : 'text-[#16A34A]'}">
                {msg.status === 'running' ? 'Running...' : (msg.elapsed ? msg.elapsed + 'ms' : 'Done')}
              </span>
              <svg class="w-3 h-3 shrink-0 transition-transform {msg.expanded ? 'rotate-180' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"/></svg>
            </button>
            {#if msg.expanded}
              <div class="mt-1 px-3 py-2 bg-[#1A1A1A] border-2 border-[#1A1A1A] text-xs font-mono text-[#9B9590] max-h-40 overflow-y-auto">
                {#if msg.input}
                  <div class="text-[0.6rem] text-[#6B6560] uppercase mb-1">Input</div>
                  <pre class="whitespace-pre-wrap text-[#E8E4E0]">{typeof msg.input === 'string' ? msg.input : JSON.stringify(msg.input, null, 2)}</pre>
                {/if}
                {#if msg.output}
                  <div class="text-[0.6rem] text-[#6B6560] uppercase mt-2 mb-1">Output</div>
                  <pre class="whitespace-pre-wrap text-[#E8E4E0]">{typeof msg.output === 'string' ? msg.output : JSON.stringify(msg.output, null, 2)}</pre>
                {/if}
              </div>
            {/if}
          </div>
        {:else if msg.type === 'approval'}
          <!-- Approval request inline -->
          <div class="max-w-[80%] mr-auto">
            <div class="px-4 py-3 bg-[#FFFBEB] border-2 border-[#D97706]">
              <div class="flex items-center gap-2 mb-2">
                <svg class="w-4 h-4 text-[#D97706]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                <span class="text-sm font-semibold text-[#D97706]">Approval Required</span>
              </div>
              <p class="text-xs text-[#6B6560] mb-1">Tool: <span class="font-mono text-[#1A1A1A]">{msg.tool_name}</span></p>
              {#if msg.input_summary}
                <p class="text-xs text-[#6B6560] mb-3">{msg.input_summary}</p>
              {/if}
              {#if !msg.resolved}
                <div class="flex gap-2">
                  <button on:click={() => handleApproval(msg.id, true)}
                    class="px-4 py-1.5 bg-[#16A34A] hover:bg-[#15803D] text-white border-2 border-[#1A1A1A] text-xs font-semibold transition-colors">
                    Approve
                  </button>
                  <button on:click={() => handleApproval(msg.id, false)}
                    class="px-4 py-1.5 bg-[#DC2626] hover:bg-[#B91C1C] text-white border-2 border-[#1A1A1A] text-xs font-semibold transition-colors">
                    Deny
                  </button>
                </div>
              {:else}
                <span class="text-xs font-medium {msg.approved ? 'text-[#16A34A]' : 'text-[#DC2626]'}">
                  {msg.approved ? 'Approved' : 'Denied'}
                </span>
              {/if}
            </div>
          </div>
        {:else}
          <!-- Regular message -->
          <div class="max-w-[80%] {msg.role === 'user' ? 'ml-auto' : 'mr-auto'}">
            <div class="text-[0.6rem] text-[#9B9590] flex items-center gap-2 mb-1">
              <span class="uppercase tracking-widest font-semibold">{msg.role}</span>
              {#if msg.timestamp}
                <span class="font-mono">{msg.timestamp}</span>
              {/if}
            </div>
            <div class="px-4 py-3 text-sm leading-relaxed break-words
              {msg.role === 'user'
                ? 'bg-[#F07030] text-white'
                : 'bg-white border-2 border-[#1A1A1A] text-[#1A1A1A]'}">
              <div class="markdown-content">
                {@html renderMarkdown(msg.text)}
              </div>
            </div>
            {#if msg.tokens}
              <div class="text-[0.6rem] text-[#9B9590] mt-1 font-mono">
                {msg.tokens.input.toLocaleString()} in / {msg.tokens.output.toLocaleString()} out
              </div>
            {/if}
          </div>
        {/if}
      {/each}

      <!-- Streaming message -->
      {#if streaming && currentStreamText}
        <div class="max-w-[80%] mr-auto">
          <div class="text-[0.6rem] uppercase tracking-widest text-[#9B9590] font-semibold mb-1">assistant</div>
          <div class="px-4 py-3 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] text-sm leading-relaxed break-words">
            <div class="markdown-content">
              {@html renderMarkdown(currentStreamText)}
            </div>
            <span class="streaming-cursor"></span>
          </div>
        </div>
      {/if}
    </div>

    <!-- Input bar -->
    <div class="flex gap-2 pt-4 border-t-2 border-[#1A1A1A] mt-2">
      <textarea
        bind:value={messageText}
        on:keydown={handleKeydown}
        disabled={sending}
        placeholder="Type a message..."
        rows="1"
        class="flex-1 px-4 py-2.5 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] text-sm
               font-sans resize-none outline-none max-h-28 leading-relaxed
               transition-all duration-200 focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20
               disabled:opacity-50 disabled:cursor-not-allowed"
      ></textarea>
      <button
        on:click={handleSend}
        disabled={sending}
        class="px-6 py-2.5 bg-[#F07030] text-white border-2 border-[#1A1A1A]
               font-semibold text-sm self-end transition-all duration-200
               shadow-brutal-sm brutal-shift disabled:opacity-40 disabled:cursor-not-allowed"
      >
        Send
      </button>
    </div>
  {/if}
</div>
