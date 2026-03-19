<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let runs = [];
  let totalRuns = 0;
  let loading = true;
  let error = '';
  let currentOffset = 0;
  const pageSize = 20;

  async function loadRuns(offset) {
    loading = true;
    error = '';
    currentOffset = offset;
    try {
      const data = await apiFetch(`/api/runs?limit=${pageSize}&offset=${offset}`);
      runs = data.runs || [];
      totalRuns = data.total || 0;
    } catch (e) {
      error = e.message;
      runs = [];
    } finally {
      loading = false;
    }
  }

  onMount(() => loadRuns(0));

  function formatTime(isoStr) {
    if (!isoStr) return '-';
    return new Date(isoStr).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  function truncate(s, max) {
    if (!s) return '';
    return s.length > max ? s.substring(0, max) + '...' : s;
  }

  $: totalPages = Math.ceil(totalRuns / pageSize);
  $: currentPage = Math.floor(currentOffset / pageSize);
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-[#E8E4E0]">Run History</h2>
    <p class="text-[#A09890] text-sm mt-1">All recorded agent runs</p>
  </div>

  {#if loading}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-8 text-center">
      <p class="text-[#A09890] text-sm animate-pulse">Loading runs...</p>
    </div>
  {:else if error}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-12 text-center">
      <p class="text-[#A09890] text-sm">Cost tracking not configured</p>
    </div>
  {:else if runs.length === 0}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-12 text-center">
      <p class="text-[#A09890] text-sm">No runs recorded yet</p>
    </div>
  {:else}
    <div class="border border-[rgba(255,255,255,0.08)] rounded-xl overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr>
            {#each ['Time', 'Session', 'Model', 'Turns', 'Tokens', 'Cost', 'Type', 'Status'] as col}
              <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)] sticky top-0">
                {col}
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each runs as run}
            {@const tokens = (run.input_tokens || 0) + (run.output_tokens || 0)}
            {@const cost = '$' + ((run.cost_cents || 0) / 100).toFixed(3)}
            <tr class="hover:bg-[#2A2A2A]/40 transition-colors duration-150">
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] font-mono text-xs text-[#A09890]">{formatTime(run.start_time)}</td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] font-mono text-[0.7rem] text-[#A09890]">{truncate(run.session_id || '', 12)}</td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-[#E8E4E0]">{run.model || '-'}</td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-[#E8E4E0]">{run.total_turns || 0}</td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-[#E8E4E0]">{tokens.toLocaleString()}</td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] font-mono text-[#E8E4E0]">{cost}</td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)]">
                <span class="inline-flex px-2.5 py-0.5 rounded-full text-[0.7rem] font-semibold
                  {run.billing_type === 'subscription' ? 'bg-emerald-400/10 text-emerald-400' : 'bg-[#F07030]/10 text-[#F07030]'}">
                  {run.billing_type || 'api'}
                </span>
              </td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)]">
                <span class="font-medium
                  {run.status === 'complete' ? 'text-emerald-400' :
                   run.status === 'running' ? 'text-[#F07030]' :
                   run.status === 'error' ? 'text-red-400' : 'text-[#A09890]'}">
                  {run.status || '-'}
                </span>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <!-- Pagination -->
    {#if totalPages > 1}
      <div class="flex justify-center gap-1 mt-5">
        {#each Array(Math.min(totalPages, 10)) as _, i}
          <button
            on:click={() => loadRuns(i * pageSize)}
            class="px-3 py-1.5 rounded-md text-xs font-medium transition-all duration-200
              {i === currentPage
                ? 'bg-[#F07030] text-white border border-[#F07030]'
                : 'bg-[#222222] border border-[rgba(255,255,255,0.08)] text-[#A09890] hover:bg-[#2A2A2A] hover:text-[#E8E4E0]'}"
          >
            {i + 1}
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>
