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
  let currentRoute = 'dashboard';
  let routeParam = '';
  let showCommandPalette = false;

  const unsubAuth = authenticated.subscribe(v => isAuthenticated = v);

  function parseHash() {
    const hash = window.location.hash || '#/dashboard';
    const parts = hash.replace('#/', '').split('/');
    currentRoute = parts[0] || 'dashboard';
    routeParam = parts.slice(1).join('/') || '';
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
      // Allow blank key (no auth mode)
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
  <div class="flex h-screen bg-gray-950">
    <Sidebar {currentRoute} />
    <main class="flex-1 overflow-y-auto p-8">
      {#if currentRoute === 'dashboard'}
        <Dashboard />
      {:else if currentRoute === 'chat'}
        <Chat sessionId={routeParam} />
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
        <Dashboard />
      {/if}
    </main>
  </div>
  <CommandPalette bind:visible={showCommandPalette} on:close={() => showCommandPalette = false} />
{/if}
