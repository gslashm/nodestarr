<!-- vzdump backups on a Proxmox storage, newest first, with the guest each belongs to. -->
<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive';
  import Lock from '@lucide/svelte/icons/lock';
  import EmptyState from '../../components/EmptyState.svelte';
  import Meter from '../../components/Meter.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { bytes, dateTime, plural } from '../../lib/format';
  import { session } from '../../lib/session.svelte';
  import type { PveBackup, PveOverview } from '../../lib/types';

  let { overview }: { overview: PveOverview; selfVmid: number | null; reload: () => Promise<void> } = $props();

  const storages = $derived(overview.storages.filter((s) => s.content.includes('backup')));
  let storage = $state('');
  let backups = $state<PveBackup[] | null>(null);
  let error = $state('');
  let guest = $state('');

  // Start on the configured backup storage, else one dedicated to backups
  // (like a "backups" directory) rather than "local".
  $effect(() => {
    if (!storage && storages.length) {
      const preferred = session.settings?.proxmox.backup_storage;
      storage = (
        storages.find((s) => s.storage === preferred) ??
        storages.find((s) => s.content.length === 1) ??
        storages[0]
      ).storage;
    }
  });

  $effect(() => {
    const s = storage;
    backups = null;
    error = '';
    if (!s) return;
    api
      .get<{ backups: PveBackup[] }>(`/proxmox/storage/${encodeURIComponent(s)}/backups`)
      .then((r) => {
        if (s === storage) backups = r.backups.sort((a, b) => (b.ctime ?? 0) - (a.ctime ?? 0));
      })
      .catch((err) => (error = errorMessage(err)));
  });

  const names = $derived(new Map(overview.guests.map((g) => [g.vmid, g.name])));
  const shown = $derived((backups ?? []).filter((b) => !guest || String(b.vmid) === guest));
  const current = $derived(storages.find((s) => s.storage === storage));
  const vmids = $derived([...new Set((backups ?? []).map((b) => b.vmid).filter((v): v is number => v !== null))].sort((a, b) => a - b));

  function fileName(volid: string): string {
    return volid.split('/').pop() ?? volid;
  }
</script>

{#if storages.length === 0}
  <EmptyState icon={Archive} title="No backup storage">Enable the "VZDump backup file" content type on a storage in Proxmox to keep guest backups.</EmptyState>
{:else}
  <div class="bar">
    <select class="input" bind:value={storage} aria-label="Storage">
      {#each storages as s (s.storage)}<option value={s.storage}>{s.storage} ({s.kind})</option>{/each}
    </select>
    {#if vmids.length > 1}
      <select class="input" bind:value={guest} aria-label="Guest">
        <option value="">Every guest</option>
        {#each vmids as id (id)}<option value={String(id)}>{id} {names.get(id) ?? ''}</option>{/each}
      </select>
    {/if}
    {#if current}
      <div class="usage">
        <Meter value={current.used} max={current.total} label="Storage used" />
        <span class="small muted">{bytes(current.total - current.used)} free of {bytes(current.total)}</span>
      </div>
    {/if}
  </div>

  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !backups}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else if shown.length === 0}
    <p class="muted">No backups on {storage}.</p>
  {:else}
    <section class="panel">
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr><th>When</th><th>Guest</th><th class="num">Size</th><th>Notes</th><th class="hide-md">File</th></tr>
          </thead>
          <tbody>
            {#each shown as b (b.volid)}
              <tr>
                <td class="shrink num">{dateTime(b.ctime)}</td>
                <td>{b.vmid ?? '?'} <span class="muted">{b.vmid !== null ? (names.get(b.vmid) ?? '') : ''}</span>{#if b.protected}<span class="lock" title="Protected"><Lock size={13} /></span>{/if}</td>
                <td class="num">{bytes(b.size)}</td>
                <td class="small">{b.notes ?? ''}</td>
                <td class="hide-md mono small faint ellipsis file" title={b.volid}>{fileName(b.volid)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
    <p class="faint small foot">{plural(shown.length, 'backup')}, {bytes(shown.reduce((a, b) => a + (b.size ?? 0), 0))}. Restore guests from the Proxmox interface.</p>
  {/if}
{/if}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
  }
  .bar select {
    width: auto;
    min-width: 180px;
  }
  .usage {
    display: grid;
    gap: 4px;
    min-width: 220px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .lock {
    margin-left: 6px;
    color: var(--accent-text);
    vertical-align: -2px;
  }
  .file {
    max-width: 320px;
  }
  .foot {
    margin-top: 8px;
  }
  @media (max-width: 1000px) {
    .hide-md {
      display: none;
    }
  }
</style>
