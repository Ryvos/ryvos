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
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Cost Analysis</h2>
    <p class="text-[#9B9590] text-sm mt-1">Token usage and spending breakdown</p>
  </div>

  <!-- Controls -->
  <div class="flex items-center gap-4 flex-wrap mb-6 p-4 bg-white border-2 border-[#1A1A1A]">
    <label class="flex items-center gap-2 text-xs text-[#6B6560] font-medium">
      From
      <input type="date" bind:value={fromDate}
        class="px-2.5 py-1.5 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] text-xs outline-none
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200" />
    </label>
    <label class="flex items-center gap-2 text-xs text-[#6B6560] font-medium">
      To
      <input type="date" bind:value={toDate}
        class="px-2.5 py-1.5 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] text-xs outline-none
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200" />
    </label>
    <label class="flex items-center gap-2 text-xs text-[#6B6560] font-medium">
      Group by
      <select bind:value={groupBy}
        class="px-2.5 py-1.5 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] text-xs outline-none
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20 transition-all duration-200">
        <option value="model">Model</option>
        <option value="provider">Provider</option>
        <option value="day">Day</option>
      </select>
    </label>
    <button on:click={loadCosts}
      class="px-4 py-1.5 bg-[#F07030] text-white border-2 border-[#1A1A1A]
             text-xs font-semibold transition-all duration-200 shadow-brutal-sm brutal-shift">
      Refresh
    </button>
  </div>

  {#if loading}
    <div class="text-center py-8">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading cost data...</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">Cost tracking not configured</p>
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
      <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
        <p class="text-[#9B9590] text-sm">No cost data for this period</p>
      </div>
    {:else}
      <div class="border-2 border-[#1A1A1A] overflow-hidden">
        <table class="w-full text-sm">
          <thead>
            <tr>
              <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">
                {capitalize(groupBy)}
              </th>
              <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Cost</th>
              <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Input Tokens</th>
              <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">Output Tokens</th>
            </tr>
          </thead>
          <tbody>
            {#each breakdown as row}
              <tr class="hover:bg-[#F7F4F0] transition-colors duration-150">
                <td class="px-4 py-3 border-b border-[#E8E4E0] font-medium text-[#1A1A1A]">{row.key}</td>
                <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-[#1A1A1A]">${((row.cost_cents || 0) / 100).toFixed(3)}</td>
                <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#9B9590]">{(row.input_tokens || 0).toLocaleString()}</td>
                <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#9B9590]">{(row.output_tokens || 0).toLocaleString()}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
</div>
