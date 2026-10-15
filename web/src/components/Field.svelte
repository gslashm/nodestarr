<!-- A form row: label, control, optional hint and error. -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    label,
    hint = '',
    error = '',
    id = undefined,
    children,
    wide = false,
  }: { label: string; hint?: string; error?: string; id?: string; children: Snippet; wide?: boolean } = $props();
</script>

<div class="field" class:wide>
  <label class="label" for={id}>{label}</label>
  <div class="control">
    {@render children()}
    {#if error}<div class="error" role="alert">{error}</div>{:else if hint}<div class="hint">{hint}</div>{/if}
  </div>
</div>

<style>
  .field {
    display: grid;
    grid-template-columns: minmax(160px, 240px) minmax(0, 1fr);
    gap: 6px 24px;
    align-items: start;
    padding: 12px 0;
  }
  .field.wide {
    grid-template-columns: 1fr;
  }
  .label {
    padding-top: 8px;
    font-weight: 500;
  }
  .control {
    display: grid;
    gap: 6px;
    min-width: 0;
    max-width: 640px;
  }
  .hint {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .error {
    color: var(--bad);
    font-size: var(--fs-sm);
  }
  @media (max-width: 760px) {
    .field {
      grid-template-columns: 1fr;
    }
    .label {
      padding-top: 0;
    }
  }
</style>
