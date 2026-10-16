<!--
  A list of strings (capabilities, DNS servers, profiles...). Reports the
  whole new list on every change instead of mutating it, so it can sit on
  top of the stack editor's text.
-->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';

  let {
    items,
    onchange,
    placeholder = '',
    mono = true,
    addLabel = 'Add',
    suggestions = [],
    label = 'Item',
  }: { items: string[]; onchange: (items: string[]) => void; placeholder?: string; mono?: boolean; addLabel?: string; suggestions?: string[]; label?: string } = $props();

  const uid = $props.id();
  let draft = $state('');
  const listId = `sl-${uid}`;
  const offer = $derived(suggestions.filter((s) => !items.includes(s)));

  function add(v = draft) {
    const t = v.trim();
    if (!t || items.includes(t)) return;
    onchange([...items, t]);
    draft = '';
  }
</script>

<div class="list">
  {#each items as item, i (i)}
    <div class="row">
      <input class="input" class:mono value={item} aria-label="{label} {i + 1}" spellcheck="false" oninput={(e) => onchange(items.map((x, j) => (j === i ? e.currentTarget.value : x)))} />
      <button type="button" class="btn ghost icon small" aria-label="Remove {item}" onclick={() => onchange(items.filter((_, j) => j !== i))}><X size={15} /></button>
    </div>
  {/each}
  <div class="row">
    <input
      class="input"
      class:mono
      {placeholder}
      bind:value={draft}
      list={offer.length ? listId : undefined}
      aria-label="New {label.toLowerCase()}"
      spellcheck="false"
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          add();
        }
      }}
    />
    <button type="button" class="btn small" onclick={() => add()} disabled={!draft.trim()}><Plus size={15} />{addLabel}</button>
  </div>
  {#if offer.length}<datalist id={listId}>{#each offer as s (s)}<option value={s}></option>{/each}</datalist>{/if}
</div>

<style>
  .list {
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
</style>
