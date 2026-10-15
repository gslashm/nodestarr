<!-- Page tabs rendered as links, so each tab has its own URL. -->
<script lang="ts">
  import { href } from '../lib/router.svelte';

  interface Tab {
    key: string;
    label: string;
    path: string;
    badge?: string | number | null;
    tone?: 'update' | 'bad' | 'warn' | '';
  }

  let { tabs, active }: { tabs: Tab[]; active: string } = $props();
</script>

<nav class="tabs" aria-label="Sections">
  {#each tabs as t (t.key)}
    <a href={href(t.path)} class:active={t.key === active} aria-current={t.key === active ? 'page' : undefined}>
      {t.label}
      {#if t.badge !== undefined && t.badge !== null && t.badge !== '' && t.badge !== 0}
        <span class="badge {t.tone ?? ''}">{t.badge}</span>
      {/if}
    </a>
  {/each}
</nav>

<style>
  .tabs {
    display: flex;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  a {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 42px;
    padding: 0 12px;
    color: var(--text-2);
    font-weight: 500;
    white-space: nowrap;
    text-decoration: none;
  }
  a:hover {
    color: var(--text);
    text-decoration: none;
  }
  a.active {
    color: var(--text);
  }
  a.active::after {
    content: '';
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: -1px;
    height: 2px;
    border-radius: 2px;
    background: var(--accent);
  }
  .badge {
    min-width: 20px;
    height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--raised);
    color: var(--text-2);
    font: 600 var(--fs-xs) / 18px var(--font-data);
    text-align: center;
  }
  .badge.update {
    background: color-mix(in srgb, var(--update) 18%, transparent);
    color: var(--update);
  }
  .badge.bad {
    background: color-mix(in srgb, var(--bad) 18%, transparent);
    color: var(--bad);
  }
  .badge.warn {
    background: color-mix(in srgb, var(--warn) 18%, transparent);
    color: var(--warn);
  }
</style>
