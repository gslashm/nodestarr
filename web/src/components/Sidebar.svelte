<!--
  Left navigation: brand, sections (with counts that need attention), the
  task tray and the account menu. Becomes a drawer on narrow screens.
-->
<script lang="ts">
  import Activity from '@lucide/svelte/icons/activity';
  import Archive from '@lucide/svelte/icons/archive';
  import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
  import Container from '@lucide/svelte/icons/container';
  import Cpu from '@lucide/svelte/icons/cpu';
  import History from '@lucide/svelte/icons/history';
  import Layers from '@lucide/svelte/icons/layers';
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import LogOut from '@lucide/svelte/icons/log-out';
  import Monitor from '@lucide/svelte/icons/monitor';
  import Moon from '@lucide/svelte/icons/moon';
  import Search from '@lucide/svelte/icons/search';
  import Server from '@lucide/svelte/icons/server';
  import Settings from '@lucide/svelte/icons/settings';
  import Sun from '@lucide/svelte/icons/sun';
  import User from '@lucide/svelte/icons/user';
  import { concerns } from '../lib/attention';
  import { hasUpdate } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href, router } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import { applyTheme, storeTheme, storedTheme, type ThemeChoice } from '../lib/theme';
  import { ui } from '../lib/ui.svelte';
  import Logo from './Logo.svelte';
  import Menu, { type MenuEntry } from './Menu.svelte';
  import TaskTray from './TaskTray.svelte';

  const updates = $derived(live.list.filter((c) => hasUpdate(c.update) && c.policy !== 'ignore').length);
  const failing = $derived(live.list.filter((c) => concerns(c).some((x) => !x.hidden && (x.kind === 'crash' || x.kind === 'health'))).length);
  const alerts = $derived(live.alerts.length);
  const proxmox = $derived(!!session.settings?.proxmox.enabled);

  const nav = $derived([
    { path: '/', label: 'Dashboard', icon: LayoutGrid },
    { path: '/containers', label: 'Containers', icon: Container, badge: failing, tone: 'bad' },
    { path: '/stacks', label: 'Stacks', icon: Layers },
    { path: '/updates', label: 'Updates', icon: CircleArrowUp, badge: updates, tone: 'update' },
    { path: '/backups', label: 'Backups', icon: Archive },
    { path: '/monitoring', label: 'Monitoring', icon: Activity, badge: alerts, tone: 'bad' },
    { path: '/activity', label: 'Activity', icon: History },
    ...(proxmox ? [{ path: '/proxmox', label: 'Proxmox', icon: Server }] : []),
    { path: '/settings', label: 'Settings', icon: Settings },
    { path: '/system', label: 'System', icon: Cpu },
  ]);

  function isActive(path: string): boolean {
    return path === '/' ? router.path === '/' : router.path === path || router.path.startsWith(`${path}/`);
  }

  let theme = $state<ThemeChoice>(storedTheme() ?? ((session.settings?.general.ui.theme as ThemeChoice) || 'dark'));

  function setTheme(t: ThemeChoice) {
    theme = t;
    storeTheme(t);
    applyTheme(t);
  }

  const accountItems = $derived<MenuEntry[]>([
    { label: 'Dark theme', icon: Moon, hint: theme === 'dark' ? 'On' : '', run: () => setTheme('dark') },
    { label: 'Light theme', icon: Sun, hint: theme === 'light' ? 'On' : '', run: () => setTheme('light') },
    { label: 'Match system', icon: Monitor, hint: theme === 'system' ? 'On' : '', run: () => setTheme('system') },
    'separator',
    { label: 'Account and security', icon: User, href: href('/settings/account') },
    ...(session.me?.via === 'session' ? [{ label: 'Sign out', icon: LogOut, run: () => session.logout() }] : []),
  ]);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" class:open={ui.navOpen} onclick={() => (ui.navOpen = false)}></div>

<aside class="sidebar" class:open={ui.navOpen} aria-label="Main navigation">
  <a class="brand" href={href('/')}>
    <Logo size={30} />
    <span class="word">node<span class="starr">starr</span></span>
    {#if session.settings?.general.instance_name && session.settings.general.instance_name !== 'Nodestarr'}
      <span class="instance ellipsis">{session.settings.general.instance_name}</span>
    {/if}
  </a>

  <button type="button" class="search" onclick={() => (ui.paletteOpen = true)}>
    <Search size={15} />
    <span>Find anything</span>
    <kbd>/</kbd>
  </button>

  <nav>
    {#each nav as item (item.path)}
      <a
        href={href(item.path)}
        class:active={isActive(item.path)}
        aria-current={isActive(item.path) ? 'page' : undefined}
        onclick={() => (ui.navOpen = false)}
      >
        <item.icon size={18} aria-hidden="true" />
        <span class="label">{item.label}</span>
        {#if item.badge}
          <span class="badge {item.tone}" title="{item.badge} need attention">{item.badge}</span>
        {/if}
      </a>
    {/each}
  </nav>

  <div class="foot">
    <TaskTray />
    <div class="account">
      <span class="conn" class:on={live.connected} title={live.connected ? 'Live updates connected' : 'Reconnecting to Nodestarr'}></span>
      <span class="user ellipsis">{session.me?.username ?? ''}</span>
      <span class="ver faint num">v{session.status?.version}</span>
      <Menu items={accountItems} label="Account" small />
    </div>
  </div>
</aside>

<style>
  .sidebar {
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    width: var(--sidebar-w);
    height: 100vh;
    height: 100dvh;
    padding: 14px 10px 10px;
    border-right: 1px solid var(--line);
    background: var(--panel);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px 14px;
    color: var(--text);
    text-decoration: none;
  }
  .word {
    font: 700 25px / 1 var(--font-data);
    letter-spacing: -0.01em;
  }
  .starr {
    color: var(--accent-text);
  }
  .instance {
    margin-left: auto;
    max-width: 80px;
    color: var(--text-3);
    font-size: var(--fs-xs);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    margin: 0 2px 12px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    color: var(--text-3);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .search span {
    flex: 1;
    text-align: left;
  }
  .search:hover {
    border-color: var(--line-strong);
    color: var(--text-2);
  }
  kbd {
    padding: 1px 6px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-sm);
    font-size: 11px;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }
  nav a {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 10px;
    border-radius: var(--radius);
    color: var(--text-2);
    font-weight: 500;
    text-decoration: none;
  }
  nav a:hover {
    background: var(--raised);
    color: var(--text);
  }
  nav a.active {
    background: var(--raised);
    color: var(--text);
  }
  nav a.active::before {
    content: '';
    position: absolute;
    left: 0;
    top: 9px;
    bottom: 9px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
  }
  nav a.active :global(svg) {
    color: var(--accent-text);
  }
  .label {
    flex: 1;
  }
  .badge {
    min-width: 20px;
    height: 19px;
    padding: 0 6px;
    border-radius: 10px;
    font: 600 var(--fs-xs) / 19px var(--font-data);
    text-align: center;
  }
  .badge.update {
    background: color-mix(in srgb, var(--update) 20%, transparent);
    color: var(--update);
  }
  .badge.bad {
    background: color-mix(in srgb, var(--bad) 20%, transparent);
    color: var(--bad);
  }
  .foot {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: auto;
    padding-top: 10px;
  }
  .account {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 2px 0 10px;
    border-top: 1px solid var(--line);
    padding-top: 10px;
  }
  .conn {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  .conn.on {
    background: var(--ok);
  }
  .user {
    flex: 1;
    font-weight: 500;
  }
  .ver {
    font-size: var(--fs-xs);
  }
  .scrim {
    display: none;
  }
  @media (max-width: 760px) {
    .sidebar {
      position: fixed;
      inset: 0 auto 0 0;
      z-index: 50;
      width: min(290px, 86vw);
      transform: translateX(-102%);
      transition: transform 200ms var(--ease);
      box-shadow: var(--shadow);
    }
    .sidebar.open {
      transform: none;
    }
    .scrim.open {
      display: block;
      position: fixed;
      inset: 0;
      z-index: 49;
      background: var(--overlay);
    }
  }
</style>
