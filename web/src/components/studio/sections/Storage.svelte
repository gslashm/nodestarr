<!--
  Folders and volumes a service keeps its data in. Relative folders are
  shown with the full path they end up at, and named volumes are declared
  at the top of the file when they're first used.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import Field from '../../Field.svelte';
  import StringList from '../StringList.svelte';
  import { api } from '../../../lib/api';
  import { interpolate } from '../../../lib/compose/interpolate';
  import { declareTopLevel, getList, getMounts, removeItem, setList, setMount, topLevelNames, type MountSpec, type MountType } from '../../../lib/compose/service';
  import { joinPath, useStudio } from '../../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../../lib/compose/yamldoc';
  import type { VolumeRow } from '../../../lib/types';
  import { ui } from '../../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();
  const uid = $props.id();

  function edit(what: string, fn: (d: YDoc, s: YAMLMap) => void) {
    try {
      studio.editService(name, fn, `svc:${name}:${what}`);
    } catch (err) {
      ui.error(err);
    }
  }

  const mounts = $derived(getMounts(doc, svc));
  const declared = $derived(topLevelNames(doc, 'volumes'));
  let dockerVolumes = $state<string[]>([]);
  onMount(() => {
    api
      .get<{ volumes: VolumeRow[] }>('/volumes')
      .then((r) => (dockerVolumes = r.volumes.filter((v) => !v.anonymous).map((v) => v.name)))
      .catch(() => {});
  });

  const TYPES: { value: MountType; label: string }[] = [
    { value: 'bind', label: 'Folder' },
    { value: 'volume', label: 'Volume' },
    { value: 'tmpfs', label: 'Memory' },
  ];

  /** Where a folder ends up on the server. */
  function where(m: MountSpec): string | null {
    if (m.type !== 'bind' || !m.source) return null;
    const src = interpolate(m.source, (n) => studio.project.get(n)).value;
    if (src.startsWith('/')) return src === m.source ? null : src;
    if (src.startsWith('~')) return null;
    return studio.dir ? joinPath(studio.dir, src) : null;
  }

  function update(i: number, patch: Partial<MountSpec>) {
    const next = { ...mounts[i], ...patch };
    edit(`mount${i}`, (d, s) => {
      setMount(d, s, i, next);
      // Compose refuses named volumes nobody declared.
      if (next.type === 'volume' && next.source && !next.source.includes('$')) declareTopLevel(d, 'volumes', next.source);
    });
  }

  function setType(i: number, type: MountType) {
    const m = mounts[i];
    let source = m.source;
    if (type === 'tmpfs') source = '';
    else if (type === 'bind' && !/^[./~$]/.test(source)) source = `./${source || name}`;
    else if (type === 'volume' && /^[./~]/.test(source)) source = source.split('/').filter((p) => p && p !== '.' && p !== '..').pop() || `${name}_data`;
    update(i, { type, source, long: type === 'tmpfs' || m.long });
  }

  function add() {
    edit('mounts-add', (d, s) => setMount(d, s, -1, { type: 'bind', source: `./${name}`, target: '', readOnly: false, options: [], long: false }));
    setTimeout(() => document.querySelectorAll<HTMLInputElement>(`[data-mount-target="${uid}"]`).item(mounts.length)?.focus(), 30);
  }

  const tmpfs = $derived(getList(doc, svc, 'tmpfs'));
</script>

<Field label="Folders and volumes" hint="Folder: a path on the server (./name is next to the compose file). Volume: storage Docker manages. Memory: lost when the container stops." wide>
  <div class="mounts">
    {#if mounts.length}
      <div class="row head"><span>Type</span><span>On the server</span><span></span><span>In the container</span><span title="Read-only">RO</span><span></span></div>
    {/if}
    {#each mounts as m, i (i)}
      {@const full = where(m)}
      <div class="row">
        <select class="input" value={m.type} aria-label="Type {i + 1}" onchange={(e) => setType(i, e.currentTarget.value as MountType)}>
          {#each TYPES as t (t.value)}<option value={t.value}>{t.label}</option>{/each}
          {#if !TYPES.some((t) => t.value === m.type)}<option value={m.type}>{m.type}</option>{/if}
        </select>
        {#if m.type === 'tmpfs'}
          <span class="faint small center">in memory</span>
        {:else}
          <input
            class="input mono"
            value={m.source}
            placeholder={m.type === 'bind' ? `./${name}` : 'Anonymous volume'}
            list={m.type === 'volume' ? `${uid}-vols` : undefined}
            aria-label="Source {i + 1}"
            spellcheck="false"
            oninput={(e) => update(i, { source: e.currentTarget.value })}
          />
        {/if}
        <span class="arrow" aria-hidden="true">→</span>
        <input class="input mono" data-mount-target={uid} value={m.target} placeholder="/config" aria-label="Path in the container {i + 1}" spellcheck="false" oninput={(e) => update(i, { target: e.currentTarget.value })} />
        <label class="ro" title="Read-only">
          <input type="checkbox" checked={m.readOnly} aria-label="Read-only {i + 1}" onchange={(e) => update(i, { readOnly: e.currentTarget.checked })} />
        </label>
        <button type="button" class="btn ghost icon small" aria-label="Remove {m.target || 'mount'}" onclick={() => edit('mounts-remove', (d, s) => removeItem(d, s, 'volumes', i))}><X size={15} /></button>
        {#if full}<span class="full faint">{full}</span>{/if}
        {#if m.type === 'volume' && m.source && !declared.includes(m.source) && !m.source.includes('$')}
          <span class="full warn">Not declared under volumes yet; it will be when you change this row.</span>
        {/if}
      </div>
    {/each}
    <datalist id="{uid}-vols">{#each [...new Set([...declared, ...dockerVolumes])] as v (v)}<option value={v}></option>{/each}</datalist>
    <div><button type="button" class="btn small" onclick={add}><Plus size={15} />Add folder or volume</button></div>
  </div>
</Field>

<Field label="Memory folders" hint="Paths in the container kept in memory (tmpfs), like /tmp. Lost when it stops.">
  <StringList items={tmpfs} onchange={(items) => edit('tmpfs', (d, s) => setList(d, s, 'tmpfs', items))} placeholder="/tmp" label="Path" />
</Field>

<style>
  .mounts {
    display: grid;
    gap: 6px;
    max-width: 820px;
  }
  .row {
    display: grid;
    grid-template-columns: 112px minmax(0, 1.3fr) 18px minmax(0, 1fr) 30px 30px;
    gap: 6px;
    align-items: center;
  }
  .row.head {
    color: var(--text-3);
    font-size: var(--fs-xs);
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .arrow {
    color: var(--text-3);
    text-align: center;
  }
  .ro {
    display: flex;
    justify-content: center;
  }
  .center {
    text-align: center;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .full {
    grid-column: 2 / -1;
    margin-top: -2px;
    font: 400 var(--fs-xs) / 1.3 var(--font-code);
    overflow-wrap: anywhere;
  }
  .full.warn {
    color: var(--warn);
    font-family: var(--font-ui);
  }
  @media (max-width: 760px) {
    .row {
      grid-template-columns: minmax(0, 1fr) 18px minmax(0, 1fr) 30px;
    }
    .row.head {
      display: none;
    }
    .full {
      grid-column: 1 / -1;
    }
  }
</style>
