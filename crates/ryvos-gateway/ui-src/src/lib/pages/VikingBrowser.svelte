<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let tree = [];
  let loading = true;
  let error = '';
  let expandedPaths = {};
  let selectedContent = null;
  let selectedPath = '';
  let contentLoading = false;

  async function loadTree(path = 'viking://') {
    try {
      const data = await apiFetch(`/api/viking/list?path=${encodeURIComponent(path)}`);
      return Array.isArray(data) ? data : (data.entries || data.items || data.children || []);
    } catch (e) {
      return [];
    }
  }

  onMount(async () => {
    try {
      tree = await loadTree('viking://');
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });

  async function toggleDir(item) {
    const path = item.path || item.name;
    if (expandedPaths[path]) {
      expandedPaths = { ...expandedPaths };
      delete expandedPaths[path];
      expandedPaths = expandedPaths;
    } else {
      const children = await loadTree(path);
      expandedPaths = { ...expandedPaths, [path]: children };
    }
  }

  async function readLeaf(item) {
    const path = item.path || item.name;
    selectedPath = path;
    contentLoading = true;
    selectedContent = null;
    try {
      const data = await apiFetch(`/api/viking/read?path=${encodeURIComponent(path)}&level=L1`);
      selectedContent = data;
    } catch (e) {
      selectedContent = { error: e.message };
    } finally {
      contentLoading = false;
    }
  }

  function isDirectory(item) {
    return item.is_directory === true || item.type === 'directory' || item.type === 'dir';
  }

  function getDisplayName(item) {
    const name = item.name || item.path || '';
    return name.split('/').filter(Boolean).pop() || name;
  }
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-bold tracking-tight text-[#E8E4E0]">Viking Browser</h2>
    <p class="text-[#A09890] text-sm mt-1">Browse the Viking context database</p>
  </div>

  {#if loading}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-8 text-center">
      <p class="text-[#A09890] text-sm animate-pulse">Loading Viking tree...</p>
    </div>
  {:else if error}
    <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-12 text-center">
      <p class="text-[#A09890] text-sm">Viking browser not available</p>
    </div>
  {:else}
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
      <!-- Tree view -->
      <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-5 max-h-[600px] overflow-y-auto">
        <h3 class="text-sm font-semibold text-[#E8E4E0] mb-4">File Tree</h3>
        {#if tree.length === 0}
          <p class="text-[#A09890] text-sm text-center py-8">No entries found at viking://</p>
        {:else}
          <div class="space-y-0.5">
            {#each tree as item}
              {#if isDirectory(item)}
                <button
                  on:click={() => toggleDir(item)}
                  class="w-full flex items-center gap-2 px-3 py-2 text-left rounded-lg text-sm text-[#E8E4E0]
                         hover:bg-[#2A2A2A] transition-colors duration-150"
                >
                  <svg class="w-4 h-4 shrink-0 text-[#A09890] transition-transform duration-150 {expandedPaths[item.path || item.name] ? 'rotate-90' : ''}"
                    viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <polyline points="9 18 15 12 9 6"/>
                  </svg>
                  <svg class="w-4 h-4 shrink-0 text-amber-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/>
                  </svg>
                  <span class="font-medium">{getDisplayName(item)}</span>
                </button>
                <!-- Expanded children -->
                {#if expandedPaths[item.path || item.name]}
                  <div class="ml-6 border-l border-[rgba(255,255,255,0.08)] pl-3 space-y-0.5">
                    {#each expandedPaths[item.path || item.name] as child}
                      {#if isDirectory(child)}
                        <button
                          on:click={() => toggleDir(child)}
                          class="w-full flex items-center gap-2 px-3 py-1.5 text-left rounded-lg text-sm text-[#A09890]
                                 hover:bg-[#2A2A2A] transition-colors duration-150"
                        >
                          <svg class="w-3.5 h-3.5 shrink-0 text-[#A09890] transition-transform duration-150 {expandedPaths[child.path || child.name] ? 'rotate-90' : ''}"
                            viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
                          <svg class="w-3.5 h-3.5 shrink-0 text-amber-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/>
                          </svg>
                          <span>{getDisplayName(child)}</span>
                        </button>
                      {:else}
                        <button
                          on:click={() => readLeaf(child)}
                          class="w-full flex items-center gap-2 px-3 py-1.5 text-left rounded-lg text-sm
                                 {selectedPath === (child.path || child.name)
                                   ? 'bg-[#F07030]/10 text-[#F07030]'
                                   : 'text-[#A09890] hover:bg-[#2A2A2A]'}
                                 transition-colors duration-150"
                        >
                          <svg class="w-3.5 h-3.5 shrink-0 text-[#A09890]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/>
                          </svg>
                          <span>{getDisplayName(child)}</span>
                        </button>
                      {/if}
                    {/each}
                  </div>
                {/if}
              {:else}
                <button
                  on:click={() => readLeaf(item)}
                  class="w-full flex items-center gap-2 px-3 py-2 text-left rounded-lg text-sm
                         {selectedPath === (item.path || item.name)
                           ? 'bg-[#F07030]/10 text-[#F07030]'
                           : 'text-[#E8E4E0] hover:bg-[#2A2A2A]'}
                         transition-colors duration-150"
                >
                  <svg class="w-4 h-4 shrink-0 text-[#A09890]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/>
                  </svg>
                  <span class="font-medium">{getDisplayName(item)}</span>
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>

      <!-- Content view -->
      <div class="bg-[#222222] border border-[rgba(255,255,255,0.08)] rounded-xl p-5 max-h-[600px] overflow-y-auto">
        <h3 class="text-sm font-semibold text-[#E8E4E0] mb-4">
          {#if selectedPath}
            <span class="font-mono text-xs text-[#A09890]">{selectedPath}</span>
          {:else}
            Content
          {/if}
        </h3>

        {#if contentLoading}
          <p class="text-[#A09890] text-sm text-center py-8 animate-pulse">Loading...</p>
        {:else if selectedContent}
          {#if selectedContent.error}
            <p class="text-red-400 text-sm">{selectedContent.error}</p>
          {:else}
            <pre class="bg-[#0F0F0F] border border-[rgba(255,255,255,0.08)] rounded-lg p-4 text-xs text-[#E8E4E0] font-mono whitespace-pre-wrap overflow-x-auto">{JSON.stringify(selectedContent, null, 2)}</pre>
          {/if}
        {:else}
          <p class="text-[#A09890] text-sm text-center py-8">Select a file from the tree to view its contents</p>
        {/if}
      </div>
    </div>
  {/if}
</div>
