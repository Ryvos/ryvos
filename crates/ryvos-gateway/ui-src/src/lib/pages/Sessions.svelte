<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let sessionList = [];
  let loading = true;
  let error = '';

  onMount(async () => {
    try {
      const data = await apiFetch('/api/sessions');
      sessionList = data.sessions || [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });

  function getSid(session) {
    return typeof session === 'string' ? session : (session.id || String(session));
  }

  function truncate(s, max) {
    if (!s || typeof s !== 'string') return String(s || '');
    return s.length > max ? s.substring(0, max) + '...' : s;
  }

  function formatTime(ts) {
    if (!ts) return '-';
    try {
      const d = new Date(ts);
      return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    } catch { return '-'; }
  }
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-[#E8E4E0]">Sessions</h2>
    <p class="text-[#A09890] text-sm mt-1">Active conversation sessions</p>
  </div>

  {#if loading}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-8 text-center">
      <p class="text-[#A09890] text-sm animate-pulse">Loading sessions...</p>
    </div>
  {:else if error}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-8 text-center">
      <p class="text-[#A09890] text-sm">Failed to load sessions</p>
    </div>
  {:else if sessionList.length === 0}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-12 text-center">
      <p class="text-[#A09890] text-sm">No active sessions</p>
    </div>
  {:else}
    <div class="border border-[rgba(255,255,255,0.08)] rounded-xl overflow-hidden">
      <table class="w-full text-sm">
        <thead>
          <tr>
            <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">
              Session ID
            </th>
            <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">
              Channel
            </th>
            <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">
              Last Active
            </th>
            <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">
              Runs
            </th>
            <th class="px-4 py-3 bg-[#222222]/80 text-left text-[0.7rem] font-semibold text-[#A09890] uppercase tracking-wider border-b border-[rgba(255,255,255,0.08)]">
              Actions
            </th>
          </tr>
        </thead>
        <tbody>
          {#each sessionList as session}
            <tr class="hover:bg-[#2A2A2A]/40 transition-colors duration-150">
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] font-mono text-xs text-[#E8E4E0]">
                {truncate(getSid(session), 40)}
              </td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-xs text-[#A09890]">
                {session.channel || '-'}
              </td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-xs text-[#A09890]">
                {formatTime(session.last_active)}
              </td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)] text-xs text-[#A09890]">
                {session.total_runs ?? '-'}
              </td>
              <td class="px-4 py-3 border-b border-[rgba(255,255,255,0.04)]">
                <a
                  href="#/chat/{encodeURIComponent(getSid(session))}"
                  class="inline-flex items-center gap-1 px-3 py-1.5 bg-[#2A2A2A] border border-[rgba(255,255,255,0.12)] rounded-md
                         text-xs text-[#E8E4E0] font-medium hover:bg-[#2A2A2A] hover:text-[#F07030]
                         hover:border-[#F07030] transition-all duration-200"
                >
                  Open Chat
                </a>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
