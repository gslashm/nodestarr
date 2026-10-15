<!-- Scheduled jobs: when each runs next, when it last ran, and a button to run it now. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Play from '@lucide/svelte/icons/play';
  import Button from '../../components/Button.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { dateTime, duration } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import { ui } from '../../lib/ui.svelte';

  interface Job {
    job: string;
    name: string;
    enabled: boolean;
    next_run: number | null;
    last_run: { started_at: number; finished_at: number | null } | null;
  }

  let jobs = $state<Job[] | null>(null);
  let error = $state('');

  const ABOUT: Record<string, { text: string; settings?: string }> = {
    'update-check': { text: 'Compares every image with its registry.', settings: '/settings/updates' },
    'auto-update': { text: 'Installs updates for containers set to update automatically.', settings: '/settings/updates' },
    backup: { text: 'Backs up every included container, then runs the after-backup hook.', settings: '/settings/backups' },
    maintenance: { text: 'Prunes old history, logs and expired backups; tidies the database.' },
    'berth-update': { text: 'Looks for a new version of Berth.', settings: '/settings/general' },
  };

  async function load() {
    try {
      jobs = (await api.get<{ jobs: Job[] }>('/system/jobs')).jobs;
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(() => {
    void load();
    const t = setInterval(() => {
      if (!document.hidden) void load();
    }, 10_000);
    return () => clearInterval(t);
  });

  async function run(j: Job) {
    try {
      await api.post(`/system/jobs/${j.job}/run`);
      ui.toast(`Started the ${j.name}`);
      setTimeout(load, 800);
    } catch (err) {
      ui.error(err);
    }
  }
</script>

{#if error}
  <p class="text-bad">{error}</p>
{:else if !jobs}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else}
  <section class="panel">
    <ul class="jobs">
      {#each jobs as j (j.job)}
        {@const running = !!j.last_run && j.last_run.finished_at === null}
        <li>
          <div class="what">
            <span class="name">{j.name[0].toUpperCase() + j.name.slice(1)}</span>
            <span class="muted small">{ABOUT[j.job]?.text ?? ''}{#if ABOUT[j.job]?.settings} <a href={href(ABOUT[j.job].settings!)}>Settings</a>{/if}</span>
          </div>
          <div class="when small">
            <span>{#if !j.enabled}<span class="faint">Off</span>{:else if j.next_run}Next {dateTime(j.next_run)}{:else}<span class="faint">No next run</span>{/if}</span>
            <span class="faint">
              {#if running}<span class="text-update">Running now</span>{:else if j.last_run}Last ran <TimeAgo ts={j.last_run.started_at} />{#if j.last_run.finished_at}, took {duration(j.last_run.finished_at - j.last_run.started_at)}{/if}{:else}Not run since Berth started{/if}
            </span>
          </div>
          {#if session.isAdmin}
            <Button small variant="ghost" icon={Play} onclick={() => run(j)} disabled={running}>Run now</Button>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .jobs {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .jobs li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 20px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--line);
  }
  .jobs li:last-child {
    border-bottom: 0;
  }
  .what {
    flex: 1 1 320px;
    display: grid;
    gap: 2px;
  }
  .name {
    font-weight: 500;
  }
  .when {
    display: grid;
    gap: 2px;
    min-width: 220px;
  }
  .small {
    font-size: var(--fs-sm);
  }
</style>
