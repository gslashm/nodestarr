<!--
  Part-to-whole ring. Slices keep their order; a hairline in the panel color
  separates them. `active` highlights one slice (by key) and dims the rest,
  so several donuts can follow one hover. The middle is a snippet.
-->
<script lang="ts" module>
  export interface DonutSlice {
    key: string;
    value: number;
    /** CSS color or var(--token). */
    color: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    slices: DonutSlice[];
    /** What the ring stands for; defaults to the sum of the slices. */
    total?: number;
    active?: string | null;
    onhover?: (key: string | null) => void;
    size?: number;
    label: string;
    children?: Snippet;
  }

  let { slices, total, active = null, onhover, size = 148, label, children }: Props = $props();

  const R = 58;
  const INNER = 43;

  function point(r: number, a: number): string {
    return `${(r * Math.sin(a)).toFixed(3)} ${(-r * Math.cos(a)).toFixed(3)}`;
  }

  // Annular sector from angle a to b (radians, clockwise from the top).
  function sector(a: number, b: number, outer: number): string {
    if (b - a >= Math.PI * 2 - 1e-6) {
      // A full ring: two half arcs, since one arc can't end where it starts.
      return `M ${point(outer, 0)} A ${outer} ${outer} 0 1 1 ${point(outer, Math.PI)} A ${outer} ${outer} 0 1 1 ${point(outer, 0)} Z M ${point(INNER, 0)} A ${INNER} ${INNER} 0 1 0 ${point(INNER, Math.PI)} A ${INNER} ${INNER} 0 1 0 ${point(INNER, 0)} Z`;
    }
    const large = b - a > Math.PI ? 1 : 0;
    return `M ${point(outer, a)} A ${outer} ${outer} 0 ${large} 1 ${point(outer, b)} L ${point(INNER, b)} A ${INNER} ${INNER} 0 ${large} 0 ${point(INNER, a)} Z`;
  }

  const arcs = $derived.by(() => {
    const sum = total ?? slices.reduce((s, x) => s + x.value, 0);
    if (!(sum > 0)) return [];
    let a = 0;
    return slices
      .filter((s) => s.value > 0)
      .map((s) => {
        const b = a + (Math.min(s.value, sum) / sum) * Math.PI * 2;
        const arc = { ...s, a, b: Math.min(b, Math.PI * 2) };
        a = arc.b;
        return arc;
      });
  });
  const single = $derived(arcs.length === 1);
</script>

<div class="donut" style:--size="{size}px">
  <svg viewBox="-62 -62 124 124" role="img" aria-label={label} onpointerleave={() => onhover?.(null)}>
    <circle class="track" r={(R + INNER) / 2} stroke-width={R - INNER} />
    {#each arcs as s (s.key)}
      <!-- Hover is a pointer shortcut; the ring's label and the caller's legend carry the same information. -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <path
        d={sector(s.a, s.b, active === s.key ? R + 3 : R)}
        fill={s.color}
        class:gap={!single}
        class:dim={active !== null && active !== s.key}
        onpointerenter={() => onhover?.(s.key)}
      />
    {/each}
  </svg>
  {#if children}<div class="middle">{@render children()}</div>{/if}
</div>

<style>
  .donut {
    position: relative;
    width: min(var(--size), 100%);
    aspect-ratio: 1;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .track {
    fill: none;
    stroke: var(--sunken);
  }
  path {
    transition: opacity 150ms var(--ease);
  }
  path.gap {
    stroke: var(--panel);
    stroke-width: 1.6;
    stroke-linejoin: round;
  }
  path.dim {
    opacity: 0.3;
  }
  .middle {
    position: absolute;
    inset: 22%;
    display: grid;
    place-content: center;
    text-align: center;
    pointer-events: none;
  }
</style>
