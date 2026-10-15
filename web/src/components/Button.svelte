<!--
  A button that shows a spinner while its async onclick runs (and ignores
  clicks meanwhile). Variants map to the global .btn classes.
-->
<script lang="ts">
  import type { Component, Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import Spinner from './Spinner.svelte';

  interface Props extends Omit<HTMLButtonAttributes, 'onclick'> {
    variant?: '' | 'primary' | 'ghost' | 'danger' | 'danger solid';
    small?: boolean;
    icon?: Component;
    /** Accessible label; required for icon-only buttons. */
    label?: string;
    busy?: boolean;
    onclick?: (e: MouseEvent) => unknown;
    children?: Snippet;
  }

  let {
    variant = '',
    small = false,
    icon: IconComponent,
    label,
    busy = false,
    disabled = false,
    type = 'button',
    onclick,
    children,
    ...rest
  }: Props = $props();

  let running = $state(false);
  const working = $derived(busy || running);

  async function handle(e: MouseEvent) {
    if (!onclick || working) return;
    const result = onclick(e);
    if (result instanceof Promise) {
      running = true;
      try {
        await result;
      } finally {
        running = false;
      }
    }
  }
</script>

<button
  {type}
  class="btn {variant}"
  class:small
  class:icon={!children}
  disabled={disabled || working}
  aria-label={label}
  title={rest.title ?? (!children ? label : undefined)}
  aria-busy={working}
  onclick={handle}
  {...rest}
>
  {#if working}
    <Spinner size={small ? 13 : 15} />
  {:else if IconComponent}
    <IconComponent size={small ? 15 : 17} aria-hidden="true" />
  {/if}
  {#if children}{@render children()}{/if}
</button>
