<!-- Horizontal usage bar; turns amber and red as it fills. -->
<script lang="ts">
  let {
    value,
    max = 100,
    warn = 80,
    bad = 92,
    label = '',
    height = 6,
  }: { value: number; max?: number; warn?: number; bad?: number; label?: string; height?: number } = $props();

  const pct = $derived(max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0);
  const tone = $derived(pct >= bad ? 'var(--bad)' : pct >= warn ? 'var(--warn)' : 'var(--ok)');
</script>

<div
  class="meter"
  role="meter"
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={Math.round(pct)}
  aria-label={label}
  style:--h="{height}px"
>
  <div class="fill" style:width="{pct}%" style:background={tone}></div>
</div>

<style>
  .meter {
    width: 100%;
    height: var(--h);
    border-radius: calc(var(--h) / 2);
    background: var(--sunken);
    border: 1px solid var(--line);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: inherit;
    transition: width 400ms var(--ease);
  }
</style>
