<!-- System: Nodestarr itself, Docker's images, volumes and networks, scheduled jobs, and Nodestarr's log. -->
<script lang="ts">
  import type { Component } from 'svelte';
  import PageHeader from '../../components/PageHeader.svelte';
  import Tabs from '../../components/Tabs.svelte';

  let { params }: { params: Record<string, string> } = $props();

  const loaders: Record<string, () => Promise<{ default: Component }>> = {
    about: () => import('./About.svelte'),
    docker: () => import('./DockerResources.svelte'),
    jobs: () => import('./Jobs.svelte'),
    logs: () => import('./NodestarrLogs.svelte'),
  };

  const tab = $derived(params.tab && loaders[params.tab] ? params.tab : 'about');
  let Tab = $state<Component | null>(null);

  $effect(() => {
    const key = tab;
    loaders[key]().then((m) => {
      if (key === tab) Tab = m.default;
    });
  });

  const tabList = [
    { key: 'about', label: 'Nodestarr', path: '/system' },
    { key: 'docker', label: 'Images, volumes, networks', path: '/system/docker' },
    { key: 'jobs', label: 'Scheduled jobs', path: '/system/jobs' },
    { key: 'logs', label: "Nodestarr's log", path: '/system/logs' },
  ];
</script>

<PageHeader title="System">
  {#snippet tabs()}
    <Tabs tabs={tabList} active={tab} />
  {/snippet}
</PageHeader>

<div class="page-body">
  {#if Tab}
    {#key tab}<Tab />{/key}
  {/if}
</div>
