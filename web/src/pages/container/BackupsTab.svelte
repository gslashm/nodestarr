<!--
  Backups of one container: what a backup would include (and why), a
  "back up now" button, and the existing archives.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Archive from '@lucide/svelte/icons/archive';
  import Check from '@lucide/svelte/icons/check';
  import Minus from '@lucide/svelte/icons/minus';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import BackupTable from '../../components/BackupTable.svelte';
  import Button from '../../components/Button.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { backupNow } from '../../lib/actions';
  import { api, errorMessage, seg } from '../../lib/api';
  import { bytes } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { BackupPlan, BackupRecord, ContainerDetail, ContainerView } from '../../lib/types';

  let { c }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  let plan = $state<{ plan: BackupPlan; sizes: Record<string, number>; root_available: boolean } | null>(null);
  let planError = $state('');
  let backups = $state<BackupRecord[] | null>(null);
  let listError = $state('');

  const METHOD: Record<string, { label: string; text: string }> = {
    live: { label: 'Copied while running', text: 'Files are copied while the app keeps running. Fine for apps without a database.' },
    pause: { label: 'Paused while copying', text: 'The container is paused for the few seconds the copy takes, so databases stay consistent.' },
    stop: { label: 'Stopped while copying', text: 'The container is stopped during the copy and started again afterwards.' },
    native: { label: "The app's own backup", text: 'The app makes the backup itself through its API, the way it recommends.' },
    dump: { label: 'Database dump', text: 'The database is dumped with its own tool while it runs, plus any included folders.' },
  };

  async function loadPlan() {
    try {
      plan = await api.get(`/containers/${seg(c.id)}/backup-plan`);
      planError = '';
    } catch (err) {
      planError = errorMessage(err);
    }
  }

  async function loadList() {
    try {
      backups = await api.get<BackupRecord[]>(`/containers/${seg(c.id)}/backups`);
      listError = '';
    } catch (err) {
      listError = errorMessage(err);
    }
  }

  onMount(() => {
    void loadPlan();
    void loadList();
  });

  // A finished backup or restore changes the list.
  let wasBusy = false;
  $effect(() => {
    const busy = c.busy;
    if (wasBusy && !busy) void loadList();
    wasBusy = busy;
  });

  const p = $derived(plan?.plan);
  const total = $derived(plan ? Object.values(plan.sizes).reduce((a, b) => a + b, 0) : 0);
  const settingsPath = $derived(`/containers/${encodeURIComponent(c.name)}/settings`);
</script>

<div class="cols">
  <section class="panel">
    <div class="panel-head">
      <h2>Archives</h2>
      {#if backups?.length}<span class="faint small">{backups.length} kept, {bytes(backups.reduce((a, b) => a + (b.exists ? b.size : 0), 0))}</span>{/if}
      {#if session.isAdmin}<Button variant="primary" small icon={Archive} onclick={() => backupNow(c)} disabled={c.busy}>Back up now</Button>{/if}
    </div>
    {#if listError}
      <p class="panel-body text-bad">{listError}</p>
    {:else if !backups}
      <p class="panel-body faint"><Spinner size={13} /> Loading…</p>
    {:else if backups.length === 0}
      <EmptyState icon={Archive} title="No backups yet">
        {#if p && !p.enabled}Backups are off for this container. <a href={href(settingsPath)}>Turn them on</a>{:else}Back it up now, or let the schedule take care of it.{/if}
      </EmptyState>
    {:else}
      <BackupTable {backups} onchanged={loadList} />
    {/if}
  </section>

  <section class="panel plan">
    <div class="panel-head">
      <h2>What gets backed up</h2>
      <a class="faint small" href={href(settingsPath)}>Change</a>
    </div>
    <div class="panel-body">
      {#if planError}
        <p class="text-bad">{planError}</p>
      {:else if !p || !plan}
        <p class="faint"><Spinner size={13} /> Working it out…</p>
      {:else}
        {#if !p.enabled}
          <p class="notice">Backups are turned off for this container, so the schedule skips it. You can still back it up by hand.</p>
        {/if}
        {#if !plan.root_available}
          <p class="notice bad"><TriangleAlert size={15} /> The backup folder isn't available. Check the path in <a href={href('/settings/backups')}>Settings</a>.</p>
        {/if}
        <dl class="kv">
          <dt>Method</dt>
          <dd>
            <strong>{METHOD[p.method]?.label ?? p.method}</strong>
            <span class="muted small">{METHOD[p.method]?.text ?? ''}</span>
          </dd>
          {#if p.db}
            <dt>Database</dt>
            <dd>{p.db.description}</dd>
          {/if}
          {#if total}
            <dt>Size</dt>
            <dd class="num">About {bytes(total)} before compression</dd>
          {/if}
        </dl>

        <h3 class="sub">Folders and volumes</h3>
        {#if p.mounts.length === 0}
          <p class="muted">Nothing mounted.</p>
        {:else}
          <ul class="mounts">
            {#each p.mounts as m (m.destination)}
              <li class:out={!m.include}>
                <span class="mark">{#if m.include}<Check size={15} />{:else}<Minus size={15} />{/if}</span>
                <span class="what">
                  <span class="mono">{m.destination}</span>
                  <span class="faint small ellipsis" title={m.source}>{m.source}</span>
                  <span class="small reason">{m.reason}</span>
                </span>
                {#if m.include && plan.sizes[m.destination] !== undefined}<span class="num faint small">{bytes(plan.sizes[m.destination])}</span>{/if}
              </li>
            {/each}
          </ul>
        {/if}

        {#if p.compose_files.length}
          <h3 class="sub">Compose files</h3>
          <ul class="plain">
            {#each p.compose_files as f (f)}<li class="mono small">{f}</li>{/each}
          </ul>
        {/if}

        {#if p.excludes.length}
          <h3 class="sub">Skipped patterns</h3>
          <p class="excludes">{#each p.excludes as x (x)}<code>{x}</code> {/each}</p>
        {/if}

        {#each p.warnings as w, i (i)}
          <p class="notice warn"><TriangleAlert size={15} /> {w}</p>
        {/each}
      {/if}
    </div>
  </section>
</div>

<style>
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(300px, 1fr);
    gap: 16px;
    align-items: start;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .plan .kv dd {
    display: grid;
    gap: 2px;
  }
  .sub {
    margin: 18px 0 8px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .mounts,
  .plain {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .plain {
    gap: 4px;
  }
  .mounts li {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: start;
  }
  .mark {
    padding-top: 2px;
    color: var(--ok);
  }
  .out .mark {
    color: var(--text-3);
  }
  .out .mono {
    color: var(--text-2);
  }
  .what {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .reason {
    color: var(--text-2);
  }
  .excludes {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0 0 12px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text-2);
  }
  .notice.bad {
    color: var(--bad);
  }
  .notice.warn {
    color: var(--warn);
    margin-top: 12px;
  }
  .notice :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  @media (max-width: 1100px) {
    .cols {
      grid-template-columns: 1fr;
    }
  }
</style>
