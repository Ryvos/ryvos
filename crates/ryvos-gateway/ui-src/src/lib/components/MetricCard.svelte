<script>
  export let label = '';
  export let value = '';
  export let type = 'default';
</script>

<div class="bg-gray-900 border border-gray-800 rounded-xl p-5 transition-all duration-200
            hover:border-gray-700 hover:-translate-y-0.5 hover:shadow-sm relative overflow-hidden group">
  <!-- Top accent line -->
  <div class="absolute top-0 left-0 right-0 h-0.5 bg-gradient-to-r from-indigo-400 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-200"></div>

  <!-- Icon -->
  <div class="w-9 h-9 rounded-md flex items-center justify-center mb-3
    {type === 'runs' ? 'bg-indigo-400/10 text-indigo-400' :
     type === 'sessions' ? 'bg-emerald-400/10 text-emerald-400' :
     type === 'spend' ? 'bg-amber-400/10 text-amber-400' :
     type === 'budget' ? 'bg-purple-400/10 text-purple-400' :
     type === 'uptime' ? 'bg-emerald-400/10 text-emerald-400' :
     'bg-gray-800 text-gray-400'}">
    {#if type === 'runs'}
      <svg class="w-[18px] h-[18px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>
    {:else if type === 'sessions'}
      <svg class="w-[18px] h-[18px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/></svg>
    {:else if type === 'spend'}
      <svg class="w-[18px] h-[18px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="1" x2="12" y2="23"/><path d="M17 5H9.5a3.5 3.5 0 000 7h5a3.5 3.5 0 010 7H6"/></svg>
    {:else if type === 'budget'}
      <svg class="w-[18px] h-[18px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
    {:else if type === 'uptime'}
      <svg class="w-[18px] h-[18px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
    {:else}
      <svg class="w-[18px] h-[18px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/></svg>
    {/if}
  </div>

  <div class="text-3xl font-bold text-gray-100 tracking-tight leading-tight">{value}</div>
  <div class="text-xs font-medium text-gray-500 mt-1.5 uppercase tracking-wider">{label}</div>
</div>
