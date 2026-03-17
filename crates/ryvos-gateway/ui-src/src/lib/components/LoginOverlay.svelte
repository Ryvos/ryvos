<script>
  import { createEventDispatcher } from 'svelte';

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
  <div class="bg-gray-900 border border-gray-800 rounded-2xl p-10 w-[380px] text-center shadow-2xl shadow-indigo-500/5">
    <!-- Logo -->
    <div class="mb-5">
      <svg class="mx-auto drop-shadow-[0_4px_12px_rgba(129,140,248,0.3)]" width="48" height="48" viewBox="0 0 48 48" fill="none">
        <rect width="48" height="48" rx="12" fill="url(#lg)"/>
        <path d="M14 24l6 6 14-14" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>
        <defs><linearGradient id="lg" x1="0" y1="0" x2="48" y2="48"><stop stop-color="#818cf8"/><stop offset="1" stop-color="#6366f1"/></linearGradient></defs>
      </svg>
    </div>

    <h2 class="text-xl font-bold text-gray-100 tracking-tight mb-1">Welcome to Ryvos</h2>
    <p class="text-gray-500 text-sm mb-6">Enter your API key to access the dashboard</p>

    <div class="mb-3">
      <input
        type="password"
        bind:value={apiKeyValue}
        on:keydown={handleKeydown}
        placeholder="rk_..."
        class="w-full px-4 py-3 bg-gray-950 border border-gray-800 rounded-lg text-gray-100 font-mono text-sm
               outline-none transition-all duration-200
               focus:border-indigo-400 focus:ring-2 focus:ring-indigo-400/20
               placeholder:text-gray-600 placeholder:font-mono"
      />
    </div>

    <button
      on:click={handleSubmit}
      class="w-full py-3 bg-gradient-to-br from-indigo-400 to-indigo-600 text-white rounded-lg
             font-semibold text-sm flex items-center justify-center gap-2
             transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-indigo-500/30
             active:translate-y-0"
    >
      <span>Connect</span>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8h10m-4-4l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>

    <p class="text-xs text-gray-600 mt-4">Leave blank if no auth is configured</p>
  </div>
</div>
