<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let healthData = null;
  let metricsData = null;
  let loading = true;

  // Budget editing
  let budgetData = null;
  let budgetLoading = false;
  let budgetEditing = false;
  let editBudgetDollars = '';
  let editWarnPct = '';
  let budgetSaving = false;
  let budgetMessage = '';

  // Model editing
  let modelData = null;
  let availableModels = [];
  let modelLoading = false;
  let modelEditing = false;
  let editModelId = '';
  let editTemperature = 0.7;
  let editMaxTokens = 4096;
  let editThinking = 'off';
  let modelSaving = false;
  let modelMessage = '';

  onMount(async () => {
    try {
      const [health, metrics] = await Promise.all([
        apiFetch('/api/health').catch(() => null),
        apiFetch('/api/metrics').catch(() => null),
      ]);
      healthData = health;
      metricsData = metrics;
    } finally {
      loading = false;
    }
    // Load budget and model in parallel
    loadBudget();
    loadModel();
  });

  async function loadBudget() {
    budgetLoading = true;
    try {
      budgetData = await apiFetch('/api/budget');
      editBudgetDollars = budgetData.monthly_budget_cents ? (budgetData.monthly_budget_cents / 100).toFixed(2) : '';
      editWarnPct = budgetData.warn_pct != null ? String(budgetData.warn_pct) : '80';
    } catch {
      budgetData = null;
    } finally {
      budgetLoading = false;
    }
  }

  async function saveBudget() {
    budgetSaving = true;
    budgetMessage = '';
    try {
      const cents = Math.round(parseFloat(editBudgetDollars) * 100);
      const warnPct = parseInt(editWarnPct) || 80;
      await apiFetch('/api/budget', {
        method: 'PUT',
        body: JSON.stringify({ monthly_budget_cents: cents, warn_pct: warnPct }),
      });
      budgetMessage = 'Budget saved. Restart required for changes to take effect.';
      budgetEditing = false;
      await loadBudget();
    } catch (e) {
      budgetMessage = 'Error: ' + e.message;
    } finally {
      budgetSaving = false;
    }
  }

  async function loadModel() {
    modelLoading = true;
    try {
      const [current, available] = await Promise.all([
        apiFetch('/api/model').catch(() => null),
        apiFetch('/api/models/available').catch(() => ({ models: [] })),
      ]);
      modelData = current;
      availableModels = available.models || available || [];
      if (modelData) {
        editModelId = modelData.model_id || '';
        editTemperature = modelData.temperature != null ? modelData.temperature : 0.7;
        editMaxTokens = modelData.max_tokens || 4096;
        editThinking = modelData.thinking || 'off';
      }
    } catch {
      modelData = null;
    } finally {
      modelLoading = false;
    }
  }

  async function saveModel() {
    modelSaving = true;
    modelMessage = '';
    try {
      await apiFetch('/api/model', {
        method: 'PUT',
        body: JSON.stringify({
          model_id: editModelId,
          temperature: parseFloat(editTemperature),
          max_tokens: parseInt(editMaxTokens),
          thinking: editThinking,
        }),
      });
      modelMessage = 'Model saved. Restart required for changes to take effect.';
      modelEditing = false;
      await loadModel();
    } catch (e) {
      modelMessage = 'Error: ' + e.message;
    } finally {
      modelSaving = false;
    }
  }

  $: budgetPct = metricsData && metricsData.monthly_budget_cents > 0
    ? Math.min(metricsData.budget_utilization_pct, 100) : 0;
  $: budgetColor = budgetPct > 90 ? 'text-[#DC2626]' : budgetPct > 70 ? 'text-amber-500' : 'text-[#F07030]';
  $: barColor = budgetPct > 90 ? 'bg-[#DC2626]' : budgetPct > 70 ? 'bg-amber-500' : 'bg-[#F07030]';
</script>

<div>
  <div class="mb-7">
    <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Settings</h2>
    <p class="text-[#9B9590] text-sm mt-1">System info, budget, and model configuration</p>
  </div>

  {#if loading}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      {#each Array(2) as _}
        <div class="bg-white border-2 border-[#1A1A1A] p-6 min-h-[160px] animate-pulse"></div>
      {/each}
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      <!-- System card -->
      <div class="bg-white border-2 border-[#1A1A1A] p-6">
        <h3 class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] mb-5">System</h3>
        {#if healthData}
          <div class="space-y-4">
            <div>
              <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Version</span>
              <p class="text-lg font-bold text-[#1A1A1A] mt-1">{healthData.version || 'unknown'}</p>
            </div>
            <div>
              <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Status</span>
              <p class="text-lg font-bold text-[#16A34A] mt-1">{healthData.status || 'unknown'}</p>
            </div>
          </div>
        {:else}
          <p class="text-[#9B9590] text-sm">Failed to load system info</p>
        {/if}
      </div>

      <!-- Budget display card (from metrics) -->
      <div class="bg-white border-2 border-[#1A1A1A] p-6">
        <h3 class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] mb-5">Budget Usage</h3>
        {#if metricsData && metricsData.monthly_budget_cents > 0}
          <div class="space-y-4">
            <div>
              <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Monthly Budget</span>
              <p class="text-lg font-bold text-[#1A1A1A] mt-1">${(metricsData.monthly_budget_cents / 100).toFixed(2)}</p>
            </div>
            <div>
              <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Spent</span>
              <p class="text-lg font-bold text-[#1A1A1A] mt-1">${(metricsData.total_cost_cents / 100).toFixed(2)}</p>
            </div>
            <div>
              <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Utilization</span>
              <p class="text-lg font-bold {budgetColor} mt-1">{metricsData.budget_utilization_pct}%</p>
            </div>
            <!-- Progress bar -->
            <div class="h-2 bg-[#F7F4F0] border border-[#1A1A1A] overflow-hidden">
              <div class="{barColor} h-full transition-all duration-500" style="width: {budgetPct}%"></div>
            </div>
          </div>
        {:else}
          <p class="text-[#9B9590] text-sm py-4">
            No budget configured. Add <code class="font-mono bg-[#F7F4F0] border border-[#E8E4E0] px-1.5 py-0.5 text-xs">[budget]</code> to your config.toml.
          </p>
        {/if}
      </div>
    </div>

    <!-- Budget Configuration Section -->
    <div class="mt-5 bg-white border-2 border-[#1A1A1A] p-6">
      <div class="flex items-center justify-between mb-5">
        <h3 class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Budget Configuration</h3>
        {#if !budgetEditing}
          <button
            on:click={() => budgetEditing = true}
            class="px-3 py-1.5 bg-white text-[#6B6560] border-2 border-[#1A1A1A] text-xs font-bold uppercase tracking-wider
                   shadow-brutal-sm brutal-shift hover:text-[#1A1A1A] transition-all duration-100"
          >
            Edit
          </button>
        {/if}
      </div>

      {#if budgetLoading}
        <p class="text-[#9B9590] text-sm animate-pulse">Loading budget...</p>
      {:else if budgetEditing}
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Monthly Budget ($)</span>
            <input
              bind:value={editBudgetDollars}
              type="number"
              step="0.01"
              min="0"
              placeholder="10.00"
              class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A] font-mono
                     outline-none focus:border-[#F07030] transition-colors"
            />
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Warn at (%)</span>
            <input
              bind:value={editWarnPct}
              type="number"
              min="0"
              max="100"
              placeholder="80"
              class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A] font-mono
                     outline-none focus:border-[#F07030] transition-colors"
            />
          </div>
        </div>
        <div class="flex items-center gap-3 mt-4">
          <button
            on:click={saveBudget}
            disabled={budgetSaving}
            class="px-5 py-2 bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                   uppercase font-bold tracking-wider text-xs transition-all duration-100
                   disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {budgetSaving ? 'Saving...' : 'Save Budget'}
          </button>
          <button
            on:click={() => budgetEditing = false}
            class="px-5 py-2 bg-white text-[#6B6560] border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                   uppercase font-bold tracking-wider text-xs transition-all duration-100 hover:text-[#1A1A1A]"
          >
            Cancel
          </button>
        </div>
      {:else if budgetData}
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Monthly Budget</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1">
              {budgetData.monthly_budget_cents ? '$' + (budgetData.monthly_budget_cents / 100).toFixed(2) : 'Not set'}
            </p>
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Warn Threshold</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1">{budgetData.warn_pct != null ? budgetData.warn_pct + '%' : '80%'}</p>
          </div>
        </div>
      {:else}
        <p class="text-[#9B9590] text-sm">Budget endpoint not available</p>
      {/if}

      {#if budgetMessage}
        <div class="mt-4 px-4 py-3 bg-[#FEF3EC] border-2 border-[#F07030] text-sm text-[#1A1A1A]">
          {budgetMessage}
        </div>
      {/if}
    </div>

    <!-- Model Configuration Section -->
    <div class="mt-5 bg-white border-2 border-[#1A1A1A] p-6">
      <div class="flex items-center justify-between mb-5">
        <h3 class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Model Configuration</h3>
        {#if !modelEditing}
          <button
            on:click={() => modelEditing = true}
            class="px-3 py-1.5 bg-white text-[#6B6560] border-2 border-[#1A1A1A] text-xs font-bold uppercase tracking-wider
                   shadow-brutal-sm brutal-shift hover:text-[#1A1A1A] transition-all duration-100"
          >
            Edit
          </button>
        {/if}
      </div>

      {#if modelLoading}
        <p class="text-[#9B9590] text-sm animate-pulse">Loading model config...</p>
      {:else if modelEditing}
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div class="md:col-span-2">
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Model</span>
            {#if availableModels.length > 0}
              <select
                bind:value={editModelId}
                class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A]
                       outline-none focus:border-[#F07030] transition-colors appearance-none"
              >
                {#each availableModels as model}
                  <option value={typeof model === 'string' ? model : model.id}>
                    {typeof model === 'string' ? model : (model.name || model.id)}
                  </option>
                {/each}
              </select>
            {:else}
              <input
                bind:value={editModelId}
                placeholder="claude-sonnet-4-20250514"
                class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A] font-mono
                       outline-none focus:border-[#F07030] transition-colors"
              />
            {/if}
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Temperature ({editTemperature})</span>
            <input
              bind:value={editTemperature}
              type="range"
              min="0"
              max="1"
              step="0.1"
              class="w-full accent-[#F07030]"
            />
            <div class="flex justify-between text-[0.6rem] text-[#9B9590] mt-1">
              <span>0.0</span>
              <span>1.0</span>
            </div>
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Max Tokens</span>
            <input
              bind:value={editMaxTokens}
              type="number"
              min="1"
              max="200000"
              placeholder="4096"
              class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A] font-mono
                     outline-none focus:border-[#F07030] transition-colors"
            />
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Thinking</span>
            <select
              bind:value={editThinking}
              class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A]
                     outline-none focus:border-[#F07030] transition-colors appearance-none"
            >
              <option value="off">Off</option>
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          </div>
        </div>
        <div class="flex items-center gap-3 mt-4">
          <button
            on:click={saveModel}
            disabled={modelSaving}
            class="px-5 py-2 bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                   uppercase font-bold tracking-wider text-xs transition-all duration-100
                   disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {modelSaving ? 'Saving...' : 'Save Model'}
          </button>
          <button
            on:click={() => modelEditing = false}
            class="px-5 py-2 bg-white text-[#6B6560] border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                   uppercase font-bold tracking-wider text-xs transition-all duration-100 hover:text-[#1A1A1A]"
          >
            Cancel
          </button>
        </div>
      {:else if modelData}
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Provider</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1">{modelData.provider || 'unknown'}</p>
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Model</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1 font-mono text-sm">{modelData.model_id || 'unknown'}</p>
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Temperature</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1">{modelData.temperature != null ? modelData.temperature : '-'}</p>
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Max Tokens</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1">{modelData.max_tokens || '-'}</p>
          </div>
          <div>
            <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590]">Thinking</span>
            <p class="text-lg font-bold text-[#1A1A1A] mt-1 capitalize">{modelData.thinking || 'off'}</p>
          </div>
        </div>
      {:else}
        <p class="text-[#9B9590] text-sm">Model endpoint not available</p>
      {/if}

      {#if modelMessage}
        <div class="mt-4 px-4 py-3 bg-[#FEF3EC] border-2 border-[#F07030] text-sm text-[#1A1A1A]">
          {modelMessage}
        </div>
      {/if}
    </div>
  {/if}
</div>
