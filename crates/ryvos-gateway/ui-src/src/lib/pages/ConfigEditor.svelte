<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let configContent = '';
  let loading = true;
  let error = '';
  let saving = false;
  let saveMessage = '';
  let originalContent = '';

  onMount(async () => {
    try {
      const data = await apiFetch('/api/config');
      configContent = data.content || data.config || (typeof data === 'string' ? data : JSON.stringify(data, null, 2));
      originalContent = configContent;
    } catch (e) {
      error = e.message.includes('403') ? 'admin_required' : e.message;
    } finally {
      loading = false;
    }
  });

  async function saveConfig() {
    saving = true;
    saveMessage = '';
    try {
      await apiFetch('/api/config', {
        method: 'PUT',
        body: JSON.stringify({ content: configContent }),
      });
      saveMessage = 'Configuration saved successfully';
      originalContent = configContent;
      setTimeout(() => { saveMessage = ''; }, 3000);
    } catch (e) {
      saveMessage = 'Error saving: ' + e.message;
    } finally {
      saving = false;
    }
  }

  $: hasChanges = configContent !== originalContent;
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Configuration</h2>
    <p class="text-[#9B9590] text-sm mt-1">Edit your Ryvos config.toml</p>
  </div>

  {#if loading}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading configuration...</p>
    </div>
  {:else if error === 'admin_required'}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <svg class="mx-auto mb-3 w-8 h-8 text-[#D97706]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
      <p class="text-[#1A1A1A] text-sm font-medium mb-1">Admin Access Required</p>
      <p class="text-[#9B9590] text-xs">The configuration editor requires an Admin API key. Connect with an admin key to edit config.toml.</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">Configuration endpoint not available: {error}</p>
    </div>
  {:else}
    <div class="bg-white border-2 border-[#1A1A1A] p-5">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-sm font-semibold text-[#1A1A1A]">config.toml</h3>
        <div class="flex items-center gap-3">
          {#if saveMessage}
            <span class="text-xs {saveMessage.startsWith('Error') ? 'text-[#DC2626]' : 'text-[#16A34A]'}">
              {saveMessage}
            </span>
          {/if}
          {#if hasChanges}
            <span class="text-xs text-[#D97706]">Unsaved changes</span>
          {/if}
          <button
            on:click={saveConfig}
            disabled={saving || !hasChanges}
            class="px-4 py-1.5 bg-[#F07030] text-white border-2 border-[#1A1A1A]
                   text-xs font-semibold transition-all duration-200
                   shadow-brutal-sm brutal-shift
                   disabled:opacity-40 disabled:cursor-not-allowed disabled:transform-none"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
      <textarea
        bind:value={configContent}
        spellcheck="false"
        class="w-full h-[500px] px-4 py-3 bg-white border-2 border-[#1A1A1A] font-mono
               text-[#1A1A1A] text-sm leading-relaxed resize-y
               outline-none transition-all duration-200
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20"
      ></textarea>
    </div>
  {/if}
</div>
