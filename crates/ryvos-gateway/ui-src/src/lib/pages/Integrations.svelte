<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let apps = [];
  let loading = true;
  let error = '';
  let successApp = '';
  let actionInProgress = '';
  let setupApp = null; // Which app's setup form is open
  let setupForm = { client_id: '', client_secret: '', api_key: '' };
  let setupSaving = false;
  let setupError = '';

  const setupLinks = {
    gmail: { url: 'https://console.cloud.google.com/apis/credentials', label: 'Google Cloud Console → APIs & Services → Credentials → Create OAuth Client ID' },
    calendar: { url: 'https://console.cloud.google.com/apis/credentials', label: 'Same as Gmail — Google OAuth covers Calendar + Drive' },
    drive: { url: 'https://console.cloud.google.com/apis/credentials', label: 'Same as Gmail — Google OAuth covers Calendar + Drive' },
    slack: { url: 'https://api.slack.com/apps', label: 'Slack API → Your Apps → Create New App → OAuth & Permissions' },
    github: { url: 'https://github.com/settings/developers', label: 'GitHub → Settings → Developer Settings → OAuth Apps → New' },
    jira: { url: 'https://developer.atlassian.com/console/myapps/', label: 'Atlassian Developer → My Apps → Create → OAuth 2.0' },
    linear: { url: 'https://linear.app/settings/api', label: 'Linear → Settings → API → Create OAuth Application' },
    notion: { url: 'https://www.notion.so/my-integrations', label: 'Notion → My Integrations → Create New Integration → Copy Internal Token' },
  };

  onMount(async () => {
    const params = new URLSearchParams(window.location.search);
    const connected = params.get('connected');
    if (connected) {
      successApp = connected;
      const url = new URL(window.location);
      url.searchParams.delete('connected');
      window.history.replaceState({}, '', url);
      setTimeout(() => { successApp = ''; }, 4000);
    }
    await loadApps();
  });

  async function loadApps() {
    loading = true;
    error = '';
    try {
      const data = await apiFetch('/api/integrations');
      apps = data.apps || [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function openSetup(app) {
    setupApp = app.id;
    setupForm = { client_id: '', client_secret: '', api_key: '' };
    setupError = '';
  }

  function closeSetup() {
    setupApp = null;
    setupForm = { client_id: '', client_secret: '', api_key: '' };
    setupError = '';
  }

  async function saveSetup(app) {
    setupSaving = true;
    setupError = '';
    try {
      // Read current config
      const configData = await apiFetch('/api/config');
      let content = configData.content || '';

      const appId = app.id;
      const configKey = (appId === 'calendar' || appId === 'drive') ? 'gmail' : appId;

      if (configKey === 'notion') {
        if (!setupForm.api_key) { setupError = 'API key is required'; setupSaving = false; return; }
        content += `\n\n[integrations.notion]\napi_key = "${setupForm.api_key}"\n`;
      } else {
        if (!setupForm.client_id || !setupForm.client_secret) { setupError = 'Both fields are required'; setupSaving = false; return; }
        // Check if already exists
        if (content.includes(`[integrations.${configKey}]`)) {
          setupError = `[integrations.${configKey}] already exists in config.toml. Edit it via the Config page.`;
          setupSaving = false;
          return;
        }
        content += `\n\n[integrations.${configKey}]\nclient_id = "${setupForm.client_id}"\nclient_secret = "${setupForm.client_secret}"\n`;
      }

      // Write back
      const res = await apiFetch('/api/config', {
        method: 'PUT',
        body: JSON.stringify({ content }),
      });
      if (res.error) {
        setupError = res.error;
      } else {
        successApp = `${app.name} configured! Restart the daemon, then click Connect.`;
        setTimeout(() => { successApp = ''; }, 6000);
        closeSetup();
        await loadApps();
      }
    } catch (e) {
      setupError = e.message.includes('403') ? 'Admin access required to save config' : e.message;
    } finally {
      setupSaving = false;
    }
  }

  async function connectApp(app) {
    actionInProgress = app.id;
    try {
      const data = await apiFetch(`/api/integrations/${app.id}/connect`, { method: 'POST' });
      if (data.error) {
        error = data.error;
      } else if (data.redirect_url) {
        window.open(data.redirect_url, '_blank', 'width=600,height=700');
      } else if (data.connected) {
        successApp = app.name;
        setTimeout(() => { successApp = ''; }, 4000);
        await loadApps();
      }
    } catch (e) {
      error = `Failed to connect ${app.name}: ${e.message}`;
    } finally {
      actionInProgress = '';
    }
  }

  async function disconnectApp(app) {
    actionInProgress = app.id;
    try {
      await apiFetch(`/api/integrations/${app.id}`, { method: 'DELETE' });
      await loadApps();
    } catch (e) {
      error = `Failed to disconnect ${app.name}: ${e.message}`;
    } finally {
      actionInProgress = '';
    }
  }
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Integrations</h2>
    <p class="text-[#9B9590] text-sm mt-1">Connect external apps and services to your agent</p>
  </div>

  {#if successApp}
    <div class="mb-5 px-4 py-3 bg-[#16A34A]/10 border-2 border-[#16A34A] text-sm text-[#1A1A1A] flex items-center justify-between">
      <span>{successApp}</span>
      <button on:click={() => successApp = ''} class="text-[#6B6560] hover:text-[#1A1A1A] text-xs font-bold uppercase tracking-wider">Dismiss</button>
    </div>
  {/if}

  {#if error}
    <div class="mb-5 px-4 py-3 bg-[#DC2626]/10 border-2 border-[#DC2626] text-sm text-[#1A1A1A] flex items-center justify-between">
      <span>{error}</span>
      <button on:click={() => error = ''} class="text-[#6B6560] hover:text-[#1A1A1A] text-xs font-bold uppercase tracking-wider">Dismiss</button>
    </div>
  {/if}

  {#if loading}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      {#each Array(4) as _}
        <div class="bg-white border-2 border-[#1A1A1A] p-5 min-h-[120px] animate-pulse"></div>
      {/each}
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      {#each apps as app (app.id)}
        <div class="bg-white border-2 border-[#1A1A1A] p-5">
          <div class="flex items-start justify-between mb-3">
            <div>
              <h3 class="text-lg font-bold text-[#1A1A1A]">{app.name}</h3>
              <p class="text-sm text-[#9B9590]">{app.actions} action{app.actions !== 1 ? 's' : ''}</p>
            </div>
            {#if app.connected}
              <span class="bg-[#16A34A]/10 text-[#16A34A] border border-[#16A34A] text-xs uppercase tracking-wider font-bold px-2 py-0.5">Connected</span>
            {:else if app.configured}
              <span class="bg-[#FEF3EC] text-[#F07030] border border-[#F07030] text-xs uppercase tracking-wider font-bold px-2 py-0.5">Ready</span>
            {/if}
          </div>

          {#if app.connected}
            <!-- Connected state -->
            <button
              on:click={() => disconnectApp(app)}
              disabled={actionInProgress === app.id}
              class="border-2 border-[#1A1A1A] text-[#6B6560] text-xs uppercase tracking-wider font-bold px-3 py-1.5
                     hover:text-[#DC2626] hover:border-[#DC2626] transition-all duration-100
                     disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {actionInProgress === app.id ? 'Disconnecting...' : 'Disconnect'}
            </button>

          {:else if app.configured}
            <!-- Configured, ready to connect -->
            <button
              on:click={() => connectApp(app)}
              disabled={actionInProgress === app.id}
              class="bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                     uppercase font-bold tracking-wider text-xs px-4 py-2 transition-all duration-100
                     disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {actionInProgress === app.id ? 'Connecting...' : 'Connect'}
            </button>

          {:else if setupApp === app.id}
            <!-- Setup form (expanded) -->
            <div class="mt-2 border-t-2 border-[#E8E4E0] pt-3">
              {#if setupLinks[app.id]}
                <a href={setupLinks[app.id].url} target="_blank" rel="noopener"
                   class="text-xs text-[#F07030] underline mb-3 block">
                  {setupLinks[app.id].label} ↗
                </a>
              {/if}

              {#if app.id === 'notion'}
                <div class="mb-3">
                  <label class="label block mb-1">Internal Integration Token</label>
                  <input
                    bind:value={setupForm.api_key}
                    type="password"
                    placeholder="ntn_..."
                    class="w-full border-2 border-[#1A1A1A] bg-white text-[#1A1A1A] text-sm px-3 py-2 font-mono
                           focus:border-[#F07030] outline-none"
                  />
                </div>
              {:else}
                <div class="mb-2">
                  <label class="label block mb-1">Client ID</label>
                  <input
                    bind:value={setupForm.client_id}
                    type="text"
                    placeholder="xxxx.apps.googleusercontent.com"
                    class="w-full border-2 border-[#1A1A1A] bg-white text-[#1A1A1A] text-sm px-3 py-2 font-mono
                           focus:border-[#F07030] outline-none"
                  />
                </div>
                <div class="mb-3">
                  <label class="label block mb-1">Client Secret</label>
                  <input
                    bind:value={setupForm.client_secret}
                    type="password"
                    placeholder="GOCSPX-..."
                    class="w-full border-2 border-[#1A1A1A] bg-white text-[#1A1A1A] text-sm px-3 py-2 font-mono
                           focus:border-[#F07030] outline-none"
                  />
                </div>
              {/if}

              {#if setupError}
                <p class="text-xs text-[#DC2626] mb-2">{setupError}</p>
              {/if}

              <div class="flex gap-2">
                <button
                  on:click={() => saveSetup(app)}
                  disabled={setupSaving}
                  class="bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                         uppercase font-bold tracking-wider text-xs px-4 py-2
                         disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  {setupSaving ? 'Saving...' : 'Save & Configure'}
                </button>
                <button
                  on:click={closeSetup}
                  class="border-2 border-[#1A1A1A] text-[#6B6560] text-xs uppercase tracking-wider font-bold px-3 py-1.5
                         hover:text-[#1A1A1A] transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>

          {:else}
            <!-- Not configured — show Setup button -->
            <button
              on:click={() => openSetup(app)}
              class="border-2 border-[#1A1A1A] text-[#1A1A1A] text-xs uppercase tracking-wider font-bold px-4 py-2
                     shadow-brutal-sm brutal-shift transition-all duration-100"
            >
              Setup
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
