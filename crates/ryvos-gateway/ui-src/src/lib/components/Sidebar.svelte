<script>
  import { connectionStatus, disconnect } from '../ws.js';
  import { clearApiKey } from '../api.js';
  import { authenticated } from '../stores.js';

  export let currentRoute = 'dashboard';

  let status = 'disconnected';
  connectionStatus.subscribe(v => status = v);

  const navItems = [
    { route: 'dashboard',  label: 'Dashboard',      icon: 'grid' },
    { route: 'chat',       label: 'Chat',            icon: 'message' },
    { route: 'sessions',   label: 'Sessions',        icon: 'layers' },
    { route: 'runs',       label: 'Runs',            icon: 'activity' },
    { route: 'costs',      label: 'Costs',           icon: 'dollar' },
    { route: 'settings',   label: 'Settings',        icon: 'settings' },
    { route: 'audit',      label: 'Audit Trail',     icon: 'shield' },
    { route: 'viking',     label: 'Viking Browser',  icon: 'database' },
    { route: 'config',     label: 'Config',          icon: 'file' },
    { route: 'channels',   label: 'Channels',        icon: 'radio' },
  ];

  function handleLogout() {
    disconnect();
    clearApiKey();
    authenticated.set(false);
  }

  function isActive(route) {
    if (route === currentRoute) return true;
    if (route === 'sessions' && currentRoute === 'chat') return true;
    return false;
  }
</script>

<aside class="w-60 bg-gray-900 border-r border-gray-800 flex flex-col h-screen shrink-0">
  <!-- Header -->
  <div class="flex items-center gap-2.5 px-5 py-4">
    <svg width="28" height="28" viewBox="0 0 48 48" fill="none">
      <rect width="48" height="48" rx="12" fill="url(#slg)"/>
      <path d="M14 24l6 6 14-14" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>
      <defs><linearGradient id="slg" x1="0" y1="0" x2="48" y2="48"><stop stop-color="#818cf8"/><stop offset="1" stop-color="#6366f1"/></linearGradient></defs>
    </svg>
    <span class="text-lg font-bold tracking-tight text-gray-100">Ryvos</span>
    <span class="text-[0.65rem] font-semibold text-indigo-400 bg-indigo-400/10 px-2 py-0.5 rounded-full">v0.6</span>
  </div>

  <!-- Nav -->
  <nav class="flex-1 flex flex-col gap-0.5 px-3 py-1 overflow-y-auto">
    {#each navItems as item}
      <a
        href="#/{item.route}"
        class="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-all duration-200
          {isActive(item.route)
            ? 'bg-indigo-400/10 text-indigo-400 font-semibold'
            : 'text-gray-400 hover:bg-gray-800 hover:text-gray-200'}"
      >
        {#if item.icon === 'grid'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
        {:else if item.icon === 'message'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
        {:else if item.icon === 'layers'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>
        {:else if item.icon === 'activity'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>
        {:else if item.icon === 'dollar'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="1" x2="12" y2="23"/><path d="M17 5H9.5a3.5 3.5 0 000 7h5a3.5 3.5 0 010 7H6"/></svg>
        {:else if item.icon === 'settings'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
        {:else if item.icon === 'shield'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
        {:else if item.icon === 'database'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/></svg>
        {:else if item.icon === 'file'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
        {:else if item.icon === 'radio'}
          <svg class="w-[18px] h-[18px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="2"/><path d="M16.24 7.76a6 6 0 010 8.49m-8.48-.01a6 6 0 010-8.49m11.31-2.82a10 10 0 010 14.14m-14.14 0a10 10 0 010-14.14"/></svg>
        {/if}
        <span>{item.label}</span>
      </a>
    {/each}
  </nav>

  <!-- Footer -->
  <div class="px-5 py-3 border-t border-gray-800 flex items-center justify-between">
    <div class="flex items-center gap-2 text-xs text-gray-500">
      <span
        class="w-1.5 h-1.5 rounded-full {status === 'connected' ? 'bg-emerald-400 shadow-[0_0_6px_theme(colors.emerald.400)]' : status === 'error' ? 'bg-red-400' : 'bg-gray-600'}"
      ></span>
      <span>{status === 'connected' ? 'Connected' : status === 'error' ? 'Error' : 'Disconnected'}</span>
    </div>
    <button
      on:click={handleLogout}
      class="text-xs text-gray-500 hover:text-red-400 hover:bg-red-400/10 px-2 py-1 rounded transition-all duration-200"
    >
      Disconnect
    </button>
  </div>
</aside>
