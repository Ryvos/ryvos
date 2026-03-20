<script>
  import { onMount, onDestroy } from 'svelte';
  import { getApiKey, setApiKey } from './lib/api.js';
  import { connect, disconnect } from './lib/ws.js';
  import { authenticated } from './lib/stores.js';

  import Sidebar from './lib/components/Sidebar.svelte';
  import LoginOverlay from './lib/components/LoginOverlay.svelte';
  import CommandPalette from './lib/components/CommandPalette.svelte';

  import Dashboard from './lib/pages/Dashboard.svelte';
  import Chat from './lib/pages/Chat.svelte';
  import Sessions from './lib/pages/Sessions.svelte';
  import Runs from './lib/pages/Runs.svelte';
  import Costs from './lib/pages/Costs.svelte';
  import Settings from './lib/pages/Settings.svelte';
  import AuditTrail from './lib/pages/AuditTrail.svelte';
  import VikingBrowser from './lib/pages/VikingBrowser.svelte';
  import ConfigEditor from './lib/pages/ConfigEditor.svelte';
  import Channels from './lib/pages/Channels.svelte';
  import Graph from './lib/pages/Graph.svelte';

  let isAuthenticated = false;
  let currentRoute = 'chat';
  let routeParam = '';
  let showCommandPalette = false;
  let sidebarOpen = false;

  const unsubAuth = authenticated.subscribe(v => isAuthenticated = v);

  function parseHash() {
    const hash = window.location.hash || '#/chat';
    const parts = hash.replace('#/', '').split('/');
    currentRoute = parts[0] || 'chat';
    routeParam = decodeURIComponent(parts.slice(1).join('/') || '');
  }

  function handleLogin(e) {
    const key = e.detail.key;
    setApiKey(key);
    authenticated.set(true);
    connect(key);
    parseHash();
  }

  function handleKeydown(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      showCommandPalette = !showCommandPalette;
    }
  }

  onMount(() => {
    const key = getApiKey();
    if (key || key === '') {
      authenticated.set(true);
      connect(key);
    }
    parseHash();
    window.addEventListener('hashchange', parseHash);
    window.addEventListener('keydown', handleKeydown);
  });

  onDestroy(() => {
    unsubAuth();
    window.removeEventListener('hashchange', parseHash);
    window.removeEventListener('keydown', handleKeydown);
    disconnect();
  });
</script>

{#if !isAuthenticated}
  <LoginOverlay on:login={handleLogin} />
{:else}
  <div class="flex h-screen bg-[#0F0F0F]">
    <!-- Mobile overlay -->
    {#if sidebarOpen}
      <!-- svelte-ignore a11y-click-events-have-key-events -->
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div class="fixed inset-0 bg-black/50 z-40 md:hidden" on:click={() => sidebarOpen = false}></div>
    {/if}

    <Sidebar {currentRoute} bind:open={sidebarOpen} />

    <main class="flex-1 overflow-y-auto p-6 md:p-8">
      <!-- Mobile hamburger -->
      <button
        class="md:hidden mb-4 p-2 rounded-lg bg-[#1A1A1A] border border-[rgba(255,255,255,0.08)] text-[#A09890] hover:text-[#E8E4E0] transition-colors"
        on:click={() => sidebarOpen = true}
      >
        <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/>
        </svg>
      </button>

      {#if currentRoute === 'chat'}
        <Chat sessionId={routeParam} />
      {:else if currentRoute === 'dashboard'}
        <Dashboard />
      {:else if currentRoute === 'sessions'}
        <Sessions />
      {:else if currentRoute === 'runs'}
        <Runs />
      {:else if currentRoute === 'costs'}
        <Costs />
      {:else if currentRoute === 'settings'}
        <Settings />
      {:else if currentRoute === 'audit'}
        <AuditTrail />
      {:else if currentRoute === 'viking'}
        <VikingBrowser />
      {:else if currentRoute === 'config'}
        <ConfigEditor />
      {:else if currentRoute === 'channels'}
        <Channels />
      {:else if currentRoute === 'graph'}
        <Graph />
      {:else}
        <Chat sessionId={routeParam} />
      {/if}
    </main>
  </div>
  <CommandPalette bind:visible={showCommandPalette} on:close={() => showCommandPalette = false} />
{/if}
