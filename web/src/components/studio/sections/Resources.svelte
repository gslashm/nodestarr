<!-- Limits (memory, CPU, processes), GPUs, devices and groups. -->
<script lang="ts">
  import Field from '../../Field.svelte';
  import StringList from '../StringList.svelte';
  import { get, getGpus, getLimit, getList, set, setGpus, setLimit, setList } from '../../../lib/compose/service';
  import { useStudio } from '../../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../../lib/compose/yamldoc';
  import { ui } from '../../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();

  function edit(what: string, fn: (d: YDoc, s: YAMLMap) => void) {
    try {
      studio.editService(name, fn, `svc:${name}:${what}`);
    } catch (err) {
      ui.error(err);
    }
  }

  const memory = $derived(getLimit(doc, svc, 'memory'));
  const cpus = $derived(getLimit(doc, svc, 'cpus'));
  const gpus = $derived(getGpus(doc, svc));
  const devices = $derived(getList(doc, svc, 'devices'));
  // Devices written in the long form are edited as YAML (under Other).
  const longDevices = $derived(((get(doc, svc, 'devices') as unknown[] | undefined) ?? []).some((d) => d !== null && typeof d === 'object'));
  const memValid = $derived(!memory.value || /^\$|^\d+(\.\d+)?\s*([bkmg]i?b?)?$/i.test(memory.value));
</script>

<Field label="Memory limit" hint="Like 512m or 2g. The app is stopped if it goes over. Empty means no limit." error={memValid ? '' : 'Use a number with a unit, like 512m or 2g.'}>
  <div class="inline">
    <input class="input mono short" class:invalid={!memValid} value={memory.value} placeholder="No limit" spellcheck="false" aria-label="Memory limit" oninput={(e) => edit('mem', (d, s) => setLimit(d, s, 'memory', e.currentTarget.value))} />
    {#each ['256m', '512m', '1g', '2g', '4g'] as v (v)}
      <button type="button" class="chip preset" class:on={memory.value === v} onclick={() => edit('mem', (d, s) => setLimit(d, s, 'memory', v))}>{v}</button>
    {/each}
  </div>
</Field>

<Field label="CPU limit" hint="Number of cores it may use, like 0.5 or 2. Empty means no limit.">
  <input class="input mono short" value={cpus.value} placeholder="No limit" inputmode="decimal" aria-label="CPU limit" oninput={(e) => edit('cpus', (d, s) => setLimit(d, s, 'cpus', e.currentTarget.value))} />
</Field>

<Field label="Process limit" hint="Most processes it may start (pids_limit); stops fork bombs.">
  <input class="input mono short" value={String(get(doc, svc, 'pids_limit') ?? '')} placeholder="No limit" inputmode="numeric" aria-label="Process limit" oninput={(e) => edit('pids', (d, s) => set(d, s, 'pids_limit', /^\d+$/.test(e.currentTarget.value) ? Number(e.currentTarget.value) : e.currentTarget.value))} />
</Field>

<Field label="NVIDIA GPUs" hint="Needs the NVIDIA container toolkit on the server.">
  <select class="input short" value={gpus === 'all' || gpus === '' ? gpus : /^\d+$/.test(gpus) ? 'count' : gpus} aria-label="GPUs" onchange={(e) => edit('gpus', (d, s) => setGpus(d, s, e.currentTarget.value === 'count' ? '1' : e.currentTarget.value))}>
    <option value="">None</option>
    <option value="all">All GPUs</option>
    <option value="count">Some of them</option>
    {#if gpus === 'custom'}<option value="custom">Chosen by ID (edit as YAML)</option>{/if}
  </select>
  {#if /^\d+$/.test(gpus)}
    <label class="count">How many <input class="input mono" type="number" min="1" value={gpus} onchange={(e) => edit('gpus', (d, s) => setGpus(d, s, e.currentTarget.value))} /></label>
  {/if}
</Field>

<Field label="Devices" hint="Hardware passed into the container, as server-path:container-path. /dev/dri is the Intel or AMD GPU for video transcoding.">
  {#if longDevices}
    <p class="faint">These are written in the long form; edit them under Other.</p>
  {:else}
    <StringList
      items={devices}
      onchange={(items) => edit('devices', (d, s) => setList(d, s, 'devices', items))}
      placeholder="/dev/dri:/dev/dri"
      suggestions={['/dev/dri:/dev/dri', '/dev/net/tun:/dev/net/tun', '/dev/ttyUSB0:/dev/ttyUSB0', '/dev/ttyACM0:/dev/ttyACM0']}
      label="Device"
    />
  {/if}
</Field>

<Field label="Extra groups" hint="Group names or IDs, like video or render for GPU access.">
  <StringList items={getList(doc, svc, 'group_add')} onchange={(items) => edit('groups', (d, s) => setList(d, s, 'group_add', items))} placeholder="render" label="Group" />
</Field>

<Field label="Shared memory" hint="Size of /dev/shm, like 256m. Docker's default is 64m; browsers and databases sometimes need more.">
  <input class="input mono short" value={String(get(doc, svc, 'shm_size') ?? '')} placeholder="64m" spellcheck="false" aria-label="Shared memory" oninput={(e) => edit('shm', (d, s) => set(d, s, 'shm_size', e.currentTarget.value))} />
</Field>

<style>
  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .short {
    max-width: 200px;
  }
  .preset {
    cursor: pointer;
  }
  .preset:hover,
  .preset.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .count {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .count .input {
    width: 80px;
    min-height: 32px;
    padding: 4px 8px;
  }
</style>
