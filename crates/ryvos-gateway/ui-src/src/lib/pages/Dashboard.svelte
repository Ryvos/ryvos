<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';
  import { activityFeed } from '../ws.js';
  import ActivityFeed from '../components/ActivityFeed.svelte';

  let loading = true;
  let error = '';

  // Data stores
  let uptimeSecs = 0;
  let activeSessions = 0;
  let totalEntries = 0;
  let toolBreakdown = [];
  let heartbeatSessions = 0;
  let vikingEntries = 0;
  let channelCount = 0;
  let version = '';
  let status = '';

  function formatUptime(secs) {
    if (!secs && secs !== 0) return '-';
    const d = Math.floor(secs / 86400);
    const h = Math.floor((secs % 86400) / 3600);
    const m = Math.floor((secs % 3600) / 60);
    if (d > 0) return `${d}d ${h}h`;
    return `${h}h ${m}m`;
  }

  function formatNumber(n) {
    if (n >= 1000) return (n / 1000).toFixed(1) + 'k';
    return String(n);
  }

  let guardianAlerts = [];
  activityFeed.subscribe(feed => {
    guardianAlerts = feed.filter(item =>
      item.kind === 'guardian_stall' ||
      item.kind === 'guardian_doom_loop' ||
      item.kind === 'guardian_budget_alert'
    ).slice(0, 5);
  });

  $: totalToolCalls = toolBreakdown.reduce((sum, t) => sum + (t.count || 0), 0);
  $: maxToolCount = toolBreakdown.length > 0 ? toolBreakdown[0].count : 1;

  onMount(async () => {
    try {
      const [metrics, auditStats, channels, health] = await Promise.all([
        apiFetch('/api/metrics').catch(() => null),
        apiFetch('/api/audit/stats').catch(() => null),
        apiFetch('/api/channels').catch(() => []),
        apiFetch('/api/health').catch(() => null),
      ]);

      if (metrics) {
        uptimeSecs = metrics.uptime_secs || 0;
        activeSessions = metrics.active_sessions || 0;
      }

      if (auditStats) {
        totalEntries = auditStats.total_entries || 0;
        toolBreakdown = auditStats.tool_breakdown || [];
        heartbeatSessions = auditStats.heartbeat_sessions || 0;
        vikingEntries = auditStats.viking_entries || 0;
      }

      if (Array.isArray(channels)) {
        channelCount = channels.length;
      } else if (channels && typeof channels === 'object') {
        channelCount = Object.keys(channels).length;
      }

      if (health) {
        version = health.version || 'unknown';
        status = health.status || 'unknown';
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });
</script>

<div class="space-y-6">
  <!-- Header -->
  <div>
    <h1 class="text-3xl font-heading">Dashboard</h1>
    <p class="text-sm text-[#9B9590] mt-1">Overview of your Ryvos instance</p>
  </div>

  <!-- Guardian Alerts -->
  {#if guardianAlerts.length > 0}
    <div class="border-2 border-[#DC2626] bg-[#DC2626]/5 p-4 mb-6">
      <div class="flex items-center gap-2 mb-2">
        <svg class="w-5 h-5 text-[#DC2626]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        <span class="label text-[#DC2626]">GUARDIAN ALERTS</span>
      </div>
      {#each guardianAlerts as alert}
        <div class="flex items-center gap-2 text-sm text-[#DC2626] py-1">
          <span class="text-xs font-mono text-[#9B9590]">{alert.time}</span>
          <span>{alert.detail}</span>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Top Metrics Strip -->
  {#if loading}
    <div class="border-2 border-[#1A1A1A] grid grid-cols-2 sm:grid-cols-5 divide-x-2 divide-[#1A1A1A]">
      {#each Array(5) as _}
        <div class="p-4 sm:p-5 animate-pulse">
          <div class="h-3 bg-[#E8E4E0] w-20 mb-3"></div>
          <div class="h-7 bg-[#E8E4E0] w-14"></div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="border-2 border-[#1A1A1A] bg-white grid grid-cols-2 sm:grid-cols-5 divide-x-2 divide-[#1A1A1A] divide-y-2 sm:divide-y-0">
      <!-- Heartbeats -->
      <div class="p-4 sm:p-5">
        <div class="flex items-center gap-2 text-[#9B9590] mb-1">
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>
          </svg>
          <span class="text-xs uppercase tracking-wider font-medium">Heartbeats</span>
        </div>
        <p class="text-2xl font-heading">{formatNumber(heartbeatSessions)}</p>
      </div>

      <!-- Sessions -->
      <div class="p-4 sm:p-5">
        <div class="flex items-center gap-2 text-[#9B9590] mb-1">
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/>
          </svg>
          <span class="text-xs uppercase tracking-wider font-medium">Sessions</span>
        </div>
        <p class="text-2xl font-heading">{activeSessions}</p>
      </div>

      <!-- Tool Calls -->
      <div class="p-4 sm:p-5">
        <div class="flex items-center gap-2 text-[#9B9590] mb-1">
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M14.7 6.3a1 1 0 000 1.4l1.6 1.6a1 1 0 001.4 0l3.77-3.77a6 6 0 01-7.94 7.94l-6.91 6.91a2.12 2.12 0 01-3-3l6.91-6.91a6 6 0 017.94-7.94l-3.76 3.76z"/>
          </svg>
          <span class="text-xs uppercase tracking-wider font-medium">Tool Calls</span>
        </div>
        <p class="text-2xl font-heading">{formatNumber(totalToolCalls)}</p>
      </div>

      <!-- Viking Memories -->
      <div class="p-4 sm:p-5">
        <div class="flex items-center gap-2 text-[#9B9590] mb-1">
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 19.5A2.5 2.5 0 016.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/>
          </svg>
          <span class="text-xs uppercase tracking-wider font-medium">Viking Memories</span>
        </div>
        <p class="text-2xl font-heading">{formatNumber(vikingEntries)}</p>
      </div>

      <!-- Uptime -->
      <div class="p-4 sm:p-5">
        <div class="flex items-center gap-2 text-[#9B9590] mb-1">
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>
          </svg>
          <span class="text-xs uppercase tracking-wider font-medium">Uptime</span>
        </div>
        <p class="text-2xl font-heading">{formatUptime(uptimeSecs)}</p>
      </div>
    </div>
  {/if}

  <!-- Tool Usage -->
  {#if !loading && toolBreakdown.length > 0}
    <div class="border-2 border-[#1A1A1A] bg-white p-5">
      <h2 class="label mb-4">Tool Usage</h2>
      {#each toolBreakdown.slice(0, 8) as item}
        <div class="flex items-center gap-3 mb-2">
          <span class="text-xs font-mono w-32 text-[#6B6560] truncate">{item.tool}</span>
          <div class="flex-1 bg-[#F7F4F0] h-5 border border-[#E8E4E0]">
            <div class="bg-[#F07030] h-full transition-all duration-500" style="width: {(item.count / maxToolCount * 100)}%"></div>
          </div>
          <span class="text-xs font-mono text-[#9B9590] w-12 text-right">{item.count}</span>
        </div>
      {/each}
    </div>
  {:else if !loading}
    <div class="border-2 border-[#1A1A1A] bg-white p-5">
      <h2 class="label mb-4">Tool Usage</h2>
      <p class="text-sm text-[#9B9590] py-4 text-center">No tool calls recorded yet. Tools will appear here as agents use them.</p>
    </div>
  {/if}

  <!-- Activity Feed + System -->
  <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
    <ActivityFeed />

    <div class="border-2 border-[#1A1A1A] bg-white p-5">
      <h2 class="label mb-4">System</h2>
      {#if loading}
        <div class="space-y-4 animate-pulse">
          <div>
            <div class="h-3 bg-[#E8E4E0] w-16 mb-2"></div>
            <div class="h-6 bg-[#E8E4E0] w-24"></div>
          </div>
          <div>
            <div class="h-3 bg-[#E8E4E0] w-16 mb-2"></div>
            <div class="h-6 bg-[#E8E4E0] w-20"></div>
          </div>
        </div>
      {:else}
        <div class="space-y-4">
          <div>
            <span class="label">Version</span>
            <p class="text-lg font-heading mt-0.5">{version || 'unknown'}</p>
          </div>
          <div>
            <span class="label">Status</span>
            <div class="flex items-center gap-2 mt-0.5">
              {#if status === 'ok' || status === 'healthy'}
                <span class="w-2 h-2 bg-[#16A34A] border border-[#1A1A1A] inline-block animate-pulse-glow"></span>
                <p class="text-lg font-heading text-[#16A34A]">{status}</p>
              {:else}
                <p class="text-lg font-heading">{status || 'unknown'}</p>
              {/if}
            </div>
          </div>
          <div>
            <span class="label">Channels Active</span>
            <p class="text-lg font-heading mt-0.5">{channelCount}</p>
          </div>
          <div>
            <span class="label">Audit Entries</span>
            <p class="text-lg font-heading mt-0.5">{formatNumber(totalEntries)}</p>
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Quick Actions -->
  <div>
    <h2 class="label mb-3">Quick Actions</h2>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <a href="#/chat" class="flex items-center gap-3 border-l-4 border-l-[#F07030] pl-4 py-3 text-sm text-[#6B6560] hover:bg-[#F7F4F0] transition-colors">
        <svg class="w-4 h-4 text-[#F07030]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/>
        </svg>
        Chat with agent
        <span class="ml-auto text-[#9B9590]">&rarr;</span>
      </a>
      <a href="#/memory" class="flex items-center gap-3 border-l-4 border-l-[#F07030] pl-4 py-3 text-sm text-[#6B6560] hover:bg-[#F7F4F0] transition-colors">
        <svg class="w-4 h-4 text-[#F07030]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 19.5A2.5 2.5 0 016.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/>
        </svg>
        Browse memory
        <span class="ml-auto text-[#9B9590]">&rarr;</span>
      </a>
      <a href="#/audit" class="flex items-center gap-3 border-l-4 border-l-[#F07030] pl-4 py-3 text-sm text-[#6B6560] hover:bg-[#F7F4F0] transition-colors">
        <svg class="w-4 h-4 text-[#F07030]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
        </svg>
        View audit trail
        <span class="ml-auto text-[#9B9590]">&rarr;</span>
      </a>
      <a href="#/channels" class="flex items-center gap-3 border-l-4 border-l-[#F07030] pl-4 py-3 text-sm text-[#6B6560] hover:bg-[#F7F4F0] transition-colors">
        <svg class="w-4 h-4 text-[#F07030]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2" y="7" width="20" height="14" rx="2" ry="2"/><path d="M16 21V5a2 2 0 00-2-2h-4a2 2 0 00-2 2v16"/>
        </svg>
        Check channels
        <span class="ml-auto text-[#9B9590]">&rarr;</span>
      </a>
    </div>
  </div>

  <!-- Error display -->
  {#if error}
    <div class="border-2 border-[#DC2626] bg-white p-4">
      <p class="text-sm text-[#DC2626]">Failed to load some data: {error}</p>
    </div>
  {/if}
</div>
