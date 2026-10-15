<!--
  Live logs over a WebSocket (a container's, or a whole stack's with each
  line tagged by service): filter, "problems only", follow, wrap, time,
  copy and download. When `running` turns true again after the stream
  ended, it resumes where it stopped instead of starting over.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import ArrowDownToLine from '@lucide/svelte/icons/arrow-down-to-line';
  import Download from '@lucide/svelte/icons/download';
  import Eraser from '@lucide/svelte/icons/eraser';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import Search from '@lucide/svelte/icons/search';
  import { logLevel } from '../lib/ansi';
  import { qs, wsUrl } from '../lib/api';
  import { fillHeight } from '../lib/fill';
  import { router } from '../lib/router.svelte';
  import type { LogLine } from '../lib/types';
  import CopyButton from './CopyButton.svelte';
  import LogView, { type ViewLine } from './LogView.svelte';
  import Spinner from './Spinner.svelte';

  interface Props {
    /** API path of the WebSocket, without the query. */
    path: string;
    /** Link that downloads the full log, if there is one. */
    downloadUrl?: string;
    /** Whether the source is running; a stopped stream resumes when this turns true. */
    running?: boolean;
    /** Service names, for stack logs: adds a service filter. */
    services?: string[];
  }

  let { path, downloadUrl, running = true, services = [] }: Props = $props();

  /** Lines kept in the browser; older ones are dropped (the download has everything). */
  const MAX_LINES = 5000;
  const PREFS_KEY = 'berth.logs';

  interface ViewPrefs {
    tail: string;
    showTime: boolean;
    wrap: boolean;
  }

  function loadPrefs(): ViewPrefs {
    const defaults: ViewPrefs = { tail: '500', showTime: true, wrap: true };
    try {
      return { ...defaults, ...JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}') };
    } catch {
      return defaults;
    }
  }

  const initial = loadPrefs();
  let tail = $state(initial.tail);
  let showTime = $state(initial.showTime);
  let wrap = $state(initial.wrap);
  let filter = $state(router.query.get('q') ?? '');
  let service = $state(router.query.get('service') ?? '');
  let problemsOnly = $state(false);
  let follow = $state(true);

  let lines = $state.raw<ViewLine[]>([]);
  let status = $state<'connecting' | 'live' | 'ended' | 'error' | 'closed'>('connecting');
  let error = $state('');
  let view: { text: () => string } | undefined = $state();

  let socket: WebSocket | null = null;
  let seq = 0;
  let pending: ViewLine[] = [];
  let flushFrame = 0;

  /** Newest timestamp per source (service, or '' for a single container) and the lines at it. */
  let newest = new Map<string, { ts: string; texts: string[] }>();
  /** While resuming: where each source left off, to skip lines already shown. */
  let resumeFrom: Map<string, { ts: string; texts: string[] }> | null = null;

  $effect(() => {
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify({ tail, showTime, wrap }));
    } catch {
      // Private mode: preferences just don't stick.
    }
  });

  const shown = $derived(
    lines.filter((l) => (!service || l.c === service) && (!problemsOnly || l.s === 'err' || logLevel(l.t) !== '')),
  );

  /** Merges new lines in timestamp order (stack logs arrive per container). */
  function flush() {
    flushFrame = 0;
    if (!pending.length) return;
    const batch = pending.sort((a, b) => (a.ts ?? '').localeCompare(b.ts ?? ''));
    pending = [];
    let next: ViewLine[];
    const last = lines[lines.length - 1];
    if (!last || !batch[0].ts || !last.ts || batch[0].ts >= last.ts) {
      next = lines.concat(batch);
    } else {
      next = [];
      let i = 0;
      let j = 0;
      while (i < lines.length || j < batch.length) {
        if (j >= batch.length || (i < lines.length && (lines[i].ts ?? '') <= (batch[j].ts ?? ''))) next.push(lines[i++]);
        else next.push(batch[j++]);
      }
    }
    if (next.length > MAX_LINES) next = next.slice(next.length - MAX_LINES);
    lines = next;
  }

  function accept(batch: LogLine[]) {
    for (const l of batch) {
      const key = l.c ?? '';
      if (l.ts && resumeFrom) {
        const r = resumeFrom.get(key);
        if (r) {
          if (l.ts < r.ts || (l.ts === r.ts && r.texts.includes(l.t))) continue;
          if (l.ts > r.ts) resumeFrom.delete(key);
        }
      }
      if (l.ts) {
        const n = newest.get(key);
        if (!n || l.ts > n.ts) newest.set(key, { ts: l.ts, texts: [l.t] });
        else if (l.ts === n.ts) n.texts.push(l.t);
      }
      pending.push({ n: seq++, ts: l.ts, s: l.s, t: l.t, c: l.c });
    }
    if (!flushFrame) flushFrame = requestAnimationFrame(flush);
  }

  /** Opens the stream. With `resume`, continues after the last lines instead of starting over. */
  function connect(resume = false) {
    socket?.close();
    let since: number | null = null;
    if (resume && newest.size) {
      resumeFrom = new Map([...newest].map(([k, v]) => [k, { ts: v.ts, texts: [...v.texts] }]));
      const oldest = [...newest.values()].reduce((a, b) => (a.ts < b.ts ? a : b)).ts;
      since = Math.floor(Date.parse(oldest) / 1000);
    } else {
      lines = [];
      pending = [];
      newest = new Map();
      resumeFrom = null;
    }
    status = 'connecting';
    error = '';
    const ws = new WebSocket(wsUrl(`${path}${qs({ tail: since ? 'all' : tail, since, timestamps: 1 })}`));
    socket = ws;
    ws.onopen = () => {
      if (socket === ws) status = 'live';
    };
    ws.onmessage = (e) => {
      if (socket !== ws) return;
      let msg: { lines?: LogLine[]; end?: boolean; error?: string };
      try {
        msg = JSON.parse(e.data as string);
      } catch {
        return;
      }
      if (msg.lines) accept(msg.lines);
      if (msg.end) status = 'ended';
      if (msg.error) {
        status = 'error';
        error = msg.error;
      }
    };
    ws.onclose = () => {
      if (socket !== ws) return;
      if (status === 'connecting') {
        status = 'error';
        error = 'Could not open the log stream.';
      } else if (status === 'live') {
        status = 'closed';
      }
    };
  }

  // Connect on mount and whenever the amount of history changes.
  $effect(() => {
    void tail;
    connect();
  });

  // When the source starts again, pick up its new output.
  let wasRunning = false;
  $effect(() => {
    if (running && !wasRunning && (status === 'ended' || status === 'closed')) connect(true);
    wasRunning = running;
  });

  // Keep the filters in the address bar so a filtered view can be shared.
  $effect(() => {
    const q = filter.trim();
    const svc = service;
    const t = setTimeout(() => router.setQuery({ q: q || null, service: svc || null }), 300);
    return () => clearTimeout(t);
  });

  onDestroy(() => {
    cancelAnimationFrame(flushFrame);
    const ws = socket;
    socket = null;
    ws?.close();
  });

  function clear() {
    lines = [];
    pending = [];
  }
</script>

<div class="logs">
  <div class="toolbar">
    <label class="search">
      <Search size={15} />
      <input class="input" type="search" placeholder="Filter lines" bind:value={filter} aria-label="Filter log lines" />
    </label>
    {#if services.length > 1}
      <select class="input compact" bind:value={service} aria-label="Service">
        <option value="">All services</option>
        {#each services as s (s)}<option value={s}>{s}</option>{/each}
      </select>
    {/if}
    <select class="input compact" bind:value={tail} aria-label="Lines to load">
      <option value="200">Last 200 lines</option>
      <option value="500">Last 500 lines</option>
      <option value="2000">Last 2,000 lines</option>
      <option value="5000">Last 5,000 lines</option>
    </select>
    <div class="toggles" role="group" aria-label="View options">
      <button type="button" class="btn small ghost" aria-pressed={problemsOnly} onclick={() => (problemsOnly = !problemsOnly)} title="Show only errors and warnings">Problems</button>
      <button type="button" class="btn small ghost" aria-pressed={showTime} onclick={() => (showTime = !showTime)}>Time</button>
      <button type="button" class="btn small ghost" aria-pressed={wrap} onclick={() => (wrap = !wrap)}>Wrap</button>
    </div>
    <span class="grow"></span>
    <span class="state">
      {#if status === 'connecting'}
        <Spinner size={13} /> Connecting
      {:else if status === 'live'}
        <span class="dot live"></span> Live
      {:else if status === 'ended'}
        <span class="dot"></span> {running ? 'Stream ended' : 'Stopped; resumes when it starts'}
      {:else}
        <span class="dot bad"></span> {status === 'closed' ? 'Disconnected' : 'Error'}
      {/if}
    </span>
    {#if status === 'error' || status === 'closed' || (status === 'ended' && running)}
      <button type="button" class="btn small" onclick={() => connect(true)}><RotateCcw size={14} />Reconnect</button>
    {/if}
    <button type="button" class="btn small ghost icon" onclick={clear} title="Clear the view" aria-label="Clear the view"><Eraser size={15} /></button>
    <CopyButton text={() => view?.text() ?? ''} label="Copy visible lines" />
    {#if downloadUrl}
      <a class="btn small ghost icon" href={downloadUrl} download title="Download the full log" aria-label="Download the full log"><Download size={15} /></a>
    {/if}
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="box" use:fillHeight={{ bottom: 44, min: 300 }}>
    <LogView
      bind:this={view}
      lines={shown}
      bind:follow
      {wrap}
      {showTime}
      {filter}
      empty={status === 'connecting' ? 'Loading…' : problemsOnly ? 'No errors or warnings in the loaded lines.' : 'No output yet.'}
    />
    {#if !follow}
      <button type="button" class="btn small jump" onclick={() => (follow = true)}><ArrowDownToLine size={14} />Jump to newest</button>
    {/if}
  </div>
  <p class="foot faint">
    {shown.length.toLocaleString()} {shown.length === 1 ? 'line' : 'lines'}{lines.length >= MAX_LINES
      ? ` (the newest ${MAX_LINES.toLocaleString()} are kept here${downloadUrl ? '; download for everything' : ''})`
      : ''}
  </p>
</div>

<style>
  .logs {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    width: min(320px, 100%);
    color: var(--text-3);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    pointer-events: none;
  }
  .search .input {
    min-height: 32px;
    padding: 4px 10px 4px 32px;
  }
  .compact {
    width: auto;
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .toggles {
    display: flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .toggles .btn[aria-pressed='true'] {
    --btn-bg: var(--accent-soft);
    --btn-fg: var(--accent-text);
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--idle);
  }
  .dot.live {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 25%, transparent);
  }
  .dot.bad {
    background: var(--bad);
  }
  .box {
    position: relative;
  }
  .jump {
    position: absolute;
    right: 18px;
    bottom: 14px;
    box-shadow: var(--shadow);
  }
  .error {
    margin: 0;
    color: var(--bad);
  }
  .foot {
    margin: -4px 0 0;
    font-size: var(--fs-sm);
  }
  @media (max-width: 760px) {
    .search {
      width: 100%;
    }
  }
</style>
