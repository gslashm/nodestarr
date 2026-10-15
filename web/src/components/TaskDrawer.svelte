<!-- Details and live log of one task (opened from toasts, the tray and lists). -->
<script lang="ts">
  import Ban from '@lucide/svelte/icons/ban';
  import { api, errorMessage } from '../lib/api';
  import { dateTime, duration } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import type { TaskSnapshot } from '../lib/types';
  import { ui } from '../lib/ui.svelte';
  import Button from './Button.svelte';
  import CopyButton from './CopyButton.svelte';
  import Dialog from './Dialog.svelte';
  import LogView, { type ViewLine } from './LogView.svelte';
  import Spinner from './Spinner.svelte';

  let snapshot = $state<TaskSnapshot | null>(null);
  let lines = $state<ViewLine[]>([]);
  let error = $state('');
  let follow = $state(true);
  let seq = 0;

  const id = $derived(ui.openTaskId);
  const task = $derived((id && live.tasks[id]) || snapshot);
  const running = $derived(task?.status === 'running' || task?.status === 'queued');

  function toLines(raw: string[]): ViewLine[] {
    return raw.map((t) => {
      // Task lines look like "12:03:04 message".
      const m = /^(\d\d:\d\d:\d\d) (.*)$/.exec(t);
      return { n: seq++, s: t.startsWith('error:') ? 'err' : 'out', t: m ? m[2] : t, ts: m ? m[1] : undefined };
    });
  }

  $effect(() => {
    const current = id;
    snapshot = null;
    lines = [];
    error = '';
    follow = true;
    if (!current) return;
    const unsubscribe = live.onTaskLog(current, (newLines) => {
      lines = [...lines, ...toLines(newLines)].slice(-5000);
    });
    api
      .get<{ task: TaskSnapshot; log: string[] }>(`/tasks/${current}`)
      .then((r) => {
        if (ui.openTaskId !== current) return;
        snapshot = r.task;
        // The fetched log already contains lines streamed meanwhile.
        lines = toLines(r.log);
      })
      .catch((err) => (error = errorMessage(err)));
    return unsubscribe;
  });

  async function cancel() {
    if (!id) return;
    try {
      await api.post(`/tasks/${id}/cancel`);
      ui.toast('Stopping the task at its next safe point.', 'info');
    } catch (err) {
      ui.error(err);
    }
  }

  const statusText: Record<string, string> = {
    queued: 'Waiting',
    running: 'Running',
    succeeded: 'Done',
    failed: 'Failed',
    cancelled: 'Cancelled',
  };
</script>

<Dialog open={!!id} title={task?.title ?? 'Task'} drawer width="720px" onclose={() => (ui.openTaskId = null)}>
  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !task}
    <div class="loading"><Spinner /> Loading…</div>
  {:else}
    <div class="summary">
      <span class="status {task.status}">
        {#if running}<Spinner size={13} />{/if}
        {statusText[task.status] ?? task.status}
      </span>
      {#if task.target_name && task.target}
        <a href={href(`/containers/${encodeURIComponent(task.target)}`)} onclick={() => (ui.openTaskId = null)}>{task.target_name}</a>
      {/if}
      <span class="muted">started {dateTime(task.started_at ?? task.created_at)}</span>
      {#if task.finished_at && task.started_at}<span class="muted">took {duration(task.finished_at - task.started_at)}</span>{/if}
      {#if task.actor}<span class="faint">by {task.actor.replace(/^user:/, '')}</span>{/if}
    </div>
    {#if running && task.progress !== null}
      <div class="progress"><span style:width="{Math.round((task.progress ?? 0) * 100)}%"></span></div>
    {/if}
    {#if task.error}<p class="error-box">{task.error}</p>{/if}
    <div class="log-wrap">
      <LogView {lines} bind:follow showTime empty={running ? 'Waiting for output…' : 'This task wrote no log.'} />
    </div>
  {/if}
  {#snippet actions()}
    {#if task}
      <CopyButton text={() => lines.map((l) => `${l.ts ?? ''} ${l.t}`.trim()).join('\n')} label="Copy log" />
    {/if}
  {/snippet}
  {#snippet footer()}
    {#if running}<Button icon={Ban} variant="danger" onclick={cancel}>Cancel task</Button>{/if}
    <button type="button" class="btn" onclick={() => (ui.openTaskId = null)}>Close</button>
  {/snippet}
</Dialog>

<style>
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    margin-bottom: 12px;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    border-radius: 12px;
    background: var(--raised);
    font: 600 var(--fs-sm) / 1 var(--font-data);
  }
  .status.succeeded {
    background: color-mix(in srgb, var(--ok) 15%, transparent);
    color: var(--ok);
  }
  .status.failed {
    background: color-mix(in srgb, var(--bad) 15%, transparent);
    color: var(--bad);
  }
  .status.running,
  .status.queued {
    color: var(--accent-text);
  }
  .progress {
    height: 4px;
    margin-bottom: 12px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 300ms var(--ease);
  }
  .error-box {
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--bad) 45%, transparent);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, transparent);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .log-wrap {
    height: calc(100vh - 260px);
    min-height: 240px;
  }
  .loading {
    display: flex;
    gap: 10px;
    align-items: center;
    color: var(--text-2);
  }
</style>
