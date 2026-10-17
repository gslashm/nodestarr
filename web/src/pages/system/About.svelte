<!-- Nodestarr itself: version and updates, what it's connected to, and how much it uses. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Button from '../../components/Button.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { bytes, duration, hasUpdate, updateLabel } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { DockerInfo, UpdateInfo } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  interface Status {
    version: string;
    instance_name: string;
    started_at: number;
    now: number;
    timezone: string;
    process: { memory: number | null; pid: number; workers: number };
    data_dir: string;
    database_size: number | null;
    docker: { host: string; connected: boolean; info: DockerInfo | null; compose: string | null };
    containers: { total: number; running: number };
    tasks_active: number;
    live_viewers: number;
    self: { id: string; name: string; identity: string; image: string; update: UpdateInfo | null } | null;
    mqtt: { enabled: boolean; connected: boolean; error: string | null };
    proxmox: { enabled: boolean };
    backups_root_available: boolean;
    repo: string | null;
  }

  let status = $state<Status | null>(null);
  let error = $state('');

  async function load() {
    try {
      status = await api.get<Status>('/system/status');
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(() => {
    void load();
    const t = setInterval(() => {
      if (!document.hidden) void load();
    }, 15_000);
    return () => clearInterval(t);
  });

  async function updateNodestarr() {
    const ok = await ui.confirm({
      title: 'Update Nodestarr?',
      message: `Nodestarr pulls ${updateLabel(status?.self?.update) ?? 'the new version'} and restarts itself. The page reconnects on its own after a few seconds.`,
      confirmLabel: 'Update Nodestarr',
    });
    if (!ok) return;
    try {
      const r = await api.post<{ task_id: string }>('/system/update');
      ui.taskStarted(r.task_id, 'Updating Nodestarr');
    } catch (err) {
      ui.error(err, "Couldn't update Nodestarr");
    }
  }

  async function checkSelf() {
    if (!status?.self) return;
    try {
      const info = await api.post<UpdateInfo>(`/containers/${encodeURIComponent(status.self.id)}/check`);
      if (status.self) status.self.update = info;
      if (info.status === 'skipped') ui.toast('Nothing newer than the build you skipped', 'success');
      else ui.toast(hasUpdate(info) ? `Nodestarr ${updateLabel(info)} is available` : 'Nodestarr is up to date', hasUpdate(info) ? 'info' : 'success');
    } catch (err) {
      ui.error(err);
    }
  }
</script>

{#if error}
  <p class="text-bad">{error}</p>
{:else if !status}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else}
  {@const s = status}
  <div class="grid">
    <section class="panel hero">
      <div class="panel-body">
        <p class="big">Nodestarr <span class="num">{s.version}</span></p>
        {#if s.self}
          {#if hasUpdate(s.self.update)}
            <p class="text-update">Version {updateLabel(s.self.update)} is available.</p>
            {#if session.isAdmin}<Button variant="primary" icon={ArrowUp} onclick={updateNodestarr}>Update Nodestarr</Button>{/if}
          {:else}
            <p class="muted">
              {s.self.update?.status === 'up_to_date' ? 'Up to date.' : s.self.update?.status === 'skipped' ? "You skipped the new build; you'll be told when a newer one comes out." : 'Not checked yet.'}
            </p>
            {#if session.isAdmin}<Button variant="ghost" onclick={checkSelf}>Check for a new version</Button>{/if}
          {/if}
          <p class="faint small">Running as container <a href={href(`/containers/${encodeURIComponent(s.self.name)}`)}>{s.self.name}</a> from <span class="mono">{s.self.image}</span></p>
        {:else}
          <p class="muted small">Nodestarr isn't running in a container it can see, so update it the way you installed it.</p>
        {/if}
        {#if s.repo}<a class="small" href="https://github.com/{s.repo}" target="_blank" rel="noopener">Source and release notes <ExternalLink size={12} /></a>{/if}
      </div>
    </section>

    <section class="panel">
      <div class="panel-head"><h2>Running</h2></div>
      <dl class="kv panel-body">
        <dt>Up for</dt><dd class="num">{duration(s.now - s.started_at)}</dd>
        <dt>Memory</dt><dd class="num">{s.process.memory ? bytes(s.process.memory) : '–'}</dd>
        <dt>Threads</dt><dd class="num">{s.process.workers} workers</dd>
        <dt>Database</dt><dd class="num">{s.database_size !== null ? bytes(s.database_size) : '–'} <span class="faint mono">{s.data_dir}</span></dd>
        <dt>Time zone</dt><dd>{s.timezone}</dd>
        <dt>Background tasks</dt><dd class="num">{s.tasks_active} running</dd>
        <dt>Open pages</dt><dd class="num">{s.live_viewers}</dd>
      </dl>
    </section>

    <section class="panel">
      <div class="panel-head"><h2>Connections</h2></div>
      <dl class="kv panel-body">
        <dt>Docker</dt>
        <dd>
          {#if s.docker.connected}<span class="text-ok">Connected</span>{:else}<span class="text-bad">Not reachable</span>{/if}
          <span class="faint mono small">{s.docker.host}</span>
        </dd>
        {#if s.docker.info}<dt>Engine</dt><dd class="num">Docker {s.docker.info.version}{s.docker.compose ? `, Compose ${s.docker.compose}` : ', no Compose'}</dd>{/if}
        <dt>Containers</dt><dd class="num">{s.containers.running} running of {s.containers.total}</dd>
        <dt>Backups</dt>
        <dd>{#if s.backups_root_available}<span class="text-ok">Folder available</span>{:else}<span class="text-bad">Folder missing</span>{/if}</dd>
        <dt>Home Assistant</dt>
        <dd>
          {#if !s.mqtt.enabled}<span class="faint">Off</span>
          {:else if s.mqtt.connected}<span class="text-ok">Connected</span>
          {:else}<span class="text-bad">Not connected</span>{#if s.mqtt.error}<span class="faint small"> {s.mqtt.error}</span>{/if}{/if}
        </dd>
        <dt>Proxmox</dt>
        <dd>{#if s.proxmox.enabled}<a href={href('/proxmox')}>Connected</a>{:else}<span class="faint">Off</span>{/if}</dd>
      </dl>
    </section>
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
    gap: 16px;
    align-items: start;
  }
  .hero .panel-body {
    display: grid;
    gap: 10px;
    justify-items: start;
  }
  .hero p {
    margin: 0;
  }
  .big {
    font: 600 26px / 1.1 var(--font-data);
  }
  .small {
    font-size: var(--fs-sm);
  }
  .kv dd {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
  }
</style>
