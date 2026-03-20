<script>
  import { connectionStatus, disconnect } from '../ws.js';
  import { clearApiKey } from '../api.js';
  import { authenticated } from '../stores.js';
  import logoUrl from '../../assets/logo.png';

  export let currentRoute = 'chat';
  export let open = true;

  let status = 'disconnected';
  connectionStatus.subscribe(v => status = v);

  const navItems = [
    { route: 'chat',       label: 'Chat',            icon: 'message' },
    { route: 'dashboard',  label: 'Dashboard',       icon: 'grid' },
    { route: 'sessions',   label: 'Sessions',        icon: 'layers' },
    { route: 'runs',       label: 'Runs',            icon: 'activity' },
    { route: 'costs',      label: 'Costs',           icon: 'dollar' },
    { route: 'channels',   label: 'Channels',        icon: 'radio' },
    { route: 'audit',      label: 'Audit Trail',     icon: 'shield' },
    { route: 'viking',     label: 'Viking Browser',  icon: 'database' },
    { route: 'config',     label: 'Config',          icon: 'file' },
    { route: 'settings',   label: 'Settings',        icon: 'settings' },
  ];

  function handleLogout() {
    disconnect();
    clearApiKey();
    authenticated.set(false);
  }

  function isActive(route) {
    if (route === currentRoute) return true;
    if (route === 'chat' && currentRoute === 'sessions') return true;
    return false;
  }

  function handleNavClick() {
    // Close sidebar on mobile after navigation
    if (window.innerWidth < 768) {
      open = false;
    }
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<aside
  class="w-60 bg-[#1A1A1A] border-r border-[rgba(255,255,255,0.08)] flex flex-col h-screen shrink-0
    fixed md:relative z-50 transition-transform duration-200 ease-out
    {open ? 'translate-x-0' : '-translate-x-full md:translate-x-0'}"
>
  <!-- Header -->
  <div class="flex items-center gap-2.5 px-5 py-4">
    <img src={logoUrl} alt="Ryvos" class="w-7 h-7 rounded-sm" />
    <span class="text-lg font-bold tracking-tight text-[#E8E4E0]">Ryvos</span>
    <span class="text-[0.6rem] font-semibold text-[#F07030] bg-[#F07030]/10 px-2 py-0.5 rounded-full">v0.6.3</span>
  </div>

  <!-- Nav -->
  <nav class="flex-1 flex flex-col gap-0.5 px-3 py-1 overflow-y-auto">
    {#each navItems as item}
      <a
        href="#/{item.route}"
        on:click={handleNavClick}
        class="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-all duration-200
          {isActive(item.route)
            ? 'bg-[#F07030]/10 text-[#F07030] font-semibold border-l-2 border-l-[#F07030] -ml-[2px]'
            : 'text-[#A09890] hover:bg-[#2A2A2A] hover:text-[#E8E4E0]'}"
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
  <div class="px-5 py-3 border-t border-[rgba(255,255,255,0.08)] flex items-center justify-between">
    <div class="flex items-center gap-2 text-xs text-[#A09890]">
      <span
        class="w-2 h-2 rounded-full {status === 'connected'
          ? 'bg-emerald-400 shadow-[0_0_8px_theme(colors.emerald.400)] animate-pulse-glow'
          : status === 'error' ? 'bg-red-400' : 'bg-gray-600'}"
      ></span>
      <span>{status === 'connected' ? 'Connected' : status === 'error' ? 'Error' : 'Disconnected'}</span>
    </div>
    <button
      on:click={handleLogout}
      class="text-xs text-[#A09890] hover:text-red-400 hover:bg-red-400/10 px-2 py-1 rounded transition-all duration-200"
    >
      Disconnect
    </button>
  </div>
</aside>
