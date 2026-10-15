<!-- Lists what a backup archive holds: the manifest summary and its files. -->
<script lang="ts">
  import { api, errorMessage, seg } from '../lib/api';
  import { bytes, dateTime, plural } from '../lib/format';
  import type { BackupRecord } from '../lib/types';
  import Dialog from './Dialog.svelte';
  import Spinner from './Spinner.svelte';

  let { backup, onclose }: { backup: BackupRecord | null; onclose: () => void } = $props();

  let entries = $state<{ name: string; size: number }[] | null>(null);
  let error = $state('');
  let filter = $state('');

  $effect(() => {
    const b = backup;
    entries = null;
    error = '';
    filter = '';
    if (!b) return;
    api
      .get<{ entries: { name: string; size: number }[] }>(`/backups/${seg(b.id)}/contents`)
      .then((r) => {
        if (backup?.id === b.id) entries = r.entries;
      })
      .catch((err) => {
        if (backup?.id === b.id) error = errorMessage(err);
      });
  });

  const shown = $derived((entries ?? []).filter((e) => !filter || e.name.toLowerCase().includes(filter.toLowerCase())));
  const items = $derived(backup?.manifest.items ?? []);
</script>

<Dialog open={!!backup} title="Backup of {backup?.target_name ?? 'container'}" width="720px" {onclose}>
  {#if backup}
    <p class="muted meta">
      {dateTime(backup.created_at)}, {bytes(backup.size)}{backup.manifest.app_version ? `, version ${backup.manifest.app_version}` : ''}{backup.manifest.image ? `, image ${backup.manifest.image}` : ''}
    </p>
    {#if items.length}
      <ul class="items">
        {#each items as it, i (i)}
          <li>
            {#if it.type === 'mount'}
              <strong class="mono">{it.destination}</strong>
              <span class="faint">{it.source}, {it.files == null ? 'files' : plural(it.files, 'file')}, {bytes(it.bytes)}</span>
            {:else if it.type === 'database'}
              <strong>Database dump</strong> <span class="faint">{it.engine ?? ''}, {bytes(it.bytes)}</span>
            {:else if it.type === 'native'}
              <strong>App backup</strong> <span class="faint">made by the app itself, {bytes(it.bytes)}</span>
            {:else if it.type === 'compose_file'}
              <strong>Compose file</strong> <span class="faint mono">{it.original}</span>
            {:else}
              <strong>{it.type}</strong>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if backup.manifest.warnings?.length}
      <div class="warnings">
        {#each backup.manifest.warnings as w, i (i)}<p class="text-warn">{w}</p>{/each}
      </div>
    {/if}
    <input class="input filter" type="search" placeholder="Filter files" bind:value={filter} aria-label="Filter files" />
    {#if error}
      <p class="text-bad">{error}</p>
    {:else if !entries}
      <p class="faint"><Spinner size={13} /> Reading the archive…</p>
    {:else}
      <div class="files">
        {#each shown.slice(0, 1000) as e}
          <div class="file"><span class="mono name">{e.name}</span><span class="num faint">{e.name.endsWith('/') ? '' : bytes(e.size)}</span></div>
        {:else}
          <p class="faint">No files match.</p>
        {/each}
      </div>
      {#if entries.length >= 2000}<p class="faint small">Showing the first 2,000 entries.</p>{/if}
    {/if}
  {/if}
</Dialog>

<style>
  .meta {
    margin-bottom: 10px;
  }
  .items {
    display: grid;
    gap: 6px;
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }
  .items li {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
  }
  .warnings p {
    margin: 0 0 6px;
  }
  .filter {
    margin: 4px 0 10px;
  }
  .files {
    max-height: 46vh;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
  }
  .file {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 3px 10px;
    font-size: 12.5px;
  }
  .file:nth-child(even) {
    background: color-mix(in srgb, var(--raised) 40%, transparent);
  }
  .name {
    overflow-wrap: anywhere;
  }
  .small {
    font-size: var(--fs-sm);
  }
</style>
