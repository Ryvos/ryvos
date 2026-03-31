<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let runs = [];
  let totalRuns = 0;
  let loading = true;
  let error = '';
  let note = '';
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
      note = data.note || '';
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
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Run History</h2>
    <p class="text-[#9B9590] text-sm mt-1">Each run is one agent execution cycle — a complete request → response → tool loop.</p>
  </div>

  {#if loading}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading runs...</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">Cost tracking not configured</p>
    </div>
  {:else if runs.length === 0}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">{note || 'No runs recorded yet'}</p>
    </div>
  {:else}
    <div class="border-2 border-[#1A1A1A] overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr>
            {#each ['Time', 'Session', 'Model', 'Turns', 'Tokens', 'Cost', 'Type', 'Status'] as col}
              <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A] sticky top-0">
                {col}
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each runs as run}
            {@const tokens = (run.input_tokens || 0) + (run.output_tokens || 0)}
            {@const cost = '$' + ((run.cost_cents || 0) / 100).toFixed(3)}
            <tr class="hover:bg-[#F7F4F0] transition-colors duration-150">
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-xs text-[#9B9590]">{formatTime(run.start_time)}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-[0.7rem] text-[#9B9590]">{truncate(run.session_id || '', 12)}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#1A1A1A]">{run.model || '-'}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#1A1A1A]">{run.total_turns || 0}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#1A1A1A]">{tokens.toLocaleString()}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-[#1A1A1A]">{cost}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0]">
                <span class="inline-flex px-2.5 py-0.5 text-[0.7rem] font-semibold
                  {run.billing_type === 'subscription' ? 'bg-[#16A34A]/10 text-[#16A34A]' : 'bg-[#F07030]/10 text-[#F07030]'}">
                  {run.billing_type || 'api'}
                </span>
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0]">
                <span class="font-medium
                  {run.status === 'complete' ? 'text-[#16A34A]' :
                   run.status === 'running' ? 'text-[#F07030]' :
                   run.status === 'error' ? 'text-[#DC2626]' : 'text-[#9B9590]'}">
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
            class="px-3 py-1.5 text-xs font-medium transition-all duration-200
              {i === currentPage
                ? 'bg-[#F07030] text-white border-2 border-[#1A1A1A]'
                : 'bg-white border-2 border-[#1A1A1A] text-[#9B9590] hover:bg-[#F7F4F0] hover:text-[#1A1A1A]'}"
          >
            {i + 1}
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>
