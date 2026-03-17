<script>
  import { activityFeed } from '../ws.js';

  let feed = [];
  activityFeed.subscribe(v => feed = v);
</script>

<div class="bg-gray-900 border border-gray-800 rounded-xl p-5">
  <div class="flex items-center justify-between mb-4">
    <h3 class="text-sm font-semibold text-gray-100">Activity Feed</h3>
    <span class="text-[0.7rem] px-2 py-0.5 rounded-full bg-indigo-400/10 text-indigo-400 font-medium">Live</span>
  </div>

  <div class="max-h-80 overflow-y-auto space-y-0">
    {#if feed.length === 0}
      <p class="text-gray-500 text-sm text-center py-8">Waiting for events...</p>
    {:else}
      {#each feed as item}
        <div class="flex items-start gap-2.5 py-2 border-b border-gray-800/50 last:border-0 text-[0.8rem] hover:bg-gray-800/30 -mx-2 px-2 rounded transition-colors duration-150">
          <span class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0
            {item.kind === 'run_error' || item.kind === 'budget_exceeded' ? 'bg-red-400' :
             item.kind === 'budget_warning' ? 'bg-amber-400' : 'bg-indigo-400'}"></span>
          <span class="text-gray-500 min-w-[60px] text-xs font-mono">{item.time}</span>
          <span class="flex-1
            {item.kind === 'run_error' || item.kind === 'budget_exceeded' ? 'text-red-400' :
             item.kind === 'budget_warning' ? 'text-amber-400' : 'text-gray-400'}">
            {item.detail}
          </span>
          {#if item.session}
            <span class="text-indigo-400/70 font-mono text-[0.7rem]">{item.session}</span>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>
