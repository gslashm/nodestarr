<!-- A small set of mutually exclusive choices (ranges, views). -->
<script lang="ts" generics="T extends string">
  let {
    options,
    value = $bindable(),
    label,
    onchange,
  }: { options: { value: T; label: string }[]; value: T; label: string; onchange?: (v: T) => void } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <button type="button"
      role="radio"
      aria-checked={o.value === value}
      class:active={o.value === value}
      onclick={() => {
        value = o.value;
        onchange?.(o.value);
      }}>{o.label}</button
    >
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    /* Keep its natural width inside grid and flex layouts. */
    justify-self: start;
    align-self: start;
    max-width: 100%;
    overflow-x: auto;
    padding: 2px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--sunken);
  }
  button {
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-2);
    font: 500 var(--fs-sm) / 1 var(--font-data);
    white-space: nowrap;
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
  }
  button.active {
    background: var(--raised);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--line-strong);
  }
</style>
