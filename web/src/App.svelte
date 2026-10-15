<script lang="ts">
  import type { Component } from 'svelte';
  import { onMount } from 'svelte';
  import { setLucideProps } from '@lucide/svelte/context';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import CommandPalette from './components/CommandPalette.svelte';
  import ConfirmHost from './components/ConfirmHost.svelte';
  import Logo from './components/Logo.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Spinner from './components/Spinner.svelte';
  import TaskDrawer from './components/TaskDrawer.svelte';
  import Toasts from './components/Toasts.svelte';
  import { errorMessage, onUnauthorized } from './lib/api';
  import { live } from './lib/live.svelte';
  import { interceptLinks, match, router } from './lib/router.svelte';
  import { session } from './lib/session.svelte';
  import Login from './pages/Login.svelte';
  import NotFound from './pages/NotFound.svelte';

  setLucideProps({ size: 17, strokeWidth: 1.9 });

  type Loader = () => Promise<{ default: Component<{ params: Record<string, string> }> }>;

  // Order matters: fixed paths before parameters.
  const routes: [string, Loader][] = [
    ['/', () => import('./pages/Dashboard.svelte')],
    ['/containers', () => import('./pages/Containers.svelte')],
    ['/containers/new', () => import('./pages/NewContainer.svelte')],
    ['/containers/:id/:tab?', () => import('./pages/container/ContainerPage.svelte')],
    ['/stacks', () => import('./pages/Stacks.svelte')],
    ['/stacks/new', () => import('./pages/NewStack.svelte')],
    ['/stacks/:name/:tab?', () => import('./pages/stack/StackPage.svelte')],
    ['/updates/:tab?', () => import('./pages/Updates.svelte')],
    ['/backups/:tab?', () => import('./pages/Backups.svelte')],
    ['/monitoring/:tab?', () => import('./pages/monitoring/Monitoring.svelte')],
    ['/activity/:tab?', () => import('./pages/Activity.svelte')],
    ['/proxmox/:tab?', () => import('./pages/proxmox/Proxmox.svelte')],
    ['/settings/:section?', () => import('./pages/settings/Settings.svelte')],
    ['/system/:tab?', () => import('./pages/system/System.svelte')],
  ];

  let Page = $state<Component<{ params: Record<string, string> }> | null>(null);
  let params = $state<Record<string, string>>({});
  let bootError = $state('');
  let navToken = 0;

  $effect(() => {
    const path = router.path;
    const token = ++navToken;
    for (const [pattern, load] of routes) {
      const p = match(pattern, path);
      if (!p) continue;
      load()
        .then((m) => {
          if (token !== navToken) return;
          Page = m.default;
          params = p;
        })
        .catch((err) => {
          // A new version was deployed and old chunks are gone: reload once.
          if (String(err).includes('Failed to fetch dynamically imported module')) location.reload();
          else bootError = errorMessage(err);
        });
      return;
    }
    Page = NotFound as Component<{ params: Record<string, string> }>;
    params = {};
  });

  async function boot() {
    bootError = '';
    try {
      await session.init();
    } catch (err) {
      bootError = errorMessage(err);
    }
  }

  onMount(() => {
    onUnauthorized(() => session.expired());
    const stop = interceptLinks();
    void boot();
    return stop;
  });

  $effect(() => {
    if (session.signedIn) void live.start();
    else live.stop();
  });
</script>

{#if !session.status}
  <div class="boot">
    <Logo size={44} />
    {#if bootError}
      <p class="text-bad">{bootError}</p>
      <button type="button" class="btn" onclick={boot}>Try again</button>
    {:else}
      <Spinner size={18} />
    {/if}
  </div>
{:else if !session.signedIn}
  <Login />
{:else}
  <div class="shell">
    <Sidebar />
    <main id="main">
      {#if !live.dockerConnected}
        <div class="banner" role="alert">
          <TriangleAlert size={18} />
          <span>Nodestarr can't reach Docker right now. What you see may be out of date; it reconnects automatically.</span>
        </div>
      {/if}
      {#if live.loadError && !live.loaded}
        <div class="boot inline">
          <p class="text-bad">{live.loadError}</p>
          <button type="button" class="btn" onclick={() => live.refresh()}>Try again</button>
        </div>
      {:else if Page}
        {#key Page}
          <Page {params} />
        {/key}
      {/if}
    </main>
  </div>
  <TaskDrawer />
  <ConfirmHost />
  <CommandPalette />
{/if}
<Toasts />

<style>
  .shell {
    display: grid;
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
    height: 100vh;
    height: 100dvh;
  }
  main {
    height: 100vh;
    height: 100dvh;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .boot {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 18px;
    min-height: 100vh;
    color: var(--text-2);
    text-align: center;
    padding: 20px;
  }
  .boot.inline {
    min-height: 60vh;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 24px;
    background: color-mix(in srgb, var(--warn) 14%, var(--bg));
    border-bottom: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
    color: var(--text);
  }
  .banner :global(svg) {
    flex: none;
    color: var(--warn);
  }
  @media (max-width: 760px) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
