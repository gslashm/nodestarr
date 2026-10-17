<!-- A disk's or test's state as a small colored label (the color always comes with words). -->
<script lang="ts">
  import type { Tone } from '../../../lib/disks';

  let { tone, label, title = '' }: { tone: Tone | ''; label: string; title?: string } = $props();
</script>

<span class="badge {tone}" title={title || undefined}><span class="dot" aria-hidden="true"></span>{label}</span>

<style>
  .badge {
    --tone: var(--text-3);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--tone);
    font: 600 var(--fs-sm) / 1.2 var(--font-data);
    white-space: nowrap;
  }
  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--tone);
  }
  .ok {
    --tone: var(--ok);
  }
  .info {
    --tone: var(--info);
  }
  .warn {
    --tone: var(--warn);
  }
  .bad {
    --tone: var(--bad);
  }
  .idle {
    --tone: var(--idle);
  }
</style>
