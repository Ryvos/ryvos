<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let healthData = null;
  let metricsData = null;
  let loading = true;

  onMount(async () => {
    try {
      const [health, metrics] = await Promise.all([
        apiFetch('/api/health').catch(() => null),
        apiFetch('/api/metrics').catch(() => null),
      ]);
      healthData = health;
      metricsData = metrics;
    } finally {
      loading = false;
    }
  });

  $: budgetPct = metricsData && metricsData.monthly_budget_cents > 0
    ? Math.min(metricsData.budget_utilization_pct, 100) : 0;
  $: budgetColor = budgetPct > 90 ? 'text-red-400' : budgetPct > 70 ? 'text-amber-400' : 'text-indigo-400';
  $: barColor = budgetPct > 90 ? 'bg-red-400' : budgetPct > 70 ? 'bg-amber-400' : 'bg-indigo-400';
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-gray-100">Settings</h2>
    <p class="text-gray-500 text-sm mt-1">System info and budget configuration</p>
  </div>

  {#if loading}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      {#each Array(2) as _}
        <div class="bg-gray-900 border border-gray-800 rounded-xl p-6 min-h-[160px] animate-pulse"></div>
      {/each}
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      <!-- System card -->
      <div class="bg-gray-900 border border-gray-800 rounded-xl p-6">
        <h3 class="text-sm font-semibold text-gray-100 mb-5">System</h3>
        {#if healthData}
          <div class="space-y-4">
            <div>
              <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">Version</span>
              <p class="text-lg font-bold text-gray-100 mt-1">{healthData.version || 'unknown'}</p>
            </div>
            <div>
              <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">Status</span>
              <p class="text-lg font-bold text-emerald-400 mt-1">{healthData.status || 'unknown'}</p>
            </div>
          </div>
        {:else}
          <p class="text-gray-500 text-sm">Failed to load system info</p>
        {/if}
      </div>

      <!-- Budget card -->
      <div class="bg-gray-900 border border-gray-800 rounded-xl p-6">
        <h3 class="text-sm font-semibold text-gray-100 mb-5">Budget</h3>
        {#if metricsData && metricsData.monthly_budget_cents > 0}
          <div class="space-y-4">
            <div>
              <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">Monthly Budget</span>
              <p class="text-lg font-bold text-gray-100 mt-1">${(metricsData.monthly_budget_cents / 100).toFixed(2)}</p>
            </div>
            <div>
              <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">Spent</span>
              <p class="text-lg font-bold text-gray-100 mt-1">${(metricsData.total_cost_cents / 100).toFixed(2)}</p>
            </div>
            <div>
              <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">Utilization</span>
              <p class="text-lg font-bold {budgetColor} mt-1">{metricsData.budget_utilization_pct}%</p>
            </div>
            <!-- Progress bar -->
            <div class="h-2 bg-gray-950 rounded-full overflow-hidden">
              <div class="{barColor} h-full rounded-full transition-all duration-500" style="width: {budgetPct}%"></div>
            </div>
          </div>
        {:else}
          <p class="text-gray-500 text-sm py-4">
            No budget configured. Add <code class="font-mono bg-gray-950 px-1.5 py-0.5 rounded text-xs">[budget]</code> to your config.toml.
          </p>
        {/if}
      </div>
    </div>
  {/if}
</div>
