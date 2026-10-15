<script lang="ts">
  import { stateColor, STATE_LABEL, type StateKey } from '../lib/format';

  let { state, size = 9, pulse = false }: { state: StateKey; size?: number; pulse?: boolean } = $props();
</script>

<span
  class="dot"
  class:pulse={pulse || state === 'starting' || state === 'restarting'}
  style:--c={stateColor(state)}
  style:--s="{size}px"
  title={STATE_LABEL[state]}
  aria-hidden="true"
></span>

<style>
  .dot {
    display: inline-block;
    flex: none;
    width: var(--s);
    height: var(--s);
    border-radius: 50%;
    background: var(--c);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--c) 18%, transparent);
  }
  .pulse {
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
