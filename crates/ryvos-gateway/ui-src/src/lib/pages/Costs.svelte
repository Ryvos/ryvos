<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';
  import MetricCard from '../components/MetricCard.svelte';

  let summary = null;
  let breakdown = [];
  let loading = true;
  let error = '';

  let now = new Date();
  let thirtyDaysAgo = new Date(now.getTime() - 30 * 86400000);
  let fromDate = thirtyDaysAgo.toISOString().split('T')[0];
  let toDate = now.toISOString().split('T')[0];
  let groupBy = 'model';

  async function loadCosts() {
    loading = true;
    error = '';
    try {
      const from = fromDate + 'T00:00:00Z';
      const to = toDate + 'T23:59:59Z';
      const data = await apiFetch(`/api/costs?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}&group_by=${groupBy}`);
      summary = data.summary || null;
      breakdown = data.breakdown || [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  onMount(() => loadCosts());

  function capitalize(s) {
    return s.charAt(0).toUpperCase() + s.slice(1);
  }
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-[#E8E4E0]">Cost Analysis</h2>
    <p class="text-[#A09890] text-sm mt-1">Token usage and spending breakdown</p>
  </div>

  <!-- Controls -->
  <div class="flex items-center gap-4 flex-wrap mb-6 p-4 bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl">
    <label class="flex items-center gap-2 text-xs text-[#A09890] font-medium">
      From
      <input type="date" bind:value={fromDate}
        class="px-2.5 py-1.5 bg-[#0F0F0F] border border-[rgba(255,255,255,0.08)] rounded-md text-[#E8E4E0] text-xs outline-none
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200" />
    </label>
    <label class="flex items-center gap-2 text-xs text-[#A09890] font-medium">
      To
      <input type="date" bind:value={toDate}
        class="px-2.5 py-1.5 bg-[#0F0F0F] border border-[rgba(255,255,255,0.08)] rounded-md text-[#E8E4E0] text-xs outline-none
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200" />
    </label>
    <label class="flex items-center gap-2 text-xs text-[#A09890] font-medium">
      Group by
      <select bind:value={groupBy}
        class="px-2.5 py-1.5 bg-[#0F0F0F] border border-[rgba(255,255,255,0.08)] rounded-md text-[#E8E4E0] text-xs outline-none
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200">
        <option value="model">Model</option>
        <option value="provider">Provider</option>
        <option value="day">Day</option>
      </select>
    </label>
    <button on:click={loadCosts}
      class="px-4 py-1.5 bg-gradient-to-br from-[#F07030] to-[#E06020] text-white rounded-md
             text-xs font-semibold transition-all duration-200 hover:shadow-lg hover:shadow-[#F07030]/30 hover:-translate-y-0.5">
      Refresh
    </button>
  </div>

  {#if loading}
    <div class="text-center py-8">
      <p class="text-[#A09890] text-sm animate-pulse">Loading cost data...</p>
    </div>
  {:else if error}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-12 text-center">
      <p class="text-[#A09890] text-sm">Cost tracking not configured</p>
    </div>
  {:else}
    <!-- Summary cards -->
    {#if summary}
      <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
        <MetricCard label="Total Cost" value={'$' + ((summary.total_cost_cents || 0) / 100).toFixed(2)} type="spend" />
        <MetricCard label="Input Tokens" value={(summary.total_input_tokens || 0).toLocaleString()} type="runs" />
        <MetricCard label="Output Tokens" value={(summary.total_output_tokens || 0).toLocaleString()} type="sessions" />
        <MetricCard label="Events" value={(summary.total_events || 0).toLocaleString()} type="uptime" />
      </div>
    {/if}

    <!-- Breakdown table -->
    {#if breakdown.length === 0}
      <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-8 text-center">
        <p class="text-[#A09890] text-sm">No cost data for this period</p>
      </div>
    {:else}
      <div class="border border-[rgba(255,255,255,0.08)] rounded-xl overflow-hidden">
        <table class="w-full text-sm">
          <thead>
            <tr>
              <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">
                {capitalize(groupBy)}
              </th>
              <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">Cost</th>
              <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">Input Tokens</th>
              <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">Output Tokens</th>
            </tr>
          </thead>
          <tbody>
            {#each breakdown as row}
              <tr class="hover:bg-[#2A2A2A]/40 transition-colors duration-150">
                <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] font-medium text-[#E8E4E0]">{row.key}</td>
                <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] font-mono text-[#E8E4E0]">${((row.cost_cents || 0) / 100).toFixed(3)}</td>
                <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-[#A09890]">{(row.input_tokens || 0).toLocaleString()}</td>
                <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-[#A09890]">{(row.output_tokens || 0).toLocaleString()}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
</div>
