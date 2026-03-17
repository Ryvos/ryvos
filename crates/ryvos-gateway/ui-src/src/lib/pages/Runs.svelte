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
    <h2 class="text-2xl font-bold tracking-tight text-gray-100">Run History</h2>
    <p class="text-gray-500 text-sm mt-1">All recorded agent runs</p>
  </div>

  {#if loading}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-8 text-center">
      <p class="text-gray-500 text-sm animate-pulse">Loading runs...</p>
    </div>
  {:else if error}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">Cost tracking not configured</p>
    </div>
  {:else if runs.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">No runs recorded yet</p>
    </div>
  {:else}
    <div class="border border-gray-800 rounded-xl overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr>
            {#each ['Time', 'Session', 'Model', 'Turns', 'Tokens', 'Cost', 'Type', 'Status'] as col}
              <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800 sticky top-0">
                {col}
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each runs as run}
            {@const tokens = (run.input_tokens || 0) + (run.output_tokens || 0)}
            {@const cost = '$' + ((run.cost_cents || 0) / 100).toFixed(3)}
            <tr class="hover:bg-gray-800/40 transition-colors duration-150">
              <td class="px-4 py-3 border-b border-gray-800/50 font-mono text-xs text-gray-400">{formatTime(run.start_time)}</td>
              <td class="px-4 py-3 border-b border-gray-800/50 font-mono text-[0.7rem] text-gray-400">{truncate(run.session_id || '', 12)}</td>
              <td class="px-4 py-3 border-b border-gray-800/50 text-gray-300">{run.model || '-'}</td>
              <td class="px-4 py-3 border-b border-gray-800/50 text-gray-300">{run.total_turns || 0}</td>
              <td class="px-4 py-3 border-b border-gray-800/50 text-gray-300">{tokens.toLocaleString()}</td>
              <td class="px-4 py-3 border-b border-gray-800/50 font-mono text-gray-300">{cost}</td>
              <td class="px-4 py-3 border-b border-gray-800/50">
                <span class="inline-flex px-2.5 py-0.5 rounded-full text-[0.7rem] font-semibold
                  {run.billing_type === 'subscription' ? 'bg-emerald-400/10 text-emerald-400' : 'bg-indigo-400/10 text-indigo-400'}">
                  {run.billing_type || 'api'}
                </span>
              </td>
              <td class="px-4 py-3 border-b border-gray-800/50">
                <span class="font-medium
                  {run.status === 'complete' ? 'text-emerald-400' :
                   run.status === 'running' ? 'text-indigo-400' :
                   run.status === 'error' ? 'text-red-400' : 'text-gray-400'}">
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
                ? 'bg-indigo-400 text-white border border-indigo-400'
                : 'bg-gray-900 border border-gray-800 text-gray-400 hover:bg-gray-800 hover:text-gray-200'}"
          >
            {i + 1}
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>
