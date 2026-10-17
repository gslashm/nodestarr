<!--
  One disk: its verdict and findings, headline numbers, history charts,
  self-tests (Nodestarr's and the drive's own log), identity, a label for
  where it sits and its warranty, the SMART attributes explained, and
  everything that happened to it.
-->
<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ClipboardCopy from '@lucide/svelte/icons/clipboard-copy';
  import Download from '@lucide/svelte/icons/download';
  import FlaskConical from '@lucide/svelte/icons/flask-conical';
  import Lightbulb from '@lucide/svelte/icons/lightbulb';
  import Moon from '@lucide/svelte/icons/moon';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { untrack } from 'svelte';
  import Button from '../../../components/Button.svelte';
  import Chart, { type ChartSeries } from '../../../components/Chart.svelte';
  import Menu, { type MenuEntry } from '../../../components/Menu.svelte';
  import Segmented from '../../../components/Segmented.svelte';
  import Spinner from '../../../components/Spinner.svelte';
  import { api, apiUrl, errorMessage } from '../../../lib/api';
  import {
    ATA_ATTRIBUTES,
    CRITICAL_ATTRIBUTES,
    LEVELS,
    MEDIA,
    TEST_KIND,
    TEST_STATUS,
    ageFrom,
    attributeTone,
    diskSize,
    powerOn,
    roleLabel,
    tempTone,
    testLength,
    testRemaining,
    verdictOf,
    warranty,
  } from '../../../lib/disks';
  import { ago, bytes, dateTime, duration, plural } from '../../../lib/format';
  import { live } from '../../../lib/live.svelte';
  import { href, router } from '../../../lib/router.svelte';
  import { session } from '../../../lib/session.svelte';
  import type { DiskDetail, DiskHistoryPoint, DiskLabel, DiskView, SmartReport } from '../../../lib/types';
  import { ui } from '../../../lib/ui.svelte';
  import HealthBadge from './HealthBadge.svelte';

  let { id }: { id: string } = $props();

  let detail = $state<DiskDetail | null>(null);
  let error = $state('');
  let range = $state<'7' | '30' | '90' | '365' | '3650'>('30');
  const days = $derived(Number(range));
  let points = $state.raw<DiskHistoryPoint[] | null>(null);
  let label = $state<DiskLabel>({ bay: '', notes: '', purchased: '', warranty_until: '' });
  let labelBase = $state('');
  let savingLabel = $state(false);

  const enc = $derived(encodeURIComponent(id));

  async function load() {
    try {
      const r = await api.get<DiskDetail>(`/proxmox/disk-health/disks/${enc}`);
      detail = r;
      error = '';
      // Follow saved changes unless the label is being edited here.
      if (JSON.stringify(label) === labelBase || !labelBase) {
        label = { ...r.disk.label };
        labelBase = JSON.stringify(r.disk.label);
      }
    } catch (err) {
      error = errorMessage(err);
    }
  }

  let timer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    void id;
    void live.disksVersion;
    untrack(() => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void load(), detail?.disk.id === id ? 400 : 0);
    });
  });
  $effect(() => {
    const t = setInterval(() => {
      if (!document.hidden && detail?.test) void load();
    }, 30_000);
    return () => clearInterval(t);
  });
  $effect(() => {
    const range = days;
    const disk = enc;
    points = null;
    api
      .get<{ points: DiskHistoryPoint[] }>(`/proxmox/disk-health/disks/${disk}/history?days=${range}`)
      .then((r) => {
        if (range === days && disk === enc) points = r.points;
      })
      .catch(() => (points = []));
  });

  const d = $derived(detail?.disk ?? null);
  const r = $derived(detail?.report ?? null);
  const v = $derived(d ? verdictOf(d) : null);
  const problems = $derived((d?.findings ?? []).filter((f) => f.level !== 'info' && f.level !== 'ok'));
  const notes = $derived((d?.findings ?? []).filter((f) => f.level === 'info'));
  const test = $derived(detail?.test ?? null);
  const flash = $derived(d?.media === 'ssd' || d?.media === 'nvme');
  const w = $derived(warranty(label.warranty_until));
  const labelDirty = $derived(JSON.stringify(label) !== labelBase);

  async function startTest(kind: 'short' | 'long' | 'conveyance') {
    if (!d) return;
    const minutes = kind === 'long' ? d.tests.long_minutes : kind === 'conveyance' ? d.tests.conveyance_minutes : d.tests.short_minutes;
    if (kind === 'long') {
      const ok = await ui.confirm({
        title: `Run a long self-test on ${d.devpath}?`,
        message: `The disk reads its whole surface${minutes ? `, which takes about ${testLength(minutes)}` : ''}. It stays usable meanwhile, a bit slower. If it spins down before the test is done, the test stops early.`,
        confirmLabel: 'Start long test',
      });
      if (!ok) return;
    }
    try {
      await api.post(`/proxmox/disk-health/disks/${enc}/tests`, { kind });
      ui.toast(`${TEST_KIND[kind]} self-test queued`, 'success');
      void load();
    } catch (err) {
      ui.error(err, "Couldn't start the test");
    }
  }

  async function abort() {
    if (!test) return;
    const ok = await ui.confirm({
      title: test.status === 'queued' ? 'Cancel the waiting test?' : `Abort the ${TEST_KIND[test.kind]?.toLowerCase()} test?`,
      confirmLabel: test.status === 'queued' ? 'Cancel test' : 'Abort test',
      danger: test.status !== 'queued',
    });
    if (!ok) return;
    try {
      await api.post(`/proxmox/disk-health/disks/${enc}/tests/abort`);
      void load();
    } catch (err) {
      ui.error(err);
    }
  }

  async function identify() {
    try {
      await api.post(`/proxmox/disk-health/disks/${enc}/identify`, { seconds: 30 });
      ui.toast(`Reading from ${d?.devpath} for 30 seconds: look for the blinking activity light.`, 'success');
    } catch (err) {
      ui.error(err, "Couldn't blink the disk");
    }
  }

  async function forget() {
    if (!d) return;
    const ok = await ui.confirm({
      title: `Forget ${d.model ?? d.devpath}?`,
      message: 'Its SMART history, test results and events are deleted.',
      confirmLabel: 'Forget disk',
      danger: true,
    });
    if (!ok) return;
    try {
      await api.del(`/proxmox/disk-health/disks/${enc}`);
      router.navigate('/proxmox/disks');
    } catch (err) {
      ui.error(err);
    }
  }

  async function saveLabel() {
    savingLabel = true;
    try {
      const res = await api.put<{ disk: DiskView }>(`/proxmox/disk-health/disks/${enc}/label`, label);
      label = { ...res.disk.label };
      labelBase = JSON.stringify(res.disk.label);
      if (detail) detail.disk = res.disk;
      ui.toast('Saved', 'success');
    } catch (err) {
      ui.error(err, "Couldn't save");
    } finally {
      savingLabel = false;
    }
  }

  function testMenu(): MenuEntry[] {
    if (!d) return [];
    const busy = !!test;
    return (['short', 'long', 'conveyance'] as const)
      .filter((k) => k !== 'conveyance' || d.tests.conveyance)
      .map((kind) => {
        const minutes = kind === 'long' ? d.tests.long_minutes : kind === 'conveyance' ? d.tests.conveyance_minutes : d.tests.short_minutes;
        return {
          label: `${TEST_KIND[kind]} test${minutes ? ` (about ${testLength(minutes)})` : ''}`,
          icon: FlaskConical,
          disabled: busy || !d.tests.available || (kind === 'long' ? !d.tests.long : !d.tests.short),
          hint: busy ? 'A test is already running or waiting' : (d.tests.reason ?? undefined),
          run: () => startTest(kind),
        };
      });
  }

  function moreMenu(): MenuEntry[] {
    const items: MenuEntry[] = [{ label: 'Copy summary as text', icon: ClipboardCopy, run: copySummary }];
    if (detail?.has_raw) items.push({ label: 'Download smartctl output (JSON)', icon: Download, href: apiUrl(`/proxmox/disk-health/disks/${enc}/raw`) });
    if (d && !d.present && session.isAdmin) items.push('separator', { label: 'Forget this disk', icon: Trash2, danger: true, run: forget });
    return items;
  }

  /** A plain-text summary, handy for a forum post or a support ticket. */
  function summary(): string {
    if (!d) return '';
    const lines = [
      `${d.model ?? 'Unknown model'}${d.serial ? ` (${d.serial})` : ''}, ${diskSize(d.size)} ${MEDIA[d.media]}${r?.firmware ? `, firmware ${r.firmware}` : ''}`,
      `Health: ${v?.label}${problems[0] ? ` (${problems[0].title})` : ''}; SMART overall: ${d.passed === null ? 'unknown' : d.passed ? 'PASSED' : 'FAILED'}`,
      `Power-on: ${powerOn(d.power_on_hours)} (${d.power_on_hours ?? '?'} h), ${d.power_cycles ?? '?'} power cycles`,
      `Temperature: ${d.temperature ?? '?'} °C${r?.temp_min_lifetime !== null && r?.temp_max_lifetime !== null && r ? ` (lifetime ${r.temp_min_lifetime}–${r.temp_max_lifetime} °C)` : ''}`,
      `Reallocated: ${d.reallocated ?? '–'}, pending: ${d.pending ?? '–'}, uncorrectable: ${d.uncorrectable ?? '–'}, CRC errors: ${d.crc_errors ?? '–'}`,
    ];
    if (d.wear_used !== null) lines.push(`Wear: ${d.wear_used}% of rated endurance used${d.bytes_written ? `, ${diskSize(d.bytes_written)} written` : ''}`);
    if (d.last_test) lines.push(`Last self-test: ${TEST_KIND[d.last_test.kind] ?? d.last_test.kind}, ${d.last_test.status || d.last_test.outcome}${d.last_test.at ? `, ${ago(d.last_test.at)}` : ''}`);
    for (const f of [...problems, ...notes]) lines.push(`- ${f.title}`);
    if (r?.attributes.length) {
      lines.push('', 'ID# ATTRIBUTE_NAME              VALUE WORST THRESH RAW');
      for (const a of r.attributes) {
        lines.push(`${String(a.id).padStart(3)} ${a.name.padEnd(26)} ${String(a.value ?? '').padStart(5)} ${String(a.worst ?? '').padStart(5)} ${String(a.thresh ?? '').padStart(6)} ${a.raw}`);
      }
    }
    return lines.join('\n');
  }

  async function copySummary() {
    try {
      await navigator.clipboard.writeText(summary());
      ui.toast('Copied', 'success');
    } catch {
      ui.toast("Couldn't copy (the clipboard needs HTTPS)", 'warning');
    }
  }

  // ---- charts ----
  const ts = $derived((points ?? []).map((p) => p.ts));
  function col(get: (p: DiskHistoryPoint) => number | null): (number | null)[] {
    return (points ?? []).map(get);
  }
  function has(values: (number | null)[]): boolean {
    return values.some((x) => x !== null);
  }
  /** Counters only get a chart once they were ever above zero. */
  function counted(values: (number | null)[]): boolean {
    return values.some((x) => x !== null && x > 0);
  }
  const intFmt = (n: number) => Math.round(n).toLocaleString();
  const temps = $derived({
    data: [ts, col((p) => (p.temp === null ? null : Math.round(p.temp * 10) / 10)), col((p) => p.temp_max)],
    series: [
      { label: 'Average', color: 'var(--info)', format: (n: number) => `${n.toFixed(1)} °C` },
      { label: 'Highest', color: 'var(--warn)', fill: false, format: (n: number) => `${n.toFixed(0)} °C` },
    ] as ChartSeries[],
  });
  const sectors = $derived({
    data: [ts, col((p) => p.realloc), col((p) => p.pending), col((p) => p.uncorrect)],
    series: [
      { label: 'Reallocated', color: 'var(--warn)', fill: false, format: intFmt },
      { label: 'Pending', color: 'var(--bad)', fill: false, format: intFmt },
      { label: 'Uncorrectable', color: 'var(--update)', fill: false, format: intFmt },
    ] as ChartSeries[],
  });
  const crc = $derived({ data: [ts, col((p) => p.crc)], series: [{ label: 'Cable errors', color: 'var(--paused)', format: intFmt }] as ChartSeries[] });
  const wear = $derived({ data: [ts, col((p) => p.wear)], series: [{ label: 'Endurance used', color: 'var(--accent)', format: (n: number) => `${n.toFixed(0)}%` }] as ChartSeries[] });
  const cycles = $derived({ data: [ts, col((p) => p.cycles)], series: [{ label: 'Head parks', color: 'var(--text-3)', format: intFmt }] as ChartSeries[] });
  const written = $derived({ data: [ts, col((p) => p.written)], series: [{ label: 'Written', color: 'var(--ok)', format: (n: number) => diskSize(n) }] as ChartSeries[] });
  const asleep = $derived({ data: [ts, col((p) => Math.round(p.asleep * 100))], series: [{ label: 'Asleep', color: 'var(--paused)', format: (n: number) => `${n}% of checks` }] as ChartSeries[] });
  const sleptAtAll = $derived((points ?? []).some((p) => p.asleep > 0));

  // ---- identity ----
  function sectorText(rep: SmartReport | null): string {
    if (!rep?.logical_sector) return '';
    const l = rep.logical_sector;
    const p = rep.physical_sector ?? l;
    if (p === l) return `${bytes(l)}`;
    return `${bytes(l)} logical, ${bytes(p)} physical${l === 512 && p === 4096 ? ' (512e)' : ''}`;
  }
  function erc(ds: number | null | undefined): string {
    if (ds === null || ds === undefined) return '–';
    return ds === 0 ? 'off' : `${(ds / 10).toFixed(1)} s`;
  }
  function nvmeWarning(cw: number | null): string {
    if (cw === null) return '–';
    if (cw === 0) return 'none';
    const bits = ['spare below threshold', 'temperature', 'reliability degraded', 'read-only', 'volatile backup failed', 'persistent memory read-only'];
    return bits.filter((_, i) => cw & (1 << i)).join(', ') || `0x${cw.toString(16)}`;
  }
  function attrStatus(tone: string, failed: string): string {
    if (failed === 'now') return 'Failing now';
    if (failed === 'past') return 'Failed in the past';
    if (tone === 'warn') return 'Watch';
    if (tone === 'bad') return 'Problem';
    return '';
  }
  /** When a drive-log entry happened, from the power-on hours. */
  function logWhen(hours: number | null): string {
    if (hours === null || !r?.power_on_hours || hours > r.power_on_hours) return '';
    return `≈ ${ago(r.collected_at - (r.power_on_hours - hours) * 3600)}`;
  }
</script>

<div class="detail">
  <a class="back" href={href('/proxmox/disks')}><ChevronLeft size={16} /> All disks</a>
  {#if !detail}
    {#if error}<p class="text-bad">{error}</p>{:else}<p class="faint"><Spinner size={13} /> Loading…</p>{/if}
  {:else if d && v}
    <div class="head">
      <div class="who">
        <h2>{d.model ?? 'Unknown disk'}</h2>
        <p class="meta">
          {#if d.serial}<span class="mono">{d.serial}</span> · {/if}{diskSize(d.size)} {MEDIA[d.media]}{d.transport ? ` (${d.transport.toUpperCase()})` : ''} ·
          <span class="mono">{d.devpath}</span> on {d.node}{#if d.label.bay} · <strong class="bay">{d.label.bay}</strong>{/if}
        </p>
      </div>
      <HealthBadge tone={v.tone} label={v.label} />
      <div class="actions">
        {#if session.isAdmin && d.present}
          <Menu items={testMenu()} label="Run a self-test">
            {#snippet trigger()}<span class="btn small"><FlaskConical size={15} /> Run test <ChevronDown size={14} /></span>{/snippet}
          </Menu>
          {#if test}<Button small variant="ghost" onclick={abort}>{test.status === 'queued' ? 'Cancel test' : 'Abort test'}</Button>{/if}
          <Button small variant="ghost" icon={Lightbulb} disabled={!d.tests.available && d.tests.reason?.startsWith('Proxmox')} onclick={identify} title="Reads from the disk for 30 seconds so its activity light blinks">Blink</Button>
        {/if}
        <Menu items={moreMenu()} small label="More actions" />
      </div>
    </div>

    {#if !d.present}
      <p class="banner idle"><TriangleAlert size={17} /><span>Not connected since {ago(d.last_seen)} (it was {d.devpath} on {d.node}). This is its last known state.</span></p>
    {:else if d.asleep}
      <p class="banner sleep"><Moon size={17} /><span>Asleep at the last check {ago(d.checked_at)}. Nodestarr didn't wake it; these readings are from {ago(d.sampled_at)}.</span></p>
    {:else if d.resting}
      <p class="banner sleep"><Moon size={17} /><span>Spins down between readings. Every reading through the Proxmox API wakes it, so Nodestarr reads it about once a day; these readings are from {ago(d.sampled_at)}.</span></p>
    {/if}
    {#if d.error}<p class="banner bad"><TriangleAlert size={17} /><span>The last read failed: {d.error}</span></p>{/if}

    {#if test}
      {@const left = testRemaining(test)}
      <section class="panel running">
        <div class="run-line">
          <strong>{TEST_KIND[test.kind]} self-test {test.status === 'running' ? 'running' : 'waiting'}</strong>
          <span class="faint small">
            {#if test.status === 'running'}
              {Math.round((test.progress ?? 0) * 100)}% done{left !== null ? ` · about ${duration(left)} left` : ''} · started {ago(test.started_at)}
            {:else}{test.message ?? 'Waiting to start'}{/if}
          </span>
        </div>
        {#if test.status === 'running'}
          <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((test.progress ?? 0) * 100)} aria-label="Self-test progress">
            <div class="fill" style:width="{Math.max(2, (test.progress ?? 0) * 100)}%"></div>
          </div>
        {/if}
      </section>
    {/if}

    {#if problems.length}
      <ul class="findings">
        {#each problems as f (f.code + f.title)}
          <li class={f.level}>
            <span class="lvl">{LEVELS[f.level].label}</span>
            <strong>{f.title}</strong>
            <span class="detail-text">{f.detail}</span>
          </li>
        {/each}
      </ul>
    {:else if d.sampled_at}
      <p class="all-good">Nothing to worry about in its SMART data{d.last_test?.outcome === 'passed' ? `, and its last ${TEST_KIND[d.last_test.kind]?.toLowerCase() ?? ''} self-test passed` : ''}.</p>
    {/if}
    {#if notes.length}
      <ul class="notes">
        {#each notes as f (f.code + f.title)}<li><strong>{f.title}.</strong> <span class="faint">{f.detail}</span></li>{/each}
      </ul>
    {/if}

    <div class="tiles">
      <section class="panel tile">
        <h3>Temperature</h3>
        <p class="big num">
          {#if d.temperature !== null}<span class="text-{tempTone(d.temperature, d.temp_limit) || 'ok'}">{d.temperature} °C</span>{:else}<span class="faint">–</span>{/if}
        </p>
        <p class="sub">
          limit {d.temp_limit ?? 50} °C{#if r?.temp_min_lifetime !== null && r?.temp_max_lifetime !== null && r} · lifetime {r.temp_min_lifetime}–{r.temp_max_lifetime} °C{/if}
        </p>
      </section>
      <section class="panel tile">
        <h3>Power-on time</h3>
        <p class="big num">{powerOn(d.power_on_hours)}</p>
        <p class="sub">{d.power_on_hours !== null ? `${d.power_on_hours.toLocaleString()} hours · ` : ''}{d.power_cycles !== null ? plural(d.power_cycles, 'power cycle') : ''}</p>
      </section>
      <section class="panel tile">
        <h3>Sectors</h3>
        <p class="big num">
          <span class:text-warn={(d.reallocated ?? 0) > 0}>{d.reallocated ?? '–'}</span><span class="faint sep">/</span><span class:text-bad={(d.pending ?? 0) > 0}>{d.pending ?? '–'}</span><span class="faint sep">/</span><span class:text-bad={(d.uncorrectable ?? 0) > 0}>{d.uncorrectable ?? '–'}</span>
        </p>
        <p class="sub">reallocated / pending / {d.media === 'nvme' ? 'media errors' : 'uncorrectable'}</p>
      </section>
      <section class="panel tile">
        <h3>Cable errors</h3>
        <p class="big num"><span class:text-warn={(d.crc_errors ?? 0) > 0}>{d.crc_errors ?? '–'}</span></p>
        <p class="sub">interface CRC errors{(d.crc_errors ?? 0) > 0 ? ' (usually the cable)' : ''}</p>
      </section>
      {#if flash}
        <section class="panel tile">
          <h3>Wear</h3>
          <p class="big num"><span class:text-warn={(d.wear_used ?? 0) >= 75} class:text-bad={(d.wear_used ?? 0) >= 90}>{d.wear_used !== null ? `${d.wear_used.toFixed(0)}%` : '–'}</span></p>
          <p class="sub">of rated endurance used{d.bytes_written ? ` · ${diskSize(d.bytes_written)} written` : ''}</p>
        </section>
      {:else}
        <section class="panel tile">
          <h3>Head parks</h3>
          <p class="big num">{d.load_cycles !== null ? d.load_cycles.toLocaleString() : '–'}</p>
          <p class="sub">load/unload cycles{d.bytes_written ? ` · ${diskSize(d.bytes_written)} written` : ''}</p>
        </section>
      {/if}
    </div>

    <section class="panel">
      <div class="panel-head">
        <h2>History</h2>
        <Segmented
          label="Time range"
          bind:value={range}
          options={[
            { value: '7', label: 'Week' },
            { value: '30', label: 'Month' },
            { value: '90', label: '3 months' },
            { value: '365', label: 'Year' },
            { value: '3650', label: 'All' },
          ]}
        />
      </div>
      <div class="panel-body">
        {#if !points}
          <p class="faint"><Spinner size={13} /> Loading…</p>
        {:else if points.length < 2}
          <p class="muted">Not enough history yet. Nodestarr reads the disk every hour by default; charts fill in from there.</p>
        {:else}
          <div class="charts">
            {#if has(temps.data[1])}<section><h4>Temperature</h4><Chart data={temps.data as number[][]} series={temps.series} yFormat={(n) => `${n.toFixed(0)}°`} yMinTop={40} yFloor={20} height={150} label="Temperature" /></section>{/if}
            {#if counted(sectors.data[1]) || counted(sectors.data[2]) || counted(sectors.data[3])}<section><h4>Bad sectors</h4><Chart data={sectors.data as number[][]} series={sectors.series} yFormat={intFmt} yMinTop={5} height={150} label="Bad sectors" /></section>{/if}
            {#if counted(crc.data[1])}<section><h4>Cable errors</h4><Chart data={crc.data as number[][]} series={crc.series} yFormat={intFmt} yMinTop={5} height={150} label="Cable errors" /></section>{/if}
            {#if flash && has(wear.data[1])}<section><h4>Endurance used</h4><Chart data={wear.data as number[][]} series={wear.series} yFormat={(n) => `${n.toFixed(0)}%`} yMinTop={10} height={150} label="Endurance used" /></section>{/if}
            {#if !flash && has(cycles.data[1])}<section><h4>Head parks</h4><Chart data={cycles.data as number[][]} series={cycles.series} yFormat={intFmt} yMinTop={10} height={150} label="Head parks" /></section>{/if}
            {#if has(written.data[1])}<section><h4>Data written</h4><Chart data={written.data as number[][]} series={written.series} yFormat={(n) => diskSize(n)} yMinTop={1e9} height={150} label="Data written" /></section>{/if}
            {#if sleptAtAll}<section><h4>Asleep</h4><Chart data={asleep.data as number[][]} series={asleep.series} yFormat={(n) => `${n.toFixed(0)}%`} yMax={100} height={150} label="Time asleep" /></section>{/if}
          </div>
        {/if}
      </div>
    </section>

    <div class="cols">
      <section class="panel">
        <div class="panel-head"><h2>Identity</h2></div>
        <dl class="kv panel-body">
          {#if r?.family}<dt>Family</dt><dd>{r.family}</dd>{/if}
          <dt>Model</dt><dd>{d.model ?? '–'}</dd>
          <dt>Serial</dt><dd class="mono">{d.serial ?? '–'}</dd>
          {#if d.wwn}<dt>WWN</dt><dd class="mono">{d.wwn}</dd>{/if}
          {#if d.firmware}<dt>Firmware</dt><dd class="mono">{d.firmware}</dd>{/if}
          <dt>Capacity</dt><dd>{diskSize(d.size)} <span class="faint">({bytes(d.size)})</span></dd>
          {#if sectorText(r)}<dt>Sectors</dt><dd>{sectorText(r)}</dd>{/if}
          <dt>Type</dt>
          <dd>{d.rpm ? `${d.rpm.toLocaleString()} rpm hard disk` : MEDIA[d.media]}{r?.form_factor ? `, ${r.form_factor}` : ''}{d.smr ? ' · SMR' : ''}</dd>
          {#if r?.link_max}<dt>Interface</dt><dd>{r.link_max}{r.link_now && r.link_now !== r.link_max ? `, running at ${r.link_now}` : ''}</dd>{/if}
          {#if r?.trim !== null && r?.trim !== undefined && flash}<dt>TRIM</dt><dd>{r.trim ? 'supported' : 'not supported'}</dd>{/if}
          {#if r && (r.erc_read !== null || r.erc_write !== null)}<dt title="SCT error recovery control (TLER): how long the disk retries a bad sector before reporting it">Error recovery</dt><dd>read {erc(r.erc_read)}, write {erc(r.erc_write)}</dd>{/if}
          <dt>Device</dt><dd><span class="mono">{d.devpath}</span> on {d.node}</dd>
          {#if d.by_id}<dt>By id</dt><dd class="mono small ellipsis" title={d.by_id}>{d.by_id.replace('/dev/disk/by-id/', '')}</dd>{/if}
          <dt>Used for</dt>
          <dd>
            {#if d.roles.length}{#each d.roles as role (role.pool + role.member)}<span class="chip" class:bad-chip={!role.ok}>{roleLabel(role)}</span> {/each}{:else}{d.used_by ?? 'nothing Proxmox knows of'}{/if}
            {#if d.osd !== null}<span class="chip">Ceph OSD {d.osd}</span>{/if}
          </dd>
          {#if d.mounts.length}<dt>Mounted at</dt><dd class="mono small">{d.mounts.join(', ')}</dd>{/if}
          <dt>Known since</dt><dd title={dateTime(d.first_seen)}>{ago(d.first_seen)}</dd>
          <dt>Read through</dt><dd>{d.source === 'helper' ? 'host access (smartctl)' : d.source === 'api' ? 'the Proxmox API' : '–'}{d.sampled_at ? `, ${ago(d.sampled_at)}` : ''}</dd>
        </dl>
      </section>

      <section class="panel">
        <div class="panel-head"><h2>Where it is</h2></div>
        <div class="panel-body label-form">
          <label class="lf">
            <span>Bay or slot</span>
            <input class="input" bind:value={label.bay} placeholder="Bay 3, top left…" disabled={!session.isAdmin} maxlength="60" />
          </label>
          <div class="dates">
            <label class="lf">
              <span>Bought</span>
              <input class="input" type="date" bind:value={label.purchased} disabled={!session.isAdmin} />
              {#if ageFrom(label.purchased)}<small class="faint">{ageFrom(label.purchased)}</small>{/if}
            </label>
            <label class="lf">
              <span>Warranty until</span>
              <input class="input" type="date" bind:value={label.warranty_until} disabled={!session.isAdmin} />
              {#if w}<small class="text-{w.tone}">{w.text}</small>{/if}
            </label>
          </div>
          <label class="lf">
            <span>Notes</span>
            <textarea class="input" rows="3" bind:value={label.notes} placeholder="Where it came from, RMA numbers…" disabled={!session.isAdmin}></textarea>
          </label>
          {#if session.isAdmin}
            <div><Button small variant={labelDirty ? 'primary' : ''} busy={savingLabel} disabled={!labelDirty} onclick={saveLabel}>Save</Button></div>
          {/if}
        </div>
      </section>
    </div>

    <section class="panel">
      <div class="panel-head"><h2>Self-tests</h2></div>
      {#if !d.tests.available && d.tests.reason}<p class="panel-body faint small reason">{d.tests.reason}</p>{/if}
      {#if detail.tests.length}
        <h3 class="table-title">Run by Nodestarr</h3>
        <div class="table-wrap">
          <table class="table">
            <thead><tr><th>Test</th><th>Result</th><th class="hide-sm">Started</th><th class="hide-sm">Took</th><th>Details</th></tr></thead>
            <tbody>
              {#each detail.tests as t (t.id)}
                {@const st = TEST_STATUS[t.status] ?? TEST_STATUS.unknown}
                <tr>
                  <td>{TEST_KIND[t.kind] ?? t.kind}{t.schedule ? '' : ''}</td>
                  <td><HealthBadge tone={st.tone} label={st.label} /></td>
                  <td class="hide-sm small" title={dateTime(t.started_at ?? t.queued_at)}>{ago(t.started_at ?? t.queued_at)}</td>
                  <td class="hide-sm small num">{t.started_at && t.finished_at ? duration(t.finished_at - t.started_at) : '–'}</td>
                  <td class="small">
                    {t.result ?? ''}{t.lba !== null ? ` at block ${t.lba.toLocaleString()}` : ''}{t.message ? `${t.result ? ' · ' : ''}${t.message}` : ''}
                    <span class="faint">{t.schedule ? ' · scheduled' : t.actor ? ` · ${t.actor.replace(/^user:/, '')}` : ''}</span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
      {#if r?.self_tests.log.length}
        <h3 class="table-title">The drive's own log</h3>
        <div class="table-wrap">
          <table class="table">
            <thead><tr><th>Test</th><th>Result</th><th class="num">At hour</th><th class="hide-sm">When</th><th class="hide-sm num">First error</th></tr></thead>
            <tbody>
              {#each r.self_tests.log as e, i (i)}
                {@const st = TEST_STATUS[e.outcome] ?? TEST_STATUS.unknown}
                <tr>
                  <td>{TEST_KIND[e.kind] ?? e.kind}</td>
                  <td><span class="text-{st.tone} small" title={st.label}>{e.status || st.label}</span>{e.remaining ? ` (${e.remaining}% left)` : ''}</td>
                  <td class="num small">{e.hours?.toLocaleString() ?? '–'}</td>
                  <td class="hide-sm small faint">{logWhen(e.hours)}</td>
                  <td class="hide-sm num small">{e.lba !== null ? e.lba.toLocaleString() : '–'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else if !detail.tests.length}
        <p class="panel-body muted small">No self-test results yet{d.source === 'api' ? ' (the Proxmox API doesn\'t show the drive\'s self-test log)' : ''}.</p>
      {/if}
    </section>

    {#if r?.attributes.length}
      <section class="panel">
        <div class="panel-head"><h2>SMART attributes</h2><span class="faint small">Normalized values count down; a value at or below its threshold means the vendor considers it failed.</span></div>
        <div class="table-wrap">
          <table class="table attrs">
            <thead>
              <tr><th class="num">ID</th><th>Attribute</th><th class="num">Value</th><th class="num hide-sm">Worst</th><th class="num hide-sm">Threshold</th><th>Raw</th><th class="hide-md">Type</th><th>Status</th></tr>
            </thead>
            <tbody>
              {#each r.attributes as a (a.id)}
                {@const tone = attributeTone(a)}
                <tr class:critical={CRITICAL_ATTRIBUTES.has(a.id)}>
                  <td class="num faint">{a.id}</td>
                  <td title={ATA_ATTRIBUTES[a.id] ?? ''}><span class:explained={!!ATA_ATTRIBUTES[a.id]}>{a.name.replaceAll('_', ' ')}</span></td>
                  <td class="num">{a.value ?? '–'}</td>
                  <td class="num hide-sm">{a.worst ?? '–'}</td>
                  <td class="num hide-sm">{a.thresh ?? '–'}</td>
                  <td class="mono small">{a.raw}</td>
                  <td class="hide-md small faint">{a.prefail ? 'Pre-fail' : 'Old age'}</td>
                  <td class="small text-{tone}">{attrStatus(tone, a.failed)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}

    {#if r && (r.protocol === 'nvme' || r.protocol === 'scsi')}
      <section class="panel">
        <div class="panel-head"><h2>{r.protocol === 'nvme' ? 'NVMe health log' : 'SAS health'}</h2></div>
        <dl class="kv panel-body">
          {#if r.protocol === 'nvme'}
            <dt>Critical warning</dt><dd class:text-bad={(r.critical_warning ?? 0) > 0}>{nvmeWarning(r.critical_warning)}</dd>
            <dt>Available spare</dt><dd>{r.available_spare !== null ? `${r.available_spare}%` : '–'}{r.spare_threshold !== null ? ` (minimum ${r.spare_threshold}%)` : ''}</dd>
            <dt>Endurance used</dt><dd>{r.wear_used !== null ? `${r.wear_used}%` : '–'}</dd>
            <dt>Media errors</dt><dd class:text-bad={(r.media_errors ?? 0) > 0}>{r.media_errors ?? '–'}</dd>
            <dt>Unsafe shutdowns</dt><dd>{r.unsafe_shutdowns ?? '–'}</dd>
            <dt>Error log entries</dt><dd>{r.error_log_entries ?? '–'}</dd>
            {#if r.temp_warning || r.temp_critical}<dt>Temperature limits</dt><dd>warning {r.temp_warning ?? '–'} °C, critical {r.temp_critical ?? '–'} °C</dd>{/if}
          {:else}
            <dt>Grown defects</dt><dd class:text-warn={(r.grown_defects ?? 0) > 0}>{r.grown_defects ?? '–'}</dd>
            <dt>Uncorrected errors</dt><dd class:text-bad={(r.uncorrected_errors ?? 0) > 0}>{r.uncorrected_errors ?? '–'}</dd>
            {#if r.temp_critical}<dt>Trip temperature</dt><dd>{r.temp_critical} °C</dd>{/if}
            {#if r.wear_used !== null}<dt>Endurance used</dt><dd>{r.wear_used}%</dd>{/if}
          {/if}
          <dt>Data read / written</dt><dd>{r.bytes_read ? diskSize(r.bytes_read) : '–'} / {r.bytes_written ? diskSize(r.bytes_written) : '–'}</dd>
        </dl>
      </section>
    {/if}

    {#if r?.text || r?.messages.length}
      <details class="panel">
        <summary class="panel-head"><h2>Raw output</h2></summary>
        <div class="panel-body">
          {#each r?.messages ?? [] as m (m)}<p class="small text-warn">{m}</p>{/each}
          {#if r?.text}<pre class="raw">{r.text}</pre>{/if}
        </div>
      </details>
    {/if}

    {#if detail.events.length}
      <section class="panel">
        <div class="panel-head"><h2>History of changes</h2></div>
        <ul class="events">
          {#each detail.events as e (e.id)}
            <li class={e.severity}>
              <span class="faint small" title={dateTime(e.ts)}>{dateTime(e.ts)}</span>
              <span class="small">{e.message}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/if}
</div>

<style>
  .detail {
    display: grid;
    gap: 16px;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 16px;
  }
  .who {
    flex: 1;
    min-width: 240px;
  }
  .who h2 {
    font-size: var(--fs-xl);
  }
  .meta {
    margin: 4px 0 0;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .bay {
    color: var(--accent-text);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .banner {
    display: flex;
    gap: 10px;
    margin: 0;
    padding: 11px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    font-size: var(--fs-sm);
  }
  .banner :global(svg) {
    flex: none;
  }
  .banner.bad {
    border-color: color-mix(in srgb, var(--bad) 40%, var(--line));
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
  }
  .banner.sleep :global(svg) {
    color: var(--paused);
  }
  .banner.idle {
    background: var(--raised);
  }
  .running {
    display: grid;
    gap: 8px;
    padding: 12px 16px;
  }
  .run-line {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 12px;
  }
  .progress {
    height: 8px;
    border-radius: 4px;
    border: 1px solid var(--line);
    background: var(--sunken);
    overflow: hidden;
  }
  .progress .fill {
    height: 100%;
    background: var(--info);
    transition: width 600ms var(--ease);
  }
  .findings,
  .notes,
  .events {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .findings li {
    --tone: var(--info);
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 12px;
    padding: 10px 14px;
    border-left: 3px solid var(--tone);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--tone) 8%, var(--panel));
  }
  .findings li.warning {
    --tone: var(--warn);
  }
  .findings li.failing {
    --tone: var(--bad);
  }
  .lvl {
    grid-row: span 2;
    color: var(--tone);
    font: 600 var(--fs-sm) / 1.4 var(--font-data);
  }
  .detail-text {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .all-good {
    margin: 0;
    padding: 10px 14px;
    border-left: 3px solid var(--ok);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--ok) 7%, var(--panel));
  }
  .notes li {
    font-size: var(--fs-sm);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
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
    font: 600 24px / 1.1 var(--font-data);
  }
  .sep {
    margin: 0 4px;
    font-weight: 400;
  }
  .sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .charts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 340px), 1fr));
    gap: 16px;
  }
  .charts h4 {
    margin: 0 0 4px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 380px), 1fr));
    gap: 16px;
    align-items: start;
  }
  .bad-chip {
    color: var(--bad);
  }
  .label-form {
    display: grid;
    gap: 12px;
  }
  .lf {
    display: grid;
    gap: 4px;
  }
  .lf > span {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .dates {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 12px;
  }
  .text-ok {
    color: var(--ok);
  }
  .text-info {
    color: var(--info);
  }
  .text-idle {
    color: var(--text-3);
  }
  .table-title {
    margin: 12px 16px 4px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .reason {
    margin: 0;
    padding-bottom: 0;
  }
  .attrs tr.critical td:nth-child(2) {
    font-weight: 600;
  }
  .explained {
    text-decoration: underline dotted var(--text-3);
    text-underline-offset: 3px;
    cursor: help;
  }
  .raw {
    max-height: 420px;
    margin: 0;
    padding: 10px 12px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 12.5px;
  }
  details > summary {
    cursor: pointer;
    list-style: none;
  }
  details > summary::-webkit-details-marker {
    display: none;
  }
  details:not([open]) > summary {
    border-bottom: 0;
  }
  .events {
    padding: 14px 16px;
  }
  .events li {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 10px;
  }
  .events li.warning span:last-child {
    color: var(--warn);
  }
  .events li.error span:last-child {
    color: var(--bad);
  }
  .events li.success span:last-child {
    color: var(--ok);
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
    .events li {
      grid-template-columns: 1fr;
      gap: 2px;
    }
  }
</style>
