<!--
  Dropdown menu. Items run their action and close the menu. Keyboard:
  Enter/Space opens, arrows move, Escape closes.
-->
<script lang="ts" module>
  import type { Component } from 'svelte';

  export interface MenuItem {
    label: string;
    icon?: Component;
    danger?: boolean;
    disabled?: boolean;
    hint?: string;
    href?: string;
    run?: () => unknown;
  }
  export type MenuEntry = MenuItem | 'separator';
</script>

<script lang="ts">
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import type { Snippet } from 'svelte';

  interface Props {
    items: MenuEntry[];
    label?: string;
    align?: 'left' | 'right';
    small?: boolean;
    trigger?: Snippet;
  }

  let { items, label = 'More actions', align = 'right', small = false, trigger }: Props = $props();

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();
  let up = $state(false);

  function toggle() {
    open = !open;
    if (open) {
      requestAnimationFrame(() => {
        if (!root || !list) return;
        const r = root.getBoundingClientRect();
        up = r.bottom + list.offsetHeight + 12 > window.innerHeight && r.top > list.offsetHeight + 12;
        list.querySelector<HTMLElement>('[role=menuitem]:not([aria-disabled=true])')?.focus();
      });
    }
  }

  function close() {
    open = false;
  }

  function onKey(e: KeyboardEvent) {
    if (!open) return;
    const els = [...(list?.querySelectorAll<HTMLElement>('[role=menuitem]:not([aria-disabled=true])') ?? [])];
    const i = els.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') {
      close();
      root?.querySelector<HTMLElement>('button')?.focus();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      els[(i + 1) % els.length]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      els[(i - 1 + els.length) % els.length]?.focus();
    }
  }

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) close();
  }

  async function choose(item: MenuItem) {
    if (item.disabled) return;
    close();
    await item.run?.();
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

<div class="menu" bind:this={root}>
  {#if trigger}
    <button type="button" class="trigger-custom" aria-haspopup="menu" aria-expanded={open} onclick={toggle}>{@render trigger()}</button>
  {:else}
    <button type="button" class="btn ghost icon" class:small aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open} onclick={toggle}>
      <EllipsisVertical size={small ? 16 : 18} />
    </button>
  {/if}
  {#if open}
    <div class="list" class:left={align === 'left'} class:up role="menu" bind:this={list}>
      {#each items as item, i (i)}
        {#if item === 'separator'}
          <div class="sep" role="separator"></div>
        {:else if item.href && !item.disabled}
          <a role="menuitem" class="item" class:danger={item.danger} href={item.href} onclick={close} tabindex="-1">
            {#if item.icon}<item.icon size={16} aria-hidden="true" />{/if}
            <span>{item.label}</span>
            {#if item.hint}<span class="hint">{item.hint}</span>{/if}
          </a>
        {:else}
          <button type="button"
            role="menuitem"
            class="item"
            class:danger={item.danger}
            aria-disabled={item.disabled}
            tabindex="-1"
            onclick={() => choose(item)}
          >
            {#if item.icon}<item.icon size={16} aria-hidden="true" />{/if}
            <span>{item.label}</span>
            {#if item.hint}<span class="hint">{item.hint}</span>{/if}
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu {
    position: relative;
    display: inline-flex;
  }
  .trigger-custom {
    all: unset;
    cursor: pointer;
  }
  .trigger-custom:focus-visible {
    outline: 2px solid var(--focus);
    border-radius: var(--radius);
  }
  .list {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 40;
    min-width: 210px;
    padding: 5px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .list.left {
    right: auto;
    left: 0;
  }
  .list.up {
    top: auto;
    bottom: calc(100% + 6px);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    text-decoration: none;
    cursor: pointer;
  }
  .item :global(svg) {
    color: var(--text-2);
  }
  .item:hover,
  .item:focus-visible {
    outline: none;
    background: var(--raised);
    text-decoration: none;
  }
  .item.danger,
  .item.danger :global(svg) {
    color: var(--bad);
  }
  .item[aria-disabled='true'] {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .hint {
    margin-left: auto;
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .sep {
    height: 1px;
    margin: 5px 4px;
    background: var(--line);
  }
</style>
