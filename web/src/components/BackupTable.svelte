<!--
  A list of backups with restore, contents, download, pin and delete. Used on
  a container's Backups tab and on the Backups page (with a container column).
-->
<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import History from '@lucide/svelte/icons/history';
  import Pin from '@lucide/svelte/icons/pin';
  import PinOff from '@lucide/svelte/icons/pin-off';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import { api, apiUrl, seg } from '../lib/api';
  import { bytes, dateTime } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { BackupRecord } from '../lib/types';
  import { ui } from '../lib/ui.svelte';
  import BackupContents from './BackupContents.svelte';
  import Menu, { type MenuEntry } from './Menu.svelte';
  import TimeAgo from './TimeAgo.svelte';

  let {
    backups,
    showTarget = false,
    onchanged,
  }: { backups: BackupRecord[]; showTarget?: boolean; onchanged?: () => void } = $props();

  let viewing = $state<BackupRecord | null>(null);

  const KIND: Record<string, string> = {
    manual: 'By hand',
    scheduled: 'Scheduled',
    'pre-update': 'Before update',
    'pre-restore': 'Before restore',
  };

  function name(b: BackupRecord): string {
    return b.target_name ?? b.manifest.name ?? 'container';
  }

  async function restore(b: BackupRecord) {
    const items = b.manifest.items ?? [];
    const hasDb = items.some((i) => i.type === 'database' || i.type === 'native');
    const hasFiles = items.some((i) => i.type === 'mount');
    const exists = !!live.byIdentity.get(b.identity);
    const r = await ui.ask({
      title: `Restore ${name(b)}?`,
      message: [
        `Puts back the data from ${dateTime(b.created_at)}${b.manifest.app_version ? ` (version ${b.manifest.app_version})` : ''}.`,
        'The container is stopped while its data is replaced, then started again.',
        exists ? '' : 'The container must exist first: deploy it, then restore.',
      ]
        .filter(Boolean)
        .join(' '),
      confirmLabel: 'Restore',
      danger: true,
      options: [
        ...(hasFiles ? [{ key: 'files', label: 'Restore folders and volumes', checked: true }] : []),
        ...(hasDb ? [{ key: 'database', label: 'Restore the database', checked: true }] : []),
        ...(hasFiles ? [{ key: 'replace', label: 'Delete files that are not in the backup', danger: true }] : []),
        { key: 'safety', label: 'Back up the current state first', checked: true },
      ],
    });
    if (!r.ok) return;
    try {
      const res = await api.post<{ task_id: string }>(`/backups/${seg(b.id)}/restore`, {
        files: hasFiles ? !!r.options.files : false,
        database: hasDb ? !!r.options.database : false,
        replace: !!r.options.replace,
        safety_backup: !!r.options.safety,
      });
      ui.taskStarted(res.task_id, `Restoring ${name(b)}`);
    } catch (err) {
      ui.error(err, "Couldn't start the restore");
    }
  }

  async function setPinned(b: BackupRecord, pinned: boolean) {
    try {
      await api.put(`/backups/${seg(b.id)}/pin`, { pinned });
      ui.toast(pinned ? 'Pinned: retention cleanup will keep it' : 'Unpinned', 'success');
      onchanged?.();
    } catch (err) {
      ui.error(err);
    }
  }

  async function remove(b: BackupRecord) {
    const ok = await ui.confirm({
      title: 'Delete this backup?',
      message: `The archive from ${dateTime(b.created_at)} (${bytes(b.size)}) is deleted from disk.`,
      confirmLabel: 'Delete',
      danger: true,
    });
    if (!ok) return;
    try {
      await api.del(`/backups/${seg(b.id)}`);
      ui.toast('Backup deleted', 'success');
      onchanged?.();
    } catch (err) {
      ui.error(err, "Couldn't delete it");
    }
  }

  function menu(b: BackupRecord): MenuEntry[] {
    const admin = session.isAdmin;
    const items: MenuEntry[] = [{ label: 'Show contents', icon: FolderOpen, run: () => (viewing = b), disabled: !b.exists }];
    if (admin) {
      items.push({ label: 'Download', icon: Download, href: apiUrl(`/backups/${seg(b.id)}/download`), disabled: !b.exists });
      items.push(b.pinned ? { label: 'Unpin', icon: PinOff, run: () => setPinned(b, false) } : { label: 'Pin (keep forever)', icon: Pin, run: () => setPinned(b, true) });
      items.push('separator', { label: 'Delete', icon: Trash2, danger: true, run: () => remove(b) });
    }
    return items;
  }
</script>

<div class="table-wrap">
  <table class="table">
    <thead>
      <tr>
        <th>When</th>
        {#if showTarget}<th>Container</th>{/if}
        <th>Type</th>
        <th>Version</th>
        <th class="num">Size</th>
        <th>State</th>
        <th class="shrink"><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      {#each backups as b (b.id)}
        <tr class:missing={!b.exists}>
          <td class="when">
            <span class="num">{dateTime(b.created_at)}</span>
            <span class="faint small"><TimeAgo ts={b.created_at} /></span>
          </td>
          {#if showTarget}
            <td>
              {#if live.byIdentity.get(b.identity)}
                <a href={href(`/containers/${encodeURIComponent(live.byIdentity.get(b.identity)!.name)}/backups`)}>{name(b)}</a>
              {:else}
                {name(b)} <span class="faint small">(gone)</span>
              {/if}
            </td>
          {/if}
          <td>{KIND[b.kind] ?? b.kind}{#if b.pinned} <Pin size={13} class="pin" aria-label="Pinned" />{/if}</td>
          <td class="num">{b.manifest.app_version ?? '–'}</td>
          <td class="num">{bytes(b.size)}</td>
          <td>
            {#if !b.exists}
              <span class="text-bad">File missing</span>
            {:else if b.status === 'partial'}
              <span class="text-warn" title={(b.manifest.warnings ?? []).join('\n')}>Partial</span>
            {:else}
              <span class="text-ok">Complete</span>
            {/if}
          </td>
          <td class="shrink">
            <div class="actions">
              {#if session.isAdmin}
                <button type="button" class="btn small" onclick={() => restore(b)} disabled={!b.exists}><History size={14} />Restore</button>
              {/if}
              <Menu items={menu(b)} small label="Backup actions" />
            </div>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<BackupContents backup={viewing} onclose={() => (viewing = null)} />

<style>
  .when {
    white-space: nowrap;
  }
  .when .small {
    margin-left: 6px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }
  tr.missing td {
    color: var(--text-3);
  }
  td :global(.pin) {
    color: var(--accent-text);
    vertical-align: -2px;
  }
</style>
