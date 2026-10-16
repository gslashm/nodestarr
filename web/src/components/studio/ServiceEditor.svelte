<!--
  One service of a stack as a form: what it runs, its environment, ports,
  storage, network, health check, resources and everything else, with the
  YAML it's written as alongside.
-->
<script lang="ts">
  import type { Component } from 'svelte';
  import Code from '@lucide/svelte/icons/code';
  import CopyPlus from '@lucide/svelte/icons/copy-plus';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import FileCode from '@lucide/svelte/icons/file-code';
  import PanelRight from '@lucide/svelte/icons/panel-right';
  import PanelRightClose from '@lucide/svelte/icons/panel-right-close';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import AppIcon from '../AppIcon.svelte';
  import Menu, { type MenuEntry } from '../Menu.svelte';
  import StatePill from '../StatePill.svelte';
  import EnvEditor from './EnvEditor.svelte';
  import YamlPreview from './YamlPreview.svelte';
  import Advanced from './sections/Advanced.svelte';
  import General from './sections/General.svelte';
  import Health from './sections/Health.svelte';
  import Network from './sections/Network.svelte';
  import Other from './sections/Other.svelte';
  import Ports from './sections/Ports.svelte';
  import Resources from './sections/Resources.svelte';
  import Storage from './sections/Storage.svelte';
  import { serviceEnv } from '../../lib/compose/environment';
  import { appSlug } from '../../lib/compose/templates';
  import { duplicateService, FORM_KEYS, get, getMounts, getPorts, rawKeys, removeService, renameService, serviceMap } from '../../lib/compose/service';
  import { useStudio } from '../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../lib/compose/yamldoc';
  import { stateOf } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { ui } from '../../lib/ui.svelte';

  let { name, scoped = false }: { name: string; scoped?: boolean } = $props();
  const studio = useStudio();

  type SectionProps = { name: string; doc: YDoc; svc: YAMLMap };
  const SECTIONS: { key: string; label: string; component: Component<SectionProps>; keys: string[] }[] = [
    { key: 'general', label: 'General', component: General, keys: ['image', 'build', 'container_name', 'restart', 'command', 'depends_on'] },
    { key: 'environment', label: 'Environment', component: EnvEditor, keys: ['environment', 'env_file'] },
    { key: 'ports', label: 'Ports', component: Ports, keys: ['ports', 'expose'] },
    { key: 'storage', label: 'Storage', component: Storage, keys: ['volumes', 'tmpfs'] },
    { key: 'network', label: 'Network', component: Network, keys: ['network_mode', 'networks', 'hostname', 'dns', 'extra_hosts'] },
    { key: 'health', label: 'Health', component: Health, keys: ['healthcheck'] },
    { key: 'resources', label: 'Resources', component: Resources, keys: ['mem_limit', 'cpus', 'pids_limit', 'gpus', 'devices', 'group_add', 'shm_size', 'deploy'] },
    { key: 'advanced', label: 'Advanced', component: Advanced, keys: [] },
    { key: 'other', label: 'Other', component: Other, keys: [] },
  ];

  const file = $derived(studio.serviceFile(name));
  const doc = $derived(studio.doc(file));
  const svc = $derived(doc ? serviceMap(doc, name) : null);
  const yamlError = $derived(studio.yamlErrors[file]);
  const overrides = $derived(studio.overridesOf(name));

  const section = $derived.by(() => {
    const sel = studio.selection;
    const want = sel.kind === 'service' ? sel.section : undefined;
    return want && SECTIONS.some((s) => s.key === want) ? want : 'general';
  });
  const current = $derived(SECTIONS.find((s) => s.key === section)!);
  const Section = $derived(current.component);

  const image = $derived(doc && svc ? String(get(doc, svc, 'image') ?? '') : '');
  const built = $derived(doc && svc ? get(doc, svc, 'build') !== undefined : false);
  const container = $derived(live.list.find((c) => c.stack === studio.name && c.service === name) ?? null);
  const slug = $derived(container?.app ?? appSlug(image));

  const counts = $derived.by((): Record<string, number> => {
    if (!doc || !svc) return {};
    const env = serviceEnv({ composes: studio.docs, service: name, project: studio.project, envFiles: studio.envFileTexts(name) });
    return {
      environment: env.rows.length,
      ports: getPorts(doc, svc).length,
      storage: getMounts(doc, svc).length,
      other: rawKeys(doc, svc).length,
    };
  });

  const focusKeys = $derived.by(() => {
    if (!doc || !svc) return [];
    if (section === 'other') return rawKeys(doc, svc);
    if (section === 'advanced') {
      const shown = new Set(SECTIONS.flatMap((s) => s.keys));
      return [...FORM_KEYS].filter((k) => !shown.has(k));
    }
    return current.keys;
  });

  let preview = $state(typeof window !== 'undefined' && window.innerWidth >= 1500);

  function go(key: string) {
    studio.select({ kind: 'service', name, section: key });
  }

  let renaming = $state(false);
  let newName = $state('');

  function rename(e: Event) {
    e.preventDefault();
    const to = newName.trim();
    renaming = false;
    if (!to || to === name) return;
    if (!/^[a-zA-Z0-9][a-zA-Z0-9_.-]*$/.test(to)) {
      ui.toast('Service names use letters, digits, ., - and _', 'warning');
      return;
    }
    try {
      for (const f of studio.composeRels) studio.editCompose(f, (d) => renameService(d, name, to), `rename:${name}`);
      studio.select({ kind: 'service', name: to, section });
      if (container) ui.toast(`Deploying replaces the ${name} container with ${to}`, 'info');
    } catch (err) {
      ui.error(err);
    }
  }

  function duplicate() {
    let to = `${name}-2`;
    for (let i = 2; studio.services.includes(to); i++) to = `${name}-${i}`;
    try {
      studio.editCompose(file, (d) => duplicateService(d, name, to), `duplicate:${name}`);
      studio.select({ kind: 'service', name: to });
      ui.toast(`Copied to ${to}. Change its ports and folders so they don't clash.`, 'success');
    } catch (err) {
      ui.error(err);
    }
  }

  async function remove() {
    const ok = await ui.confirm({
      title: `Remove ${name} from the stack?`,
      message: container ? 'Its container keeps running until you deploy; deploying with "remove orphans" or taking the stack down removes it.' : 'Services that start after it no longer wait for it.',
      confirmLabel: 'Remove service',
      danger: true,
    });
    if (!ok) return;
    try {
      for (const f of studio.composeRels) studio.editCompose(f, (d) => removeService(d, name), `remove:${name}`);
      studio.select({ kind: 'stack' });
    } catch (err) {
      ui.error(err);
    }
  }

  const menu = $derived<MenuEntry[]>([
    { label: 'Rename', icon: Pencil, run: () => ((newName = name), (renaming = true)) },
    { label: 'Duplicate', icon: CopyPlus, run: duplicate },
    ...(container ? [{ label: 'Open the container', icon: ExternalLink, href: href(`/containers/${encodeURIComponent(container.name)}`) }] : []),
    { label: `Show ${file}`, icon: FileCode, run: () => studio.select({ kind: 'file', rel: file }) },
    'separator',
    { label: 'Remove from the stack', icon: Trash2, danger: true, run: remove },
  ]);
</script>

<div class="service" class:with-preview={preview && !!svc}>
  <div class="main">
    <header class="head">
      <AppIcon name={name} {slug} icon={container?.prefs.icon ?? ''} size={40} />
      <div class="titles">
        {#if renaming}
          <form onsubmit={rename}>
            <!-- svelte-ignore a11y_autofocus -->
            <input class="input mono rename" bind:value={newName} aria-label="New service name" autofocus onblur={rename} spellcheck="false" />
          </form>
        {:else}
          <h2>{name}</h2>
        {/if}
        <span class="sub mono">{built ? 'Built from a Dockerfile' : image || 'No image yet'}</span>
      </div>
      {#if container}<StatePill state={stateOf(container)} />{/if}
      <span class="grow"></span>
      {#if svc}
        <button type="button" class="btn small ghost" onclick={() => (preview = !preview)} title={preview ? 'Hide the YAML' : 'Show the YAML next to the form'}>
          {#if preview}<PanelRightClose size={15} />{:else}<PanelRight size={15} />{/if}YAML
        </button>
      {/if}
      {#if !scoped}<Menu items={menu} label="Actions for {name}" />{/if}
    </header>

    {#if yamlError}
      <div class="banner bad">
        <TriangleAlert size={16} />
        <span>{file} has a YAML error, so the form can't read it: {yamlError}</span>
        <button type="button" class="btn small" onclick={() => studio.select({ kind: 'file', rel: file })}><Code size={14} />Fix it in the code</button>
      </div>
    {:else if !svc || !doc}
      <div class="banner">This service isn't in {file} any more.</div>
    {:else}
      {#if overrides.length}
        <div class="banner">
          <span>
            {#each overrides as o (o.file)}<span class="mono">{o.file}</span> also sets {o.keys.join(', ')} for {name}; its values win.{' '}{/each}
          </span>
          <button type="button" class="btn small ghost" onclick={() => studio.select({ kind: 'file', rel: overrides[0].file })}>Open it</button>
        </div>
      {/if}

      <nav class="sections" aria-label="Settings of {name}">
        {#each SECTIONS as s (s.key)}
          {#if s.key !== 'other' || counts.other}
            <button type="button" class:active={s.key === section} aria-current={s.key === section ? 'page' : undefined} onclick={() => go(s.key)}>
              {s.label}
              {#if counts[s.key]}<span class="count">{counts[s.key]}</span>{/if}
            </button>
          {/if}
        {/each}
        {#if !counts.other}<button type="button" class:active={section === 'other'} onclick={() => go('other')} title="Settings as YAML">YAML…</button>{/if}
      </nav>

      <section class="panel body">
        {#key `${name}:${section}`}
          <Section {name} {doc} {svc} />
        {/key}
      </section>
    {/if}
  </div>

  {#if preview && svc}
    <aside class="side">
      <YamlPreview rel={file} service={name} keys={focusKeys} />
    </aside>
  {/if}
</div>

<style>
  .service {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 16px;
    align-items: start;
  }
  .service.with-preview {
    grid-template-columns: minmax(0, 1fr) minmax(320px, 0.62fr);
  }
  .main {
    display: grid;
    gap: 12px;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .titles {
    display: grid;
    min-width: 0;
  }
  h2 {
    font-size: var(--fs-lg);
    overflow-wrap: anywhere;
  }
  .rename {
    min-height: 32px;
    padding: 4px 8px;
    font-size: var(--fs-md);
  }
  .sub {
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .banner.bad {
    border-color: color-mix(in srgb, var(--bad) 45%, var(--line));
  }
  .banner.bad :global(svg) {
    color: var(--bad);
  }
  .banner span {
    flex: 1;
    min-width: 0;
  }
  .sections {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .sections button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 11px;
    border: 1px solid var(--line);
    border-radius: 16px;
    background: var(--panel);
    color: var(--text-2);
    font: 500 var(--fs-sm) / 1 var(--font-ui);
    cursor: pointer;
  }
  .sections button:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
  .sections button.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
  }
  .count {
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--raised);
    color: var(--text-2);
    font: 600 var(--fs-xs) / 17px var(--font-data);
    text-align: center;
  }
  .body {
    padding: 4px 16px 12px;
  }
  .side {
    position: sticky;
    top: 124px;
    height: calc(100vh - 210px);
    min-height: 320px;
  }
  @media (max-width: 1100px) {
    .service.with-preview {
      grid-template-columns: minmax(0, 1fr);
    }
    .side {
      position: static;
      height: 420px;
    }
  }
</style>
