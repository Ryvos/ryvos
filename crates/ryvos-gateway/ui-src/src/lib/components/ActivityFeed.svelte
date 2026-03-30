<script>
  import { activityFeed } from '../ws.js';

  let feed = [];
  activityFeed.subscribe(v => feed = v);
</script>

<div class="bg-white border-2 border-[#1A1A1A] p-5">
  <div class="flex items-center justify-between mb-4">
    <h3 class="text-sm font-semibold text-[#1A1A1A]">Activity Feed</h3>
    <span class="text-[0.7rem] px-2 py-0.5 bg-[#FEF3EC] text-[#F07030] border border-[#F07030] font-medium">Live</span>
  </div>

  <div class="max-h-[480px] overflow-y-auto space-y-0">
    {#if feed.length === 0}
      <p class="text-[#9B9590] text-sm text-center py-8">Events from heartbeats, agent runs, and cron jobs will appear here.</p>
    {:else}
      {#each feed as item}
        <div class="flex items-start gap-2.5 py-2 border-b border-[#E8E4E0] last:border-0 text-[0.8rem] hover:bg-[#F7F4F0] -mx-2 px-2 transition-colors duration-150">
          <span class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0
            {item.kind === 'run_error' || item.kind === 'budget_exceeded' ? 'bg-[#DC2626]' :
             item.kind === 'budget_warning' ? 'bg-amber-500' : 'bg-[#F07030]'}"></span>
          <span class="text-[#9B9590] min-w-[60px] text-xs font-mono">{item.time}</span>
          <span class="flex-1
            {item.kind === 'run_error' || item.kind === 'budget_exceeded' ? 'text-[#DC2626]' :
             item.kind === 'budget_warning' ? 'text-amber-600' : 'text-[#6B6560]'}">
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
