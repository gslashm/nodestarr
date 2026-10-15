<!--
  Docker's images, volumes and networks: what uses disk space, what is in
  use and by whom, removal of single items, and a guided cleanup.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Eraser from '@lucide/svelte/icons/eraser';
  import Plus from '@lucide/svelte/icons/plus';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Button from '../../components/Button.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import Field from '../../components/Field.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { bytes, dateTime, plural, shortDigest, shortImage } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ImageRow, NetworkRow, UsageGroup, VolumeRow } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  type View = 'images' | 'volumes' | 'networks';

  let view = $state<View>('images');
  let df = $state<{ images: UsageGroup; containers: UsageGroup; volumes: UsageGroup; build_cache: UsageGroup } | null>(null);
  let images = $state<ImageRow[] | null>(null);
  let volumes = $state<VolumeRow[] | null>(null);
  let networks = $state<NetworkRow[] | null>(null);
  let error = $state('');
  let filter = $state('');
  let sizesLoading = $state(false);

  let cleanup = $state<{
    dangling_images: boolean;
    unused_images: boolean;
    include_rollback: boolean;
    containers: boolean;
    volumes: boolean;
    named_volumes: boolean;
    networks: boolean;
    build_cache: boolean;
  } | null>(null);

  let newVolume = $state<{ name: string } | null>(null);
  let newNetwork = $state<{ name: string; driver: string; subnet: string; gateway: string; internal: boolean } | null>(null);

  async function loadDf() {
    try {
      df = await api.get('/system/df');
    } catch {
      // The tables work without the totals.
    }
  }

  async function loadImages() {
    try {
      images = (await api.get<{ images: ImageRow[] }>('/images')).images;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function loadVolumes(sizes = false) {
    if (sizes) sizesLoading = true;
    try {
      volumes = (await api.get<{ volumes: VolumeRow[] }>(`/volumes${sizes ? '?sizes=1' : ''}`)).volumes;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      sizesLoading = false;
    }
  }

  async function loadNetworks() {
    try {
      networks = (await api.get<{ networks: NetworkRow[] }>('/networks')).networks;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(() => {
    void loadDf();
    void loadImages();
    void loadVolumes();
    void loadNetworks();
  });

  // A finished cleanup changes everything.
  let pruning = 0;
  $effect(() => {
    const n = live.activeTasks.filter((t) => t.kind === 'prune').length;
    if (n < pruning) {
      void loadDf();
      void loadImages();
      void loadVolumes();
      void loadNetworks();
    }
    pruning = n;
  });

  const needle = $derived(filter.trim().toLowerCase());
  const shownImages = $derived(
    (images ?? [])
      .filter((i) => !needle || i.tags.some((t) => t.toLowerCase().includes(needle)) || i.id.includes(needle))
      .sort((a, b) => Number(a.containers.length > 0) - Number(b.containers.length > 0) || b.size - a.size),
  );
  const shownVolumes = $derived((volumes ?? []).filter((v) => !needle || v.name.toLowerCase().includes(needle) || (v.stack ?? '').includes(needle)));
  const shownNetworks = $derived((networks ?? []).filter((n) => !needle || n.name.toLowerCase().includes(needle)));

  function containerName(idOrName: string): string {
    const c = live.containers[idOrName] ?? live.find(idOrName);
    return c?.display_name ?? idOrName.replace(/^\//, '').slice(0, 24);
  }

  function containerHref(idOrName: string): string | null {
    const c = live.containers[idOrName] ?? live.find(idOrName);
    return c ? href(`/containers/${encodeURIComponent(c.name)}`) : null;
  }

  async function removeImage(i: ImageRow) {
    const used = i.containers.length > 0;
    const r = await ui.ask({
      title: `Remove ${i.tags[0] ? shortImage(i.tags[0]) : shortDigest(i.id)}?`,
      message: used
        ? `${plural(i.containers.length, 'container')} still use this image. Removing it only works if those containers are removed first.`
        : `Frees ${bytes(i.size)}. It can be pulled again later.${i.rollback ? ' This is a rollback image: the update it belongs to can no longer be rolled back.' : ''}`,
      confirmLabel: 'Remove',
      danger: true,
      options: i.tags.length > 1 ? [{ key: 'force', label: `Remove all ${i.tags.length} tags of it` }] : [],
    });
    if (!r.ok) return;
    try {
      await api.del(`/images/${encodeURIComponent(i.id)}${r.options.force ? '?force=1' : ''}`);
      ui.toast('Image removed', 'success');
      await Promise.all([loadImages(), loadDf()]);
    } catch (err) {
      ui.error(err, "Couldn't remove it");
    }
  }

  async function removeVolume(v: VolumeRow) {
    const r = await ui.ask({
      title: `Delete volume ${v.anonymous ? shortDigest(v.name) : v.name}?`,
      message: 'Everything stored in it is deleted for good.',
      confirmLabel: 'Delete',
      danger: true,
      typeToConfirm: v.anonymous ? undefined : v.name,
    });
    if (!r.ok) return;
    try {
      await api.del(`/volumes/${encodeURIComponent(v.name)}`);
      ui.toast('Volume deleted', 'success');
      await Promise.all([loadVolumes(), loadDf()]);
    } catch (err) {
      ui.error(err, "Couldn't delete it");
    }
  }

  async function removeNetwork(n: NetworkRow) {
    const ok = await ui.confirm({ title: `Remove network ${n.name}?`, confirmLabel: 'Remove', danger: true });
    if (!ok) return;
    try {
      await api.del(`/networks/${encodeURIComponent(n.id)}`);
      await loadNetworks();
    } catch (err) {
      ui.error(err, "Couldn't remove it");
    }
  }

  async function createVolume() {
    if (!newVolume) return;
    try {
      await api.post('/volumes', { name: newVolume.name.trim() });
      newVolume = null;
      await loadVolumes();
    } catch (err) {
      ui.error(err, "Couldn't create it");
    }
  }

  async function createNetwork() {
    if (!newNetwork) return;
    try {
      await api.post('/networks', {
        name: newNetwork.name.trim(),
        driver: newNetwork.driver,
        internal: newNetwork.internal,
        subnet: newNetwork.subnet.trim() || null,
        gateway: newNetwork.gateway.trim() || null,
      });
      newNetwork = null;
      await loadNetworks();
    } catch (err) {
      ui.error(err, "Couldn't create it");
    }
  }

  async function runCleanup() {
    if (!cleanup) return;
    try {
      const r = await api.post<{ task_id: string }>('/system/prune', cleanup);
      ui.taskStarted(r.task_id, 'Cleaning up Docker');
      cleanup = null;
    } catch (err) {
      ui.error(err, "Couldn't start the cleanup");
    }
  }

  const reclaimable = $derived(df ? df.images.reclaimable + df.containers.reclaimable + df.volumes.reclaimable + df.build_cache.reclaimable : 0);
</script>

{#if df}
  <div class="usage">
    {#each [['Images', df.images], ['Containers', df.containers], ['Volumes', df.volumes], ['Build cache', df.build_cache]] as [label, g] (label)}
      {@const u = g as UsageGroup}
      <section class="panel tile">
        <h3>{label}</h3>
        <p class="big num">{bytes(u.size)}</p>
        <p class="sub">{u.count} total, {u.active} in use{u.reclaimable > 0 ? `, ${bytes(u.reclaimable)} can be freed` : ''}</p>
      </section>
    {/each}
  </div>
{/if}

<div class="bar">
  <Segmented
    label="Show"
    bind:value={view}
    options={[
      { value: 'images', label: `Images${images ? ` (${images.length})` : ''}` },
      { value: 'volumes', label: `Volumes${volumes ? ` (${volumes.length})` : ''}` },
      { value: 'networks', label: `Networks${networks ? ` (${networks.length})` : ''}` },
    ]}
  />
  <input class="input filter" type="search" placeholder="Filter" bind:value={filter} aria-label="Filter" />
  <span class="grow"></span>
  {#if session.isAdmin}
    {#if view === 'volumes'}
      <Button small variant="ghost" onclick={() => loadVolumes(true)} busy={sizesLoading}>Measure sizes</Button>
      <Button small variant="ghost" icon={Plus} onclick={() => (newVolume = { name: '' })}>New volume</Button>
    {:else if view === 'networks'}
      <Button small variant="ghost" icon={Plus} onclick={() => (newNetwork = { name: '', driver: 'bridge', subnet: '', gateway: '', internal: false })}>New network</Button>
    {/if}
    <Button
      small
      icon={Eraser}
      onclick={() =>
        (cleanup = { dangling_images: true, unused_images: false, include_rollback: false, containers: false, volumes: false, named_volumes: false, networks: true, build_cache: true })}
      >Clean up{reclaimable ? ` (${bytes(reclaimable)})` : ''}</Button
    >
  {/if}
</div>

{#if error}<p class="text-bad">{error}</p>{/if}

<section class="panel">
  <div class="table-wrap">
    {#if view === 'images'}
      {#if !images}
        <p class="pad faint"><Spinner size={13} /> Loading…</p>
      {:else}
        <table class="table">
          <thead><tr><th>Image</th><th class="num">Size</th><th>Created</th><th>Used by</th><th class="shrink"></th></tr></thead>
          <tbody>
            {#each shownImages as i (i.id)}
              <tr>
                <td>
                  {#if i.tags.length}
                    {#each i.tags.slice(0, 3) as t (t)}<div class="mono small">{shortImage(t)}</div>{/each}
                    {#if i.tags.length > 3}<div class="faint small">and {i.tags.length - 3} more tags</div>{/if}
                  {:else}
                    <span class="mono small faint">{shortDigest(i.id)}</span>
                  {/if}
                  {#if i.dangling}<span class="chip">unused leftover</span>{/if}
                  {#if i.rollback}<span class="chip rb">kept for rollback</span>{/if}
                </td>
                <td class="num">{bytes(i.size)}</td>
                <td class="small">{dateTime(i.created)}</td>
                <td class="small">
                  {#each i.containers as c (c)}
                    {@const link = containerHref(c)}
                    {#if link}<a href={link}>{containerName(c)}</a>{:else}{containerName(c)}{/if}{' '}
                  {:else}<span class="faint">nothing</span>{/each}
                </td>
                <td class="shrink">
                  {#if session.isAdmin}<button type="button" class="btn small ghost icon" onclick={() => removeImage(i)} aria-label="Remove image" title="Remove"><Trash2 size={15} /></button>{/if}
                </td>
              </tr>
            {:else}
              <tr><td colspan="5" class="faint">No images match.</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {:else if view === 'volumes'}
      {#if !volumes}
        <p class="pad faint"><Spinner size={13} /> Loading…</p>
      {:else}
        <table class="table">
          <thead><tr><th>Volume</th><th>Stack</th><th class="num">Size</th><th>Used by</th><th class="shrink"></th></tr></thead>
          <tbody>
            {#each shownVolumes as v (v.name)}
              <tr>
                <td>
                  <span class="mono small" title={v.mountpoint}>{v.anonymous ? shortDigest(v.name) : v.name}</span>
                  {#if v.anonymous}<span class="chip">anonymous</span>{/if}
                  {#if v.driver !== 'local'}<span class="chip">{v.driver}</span>{/if}
                </td>
                <td class="small">{v.stack ?? ''}</td>
                <td class="num">{v.size !== null && v.size >= 0 ? bytes(v.size) : '–'}</td>
                <td class="small">
                  {#each v.containers as c (c)}
                    {@const link = containerHref(c)}
                    {#if link}<a href={link}>{containerName(c)}</a>{:else}{containerName(c)}{/if}{' '}
                  {:else}<span class="faint">nothing</span>{/each}
                </td>
                <td class="shrink">
                  {#if session.isAdmin}<button type="button" class="btn small ghost icon" onclick={() => removeVolume(v)} disabled={v.containers.length > 0} aria-label="Delete volume" title={v.containers.length ? 'In use' : 'Delete'}><Trash2 size={15} /></button>{/if}
                </td>
              </tr>
            {:else}
              <tr><td colspan="5" class="faint">No volumes match.</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {:else}
      {#if !networks}
        <p class="pad faint"><Spinner size={13} /> Loading…</p>
      {:else}
        <table class="table">
          <thead><tr><th>Network</th><th>Driver</th><th>Subnet</th><th>Used by</th><th class="shrink"></th></tr></thead>
          <tbody>
            {#each shownNetworks as n (n.id)}
              <tr>
                <td>
                  <span class="mono small">{n.name}</span>
                  {#if n.builtin}<span class="chip">built in</span>{/if}
                  {#if n.internal}<span class="chip">internal</span>{/if}
                  {#if n.stack}<span class="faint small"> {n.stack}</span>{/if}
                </td>
                <td class="small">{n.driver}</td>
                <td class="mono small">{n.subnets.join(', ')}</td>
                <td class="small">
                  {#each n.containers as c (c)}
                    {@const link = containerHref(c)}
                    {#if link}<a href={link}>{containerName(c)}</a>{:else}{containerName(c)}{/if}{' '}
                  {:else}<span class="faint">nothing</span>{/each}
                </td>
                <td class="shrink">
                  {#if session.isAdmin && !n.builtin}<button type="button" class="btn small ghost icon" onclick={() => removeNetwork(n)} disabled={n.containers.length > 0} aria-label="Remove network" title={n.containers.length ? 'In use' : 'Remove'}><Trash2 size={15} /></button>{/if}
                </td>
              </tr>
            {:else}
              <tr><td colspan="5" class="faint">No networks match.</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </div>
</section>

<Dialog open={!!cleanup} title="Clean up Docker" width="560px" onclose={() => (cleanup = null)}>
  {#if cleanup}
    <div class="clean">
      <Toggle bind:checked={cleanup.dangling_images} label="Unused leftover images" hint="Untagged images left behind by updates and builds." />
      <Toggle bind:checked={cleanup.unused_images} label="All images no container uses" hint="They are pulled again if you need them." />
      {#if cleanup.unused_images}
        <Toggle bind:checked={cleanup.include_rollback} label="Including images kept for rollback" hint="Recent updates can then no longer be rolled back." />
      {/if}
      <Toggle bind:checked={cleanup.containers} label="Stopped containers" hint="Containers in stacks come back when the stack is deployed." />
      <Toggle bind:checked={cleanup.networks} label="Unused networks" />
      <Toggle bind:checked={cleanup.build_cache} label="Build cache" />
      <Toggle bind:checked={cleanup.volumes} label="Unused anonymous volumes" hint="Volumes without a name that no container uses." />
      {#if cleanup.volumes}
        <Toggle bind:checked={cleanup.named_volumes} label="Also unused named volumes" hint="Their data is deleted for good." />
      {/if}
    </div>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (cleanup = null)}>Cancel</button>
    <button type="button" class="btn {cleanup?.named_volumes ? 'danger solid' : 'primary'}" onclick={runCleanup}>Clean up</button>
  {/snippet}
</Dialog>

<Dialog open={!!newVolume} title="New volume" width="440px" onclose={() => (newVolume = null)}>
  {#if newVolume}
    <form
      id="vol-form"
      onsubmit={(e) => {
        e.preventDefault();
        void createVolume();
      }}
    >
      <input class="input mono" bind:value={newVolume.name} placeholder="app-data" aria-label="Name" />
    </form>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (newVolume = null)}>Cancel</button>
    <button class="btn primary" form="vol-form" type="submit" disabled={!newVolume?.name.trim()}>Create</button>
  {/snippet}
</Dialog>

<Dialog open={!!newNetwork} title="New network" width="520px" onclose={() => (newNetwork = null)}>
  {#if newNetwork}
    <form
      id="net-form"
      onsubmit={(e) => {
        e.preventDefault();
        void createNetwork();
      }}
    >
      <Field label="Name" id="n-name"><input id="n-name" class="input mono" bind:value={newNetwork.name} placeholder="proxy" /></Field>
      <Field label="Driver" id="n-driver">
        <select id="n-driver" class="input" bind:value={newNetwork.driver}>
          <option value="bridge">bridge</option>
          <option value="macvlan">macvlan</option>
          <option value="ipvlan">ipvlan</option>
        </select>
      </Field>
      <Field label="Subnet" id="n-subnet" hint="Optional, like 172.30.0.0/24.">
        <input id="n-subnet" class="input mono" bind:value={newNetwork.subnet} />
      </Field>
      <Field label="Gateway" id="n-gw" hint="Optional.">
        <input id="n-gw" class="input mono" bind:value={newNetwork.gateway} />
      </Field>
      <Field label="Internal">
        <Toggle bind:checked={newNetwork.internal} label="No access to the outside" />
      </Field>
    </form>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (newNetwork = null)}>Cancel</button>
    <button class="btn primary" form="net-form" type="submit" disabled={!newNetwork?.name.trim()}>Create</button>
  {/snippet}
</Dialog>

<style>
  .usage {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px;
    margin-bottom: 18px;
  }
  .tile {
    display: grid;
    gap: 4px;
    padding: 14px 16px;
  }
  .tile h3 {
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .tile p {
    margin: 0;
  }
  .big {
    font: 600 22px / 1.1 var(--font-data);
  }
  .sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  .filter {
    max-width: 240px;
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .rb {
    color: var(--accent-text);
  }
  .pad {
    margin: 0;
    padding: 14px 16px;
  }
  .clean {
    display: grid;
    gap: 14px;
  }
</style>
