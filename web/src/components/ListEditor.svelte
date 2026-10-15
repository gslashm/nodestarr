<!-- Edits a list of strings (folders, patterns, networks). -->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';

  let {
    items = $bindable([]),
    placeholder = '',
    mono = false,
    addLabel = 'Add',
  }: { items: string[]; placeholder?: string; mono?: boolean; addLabel?: string } = $props();

  let draft = $state('');

  function add() {
    const v = draft.trim();
    if (!v || items.includes(v)) return;
    items = [...items, v];
    draft = '';
  }
</script>

<div class="list">
  {#each items as item, i (i)}
    <div class="row">
      <input class="input" class:mono bind:value={items[i]} aria-label="Item {i + 1}" />
      <button type="button" class="btn ghost icon small" aria-label="Remove {item}" onclick={() => (items = items.filter((_, j) => j !== i))}>
        <X size={15} />
      </button>
    </div>
  {/each}
  <div class="row">
    <input
      class="input"
      class:mono
      {placeholder}
      bind:value={draft}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          add();
        }
      }}
    />
    <button type="button" class="btn small" onclick={add} disabled={!draft.trim()}><Plus size={15} />{addLabel}</button>
  </div>
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
</style>
