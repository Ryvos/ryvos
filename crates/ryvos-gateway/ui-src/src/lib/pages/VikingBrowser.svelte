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

<!-- Recursive tree node snippet -->
{#snippet treeNode(items, depth)}
  {#each items as item}
    {#if isDirectory(item)}
      <button
        on:click={() => toggleDir(item)}
        class="w-full flex items-center gap-2 px-3 text-left text-sm hover:bg-[#F7F4F0] transition-colors duration-150
               {depth === 0 ? 'py-2 text-[#1A1A1A]' : 'py-1.5 text-[#6B6560]'}"
      >
        <svg class="w-4 h-4 shrink-0 text-[#9B9590] transition-transform duration-150 {expandedPaths[item.path || item.name] ? 'rotate-90' : ''}"
          viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="9 18 15 12 9 6"/>
        </svg>
        <svg class="w-4 h-4 shrink-0 text-amber-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/>
        </svg>
        <span class="{depth === 0 ? 'font-medium' : ''}">{getDisplayName(item)}</span>
      </button>
      {#if expandedPaths[item.path || item.name]}
        <div class="ml-6 border-l-2 border-[#E8E4E0] pl-3 space-y-0.5">
          {@render treeNode(expandedPaths[item.path || item.name], depth + 1)}
        </div>
      {/if}
    {:else}
      <button
        on:click={() => readLeaf(item)}
        class="w-full flex items-center gap-2 px-3 text-left text-sm transition-colors duration-150
               {depth === 0 ? 'py-2' : 'py-1.5'}
               {selectedPath === (item.path || item.name)
                 ? 'bg-[#FEF3EC] text-[#F07030]'
                 : (depth === 0 ? 'text-[#1A1A1A]' : 'text-[#6B6560]') + ' hover:bg-[#F7F4F0]'}"
      >
        <svg class="w-4 h-4 shrink-0 text-[#9B9590]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/>
        </svg>
        <span class="{depth === 0 ? 'font-medium' : ''}">{getDisplayName(item)}</span>
      </button>
    {/if}
  {/each}
{/snippet}

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Viking Browser</h2>
    <p class="text-[#9B9590] text-sm mt-1">Browse the Viking context database</p>
  </div>

  {#if loading}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading Viking tree...</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">Viking browser not available</p>
    </div>
  {:else}
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
      <!-- Tree view -->
      <div class="bg-white border-2 border-[#1A1A1A] p-5 max-h-[600px] overflow-y-auto">
        <h3 class="text-sm font-semibold text-[#1A1A1A] mb-4">File Tree</h3>
        {#if tree.length === 0}
          <p class="text-[#9B9590] text-sm text-center py-8">No entries found at viking://</p>
        {:else}
          <div class="space-y-0.5">
            {@render treeNode(tree, 0)}
          </div>
        {/if}
      </div>

      <!-- Content view -->
      <div class="bg-white border-2 border-[#1A1A1A] p-5 max-h-[600px] overflow-y-auto">
        <h3 class="text-sm font-semibold text-[#1A1A1A] mb-4">
          {#if selectedPath}
            <span class="font-mono text-xs text-[#9B9590]">{selectedPath}</span>
          {:else}
            Content
          {/if}
        </h3>

        {#if contentLoading}
          <p class="text-[#9B9590] text-sm text-center py-8 animate-pulse">Loading...</p>
        {:else if selectedContent}
          {#if selectedContent.error}
            <p class="text-[#DC2626] text-sm">{selectedContent.error}</p>
          {:else}
            <pre class="bg-[#1A1A1A] text-[#E8E4E0] border-2 border-[#1A1A1A] p-4 text-xs font-mono whitespace-pre-wrap overflow-x-auto">{JSON.stringify(selectedContent, null, 2)}</pre>
          {/if}
        {:else}
          <p class="text-[#9B9590] text-sm text-center py-8">Select a file from the tree to view its contents</p>
        {/if}
      </div>
    </div>
  {/if}
</div>
