<!--
  Host and container resources: live numbers, host history charts, disks,
  Docker engine facts, and a sortable table of what each container uses.
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import ArrowDown from '@lucide/svelte/icons/arrow-down';
  import AppIcon from '../../components/AppIcon.svelte';
  import Chart, { type ChartSeries } from '../../components/Chart.svelte';
  import Meter from '../../components/Meter.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Sparkline from '../../components/Sparkline.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api } from '../../lib/api';
  import { bytes, duration, isUp, pct, plural, rate, stateOf } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import type { DiskUsage, DockerInfo, HostStats, Series } from '../../lib/types';

  type Range = '1h' | '6h' | '24h' | '7d' | '30d';
  type SortKey = 'cpu' | 'mem' | 'net' | 'disk' | 'name';

  interface Overview {
    host: HostStats | null;
    disks: DiskUsage[];
    docker: DockerInfo | null;
    history_enabled: boolean;
  }

  let overview = $state<Overview | null>(null);
  let range = $state<Range>('6h');
  let series = $state.raw<Series | null>(null);
  let loadingHistory = $state(false);
  let sort = $state<SortKey>('cpu');

  onMount(() => {
    api
      .get<Overview>('/monitoring/overview')
      .then((r) => (overview = r))
      .catch(() => {});
  });

  $effect(() => {
    const r = range;
    loadingHistory = true;
    api
      .get<{ series: Series }>(`/monitoring/history?target=host&range=${r}&points=360`)
      .then((res) => {
        if (r === untrack(() => range)) series = res.series;
      })
      .catch(() => {})
      .finally(() => (loadingHistory = false));
  });

  const host = $derived(live.host ?? overview?.host ?? null);
  /** Recent history followed by live samples, for the CPU sparkline. */
  const cpuTrend = $derived([...(series?.cpu.slice(-40) ?? []), ...live.hostTrail.map((h) => h.cpu)].slice(-80));
  const memUsed = $derived(host ? host.mem_total - host.mem_available : 0);
  const swapUsed = $derived(host ? host.swap_total - host.swap_free : 0);

  const cpuSeries: ChartSeries[] = [{ label: 'CPU', color: 'var(--info)', format: (v) => pct(v, 1) }];
  const memSeries: ChartSeries[] = [{ label: 'Used', color: 'var(--accent)', format: (v) => bytes(v) }];
  const cpuData = $derived(series ? [series.ts, series.cpu] : []);
  const memData = $derived(series ? [series.ts, series.mem] : []);

  const rows = $derived.by(() => {
    const list = live.list
      .filter((c) => isUp(stateOf(c)) && c.stats)
      .map((c) => {
        const s = c.stats!;
        return { c, s, net: s.net_rx + s.net_tx, disk: s.blk_read + s.blk_write };
      });
    const by: Record<SortKey, (a: (typeof list)[number], b: (typeof list)[number]) => number> = {
      cpu: (a, b) => b.s.cpu - a.s.cpu,
      mem: (a, b) => b.s.mem - a.s.mem,
      net: (a, b) => b.net - a.net,
      disk: (a, b) => b.disk - a.disk,
      name: (a, b) => a.c.display_name.localeCompare(b.c.display_name),
    };
    return list.sort(by[sort]);
  });

  const totals = $derived(
    rows.reduce((t, r) => ({ cpu: t.cpu + r.s.cpu, mem: t.mem + r.s.mem }), { cpu: 0, mem: 0 }),
  );
</script>

<div class="resources">
  <div class="tiles">
    <section class="panel tile">
      <h3>CPU</h3>
      <p class="big num">{host ? pct(host.cpu, 0) : '–'}</p>
      <Sparkline values={cpuTrend} width={220} height={30} max={100} color="var(--info)" label="Host CPU, recent" stretch />
      <p class="sub">{host ? `${plural(host.cpus, 'core')}, load ${host.load.map((l) => l.toFixed(2)).join(' ')}` : ''}</p>
    </section>
    <section class="panel tile">
      <h3>Memory</h3>
      <p class="big num">{host ? bytes(memUsed) : '–'}</p>
      {#if host}<Meter value={memUsed} max={host.mem_total} warn={85} bad={94} label="Host memory used" />{/if}
      <p class="sub">{host ? `of ${bytes(host.mem_total)}${host.swap_total ? `, swap ${bytes(swapUsed)} of ${bytes(host.swap_total)}` : ''}` : ''}</p>
    </section>
    <section class="panel tile">
      <h3>Containers</h3>
      <p class="big num">{pct(totals.cpu, 1)} <span class="unit">CPU</span></p>
      <p class="mid num">{bytes(totals.mem)} <span class="unit">memory</span></p>
      <p class="sub">{plural(rows.length, 'running container')}</p>
    </section>
    <section class="panel tile">
      <h3>Host</h3>
      <p class="big num">{host ? duration(host.uptime) : '–'}</p>
      <p class="sub">up since the last reboot</p>
      {#if overview?.docker}<p class="sub">Docker {overview.docker.version}{overview.docker.compose_version ? `, Compose ${overview.docker.compose_version}` : ''}</p>{/if}
    </section>
  </div>

  <div class="charts-head">
    <h2>Host history</h2>
    {#if loadingHistory}<Spinner size={14} />{/if}
    <span class="grow"></span>
    <Segmented
      label="Time range"
      bind:value={range}
      options={[
        { value: '1h', label: '1 hour' },
        { value: '6h', label: '6 hours' },
        { value: '24h', label: '24 hours' },
        { value: '7d', label: '7 days' },
        { value: '30d', label: '30 days' },
      ]}
    />
  </div>
  {#if overview && !overview.history_enabled}
    <p class="muted">History is turned off. <a href={href('/settings/monitoring')}>Turn it on</a> to see charts here.</p>
  {:else if series && series.ts.length > 1}
    <div class="charts">
      <section class="panel"><div class="panel-head"><h3>CPU</h3></div><div class="panel-body"><Chart data={cpuData} series={cpuSeries} yFormat={(v) => pct(v)} yMax={100} label="Host CPU over time" /></div></section>
      <section class="panel">
        <div class="panel-head"><h3>Memory</h3></div>
        <div class="panel-body"><Chart data={memData} series={memSeries} yFormat={(v) => bytes(v, 0)} yMax={host?.mem_total} label="Host memory over time" /></div>
      </section>
    </div>
  {:else if series}
    <p class="muted">Not enough samples for this range yet.</p>
  {/if}

  <div class="split">
    <section class="panel">
      <div class="panel-head">
        <h2>Containers</h2>
        <label class="sort">
          <ArrowDown size={14} />
          <select class="input" bind:value={sort} aria-label="Sort by">
            <option value="cpu">CPU</option>
            <option value="mem">Memory</option>
            <option value="net">Network</option>
            <option value="disk">Disk</option>
            <option value="name">Name</option>
          </select>
        </label>
      </div>
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th>Container</th>
              <th class="num">CPU</th>
              <th class="hide-sm"><span class="sr-only">CPU trend</span></th>
              <th class="num">Memory</th>
              <th class="num hide-sm">Network</th>
              <th class="num hide-sm">Disk</th>
            </tr>
          </thead>
          <tbody>
            {#each rows as { c, s } (c.id)}
              <tr>
                <td>
                  <a class="who" href={href(`/containers/${encodeURIComponent(c.name)}/stats`)}>
                    <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={22} />
                    <span class="ellipsis">{c.display_name}</span>
                  </a>
                </td>
                <td class="num">{pct(s.cpu, 1)}</td>
                <td class="hide-sm spark"><Sparkline values={live.cpuTrail[c.identity] ?? []} width={90} height={20} color="var(--info)" label="CPU trend of {c.display_name}" /></td>
                <td class="num">
                  {bytes(s.mem)}
                  {#if s.mem_limit && host && s.mem_limit < host.mem_total}<span class="faint small"> / {bytes(s.mem_limit)}</span>{/if}
                </td>
                <td class="num hide-sm">{rate(s.net_rx + s.net_tx)}</td>
                <td class="num hide-sm">{rate(s.blk_read + s.blk_write)}</td>
              </tr>
            {:else}
              <tr><td colspan="6" class="faint">No running containers, or no samples yet.</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <div class="side">
      <section class="panel">
        <div class="panel-head"><h2>Disks</h2></div>
        <div class="panel-body disks">
          {#each overview?.disks ?? [] as d (d.path)}
            <div class="disk">
              <div class="label"><span class="mono ellipsis" title={d.path}>{d.path}</span><span class="num">{pct((d.used / d.total) * 100)}</span></div>
              <Meter value={d.used} max={d.total} label="Disk used on {d.path}" />
              <p class="sub">{bytes(d.available)} free of {bytes(d.total)}</p>
            </div>
          {:else}
            <p class="faint">No disks watched. Add paths in Settings > Monitoring.</p>
          {/each}
        </div>
      </section>
      {#if overview?.docker}
        {@const d = overview.docker}
        <section class="panel">
          <div class="panel-head"><h2>Docker engine</h2></div>
          <dl class="kv panel-body">
            <dt>Version</dt><dd class="num">{d.version} <span class="faint">(API {d.api_version})</span></dd>
            {#if d.compose_version}<dt>Compose</dt><dd class="num">{d.compose_version}</dd>{/if}
            <dt>System</dt><dd>{d.os}, {d.arch}</dd>
            <dt>Kernel</dt><dd class="num">{d.kernel}</dd>
            <dt>Storage</dt><dd>{d.storage_driver} <span class="faint mono">{d.root_dir}</span></dd>
            <dt>Resources</dt><dd>{plural(d.cpus, 'CPU')}, {bytes(d.memory)}</dd>
            <dt>Objects</dt><dd><a href={href('/system/docker')}>{plural(d.containers, 'container')}, {plural(d.images, 'image')}</a></dd>
          </dl>
        </section>
      {/if}
    </div>
  </div>
</div>

<style>
  .resources {
    display: grid;
    gap: 16px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 14px;
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
    font: 600 26px / 1.1 var(--font-data);
  }
  .mid {
    font: 600 var(--fs-lg) / 1.2 var(--font-data);
  }
  .unit {
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .charts-head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
  }
  .charts-head h2 {
    font-size: var(--fs-md);
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
    gap: 16px;
  }
  .charts .panel-body {
    padding: 10px 12px 6px;
  }
  .split {
    display: grid;
    grid-template-columns: minmax(0, 1.7fr) minmax(280px, 1fr);
    gap: 16px;
    align-items: start;
  }
  .side {
    display: grid;
    gap: 16px;
  }
  .sort {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
  }
  .sort .input {
    width: auto;
    min-height: 30px;
    padding-top: 3px;
    padding-bottom: 3px;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
    color: var(--text);
  }
  .spark {
    width: 100px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .disks {
    display: grid;
    gap: 14px;
  }
  .disk {
    display: grid;
    gap: 5px;
  }
  .disk .label {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    font-size: var(--fs-sm);
  }
  .disk .sub {
    margin: 0;
  }
  @media (max-width: 1100px) {
    .split {
      grid-template-columns: 1fr;
    }
  }
  @media (max-width: 760px) {
    .hide-sm {
      display: none;
    }
    .charts {
      grid-template-columns: 1fr;
    }
  }
</style>
