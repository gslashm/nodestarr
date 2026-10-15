<!--
  The overview: every container as a tile, grouped by stack, what needs attention,
  host resources and recent activity.
-->
<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
  import Globe from '@lucide/svelte/icons/globe';
  import HardDrive from '@lucide/svelte/icons/hard-drive';
  import Plus from '@lucide/svelte/icons/plus';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Ship from '@lucide/svelte/icons/ship';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { onMount } from 'svelte';
  import AppIcon from '../components/AppIcon.svelte';
  import Button from '../components/Button.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import Meter from '../components/Meter.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import Sparkline from '../components/Sparkline.svelte';
  import TimeAgo from '../components/TimeAgo.svelte';
  import ContainerGrid from '../components/ContainerGrid.svelte';
  import { confirmUpdate, runAction } from '../lib/actions';
  import { api } from '../lib/api';
  import { bytes, duration, hasUpdate, isUp, STATE_LABEL, stateOf, updateLabel } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { ActivityEntry, ContainerView, DiskUsage, DockerInfo, HostStats, Series } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();

  let disks = $state<DiskUsage[]>([]);
  let docker = $state<DockerInfo | null>(null);
  let initialHost = $state<HostStats | null>(null);
  let recent = $state<ActivityEntry[]>([]);
  let cpuHistory = $state<number[]>([]);

  const host = $derived(live.host ?? initialHost);
  const all = $derived(live.list);
  const running = $derived(all.filter((c) => isUp(stateOf(c))).length);
  const updates = $derived(all.filter((c) => hasUpdate(c.update) && c.policy !== 'ignore'));
  const activity = $derived([...live.activity, ...recent.filter((r) => !live.activity.some((a) => a.id === r.id))].slice(0, 10));

  interface Attention {
    key: string;
    c?: ContainerView;
    tone: 'bad' | 'warn' | 'update';
    title: string;
    detail: string;
    fix?: { label: string; run: () => unknown };
    link?: string;
  }

  const attention = $derived.by(() => {
    const out: Attention[] = [];
    for (const c of all) {
      const st = stateOf(c);
      const path = `/containers/${encodeURIComponent(c.name)}`;
      if (st === 'crashed' || st === 'dead') {
        out.push({
          key: `crash:${c.id}`,
          c,
          tone: 'bad',
          title: `${c.display_name} ${c.oom_killed ? 'ran out of memory' : `exited with code ${c.exit_code}`}`,
          detail: c.finished_at ? `Stopped ${duration(Date.now() / 1000 - c.finished_at)} ago` : STATE_LABEL[st],
          fix: session.isAdmin ? { label: 'Start', run: () => runAction(c, 'start') } : undefined,
          link: `${path}/logs`,
        });
      } else if (st === 'unhealthy' || st === 'restarting') {
        out.push({
          key: `health:${c.id}`,
          c,
          tone: 'bad',
          title: st === 'unhealthy' ? `${c.display_name} is unhealthy` : `${c.display_name} keeps restarting`,
          detail: `Restarted ${c.restart_count} times`,
          fix: session.isAdmin ? { label: 'Restart', run: () => runAction(c, 'restart') } : undefined,
          link: `${path}/logs`,
        });
      } else if (c.uptime?.state === 'down') {
        out.push({ key: `web:${c.id}`, c, tone: 'bad', title: `${c.display_name}'s web UI isn't answering`, detail: 'The container runs, but its page fails to load', link: path });
      }
      const issues = c.app_status?.health.filter((h) => h.level !== 'notice') ?? [];
      if (issues.length) {
        out.push({ key: `app:${c.id}`, c, tone: issues.some((h) => h.level === 'error') ? 'bad' : 'warn', title: `${c.display_name} reports ${issues.length === 1 ? 'a problem' : `${issues.length} problems`}`, detail: issues[0].message, link: path });
      }
      if (c.prefs.held) {
        out.push({ key: `held:${c.id}`, c, tone: 'warn', title: `Automatic updates paused for ${c.display_name}`, detail: 'A recent update was rolled back', link: `${path}/updates` });
      }
    }
    for (const a of live.alerts) {
      if (out.some((o) => o.c?.identity === a.target)) continue;
      out.push({ key: `alert:${a.id}`, tone: a.severity === 'error' ? 'bad' : 'warn', title: a.target_name ?? a.target, detail: a.message, link: '/monitoring/alerts' });
    }
    for (const c of updates.slice(0, 6)) {
      out.push({
        key: `upd:${c.id}`,
        c,
        tone: 'update',
        title: `${c.display_name} can be updated`,
        detail: `${c.version ?? ''} ${c.version ? 'to ' : ''}${updateLabel(c.update) ?? ''}`.trim(),
        fix: session.isAdmin ? { label: 'Update', run: () => confirmUpdate(c) } : undefined,
        link: `/containers/${encodeURIComponent(c.name)}/updates`,
      });
    }
    return out;
  });

  const memUsed = $derived(host ? host.mem_total - host.mem_available : 0);
  const cpuTrend = $derived([...cpuHistory, ...live.hostTrail.map((h) => h.cpu)].slice(-90));

  onMount(() => {
    api
      .get<{ host: HostStats | null; disks: DiskUsage[]; docker: DockerInfo }>('/monitoring/overview')
      .then((r) => {
        initialHost = r.host;
        disks = r.disks;
        docker = r.docker;
      })
      .catch(() => {});
    api
      .get<{ entries: ActivityEntry[] }>('/activity?limit=10')
      .then((r) => (recent = r.entries))
      .catch(() => {});
    api
      .get<{ series: Series }>('/monitoring/history?target=host&range=1h&points=60')
      .then((r) => (cpuHistory = r.series.cpu))
      .catch(() => {});
  });

  async function checkAll() {
    try {
      const r = await api.post<{ started: boolean; message?: string }>('/updates/check');
      ui.toast(r.started ? 'Checking every container for updates…' : (r.message ?? 'A check is already running'));
    } catch (err) {
      ui.error(err);
    }
  }

  const severityTone: Record<string, string> = { error: 'bad', warning: 'warn', success: 'ok', info: '' };
</script>

<PageHeader title="Dashboard" subtitle={session.settings?.general.instance_name !== 'Nodestarr' ? session.settings?.general.instance_name : ''}>
  {#snippet actions()}
    {#if session.isAdmin}
      <Button variant="ghost" icon={CircleArrowUp} onclick={checkAll} busy={live.checkingUpdates}>Check for updates</Button>
      <a class="btn ghost" href={href('/containers/new')}><Plus size={17} />Add container</a>
    {/if}
  {/snippet}
</PageHeader>

<div class="content">
  <div class="summary">
    <a href={href('/containers?state=running')} class="stat">
      <span class="dot ok"></span>
      <span class="num">{running}</span> running
    </a>
    <a href={href('/containers?state=stopped')} class="stat">
      <span class="dot idle"></span>
      <span class="num">{all.length - running}</span> stopped
    </a>
    <a href={href('/updates')} class="stat" class:hot={updates.length > 0}>
      <ArrowUp size={15} class={updates.length ? 'text-update' : 'faint'} />
      <span class="num">{updates.length}</span>
      {updates.length === 1 ? 'update' : 'updates'}
    </a>
    <a href={href('/monitoring/alerts')} class="stat" class:hot-bad={attention.some((a) => a.tone === 'bad')}>
      <TriangleAlert size={15} class={attention.some((a) => a.tone === 'bad') ? 'text-bad' : 'faint'} />
      <span class="num">{attention.filter((a) => a.tone !== 'update').length}</span> need attention
    </a>
  </div>

  <section class="panel grid-panel" aria-label="Containers by stack">
    {#if !live.loaded}
      <div class="placeholder">Loading containers…</div>
    {:else if all.length === 0}
      <EmptyState icon={Ship} title="No containers yet">
        Nodestarr shows every container on this Docker host, including the ones in your compose stacks.
        {#snippet actions()}
          {#if session.isAdmin}
            <a class="btn primary" href={href('/containers/new')}>Add a container</a>
            <a class="btn" href={href('/stacks/new')}>Create a stack</a>
          {/if}
        {/snippet}
      </EmptyState>
    {:else}
      <ContainerGrid containers={all} />
    {/if}
  </section>

  <div class="columns">
    <section class="panel">
      <div class="panel-head"><h2>Needs attention</h2></div>
      {#if attention.length === 0}
        <p class="calm">All quiet. Every container is running as expected and nothing is waiting for an update.</p>
      {:else}
        <ul class="attention">
          {#each attention.slice(0, 12) as a (a.key)}
            <li class="tone-{a.tone}">
              {#if a.c}
                <AppIcon name={a.c.display_name} slug={a.c.app} icon={a.c.prefs.icon} size={28} />
              {:else}
                <span class="ico"><CircleAlert size={18} /></span>
              {/if}
              <a class="text" href={href(a.link ?? '/')}>
                <span class="title">{a.title}</span>
                <span class="detail ellipsis">{a.detail}</span>
              </a>
              {#if a.tone === 'bad' && a.c?.uptime?.state === 'down'}<Globe size={16} class="text-bad" />{/if}
              {#if a.fix}
                <Button small variant={a.tone === 'update' ? '' : 'ghost'} icon={a.tone === 'update' ? ArrowUp : RotateCw} onclick={a.fix.run}>{a.fix.label}</Button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if attention.length > 12}<a class="more" href={href('/containers?state=attention')}>{attention.length - 12} more</a>{/if}
      {/if}
    </section>

    <div class="side">
      <section class="panel">
        <div class="panel-head">
          <h2>Host</h2>
          {#if docker}<span class="faint num">Docker {docker.version}</span>{/if}
        </div>
        <div class="panel-body host">
          {#if host}
            <div class="metric">
              <div class="label"><span>CPU</span><span class="num">{host.cpu.toFixed(0)}% of {host.cpus} cores</span></div>
              <Sparkline values={cpuTrend} width={300} height={34} max={100} color="var(--info)" label="CPU over the last hour" stretch />
            </div>
            <div class="metric">
              <div class="label"><span>Memory</span><span class="num">{bytes(memUsed)} of {bytes(host.mem_total)}</span></div>
              <Meter value={memUsed} max={host.mem_total} warn={85} bad={94} label="Memory used" />
            </div>
            <div class="metric small">
              <span class="muted">Load {host.load.map((l) => l.toFixed(2)).join(', ')}</span>
              <span class="muted">Up {duration(host.uptime)}</span>
            </div>
          {:else}
            <p class="muted">Collecting…</p>
          {/if}
          {#each disks as d (d.path)}
            <div class="metric">
              <div class="label">
                <span class="ellipsis"><HardDrive size={14} class="faint" /> {d.path}</span>
                <span class="num">{bytes(d.available)} free</span>
              </div>
              <Meter value={d.used} max={d.total} label="Disk used on {d.path}" />
            </div>
          {/each}
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <h2>Recent activity</h2>
          <a href={href('/activity')} class="faint">All activity</a>
        </div>
        {#if activity.length === 0}
          <p class="calm">Nothing has happened yet.</p>
        {:else}
          <ul class="activity">
            {#each activity as e (e.id)}
              <li class="sev-{severityTone[e.severity]}">
                <span class="mark" aria-hidden="true"></span>
                <span class="msg">{e.message}</span>
                <TimeAgo ts={e.ts} />
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    </div>
  </div>
</div>

<style>
  .content {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 18px 24px 40px;
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 14px;
    border: 1px solid var(--line);
    border-radius: 17px;
    background: var(--panel);
    color: var(--text-2);
    text-decoration: none;
  }
  .stat:hover {
    border-color: var(--line-strong);
    color: var(--text);
    text-decoration: none;
  }
  .stat .num {
    color: var(--text);
    font-weight: 600;
    font-size: 16px;
  }
  .stat.hot {
    border-color: color-mix(in srgb, var(--update) 45%, var(--line));
  }
  .stat.hot-bad {
    border-color: color-mix(in srgb, var(--bad) 45%, var(--line));
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.idle {
    background: var(--idle);
  }
  .grid-panel {
    padding: 14px 18px;
  }
  .placeholder {
    padding: 30px;
    color: var(--text-3);
  }
  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1.25fr) minmax(300px, 1fr);
    gap: 18px;
    align-items: start;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .calm {
    margin: 0;
    padding: 18px 16px;
    color: var(--text-2);
  }
  .attention {
    margin: 0;
    padding: 6px;
    list-style: none;
  }
  .attention li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border-radius: var(--radius);
    border-left: 3px solid transparent;
  }
  .attention li:hover {
    background: var(--raised);
  }
  .attention li.tone-bad {
    border-left-color: var(--bad);
  }
  .attention li.tone-warn {
    border-left-color: var(--warn);
  }
  .attention li.tone-update {
    border-left-color: var(--update);
  }
  .attention .ico {
    display: grid;
    place-items: center;
    width: 28px;
    color: var(--warn);
  }
  .attention .text {
    display: grid;
    flex: 1;
    min-width: 0;
    color: inherit;
    text-decoration: none;
  }
  .title {
    font-weight: 600;
  }
  .detail {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .more {
    display: block;
    padding: 0 16px 12px;
  }
  .host {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .metric {
    display: grid;
    gap: 6px;
  }
  .metric.small {
    display: flex;
    justify-content: space-between;
    font-size: var(--fs-sm);
  }
  .metric .label {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    font-size: var(--fs-sm);
  }
  .metric .label .num {
    color: var(--text-2);
  }
  .activity {
    margin: 0;
    padding: 8px 16px 12px;
    list-style: none;
  }
  .activity li {
    display: grid;
    grid-template-columns: 8px 1fr auto;
    align-items: baseline;
    gap: 10px;
    padding: 6px 0;
    font-size: var(--fs-sm);
  }
  .activity .mark {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--line-strong);
    transform: translateY(-2px);
  }
  .activity li.sev-ok .mark {
    background: var(--ok);
  }
  .activity li.sev-warn .mark {
    background: var(--warn);
  }
  .activity li.sev-bad .mark {
    background: var(--bad);
  }
  .activity :global(time) {
    color: var(--text-3);
    white-space: nowrap;
  }
  @media (max-width: 1100px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }
  @media (max-width: 760px) {
    .content {
      padding: 14px 12px 32px;
    }
  }
</style>
