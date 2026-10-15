<!--
  Uptime checks: every container web UI Berth watches plus custom checks,
  with recent results, response times, and a tester for any address.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Activity from '@lucide/svelte/icons/activity';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Plus from '@lucide/svelte/icons/plus';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Button from '../../components/Button.svelte';
  import Chart, { type ChartSeries } from '../../components/Chart.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { api, errorMessage, seg } from '../../lib/api';
  import { dateTime, duration, pct, plural, resolveLink } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { Beat, UptimeStatus } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let enabled = $state(true);
  let interval = $state(60);
  /** The check shown in the side panel (kept by id so it stays live). */
  let openId = $state<string | null>(null);
  let beatsRange = $state<'24h' | '7d'>('24h');
  let beats = $state.raw<Beat[] | null>(null);

  let testUrl = $state('');
  let testStatus = $state('');
  let testKeyword = $state('');
  let testing = $state(false);
  let testResult = $state<{ ok: boolean; text: string } | null>(null);

  onMount(() => {
    api
      .get<{ enabled: boolean; interval_secs: number; checks: UptimeStatus[] }>('/uptime')
      .then((r) => {
        enabled = r.enabled;
        interval = r.interval_secs;
        live.uptime = Object.fromEntries(r.checks.map((u) => [u.id, u]));
      })
      .catch(() => {});
  });

  const checks = $derived(
    Object.values(live.uptime).sort((a, b) => Number(b.state === 'down') - Number(a.state === 'down') || a.name.localeCompare(b.name)),
  );
  const down = $derived(checks.filter((c) => c.state === 'down').length);

  const open = $derived(openId ? (live.uptime[openId] ?? null) : null);

  $effect(() => {
    const id = openId;
    const r = beatsRange;
    beats = null;
    if (!id) return;
    api
      .get<{ beats: Beat[] }>(`/uptime/${seg(id)}/beats?range=${r}`)
      .then((res) => {
        if (openId === id) beats = res.beats;
      })
      .catch((err) => ui.error(err));
  });

  async function runNow() {
    try {
      await api.post('/uptime/run');
      ui.toast('Checking every address now');
    } catch (err) {
      ui.error(err);
    }
  }

  async function test() {
    if (!testUrl.trim()) return;
    testing = true;
    testResult = null;
    try {
      const r = await api.post<{ ok: boolean; status: number | null; latency_ms: number; error: string | null }>('/uptime/test', {
        url: testUrl.trim(),
        expect_status: testStatus,
        keyword: testKeyword,
      });
      testResult = r.ok
        ? { ok: true, text: `Healthy: ${r.status ? `status ${r.status}, ` : ''}${r.latency_ms} ms` }
        : { ok: false, text: r.error ?? `Unhealthy: status ${r.status ?? 'none'}` };
    } catch (err) {
      testResult = { ok: false, text: errorMessage(err) };
    } finally {
      testing = false;
    }
  }

  function container(c: UptimeStatus) {
    return c.identity ? live.byIdentity.get(c.identity) : undefined;
  }

  /** The most recent results, oldest first, padded so every bar has the same width. */
  function recent(c: UptimeStatus, n = 40): (Beat | null)[] {
    const b = c.beats.slice(-n);
    return [...Array(Math.max(0, n - b.length)).fill(null), ...b];
  }

  const latencySeries: ChartSeries[] = [{ label: 'Response time', color: 'var(--info)', format: (v) => `${Math.round(v)} ms` }];
  const latencyData = $derived(beats ? [beats.map((b) => b.ts), beats.map((b) => (b.ok ? b.latency_ms : 0))] : []);
  const failures = $derived(beats ? beats.filter((b) => !b.ok).length : 0);
</script>

<div class="uptime">
  <div class="head">
    <p class="muted summary">
      {#if !enabled}
        Uptime checks are off. <a href={href('/settings/uptime')}>Turn them on</a>
      {:else}
        {#if checks.length}
          {plural(checks.length, 'check')}{down ? `, ${down} down` : ', all up'}. Checked every {duration(interval)}.
        {:else}
          No checks yet.
        {/if}
      {/if}
    </p>
    <span class="grow"></span>
    {#if session.isAdmin}
      <Button variant="ghost" icon={RefreshCw} onclick={runNow} disabled={!enabled}>Check now</Button>
      <a class="btn ghost" href={href('/settings/uptime')}><Plus size={17} />Add a check</a>
    {/if}
  </div>

  {#if checks.length === 0}
    <EmptyState icon={Activity} title="Nothing to check yet">
      Berth checks the web UI of containers it can reach on their own. Add your own addresses in Settings > Uptime.
    </EmptyState>
  {:else}
    <section class="panel list">
      {#each checks as c (c.id)}
        {@const ct = container(c)}
        <button type="button" class="check {c.state}" onclick={() => (openId = c.id)}>
          <span class="state-dot" aria-hidden="true"></span>
          <span class="who">
            <span class="name">{ct?.display_name ?? c.name}</span>
            <span class="url mono ellipsis">{c.url}</span>
          </span>
          <span class="bars" aria-label="Recent results">
            {#each recent(c) as b, i (i)}
              <span class="bar" class:ok={b?.ok} class:fail={b && !b.ok} title={b ? `${dateTime(b.ts)}: ${b.ok ? `${b.latency_ms} ms` : 'failed'}` : ''}></span>
            {/each}
          </span>
          <span class="nums">
            <span class="num">{c.latency_ms !== null && c.state === 'up' ? `${c.latency_ms} ms` : c.state === 'down' ? 'Down' : '–'}</span>
            <span class="faint small num">{c.uptime_24h !== null ? `${pct(c.uptime_24h, 1)} 24 h` : ''}</span>
          </span>
        </button>
      {/each}
    </section>
  {/if}

  {#if session.isAdmin}
    <section class="panel tester">
      <div class="panel-head"><h2>Test an address</h2></div>
      <form
        class="panel-body test-form"
        onsubmit={(e) => {
          e.preventDefault();
          void test();
        }}
      >
        <input class="input mono url-in" placeholder="http://192.168.1.50:8989/ or tcp://host:port" bind:value={testUrl} aria-label="Address" />
        <input class="input mono small-in" placeholder="200-399" bind:value={testStatus} aria-label="Healthy status codes" />
        <input class="input small-in" placeholder="Must contain (optional)" bind:value={testKeyword} aria-label="Text the page must contain" />
        <Button type="submit" busy={testing} disabled={!testUrl.trim()}>Test</Button>
        {#if testResult}<p class={testResult.ok ? 'text-ok' : 'text-bad'}>{testResult.text}</p>{/if}
      </form>
    </section>
  {/if}
</div>

<Dialog open={!!open} title={open ? (container(open)?.display_name ?? open.name) : ''} width="760px" drawer onclose={() => (openId = null)}>
  {#if open}
    {@const ct = container(open)}
    <dl class="kv">
      <dt>Address</dt>
      <dd><a href={resolveLink(open.url)} target="_blank" rel="noopener" class="mono">{open.url} <ExternalLink size={12} /></a></dd>
      <dt>State</dt>
      <dd class:text-ok={open.state === 'up'} class:text-bad={open.state === 'down'}>
        {open.state === 'up' ? 'Up' : open.state === 'down' ? 'Down' : open.state === 'paused' ? 'Paused (container stopped)' : 'Waiting for the first check'}
        {#if open.since}<span class="faint"> since <TimeAgo ts={open.since} /></span>{/if}
      </dd>
      {#if open.error}<dt>Last error</dt><dd class="text-bad">{open.error}</dd>{/if}
      {#if open.status_code}<dt>Status code</dt><dd class="num">{open.status_code}</dd>{/if}
      <dt>Last check</dt><dd>{#if open.last_check}<TimeAgo ts={open.last_check} />{:else}–{/if}</dd>
      <dt>Uptime</dt><dd class="num">{open.uptime_24h !== null ? pct(open.uptime_24h, 2) : '–'} last 24 h, {open.uptime_7d !== null ? pct(open.uptime_7d, 2) : '–'} last 7 days</dd>
      {#if ct}<dt>Container</dt><dd><a href={href(`/containers/${encodeURIComponent(ct.name)}`)}>{ct.display_name}</a>{#if !open.custom} <span class="faint">(checked automatically)</span>{/if}</dd>{/if}
    </dl>
    <div class="beats-head">
      <h3>Response time</h3>
      <Segmented label="Range" bind:value={beatsRange} options={[{ value: '24h', label: '24 hours' }, { value: '7d', label: '7 days' }]} />
    </div>
    {#if !beats}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if beats.length < 2}
      <p class="muted">Not enough results yet.</p>
    {:else}
      <Chart data={latencyData} series={latencySeries} yFormat={(v) => `${Math.round(v)} ms`} yMinTop={10} height={180} label="Response time" />
      <p class="muted small">{plural(beats.length, 'check')}, {plural(failures, 'failure')}.</p>
    {/if}
  {/if}
</Dialog>

<style>
  .uptime {
    display: grid;
    gap: 16px;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .summary {
    margin: 0;
  }
  .list {
    display: grid;
    overflow: hidden;
  }
  .check {
    display: grid;
    grid-template-columns: 12px minmax(160px, 1.2fr) minmax(0, 1.6fr) 110px;
    align-items: center;
    gap: 14px;
    padding: 10px 16px;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .check:last-child {
    border-bottom: 0;
  }
  .check:hover {
    background: var(--raised);
  }
  .state-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--idle);
  }
  .up .state-dot {
    background: var(--ok);
  }
  .down .state-dot {
    background: var(--bad);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--bad) 25%, transparent);
  }
  .who {
    display: grid;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .url {
    color: var(--text-3);
    font-size: 12px;
  }
  .bars {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    min-width: 0;
  }
  .bar {
    flex: 1;
    max-width: 8px;
    height: 100%;
    border-radius: 2px;
    background: var(--line);
  }
  .bar.ok {
    background: color-mix(in srgb, var(--ok) 75%, transparent);
  }
  .bar.fail {
    background: var(--bad);
  }
  .nums {
    display: grid;
    justify-items: end;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .test-form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .test-form p {
    flex-basis: 100%;
    margin: 4px 0 0;
  }
  .url-in {
    flex: 2 1 280px;
  }
  .small-in {
    flex: 1 1 140px;
  }
  .beats-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 20px 0 8px;
  }
  .beats-head h3 {
    font-size: var(--fs-md);
  }
  @media (max-width: 760px) {
    .check {
      grid-template-columns: 12px minmax(0, 1fr) 90px;
    }
    .bars {
      display: none;
    }
  }
</style>
