<!--
  Backups: where they go and how much room is left, the schedule, which
  containers are covered and when each was last backed up, and every
  archive with restore, download and cleanup.
-->
<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive';
  import FolderSearch from '@lucide/svelte/icons/folder-search';
  import Play from '@lucide/svelte/icons/play';
  import Settings from '@lucide/svelte/icons/settings';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import AppIcon from '../components/AppIcon.svelte';
  import BackupTable from '../components/BackupTable.svelte';
  import Button from '../components/Button.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import Menu from '../components/Menu.svelte';
  import Meter from '../components/Meter.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import Spinner from '../components/Spinner.svelte';
  import Tabs from '../components/Tabs.svelte';
  import TimeAgo from '../components/TimeAgo.svelte';
  import { backupNow } from '../lib/actions';
  import { clock } from '../lib/clock.svelte';
  import { api, errorMessage } from '../lib/api';
  import { bytes, dateTime, plural } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { BackupRecord } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params }: { params: Record<string, string> } = $props();

  interface ListResponse {
    backups: BackupRecord[];
    root: string;
    root_available: boolean;
    disk: { total: number; used: number; available: number } | null;
    total_size: number;
    schedule_enabled: boolean;
    next_run: number | null;
  }

  const tab = $derived(params.tab === 'archives' ? 'archives' : 'containers');
  let data = $state<ListResponse | null>(null);
  let error = $state('');
  let target = $state('');
  let kind = $state('');

  async function load() {
    try {
      data = await api.get<ListResponse>('/backups?limit=1000');
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  $effect(() => {
    void load();
  });

  // Reload when a backup or restore task finishes.
  let running = 0;
  $effect(() => {
    const n = live.activeTasks.filter((t) => t.kind === 'backup' || t.kind === 'restore').length;
    if (n < running) void load();
    running = n;
  });

  const STALE_DAYS = 7;

  const coverage = $derived(
    live.list
      .filter((c) => !c.prefs.hidden)
      .map((c) => ({ c, included: c.prefs.backup_enabled, age: c.last_backup ? clock.now - c.last_backup : null }))
      .sort((a, b) => Number(b.included) - Number(a.included) || (b.age ?? Infinity) - (a.age ?? Infinity)),
  );
  const stale = $derived(coverage.filter((x) => x.included && (x.age === null || x.age > STALE_DAYS * 86400)));
  const archives = $derived((data?.backups ?? []).filter((b) => (!target || b.identity === target) && (!kind || b.kind === kind)));
  const targets = $derived(
    [...new Map((data?.backups ?? []).map((b) => [b.identity, b.target_name ?? b.identity])).entries()].sort((a, b) => a[1].localeCompare(b[1])),
  );
  const s = $derived(session.settings?.backups);

  const tabList = $derived([
    { key: 'containers', label: 'Containers', path: '/backups', badge: (data?.schedule_enabled && stale.length) || null, tone: 'warn' as const },
    { key: 'archives', label: 'Archives', path: '/backups/archives', badge: data?.backups.length || null },
  ]);

  async function runScheduled() {
    try {
      await api.post('/backups/scheduled/run');
      ui.toast('Backing up every included container now');
    } catch (err) {
      ui.error(err);
    }
  }

  async function reindex() {
    try {
      const r = await api.post<{ indexed: number }>('/backups/reindex');
      ui.toast(`Found ${plural(r.indexed, 'archive')} in the backup folder`, 'success');
      await load();
    } catch (err) {
      ui.error(err);
    }
  }
</script>

<PageHeader
  title="Backups"
  subtitle={data ? `${plural(data.backups.length, 'backup')}, ${bytes(data.total_size)}${data.next_run ? `, next run ${dateTime(data.next_run)}` : ''}` : ''}
>
  {#snippet actions()}
    {#if session.isAdmin}
      <Button variant="primary" icon={Play} onclick={runScheduled}>Back up all now</Button>
      <Menu
        label="More"
        items={[
          { label: 'Rescan the backup folder', icon: FolderSearch, run: reindex },
          { label: 'Backup settings', icon: Settings, href: href('/settings/backups') },
        ]}
      />
    {/if}
  {/snippet}
  {#snippet tabs()}
    <Tabs tabs={tabList} active={tab} />
  {/snippet}
</PageHeader>

<div class="page-body">
  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !data}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else}
    {#if !data.root_available}
      <div class="banner">
        <TriangleAlert size={18} />
        <span>The backup folder <code>{data.root}</code> isn't available, so backups can't be written. Check that it's mounted, or change it in <a href={href('/settings/backups')}>Settings</a>.</span>
      </div>
    {/if}

    <div class="cards">
      <section class="panel card">
        <h3>Storage</h3>
        <p class="mono small path" title={data.root}>{data.root}</p>
        {#if data.disk}
          <Meter value={data.disk.used} max={data.disk.total} warn={85} bad={95} label="Disk use of the backup folder" />
          <p class="small muted">{bytes(data.disk.available)} free of {bytes(data.disk.total)}; backups use {bytes(data.total_size)}</p>
        {/if}
      </section>
      <section class="panel card">
        <h3>Schedule</h3>
        {#if data.schedule_enabled && s}
          <p class="big num">{data.next_run ? dateTime(data.next_run) : '–'}</p>
          <p class="small muted">Next run, on the schedule <code>{s.cron}</code></p>
        {:else}
          <p class="big muted">Off</p>
          <p class="small muted">Only manual and pre-update backups are made. <a href={href('/settings/backups')}>Turn on</a></p>
        {/if}
      </section>
      <section class="panel card">
        <h3>Kept</h3>
        {#if s}
          <p class="big num">{s.retention_count ? `${s.retention_count} per container` : 'All of them'}</p>
          <p class="small muted">
            {s.retention_days ? `and at most ${s.retention_days} days old` : 'regardless of age'}; pre-update backups for {s.pre_update_retention_days} days. Pinned backups are always kept.
          </p>
        {/if}
      </section>
    </div>

    {#if tab === 'containers'}
      {#if stale.length && data.schedule_enabled}
        <p class="stale"><TriangleAlert size={15} /> {plural(stale.length, 'container')} included in backups {stale.length === 1 ? "hasn't" : "haven't"} been backed up in the last {STALE_DAYS} days.</p>
      {/if}
      <section class="panel">
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                <th>Container</th>
                <th>{data.schedule_enabled ? 'Scheduled' : 'Included'}</th>
                <th>Last backup</th>
                <th class="shrink"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each coverage as { c, included, age } (c.id)}
                <tr>
                  <td>
                    <a class="who" href={href(`/containers/${encodeURIComponent(c.name)}/backups`)}>
                      <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={24} />
                      <span>{c.display_name}</span>
                      {#if c.stack}<span class="faint small">{c.stack}</span>{/if}
                    </a>
                  </td>
                  <td>{#if included}Included{:else}<span class="faint">Not included</span>{/if}</td>
                  <td class:text-warn={data.schedule_enabled && included && (age === null || age > STALE_DAYS * 86400)}>
                    {#if c.last_backup}<TimeAgo ts={c.last_backup} />{:else}never{/if}
                  </td>
                  <td class="shrink">
                    {#if session.isAdmin}
                      <Button small variant="ghost" icon={Archive} onclick={() => backupNow(c)} disabled={c.busy}>Back up</Button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {:else}
      <div class="filters">
        <select class="input" bind:value={target} aria-label="Container">
          <option value="">All containers</option>
          {#each targets as [id, label] (id)}<option value={id}>{label}</option>{/each}
        </select>
        <select class="input" bind:value={kind} aria-label="Type">
          <option value="">Every type</option>
          <option value="manual">By hand</option>
          <option value="scheduled">Scheduled</option>
          <option value="pre-update">Before update</option>
          <option value="pre-restore">Before restore</option>
        </select>
        <span class="faint small">{plural(archives.length, 'archive')}, {bytes(archives.reduce((a, b) => a + (b.exists ? b.size : 0), 0))}</span>
      </div>
      <section class="panel">
        {#if archives.length}
          <BackupTable backups={archives} showTarget onchanged={load} />
        {:else}
          <EmptyState icon={Archive} title={data.backups.length ? 'No backups match' : 'No backups yet'}>
            {data.backups.length ? 'Try other filters.' : 'Back up a container, or turn on scheduled backups.'}
          </EmptyState>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  .banner {
    display: flex;
    gap: 10px;
    margin-bottom: 16px;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--bad) 40%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
  }
  .banner :global(svg) {
    flex: none;
    color: var(--bad);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 14px;
    margin-bottom: 18px;
  }
  .card {
    display: grid;
    align-content: start;
    gap: 6px;
    padding: 14px 16px;
  }
  .card h3 {
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .card p {
    margin: 0;
  }
  .big {
    font: 600 var(--fs-lg) / 1.3 var(--font-data);
  }
  .small {
    font-size: var(--fs-sm);
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stale {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 12px;
    color: var(--warn);
  }
  .who {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
  }
  .filters .input {
    width: auto;
    min-width: 180px;
  }
</style>
