<script>
  import { createEventDispatcher } from 'svelte';
  import logoUrl from '../../assets/logo.png';

  const dispatch = createEventDispatcher();
  let apiKeyValue = '';

  function handleSubmit() {
    dispatch('login', { key: apiKeyValue.trim() });
  }

  function handleKeydown(e) {
    if (e.key === 'Enter') handleSubmit();
  }
</script>

<div class="fixed inset-0 bg-black/40 flex items-center justify-center z-50">
  <div class="bg-white border-2 border-[#1A1A1A] shadow-[4px_4px_0px_#1A1A1A] p-8 w-[380px] text-center">
    <!-- Logo -->
    <div class="mb-5">
      <img src={logoUrl} alt="Ryvos" class="mx-auto w-14 h-14" />
    </div>

    <h2 class="text-xl font-heading font-bold text-[#1A1A1A] tracking-tight mb-1">Welcome to Ryvos</h2>
    <p class="text-[#6B6560] text-sm mb-6">Enter your API key to access the dashboard</p>

    <div class="mb-1">
      <label class="block text-left text-[11px] uppercase font-bold tracking-[0.1em] text-[#9B9590] mb-1.5">API Key</label>
      <input
        type="password"
        bind:value={apiKeyValue}
        on:keydown={handleKeydown}
        placeholder="rk_..."
        class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-[#1A1A1A] font-mono text-sm
               outline-none transition-colors duration-100
               focus:border-[#F07030]
               placeholder:text-[#9B9590] placeholder:font-mono"
      />
    </div>

    <p class="text-[11px] text-[#9B9590] mb-5 text-left">Leave blank if no auth is configured</p>

    <button
      on:click={handleSubmit}
      class="w-full py-3 bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-[2px_2px_0px_#1A1A1A]
             uppercase font-bold text-sm tracking-wider flex items-center justify-center gap-2
             transition-all duration-100
             hover:-translate-y-0.5 hover:shadow-[4px_4px_0px_#1A1A1A]
             active:translate-x-[2px] active:translate-y-[2px] active:shadow-none"
    >
      <span>Connect</span>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8h10m-4-4l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="square" stroke-linejoin="miter"/></svg>
    </button>
  </div>
</div>
