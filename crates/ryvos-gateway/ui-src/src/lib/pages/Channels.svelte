<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let channels = [];
  let loading = true;
  let error = '';

  onMount(async () => {
    try {
      const data = await apiFetch('/api/channels');
      channels = data.channels || data || [];
      if (!Array.isArray(channels)) {
        channels = Object.entries(channels).map(([name, info]) => ({
          name,
          ...(typeof info === 'object' ? info : { status: info }),
        }));
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-gray-100">Channels</h2>
    <p class="text-gray-500 text-sm mt-1">Integration channel status</p>
  </div>

  {#if loading}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-8 text-center">
      <p class="text-gray-500 text-sm animate-pulse">Loading channels...</p>
    </div>
  {:else if error}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">Channels endpoint not available</p>
    </div>
  {:else if channels.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-12 text-center">
      <p class="text-gray-500 text-sm">No channels configured</p>
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
      {#each channels as channel}
        <div class="bg-gray-900 border border-gray-800 rounded-xl p-5 hover:border-gray-700 transition-all duration-200">
          <div class="flex items-center justify-between mb-3">
            <h3 class="text-sm font-semibold text-gray-100">{channel.name || 'Unknown'}</h3>
            <span class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[0.7rem] font-semibold
              {channel.status === 'connected' || channel.status === 'active' || channel.status === 'ok'
                ? 'bg-emerald-400/10 text-emerald-400'
                : channel.status === 'error'
                  ? 'bg-red-400/10 text-red-400'
                  : 'bg-gray-800 text-gray-400'}">
              <span class="w-1.5 h-1.5 rounded-full
                {channel.status === 'connected' || channel.status === 'active' || channel.status === 'ok'
                  ? 'bg-emerald-400'
                  : channel.status === 'error'
                    ? 'bg-red-400'
                    : 'bg-gray-500'}"></span>
              {channel.status || 'unknown'}
            </span>
          </div>
          {#if channel.type}
            <p class="text-xs text-gray-500">Type: {channel.type}</p>
          {/if}
          {#if channel.description}
            <p class="text-xs text-gray-400 mt-1">{channel.description}</p>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
