<!--
  The Proxmox node itself: details, storages, history charts, physical
  disks with their health (from disk health when it's on, so opening this
  page doesn't wake sleeping disks), and pending package updates.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Button from '../../components/Button.svelte';
  import Meter from '../../components/Meter.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { MEDIA, diskSize, tempTone, verdictOf } from '../../lib/disks';
  import { bytes, duration, plural } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { DiskHealthOverview, DiskView, PveAptUpdate, PveDisk, PveOverview } from '../../lib/types';
  import HealthBadge from './disks/HealthBadge.svelte';
  import { ui } from '../../lib/ui.svelte';
  import RrdCharts from './RrdCharts.svelte';

  let { overview }: { overview: PveOverview; selfVmid: number | null; reload: () => Promise<void> } = $props();

  let disks = $state<PveDisk[] | null>(null);
  let health = $state<DiskView[] | null>(null);
  let disksError = $state('');
  let apt = $state<PveAptUpdate[] | null>(null);
  let aptError = $state('');

  const n = $derived(overview.node);

  async function loadApt() {
    try {
      const r = await api.get<{ updates: PveAptUpdate[] }>('/proxmox/apt');
      apt = r.updates;
      aptError = '';
    } catch (err) {
      aptError = errorMessage(err);
    }
  }

  onMount(() => {
    // Disk health already has every disk's SMART data; asking Proxmox again
    // would wake the sleeping ones.
    api
      .get<DiskHealthOverview>('/proxmox/disk-health')
      .then((r) => {
        if (r.enabled) health = r.disks.filter((d) => d.present && d.node === n.node);
        else return loadPveDisks();
      })
      .catch(() => loadPveDisks());
    void loadApt();
  });

  function loadPveDisks() {
    return api
      .get<{ disks: PveDisk[] }>('/proxmox/disks')
      .then((r) => (disks = r.disks))
      .catch((err) => (disksError = errorMessage(err)));
  }

  async function refreshApt() {
    try {
      const r = await api.post<{ task_id: string }>('/proxmox/apt/refresh');
      ui.taskStarted(r.task_id, 'Refreshing Proxmox package lists');
    } catch (err) {
      ui.error(err);
    }
  }

  function wear(d: PveDisk): string {
    if (typeof d.wearout === 'number') return `${100 - d.wearout}% life used`;
    return '';
  }
</script>

<div class="host">
  <div class="cols">
    <section class="panel">
      <div class="panel-head"><h2>Node</h2></div>
      <dl class="kv panel-body">
        <dt>Name</dt><dd>{n.node}</dd>
        <dt>Version</dt><dd class="num">Proxmox VE {n.pve_version}</dd>
        <dt>Kernel</dt><dd class="num">{n.kernel}</dd>
        <dt>CPU</dt><dd>{n.cpu_model} <span class="faint">({plural(n.cpus, 'thread')})</span></dd>
        <dt>Uptime</dt><dd class="num">{duration(n.uptime)}</dd>
        <dt>Load</dt><dd class="num">{n.loadavg.join(', ')}</dd>
        <dt>Swap</dt><dd class="num">{bytes(n.swap_used)} of {bytes(n.swap_total)}</dd>
      </dl>
    </section>
    <section class="panel">
      <div class="panel-head"><h2>Storage</h2></div>
      <div class="panel-body storages">
        {#each overview.storages as s (s.storage)}
          <div class="storage" class:off={!s.active}>
            <div class="label">
              <span><strong>{s.storage}</strong> <span class="faint small">{s.kind}{s.shared ? ', shared' : ''}{s.active ? '' : ', inactive'}</span></span>
              <span class="num small">{s.total ? `${bytes(s.used)} / ${bytes(s.total)}` : ''}</span>
            </div>
            {#if s.total}<Meter value={s.used} max={s.total} warn={85} bad={94} label="Used on {s.storage}" />{/if}
            <span class="faint small">{s.content.join(', ')}</span>
          </div>
        {/each}
      </div>
    </section>
  </div>

  <section class="panel">
    <div class="panel-head"><h2>History</h2></div>
    <div class="panel-body"><RrdCharts /></div>
  </section>

  <div class="cols">
    <section class="panel">
      <div class="panel-head">
        <h2>Disks</h2>
        {#if health}<a class="btn small ghost" href={href('/proxmox/disks')}>Health and history</a>{/if}
      </div>
      {#if health}
        <div class="table-wrap">
          <table class="table">
            <thead><tr><th>Device</th><th>Model</th><th class="num">Size</th><th>Health</th><th class="num">Temp</th></tr></thead>
            <tbody>
              {#each health as d (d.id)}
                {@const v = verdictOf(d)}
                <tr>
                  <td class="mono small"><a href={href(`/proxmox/disks/${encodeURIComponent(d.id)}`)}>{d.devpath}</a> <span class="faint">{MEDIA[d.media]}</span></td>
                  <td class="small">{d.model ?? ''}{d.used_by ? ` (${d.used_by})` : ''}</td>
                  <td class="num">{diskSize(d.size)}</td>
                  <td><HealthBadge tone={v.tone} label={v.label} /></td>
                  <td class="num"><span class="text-{tempTone(d.temperature, d.temp_limit) || 'ok'}">{d.temperature !== null ? `${d.temperature}°` : '–'}</span></td>
                </tr>
              {:else}
                <tr><td colspan="5" class="faint">No disks read yet.</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else if disksError}
        <p class="panel-body text-bad">{disksError}</p>
      {:else if !disks}
        <p class="panel-body faint"><Spinner size={13} /> Loading…</p>
      {:else if disks.length === 0}
        <p class="panel-body muted">No disks reported.</p>
      {:else}
        <div class="table-wrap">
          <table class="table">
            <thead><tr><th>Device</th><th>Model</th><th class="num">Size</th><th>Health</th></tr></thead>
            <tbody>
              {#each disks as d (d.devpath)}
                <tr>
                  <td class="mono small">{d.devpath} <span class="faint">{d.type ?? ''}</span></td>
                  <td class="small">{d.model ?? ''}{d.used ? ` (${d.used})` : ''}</td>
                  <td class="num">{bytes(d.size)}</td>
                  <td>
                    <span class:text-ok={d.health === 'PASSED' || d.health === 'OK'} class:text-bad={!!d.health && d.health !== 'PASSED' && d.health !== 'OK' && d.health !== 'UNKNOWN'}>
                      {d.health ?? 'Unknown'}
                    </span>
                    {#if wear(d)}<span class="faint small"> {wear(d)}</span>{/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>

    <section class="panel">
      <div class="panel-head">
        <h2>Package updates</h2>
        {#if session.isAdmin}<Button small variant="ghost" icon={RefreshCw} onclick={refreshApt}>Refresh lists</Button>{/if}
      </div>
      {#if aptError}
        <p class="panel-body text-bad">{aptError}</p>
      {:else if !apt}
        <p class="panel-body faint"><Spinner size={13} /> Loading…</p>
      {:else if apt.length === 0}
        <p class="panel-body muted">The node is up to date (as of the last package list refresh).</p>
      {:else}
        <p class="panel-body muted small note">{plural(apt.length, 'package')} can be updated. Install them from the node's shell or the Proxmox interface (Updates).</p>
        <div class="table-wrap apt">
          <table class="table">
            <thead><tr><th>Package</th><th>From</th><th>To</th></tr></thead>
            <tbody>
              {#each apt as u (u.Package)}
                <tr>
                  <td><span class="mono small">{u.Package}</span>{#if u.Priority === 'important' || u.Priority === 'required'}<span class="chip imp">{u.Priority}</span>{/if}</td>
                  <td class="num small faint">{u.OldVersion}</td>
                  <td class="num small">{u.Version}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>
  </div>
</div>

<style>
  .host {
    display: grid;
    gap: 16px;
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
    gap: 16px;
    align-items: start;
  }
  .storages {
    display: grid;
    gap: 14px;
  }
  .storage {
    display: grid;
    gap: 4px;
  }
  .storage.off {
    opacity: 0.6;
  }
  .label {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .note {
    margin: 0;
    padding-bottom: 0;
  }
  .apt {
    max-height: 420px;
    overflow-y: auto;
  }
  .imp {
    margin-left: 6px;
    color: var(--warn);
  }
</style>
