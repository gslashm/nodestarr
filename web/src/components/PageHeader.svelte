<!--
  The sticky page toolbar: title (with optional icon and back link) on the
  left, page actions on the right, optional tabs underneath.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import Menu from '@lucide/svelte/icons/menu';
  import { href } from '../lib/router.svelte';
  import { ui } from '../lib/ui.svelte';

  interface Props {
    title: string;
    subtitle?: string;
    back?: { label: string; path: string };
    lead?: Snippet;
    actions?: Snippet;
    tabs?: Snippet;
    meta?: Snippet;
  }

  let { title, subtitle = '', back, lead, actions, tabs, meta }: Props = $props();
</script>

<header class="page-header">
  <div class="bar">
    <button type="button" class="btn ghost icon nav-toggle" aria-label="Open navigation" onclick={() => (ui.navOpen = true)}>
      <Menu size={20} />
    </button>
    {#if back}
      <a class="back btn ghost icon" href={href(back.path)} aria-label="Back to {back.label}" title="Back to {back.label}">
        <ArrowLeft size={18} />
      </a>
    {/if}
    {#if lead}{@render lead()}{/if}
    <div class="titles">
      <h1>{title}</h1>
      {#if subtitle}<div class="subtitle">{subtitle}</div>{/if}
      {#if meta}<div class="subtitle">{@render meta()}</div>{/if}
    </div>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </div>
  {#if tabs}<div class="tabs-row">{@render tabs()}</div>{/if}
</header>

<style>
  .page-header {
    position: sticky;
    top: 0;
    z-index: 20;
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(10px);
    border-bottom: 1px solid var(--line);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--toolbar-h);
    padding: 8px 24px;
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  h1 {
    font-size: 21px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subtitle {
    margin-top: 1px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .tabs-row {
    padding: 0 18px;
  }
  .back {
    margin-left: -8px;
  }
  .nav-toggle {
    display: none;
    margin-left: -8px;
  }
  @media (max-width: 760px) {
    .bar {
      padding: 8px 14px;
      flex-wrap: wrap;
    }
    .nav-toggle {
      display: inline-flex;
    }
    .actions {
      width: 100%;
      justify-content: flex-start;
      order: 3;
    }
    .tabs-row {
      padding: 0 8px;
    }
    h1 {
      font-size: 19px;
    }
  }
</style>
