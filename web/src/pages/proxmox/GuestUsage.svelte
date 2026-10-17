<!--
  How a node's CPU, memory, disk and network split across its guests, as
  four donuts with one legend. Pointing at a guest (in a donut or the
  legend) highlights it in all four. In a cluster, a picker switches nodes.
-->
<script lang="ts">
  import Donut from '../../components/Donut.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import { bytes, pct, plural } from '../../lib/format';
  import { breakdown, featuredGuests, guestsOn, METRICS, nodeCapacities, type Breakdown, type Slice, type UsageMetric } from '../../lib/pveusage';
  import type { PveOverview } from '../../lib/types';

  let { overview }: { overview: PveOverview } = $props();

  const caps = $derived(nodeCapacities(overview));
  let picked = $state('');
  const cap = $derived(caps.find((c) => c.node === picked) ?? caps.find((c) => c.node === overview.node.node) ?? caps[0]);
  const guests = $derived(guestsOn(overview, cap.node));
  const featured = $derived(featuredGuests(guests));
  const charts = $derived(METRICS.map((m) => breakdown(m, cap, guests, featured)));
  const others = $derived(Math.max(0, ...charts.map((c) => c.others)));
  const hasHost = $derived(charts.some((c) => c.slices.some((s) => s.kind === 'host')));
  const hasVms = $derived(guests.some((g) => g.kind === 'qemu'));

  let hover = $state<string | null>(null);

  const TITLE: Record<UsageMetric, string> = { cpu: 'CPU', mem: 'Memory', disk: 'Disk', net: 'Network' };

  function color(s: Pick<Slice, 'kind' | 'slot'>): string {
    if (s.kind === 'guest') return `var(--usage-${(s.slot ?? 0) + 1})`;
    if (s.kind === 'other') return 'var(--text-3)';
    if (s.kind === 'host') return 'var(--line-strong)';
    return 'var(--sunken)';
  }

  function cores(n: number): string {
    return n < 10 ? n.toFixed(1) : n.toFixed(0);
  }

  function amount(metric: UsageMetric, v: number): string {
    return metric === 'cpu' ? `${cores(v)} ${v === 1 ? 'core' : 'cores'}` : bytes(v);
  }

  function caption(b: Breakdown): string {
    switch (b.metric) {
      case 'cpu':
        return `${cores(b.used)} of ${plural(cap.cpus, 'core')} in use`;
      case 'mem':
        return `${bytes(b.used)} of ${bytes(b.total)} in use`;
      case 'disk':
        return hasVms ? 'used by containers (VMs don’t report it)' : 'used by guests';
      case 'net':
        return 'in and out since each guest started';
    }
  }

  function describe(b: Breakdown): string {
    const parts = b.slices.map((s) => `${s.label} ${amount(b.metric, s.value)}`);
    return `${TITLE[b.metric]} on ${cap.node}: ${parts.length ? parts.join(', ') : 'nothing reported'}`;
  }

  const legend = $derived([
    ...featured.map((g, slot) => ({ key: `g${g.vmid}`, kind: 'guest' as const, slot, label: g.name || String(g.vmid), note: String(g.vmid), title: '' })),
    ...(others ? [{ key: 'other', kind: 'other' as const, slot: undefined, label: 'Other', note: plural(others, 'guest'), title: '' }] : []),
    ...(hasHost
      ? [{ key: 'host', kind: 'host' as const, slot: undefined, label: 'Host', note: '', title: 'Used by the node itself rather than a guest: Proxmox services, ZFS cache and the like' }]
      : []),
    { key: 'free', kind: 'free' as const, slot: undefined, label: 'Free', note: '', title: '' },
  ]);
</script>

<section class="panel usage">
  <div class="panel-head">
    <h2>Usage by guest</h2>
    {#if caps.length > 1}
      <Segmented
        label="Node"
        options={caps.map((c) => ({ value: c.node, label: c.online ? c.node : `${c.node} (offline)` }))}
        value={cap.node}
        onchange={(v) => {
          picked = v;
          hover = null;
        }}
      />
    {/if}
  </div>
  <div class="panel-body">
    <div class="charts">
      {#each charts as b (b.metric)}
        {@const s = hover ? b.slices.find((x) => x.key === hover) : null}
        {@const share = b.metric === 'cpu' || b.metric === 'mem'}
        <figure>
          <figcaption>{TITLE[b.metric]}</figcaption>
          <Donut
            label={describe(b)}
            total={b.total}
            active={hover}
            onhover={(k) => (hover = k)}
            slices={b.slices.map((x) => ({ key: x.key, value: x.value, color: color(x) }))}
          >
            {#if hover}
              <span class="big num">{s && b.total ? pct((s.value / b.total) * 100, s.value / b.total < 0.1 ? 1 : 0) : '–'}</span>
              <span class="small num">{s ? amount(b.metric, s.value) : 'none'}</span>
            {:else if share}
              <span class="big num">{b.total ? pct((b.used / b.total) * 100) : '–'}</span>
              <span class="small">in use</span>
            {:else}
              <span class="big num">{b.total ? bytes(b.total) : '–'}</span>
              <span class="small">total</span>
            {/if}
          </Donut>
          <p class="caption">{caption(b)}</p>
        </figure>
      {/each}
    </div>

    <ul class="legend" aria-label="Legend">
      {#each legend as l (l.key)}
        <li>
          <button
            type="button"
            class:active={hover === l.key}
            title={l.title || undefined}
            onpointerenter={() => (hover = l.key)}
            onpointerleave={() => (hover = null)}
            onfocus={() => (hover = l.key)}
            onblur={() => (hover = null)}
          >
            <span class="swatch" class:free={l.kind === 'free'} style:background={color(l)}></span>
            <span>{l.label}</span>
            {#if l.note}<span class="faint num">{l.note}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  </div>
</section>

<style>
  /* Guest colors: muted, away from the state colors and the accent, and
     checked for color-blind separation between neighbors on each theme's panel. */
  .usage {
    --usage-1: #4b9bd8;
    --usage-2: #cf7a3e;
    --usage-3: #2fa59b;
    --usage-4: #7584d6;
    --usage-5: #ad8d29;
    margin-bottom: 20px;
  }
  :global(:root[data-theme='light']) .usage {
    --usage-1: #2f7fbf;
    --usage-2: #b8622a;
    --usage-3: #00907f;
    --usage-4: #4f5fb8;
    --usage-5: #957617;
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(136px, 100%), 1fr));
    gap: 20px 16px;
  }
  figure {
    display: grid;
    min-width: 0;
    justify-items: center;
    align-content: start;
    gap: 10px;
    margin: 0;
  }
  figcaption {
    justify-self: stretch;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .big {
    font: 600 var(--fs-lg) / 1.1 var(--font-data);
  }
  .small {
    color: var(--text-2);
    font-size: var(--fs-xs);
  }
  .caption {
    max-width: 22ch;
    margin: 0;
    color: var(--text-2);
    font-size: var(--fs-sm);
    text-align: center;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 6px;
    margin: 18px 0 0;
    padding: 12px 0 0;
    border-top: 1px solid var(--line);
    list-style: none;
  }
  .legend button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: default;
  }
  .legend button:hover,
  .legend button.active {
    background: var(--raised);
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }
  .swatch.free {
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }
</style>
