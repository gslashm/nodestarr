<!-- Recent Proxmox node tasks (backups, snapshots, starts and stops), with their logs. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { dateTime, duration } from '../../lib/format';
  import type { PveOverview, PveTask } from '../../lib/types';

  let { overview }: { overview: PveOverview; selfVmid: number | null; reload: () => Promise<void> } = $props();

  let tasks = $state<PveTask[] | null>(null);
  let error = $state('');
  let open = $state<PveTask | null>(null);
  let log = $state<string[] | null>(null);

  const TYPES: Record<string, string> = {
    vzdump: 'Backup',
    vzsnapshot: 'Snapshot',
    vzrollback: 'Snapshot rollback',
    vzdelsnapshot: 'Delete snapshot',
    qmsnapshot: 'Snapshot',
    qmrollback: 'Snapshot rollback',
    qmdelsnapshot: 'Delete snapshot',
    vzstart: 'Start',
    vzstop: 'Stop',
    vzshutdown: 'Shut down',
    vzreboot: 'Reboot',
    qmstart: 'Start',
    qmstop: 'Stop',
    qmshutdown: 'Shut down',
    qmreboot: 'Reboot',
    qmsuspend: 'Suspend',
    qmresume: 'Resume',
    aptupdate: 'Refresh package lists',
    vncproxy: 'Console',
    vncshell: 'Shell',
    termproxy: 'Console',
    imgdel: 'Delete volume',
    srvreload: 'Reload service',
    srvrestart: 'Restart service',
  };

  async function load() {
    try {
      const r = await api.get<{ tasks: PveTask[] }>('/proxmox/tasks?limit=100');
      tasks = r.tasks;
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(() => {
    void load();
    const t = setInterval(() => {
      if (!document.hidden) void load();
    }, 15_000);
    return () => clearInterval(t);
  });

  async function show(t: PveTask) {
    open = t;
    log = null;
    try {
      const r = await api.get<{ lines: string[] }>(`/proxmox/tasks/${encodeURIComponent(t.upid)}/log`);
      if (open?.upid === t.upid) log = r.lines;
    } catch (err) {
      log = [errorMessage(err)];
    }
  }

  const names = $derived(new Map(overview.guests.map((g) => [String(g.vmid), g.name])));
</script>

{#if error}
  <p class="text-bad">{error}</p>
{:else if !tasks}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else if tasks.length === 0}
  <p class="muted">No recent tasks on this node.</p>
{:else}
  <section class="panel">
    <div class="table-wrap">
      <table class="table">
        <thead>
          <tr><th>Started</th><th>Task</th><th>Guest</th><th class="hide-sm">By</th><th>Took</th><th>Result</th></tr>
        </thead>
        <tbody>
          {#each tasks as t (t.upid)}
            {@const ok = t.status === 'OK'}
            <tr class="clickable" onclick={() => show(t)}>
              <td class="shrink num">{dateTime(t.starttime)}</td>
              <td>{TYPES[t.type] ?? t.type}</td>
              <td>{t.id ?? ''} <span class="muted">{t.id ? (names.get(t.id) ?? '') : ''}</span></td>
              <td class="hide-sm small">{t.user ?? ''}</td>
              <td class="shrink num">{t.starttime ? duration((t.endtime ?? Date.now() / 1000) - t.starttime) : '–'}</td>
              <td class="shrink">
                {#if t.status === null}<span class="text-update">Running</span>{:else if ok}<span class="text-ok">OK</span>{:else}<span class="text-bad ellipsis err" title={t.status}>{t.status}</span>{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </section>
{/if}

<Dialog open={!!open} title={open ? `${TYPES[open.type] ?? open.type}${open.id ? ` ${open.id}` : ''}` : ''} width="860px" drawer onclose={() => (open = null)}>
  {#if !log}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else}
    <pre class="log">{log.join('\n')}</pre>
  {/if}
  {#snippet actions()}
    {#if log}<CopyButton text={log.join('\n')} label="Copy log" />{/if}
  {/snippet}
</Dialog>

<style>
  .small {
    font-size: var(--fs-sm);
  }
  .err {
    display: inline-block;
    max-width: 260px;
    vertical-align: bottom;
  }
  .log {
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 12.5px;
    line-height: 1.5;
  }
  @media (max-width: 760px) {
    .hide-sm {
      display: none;
    }
  }
</style>
