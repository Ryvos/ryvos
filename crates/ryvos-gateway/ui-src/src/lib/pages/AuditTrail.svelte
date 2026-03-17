<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let entries = [];
  let loading = true;
  let error = '';
  let filterTool = '';

  async function loadAudit() {
    loading = true;
    error = '';
    try {
      let url = '/api/audit?limit=50';
      if (filterTool) url += `&tool=${encodeURIComponent(filterTool)}`;
      const data = await apiFetch(url);
      entries = data.entries || data.events || data.audit || [];
    } catch (e) {
      error = e.message;
      entries = [];
    } finally {
      loading = false;
    }
  }

  onMount(() => loadAudit());

  function truncate(s, max) {
    if (!s) return '';
    if (typeof s === 'object') s = JSON.stringify(s);
    return s.length > max ? s.substring(0, max) + '...' : s;
  }

  function formatTime(isoStr) {
    if (!isoStr) return '-';
    return new Date(isoStr).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function handleFilter() {
    loadAudit();
  }
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-gray-100">Audit Trail</h2>
    <p class="text-gray-500 text-sm mt-1">Tool execution audit log</p>
  </div>

  <!-- Filter bar -->
  <div class="flex items-center gap-3 mb-6 p-4 bg-gray-900 border border-gray-800 rounded-xl">
    <label class="flex items-center gap-2 text-xs text-gray-500 font-medium">
      Filter by tool
      <input
        type="text"
        bind:value={filterTool}
        placeholder="e.g. bash, read_file"
        class="px-2.5 py-1.5 bg-gray-950 border border-gray-800 rounded-md text-gray-200 text-xs outline-none w-48
               focus:border-indigo-400 focus:ring-2 focus:ring-indigo-400/20 transition-all duration-200
               placeholder:text-gray-600"
      />
    </label>
    <button on:click={handleFilter}
      class="px-4 py-1.5 bg-gradient-to-br from-indigo-400 to-indigo-600 text-white rounded-md
             text-xs font-semibold transition-all duration-200 hover:shadow-lg hover:shadow-indigo-500/30">
      Apply
    </button>
    {#if filterTool}
      <button on:click={() => { filterTool = ''; loadAudit(); }}
        class="px-3 py-1.5 bg-gray-800 border border-gray-700 text-gray-400 rounded-md text-xs hover:text-gray-200 transition-colors">
        Clear
      </button>
    {/if}
  </div>

  {#if loading}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-8 text-center">
      <p class="text-gray-500 text-sm animate-pulse">Loading audit entries...</p>
    </div>
  {:else if error}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">Audit trail not available</p>
    </div>
  {:else if entries.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">No audit entries found</p>
    </div>
  {:else}
    <div class="border border-gray-800 rounded-xl overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr>
            <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800">Time</th>
            <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800">Tool</th>
            <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800">Input</th>
            <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800">Outcome</th>
          </tr>
        </thead>
        <tbody>
          {#each entries as entry}
            <tr class="hover:bg-gray-800/40 transition-colors duration-150">
              <td class="px-4 py-3 border-b border-gray-800/50 font-mono text-xs text-gray-400">
                {formatTime(entry.timestamp || entry.time)}
              </td>
              <td class="px-4 py-3 border-b border-gray-800/50">
                <span class="inline-flex px-2.5 py-0.5 rounded-full text-[0.7rem] font-semibold bg-indigo-400/10 text-indigo-400">
                  {entry.tool || entry.tool_name || '-'}
                </span>
              </td>
              <td class="px-4 py-3 border-b border-gray-800/50 text-gray-400 font-mono text-xs max-w-[300px] truncate">
                {truncate(entry.input || entry.input_summary || '', 80)}
              </td>
              <td class="px-4 py-3 border-b border-gray-800/50">
                <span class="font-medium
                  {entry.outcome === 'success' || entry.outcome === 'ok' ? 'text-emerald-400' :
                   entry.outcome === 'error' || entry.outcome === 'blocked' ? 'text-red-400' :
                   'text-gray-400'}">
                  {entry.outcome || entry.status || '-'}
                </span>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
