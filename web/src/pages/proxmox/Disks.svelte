<!--
  Disk health and storage pools of the Proxmox nodes: every physical disk
  with its SMART verdict, temperature and history, the pools they form
  (ZFS, mdadm, SnapRAID, LVM thin) with their redundancy and scrubs, and
  SMART self-tests with their schedules. A disk id in the URL opens that
  disk's page.
-->
<script lang="ts">
  import CalendarClock from '@lucide/svelte/icons/calendar-clock';
  import FlaskConical from '@lucide/svelte/icons/flask-conical';
  import HardDrive from '@lucide/svelte/icons/hard-drive';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Lightbulb from '@lucide/svelte/icons/lightbulb';
  import Moon from '@lucide/svelte/icons/moon';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Plus from '@lucide/svelte/icons/plus';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Settings from '@lucide/svelte/icons/settings';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { untrack } from 'svelte';
  import Button from '../../components/Button.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Menu, { type MenuEntry } from '../../components/Menu.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Sparkline from '../../components/Sparkline.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage } from '../../lib/api';
  import {
    MEDIA,
    TEST_KIND,
    TEST_STATUS,
    compareDisks,
    describeSchedule,
    diskSize,
    powerOn,
    recommendedSchedules,
    roleLabel,
    tempTone,
    testLength,
    testRemaining,
    verdictOf,
  } from '../../lib/disks';
  import { ago, dateTime, duration, plural } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href, router } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { DiskHealthOverview, DiskTest, DiskView, PveOverview, TestSchedule } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';
  import DiskDetail from './disks/DiskDetail.svelte';
  import HealthBadge from './disks/HealthBadge.svelte';
  import PoolCard from './disks/PoolCard.svelte';
  import ScheduleDialog from './disks/ScheduleDialog.svelte';

  let { sub }: { overview: PveOverview; selfVmid: number | null; reload: () => Promise<void>; sub: string | null } = $props();

  let data = $state<DiskHealthOverview | null>(null);
  let error = $state('');
  let refreshing = $state(false);
  let filter = $state<'all' | 'problems' | 'hdd' | 'ssd' | 'nvme'>('all');
  let nodeFilter = $state('');
  let editing = $state<TestSchedule | null>(null);
  let creating = $state(false);

  async function load() {
    try {
      data = await api.get<DiskHealthOverview>('/proxmox/disk-health');
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  // Refetch when the server says something changed (debounced), and every
  // minute while visible so progress and "ago" stay current.
  let timer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    void live.disksVersion;
    untrack(() => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void load(), data ? 400 : 0);
    });
  });
  $effect(() => {
    const t = setInterval(() => {
      if (!document.hidden) void load();
    }, 60_000);
    return () => clearInterval(t);
  });

  const present = $derived((data?.disks ?? []).filter((d) => d.present));
  const gone = $derived((data?.disks ?? []).filter((d) => !d.present));
  const byId = $derived(new Map((data?.disks ?? []).map((d) => [d.id, d])));
  const nodes = $derived([...new Set(present.map((d) => d.node))].sort());
  const multiNode = $derived(nodes.length > 1);
  const usable = $derived(new Set((data?.host_access.nodes ?? []).filter((n) => n.ok && !n.outdated && n.helper !== null).map((n) => n.node)));
  const problemCount = $derived(present.filter((d) => verdictOf(d).rank >= 1).length);
  const shown = $derived(
    present
      .filter((d) => !nodeFilter || d.node === nodeFilter)
      .filter((d) => {
        switch (filter) {
          case 'problems':
            return verdictOf(d).rank >= 1 || !!d.error;
          case 'hdd':
            return d.media === 'hdd';
          case 'ssd':
            return d.media === 'ssd';
          case 'nvme':
            return d.media === 'nvme';
          default:
            return true;
        }
      })
      .sort(compareDisks),
  );
  const counts = $derived.by(() => {
    const c = { ok: 0, watch: 0, warning: 0, failing: 0, unknown: 0 };
    for (const d of present) {
      const v = verdictOf(d);
      if (v.rank < 0) c.unknown++;
      else if (d.verdict === 'watch') c.watch++;
      else if (d.verdict === 'warning') c.warning++;
      else if (d.verdict === 'failing') c.failing++;
      else c.ok++;
    }
    return c;
  });
  const hottest = $derived(
    present.filter((d) => d.temperature !== null && !d.asleep && !d.resting).sort((a, b) => (b.temperature ?? 0) - (a.temperature ?? 0))[0] ?? null,
  );
  const avgTemp = $derived.by(() => {
    const t = present.filter((d) => d.temperature !== null && !d.asleep && !d.resting).map((d) => d.temperature as number);
    return t.length ? t.reduce((a, b) => a + b, 0) / t.length : null;
  });
  const capacity = $derived(present.reduce((s, d) => s + (d.size ?? 0), 0));
  const pools = $derived(data?.pools ?? []);
  const poolProblems = $derived(pools.filter((p) => p.health !== 'ok').length);
  const active = $derived(data?.tests.active ?? []);
  const nextRun = $derived(
    (data?.schedules ?? [])
      .filter((s) => s.enabled && s.next_run)
      .sort((a, b) => (a.next_run ?? 0) - (b.next_run ?? 0))[0] ?? null,
  );
  const hostOff = $derived(!!data && !data.host_access.enabled);
  const brokenNodes = $derived(
    (data?.host_access.nodes ?? []).filter((n) => !n.ok || n.outdated || !n.smartctl).filter((n) => n.error),
  );

  async function refresh() {
    refreshing = true;
    try {
      const r = await api.post<{ started: boolean }>('/proxmox/disk-health/refresh');
      ui.toast(r.started ? 'Reading the disks now. This can take a minute.' : 'A read is already running.', 'info');
    } catch (err) {
      ui.error(err);
    } finally {
      refreshing = false;
    }
  }

  async function startTest(d: DiskView, kind: 'short' | 'long' | 'conveyance') {
    const minutes = kind === 'long' ? d.tests.long_minutes : kind === 'conveyance' ? d.tests.conveyance_minutes : d.tests.short_minutes;
    if (kind === 'long') {
      const ok = await ui.confirm({
        title: `Run a long self-test on ${d.devpath}?`,
        message: `The disk reads its whole surface${minutes ? `, which takes about ${testLength(minutes)}` : ''}. It stays usable meanwhile, a bit slower. If it spins down before the test is done, the test stops early. You can abort the test at any time.`,
        confirmLabel: 'Start long test',
      });
      if (!ok) return;
    }
    try {
      await api.post(`/proxmox/disk-health/disks/${encodeURIComponent(d.id)}/tests`, { kind });
      ui.toast(`${TEST_KIND[kind]} self-test queued on ${d.devpath}`, 'success');
      void load();
    } catch (err) {
      ui.error(err, "Couldn't start the test");
    }
  }

  async function identify(d: DiskView) {
    try {
      await api.post(`/proxmox/disk-health/disks/${encodeURIComponent(d.id)}/identify`, { seconds: 30 });
      ui.toast(`Reading from ${d.devpath} for 30 seconds: look for the blinking activity light.`, 'success');
    } catch (err) {
      ui.error(err, "Couldn't blink the disk");
    }
  }

  async function stopTest(t: DiskTest) {
    const d = byId.get(t.disk);
    const ok = await ui.confirm({
      title: t.status === 'queued' ? 'Cancel the waiting test?' : `Abort the ${TEST_KIND[t.kind]?.toLowerCase()} test on ${d?.devpath ?? t.devpath}?`,
      message: t.status === 'queued' ? undefined : 'The drive stops testing right away and logs the test as aborted.',
      confirmLabel: t.status === 'queued' ? 'Cancel test' : 'Abort test',
      danger: t.status !== 'queued',
    });
    if (!ok) return;
    try {
      await api.del(`/proxmox/disk-health/tests/${encodeURIComponent(t.id)}`);
      void load();
    } catch (err) {
      ui.error(err);
    }
  }

  function diskMenu(d: DiskView): MenuEntry[] {
    const items: MenuEntry[] = [{ label: 'Details and history', run: () => router.navigate(`/proxmox/disks/${encodeURIComponent(d.id)}`) }];
    if (!session.isAdmin) return items;
    const why = d.tests.reason ?? undefined;
    const busy = active.some((t) => t.disk === d.id);
    items.push('separator');
    for (const kind of ['short', 'long', 'conveyance'] as const) {
      if (kind === 'conveyance' && !d.tests.conveyance) continue;
      const minutes = kind === 'long' ? d.tests.long_minutes : kind === 'conveyance' ? d.tests.conveyance_minutes : d.tests.short_minutes;
      items.push({
        label: `${TEST_KIND[kind]} self-test${minutes ? ` (${testLength(minutes)})` : ''}`,
        icon: FlaskConical,
        disabled: !d.tests.available || busy || (kind === 'long' ? !d.tests.long : !d.tests.short),
        hint: busy ? 'A test is already running or waiting' : why,
        run: () => startTest(d, kind),
      });
    }
    items.push({ label: 'Blink activity light', icon: Lightbulb, disabled: !usable.has(d.node), hint: usable.has(d.node) ? 'Reads from the disk for 30 seconds' : 'Needs host access', run: () => identify(d) });
    return items;
  }

  async function saveSchedules(list: TestSchedule[]) {
    try {
      const clean = list.map(({ next_run: _n, covers: _c, ...s }) => s);
      const r = await api.put<{ schedules: TestSchedule[] }>('/proxmox/disk-health/schedules', clean);
      if (data) data.schedules = r.schedules;
      return true;
    } catch (err) {
      ui.error(err, "Couldn't save the schedules");
      return false;
    }
  }

  async function toggleSchedule(s: TestSchedule, enabled: boolean) {
    await saveSchedules((data?.schedules ?? []).map((x) => (x.id === s.id ? { ...x, enabled } : x)));
  }

  async function deleteSchedule(s: TestSchedule) {
    const ok = await ui.confirm({ title: `Delete "${s.name}"?`, message: 'Tests it already queued keep running.', confirmLabel: 'Delete', danger: true });
    if (ok) await saveSchedules((data?.schedules ?? []).filter((x) => x.id !== s.id));
  }

  async function addRecommended() {
    const existing = data?.schedules ?? [];
    const add = recommendedSchedules().filter((r) => !existing.some((e) => e.id === r.id));
    if (await saveSchedules([...existing, ...add])) ui.toast('Added a weekly short test and a monthly long test.', 'success');
  }

  async function onScheduleSaved(s: TestSchedule) {
    const existing = data?.schedules ?? [];
    const next = existing.some((x) => x.id === s.id) ? existing.map((x) => (x.id === s.id ? s : x)) : [...existing, s];
    if (await saveSchedules(next)) {
      editing = null;
      creating = false;
    }
  }

  async function forget(d: DiskView) {
    const ok = await ui.confirm({
      title: `Forget ${d.model ?? d.devpath}?`,
      message: 'Its SMART history, test results and events are deleted. If the disk comes back, it starts a new history.',
      confirmLabel: 'Forget disk',
      danger: true,
    });
    if (!ok) return;
    try {
      await api.del(`/proxmox/disk-health/disks/${encodeURIComponent(d.id)}`);
      void load();
    } catch (err) {
      ui.error(err);
    }
  }

  function testTone(t: DiskTest) {
    return TEST_STATUS[t.status] ?? TEST_STATUS.unknown;
  }

  function eventLink(subject: string): string | null {
    if (subject.startsWith('pool:')) return null;
    return href(`/proxmox/disks/${encodeURIComponent(subject)}`);
  }

  function subjectName(subject: string): string {
    if (subject.startsWith('pool:')) {
      const p = pools.find((x) => `pool:${x.id}` === subject);
      return p ? p.name : subject.split('/').pop() ?? subject;
    }
    const d = byId.get(subject);
    return d ? `${d.devpath.replace('/dev/', '')}${d.label.bay ? ` (${d.label.bay})` : ''}` : subject;
  }
</script>

{#if sub}
  <DiskDetail id={sub} />
{:else if !data}
  {#if error}<p class="text-bad">{error}</p>{:else}<p class="faint"><Spinner size={13} /> Loading disk health…</p>{/if}
{:else if !data.enabled}
  <EmptyState icon={HardDrive} title="Disk health is off">
    Turn it on to follow every disk's SMART data, temperature and history, see your pools' redundancy and scrubs, and run self-tests.
    {#snippet actions()}
      {#if session.isAdmin}<a class="btn primary" href={href('/settings/proxmox')}>Open Proxmox settings</a>{/if}
    {/snippet}
  </EmptyState>
{:else}
  <div class="disks">
    <div class="toolbar">
      <span class="faint small">
        {#if data.sampling.running}<Spinner size={12} /> Reading the disks…
        {:else if data.sampling.last_smart}SMART read {ago(data.sampling.last_smart)}{#if data.sampling.next_smart}, next {ago(data.sampling.next_smart)}{/if}
        {:else}Waiting for the first read…{/if}
      </span>
      <span class="grow"></span>
      {#if session.isAdmin}
        <a class="btn ghost small" href={href('/settings/proxmox')}><Settings size={15} /> Settings</a>
        <Button small icon={RefreshCw} busy={refreshing || data.sampling.running} onclick={refresh}>Read now</Button>
      {/if}
    </div>

    {#if error}<p class="banner bad"><TriangleAlert size={17} /><span>Couldn't refresh: {error}</span></p>{/if}
    {#if data.sampling.error}<p class="banner bad"><TriangleAlert size={17} /><span>The last read failed: {data.sampling.error}</span></p>{/if}
    {#each Object.entries(data.sampling.node_errors) as [node, err] (node)}
      <p class="banner bad"><TriangleAlert size={17} /><span>Couldn't read {node}: {err}. Its disks show their last known state.</span></p>
    {/each}
    {#if hostOff}
      <div class="banner info">
        <KeyRound size={17} />
        <span>
          <strong>Self-tests need host access.</strong> Proxmox can read SMART data but can't start self-tests, and every read through it wakes sleeping
          disks. With host access, Nodestarr runs a small, locked-down helper on each node over SSH: self-tests and schedules, mdadm and SnapRAID arrays,
          and spun-down disks stay asleep.
          {#if session.isAdmin}<a href={href('/settings/proxmox')}>Set up host access</a>{/if}
        </span>
      </div>
    {:else if brokenNodes.length}
      {#each brokenNodes as n (n.node)}
        <p class="banner warn"><TriangleAlert size={17} /><span>Host access to {n.node}: {n.error} {#if session.isAdmin}<a href={href('/settings/proxmox')}>Check it</a>{/if}</span></p>
      {/each}
    {/if}

    <div class="tiles">
      <section class="panel tile">
        <h3>Disks</h3>
        <p class="big num">{present.length}</p>
        <p class="sub">
          {#if counts.failing}<span class="text-bad">{counts.failing} failing</span> · {/if}
          {#if counts.warning}<span class="text-warn">{counts.warning} need attention</span> · {/if}
          {#if counts.watch}<span class="watch">{counts.watch} to watch</span> · {/if}
          {counts.ok} healthy{#if counts.unknown} · {counts.unknown} not read{/if}
        </p>
      </section>
      <section class="panel tile">
        <h3>Pools</h3>
        <p class="big num">{pools.length}</p>
        <p class="sub">{#if !pools.length}none found{:else if poolProblems}<span class="text-warn">{plural(poolProblems, 'pool')} not healthy</span>{:else}all healthy{/if}</p>
      </section>
      <section class="panel tile">
        <h3>Temperature</h3>
        {#if hottest}
          <p class="big num"><span class="text-{tempTone(hottest.temperature, hottest.temp_limit) || 'ok'}">{hottest.temperature} °C</span></p>
          <p class="sub">hottest: {hottest.devpath.replace('/dev/', '')}{multiNode ? ` on ${hottest.node}` : ''}{avgTemp !== null ? ` · average ${avgTemp.toFixed(0)} °C` : ''}</p>
        {:else}
          <p class="big num faint">–</p>
          <p class="sub">no readings yet</p>
        {/if}
      </section>
      <section class="panel tile">
        <h3>Self-tests</h3>
        <p class="big num">{active.filter((t) => t.status === 'running').length}</p>
        <p class="sub">
          running{#if active.some((t) => t.status === 'queued')}, {active.filter((t) => t.status === 'queued').length} waiting{/if}
          {#if nextRun?.next_run}· next “{nextRun.name}” {ago(nextRun.next_run)}{:else if !data.schedules.length}· no schedules{/if}
        </p>
      </section>
      <section class="panel tile">
        <h3>Raw capacity</h3>
        <p class="big num">{diskSize(capacity)}</p>
        <p class="sub">in {plural(present.length, 'disk')}{multiNode ? ` on ${plural(nodes.length, 'node')}` : ''}</p>
      </section>
    </div>

    {#if pools.length}
      <h2 class="section">Pools</h2>
      <div class="pools">
        {#each pools as p (p.id)}
          <PoolCard pool={p} disks={byId} canControl={usable.has(p.node)} {multiNode} onchanged={load} />
        {/each}
      </div>
    {/if}

    <div class="bar">
      <h2 class="section">Disks</h2>
      <span class="grow"></span>
      {#if multiNode}
        <select class="input node-filter" bind:value={nodeFilter} aria-label="Node">
          <option value="">All nodes</option>
          {#each nodes as n (n)}<option value={n}>{n}</option>{/each}
        </select>
      {/if}
      <Segmented
        label="Show"
        bind:value={filter}
        options={[
          { value: 'all', label: 'All' },
          { value: 'problems', label: problemCount ? `Problems (${problemCount})` : 'Problems' },
          { value: 'hdd', label: 'HDD' },
          { value: 'ssd', label: 'SSD' },
          { value: 'nvme', label: 'NVMe' },
        ]}
      />
    </div>
    <section class="panel">
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th>Disk</th>
              <th class="hide-sm">Health</th>
              <th class="hide-md">Pool</th>
              <th>Temp</th>
              <th class="hide-md">Age</th>
              <th class="num hide-sm" title="Reallocated / pending / uncorrectable sectors">Sectors</th>
              <th class="hide-lg">Wear</th>
              <th class="hide-md">Last test</th>
              <th class="shrink"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each shown as d (d.id)}
              {@const v = verdictOf(d)}
              {@const running = active.find((t) => t.disk === d.id)}
              {@const temps = d.temps.filter((t): t is number => t !== null)}
              <tr class="clickable" onclick={(e) => { if (!(e.target as HTMLElement).closest('button, a, [role=menu]')) router.navigate(`/proxmox/disks/${encodeURIComponent(d.id)}`); }}>
                <td>
                  <div class="disk-cell">
                    <span class="dev">
                      <a href={href(`/proxmox/disks/${encodeURIComponent(d.id)}`)} class="mono">{d.devpath.replace('/dev/', '')}</a>
                      <span class="chip">{MEDIA[d.media]}</span>
                      {#if d.label.bay}<span class="chip bay-chip">{d.label.bay}</span>{/if}
                      {#if multiNode}<span class="faint small">{d.node}</span>{/if}
                    </span>
                    <span class="model small ellipsis" title={`${d.model ?? ''} ${d.serial ?? ''}`}>{d.model ?? 'Unknown model'} <span class="faint">· {diskSize(d.size)}{d.serial ? ` · ${d.serial}` : ''}</span></span>
                    <span class="show-sm"><HealthBadge tone={v.tone} label={v.label} /></span>
                  </div>
                </td>
                <td class="hide-sm">
                  <HealthBadge tone={v.tone} label={v.label} title={d.findings.filter((f) => f.level !== 'info').map((f) => f.title).join('\n')} />
                  {#if d.findings.find((f) => f.level !== 'info' && f.level !== 'ok')}
                    <div class="faint small ellipsis why">{d.findings.find((f) => f.level !== 'info' && f.level !== 'ok')?.title}</div>
                  {:else if d.error}
                    <div class="faint small ellipsis why" title={d.error}>{d.error}</div>
                  {/if}
                </td>
                <td class="hide-md">
                  {#if d.roles.length}
                    {#each d.roles as r (r.pool + r.group + r.member)}<span class="chip role" class:bad={!r.ok}>{roleLabel(r)}</span>{/each}
                  {:else if d.used_by}<span class="faint small">{d.used_by}</span>{:else}<span class="faint small">–</span>{/if}
                </td>
                <td>
                  <div class="temp">
                    {#if d.temperature !== null}
                      <span class="num text-{tempTone(d.temperature, d.temp_limit) || 'ok'}" class:dim={d.asleep || d.resting}>{d.temperature}°</span>
                    {:else}<span class="faint">–</span>{/if}
                    {#if d.asleep}<span class="asleep" title="Asleep at the last check (spun down); the reading is older"><Moon size={13} /></span>
                    {:else if d.resting}<span class="asleep" title="Spins down between readings, so it's read about once a day; the reading is older"><Moon size={13} /></span>{/if}
                    {#if temps.length > 1}<span class="hide-sm"><Sparkline values={temps} width={56} height={18} color="var(--text-3)" label="Temperature of the last day" /></span>{/if}
                  </div>
                </td>
                <td class="hide-md num small nowrap">{powerOn(d.power_on_hours)}</td>
                <td class="num hide-sm small">
                  {#if d.reallocated === null && d.pending === null && d.uncorrectable === null}<span class="faint">–</span>
                  {:else}
                    <span class:text-warn={(d.reallocated ?? 0) > 0}>{d.reallocated ?? '–'}</span><span class="faint"> / </span><span class:text-bad={(d.pending ?? 0) > 0}>{d.pending ?? '–'}</span><span class="faint"> / </span><span class:text-bad={(d.uncorrectable ?? 0) > 0}>{d.uncorrectable ?? '–'}</span>
                  {/if}
                </td>
                <td class="hide-lg small num nowrap">
                  {#if d.wear_used !== null && d.media !== 'hdd'}<span class:text-warn={d.wear_used >= 75} class:text-bad={d.wear_used >= 90}>{d.wear_used.toFixed(0)}% used</span>
                  {:else if d.media === 'hdd' && (d.load_cycles ?? 0) >= 100_000}<span class:text-warn={(d.load_cycles ?? 0) >= 300_000} title="Head load/unload cycles; many disks are rated for 300,000 to 600,000">{Math.round((d.load_cycles ?? 0) / 1000)}k parks</span>
                  {:else}<span class="faint">–</span>{/if}
                </td>
                <td class="hide-md small">
                  {#if running}
                    <span class="text-info">{TEST_KIND[running.kind]} {running.status === 'running' ? `${Math.round((running.progress ?? 0) * 100)}%` : 'waiting'}</span>
                  {:else if d.last_test}
                    {@const st = TEST_STATUS[d.last_test.outcome] ?? TEST_STATUS.unknown}
                    <span class="text-{st.tone}">{st.label}</span>
                    <span class="faint">{TEST_KIND[d.last_test.kind]?.toLowerCase() ?? d.last_test.kind}{d.last_test.at ? `, ${d.last_test.estimated ? '≈' : ''}${ago(d.last_test.at)}` : ''}</span>
                  {:else}<span class="faint">never</span>{/if}
                </td>
                <td class="shrink"><Menu items={diskMenu(d)} small label="Actions for {d.devpath}" /></td>
              </tr>
            {:else}
              <tr><td colspan="9" class="faint">{present.length ? 'No disks match.' : 'No disks found yet.'}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <div class="cols">
      <section class="panel">
        <div class="panel-head"><h2>Self-tests</h2></div>
        <div class="panel-body tests">
          {#if active.length}
            <ul class="runs">
              {#each active as t (t.id)}
                {@const d = byId.get(t.disk)}
                {@const left = testRemaining(t)}
                <li>
                  <div class="run-head">
                    <span><strong>{TEST_KIND[t.kind]}</strong> on <a href={href(`/proxmox/disks/${encodeURIComponent(t.disk)}`)}>{d ? d.devpath.replace('/dev/', '') : t.devpath}</a>{multiNode ? ` (${t.node})` : ''}</span>
                    <span class="faint small">{t.status === 'running' ? (left !== null ? `about ${duration(left)} left` : 'running') : t.message || 'waiting'}</span>
                    {#if session.isAdmin}<Button small variant="ghost" onclick={() => stopTest(t)}>{t.status === 'queued' ? 'Cancel' : 'Abort'}</Button>{/if}
                  </div>
                  {#if t.status === 'running'}
                    <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((t.progress ?? 0) * 100)} aria-label="Test progress">
                      <div class="fill" style:width="{Math.max(2, (t.progress ?? 0) * 100)}%"></div>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="muted small">No self-test is running. Run one from a disk's menu, or let a schedule start them.</p>
          {/if}
          {#if data.tests.recent.length}
            <h3 class="sub-h">Recent results</h3>
            <ul class="recent">
              {#each data.tests.recent.slice(0, 8) as t (t.id)}
                {@const st = testTone(t)}
                <li>
                  <HealthBadge tone={st.tone} label={st.label} />
                  <span class="grow ellipsis small">
                    {TEST_KIND[t.kind]} on <a href={href(`/proxmox/disks/${encodeURIComponent(t.disk)}`)}>{subjectName(t.disk)}</a>
                    {#if t.result && t.status !== 'passed'}<span class="faint">· {t.result}</span>{:else if t.message}<span class="faint">· {t.message}</span>{/if}
                  </span>
                  <span class="faint small" title={dateTime(t.finished_at)}>{ago(t.finished_at)}</span>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <h2>Schedules</h2>
          {#if session.isAdmin}<Button small variant="ghost" icon={Plus} onclick={() => (creating = true)}>Add</Button>{/if}
        </div>
        <div class="panel-body">
          {#if data.schedules.length}
            <ul class="schedules">
              {#each data.schedules as s (s.id)}
                <li class:off={!s.enabled}>
                  <div class="grow">
                    <div class="row"><CalendarClock size={15} /><strong>{s.name}</strong></div>
                    <div class="small muted">{describeSchedule(s)}</div>
                    <div class="small faint">
                      {plural(s.covers ?? 0, 'disk')}{#if s.enabled && s.next_run} · next <span title={dateTime(s.next_run)}>{ago(s.next_run)}</span>{/if}
                    </div>
                  </div>
                  {#if session.isAdmin}
                    <Toggle checked={s.enabled} label="Enabled" onchange={(v) => toggleSchedule(s, v)} />
                    <Button small variant="ghost" icon={Pencil} label="Edit {s.name}" onclick={() => (editing = s)} />
                    <Button small variant="ghost" icon={Trash2} label="Delete {s.name}" onclick={() => deleteSchedule(s)} />
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="muted small">No schedules yet. Regular short tests catch failing electronics; monthly long tests read every sector and find bad ones before your data does.</p>
            {#if session.isAdmin}<Button small icon={CalendarClock} onclick={addRecommended}>Add a weekly short and a monthly long test</Button>{/if}
          {/if}
          {#if hostOff && data.schedules.length}<p class="faint small">Schedules run once host access is set up.</p>{/if}
        </div>
      </section>
    </div>

    {#if gone.length}
      <details class="panel">
        <summary class="panel-head"><h2>No longer connected ({gone.length})</h2></summary>
        <div class="table-wrap">
          <table class="table">
            <thead><tr><th>Disk</th><th>Last seen</th><th class="hide-sm">Power-on time</th><th class="shrink"></th></tr></thead>
            <tbody>
              {#each gone as d (d.id)}
                <tr>
                  <td><a href={href(`/proxmox/disks/${encodeURIComponent(d.id)}`)}>{d.model ?? d.id}</a> <span class="faint small">{d.serial ?? ''} · was {d.devpath} on {d.node}</span></td>
                  <td class="small"><TimeAgo ts={d.last_seen} /></td>
                  <td class="hide-sm small num">{powerOn(d.power_on_hours)}</td>
                  <td class="shrink">{#if session.isAdmin}<Button small variant="ghost" icon={Trash2} label="Forget {d.model ?? d.id}" onclick={() => forget(d)} />{/if}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </details>
    {/if}

    {#if data.events.length}
      <details class="panel">
        <summary class="panel-head"><h2>Recent changes</h2></summary>
        <ul class="events">
          {#each data.events.slice(0, 40) as e (e.id)}
            {@const link = eventLink(e.subject)}
            <li class={e.severity}>
              <span class="faint small when" title={dateTime(e.ts)}>{ago(e.ts)}</span>
              <span class="small who">{#if link}<a href={link}>{subjectName(e.subject)}</a>{:else}{subjectName(e.subject)}{/if}</span>
              <span class="small what">{e.message}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>

  <ScheduleDialog
    open={creating || !!editing}
    schedule={editing}
    disks={present}
    {nodes}
    onclose={() => {
      editing = null;
      creating = false;
    }}
    onsave={onScheduleSaved}
  />
{/if}

<style>
  .disks {
    display: grid;
    gap: 16px;
  }
  .toolbar,
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .section {
    margin: 4px 0 -4px;
    font-size: var(--fs-md);
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
    margin-top: 1px;
  }
  .banner.bad {
    border-color: color-mix(in srgb, var(--bad) 40%, var(--line));
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
  }
  .banner.bad :global(svg) {
    color: var(--bad);
  }
  .banner.warn {
    border-color: color-mix(in srgb, var(--warn) 40%, var(--line));
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
  }
  .banner.warn :global(svg) {
    color: var(--warn);
  }
  .banner.info {
    border-color: color-mix(in srgb, var(--info) 35%, var(--line));
    background: color-mix(in srgb, var(--info) 7%, var(--panel));
  }
  .banner.info :global(svg) {
    color: var(--info);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
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
  .sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .watch,
  .text-info {
    color: var(--info);
  }
  .text-idle {
    color: var(--text-3);
  }
  .pools {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 430px), 1fr));
    gap: 16px;
    align-items: start;
  }
  .node-filter {
    width: auto;
    height: 32px;
  }
  .disk-cell {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .dev {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .dev a {
    color: var(--text);
    font-weight: 600;
  }
  .bay-chip {
    color: var(--accent-text);
  }
  .model {
    max-width: 340px;
  }
  .why {
    max-width: 220px;
  }
  .role {
    margin: 1px 4px 1px 0;
  }
  .role.bad {
    color: var(--bad);
  }
  .nowrap {
    white-space: nowrap;
  }
  .temp {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .dim {
    opacity: 0.65;
  }
  .asleep {
    display: inline-flex;
    color: var(--paused);
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 420px), 1fr));
    gap: 16px;
    align-items: start;
  }
  .tests {
    display: grid;
    gap: 12px;
  }
  .runs,
  .recent,
  .schedules,
  .events {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .runs li {
    display: grid;
    gap: 6px;
  }
  .run-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 4px 10px;
  }
  .progress {
    height: 6px;
    border-radius: 3px;
    border: 1px solid var(--line);
    background: var(--sunken);
    overflow: hidden;
  }
  .progress .fill {
    height: 100%;
    background: var(--info);
    transition: width 600ms var(--ease);
  }
  .sub-h {
    margin: 4px 0 0;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .recent li {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .schedules li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }
  .schedules li:last-child {
    border-bottom: 0;
  }
  .schedules li.off {
    opacity: 0.6;
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
    grid-template-columns: 90px minmax(80px, 160px) 1fr;
    gap: 10px;
    align-items: baseline;
  }
  .events li.warning .what {
    color: var(--warn);
  }
  .events li.error .what {
    color: var(--bad);
  }
  .show-sm {
    display: none;
  }
  @media (max-width: 1200px) {
    .hide-lg {
      display: none;
    }
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
    .show-sm {
      display: inline;
    }
    .events li {
      grid-template-columns: 1fr;
      gap: 2px;
    }
  }
</style>
