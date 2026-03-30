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
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Sessions</h2>
    <p class="text-[#9B9590] text-sm mt-1">Active conversation sessions</p>
  </div>

  {#if loading}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading sessions...</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm">Failed to load sessions</p>
    </div>
  {:else if sessionList.length === 0}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">No active sessions</p>
    </div>
  {:else}
    <div class="border-2 border-[#1A1A1A] overflow-hidden">
      <table class="w-full text-sm">
        <thead>
          <tr>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">
              Session ID
            </th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">
              Channel
            </th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">
              Last Active
            </th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">
              Runs
            </th>
            <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A]">
              Actions
            </th>
          </tr>
        </thead>
        <tbody>
          {#each sessionList as session}
            <tr class="hover:bg-[#F7F4F0] transition-colors duration-150">
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-xs text-[#1A1A1A]">
                {truncate(getSid(session), 40)}
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-xs text-[#9B9590]">
                {session.channel || '-'}
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-xs text-[#9B9590]">
                {formatTime(session.last_active)}
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-xs text-[#9B9590]">
                {session.total_runs ?? '-'}
              </td>
              <td class="px-4 py-3 border-b border-[#E8E4E0]">
                <a
                  href="#/chat/{encodeURIComponent(getSid(session))}"
                  class="inline-flex items-center gap-1 px-3 py-1.5 bg-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                         text-xs text-[#1A1A1A] font-medium hover:text-[#F07030]
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
