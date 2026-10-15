<!-- A tiny trend line (SVG). Values are scaled to [min(0), max]. -->
<script lang="ts">
  let {
    values,
    width = 80,
    height = 22,
    max = undefined,
    color = 'var(--info)',
    fill = true,
    label = 'Trend',
    stretch = false,
  }: { values: number[]; width?: number; height?: number; max?: number; color?: string; fill?: boolean; label?: string; stretch?: boolean } = $props();

  const points = $derived.by(() => {
    if (values.length < 2) return '';
    const top = Math.max(max ?? 0, ...values, 1e-9);
    const step = width / (values.length - 1);
    return values
      .map((v, i) => `${(i * step).toFixed(1)},${(height - 1.5 - (Math.max(0, v) / top) * (height - 3)).toFixed(1)}`)
      .join(' ');
  });
</script>

<svg
  width={stretch ? '100%' : width}
  {height}
  viewBox="0 0 {width} {height}"
  preserveAspectRatio={stretch ? 'none' : undefined}
  role="img"
  aria-label={label}
  style:--c={color}
>
  {#if points}
    {#if fill}<polygon points="0,{height} {points} {width},{height}" class="area" />{/if}
    <polyline {points} class="line" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    overflow: visible;
  }
  .line {
    fill: none;
    stroke: var(--c);
    stroke-width: 1.5;
    stroke-linejoin: round;
    stroke-linecap: round;
    vector-effect: non-scaling-stroke;
  }
  .area {
    fill: color-mix(in srgb, var(--c) 16%, transparent);
    stroke: none;
  }
</style>
