<script lang="ts">
  import { stateColor, STATE_LABEL, type StateKey } from '../lib/format';
  import StateDot from './StateDot.svelte';

  let { state, detail = '' }: { state: StateKey; detail?: string } = $props();
</script>

<span class="pill" style:--c={stateColor(state)}>
  <StateDot {state} size={7} />
  <span>{STATE_LABEL[state]}</span>
  {#if detail}<span class="detail">{detail}</span>{/if}
</span>

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 24px;
    padding: 0 10px 0 9px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--c) 12%, transparent);
    color: color-mix(in srgb, var(--c) 72%, var(--text));
    font: 600 var(--fs-sm) / 1 var(--font-data);
    white-space: nowrap;
  }
  .detail {
    color: var(--text-2);
    font-weight: 500;
  }
</style>
