<!--
  Resource use of one container: live numbers and history charts for CPU,
  memory, network and disk. Short ranges keep updating from the live feed.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import ChartLine from '@lucide/svelte/icons/chart-line';
  import Chart, { type ChartSeries } from '../../components/Chart.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage, qs, seg } from '../../lib/api';
  import { bytes, isUp, pct, rate, stateOf } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail, ContainerView, LiveStat, Series } from '../../lib/types';

  let { c, detail }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  type Range = '15m' | '1h' | '6h' | '24h' | '7d' | '30d';
  const RANGE_SECS: Record<Range, number> = { '15m': 900, '1h': 3600, '6h': 21600, '24h': 86400, '7d': 604800, '30d': 2592000 };
  const RANGE_KEY = 'berth.stats.range';

  function storedRange(): Range {
    try {
      const r = localStorage.getItem(RANGE_KEY);
      if (r && r in RANGE_SECS) return r as Range;
    } catch {
      // Ignore: storage unavailable.
    }
    return '1h';
  }

  let range = $state<Range>(storedRange());
  let series = $state.raw<Series | null>(null);
  let loading = $state(true);
  let error = $state('');

  const up = $derived(isUp(stateOf(c)));
  const live = $derived(c.stats);
  const historyOn = $derived(session.settings?.monitoring.history_enabled ?? true);

  async function load(r: Range) {
    loading = true;
    error = '';
    try {
      const res = await api.get<{ series: Series; latest: LiveStat | null }>(`/containers/${seg(c.id)}/stats${qs({ range: r })}`);
      if (r === range) series = res.series;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    const r = range;
    try {
      localStorage.setItem(RANGE_KEY, r);
    } catch {
      // Ignore: storage unavailable.
    }
    void load(r);
  });

  /** Appends a live sample, dropping points that fell out of the range. */
  function append(s: LiveStat, r: Range) {
    const cur = series;
    if (!cur || (cur.ts.length && s.ts <= cur.ts[cur.ts.length - 1])) return;
    const from = s.ts - RANGE_SECS[r];
    let start = cur.ts.findIndex((t) => t >= from);
    if (start < 0) start = cur.ts.length;
    const keep = <T,>(arr: T[], v: T) => [...arr.slice(start), v];
    series = {
      ts: keep(cur.ts, s.ts),
      cpu: keep(cur.cpu, s.cpu),
      mem: keep(cur.mem, s.mem),
      mem_limit: keep(cur.mem_limit, s.mem_limit),
      net_rx: keep(cur.net_rx, s.net_rx),
      net_tx: keep(cur.net_tx, s.net_tx),
      blk_r: keep(cur.blk_r, s.blk_read),
      blk_w: keep(cur.blk_w, s.blk_write),
    };
  }

  // Short ranges follow the live feed.
  $effect(() => {
    const s = live;
    const r = range;
    if (s && (r === '15m' || r === '1h')) untrack(() => append(s, r));
  });

  // Periodic refresh for long ranges (new rollups arrive slowly).
  $effect(() => {
    if (range === '15m' || range === '1h') return;
    const r = range;
    const t = setInterval(() => {
      if (!document.hidden) void load(r);
    }, 5 * 60_000);
    return () => clearInterval(t);
  });

  const limit = $derived(detail?.memory_limit ?? null);
  const hasData = $derived(!!series && series.ts.length > 1);

  const cpuSeries: ChartSeries[] = [{ label: 'CPU', color: 'var(--accent)', format: (v) => pct(v, 1) }];
  const memSeries = $derived<ChartSeries[]>([
    { label: 'Memory', color: 'var(--info)', format: (v) => bytes(v) },
    ...(limit ? [{ label: 'Limit', color: 'var(--bad)', fill: false, format: (v: number) => bytes(v) }] : []),
  ]);
  const netSeries: ChartSeries[] = [
    { label: 'Received', color: 'var(--ok)', format: (v) => rate(v) },
    { label: 'Sent', color: 'var(--update)', format: (v) => rate(v) },
  ];
  const diskSeries: ChartSeries[] = [
    { label: 'Read', color: 'var(--paused)', format: (v) => rate(v) },
    { label: 'Written', color: 'var(--warn)', format: (v) => rate(v) },
  ];

  const cpuData = $derived(series ? [series.ts, series.cpu] : []);
  const memData = $derived(series ? (limit ? [series.ts, series.mem, series.ts.map(() => limit)] : [series.ts, series.mem]) : []);
  const netData = $derived(series ? [series.ts, series.net_rx, series.net_tx] : []);
  const diskData = $derived(series ? [series.ts, series.blk_r, series.blk_w] : []);
</script>

<div class="stats">
  <div class="toolbar">
    <Segmented
      label="Time range"
      bind:value={range}
      options={[
        { value: '15m', label: '15 min' },
        { value: '1h', label: '1 hour' },
        { value: '6h', label: '6 hours' },
        { value: '24h', label: '24 hours' },
        { value: '7d', label: '7 days' },
        { value: '30d', label: '30 days' },
      ]}
    />
    {#if loading}<Spinner size={14} />{/if}
    {#if !historyOn}
      <span class="faint note">History is off, so only live numbers are shown. <a href={href('/settings/monitoring')}>Turn it on</a></span>
    {/if}
  </div>

  <div class="tiles">
    <div class="tile">
      <span class="label">CPU</span>
      <span class="value num">{up && live ? pct(live.cpu, 1) : '–'}</span>
      <span class="sub faint">100% is one full core</span>
    </div>
    <div class="tile">
      <span class="label">Memory</span>
      <span class="value num">{up && live ? bytes(live.mem) : '–'}</span>
      <span class="sub faint">{limit ? `of ${bytes(limit)} limit` : 'no limit set'}</span>
    </div>
    <div class="tile">
      <span class="label">Network</span>
      <span class="value num">{up && live ? `↓ ${rate(live.net_rx)}` : '–'}</span>
      <span class="sub faint num">{up && live ? `↑ ${rate(live.net_tx)}` : ''}</span>
    </div>
    <div class="tile">
      <span class="label">Disk</span>
      <span class="value num">{up && live ? `R ${rate(live.blk_read)}` : '–'}</span>
      <span class="sub faint num">{up && live ? `W ${rate(live.blk_write)}` : ''}</span>
    </div>
    <div class="tile">
      <span class="label">Processes</span>
      <span class="value num">{up && live ? live.pids : '–'}</span>
      <span class="sub faint">&nbsp;</span>
    </div>
  </div>

  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !loading && !hasData}
    <EmptyState icon={ChartLine} title="No history for this range yet">
      {#if !up}It isn't running, so nothing is being recorded.{:else if historyOn}Samples are recorded every {session.settings?.monitoring.sample_secs ?? 60} seconds; the charts fill in shortly.{:else}History is turned off.{/if}
    </EmptyState>
  {:else if series}
    <div class="charts">
      <section class="panel">
        <div class="panel-head"><h3>CPU</h3></div>
        <div class="panel-body"><Chart data={cpuData} series={cpuSeries} yFormat={(v) => pct(v, v > 0 && v < 10 ? 1 : 0)} yMinTop={1} label="CPU use over time" /></div>
      </section>
      <section class="panel">
        <div class="panel-head"><h3>Memory</h3></div>
        <div class="panel-body"><Chart data={memData} series={memSeries} yFormat={(v) => bytes(v, 0)} yMinTop={1024 * 1024} label="Memory use over time" /></div>
      </section>
      <section class="panel">
        <div class="panel-head"><h3>Network</h3></div>
        <div class="panel-body"><Chart data={netData} series={netSeries} yFormat={(v) => rate(v)} yMinTop={1024} label="Network traffic over time" /></div>
      </section>
      <section class="panel">
        <div class="panel-head"><h3>Disk</h3></div>
        <div class="panel-body"><Chart data={diskData} series={diskSeries} yFormat={(v) => rate(v)} yMinTop={1024} label="Disk reads and writes over time" /></div>
      </section>
    </div>
  {/if}
</div>

<style>
  .stats {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }
  .note {
    font-size: var(--fs-sm);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 12px;
  }
  .tile {
    display: grid;
    gap: 2px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .label {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .value {
    font-size: 22px;
    font-weight: 600;
  }
  .sub {
    font-size: var(--fs-sm);
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
    gap: 16px;
  }
  .charts .panel-body {
    padding: 10px 12px 6px;
  }
  @media (max-width: 760px) {
    .charts {
      grid-template-columns: 1fr;
    }
  }
</style>
