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

  function truncate(s, max) {
    return s.length > max ? s.substring(0, max) + '...' : s;
  }
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-gray-100">Sessions</h2>
    <p class="text-gray-500 text-sm mt-1">Active conversation sessions</p>
  </div>

  {#if loading}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-8 text-center">
      <p class="text-gray-500 text-sm animate-pulse">Loading sessions...</p>
    </div>
  {:else if error}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-8 text-center">
      <p class="text-gray-500 text-sm">Failed to load sessions</p>
    </div>
  {:else if sessionList.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">No active sessions</p>
    </div>
  {:else}
    <div class="border border-gray-800 rounded-xl overflow-hidden">
      <table class="w-full text-sm">
        <thead>
          <tr>
            <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800">
              Session ID
            </th>
            <th class="px-4 py-3 bg-gray-900/80 text-left text-[0.7rem] font-semibold text-gray-500 uppercase tracking-wider border-b border-gray-800">
              Actions
            </th>
          </tr>
        </thead>
        <tbody>
          {#each sessionList as session}
            <tr class="hover:bg-gray-800/40 transition-colors duration-150">
              <td class="px-4 py-3 border-b border-gray-800/50 font-mono text-xs text-gray-300">
                {truncate(session, 50)}
              </td>
              <td class="px-4 py-3 border-b border-gray-800/50">
                <a
                  href="#/chat/{encodeURIComponent(session)}"
                  class="inline-flex items-center gap-1 px-3 py-1.5 bg-gray-800 border border-gray-700 rounded-md
                         text-xs text-gray-300 font-medium hover:bg-gray-700 hover:text-indigo-400
                         hover:border-indigo-400 transition-all duration-200"
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
