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
      error = e.message;
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
    <h2 class="text-2xl font-bold tracking-tight text-[#E8E4E0]">Configuration</h2>
    <p class="text-[#A09890] text-sm mt-1">Edit your Ryvos config.toml</p>
  </div>

  {#if loading}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-8 text-center">
      <p class="text-[#A09890] text-sm animate-pulse">Loading configuration...</p>
    </div>
  {:else if error}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-12 text-center">
      <p class="text-[#A09890] text-sm">Configuration endpoint not available</p>
    </div>
  {:else}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-5">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-sm font-semibold text-[#E8E4E0]">config.toml</h3>
        <div class="flex items-center gap-3">
          {#if saveMessage}
            <span class="text-xs {saveMessage.startsWith('Error') ? 'text-red-400' : 'text-emerald-400'}">
              {saveMessage}
            </span>
          {/if}
          {#if hasChanges}
            <span class="text-xs text-amber-400">Unsaved changes</span>
          {/if}
          <button
            on:click={saveConfig}
            disabled={saving || !hasChanges}
            class="px-4 py-1.5 bg-gradient-to-br from-[#F07030] to-[#E06020] text-white rounded-md
                   text-xs font-semibold transition-all duration-200
                   hover:shadow-lg hover:shadow-[#F07030]/30 hover:-translate-y-0.5
                   disabled:opacity-40 disabled:cursor-not-allowed disabled:transform-none"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
      <textarea
        bind:value={configContent}
        spellcheck="false"
        class="w-full h-[500px] px-4 py-3 bg-[#0F0F0F] border border-[rgba(255,255,255,0.08)] rounded-lg
               text-[#E8E4E0] font-mono text-sm leading-relaxed resize-y
               outline-none transition-all duration-200
               focus:border-[#F07030] focus:ring-2 focus:ring-[#F07030]/20"
      ></textarea>
    </div>
  {/if}
</div>
