<!--
  Edits KEY=value pairs (environment, labels, sysctls). Rows for small
  edits, or plain text to paste a whole .env file at once.
-->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import type { KeyValue } from '../lib/types';

  let {
    items = $bindable([]),
    keyLabel = 'Name',
    valueLabel = 'Value',
    keyPlaceholder = 'KEY',
    addLabel = 'Add',
    textMode = true,
  }: { items: KeyValue[]; keyLabel?: string; valueLabel?: string; keyPlaceholder?: string; addLabel?: string; textMode?: boolean } = $props();

  let asText = $state(false);
  let text = $state('');

  function toText(list: KeyValue[]): string {
    return list.map((kv) => `${kv.key}=${kv.value}`).join('\n');
  }

  function fromText(t: string): KeyValue[] {
    const out: KeyValue[] = [];
    for (const raw of t.split('\n')) {
      const line = raw.trim();
      if (!line || line.startsWith('#')) continue;
      const i = line.indexOf('=');
      const key = (i < 0 ? line : line.slice(0, i)).replace(/^export\s+/, '').trim();
      let value = i < 0 ? '' : line.slice(i + 1);
      if (value.length >= 2 && (value[0] === '"' || value[0] === "'") && value.endsWith(value[0])) value = value.slice(1, -1);
      if (key) out.push({ key, value });
    }
    return out;
  }

  function switchMode() {
    if (asText) items = fromText(text);
    else text = toText(items);
    asText = !asText;
  }
</script>

<div class="kv-editor">
  {#if asText}
    <textarea
      class="input mono"
      rows={Math.min(Math.max(items.length + 2, 5), 18)}
      bind:value={text}
      oninput={() => (items = fromText(text))}
      spellcheck="false"
      aria-label="{keyLabel}={valueLabel} lines"
      placeholder="KEY=value"
    ></textarea>
  {:else}
    {#if items.length}
      <div class="grid">
        <span class="head">{keyLabel}</span>
        <span class="head">{valueLabel}</span>
        <span></span>
        {#each items as kv, i (i)}
          <input class="input mono" bind:value={kv.key} placeholder={keyPlaceholder} aria-label="{keyLabel} {i + 1}" spellcheck="false" />
          <input class="input mono" bind:value={kv.value} aria-label="{valueLabel} {i + 1}" spellcheck="false" />
          <button type="button" class="btn ghost icon small" aria-label="Remove {kv.key || 'row'}" onclick={() => (items = items.filter((_, j) => j !== i))}>
            <X size={15} />
          </button>
        {/each}
      </div>
    {/if}
  {/if}
  <div class="actions">
    {#if !asText}
      <button type="button" class="btn small" onclick={() => (items = [...items, { key: '', value: '' }])}><Plus size={15} />{addLabel}</button>
    {/if}
    {#if textMode}
      <button type="button" class="btn small ghost" onclick={switchMode}>{asText ? 'Edit as rows' : 'Edit as text'}</button>
    {/if}
  </div>
</div>

<style>
  .kv-editor {
    display: grid;
    gap: 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) auto;
    gap: 6px;
    align-items: center;
  }
  .head {
    color: var(--text-3);
    font-size: var(--fs-xs);
  }
  .actions {
    display: flex;
    gap: 6px;
  }
</style>
