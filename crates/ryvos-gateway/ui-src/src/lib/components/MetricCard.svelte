<script>
  export let label = '';
  export let value = '';
  export let icon = '';
  export let description = '';
</script>

<div class="bg-white border-2 border-[#1A1A1A] shadow-brutal brutal-shift transition-all duration-200 p-5">
  <div class="flex items-start justify-between">
    <div>
      <p class="label">{label}</p>
      <p class="mt-1 text-3xl font-heading tracking-tight">{value}</p>
      {#if description}
        <p class="mt-1 text-sm text-[#9B9590]">{description}</p>
      {/if}
    </div>
    <div class="border-2 border-[#1A1A1A] bg-[#F07030] text-white p-2.5">
      <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        {@html icon}
      </svg>
    </div>
  </div>
</div>
