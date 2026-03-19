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

<div class="fixed inset-0 bg-black/80 backdrop-blur-sm flex items-center justify-center z-50">
  <div class="bg-[#1A1A1A] border border-[rgba(255,255,255,0.08)] rounded-2xl p-10 w-[380px] text-center shadow-2xl shadow-[#F07030]/5">
    <!-- Logo -->
    <div class="mb-5">
      <img src={logoUrl} alt="Ryvos" class="mx-auto w-14 h-14 rounded-md drop-shadow-[0_4px_12px_rgba(240,112,48,0.3)]" />
    </div>

    <h2 class="text-xl font-bold text-[#E8E4E0] tracking-tight mb-1">Welcome to Ryvos</h2>
    <p class="text-[#A09890] text-sm mb-6">Enter your API key to access the dashboard</p>

    <div class="mb-3">
      <input
        type="password"
        bind:value={apiKeyValue}
        on:keydown={handleKeydown}
        placeholder="rk_..."
        class="w-full px-4 py-3 bg-[#0F0F0F] border border-[rgba(255,255,255,0.08)] rounded-lg text-[#E8E4E0] font-mono text-sm
               outline-none transition-all duration-200
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20
               placeholder:text-[#555] placeholder:font-mono"
      />
    </div>

    <button
      on:click={handleSubmit}
      class="w-full py-3 bg-gradient-to-br from-[#F07030] to-[#E06020] text-white rounded-lg
             font-semibold text-sm flex items-center justify-center gap-2
             transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-[#F07030]/30
             active:translate-y-0"
    >
      <span>Connect</span>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8h10m-4-4l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>

    <p class="text-xs text-[#555] mt-4">Leave blank if no auth is configured</p>
  </div>
</div>
