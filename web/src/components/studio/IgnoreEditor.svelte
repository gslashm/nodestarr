<!--
  A .dockerignore file: patterns of files the build doesn't send to Docker
  (keeps builds fast and secrets like .env out of images). Comments and
  blank lines are kept where they are.
-->
<script lang="ts">
  import Code from '@lucide/svelte/icons/code';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import CodeEditor from '../CodeEditor.svelte';
  import { useStudio } from '../../lib/compose/studio.svelte';

  let { rel }: { rel: string } = $props();
  const studio = useStudio();

  let asCode = $state(false);
  let draft = $state('');

  const text = $derived(studio.text(rel));
  const lines = $derived(text === '' ? [] : text.replace(/\r\n/g, '\n').replace(/\n$/, '').split('\n'));
  const patterns = $derived(lines.map((l, i) => ({ line: i, text: l })).filter((l) => l.text.trim() && !l.text.trim().startsWith('#')));

  const PRESETS = ['.git', '.env', 'node_modules', '*.log', '__pycache__', '.venv', 'Dockerfile*', '.dockerignore', '**/*.md', 'tmp'];
  const missing = $derived(PRESETS.filter((p) => !patterns.some((x) => x.text.trim() === p)));

  function write(next: string[], what: string) {
    studio.setText(rel, next.length ? `${next.join('\n')}\n` : '', `ignore:${rel}:${what}`);
  }

  function setLine(i: number, value: string) {
    write(
      lines.map((l, j) => (j === i ? value : l)),
      `line${i}`,
    );
  }

  function removeLine(i: number) {
    write(
      lines.filter((_, j) => j !== i),
      'remove',
    );
  }

  function add(p = draft) {
    const t = p.trim();
    if (!t || patterns.some((x) => x.text.trim() === t)) return;
    write([...lines, t], 'add');
    draft = '';
  }
</script>

<div class="ignore">
  <div class="toolbar">
    <span class="muted small">Files the build leaves out. <code>!pattern</code> brings something back.</span>
    <span class="grow"></span>
    <button type="button" class="btn small ghost" onclick={() => (asCode = !asCode)}><Code size={15} />{asCode ? 'Edit as a list' : 'Edit as text'}</button>
  </div>
  {#if asCode}
    <CodeEditor bind:value={() => text, (v) => studio.setText(rel, v, `ignore:${rel}:code`)} language="shell" height="320px" label="Contents of {rel}" onsave={() => studio.requestSave()} />
  {:else}
    <div class="rows">
      {#each patterns as p (p.line)}
        <div class="row">
          <input class="input mono" value={p.text} spellcheck="false" aria-label="Pattern" oninput={(e) => setLine(p.line, e.currentTarget.value)} />
          <button type="button" class="btn ghost icon small" aria-label="Remove {p.text}" onclick={() => removeLine(p.line)}><X size={15} /></button>
        </div>
      {/each}
      <div class="row">
        <input
          class="input mono"
          placeholder="*.tmp"
          bind:value={draft}
          spellcheck="false"
          aria-label="New pattern"
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add();
            }
          }}
        />
        <button type="button" class="btn small" onclick={() => add()} disabled={!draft.trim()}><Plus size={15} />Add</button>
      </div>
    </div>
    {#if missing.length}
      <div class="presets">
        <span class="faint small">Common:</span>
        {#each missing as p (p)}<button type="button" class="chip preset" onclick={() => add(p)}>{p}</button>{/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .ignore {
    display: grid;
    gap: 10px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .rows {
    display: grid;
    gap: 6px;
    max-width: 560px;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .preset {
    cursor: pointer;
    font-family: var(--font-code);
  }
  .preset:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
</style>
