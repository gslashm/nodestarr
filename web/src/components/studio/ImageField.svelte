<!--
  A service's image: the reference, a tag picker fed by the registry, and
  what the image sets up by itself, with one-click suggestions (publish the
  ports it listens on, keep the folders it stores data in).
-->
<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import HardDrive from '@lucide/svelte/icons/hard-drive';
  import Network from '@lucide/svelte/icons/network';
  import Package from '@lucide/svelte/icons/package';
  import Tag from '@lucide/svelte/icons/tag';
  import Spinner from '../Spinner.svelte';
  import { setDotenv, dotenvMap } from '../../lib/compose/dotenv';
  import { images } from '../../lib/compose/images.svelte';
  import { planTagEdit, splitImage } from '../../lib/compose/imageref';
  import { interpolate } from '../../lib/compose/interpolate';
  import { get, getMounts, getPorts, set, setMount, setPort } from '../../lib/compose/service';
  import { useStudio } from '../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../lib/compose/yamldoc';
  import { ago } from '../../lib/format';
  import { ui } from '../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();

  const raw = $derived(String(get(doc, svc, 'image') ?? ''));
  const resolved = $derived(interpolate(raw, (n) => studio.project.get(n)).value);
  const parts = $derived(splitImage(resolved));
  const info = $derived(images.inspect[resolved.trim()]);
  const tags = $derived(images.tags[resolved.trim()]);
  let open = $state(false);
  let tagFilter = $state('');
  let root: HTMLDivElement | undefined = $state();

  // Look the image up once the reference stops changing.
  $effect(() => {
    const ref = resolved.trim();
    if (!ref) return;
    const t = setTimeout(() => images.loadInspect(ref), 500);
    return () => clearTimeout(t);
  });

  function setImage(value: string) {
    try {
      studio.editService(name, (d, s) => set(d, s, 'image', value), `svc:${name}:image`);
    } catch (err) {
      ui.error(err);
    }
  }

  function toggle() {
    open = !open;
    if (open) images.loadTags(resolved);
  }

  function pickTag(tag: string) {
    open = false;
    const plan = planTagEdit(raw, tag);
    if (!plan) {
      ui.toast("The image uses variables in a way the editor can't change; edit it directly", 'warning');
      return;
    }
    try {
      if (plan.kind === 'literal') setImage(plan.value);
      else {
        const env = studio.text(studio.envRel);
        let value = tag;
        if (plan.kind === 'image-var') {
          const current = dotenvMap(env).get(plan.name)?.value ?? '';
          const inner = planTagEdit(current, tag);
          if (!current || inner?.kind !== 'literal') {
            ui.toast(`Set ${plan.name} in ${studio.envRel} first`, 'warning');
            return;
          }
          value = inner.value;
        }
        studio.setText(studio.envRel, setDotenv(env, plan.name, value), `svc:${name}:tag`);
        ui.toast(`Changed ${plan.name} in ${studio.envRel}`, 'success');
      }
    } catch (err) {
      ui.error(err);
    }
  }

  const tagList = $derived.by(() => {
    if (tags?.state !== 'ok') return { channels: [], versions: [] };
    const q = tagFilter.trim().toLowerCase();
    const f = (t: { name: string }) => !q || t.name.toLowerCase().includes(q);
    return { channels: tags.data.channels.filter(f), versions: tags.data.versions.filter(f).slice(0, 80) };
  });

  // Suggestions from what the image declares.
  const published = $derived(new Set(getPorts(doc, svc).map((p) => `${p.target.split('-')[0]}/${p.protocol || 'tcp'}`)));
  const mounted = $derived(new Set(getMounts(doc, svc).map((m) => m.target.replace(/\/$/, ''))));
  const hostNet = $derived(String(get(doc, svc, 'network_mode') ?? '') === 'host');
  const portHints = $derived(info?.state === 'ok' && !hostNet ? info.data.defaults.ports.filter((p) => !published.has(p.includes('/') ? p : `${p}/tcp`)) : []);
  const volumeHints = $derived(info?.state === 'ok' ? info.data.defaults.volumes.filter((v) => !mounted.has(v.replace(/\/$/, ''))) : []);

  function publish(port: string) {
    const [num, proto = 'tcp'] = port.split('/');
    studio.editService(name, (d, s) => setPort(d, s, -1, { hostIp: '', published: num, target: num, protocol: proto === 'tcp' ? '' : proto, long: false }), `svc:${name}:suggest`);
  }

  function keep(target: string) {
    const leaf = target.split('/').filter(Boolean).pop() ?? 'data';
    const source = `./${name}${volumeHints.length > 1 || leaf !== 'config' ? `/${leaf}` : ''}`;
    studio.editService(name, (d, s) => setMount(d, s, -1, { type: 'bind', source, target, readOnly: false, options: [], long: false }), `svc:${name}:suggest`);
  }

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="image-field" bind:this={root}>
  <div class="line">
    <input
      class="input mono"
      value={raw}
      placeholder="repository/image:tag"
      aria-label="Image"
      spellcheck="false"
      autocomplete="off"
      oninput={(e) => setImage(e.currentTarget.value)}
    />
    <button
      type="button"
      class="btn tag-btn"
      onclick={toggle}
      aria-expanded={open}
      aria-haspopup="listbox"
      aria-label="Tag {parts.tag || 'latest'}: pick another"
      disabled={!resolved.trim()}
      title="Pick a tag"
    >
      <Tag size={15} /><span class="mono">{parts.tag || 'latest'}</span><ChevronDown size={14} />
    </button>
  </div>
  {#if raw !== resolved && raw}
    <span class="sub faint">becomes <span class="mono">{resolved}</span></span>
  {/if}

  {#if open}
    <div class="tags" role="listbox" aria-label="Tags">
      <input class="input" placeholder="Filter tags" bind:value={tagFilter} aria-label="Filter tags" />
      {#if !tags || tags.state === 'loading'}
        <p class="faint pad"><Spinner size={13} /> Asking the registry…</p>
      {:else if tags.state === 'error'}
        <p class="text-bad pad">{tags.message}</p>
      {:else}
        <div class="list">
          {#if tagList.channels.length}<p class="group">Channels</p>{/if}
          {#each tagList.channels as t (t.name)}
            <button type="button" role="option" aria-selected={t.name === parts.tag} class:current={t.name === (parts.tag || 'latest')} onclick={() => pickTag(t.name)}>
              <span class="mono">{t.name}</span>{#if t.updated}<span class="faint">{ago(t.updated)}</span>{/if}
            </button>
          {/each}
          {#if tagList.versions.length}<p class="group">Versions</p>{/if}
          {#each tagList.versions as t (t.name)}
            <button type="button" role="option" aria-selected={t.name === parts.tag} class:current={t.name === parts.tag} onclick={() => pickTag(t.name)}>
              <span class="mono">{t.name}</span>{#if t.updated}<span class="faint">{ago(t.updated)}</span>{/if}
            </button>
          {/each}
          {#if !tagList.channels.length && !tagList.versions.length}<p class="faint pad">No tags match.</p>{/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if info?.state === 'ok'}
    {@const d = info.data}
    <div class="card">
      <div class="head">
        <Package size={15} />
        <span class="title">{d.title || splitImage(d.image).repo.split('/').pop()}</span>
        {#if d.version}<span class="chip">{d.version}</span>{/if}
        <span class="faint small">{d.source === 'local' ? 'on this machine' : 'from the registry'}{d.created ? `, built ${ago(d.created)}` : ''}</span>
        <span class="grow"></span>
        {#if d.url}<a class="faint small" href={d.url} target="_blank" rel="noopener">Source <ExternalLink size={12} /></a>{/if}
      </div>
      {#if d.description}<p class="desc muted">{d.description}</p>{/if}
      {#if portHints.length || volumeHints.length}
        <div class="hints">
          {#each portHints as p (p)}
            <button type="button" class="hint" onclick={() => publish(p)}><Network size={14} />Publish {p}</button>
          {/each}
          {#each volumeHints as v (v)}
            <button type="button" class="hint" onclick={() => keep(v)}><HardDrive size={14} />Keep {v} in a folder</button>
          {/each}
        </div>
      {/if}
    </div>
  {:else if info?.state === 'error' && resolved}
    <p class="sub faint">Couldn't look this image up: {info.message}</p>
  {/if}
</div>

<style>
  .image-field {
    position: relative;
    display: grid;
    gap: 6px;
  }
  .line {
    display: flex;
    gap: 6px;
  }
  .line .input {
    flex: 1;
    min-width: 0;
  }
  .tag-btn {
    max-width: 45%;
    gap: 6px;
  }
  .tag-btn .mono {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: var(--fs-xs);
  }
  .tags {
    position: absolute;
    top: 40px;
    right: 0;
    z-index: 30;
    display: grid;
    gap: 6px;
    width: min(360px, 100%);
    padding: 8px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .tags .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .list {
    display: grid;
    max-height: 320px;
    overflow-y: auto;
  }
  .list button {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .list button:hover {
    background: var(--raised);
  }
  .list button.current {
    background: var(--accent-soft);
  }
  .group {
    margin: 6px 8px 2px;
    color: var(--text-3);
    font-size: var(--fs-xs);
  }
  .pad {
    margin: 0;
    padding: 6px 8px;
  }
  .card {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--raised) 55%, transparent);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .head :global(svg) {
    color: var(--text-3);
  }
  .title {
    font-weight: 600;
  }
  .small {
    font-size: var(--fs-xs);
  }
  .desc {
    margin: 0;
    font-size: var(--fs-sm);
  }
  .hints {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border: 1px dashed color-mix(in srgb, var(--accent) 55%, var(--line-strong));
    border-radius: 14px;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .hint:hover {
    background: var(--accent-soft);
  }
  .hint :global(svg) {
    color: var(--accent-text);
  }
</style>
