<script>
  import { createEventDispatcher } from 'svelte';

  export let visible = false;

  const dispatch = createEventDispatcher();
  let searchQuery = '';
  let inputEl;
  let selectedIndex = 0;

  const allPages = [
    { route: 'chat', label: 'Chat', desc: 'Send messages to agent' },
    { route: 'dashboard', label: 'Dashboard', desc: 'Overview and metrics' },
    { route: 'sessions', label: 'Sessions', desc: 'Active conversation sessions' },
    { route: 'runs', label: 'Runs', desc: 'Agent run history' },
    { route: 'costs', label: 'Costs', desc: 'Token usage and spending' },
    { route: 'channels', label: 'Channels', desc: 'Channel integrations status' },
    { route: 'audit', label: 'Audit Trail', desc: 'Tool execution audit log' },
    { route: 'viking', label: 'Viking Browser', desc: 'Browse Viking context database' },
    { route: 'config', label: 'Config', desc: 'Edit configuration file' },
    { route: 'settings', label: 'Settings', desc: 'System info and config' },
  ];

  $: filteredPages = searchQuery.trim()
    ? allPages.filter(p =>
        p.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
        p.desc.toLowerCase().includes(searchQuery.toLowerCase()))
    : allPages;

  $: if (selectedIndex >= filteredPages.length) selectedIndex = Math.max(0, filteredPages.length - 1);

  $: if (visible && inputEl) {
    setTimeout(() => inputEl && inputEl.focus(), 50);
  }

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      close();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, filteredPages.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (e.key === 'Enter' && filteredPages.length > 0) {
      navigate(filteredPages[selectedIndex]);
    }
  }

  function navigate(page) {
    window.location.hash = '#/' + page.route;
    close();
  }

  function close() {
    searchQuery = '';
    selectedIndex = 0;
    dispatch('close');
  }

  function handleBackdropClick(e) {
    if (e.target === e.currentTarget) close();
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="fixed inset-0 bg-black/60 backdrop-blur-sm z-50 flex items-start justify-center pt-[20vh]"
    on:click={handleBackdropClick}
    on:keydown={handleKeydown}
  >
    <div class="bg-[#1A1A1A] border border-[rgba(255,255,255,0.12)] rounded-xl w-[520px] shadow-2xl overflow-hidden">
      <!-- Search input -->
      <div class="flex items-center gap-3 px-4 py-3 border-b border-[rgba(255,255,255,0.08)]">
        <svg class="w-5 h-5 text-[#A09890] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>
        </svg>
        <input
          bind:this={inputEl}
          bind:value={searchQuery}
          placeholder="Search pages..."
          class="flex-1 bg-transparent text-[#E8E4E0] text-sm outline-none placeholder:text-[#555]"
        />
        <kbd class="text-[0.65rem] text-[#A09890] bg-[#2A2A2A] px-1.5 py-0.5 rounded border border-[rgba(255,255,255,0.08)]">ESC</kbd>
      </div>

      <!-- Results -->
      <div class="max-h-72 overflow-y-auto py-2">
        {#each filteredPages as page, i}
          <button
            class="w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors duration-100
              {i === selectedIndex ? 'bg-[#F07030]/10 text-[#F07030]' : 'text-[#E8E4E0] hover:bg-[#2A2A2A]'}"
            on:click={() => navigate(page)}
            on:mouseenter={() => selectedIndex = i}
          >
            <span class="font-medium text-sm">{page.label}</span>
            <span class="text-xs text-[#A09890]">{page.desc}</span>
          </button>
        {/each}
        {#if filteredPages.length === 0}
          <p class="text-[#A09890] text-sm text-center py-4">No matching pages</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
