<!--
  Update history: what changed, how it was triggered, how it went, with the
  task log and a rollback button for updates that can still be undone.
-->
<script lang="ts">
  import Undo2 from '@lucide/svelte/icons/undo-2';
  import { api } from '../lib/api';
  import { dateTime, imageTag } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { HistoryEntry } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { entries, showTarget = false }: { entries: HistoryEntry[]; showTarget?: boolean } = $props();

  const STATUS: Record<string, { label: string; tone: string }> = {
    applied: { label: 'Updated', tone: 'ok' },
    failed: { label: 'Failed', tone: 'bad' },
    rolled_back: { label: 'Rolled back', tone: 'warn' },
    left_running: { label: 'Left running', tone: 'warn' },
    reverted: { label: 'Undone later', tone: 'idle' },
  };

  const TRIGGER: Record<string, string> = { manual: 'By hand', auto: 'Automatic', api: 'API', mqtt: 'Home Assistant' };

  function change(h: HistoryEntry): string {
    const from = h.from_version ?? imageTag(h.from_ref ?? '');
    const to = h.to_version ?? imageTag(h.to_ref ?? '');
    if (from === to) return h.to_version ? to : `New build of ${to}`;
    return `${from} → ${to}`;
  }

  async function rollback(h: HistoryEntry) {
    const c = live.byIdentity.get(h.identity);
    const name = c?.display_name ?? h.target_name ?? 'the container';
    const ok = await ui.confirm({
      title: `Roll ${name} back?`,
      message: `It goes back to ${h.from_version ?? imageTag(h.from_ref ?? '')} using the image kept from before this update. Compose file changes made by the update are undone, and the newer build is skipped so it isn't installed again on its own.`,
      confirmLabel: 'Roll back',
      danger: true,
    });
    if (!ok) return;
    try {
      const r = await api.post<{ task_id: string }>(`/containers/${encodeURIComponent(h.identity)}/rollback`, { history_id: h.id });
      ui.taskStarted(r.task_id, `Rolling back ${name}`);
    } catch (err) {
      ui.error(err, "Couldn't roll back");
    }
  }
</script>

<div class="table-wrap">
  <table class="table">
    <thead>
      <tr>
        <th>When</th>
        {#if showTarget}<th>Container</th>{/if}
        <th>Change</th>
        <th>How</th>
        <th>Result</th>
        <th class="shrink"><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      {#each entries as h (h.id)}
        {@const c = live.byIdentity.get(h.identity)}
        <tr>
          <td class="shrink num">{dateTime(h.ts)}</td>
          {#if showTarget}
            <td>
              {#if c}<a href={href(`/containers/${encodeURIComponent(c.name)}/updates`)}>{c.display_name}</a>{:else}{h.target_name ?? h.identity}{/if}
            </td>
          {/if}
          <td>
            <span class="mono">{change(h)}</span>
            {#if h.message}<div class="faint small">{h.message}</div>{/if}
          </td>
          <td class="shrink">{TRIGGER[h.trigger ?? ''] ?? h.trigger ?? ''}</td>
          <td class="shrink"><span class="result {STATUS[h.status]?.tone ?? ''}">{STATUS[h.status]?.label ?? h.status}</span></td>
          <td class="shrink">
            <div class="row-actions">
              {#if h.task_id}<button type="button" class="btn small ghost" onclick={() => (ui.openTaskId = h.task_id)}>Log</button>{/if}
              {#if session.isAdmin && h.can_rollback}
                <button type="button" class="btn small" onclick={() => rollback(h)} disabled={c?.busy}><Undo2 size={14} />Roll back</button>
              {/if}
            </div>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .small {
    font-size: var(--fs-sm);
  }
  .result {
    font-weight: 500;
  }
  .result.ok {
    color: var(--ok);
  }
  .result.bad {
    color: var(--bad);
  }
  .result.warn {
    color: var(--warn);
  }
  .result.idle {
    color: var(--text-3);
  }
  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
  }
</style>
