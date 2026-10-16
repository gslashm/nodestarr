<!--
  The Proxmox node at a glance, how its resources split across guests, and
  its guests (containers and VMs), with power actions, snapshots, vzdump
  backups and history charts per guest.
-->
<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive';
  import Camera from '@lucide/svelte/icons/camera';
  import ChartLine from '@lucide/svelte/icons/chart-line';
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import Power from '@lucide/svelte/icons/power';
  import PowerOff from '@lucide/svelte/icons/power-off';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Dialog from '../../components/Dialog.svelte';
  import Menu, { type MenuEntry } from '../../components/Menu.svelte';
  import Meter from '../../components/Meter.svelte';
  import { api } from '../../lib/api';
  import { bytes, duration, pct, plural } from '../../lib/format';
  import { session } from '../../lib/session.svelte';
  import type { PveGuest, PveOverview } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';
  import GuestBackup from './GuestBackup.svelte';
  import GuestSnapshots from './GuestSnapshots.svelte';
  import GuestUsage from './GuestUsage.svelte';
  import RrdCharts from './RrdCharts.svelte';

  let { overview }: { overview: PveOverview; selfVmid: number | null; reload: () => Promise<void> } = $props();

  type Panel = { kind: 'snapshots' | 'backup' | 'charts'; guest: PveGuest };
  let panel = $state<Panel | null>(null);
  let filter = $state<'all' | 'running' | 'stopped'>('all');

  const n = $derived(overview.node);
  const guests = $derived(
    overview.guests
      .filter((g) => !g.template && (filter === 'all' || (filter === 'running' ? g.status === 'running' : g.status !== 'running')))
      .sort((a, b) => a.vmid - b.vmid),
  );
  const running = $derived(overview.guests.filter((g) => g.status === 'running').length);

  type Action = 'start' | 'shutdown' | 'reboot' | 'stop' | 'suspend' | 'resume';
  const VERB: Record<Action, string> = { start: 'Start', shutdown: 'Shut down', reboot: 'Reboot', stop: 'Stop', suspend: 'Suspend', resume: 'Resume' };

  async function act(g: PveGuest, action: Action) {
    if (action !== 'start' && action !== 'resume') {
      const what: Record<string, string> = {
        shutdown: 'It is asked to shut down cleanly.',
        reboot: 'It is asked to restart cleanly.',
        stop: 'It is stopped immediately, like pulling the plug. Unsaved data can be lost.',
        suspend: 'It is paused in memory until you resume it.',
      };
      const r = await ui.ask({
        title: `${VERB[action]} ${g.name} (${g.vmid})?`,
        message: `${what[action]}${g.is_self ? ' This guest runs Nodestarr, so Nodestarr goes offline too.' : ''}`,
        confirmLabel: VERB[action],
        danger: action === 'stop' || g.is_self,
        typeToConfirm: g.is_self ? g.name : undefined,
      });
      if (!r.ok) return;
    }
    try {
      const res = await api.post<{ task_id: string }>(`/proxmox/guests/${g.vmid}/actions/${action}`, { confirm_self: g.is_self });
      ui.taskStarted(res.task_id, `${VERB[action]} ${g.name}`);
    } catch (err) {
      ui.error(err, `Couldn't ${VERB[action].toLowerCase()} ${g.name}`);
    }
  }

  function menu(g: PveGuest): MenuEntry[] {
    const on = g.status === 'running';
    const paused = g.status === 'paused' || g.status === 'suspended';
    const items: MenuEntry[] = [
      { label: 'History charts', icon: ChartLine, run: () => (panel = { kind: 'charts', guest: g }) },
    ];
    if (!session.isAdmin) return items;
    items.push(
      { label: 'Snapshots', icon: Camera, run: () => (panel = { kind: 'snapshots', guest: g }) },
      { label: 'Back up now (vzdump)', icon: Archive, run: () => (panel = { kind: 'backup', guest: g }) },
      'separator',
    );
    if (paused) items.push({ label: 'Resume', icon: Play, run: () => act(g, 'resume') });
    else if (on) {
      items.push(
        { label: 'Reboot', icon: RotateCw, run: () => act(g, 'reboot') },
        { label: 'Shut down', icon: Power, run: () => act(g, 'shutdown') },
      );
      if (g.kind === 'qemu') items.push({ label: 'Suspend', icon: Pause, run: () => act(g, 'suspend') });
      items.push({ label: 'Stop (force)', icon: PowerOff, danger: true, run: () => act(g, 'stop') });
    } else items.push({ label: 'Start', icon: Play, run: () => act(g, 'start') });
    return items;
  }

  const panelTitle = $derived(
    panel ? `${panel.kind === 'snapshots' ? 'Snapshots' : panel.kind === 'backup' ? 'Back up' : 'History'}: ${panel.guest.name} (${panel.guest.vmid})` : '',
  );
</script>

<div class="tiles">
  <section class="panel tile">
    <h3>CPU</h3>
    <p class="big num">{pct(n.cpu * 100, 0)}</p>
    <Meter value={n.cpu * 100} label="Node CPU" />
    <p class="sub">{plural(n.cpus, 'core')}, load {n.loadavg.join(' ')}</p>
  </section>
  <section class="panel tile">
    <h3>Memory</h3>
    <p class="big num">{bytes(n.mem_used)}</p>
    <Meter value={n.mem_used} max={n.mem_total} warn={85} bad={94} label="Node memory" />
    <p class="sub">of {bytes(n.mem_total)}{n.ksm_shared ? `, ${bytes(n.ksm_shared)} shared by KSM` : ''}</p>
  </section>
  <section class="panel tile">
    <h3>Root disk</h3>
    <p class="big num">{bytes(n.rootfs_used)}</p>
    <Meter value={n.rootfs_used} max={n.rootfs_total} label="Node root disk" />
    <p class="sub">of {bytes(n.rootfs_total)}</p>
  </section>
  <section class="panel tile">
    <h3>Node</h3>
    <p class="big num">{duration(n.uptime)}</p>
    <p class="sub">up, {plural(running, 'guest')} running</p>
    <p class="sub ellipsis" title={n.cpu_model}>{n.cpu_model}</p>
  </section>
</div>

<GuestUsage {overview} />

<div class="bar">
  <h2>Guests</h2>
  <div class="filters" role="radiogroup" aria-label="Show">
    {#each [['all', 'All'], ['running', 'Running'], ['stopped', 'Stopped']] as [v, label] (v)}
      <button type="button" role="radio" aria-checked={filter === v} class:active={filter === v} onclick={() => (filter = v as typeof filter)}>{label}</button>
    {/each}
  </div>
</div>

<section class="panel">
  <div class="table-wrap">
    <table class="table">
      <thead>
        <tr>
          <th class="shrink">ID</th>
          <th>Name</th>
          <th>State</th>
          <th class="num">CPU</th>
          <th class="hide-sm">Memory</th>
          <th class="hide-md">Disk</th>
          <th class="hide-md">Up</th>
          <th class="shrink"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each guests as g (g.vmid)}
          {@const on = g.status === 'running'}
          <tr>
            <td class="shrink num faint">{g.vmid}</td>
            <td>
              <div class="name">
                <span class="kind" title={g.kind === 'lxc' ? 'Container (LXC)' : 'Virtual machine'}>{g.kind === 'lxc' ? 'CT' : 'VM'}</span>
                <span>{g.name}</span>
                {#if g.is_self}<span class="chip self" title="Nodestarr runs in this guest">Nodestarr</span>{/if}
                {#each g.tags as t (t)}<span class="chip">{t}</span>{/each}
              </div>
            </td>
            <td><span class="state {g.status}">{g.status === 'running' ? 'Running' : g.status === 'stopped' ? 'Stopped' : g.status}</span></td>
            <td class="num">{on ? pct(g.cpu * 100, 1) : '–'}</td>
            <td class="hide-sm">
              {#if on && g.maxmem}
                <div class="usage"><Meter value={g.mem} max={g.maxmem} height={5} label="Memory of {g.name}" /><span class="num small">{bytes(g.mem)} / {bytes(g.maxmem)}</span></div>
              {:else}<span class="faint">–</span>{/if}
            </td>
            <td class="hide-md">
              {#if g.maxdisk && g.disk}
                <div class="usage"><Meter value={g.disk} max={g.maxdisk} height={5} label="Disk of {g.name}" /><span class="num small">{bytes(g.disk)} / {bytes(g.maxdisk)}</span></div>
              {:else}<span class="faint small">{g.maxdisk ? bytes(g.maxdisk) : '–'}</span>{/if}
            </td>
            <td class="hide-md num">{on ? duration(g.uptime) : '–'}</td>
            <td class="shrink">
              <div class="row-actions">
                {#if session.isAdmin && !on && g.status !== 'paused'}
                  <button type="button" class="btn small ghost icon" onclick={() => act(g, 'start')} title="Start {g.name}" aria-label="Start {g.name}"><Play size={15} /></button>
                {/if}
                <Menu items={menu(g)} small label="Actions for {g.name}" />
              </div>
            </td>
          </tr>
        {:else}
          <tr><td colspan="8" class="faint">No guests.</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<Dialog open={!!panel} title={panelTitle} width={panel?.kind === 'charts' ? '980px' : '620px'} onclose={() => (panel = null)}>
  {#if panel?.kind === 'snapshots'}
    <GuestSnapshots guest={panel.guest} />
  {:else if panel?.kind === 'backup'}
    <GuestBackup guest={panel.guest} storages={overview.storages} ondone={() => (panel = null)} />
  {:else if panel?.kind === 'charts'}
    <RrdCharts vmid={panel.guest.vmid} />
  {/if}
</Dialog>

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 14px;
    margin-bottom: 20px;
  }
  .tile {
    display: grid;
    align-content: start;
    gap: 6px;
    padding: 14px 16px;
  }
  .tile h3 {
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .tile p {
    margin: 0;
  }
  .big {
    font: 600 24px / 1.1 var(--font-data);
  }
  .sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }
  .bar h2 {
    flex: 1;
    font-size: var(--fs-md);
  }
  .filters {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--sunken);
  }
  .filters button {
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-2);
    font: 500 var(--fs-sm) / 1 var(--font-data);
    cursor: pointer;
  }
  .filters button.active {
    background: var(--raised);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .name {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .kind {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--raised);
    color: var(--text-3);
    font: 600 10.5px / 1.4 var(--font-data);
  }
  .self {
    color: var(--accent-text);
  }
  .state {
    font-weight: 500;
  }
  .state.running {
    color: var(--ok);
  }
  .state.stopped {
    color: var(--text-3);
  }
  .state.paused,
  .state.suspended {
    color: var(--paused);
  }
  .usage {
    display: grid;
    gap: 3px;
    min-width: 120px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }
  @media (max-width: 1000px) {
    .hide-md {
      display: none;
    }
  }
  @media (max-width: 760px) {
    .hide-sm {
      display: none;
    }
  }
</style>
