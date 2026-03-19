<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';
  import MetricCard from '../components/MetricCard.svelte';
  import ActivityFeed from '../components/ActivityFeed.svelte';

  let metricsData = null;
  let healthData = null;
  let loading = true;
  let error = '';

  function formatDuration(secs) {
    if (!secs && secs !== 0) return '-';
    if (secs < 60) return secs + 's';
    if (secs < 3600) return Math.floor(secs / 60) + 'm';
    if (secs < 86400) return Math.floor(secs / 3600) + 'h ' + Math.floor((secs % 3600) / 60) + 'm';
    return Math.floor(secs / 86400) + 'd ' + Math.floor((secs % 86400) / 3600) + 'h';
  }

  $: allZero = metricsData && (metricsData.total_runs || 0) === 0
    && (metricsData.active_sessions || 0) === 0
    && (metricsData.total_cost_cents || 0) === 0;

  onMount(async () => {
    try {
      const [metrics, health] = await Promise.all([
        apiFetch('/api/metrics').catch(() => null),
        apiFetch('/api/health').catch(() => null),
      ]);
      metricsData = metrics;
      healthData = health;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-[#E8E4E0]">Dashboard</h2>
    <p class="text-[#A09890] text-sm mt-1">Overview of your Ryvos instance</p>
  </div>

  <!-- Metric Cards -->
  {#if loading}
    <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3 mb-7">
      {#each Array(5) as _}
        <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-5 min-h-[100px] animate-pulse">
          <div class="h-4 bg-[#2A2A2A] rounded w-20 mt-8"></div>
        </div>
      {/each}
    </div>
  {:else if metricsData}
    <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3 mb-7">
      <MetricCard label="Runs" value={metricsData.total_runs ?? 0} type="runs" />
      <MetricCard label="Sessions" value={metricsData.active_sessions ?? 0} type="sessions" />
      <MetricCard label="Spend" value={'$' + ((metricsData.total_cost_cents || 0) / 100).toFixed(2)} type="spend" />
      <MetricCard
        label="Budget"
        value={metricsData.monthly_budget_cents > 0 ? metricsData.budget_utilization_pct + '%' : 'Unlimited'}
        type="budget"
      />
      <MetricCard label="Uptime" value={formatDuration(metricsData.uptime_secs)} type="uptime" />
    </div>
    {#if allZero}
      <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-4 mb-7">
        <p class="text-[#A09890] text-sm">
          All metrics are showing 0. To enable budget tracking, add a <code class="font-mono bg-[#0F0F0F] px-1.5 py-0.5 rounded text-xs">[budget]</code> section to your config.toml. Runs and sessions will populate as agents are used.
        </p>
      </div>
    {/if}
  {:else}
    <p class="text-[#A09890] mb-7">Failed to load metrics</p>
  {/if}

  <!-- Activity Feed -->
  <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
    <ActivityFeed />
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-5">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-sm font-semibold text-[#E8E4E0]">System</h3>
      </div>
      {#if healthData}
        <div class="space-y-3">
          <div>
            <span class="text-xs font-medium text-[#A09890] uppercase tracking-wider">Version</span>
            <p class="text-lg font-semibold text-[#E8E4E0] mt-0.5">{healthData.version || 'unknown'}</p>
          </div>
          <div>
            <span class="text-xs font-medium text-[#A09890] uppercase tracking-wider">Status</span>
            <p class="text-lg font-semibold text-emerald-400 mt-0.5">{healthData.status || 'unknown'}</p>
          </div>
        </div>
      {:else}
        <p class="text-[#A09890] text-sm text-center py-8">System info unavailable</p>
      {/if}
    </div>
  </div>
</div>
