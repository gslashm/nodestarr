<!--
  The YAML a service is written as, next to its form: changed lines are
  marked, and the lines of the section being edited are highlighted, so
  every click in the form shows what it does to the file.
-->
<script lang="ts">
  import { diffLines } from '../../lib/diff';
  import { serviceMap, servicesMap } from '../../lib/compose/service';
  import { useStudio } from '../../lib/compose/studio.svelte';
  import { pairLines, pairOf } from '../../lib/compose/yamldoc';

  let { rel, service, keys = [] }: { rel: string; service: string; keys?: string[] } = $props();
  const studio = useStudio();

  let whole = $state(false);
  let box: HTMLDivElement | undefined = $state();

  const text = $derived(studio.text(rel));
  const lines = $derived(text.replace(/\r\n/g, '\n').split('\n'));
  const doc = $derived(studio.doc(rel));

  /** New-file line numbers (1-based) that differ from the saved file. */
  const changed = $derived.by(() => {
    const original = studio.files[rel]?.original ?? '';
    const set = new Set<number>();
    let n = 0;
    for (const d of diffLines(original, text)) {
      if (d.op === '-') continue;
      n++;
      if (d.op === '+') set.add(n);
    }
    return set;
  });

  const range = $derived.by((): [number, number] => {
    const services = doc ? servicesMap(doc) : null;
    const p = services ? pairOf(services, service) : undefined;
    return (p && pairLines(text, p)) || [1, lines.length];
  });

  const focus = $derived.by(() => {
    const set = new Set<number>();
    const svc = doc ? serviceMap(doc, service) : null;
    if (!svc) return set;
    for (const k of keys) {
      const p = pairOf(svc, k);
      const r = p && pairLines(text, p);
      if (r) for (let l = r[0]; l <= r[1]; l++) set.add(l);
    }
    return set;
  });

  const shown = $derived.by(() => {
    const [a, b] = whole ? [1, lines.length] : range;
    const out: { n: number; text: string }[] = [];
    for (let l = a; l <= Math.min(b, lines.length); l++) out.push({ n: l, text: lines[l - 1] });
    // Trailing empty line of the file isn't worth a row.
    if (out.length > 1 && out[out.length - 1].text === '') out.pop();
    return out;
  });

  // Bring the highlighted lines into view when the section changes.
  $effect(() => {
    const first = [...focus].sort((a, b) => a - b)[0];
    if (!box || !first) return;
    const el = box.querySelector<HTMLElement>(`[data-line="${first}"]`);
    if (el) box.scrollTo({ top: Math.max(0, el.offsetTop - 48), behavior: 'smooth' });
  });

  interface Tok {
    t: string;
    c: string;
  }

  /** Light YAML highlighting: keys, strings, numbers and comments. */
  function tokens(line: string): Tok[] {
    const out: Tok[] = [];
    let rest = line;
    const m = /^(\s*(?:-\s+)?)((?:"[^"]*"|'[^']*'|[^\s#'"][^:#]*?))(:)(?=\s|$)/.exec(rest);
    if (m) {
      out.push({ t: m[1], c: 'p' }, { t: m[2], c: 'k' }, { t: m[3], c: 'p' });
      rest = rest.slice(m[0].length);
    } else {
      const dash = /^(\s*-\s?)/.exec(rest);
      if (dash) {
        out.push({ t: dash[1], c: 'p' });
        rest = rest.slice(dash[0].length);
      }
    }
    const hash = rest.search(/(^|\s)#/);
    let comment = '';
    if (hash >= 0 && !/^[^"']*["'][^"']*#/.test(rest.slice(0, hash + 1))) {
      comment = rest.slice(hash);
      rest = rest.slice(0, hash);
    }
    if (rest) {
      const v = rest.trim();
      const c = /^["']/.test(v) ? 's' : /^-?\d+(\.\d+)?$|^(true|false|null|~)$/.test(v) ? 'nm' : /^[&*!]/.test(v) ? 'a' : /^[|>][-+]?$/.test(v) ? 'p' : 'v';
      out.push({ t: rest, c });
    }
    if (comment) out.push({ t: comment, c: 'c' });
    return out;
  }
</script>

<div class="preview">
  <div class="bar">
    <span class="mono name">{rel}</span>
    <span class="grow"></span>
    {#if changed.size}<span class="chip changed">{changed.size} changed</span>{/if}
    <button type="button" class="btn small ghost" onclick={() => (whole = !whole)}>{whole ? `Only ${service}` : 'Whole file'}</button>
  </div>
  <div class="code" bind:this={box}>
    {#each shown as l (l.n)}
      <div class="line" class:changed={changed.has(l.n)} class:focus={focus.has(l.n)} class:outside={whole && (l.n < range[0] || l.n > range[1])} data-line={l.n}>
        <span class="ln">{l.n}</span><span class="t">{#each tokens(l.text) as tok, i (i)}<span class={tok.c}>{tok.t}</span>{/each}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .preview {
    display: flex;
    flex-direction: column;
    min-height: 0;
    max-height: 100%;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--sunken);
    overflow: hidden;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 6px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }
  .name {
    color: var(--text-2);
  }
  .chip.changed {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
    color: var(--accent-text);
  }
  .code {
    flex: 1;
    min-height: 0;
    padding: 6px 0;
    overflow: auto;
    font: 400 12.5px / 1.6 var(--font-code);
    --k: #82aaff;
    --s: #c3e88d;
    --nm: #f78c6c;
    --a: #ffcb6b;
  }
  :global([data-theme='light']) .code {
    --k: #1f5fbf;
    --s: #2e7d32;
    --nm: #c2410c;
    --a: #8a5a00;
  }
  .line {
    display: grid;
    grid-template-columns: 3em minmax(0, 1fr);
    border-left: 2px solid transparent;
    white-space: pre;
  }
  .line.focus {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
  }
  .line.changed {
    border-left-color: var(--accent);
  }
  .line.outside {
    opacity: 0.5;
  }
  .ln {
    padding-right: 10px;
    color: var(--text-3);
    text-align: right;
    user-select: none;
  }
  .t {
    padding-right: 12px;
  }
  .k {
    color: var(--k);
  }
  .s {
    color: var(--s);
  }
  .nm {
    color: var(--nm);
  }
  .a {
    color: var(--a);
  }
  .c {
    color: var(--text-3);
    font-style: italic;
  }
  .p {
    color: var(--text-2);
  }
</style>
