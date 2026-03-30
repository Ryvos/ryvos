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
  $: budgetColor = budgetPct > 90 ? 'text-[#DC2626]' : budgetPct > 70 ? 'text-amber-500' : 'text-[#F07030]';
  $: barColor = budgetPct > 90 ? 'bg-[#DC2626]' : budgetPct > 70 ? 'bg-amber-500' : 'bg-[#F07030]';
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Settings</h2>
    <p class="text-[#9B9590] text-sm mt-1">System info and budget configuration</p>
  </div>

  {#if loading}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      {#each Array(2) as _}
        <div class="bg-white border-2 border-[#1A1A1A] p-6 min-h-[160px] animate-pulse"></div>
      {/each}
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      <!-- System card -->
      <div class="bg-white border-2 border-[#1A1A1A] p-6">
        <h3 class="text-sm font-semibold text-[#1A1A1A] mb-5">System</h3>
        {#if healthData}
          <div class="space-y-4">
            <div>
              <span class="text-xs uppercase tracking-wider font-bold text-[#9B9590]">Version</span>
              <p class="text-lg font-bold text-[#1A1A1A] mt-1">{healthData.version || 'unknown'}</p>
            </div>
            <div>
              <span class="text-xs uppercase tracking-wider font-bold text-[#9B9590]">Status</span>
              <p class="text-lg font-bold text-[#16A34A] mt-1">{healthData.status || 'unknown'}</p>
            </div>
          </div>
        {:else}
          <p class="text-[#9B9590] text-sm">Failed to load system info</p>
        {/if}
      </div>

      <!-- Budget card -->
      <div class="bg-white border-2 border-[#1A1A1A] p-6">
        <h3 class="text-sm font-semibold text-[#1A1A1A] mb-5">Budget</h3>
        {#if metricsData && metricsData.monthly_budget_cents > 0}
          <div class="space-y-4">
            <div>
              <span class="text-xs uppercase tracking-wider font-bold text-[#9B9590]">Monthly Budget</span>
              <p class="text-lg font-bold text-[#1A1A1A] mt-1">${(metricsData.monthly_budget_cents / 100).toFixed(2)}</p>
            </div>
            <div>
              <span class="text-xs uppercase tracking-wider font-bold text-[#9B9590]">Spent</span>
              <p class="text-lg font-bold text-[#1A1A1A] mt-1">${(metricsData.total_cost_cents / 100).toFixed(2)}</p>
            </div>
            <div>
              <span class="text-xs uppercase tracking-wider font-bold text-[#9B9590]">Utilization</span>
              <p class="text-lg font-bold {budgetColor} mt-1">{metricsData.budget_utilization_pct}%</p>
            </div>
            <!-- Progress bar -->
            <div class="h-2 bg-[#F7F4F0] border border-[#1A1A1A] overflow-hidden">
              <div class="{barColor} h-full transition-all duration-500" style="width: {budgetPct}%"></div>
            </div>
          </div>
        {:else}
          <p class="text-[#9B9590] text-sm py-4">
            No budget configured. Add <code class="font-mono bg-[#F7F4F0] border border-[#E8E4E0] px-1.5 py-0.5 text-xs">[budget]</code> to your config.toml.
          </p>
        {/if}
      </div>
    </div>
  {/if}
</div>
