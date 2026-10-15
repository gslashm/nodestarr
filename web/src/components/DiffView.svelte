<!--
  Shows a diff: either a unified diff from the server (`unified`) or one
  computed here from two texts (`before` and `after`).
-->
<script lang="ts">
  import { diffLines, diffRows, parseUnified } from '../lib/diff';

  let {
    unified,
    before,
    after,
    context = 3,
    empty = 'No changes.',
  }: { unified?: string; before?: string; after?: string; context?: number; empty?: string } = $props();

  const rows = $derived(unified === undefined ? diffRows(diffLines(before ?? '', after ?? ''), context) : parseUnified(unified));
  const stats = $derived({
    added: rows.filter((r) => r.kind === 'add').length,
    removed: rows.filter((r) => r.kind === 'del').length,
  });
</script>

{#if !rows.some((r) => r.kind === 'add' || r.kind === 'del')}
  <p class="none">{empty}</p>
{:else}
  <div class="stats">
    <span class="text-ok">+{stats.added}</span>
    <span class="text-bad">−{stats.removed}</span>
  </div>
  <div class="diff-view" role="table" aria-label="Changes">
    {#each rows as r, i (i)}
      <div class="row {r.kind}" role="row">
        <span class="ln" role="cell">{r.old ?? ''}</span>
        <span class="ln" role="cell">{r.new ?? ''}</span>
        <span class="sign" role="cell">{r.kind === 'add' ? '+' : r.kind === 'del' ? '−' : ''}</span>
        <span class="text" role="cell">{r.text}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .none {
    margin: 0;
    color: var(--text-2);
  }
  .stats {
    display: flex;
    gap: 10px;
    margin-bottom: 6px;
    font: 600 var(--fs-sm) / 1 var(--font-data);
  }
  .diff-view {
    overflow: auto;
    max-height: 60vh;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font: 400 12.5px / 1.55 var(--font-code);
  }
  .row {
    display: grid;
    grid-template-columns: 3.2em 3.2em 1.4em minmax(0, 1fr);
    min-width: max-content;
  }
  .ln {
    padding-right: 8px;
    color: var(--text-3);
    text-align: right;
    user-select: none;
  }
  .sign {
    color: var(--text-3);
    user-select: none;
  }
  .text {
    white-space: pre;
    padding-right: 12px;
  }
  .add {
    background: color-mix(in srgb, var(--ok) 13%, transparent);
  }
  .add .sign,
  .add .text {
    color: color-mix(in srgb, var(--ok) 70%, var(--text));
  }
  .del {
    background: color-mix(in srgb, var(--bad) 13%, transparent);
  }
  .del .sign,
  .del .text {
    color: color-mix(in srgb, var(--bad) 70%, var(--text));
  }
  .hunk {
    background: var(--raised);
    color: var(--text-3);
  }
  .hunk .text {
    padding: 2px 0;
  }
</style>
