<!--
  Scrolling log output with ANSI colors, optional timestamps and a filter.
  Follows new lines while scrolled to the bottom; scrolling up pauses that.
-->
<script lang="ts" module>
  export interface ViewLine {
    /** Sequence number, unique within one view. */
    n: number;
    ts?: string;
    s: string;
    t: string;
    c?: string;
  }
</script>

<script lang="ts">
  import { ansiToHtml, escapeHtml, logLevel, stripAnsi } from '../lib/ansi';
  import { monogramColor } from '../lib/monogram';

  interface Props {
    lines: ViewLine[];
    follow?: boolean;
    wrap?: boolean;
    showTime?: boolean;
    filter?: string;
    empty?: string;
    height?: string;
  }

  let {
    lines,
    follow = $bindable(true),
    wrap = true,
    showTime = true,
    filter = '',
    empty = 'No output yet.',
    height = '100%',
  }: Props = $props();

  let box: HTMLDivElement | undefined = $state();

  const needle = $derived(filter.trim().toLowerCase());
  const visible = $derived(needle ? lines.filter((l) => stripAnsi(l.t).toLowerCase().includes(needle) || l.c?.toLowerCase().includes(needle)) : lines);

  function level(t: string): string {
    const l = logLevel(t);
    return l ? `lvl-${l}` : '';
  }

  function render(t: string): string {
    if (!needle) return ansiToHtml(t);
    // Highlighting works on plain text, so colors are dropped for matching lines.
    const plain = stripAnsi(t);
    const lower = plain.toLowerCase();
    let out = '';
    let i = 0;
    for (;;) {
      const j = lower.indexOf(needle, i);
      if (j < 0) break;
      out += escapeHtml(plain.slice(i, j)) + `<mark>${escapeHtml(plain.slice(j, j + needle.length))}</mark>`;
      i = j + needle.length;
    }
    return out + escapeHtml(plain.slice(i));
  }

  function shortTime(ts?: string): string {
    if (!ts) return '';
    const d = new Date(ts);
    return Number.isNaN(d.getTime()) ? '' : d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
  }

  function onScroll() {
    if (!box) return;
    const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
    if (follow !== atBottom) follow = atBottom;
  }

  $effect(() => {
    void visible.length;
    if (follow && box) {
      requestAnimationFrame(() => {
        if (box) box.scrollTop = box.scrollHeight;
      });
    }
  });

  /** Visible lines as plain text (for copying). */
  export function text(): string {
    return visible.map((l) => `${showTime && l.ts ? `${l.ts} ` : ''}${l.c ? `${l.c} | ` : ''}${stripAnsi(l.t)}`).join('\n');
  }
</script>

<div class="log" class:wrap bind:this={box} onscroll={onScroll} style:height role="log" aria-live="off" tabindex="-1">
  {#if visible.length === 0}
    <div class="empty">{needle ? 'No lines match the filter.' : empty}</div>
  {:else}
    {#each visible as l (l.n)}
      <div class="line {level(l.t)}" class:err={l.s === 'err'}>
        {#if showTime && l.ts}<span class="ts" title={l.ts}>{shortTime(l.ts)}</span>{/if}
        {#if l.c}<span class="svc" style:color="color-mix(in srgb, {monogramColor(l.c)} 70%, var(--text))">{l.c}</span>{/if}
        <span class="t">{@html render(l.t)}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .log {
    overflow: auto;
    padding: 8px 0;
    background: var(--sunken);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    font: 400 12.5px / 1.55 var(--font-code);
    color: var(--text);
  }
  .line {
    display: flex;
    gap: 10px;
    padding: 0 12px;
    white-space: pre;
    border-left: 2px solid transparent;
  }
  .wrap .line {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .line:hover {
    background: color-mix(in srgb, var(--raised) 60%, transparent);
  }
  .line.err {
    border-left-color: color-mix(in srgb, var(--bad) 60%, transparent);
  }
  .lvl-error .t {
    color: color-mix(in srgb, var(--bad) 75%, var(--text));
  }
  .lvl-warn .t {
    color: color-mix(in srgb, var(--warn) 75%, var(--text));
  }
  .ts {
    flex: none;
    color: var(--text-3);
    user-select: none;
  }
  .svc {
    flex: none;
    min-width: 80px;
    color: var(--accent-text);
    user-select: none;
  }
  .t {
    min-width: 0;
  }
  .empty {
    padding: 16px;
    color: var(--text-3);
    font-family: var(--font-ui);
  }
  .log :global(mark) {
    background: color-mix(in srgb, var(--accent) 45%, transparent);
    color: inherit;
    border-radius: 2px;
  }
  .log :global(.a-bold) {
    font-weight: 600;
  }
  .log :global(.a-r),
  .log :global(.a-R) {
    color: #f07178;
  }
  .log :global(.a-g),
  .log :global(.a-G) {
    color: #9ccc65;
  }
  .log :global(.a-y),
  .log :global(.a-Y) {
    color: #ffcb6b;
  }
  .log :global(.a-b),
  .log :global(.a-B) {
    color: #82aaff;
  }
  .log :global(.a-m),
  .log :global(.a-M) {
    color: #c792ea;
  }
  .log :global(.a-c),
  .log :global(.a-C) {
    color: #89ddff;
  }
  .log :global(.a-K) {
    color: var(--text-3);
  }
  :global([data-theme='light']) .log :global(.a-r),
  :global([data-theme='light']) .log :global(.a-R) {
    color: #c62828;
  }
  :global([data-theme='light']) .log :global(.a-g),
  :global([data-theme='light']) .log :global(.a-G) {
    color: #2e7d32;
  }
  :global([data-theme='light']) .log :global(.a-y),
  :global([data-theme='light']) .log :global(.a-Y) {
    color: #9a6700;
  }
  :global([data-theme='light']) .log :global(.a-b),
  :global([data-theme='light']) .log :global(.a-B) {
    color: #1f5fbf;
  }
  :global([data-theme='light']) .log :global(.a-c),
  :global([data-theme='light']) .log :global(.a-C) {
    color: #00838f;
  }
</style>
