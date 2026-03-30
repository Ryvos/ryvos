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
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Audit Trail</h2>
    <p class="text-[#9B9590] text-sm mt-1">Tool execution audit log</p>
  </div>

  <!-- Filter bar -->
  <div class="flex items-center gap-3 mb-6 p-4 bg-white border-2 border-[#1A1A1A]">
    <label class="flex items-center gap-2 text-xs text-[#6B6560] font-medium">
      Filter by tool
      <input
        type="text"
        bind:value={filterTool}
        placeholder="e.g. bash, read_file"
        class="px-2.5 py-1.5 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] text-xs outline-none w-48
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200
               placeholder:text-[#9B9590]"
      />
    </label>
    <button on:click={handleFilter}
      class="px-4 py-1.5 bg-[#F07030] text-white border-2 border-[#1A1A1A]
             text-xs font-semibold transition-all duration-200 shadow-brutal-sm brutal-shift">
      Apply
    </button>
    {#if filterTool}
      <button on:click={() => { filterTool = ''; loadAudit(); }}
        class="px-3 py-1.5 bg-white border-2 border-[#1A1A1A] text-[#6B6560] text-xs hover:text-[#1A1A1A] hover:bg-[#F7F4F0] transition-colors">
        Clear
      </button>
    {/if}
  </div>

  {#if loading}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading audit entries...</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">Audit trail not available</p>
    </div>
  {:else if entries.length === 0}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">No audit entries found</p>
    </div>
  {:else}
    <div class="border-2 border-[#1A1A1A] overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Time</th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Tool</th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Input</th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Outcome</th>
          </tr>
        </thead>
        <tbody>
          {#each entries as entry}
            <tr class="hover:bg-[#F7F4F0] transition-colors duration-150">
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-xs text-[#9B9590]">
                {formatTime(entry.timestamp || entry.time)}
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0]">
                <span class="inline-flex px-2.5 py-0.5 text-[0.7rem] font-semibold bg-[#FEF3EC] text-[#F07030] border border-[#F07030]">
                  {entry.tool || entry.tool_name || '-'}
                </span>
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#6B6560] font-mono text-xs max-w-[300px] truncate">
                {truncate(entry.input || entry.input_summary || '', 80)}
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0]">
                <span class="font-medium
                  {entry.outcome === 'success' || entry.outcome === 'ok' ? 'text-[#16A34A]' :
                   entry.outcome === 'error' || entry.outcome === 'blocked' ? 'text-[#DC2626]' :
                   'text-[#9B9590]'}">
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
