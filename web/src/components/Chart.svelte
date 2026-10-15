<!--
  Time-series chart (uPlot, loaded on first use). `data[0]` holds unix
  seconds; each further array is one series. Colors come from the theme, and
  the chart follows its container's width.
-->
<script lang="ts" module>
  export interface ChartSeries {
    label: string;
    /** CSS color or var(--token). */
    color: string;
    fill?: boolean;
    /** Formats values in the legend and tooltip. */
    format?: (v: number) => string;
  }
</script>

<script lang="ts">
  import { onDestroy } from 'svelte';
  import type uPlotType from 'uplot';
  import { dateTime, tickLabel } from '../lib/format';
  import { activeTheme } from '../lib/theme';
  import Spinner from './Spinner.svelte';

  interface Props {
    data: number[][];
    series: ChartSeries[];
    height?: number;
    yFormat?: (v: number) => string;
    /** Fixed top of the y axis (like 100 for percentages). */
    yMax?: number;
    /** The y axis reaches at least this high, so flat or empty data doesn't look noisy. */
    yMinTop?: number;
    label?: string;
  }

  let { data, series, height = 200, yFormat = (v) => String(v), yMax, yMinTop = 1, label = 'Chart' }: Props = $props();

  let host: HTMLDivElement | undefined = $state();
  let plot: uPlotType | null = null;
  let loading = $state(true);
  let theme = $state(activeTheme());

  function cssColor(value: string): string {
    if (!value.startsWith('var(')) return value;
    const name = value.slice(4, -1).trim();
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#888';
  }

  function withAlpha(color: string, alpha: number): string {
    const c = color.trim();
    if (/^#[0-9a-f]{6}$/i.test(c)) {
      const n = parseInt(c.slice(1), 16);
      return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
    }
    return c;
  }

  async function build() {
    if (!host) return;
    const { default: uPlot } = await import('uplot');
    await import('uplot/dist/uPlot.min.css');
    if (!host) return;
    plot?.destroy();
    const grid = cssColor('var(--line)');
    const text = cssColor('var(--text-3)');
    const font = '12px "Barlow Semi Condensed", sans-serif';
    const opts: uPlotType.Options = {
      width: host.clientWidth || 600,
      height,
      cursor: { drag: { x: false, y: false }, points: { size: 6 } },
      legend: { show: true, live: true },
      scales: {
        x: { time: true },
        y: { range: (_u, min, max) => [Math.min(0, min), yMax ?? Math.max(max * 1.1, yMinTop)] },
      },
      axes: [
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid, width: 1 },
          font,
          values: (u, vals) => {
            const span = (u.scales.x.max ?? 0) - (u.scales.x.min ?? 0);
            return vals.map((v) => tickLabel(v, span));
          },
        },
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { show: false },
          font,
          size: 60,
          values: (_u, vals) => vals.map((v) => (v === null ? '' : yFormat(v))),
        },
      ],
      series: [
        { value: (_u, v) => (v === null ? '–' : dateTime(v)) },
        ...series.map((s) => {
          const stroke = cssColor(s.color);
          return {
            label: s.label,
            stroke,
            width: 1.6,
            fill: s.fill === false ? undefined : withAlpha(stroke, 0.12),
            points: { show: false },
            value: (_u: uPlotType, v: number | null) => (v === null || v === undefined ? '–' : (s.format ?? yFormat)(v)),
          };
        }),
      ],
    };
    plot = new uPlot(opts, data as uPlotType.AlignedData, host);
    loading = false;
  }

  // Rebuild on theme or series changes; push new data otherwise.
  $effect(() => {
    void theme;
    void series.length;
    void build();
  });

  $effect(() => {
    if (plot && data) plot.setData(data as uPlotType.AlignedData);
  });

  $effect(() => {
    if (!host) return;
    const ro = new ResizeObserver(() => {
      if (plot && host) plot.setSize({ width: host.clientWidth, height });
    });
    ro.observe(host);
    const mo = new MutationObserver(() => (theme = activeTheme()));
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  });

  onDestroy(() => plot?.destroy());
</script>

<div class="chart" bind:this={host} style:min-height="{height + 34}px" role="img" aria-label={label}>
  {#if loading}<div class="loading"><Spinner size={14} /></div>{/if}
</div>

<style>
  .chart {
    position: relative;
    width: 100%;
  }
  .loading {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--text-3);
  }
  .chart :global(.u-legend) {
    color: var(--text-2);
    font: 500 var(--fs-sm) / 1.3 var(--font-data);
    text-align: left;
    padding: 4px 0 0;
  }
  .chart :global(.u-legend .u-marker) {
    border-radius: 2px;
  }
  .chart :global(.u-legend th) {
    font-weight: 500;
  }
  .chart :global(.u-select) {
    background: var(--accent-soft);
  }
</style>
