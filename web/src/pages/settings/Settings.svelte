<!-- Settings: a section list on the left, the chosen section on the right. -->
<script lang="ts">
  import type { Component } from 'svelte';
  import PageHeader from '../../components/PageHeader.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { href, router } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';

  let { params }: { params: Record<string, string> } = $props();

  interface Section {
    key: string;
    label: string;
    load: () => Promise<{ default: Component }>;
    admin?: boolean;
    group: string;
  }

  const SECTIONS: Section[] = [
    { key: 'general', label: 'General', group: 'Berth', load: () => import('./General.svelte') },
    { key: 'account', label: 'Your account', group: 'Berth', load: () => import('./Account.svelte') },
    { key: 'users', label: 'Users', group: 'Berth', admin: true, load: () => import('./Users.svelte') },
    { key: 'apikeys', label: 'API keys', group: 'Berth', admin: true, load: () => import('./ApiKeys.svelte') },
    { key: 'security', label: 'Sign-in and security', group: 'Berth', load: () => import('./Security.svelte') },
    { key: 'docker', label: 'Docker', group: 'Containers', load: () => import('./Docker.svelte') },
    { key: 'updates', label: 'Updates', group: 'Containers', load: () => import('./UpdatesSettings.svelte') },
    { key: 'backups', label: 'Backups', group: 'Containers', load: () => import('./BackupsSettings.svelte') },
    { key: 'integrations', label: 'App integrations', group: 'Containers', load: () => import('./IntegrationsSettings.svelte') },
    { key: 'notifications', label: 'Notifications', group: 'Watching', load: () => import('./Notifications.svelte') },
    { key: 'monitoring', label: 'Monitoring and alerts', group: 'Watching', load: () => import('./MonitoringSettings.svelte') },
    { key: 'uptime', label: 'Uptime', group: 'Watching', load: () => import('./UptimeSettings.svelte') },
    { key: 'proxmox', label: 'Proxmox', group: 'Connections', load: () => import('./ProxmoxSettings.svelte') },
    { key: 'homeassistant', label: 'Home Assistant', group: 'Connections', load: () => import('./Mqtt.svelte') },
    { key: 'transfer', label: 'Import and export', group: 'Connections', load: () => import('./ImportExport.svelte') },
  ];

  const visible = $derived(SECTIONS.filter((s) => !s.admin || session.isAdmin));
  const current = $derived(visible.find((s) => s.key === params.section) ?? visible[0]);
  const groups = $derived([...new Set(visible.map((s) => s.group))].map((g) => ({ name: g, items: visible.filter((s) => s.group === g) })));

  let Section = $state<Component | null>(null);
  let loading = $state(false);

  $effect(() => {
    const s = current;
    loading = true;
    s.load().then((m) => {
      if (s === current) {
        Section = m.default;
        loading = false;
      }
    });
  });
</script>

<PageHeader title="Settings" subtitle={session.isAdmin ? '' : 'Read-only: only admins can change settings.'} />

<div class="page-body settings">
  <nav aria-label="Settings sections">
    <select class="input mobile" value={current.key} onchange={(e) => router.navigate(`/settings/${e.currentTarget.value}`)} aria-label="Section">
      {#each visible as s (s.key)}<option value={s.key}>{s.label}</option>{/each}
    </select>
    <div class="desktop">
      {#each groups as g (g.name)}
        <p class="group">{g.name}</p>
        {#each g.items as s (s.key)}
          <a href={href(`/settings/${s.key}`)} class:active={s.key === current.key} aria-current={s.key === current.key ? 'page' : undefined}>{s.label}</a>
        {/each}
      {/each}
    </div>
  </nav>
  <div class="content">
    {#if !session.settings || (loading && !Section)}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if Section}
      {#key current.key}<Section />{/key}
    {/if}
  </div>
</div>

<style>
  .settings {
    display: grid;
    grid-template-columns: 210px minmax(0, 1fr);
    gap: 28px;
    align-items: start;
  }
  nav {
    position: sticky;
    top: 16px;
  }
  .desktop {
    display: grid;
    gap: 1px;
  }
  .group {
    margin: 14px 0 4px;
    padding: 0 10px;
    color: var(--text-3);
    font: 600 var(--fs-xs) / 1 var(--font-data);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .group:first-child {
    margin-top: 0;
  }
  nav a {
    display: block;
    padding: 7px 10px;
    border-radius: var(--radius);
    color: var(--text-2);
    text-decoration: none;
  }
  nav a:hover {
    background: var(--raised);
    color: var(--text);
  }
  nav a.active {
    background: var(--accent-soft);
    color: var(--text);
    font-weight: 500;
  }
  .mobile {
    display: none;
  }
  .content {
    min-width: 0;
  }
  @media (max-width: 860px) {
    .settings {
      grid-template-columns: 1fr;
      gap: 14px;
    }
    nav {
      position: static;
    }
    .desktop {
      display: none;
    }
    .mobile {
      display: block;
    }
  }
</style>
