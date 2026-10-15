<script lang="ts">
  import type { Component, Snippet } from 'svelte';

  let {
    icon: Icon,
    title,
    children,
    actions,
  }: { icon?: Component; title: string; children?: Snippet; actions?: Snippet } = $props();
</script>

<div class="empty">
  {#if Icon}<span class="icon"><Icon size={26} aria-hidden="true" /></span>{/if}
  <h3>{title}</h3>
  {#if children}<div class="text">{@render children()}</div>{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 48px 20px;
    text-align: center;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    margin-bottom: 6px;
    border-radius: 14px;
    background: var(--raised);
    color: var(--text-2);
  }
  h3 {
    font-size: var(--fs-lg);
  }
  .text {
    max-width: 460px;
    color: var(--text-2);
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
</style>
