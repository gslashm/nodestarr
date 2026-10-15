<!--
  Proxmox history charts (RRD data) for the node or one guest: CPU,
  memory, network and disk or I/O wait, over an hour to a year.
-->
<script lang="ts">
  import Chart, { type ChartSeries } from '../../components/Chart.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage, qs } from '../../lib/api';
  import { bytes, pct, rate } from '../../lib/format';

  let { vmid = null }: { vmid?: number | null } = $props();

  type Timeframe = 'hour' | 'day' | 'week' | 'month' | 'year';
  type Point = Record<string, number | undefined>;

  let timeframe = $state<Timeframe>('hour');
  let points = $state.raw<Point[] | null>(null);
  let error = $state('');

  $effect(() => {
    const tf = timeframe;
    const id = vmid;
    points = null;
    error = '';
    api
      .get<{ points: Point[] }>(`/proxmox/rrd${qs({ vmid: id, timeframe: tf })}`)
      .then((r) => {
        if (tf === timeframe && id === vmid) points = r.points.filter((p) => typeof p.time === 'number');
      })
      .catch((err) => (error = errorMessage(err)));
  });

  /** One column of the data; missing samples become gaps. */
  function col(key: string, scale = 1): (number | null)[] {
    return (points ?? []).map((p) => (typeof p[key] === 'number' ? (p[key] as number) * scale : null));
  }

  const ts = $derived((points ?? []).map((p) => p.time as number));
  const node = $derived(vmid === null);

  const cpu = $derived({ data: [ts, col('cpu', 100)], series: [{ label: 'CPU', color: 'var(--info)', format: (v: number) => pct(v, 1) }] as ChartSeries[] });
  const mem = $derived({
    data: node ? [ts, col('memused'), col('memtotal')] : [ts, col('mem'), col('maxmem')],
    series: [
      { label: 'Used', color: 'var(--accent)', format: (v: number) => bytes(v) },
      { label: 'Total', color: 'var(--text-3)', fill: false, format: (v: number) => bytes(v) },
    ] as ChartSeries[],
  });
  const net = $derived({
    data: [ts, col('netin'), col('netout')],
    series: [
      { label: 'In', color: 'var(--ok)', format: (v: number) => rate(v) },
      { label: 'Out', color: 'var(--update)', format: (v: number) => rate(v) },
    ] as ChartSeries[],
  });
  const io = $derived(
    node
      ? { data: [ts, col('iowait', 100)], series: [{ label: 'I/O wait', color: 'var(--warn)', format: (v: number) => pct(v, 1) }] as ChartSeries[], fmt: (v: number) => pct(v, 1), top: 1 }
      : {
          data: [ts, col('diskread'), col('diskwrite')],
          series: [
            { label: 'Read', color: 'var(--paused)', format: (v: number) => rate(v) },
            { label: 'Written', color: 'var(--warn)', format: (v: number) => rate(v) },
          ] as ChartSeries[],
          fmt: (v: number) => rate(v),
          top: 1024,
        },
  );
</script>

<div class="rrd">
  <Segmented
    label="Time range"
    bind:value={timeframe}
    options={[
      { value: 'hour', label: 'Hour' },
      { value: 'day', label: 'Day' },
      { value: 'week', label: 'Week' },
      { value: 'month', label: 'Month' },
      { value: 'year', label: 'Year' },
    ]}
  />
  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !points}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else if points.length < 2}
    <p class="muted">No history for this range.</p>
  {:else}
    <div class="charts">
      <section><h4>CPU</h4><Chart data={cpu.data as number[][]} series={cpu.series} yFormat={(v) => pct(v)} yMinTop={1} height={150} label="CPU" /></section>
      <section><h4>Memory</h4><Chart data={mem.data as number[][]} series={mem.series} yFormat={(v) => bytes(v, 0)} yMinTop={1024 * 1024} height={150} label="Memory" /></section>
      <section><h4>Network</h4><Chart data={net.data as number[][]} series={net.series} yFormat={(v) => rate(v)} yMinTop={1024} height={150} label="Network" /></section>
      <section><h4>{node ? 'I/O wait' : 'Disk'}</h4><Chart data={io.data as number[][]} series={io.series} yFormat={io.fmt} yMinTop={io.top} height={150} label={node ? 'I/O wait' : 'Disk'} /></section>
    </div>
  {/if}
</div>

<style>
  .rrd {
    display: grid;
    gap: 12px;
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 16px;
  }
  h4 {
    margin: 0 0 4px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
</style>
