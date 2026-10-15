<!--
  Quick finder: jump to any container, stack or page, or run a common
  action. Opens with "/" or Ctrl/Cmd+K.
-->
<script lang="ts">
  import type { Component } from 'svelte';
  import Activity from '@lucide/svelte/icons/activity';
  import Archive from '@lucide/svelte/icons/archive';
  import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
  import Cpu from '@lucide/svelte/icons/cpu';
  import History from '@lucide/svelte/icons/history';
  import Layers from '@lucide/svelte/icons/layers';
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import Plus from '@lucide/svelte/icons/plus';
  import Search from '@lucide/svelte/icons/search';
  import Settings from '@lucide/svelte/icons/settings';
  import { api } from '../lib/api';
  import { stateOf } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { router } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import { ui } from '../lib/ui.svelte';
  import AppIcon from './AppIcon.svelte';
  import StateDot from './StateDot.svelte';

  interface Result {
    key: string;
    label: string;
    detail?: string;
    icon?: Component;
    container?: (typeof live.list)[number];
    run: () => void;
  }

  let query = $state('');
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let dialog: HTMLDialogElement | undefined = $state();

  const pages: Result[] = [
    { key: 'p:/', label: 'Dashboard', icon: LayoutGrid, run: () => router.navigate('/') },
    { key: 'p:/containers', label: 'Containers', icon: LayoutGrid, run: () => router.navigate('/containers') },
    { key: 'p:/stacks', label: 'Stacks', icon: Layers, run: () => router.navigate('/stacks') },
    { key: 'p:/updates', label: 'Updates', icon: CircleArrowUp, run: () => router.navigate('/updates') },
    { key: 'p:/backups', label: 'Backups', icon: Archive, run: () => router.navigate('/backups') },
    { key: 'p:/monitoring', label: 'Monitoring', icon: Activity, run: () => router.navigate('/monitoring') },
    { key: 'p:/activity', label: 'Activity', icon: History, run: () => router.navigate('/activity') },
    { key: 'p:/settings', label: 'Settings', icon: Settings, run: () => router.navigate('/settings') },
    { key: 'p:/system', label: 'System', icon: Cpu, run: () => router.navigate('/system') },
    { key: 'p:/system/docker', label: 'Images, volumes and networks', icon: Cpu, run: () => router.navigate('/system/docker') },
    { key: 'p:/settings/notifications', label: 'Notification settings', icon: Settings, run: () => router.navigate('/settings/notifications') },
  ];

  const actions: Result[] = $derived(
    session.isAdmin
      ? [
          { key: 'a:new-container', label: 'Add a container', icon: Plus, run: () => router.navigate('/containers/new') },
          { key: 'a:new-stack', label: 'Create a stack', icon: Plus, run: () => router.navigate('/stacks/new') },
          {
            key: 'a:check',
            label: 'Check all containers for updates',
            icon: CircleArrowUp,
            run: () =>
              api
                .post('/updates/check')
                .then(() => ui.toast('Checking for updates…'))
                .catch((e) => ui.error(e)),
          },
        ]
      : [],
  );

  function score(text: string, q: string): number {
    const t = text.toLowerCase();
    if (t.startsWith(q)) return 3;
    if (t.split(/[\s/_-]+/).some((w) => w.startsWith(q))) return 2;
    return t.includes(q) ? 1 : 0;
  }

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const containers: Result[] = live.list.map((c) => ({
      key: `c:${c.id}`,
      label: c.display_name,
      detail: [c.stack, c.app_name && c.app_name !== c.display_name ? c.app_name : '', c.version].filter(Boolean).join(', '),
      container: c,
      run: () => router.navigate(`/containers/${encodeURIComponent(c.name)}`),
    }));
    const stacks: Result[] = [...new Set(live.list.map((c) => c.stack).filter((s): s is string => !!s))].map((s) => ({
      key: `s:${s}`,
      label: s,
      detail: 'Stack',
      icon: Layers,
      run: () => router.navigate(`/stacks/${encodeURIComponent(s)}`),
    }));
    const all = [...containers, ...stacks, ...pages, ...actions];
    if (!q) return [...containers.slice(0, 8), ...pages.slice(0, 6)];
    return all
      .map((r) => ({ r, s: Math.max(score(r.label, q), score(r.detail ?? '', q) - 1, r.container ? score(r.container.name, q) : 0) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 14)
      .map((x) => x.r);
  });

  $effect(() => {
    void results;
    active = 0;
  });

  $effect(() => {
    if (!dialog) return;
    if (ui.paletteOpen && !dialog.open) {
      query = '';
      dialog.showModal();
      requestAnimationFrame(() => input?.focus());
    } else if (!ui.paletteOpen && dialog.open) dialog.close();
  });

  function choose(r: Result | undefined) {
    if (!r) return;
    ui.paletteOpen = false;
    r.run();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, results.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      choose(results[active]);
    }
  }

  function onGlobalKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const typing = target.closest('input, textarea, select, [contenteditable], .cm-editor, .xterm');
    if ((e.key === 'k' && (e.metaKey || e.ctrlKey)) || (e.key === '/' && !typing && !e.metaKey && !e.ctrlKey)) {
      e.preventDefault();
      ui.paletteOpen = true;
    }
  }
</script>

<svelte:window onkeydown={onGlobalKey} />

<dialog
  bind:this={dialog}
  class="palette"
  aria-label="Find"
  oncancel={(e) => {
    e.preventDefault();
    ui.paletteOpen = false;
  }}
  onclick={(e) => {
    if (e.target === dialog) ui.paletteOpen = false;
  }}
>
  <div class="box">
    <div class="search">
      <Search size={18} />
      <input
        bind:this={input}
        bind:value={query}
        onkeydown={onKey}
        placeholder="Find a container, stack or page"
        aria-label="Search"
        aria-controls="palette-results"
        autocomplete="off"
        spellcheck="false"
      />
      <kbd>Esc</kbd>
    </div>
    <ul id="palette-results" role="listbox">
      {#each results as r, i (r.key)}
        <li role="option" aria-selected={i === active}>
          <button type="button" class:active={i === active} onmouseenter={() => (active = i)} onclick={() => choose(r)}>
            {#if r.container}
              <AppIcon name={r.container.display_name} slug={r.container.app} icon={r.container.prefs.icon} size={22} />
            {:else if r.icon}
              <span class="ico"><r.icon size={16} /></span>
            {/if}
            <span class="label">{r.label}</span>
            {#if r.detail}<span class="detail ellipsis">{r.detail}</span>{/if}
            {#if r.container}<StateDot state={stateOf(r.container)} size={7} />{/if}
          </button>
        </li>
      {:else}
        <li class="none">Nothing matches “{query}”.</li>
      {/each}
    </ul>
  </div>
</dialog>

<style>
  .palette {
    width: min(620px, calc(100vw - 24px));
    margin-top: 12vh;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--panel);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  .palette::backdrop {
    background: var(--overlay);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
  }
  .search input {
    flex: 1;
    border: 0;
    background: none;
    color: var(--text);
    font: 400 17px / 1.4 var(--font-ui);
  }
  .search input:focus {
    outline: none;
  }
  kbd {
    padding: 1px 6px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-sm);
    font-size: 11px;
  }
  ul {
    max-height: 420px;
    margin: 0;
    padding: 6px;
    overflow-y: auto;
    list-style: none;
  }
  li button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  li button.active {
    background: var(--raised);
  }
  .ico {
    display: grid;
    place-items: center;
    width: 22px;
    color: var(--text-2);
  }
  .label {
    font-weight: 500;
  }
  .detail {
    flex: 1;
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .none {
    padding: 14px;
    color: var(--text-3);
  }
</style>
