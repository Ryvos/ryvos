<script>
  import { onMount, onDestroy } from 'svelte';
  import { apiFetch } from '../api.js';
  import { activityFeed, streamingText } from '../ws.js';

  let goalDescription = '';
  let channel = '';
  let executing = false;
  let activeSession = '';
  let result = '';
  let error = '';
  let currentStream = '';
  let toolEvents = [];
  let history = [];
  let historyLoading = true;

  // Subscribe to real-time streaming
  const unsubStream = streamingText.subscribe(v => {
    if (executing && v) currentStream = v;
  });

  const unsubActivity = activityFeed.subscribe(feed => {
    if (!executing || feed.length === 0) return;
    const latest = feed[0];
    if (latest.kind === 'tool_start') {
      toolEvents = [{ kind: 'start', tool: latest.text, time: latest.time }, ...toolEvents].slice(0, 30);
    } else if (latest.kind === 'tool_end') {
      toolEvents = [{ kind: 'end', tool: latest.text, time: latest.time }, ...toolEvents].slice(0, 30);
    } else if (latest.kind === 'run_complete') {
      executing = false;
      result = currentStream || 'Goal execution completed.';
      loadHistory();
    } else if (latest.kind === 'run_error') {
      executing = false;
      error = latest.text || 'Goal execution failed.';
    }
  });

  async function executeGoal() {
    if (!goalDescription.trim()) return;
    executing = true;
    result = '';
    error = '';
    currentStream = '';
    toolEvents = [];

    try {
      const data = await apiFetch('/api/goals/run', {
        method: 'POST',
        body: JSON.stringify({
          description: goalDescription,
          channel: channel || undefined,
        }),
      });
      activeSession = data.session_id;
    } catch (e) {
      error = e.message;
      executing = false;
    }
  }

  async function loadHistory() {
    historyLoading = true;
    try {
      const data = await apiFetch('/api/goals/history');
      history = data.runs || [];
    } catch (e) {
      history = [];
    } finally {
      historyLoading = false;
    }
  }

  function formatTime(isoStr) {
    if (!isoStr) return '-';
    return new Date(isoStr).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  onMount(() => loadHistory());
  onDestroy(() => { unsubStream(); unsubActivity(); });
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Goals</h2>
    <p class="text-[#9B9590] text-sm mt-1">Director-driven goal orchestration — OODA loop with auto-evolving execution graphs.</p>
  </div>

  <!-- Goal Input -->
  <div class="bg-white border-2 border-[#1A1A1A] p-6 mb-6">
    <label class="text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-2">Goal Description</label>
    <textarea
      bind:value={goalDescription}
      placeholder="Describe what you want the agent to achieve..."
      rows="3"
      class="w-full px-4 py-3 border-2 border-[#1A1A1A] bg-[#FEFCF9] text-[#1A1A1A] font-mono text-sm resize-none focus:outline-none focus:border-[#F07030]"
    ></textarea>

    <div class="flex items-center gap-4 mt-4">
      <div class="flex-1">
        <label class="text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Route result to</label>
        <select
          bind:value={channel}
          class="px-3 py-2 border-2 border-[#1A1A1A] bg-white text-sm text-[#6B6560] focus:outline-none"
        >
          <option value="">None (UI only)</option>
          <option value="telegram">Telegram</option>
        </select>
      </div>
      <button
        on:click={executeGoal}
        disabled={executing || !goalDescription.trim()}
        class="px-8 py-3 text-sm font-bold uppercase tracking-wider transition-all duration-200
          {executing
            ? 'bg-[#9B9590] text-white border-2 border-[#1A1A1A] cursor-wait'
            : 'bg-[#F07030] text-white border-2 border-[#1A1A1A] hover:shadow-brutal active:translate-x-[2px] active:translate-y-[2px] active:shadow-none'}"
      >
        {executing ? 'Executing...' : 'Execute Goal'}
      </button>
    </div>
  </div>

  <!-- Live Execution Feed -->
  {#if executing || result || error}
    <div class="bg-white border-2 border-[#1A1A1A] p-6 mb-6">
      <div class="flex items-center justify-between mb-3">
        <span class="text-xs uppercase tracking-wider font-bold text-[#9B9590]">Execution</span>
        {#if executing}
          <span class="inline-flex items-center gap-1.5 text-xs font-semibold text-[#F07030]">
            <span class="w-2 h-2 bg-[#F07030] rounded-full animate-pulse"></span>
            Running
          </span>
        {:else if error}
          <span class="text-xs font-semibold text-[#DC2626]">Failed</span>
        {:else}
          <span class="text-xs font-semibold text-[#16A34A]">Complete</span>
        {/if}
      </div>

      {#if activeSession}
        <p class="text-xs font-mono text-[#9B9590] mb-3">Session: {activeSession}</p>
      {/if}

      <!-- Tool activity -->
      {#if toolEvents.length > 0}
        <div class="mb-4 max-h-40 overflow-y-auto">
          {#each toolEvents as evt}
            <div class="flex items-center gap-2 py-0.5 text-xs font-mono">
              <span class={evt.kind === 'start' ? 'text-[#F07030]' : 'text-[#16A34A]'}>
                {evt.kind === 'start' ? '>' : '<'}
              </span>
              <span class="text-[#6B6560]">{evt.tool}</span>
            </div>
          {/each}
        </div>
      {/if}

      <!-- Streaming text -->
      {#if currentStream && executing}
        <div class="border-t-2 border-[#E8E4E0] pt-3 mt-3">
          <pre class="text-sm text-[#1A1A1A] whitespace-pre-wrap font-mono leading-relaxed">{currentStream}</pre>
        </div>
      {/if}

      <!-- Final result -->
      {#if result && !executing}
        <div class="border-t-2 border-[#E8E4E0] pt-3 mt-3">
          <pre class="text-sm text-[#1A1A1A] whitespace-pre-wrap font-mono leading-relaxed">{result}</pre>
        </div>
      {/if}

      {#if error}
        <div class="border-t-2 border-[#DC2626]/30 pt-3 mt-3">
          <p class="text-sm text-[#DC2626] font-mono">{error}</p>
        </div>
      {/if}
    </div>
  {/if}

  <!-- History -->
  <div>
    <h3 class="text-lg font-heading font-bold text-[#1A1A1A] mb-3">Run History</h3>
    {#if historyLoading}
      <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
        <p class="text-[#9B9590] text-sm animate-pulse">Loading...</p>
      </div>
    {:else if history.length === 0}
      <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
        <p class="text-[#9B9590] text-sm">No goal runs yet. Execute your first goal above.</p>
      </div>
    {:else}
      <div class="border-2 border-[#1A1A1A] overflow-x-auto">
        <table class="w-full text-sm">
          <thead>
            <tr>
              {#each ['Time', 'Session', 'Turns', 'Status'] as col}
                <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">{col}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each history as run}
              <tr class="hover:bg-[#F7F4F0] transition-colors duration-150">
                <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-xs text-[#9B9590]">{formatTime(run.start_time)}</td>
                <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-[0.7rem] text-[#9B9590]">{(run.session_id || '').substring(0, 20)}</td>
                <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#1A1A1A]">{run.total_turns || 0}</td>
                <td class="px-4 py-3 border-b border-[#E8E4E0]">
                  <span class="font-medium {run.status === 'complete' ? 'text-[#16A34A]' : run.status === 'running' ? 'text-[#F07030]' : 'text-[#DC2626]'}">
                    {run.status || '-'}
                  </span>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
