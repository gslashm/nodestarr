<!-- Overview of one container: problems first, then status, image, app, network, storage and configuration. -->
<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Search from '@lucide/svelte/icons/search';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import StatePill from '../../components/StatePill.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { checkNow } from '../../lib/actions';
  import { api, seg } from '../../lib/api';
  import { bytes, dateTime, hasUpdate, isUp, pct, plural, resolveLink, shortDigest, stateOf, titleCase, updateLabel } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail, ContainerView } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { c, detail }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  let envFilter = $state('');
  let showLabels = $state(false);

  const st = $derived(stateOf(c));
  const up = $derived(isUp(st));
  const app = $derived(c.app_status);
  const issues = $derived(app?.health.filter((h) => h.level !== 'notice') ?? []);
  const env = $derived((detail?.env ?? []).filter((e) => !envFilter || `${e.key}=${e.value}`.toLowerCase().includes(envFilter.toLowerCase())));

  const uptimeText = $derived.by(() => {
    const u = detail?.uptime;
    if (!u) return '';
    const parts: string[] = [];
    if (u.latency_ms !== null) parts.push(`${u.latency_ms} ms`);
    if (u.uptime_24h !== null) parts.push(`${pct(u.uptime_24h, 1)} over 24 hours`);
    return parts.length ? `(${parts.join(', ')})` : '';
  });

  async function refreshApp() {
    try {
      const s = await api.post<ContainerView['app_status']>(`/containers/${seg(c.id)}/app/refresh`);
      c.app_status = s;
      ui.toast(s?.reachable ? `${c.display_name} answered` : `${c.display_name} didn't answer${s?.error ? `: ${s.error}` : ''}`, s?.reachable ? 'success' : 'warning');
    } catch (err) {
      ui.error(err);
    }
  }

  const credText: Record<string, string> = {
    discovered: 'API key read from the app’s config file',
    configured: 'Using the credentials set in Settings',
    missing: 'No API key found',
    not_needed: 'No credentials needed',
  };
</script>

<div class="grid">
  {#if st === 'crashed' || st === 'dead' || st === 'unhealthy' || st === 'restarting' || c.uptime?.state === 'down' || issues.length}
    <section class="alerts span">
      {#if st === 'crashed' || st === 'dead'}
        <div class="alert bad">
          <CircleAlert size={18} />
          <div>
            <strong>{c.oom_killed ? 'Killed for using too much memory' : `Exited with code ${c.exit_code}`}</strong>
            <span>{c.finished_at ? `at ${dateTime(c.finished_at)}. ` : ''}Check the <a href={href(`/containers/${encodeURIComponent(c.name)}/logs`)}>logs</a> for what happened before it stopped.</span>
          </div>
        </div>
      {/if}
      {#if st === 'unhealthy' && detail?.health_log.length}
        <div class="alert bad">
          <CircleAlert size={18} />
          <div>
            <strong>The health check is failing</strong>
            <pre class="out">{detail.health_log[0].output || `exit code ${detail.health_log[0].exit_code}`}</pre>
          </div>
        </div>
      {/if}
      {#if st === 'restarting'}
        <div class="alert bad">
          <CircleAlert size={18} />
          <div><strong>Keeps restarting</strong><span>Docker restarted it {c.restart_count} times. The logs usually show why.</span></div>
        </div>
      {/if}
      {#if c.uptime?.state === 'down'}
        <div class="alert bad">
          <CircleAlert size={18} />
          <div>
            <strong>The web UI isn't answering</strong>
            <span>{detail?.uptime?.error ?? 'The uptime check failed several times in a row.'}</span>
          </div>
        </div>
      {/if}
      {#each issues as h}
        <div class="alert {h.level === 'error' ? 'bad' : 'warn'}">
          <TriangleAlert size={18} />
          <div>
            <strong>{h.message}</strong>
            <span>
              Reported by {c.display_name}{h.source ? ` (${h.source})` : ''}.
              {#if h.wiki}<a href={h.wiki} target="_blank" rel="noopener">How to fix it <ExternalLink size={12} /></a>{/if}
            </span>
          </div>
        </div>
      {/each}
    </section>
  {/if}

  <section class="panel">
    <div class="panel-head"><h2>Status</h2></div>
    <dl class="kv panel-body">
      <dt>State</dt>
      <dd><StatePill state={st} /></dd>
      {#if up && c.started_at}<dt>Started</dt><dd><TimeAgo ts={c.started_at} /></dd>{/if}
      {#if !up && c.finished_at}<dt>Stopped</dt><dd><TimeAgo ts={c.finished_at} /></dd>{/if}
      <dt>Restarts</dt>
      <dd class="num">{c.restart_count} <span class="faint">(policy: {c.restart_policy || 'no'})</span></dd>
      {#if c.health}<dt>Health check</dt><dd>{titleCase(c.health)}</dd>{/if}
      {#if detail?.uptime}
        <dt>Web UI</dt>
        <dd>
          <span class={detail.uptime.state === 'up' ? 'text-ok' : detail.uptime.state === 'down' ? 'text-bad' : 'muted'}>{titleCase(detail.uptime.state)}</span>
          <span class="faint num">{uptimeText}</span>
        </dd>
      {/if}
      {#if up && c.stats}
        <dt>CPU</dt>
        <dd class="num">{pct(c.stats.cpu, 1)}</dd>
        <dt>Memory</dt>
        <dd class="num">{bytes(c.stats.mem)}{#if detail?.memory_limit} <span class="faint">of {bytes(detail.memory_limit)} limit</span>{/if}</dd>
      {/if}
      {#if c.last_backup}<dt>Last backup</dt><dd><TimeAgo ts={c.last_backup} /></dd>{/if}
    </dl>
  </section>

  <section class="panel">
    <div class="panel-head">
      <h2>Image</h2>
      {#if session.isAdmin}<Button small variant="ghost" icon={Search} onclick={() => checkNow(c)}>Check now</Button>{/if}
    </div>
    <dl class="kv panel-body">
      <dt>Image</dt>
      <dd class="mono copyable">{c.image} <CopyButton text={c.image} label="Copy image" /></dd>
      {#if c.version}<dt>Version</dt><dd class="num">{c.version} {#if c.version_source}<span class="faint">(from {c.version_source})</span>{/if}</dd>{/if}
      <dt>Updates</dt>
      <dd>
        {#if hasUpdate(c.update)}
          <a class="text-update" href={href(`/containers/${encodeURIComponent(c.name)}/updates`)}>Available: {updateLabel(c.update)}</a>
        {:else if c.update?.status === 'up_to_date'}
          <span class="text-ok">Up to date</span> <span class="faint">(checked <TimeAgo ts={c.update.checked_at} />)</span>
        {:else if c.update?.status === 'error'}
          <span class="text-bad">Check failed</span> <span class="faint">{c.update.error}</span>
        {:else if c.update?.status === 'local'}
          <span class="muted">Built locally; no registry to compare with</span>
        {:else if c.update?.status === 'pinned'}
          <span class="muted">Pinned to a digest</span>
        {:else if c.policy === 'ignore'}
          <span class="muted">Not checked (ignored)</span>
        {:else}
          <span class="muted">Not checked yet</span>
        {/if}
      </dd>
      {#if detail?.image}
        <dt>Built</dt>
        <dd>{detail.image.created ? dateTime(detail.image.created) : '–'}</dd>
        <dt>Size</dt>
        <dd class="num">{bytes(detail.image.size)} <span class="faint">{detail.image.os}/{detail.image.arch}</span></dd>
        <dt>Image ID</dt>
        <dd class="mono">{shortDigest(detail.image.id)}</dd>
        {#if detail.image.source}<dt>Source</dt><dd><a href={detail.image.source} target="_blank" rel="noopener" class="ellipsis">{detail.image.source.replace(/^https?:\/\//, '')}</a></dd>{/if}
      {/if}
    </dl>
  </section>

  {#if app}
    <section class="panel">
      <div class="panel-head">
        <h2>{c.app_name ?? 'App'}</h2>
        <Button small variant="ghost" icon={RefreshCw} onclick={refreshApp}>Ask again</Button>
      </div>
      <dl class="kv panel-body">
        <dt>Answers</dt>
        <dd>
          {#if app.reachable}<span class="text-ok">Yes</span>{:else}<span class="text-bad">No</span>{/if}
          <span class="faint">(<TimeAgo ts={app.checked_at} />)</span>
        </dd>
        {#if app.error}<dt>Problem</dt><dd class="text-bad">{app.error}</dd>{/if}
        {#if app.version}<dt>Version</dt><dd class="num">{app.version}{app.branch ? ` (${app.branch})` : ''}</dd>{/if}
        {#each Object.entries(app.details) as [k, v] (k)}
          <dt>{k}</dt>
          <dd class="num">{v}</dd>
        {/each}
        <dt>API access</dt>
        <dd class="muted">{credText[app.credentials] ?? app.credentials}{#if app.credentials === 'missing'}. <a href={href(`/containers/${encodeURIComponent(c.name)}/settings`)}>Add one</a>{/if}</dd>
      </dl>
    </section>
  {/if}

  <section class="panel">
    <div class="panel-head"><h2>Network</h2></div>
    <div class="panel-body stack-sm">
      {#if c.ports.length}
        <div>
          <h3 class="sub">Ports</h3>
          <ul class="plain">
            {#each c.ports as p (`${p.private_port}/${p.protocol}/${p.public_port}`)}
              <li class="num">
                {#if p.public_port}
                  {#if p.protocol === 'tcp'}
                    <a href={resolveLink(`http://{host}:${p.public_port}`)} target="_blank" rel="noopener">{p.public_port}</a>
                  {:else}{p.public_port}{/if}
                  <span class="faint">→ {p.private_port}/{p.protocol}</span>
                  {#if p.host_ip && p.host_ip !== '0.0.0.0' && p.host_ip !== '::'}<span class="faint"> on {p.host_ip}</span>{/if}
                {:else}
                  <span class="faint">{p.private_port}/{p.protocol} (not published)</span>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}
      <div>
        <h3 class="sub">Networks</h3>
        {#if detail?.networks.length}
          <ul class="plain">
            {#each detail.networks as n (n.name)}
              <li><span class="mono">{n.name}</span> <span class="faint num">{n.ip || ''}</span></li>
            {/each}
          </ul>
        {:else}
          <p class="muted">{c.network_mode}</p>
        {/if}
      </div>
      {#if detail?.hostname}<p class="faint small">Hostname <span class="mono">{detail.hostname}</span></p>{/if}
    </div>
  </section>

  <section class="panel">
    <div class="panel-head"><h2>Storage</h2></div>
    <div class="panel-body">
      {#if c.mounts.length === 0}
        <p class="muted">No volumes or folders mounted. Everything the app writes is lost when the container is recreated.</p>
      {:else}
        <ul class="mounts">
          {#each c.mounts as m (m.destination)}
            <li>
              <span class="mono dest">{m.destination}</span>
              <span class="src ellipsis" title={m.source}>
                {m.kind === 'volume' ? `volume ${m.name ?? ''}` : m.source}
              </span>
              <span class="chip">{m.kind}{m.rw ? '' : ', read-only'}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>

  <section class="panel span">
    <div class="panel-head">
      <h2>Environment</h2>
      <input class="input filter" placeholder="Filter" bind:value={envFilter} aria-label="Filter environment variables" />
    </div>
    <div class="panel-body">
      {#if !detail}
        <p class="faint">Loading…</p>
      {:else if detail.env.length === 0}
        <p class="muted">No environment variables.</p>
      {:else}
        <div class="env">
          {#each env as e}
            <span class="mono key">{e.key}</span>
            <span class="mono val" class:secret={e.secret}>{e.value}</span>
          {/each}
        </div>
        <p class="faint small">Values that look like passwords or keys are hidden.</p>
      {/if}
      {#if detail}
        <div class="cmd">
          {#if detail.entrypoint?.length}<p><span class="muted">Entrypoint</span> <code>{detail.entrypoint.join(' ')}</code></p>{/if}
          {#if detail.command?.length}<p><span class="muted">Command</span> <code>{detail.command.join(' ')}</code></p>{/if}
          {#if detail.user}<p><span class="muted">User</span> <code>{detail.user}</code></p>{/if}
          {#if detail.privileged}<p class="text-warn">Runs privileged (full access to the host).</p>{/if}
          {#if detail.devices.length}<p><span class="muted">Devices</span> {#each detail.devices as d}<code>{d.host}</code> {/each}</p>{/if}
          {#if detail.cpu_limit}<p><span class="muted">CPU limit</span> {detail.cpu_limit} cores</p>{/if}
        </div>
        <button type="button" class="btn ghost small" onclick={() => (showLabels = !showLabels)}>{showLabels ? 'Hide' : 'Show'} {plural(detail.labels.length, 'label')}</button>
        {#if showLabels}
          <div class="env labels">
            {#each detail.labels as l (l.key)}
              <span class="mono key">{l.key}</span>
              <span class="mono val">{l.value}</span>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </section>

  {#if detail?.notes}
    <section class="panel span">
      <div class="panel-head"><h2>Notes</h2><a href={href(`/containers/${encodeURIComponent(c.name)}/settings`)} class="faint">Edit</a></div>
      <p class="panel-body notes">{detail.notes}</p>
    </section>
  {/if}
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(380px, 1fr));
    gap: 16px;
    align-items: start;
  }
  .span {
    grid-column: 1 / -1;
  }
  .alerts {
    display: grid;
    gap: 8px;
  }
  .alert {
    display: flex;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--warn);
    border-radius: var(--radius);
    background: var(--panel);
  }
  .alert :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--warn);
  }
  .alert.bad {
    border-left-color: var(--bad);
  }
  .alert.bad :global(svg) {
    color: var(--bad);
  }
  .alert div {
    display: grid;
    gap: 2px;
  }
  .alert span {
    color: var(--text-2);
  }
  .out {
    margin-top: 4px;
    color: var(--text-2);
    font-size: 12.5px;
  }
  .kv dd {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
  }
  .copyable {
    gap: 2px;
  }
  .stack-sm {
    display: grid;
    gap: 14px;
  }
  .sub {
    margin-bottom: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .plain {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
  }
  .small {
    font-size: var(--fs-sm);
    margin: 0;
  }
  .mounts {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 10px;
  }
  .mounts li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 2px 10px;
  }
  .mounts .src {
    grid-column: 1;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .mounts .chip {
    grid-row: 1 / span 2;
    grid-column: 2;
    align-self: center;
  }
  .filter {
    width: 180px;
    min-height: 30px;
    padding: 4px 8px;
  }
  .env {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 18px;
    margin-bottom: 8px;
  }
  .env .key {
    color: var(--text-2);
  }
  .env .val {
    overflow-wrap: anywhere;
  }
  .env .val.secret {
    color: var(--text-3);
  }
  .labels {
    margin-top: 10px;
  }
  .cmd {
    display: grid;
    gap: 4px;
    margin: 12px 0;
  }
  .cmd p {
    margin: 0;
  }
  .notes {
    margin: 0;
    white-space: pre-wrap;
  }
  @media (max-width: 760px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
