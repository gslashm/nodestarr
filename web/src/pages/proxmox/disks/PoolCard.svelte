<!--
  One storage pool: capacity and growth, how many disk failures it survives,
  its vdevs or groups drawn as drive bays (colored by each disk's health),
  scrub or rebuild progress with controls, and what needs attention.
-->
<script lang="ts">
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import ShieldAlert from '@lucide/svelte/icons/shield-alert';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import ShieldX from '@lucide/svelte/icons/shield-x';
  import Square from '@lucide/svelte/icons/square';
  import Button from '../../../components/Button.svelte';
  import Meter from '../../../components/Meter.svelte';
  import { api } from '../../../lib/api';
  import { LEVELS, POOL_KIND, diskSize, fullIn, growth, scanText, shortName, tempTone, toleranceText, verdictOf, type Tone } from '../../../lib/disks';
  import { ago, bytes, plural } from '../../../lib/format';
  import { href } from '../../../lib/router.svelte';
  import { session } from '../../../lib/session.svelte';
  import type { DiskView, Pool, PoolGroup, PoolMember } from '../../../lib/types';
  import { ui } from '../../../lib/ui.svelte';
  import HealthBadge from './HealthBadge.svelte';

  let {
    pool,
    disks,
    canControl,
    multiNode = false,
    onchanged,
  }: { pool: Pool; disks: Map<string, DiskView>; canControl: boolean; multiNode?: boolean; onchanged: () => void } = $props();

  const p = $derived(pool);
  const health = $derived(
    ({ ok: { label: 'Healthy', tone: 'ok' }, degraded: { label: 'Degraded', tone: 'warn' }, faulted: { label: 'Faulted', tone: 'bad' }, unknown: { label: 'Unknown', tone: 'idle' } } as const)[p.health],
  );
  const dataMembers = $derived(p.groups.filter((g) => g.class === 'data').reduce((n, g) => n + g.members.length, 0));
  const rebuilding = $derived(!!p.scan && p.scan.state === 'running' && ['resilver', 'recovery', 'reshape'].includes(p.scan.kind));
  const tol = $derived(toleranceText(p.tolerance, dataMembers, rebuilding));
  const scan = $derived(p.scan);
  const running = $derived(scan?.state === 'running');
  const pct = $derived(p.size && p.used !== null ? (p.used / p.size) * 100 : null);
  // The scan line already shows a running scrub or rebuild.
  const shownFindings = $derived(p.findings.filter((f) => f.code !== 'scan_running'));
  const problems = $derived(shownFindings.filter((f) => f.level !== 'info' && f.level !== 'ok'));
  const notes = $derived(shownFindings.filter((f) => f.level === 'info'));

  function groupLabel(g: PoolGroup): string {
    if (p.kind === 'snapraid') return g.class === 'parity' ? 'Parity' : 'Data';
    if (p.kind === 'md') return g.class === 'spare' ? 'Spares' : p.layout;
    if (p.kind === 'lvmthin') return `Volume group ${g.name}`;
    const cls: Record<string, string> = { log: 'Log', cache: 'Cache', spare: 'Spares', special: 'Special', dedup: 'Dedup' };
    const kind: Record<string, string> = { raidz: `RAID-Z${g.parity}`, draid: `dRAID${g.parity}`, mirror: 'Mirror', disk: 'Disk' };
    const k = kind[g.kind] ?? g.kind;
    return cls[g.class] ? (g.kind === 'disk' ? cls[g.class] : `${cls[g.class]} · ${k}`) : k;
  }

  function memberTone(m: PoolMember, d: DiskView | undefined): Tone | '' {
    if (!m.ok) return m.role === 'spare' ? 'idle' : m.state === 'rebuilding' ? 'warn' : 'bad';
    if (m.state === 'AVAIL') return 'idle';
    if (!d) return '';
    const v = verdictOf(d);
    return v.tone === 'ok' && d.verdict === 'ok' ? 'ok' : v.tone;
  }

  function memberTitle(m: PoolMember, d: DiskView | undefined): string {
    const lines = [m.path ?? m.name];
    if (d) lines.push(`${d.model ?? ''} ${d.serial ? `(${d.serial})` : ''}`.trim(), `${d.devpath} on ${d.node}${d.label.bay ? ` · ${d.label.bay}` : ''}`);
    if (m.state) lines.push(`State: ${m.state}`);
    if (m.note) lines.push(m.note);
    const errs = [m.read, m.write, m.cksum];
    if (errs.some((e) => e)) lines.push(`Errors: read ${m.read ?? 0}, write ${m.write ?? 0}, checksum ${m.cksum ?? 0}`);
    if (d?.temperature !== null && d?.temperature !== undefined) lines.push(`${d.temperature} °C${d.asleep ? ' (asleep, last reading)' : d.resting ? ' (spins down, last daily reading)' : ''}`);
    if (d && d.verdict !== 'ok') lines.push(LEVELS[d.verdict].label + (d.findings[0] ? `: ${d.findings[0].title}` : ''));
    if (m.size) lines.push(`${bytes(m.used)} of ${bytes(m.size)} used`);
    return lines.filter(Boolean).join('\n');
  }

  async function scrub(action: 'start' | 'pause' | 'stop') {
    if (action === 'start' && scan?.state !== 'paused') {
      const ok = await ui.confirm({
        title: `Scrub ${p.name}?`,
        message: `A scrub reads every block in the pool and repairs what it can from redundancy. The pool stays usable, but slower, until it finishes${p.size ? ` (it reads up to ${bytes(p.used)})` : ''}.`,
        confirmLabel: 'Start scrub',
      });
      if (!ok) return;
    }
    if (action === 'stop') {
      const ok = await ui.confirm({ title: `Stop the scrub of ${p.name}?`, message: 'Progress is lost; the next scrub starts from the beginning. Pause it instead to continue later.', confirmLabel: 'Stop scrub', danger: true });
      if (!ok) return;
    }
    try {
      await api.post('/proxmox/disk-health/scrub', { pool: p.id, action });
      ui.toast(action === 'start' ? `Scrub of ${p.name} ${scan?.state === 'paused' ? 'resumed' : 'started'}` : action === 'pause' ? `Scrub of ${p.name} paused` : `Scrub of ${p.name} stopped`, 'success');
      onchanged();
    } catch (err) {
      ui.error(err, "Couldn't control the scrub");
    }
  }
</script>

<section class="panel pool">
  <div class="panel-head">
    <div class="title">
      <h3>{p.name}</h3>
      <span class="chip">{POOL_KIND[p.kind]}</span>
      <span class="faint small">{p.kind === 'lvmthin' ? '' : p.layout}{multiNode ? `${p.kind === 'lvmthin' ? '' : ' · '}${p.node}` : ''}</span>
    </div>
    <HealthBadge tone={health.tone} label={p.state && p.kind === 'zfs' ? p.state : health.label} />
  </div>
  <div class="panel-body body">
    {#if p.size && p.used === null}
      <p class="faint small size-only">{bytes(p.size)}{p.kind === 'md' ? ' array (what fills it is up to the filesystem on top)' : ''}</p>
    {:else if p.size}
      <div class="capacity">
        <div class="cap-line">
          <span><strong class="num">{bytes(p.used)}</strong> <span class="faint">of {bytes(p.size)} used{pct !== null ? ` (${pct.toFixed(0)}%)` : ''}</span></span>
          {#if p.growth_per_day !== null}
            <span class="faint small" title="Average over the last 30 days">{growth(p.growth_per_day)}{p.full_in_days !== null ? `, ${fullIn(p.full_in_days)}` : ''}</span>
          {/if}
        </div>
        <Meter value={p.used ?? 0} max={p.size} warn={p.kind === 'zfs' ? 80 : 85} bad={92} label="{p.name} used" />
        {#if p.fragmentation !== null || (p.dedup && p.dedup > 1)}
          <span class="faint small">
            {#if p.fragmentation !== null}Free space fragmentation {p.fragmentation}%{/if}{#if p.dedup && p.dedup > 1} · dedup ratio {p.dedup.toFixed(2)}×{/if}
          </span>
        {/if}
      </div>
    {/if}
    {#if p.kind === 'lvmthin' && p.metadata_size}
      <div class="capacity">
        <div class="cap-line"><span class="faint small">Metadata {bytes(p.metadata_used)} of {bytes(p.metadata_size)}</span></div>
        <Meter value={p.metadata_used ?? 0} max={p.metadata_size} warn={80} bad={90} height={4} label="{p.name} metadata used" />
      </div>
    {/if}

    {#if tol.text}
      <p class="tol {tol.tone}">
        {#if tol.tone === 'ok'}<ShieldCheck size={16} />{:else if tol.tone === 'bad'}<ShieldX size={16} />{:else}<ShieldAlert size={16} />{/if}
        <span>{tol.text}{p.kind === 'snapraid' && tol.tone === 'ok' ? ' (for data synced so far)' : ''}</span>
      </p>
    {/if}

    <div class="groups">
      {#each p.groups as g (g.name + g.class)}
        <div class="group">
          <span class="gname">{groupLabel(g)}</span>
          <div class="bays">
            {#each g.members as m, i (i)}
              {@const d = m.disk ? disks.get(m.disk) : undefined}
              {@const tone = memberTone(m, d)}
              {@const label = d ? shortName(d) : p.kind === 'snapraid' ? m.name : m.name.length > 14 ? `${m.name.slice(0, 12)}…` : m.name}
              <a
                class="bay {tone}"
                class:asleep={d?.asleep || d?.resting}
                class:linked={!!d}
                class:replacing={g.members.some((o) => o !== m && o.slot === m.slot)}
                href={d ? href(`/proxmox/disks/${encodeURIComponent(d.id)}`) : undefined}
                title={memberTitle(m, d)}
              >
                <span class="bay-name">{p.kind === 'snapraid' ? m.name : label}</span>
                <span class="bay-meta num">
                  {#if !m.ok}{m.state?.toLowerCase() ?? 'down'}{:else if d?.temperature !== null && d?.temperature !== undefined}<span class={tempTone(d.temperature, d.temp_limit) === 'ok' ? '' : `text-${tempTone(d.temperature, d.temp_limit)}`}>{d.temperature}°</span>{:else if m.state === 'AVAIL'}spare{:else}&nbsp;{/if}
                </span>
                {#if p.kind === 'snapraid' && m.size}<span class="bay-fill" style:width="{Math.min(100, ((m.used ?? 0) / m.size) * 100)}%"></span>{/if}
              </a>
            {/each}
          </div>
        </div>
      {/each}
    </div>

    {#if scan && (scan.state !== 'none' || p.kind === 'zfs')}
      <div class="scan">
        {#if running && scan.progress !== null}
          <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(scan.progress * 100)} aria-label={scanText(scan)}>
            <div class="fill {scan.kind}" style:width="{scan.progress * 100}%"></div>
          </div>
        {/if}
        <div class="scan-line">
          <span class:text-warn={scan.state === 'running' && (scan.kind === 'resilver' || scan.kind === 'recovery')} class:text-bad={(scan.errors ?? 0) > 0} class="small">{scanText(scan) || 'No scrub information'}</span>
          {#if canControl && p.kind === 'zfs' && session.isAdmin}
            <span class="scan-actions">
              {#if running && scan.kind === 'scrub'}
                <Button small variant="ghost" icon={Pause} onclick={() => scrub('pause')}>Pause</Button>
                <Button small variant="ghost" icon={Square} onclick={() => scrub('stop')}>Stop</Button>
              {:else if scan.state === 'paused'}
                <Button small variant="ghost" icon={Play} onclick={() => scrub('start')}>Resume</Button>
                <Button small variant="ghost" icon={Square} onclick={() => scrub('stop')}>Stop</Button>
              {:else if !running}
                <Button small variant="ghost" icon={Play} onclick={() => scrub('start')}>Scrub now</Button>
              {/if}
            </span>
          {/if}
        </div>
      </div>
    {/if}

    {#if p.snapraid}
      {@const sr = p.snapraid}
      <dl class="kv small sr">
        <dt>Last sync</dt>
        <dd>{sr.last_sync ? ago(sr.last_sync) : 'unknown'}{sr.busy ? ' · a snapraid command is running' : ''}{sr.sync_progress !== null ? ` · syncing ${sr.sync_progress}%` : ''}</dd>
        {#if sr.oldest_scrub_days !== null}
          <dt>Scrubbed</dt>
          <dd>oldest {plural(sr.oldest_scrub_days, 'day')} ago, median {sr.median_scrub_days ?? '–'}{sr.not_scrubbed_pct !== null ? ` · ${sr.not_scrubbed_pct}% not scrubbed yet` : ''}</dd>
        {/if}
        <dt>Errors</dt>
        <dd class:text-bad={(sr.errors ?? 0) > 0} class:text-ok={sr.errors === 0}>{sr.errors === null ? 'not checked yet' : sr.errors === 0 ? 'none' : sr.errors}</dd>
        <dt>Content files</dt>
        <dd class="mono">{sr.content_files.join(', ') || '–'}</dd>
        {#if sr.status_at}<dt>Status read</dt><dd>{ago(sr.status_at)}</dd>{/if}
      </dl>
    {/if}

    {#if problems.length || p.status_text}
      <ul class="findings">
        {#each problems as f (f.code + f.title)}
          <li class={f.level}><strong>{f.title}</strong>{#if f.detail}<span class="detail">{f.detail}</span>{/if}</li>
        {/each}
        {#if p.status_text && !problems.some((f) => f.detail.includes(p.status_text ?? ''))}
          <li class="watch"><span class="detail">{p.status_text}</span></li>
        {/if}
      </ul>
    {/if}
    {#if notes.length}
      <ul class="notes">
        {#each notes as f (f.code + f.title)}<li title={f.detail}>{f.title}</li>{/each}
      </ul>
    {/if}
    {#if p.kind === 'zfs' && p.size && p.groups.some((g) => g.kind === 'raidz' || g.kind === 'draid')}
      <p class="faint small foot">Sizes are raw, including parity. Usable space is roughly {diskSize(p.size * (1 - (p.groups.find((g) => g.class === 'data')?.parity ?? 0) / Math.max(1, p.groups.find((g) => g.class === 'data')?.members.length ?? 1)))} (decimal).</p>
    {/if}
  </div>
</section>

<style>
  .pool {
    display: flex;
    flex-direction: column;
  }
  .title {
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .title h3 {
    flex: none;
    font-size: var(--fs-lg);
  }
  .body {
    display: grid;
    gap: 14px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .capacity {
    display: grid;
    gap: 5px;
  }
  .cap-line {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 4px 12px;
  }
  .tol {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    font-weight: 500;
  }
  .tol.ok :global(svg) {
    color: var(--ok);
  }
  .tol:not(.ok, .warn, .bad) {
    color: var(--text-2);
    font-weight: 400;
  }
  .size-only {
    margin: 0;
  }
  .tol.warn {
    color: var(--warn);
  }
  .tol.bad {
    color: var(--bad);
  }
  .groups {
    display: grid;
    gap: 10px;
  }
  .group {
    display: grid;
    gap: 5px;
  }
  .gname {
    color: var(--text-2);
    font: 600 var(--fs-xs) / 1 var(--font-data);
    letter-spacing: 0.02em;
  }
  .bays {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .bay {
    --tone: var(--line-strong);
    position: relative;
    display: grid;
    gap: 2px;
    min-width: 62px;
    padding: 7px 9px 6px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-top: 3px solid var(--tone);
    border-radius: var(--radius-sm);
    background: var(--raised);
    color: var(--text);
    text-decoration: none;
  }
  .bay.linked:hover {
    border-color: var(--text-3);
    border-top-color: var(--tone);
    text-decoration: none;
  }
  .bay.ok {
    --tone: var(--ok);
  }
  .bay.info {
    --tone: var(--info);
  }
  .bay.warn {
    --tone: var(--warn);
  }
  .bay.bad {
    --tone: var(--bad);
    background: color-mix(in srgb, var(--bad) 10%, var(--raised));
  }
  .bay.idle {
    --tone: var(--idle);
    border-style: dashed;
    border-top-style: solid;
  }
  .bay.asleep {
    opacity: 0.75;
  }
  .bay.replacing {
    outline: 1px dashed var(--warn);
    outline-offset: 1px;
  }
  .bay-name {
    font: 600 var(--fs-sm) / 1.1 var(--font-data);
    white-space: nowrap;
  }
  .bay-meta {
    color: var(--text-2);
    font-size: var(--fs-xs);
  }
  .bay-fill {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    background: var(--accent);
    opacity: 0.7;
  }
  .scan {
    display: grid;
    gap: 6px;
  }
  .progress {
    height: 6px;
    border-radius: 3px;
    background: var(--sunken);
    border: 1px solid var(--line);
    overflow: hidden;
  }
  .progress .fill {
    height: 100%;
    background: var(--info);
    transition: width 600ms var(--ease);
  }
  .progress .fill.resilver,
  .progress .fill.recovery {
    background: var(--warn);
  }
  .scan-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px 12px;
  }
  .scan-actions {
    display: inline-flex;
    gap: 2px;
  }
  .sr {
    gap: 5px 14px;
  }
  .findings,
  .notes {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .findings li {
    --tone: var(--info);
    display: grid;
    gap: 2px;
    padding: 7px 10px;
    border-left: 3px solid var(--tone);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--tone) 8%, var(--panel));
    font-size: var(--fs-sm);
  }
  .findings li.warning {
    --tone: var(--warn);
  }
  .findings li.failing {
    --tone: var(--bad);
  }
  .detail {
    color: var(--text-2);
  }
  .notes li {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .notes li::before {
    content: '· ';
  }
  .foot {
    margin: 0;
  }
</style>
