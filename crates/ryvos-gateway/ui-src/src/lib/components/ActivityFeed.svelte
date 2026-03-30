<script>
  import { activityFeed } from '../ws.js';

  let feed = [];
  activityFeed.subscribe(v => feed = v);
</script>

<div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-5">
  <div class="flex items-center justify-between mb-4">
    <h3 class="text-sm font-semibold text-[#E8E4E0]">Activity Feed</h3>
    <span class="text-[0.7rem] px-2 py-0.5 rounded-full bg-[#F07030]/10 text-[#F07030] font-medium">Live</span>
  </div>

  <div class="max-h-[480px] overflow-y-auto space-y-0">
    {#if feed.length === 0}
      <p class="text-[#A09890] text-sm text-center py-8">Events from heartbeats, agent runs, and cron jobs will appear here.</p>
    {:else}
      {#each feed as item}
        <div class="flex items-start gap-2.5 py-2 border-b border-[rgba(255,255,255,0.04)] last:border-0 text-[0.8rem] hover:bg-[#2A2A2A] -mx-2 px-2 rounded transition-colors duration-150">
          <span class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0
            {item.kind === 'run_error' || item.kind === 'budget_exceeded' ? 'bg-red-400' :
             item.kind === 'budget_warning' ? 'bg-amber-400' : 'bg-[#F07030]'}"></span>
          <span class="text-[#A09890] min-w-[60px] text-xs font-mono">{item.time}</span>
          <span class="flex-1
            {item.kind === 'run_error' || item.kind === 'budget_exceeded' ? 'text-red-400' :
             item.kind === 'budget_warning' ? 'text-amber-400' : 'text-[#A09890]'}">
            {item.detail}
          </span>
          {#if item.session}
            <span class="text-[#F07030]/70 font-mono text-[0.7rem]">{item.session}</span>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>
