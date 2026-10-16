<!--
  Key/value pairs (labels, sysctls, build arguments, log options). Each
  change reports the whole new list.
-->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';

  interface Entry {
    key: string;
    value: string | null;
  }

  let {
    entries,
    onchange,
    keyPlaceholder = 'name',
    valuePlaceholder = 'value',
    addLabel = 'Add',
  }: { entries: Entry[]; onchange: (entries: Entry[]) => void; keyPlaceholder?: string; valuePlaceholder?: string; addLabel?: string } = $props();

  let key = $state('');
  let value = $state('');

  function add(e: Event) {
    e.preventDefault();
    const k = key.trim();
    if (!k || entries.some((x) => x.key === k)) return;
    onchange([...entries, { key: k, value }]);
    key = '';
    value = '';
  }

  const update = (i: number, patch: Partial<Entry>) => onchange(entries.map((x, j) => (j === i ? { ...x, ...patch } : x)));
</script>

<div class="kv">
  {#each entries as e, i (i)}
    <div class="row">
      <input class="input mono" value={e.key} aria-label="Name {i + 1}" spellcheck="false" onchange={(ev) => update(i, { key: ev.currentTarget.value.trim() })} />
      <input class="input mono" value={e.value ?? ''} aria-label="Value of {e.key}" spellcheck="false" oninput={(ev) => update(i, { value: ev.currentTarget.value })} />
      <button type="button" class="btn ghost icon small" aria-label="Remove {e.key}" onclick={() => onchange(entries.filter((_, j) => j !== i))}><X size={15} /></button>
    </div>
  {/each}
  <form class="row" onsubmit={add}>
    <input class="input mono" placeholder={keyPlaceholder} bind:value={key} aria-label="New name" spellcheck="false" />
    <input class="input mono" placeholder={valuePlaceholder} bind:value={value} aria-label="New value" spellcheck="false" />
    <button type="submit" class="btn small" disabled={!key.trim()}><Plus size={15} />{addLabel}</button>
  </form>
</div>

<style>
  .kv {
    display: grid;
    gap: 6px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr) auto;
    gap: 6px;
    align-items: center;
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
</style>
