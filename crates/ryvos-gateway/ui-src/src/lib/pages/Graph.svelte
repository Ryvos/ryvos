<script>
  import { onMount } from 'svelte';
  import { apiFetch } from '../api.js';

  let jobs = [];
  let loading = true;
  let error = '';
  let showAddForm = false;
  let newJob = { name: '', schedule: '', prompt: '', channel: '', goal: '' };
  let saving = false;
  let message = '';

  onMount(async () => {
    await loadJobs();
  });

  async function loadJobs() {
    loading = true;
    try {
      const data = await apiFetch('/api/cron');
      jobs = data.jobs || [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function addJob() {
    if (!newJob.name || !newJob.schedule || !newJob.prompt) return;
    saving = true;
    message = '';
    try {
      const body = { ...newJob };
      if (!body.channel) delete body.channel;
      if (!body.goal) delete body.goal;
      const res = await apiFetch('/api/cron', { method: 'POST', body: JSON.stringify(body) });
      if (res.ok) {
        message = res.note || 'Job added';
        newJob = { name: '', schedule: '', prompt: '', channel: '', goal: '' };
        showAddForm = false;
        await loadJobs();
      } else {
        message = res.error || 'Failed to add job';
      }
    } catch (e) {
      message = 'Error: ' + e.message;
    } finally {
      saving = false;
    }
  }

  async function deleteJob(name) {
    try {
      const res = await apiFetch(`/api/cron/${encodeURIComponent(name)}`, { method: 'DELETE' });
      message = res.note || 'Job removed';
      await loadJobs();
    } catch (e) {
      message = 'Error: ' + e.message;
    }
  }

  function truncate(s, max) {
    if (!s) return '';
    return s.length > max ? s.substring(0, max) + '...' : s;
  }
</script>

<div>
  <div class="mb-7 flex items-end justify-between">
    <div>
      <h2 class="text-2xl font-heading font-bold tracking-tight text-[#1A1A1A]">Cron Jobs</h2>
      <p class="text-[#9B9590] text-sm mt-1">Scheduled agent tasks</p>
    </div>
    <button
      on:click={() => showAddForm = !showAddForm}
      class="px-4 py-2 bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
             uppercase font-bold tracking-wider text-xs transition-all duration-100"
    >
      {showAddForm ? 'Cancel' : 'Add Job'}
    </button>
  </div>

  <!-- Add Job Form -->
  {#if showAddForm}
    <div class="bg-white border-2 border-[#1A1A1A] p-6 mb-5">
      <h3 class="text-sm font-heading font-bold text-[#1A1A1A] mb-4">New Cron Job</h3>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div>
          <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Name</span>
          <input
            bind:value={newJob.name}
            placeholder="daily-summary"
            class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A]
                   outline-none focus:border-[#F07030] transition-colors"
          />
        </div>
        <div>
          <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Schedule</span>
          <input
            bind:value={newJob.schedule}
            placeholder="0 8 * * *"
            class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A] font-mono
                   outline-none focus:border-[#F07030] transition-colors"
          />
        </div>
        <div class="md:col-span-2">
          <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Prompt</span>
          <textarea
            bind:value={newJob.prompt}
            placeholder="Summarize my unread emails and create a daily briefing..."
            rows="3"
            class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A]
                   outline-none resize-none focus:border-[#F07030] transition-colors"
          ></textarea>
        </div>
        <div>
          <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Channel <span class="text-[#9B9590] normal-case font-normal">(optional)</span></span>
          <input
            bind:value={newJob.channel}
            placeholder="telegram"
            class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A]
                   outline-none focus:border-[#F07030] transition-colors"
          />
        </div>
        <div>
          <span class="label text-xs uppercase tracking-wider font-bold text-[#9B9590] block mb-1">Goal <span class="text-[#9B9590] normal-case font-normal">(optional)</span></span>
          <input
            bind:value={newJob.goal}
            placeholder="Keep me informed"
            class="w-full px-3 py-2 bg-white border-2 border-[#1A1A1A] text-sm text-[#1A1A1A]
                   outline-none focus:border-[#F07030] transition-colors"
          />
        </div>
      </div>
      <div class="flex items-center gap-3 mt-5">
        <button
          on:click={addJob}
          disabled={saving || !newJob.name || !newJob.schedule || !newJob.prompt}
          class="px-5 py-2 bg-[#F07030] text-white border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                 uppercase font-bold tracking-wider text-xs transition-all duration-100
                 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {saving ? 'Saving...' : 'Save Job'}
        </button>
        <button
          on:click={() => showAddForm = false}
          class="px-5 py-2 bg-white text-[#6B6560] border-2 border-[#1A1A1A] shadow-brutal-sm brutal-shift
                 uppercase font-bold tracking-wider text-xs transition-all duration-100 hover:text-[#1A1A1A]"
        >
          Cancel
        </button>
      </div>
    </div>
  {/if}

  <!-- Jobs Table -->
  {#if loading}
    <div class="bg-white border-2 border-[#1A1A1A] p-8 text-center">
      <p class="text-[#9B9590] text-sm animate-pulse">Loading cron jobs...</p>
    </div>
  {:else if error}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <p class="text-[#9B9590] text-sm">{error}</p>
    </div>
  {:else if jobs.length === 0}
    <div class="bg-white border-2 border-[#1A1A1A] p-12 text-center">
      <svg class="mx-auto mb-4 text-[#9B9590]" width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10"/>
        <polyline points="12 6 12 12 16 14"/>
      </svg>
      <h3 class="text-lg font-semibold text-[#6B6560] mb-2">No cron jobs configured</h3>
      <p class="text-sm text-[#9B9590] max-w-md mx-auto">
        Add a cron job to schedule recurring agent tasks. Jobs run on a cron schedule and execute the given prompt.
      </p>
    </div>
  {:else}
    <div class="border-2 border-[#1A1A1A] overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr>
            {#each ['Name', 'Schedule', 'Prompt', 'Channel', 'Goal', 'Actions'] as col}
              <th class="px-4 py-3 bg-[#F7F4F0] text-left text-xs uppercase tracking-wider font-bold text-[#9B9590] border-b-2 border-[#1A1A1A] sticky top-0">
                {col}
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each jobs as job}
            <tr class="hover:bg-[#F7F4F0] transition-colors duration-150">
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-semibold text-[#1A1A1A]">{job.name || '-'}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] font-mono text-xs text-[#6B6560]">{job.schedule || '-'}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#6B6560] max-w-[200px]" title={job.prompt || ''}>{truncate(job.prompt || '', 50)}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#9B9590]">{job.channel || '-'}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0] text-[#9B9590]">{job.goal || '-'}</td>
              <td class="px-4 py-3 border-b border-[#E8E4E0]">
                <button
                  on:click={() => deleteJob(job.name)}
                  class="px-3 py-1 bg-white text-[#DC2626] border-2 border-[#DC2626] text-xs font-bold uppercase tracking-wider
                         hover:bg-[#DC2626] hover:text-white transition-all duration-100"
                >
                  Delete
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <!-- Feedback message -->
  {#if message}
    <div class="mt-4 px-4 py-3 bg-[#FEF3EC] border-2 border-[#F07030] text-sm text-[#1A1A1A] flex items-center justify-between">
      <span>{message}</span>
      <button on:click={() => message = ''} class="text-[#9B9590] hover:text-[#1A1A1A] text-xs font-bold uppercase">Dismiss</button>
    </div>
  {/if}

  <!-- Restart note -->
  {#if message && (message.includes('added') || message.includes('removed'))}
    <div class="mt-3 px-4 py-2 bg-[#FFFBEB] border-2 border-[#D97706] text-xs text-[#D97706] font-semibold">
      Restart required for cron changes to take effect.
    </div>
  {/if}
</div>
