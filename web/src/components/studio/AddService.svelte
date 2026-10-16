<!--
  Adds a service to the stack: from a gallery of common apps, from a
  `docker run` command, from a standalone container already on the server,
  or blank. Apps get a short setup step: the name, free host ports and the
  stack variables they use (passwords are generated).
-->
<script lang="ts">
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import Box from '@lucide/svelte/icons/box';
  import Container from '@lucide/svelte/icons/container';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Search from '@lucide/svelte/icons/search';
  import SquareTerminal from '@lucide/svelte/icons/square-terminal';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import AppIcon from '../AppIcon.svelte';
  import Button from '../Button.svelte';
  import { api, seg } from '../../lib/api';
  import { dotenvMap, setDotenv } from '../../lib/compose/dotenv';
  import { formatPort, parsePort, addService, declareTopLevel, serviceMap } from '../../lib/compose/service';
  import { useStudio } from '../../lib/compose/studio.svelte';
  import { generatePassword, instantiate, SERVICE_TEMPLATES, stackVariables, appSlug, type ServiceTemplate } from '../../lib/compose/templates';
  import { parseYaml } from '../../lib/compose/yamldoc';
  import { live } from '../../lib/live.svelte';
  import type { ContainerSpec } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { onadded, compact = false }: { onadded: (name: string) => void; compact?: boolean } = $props();
  const studio = useStudio();

  type Way = 'apps' | 'run' | 'container' | 'blank';
  let way = $state<Way>('apps');
  let query = $state('');
  let category = $state('');
  let chosen = $state.raw<ServiceTemplate | null>(null);
  /** A service body from a run command or container, waiting for a name. */
  let converted = $state.raw<{ body: Record<string, unknown>; from: string; warnings: string[] } | null>(null);
  let name = $state('');
  let ports = $state<Record<string, string>>({});
  let vars = $state<Record<string, string>>({});
  let runCommand = $state('');
  let blankImage = $state('');
  let busy = $state(false);

  const categories = [...new Set(SERVICE_TEMPLATES.map((t) => t.category))];
  const shown = $derived(
    SERVICE_TEMPLATES.filter((t) => (!category || t.category === category) && (!query.trim() || `${t.name} ${t.description} ${t.slug}`.toLowerCase().includes(query.trim().toLowerCase()))),
  );
  const env = $derived(dotenvMap(studio.text(studio.envRel)));
  const standalone = $derived(live.list.filter((c) => !c.stack && !c.is_self));

  function unique(base: string): string {
    const clean = base.toLowerCase().replace(/[^a-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'app';
    if (!studio.services.includes(clean)) return clean;
    for (let i = 2; ; i++) if (!studio.services.includes(`${clean}-${i}`)) return `${clean}-${i}`;
  }

  /** Host ports used anywhere on the server or in this stack. */
  const usedPorts = $derived.by(() => {
    const s = new Set<string>();
    for (const c of live.list) for (const p of c.ports) if (p.public_port) s.add(String(p.public_port));
    for (const { doc } of studio.docs) {
      for (const svcName of studio.services) {
        const svc = serviceMap(doc, svcName);
        const list = (svc?.toJS(doc) as { ports?: unknown[] } | undefined)?.ports ?? [];
        for (const raw of list) {
          const p = typeof raw === 'string' || typeof raw === 'number' ? parsePort(String(raw)) : null;
          if (p?.published) s.add(p.published);
        }
      }
    }
    return s;
  });

  function freePort(want: string): string {
    if (!/^\d+$/.test(want) || !usedPorts.has(want)) return want;
    for (let n = Number(want) + 1; n < Number(want) + 500; n++) if (!usedPorts.has(String(n))) return String(n);
    return want;
  }

  function choose(t: ServiceTemplate) {
    chosen = t;
    name = unique(t.slug);
    const p: Record<string, string> = {};
    for (const raw of (t.service.ports as string[] | undefined) ?? []) {
      const spec = parsePort(raw);
      if (spec?.published && !(spec.published in p)) p[spec.published] = freePort(spec.published);
    }
    ports = p;
    const known = stackVariables();
    const v: Record<string, string> = {};
    for (const n of t.vars ?? []) {
      if (env.has(n)) continue;
      v[n] = known[n]?.secret ? generatePassword() : (known[n]?.value ?? '');
    }
    vars = v;
  }

  const nameOk = $derived(/^[a-z0-9][a-z0-9_.-]*$/.test(name) && !studio.services.includes(name));

  function addToStack(body: Record<string, unknown>, extra: { volumes?: string[]; files?: { path: string; content: string }[]; vars?: Record<string, string>; note?: string }) {
    const main = studio.composeRels[0] ?? 'compose.yaml';
    const label = `add:${name}`;
    try {
      studio.editCompose(
        main,
        (doc) => {
          addService(doc, name, body);
          for (const v of extra.volumes ?? []) declareTopLevel(doc, 'volumes', v);
        },
        label,
      );
      let envText = studio.text(studio.envRel);
      for (const [k, v] of Object.entries(extra.vars ?? {})) if (!env.has(k)) envText = setDotenv(envText, k, v, { literal: v.includes('$') });
      if (envText !== studio.text(studio.envRel)) studio.setText(studio.envRel, envText, label);
      for (const f of extra.files ?? []) {
        const path = instantiate(f.path, name);
        if (!studio.files[path]) studio.addFile(path, f.content);
      }
      if (extra.note) ui.toast(extra.note, 'info');
      onadded(name);
    } catch (err) {
      ui.error(err, "Couldn't add it");
    }
  }

  function addTemplate(e: Event) {
    e.preventDefault();
    if (!chosen || !nameOk) return;
    const body = instantiate(structuredClone(chosen.service), name);
    if (Array.isArray(body.ports)) {
      body.ports = (body.ports as string[]).map((raw) => {
        const spec = parsePort(raw);
        return spec?.published && ports[spec.published] ? formatPort({ ...spec, published: ports[spec.published] }) : raw;
      });
    }
    addToStack(body, { volumes: chosen.volumes, files: chosen.files, vars, note: chosen.note });
  }

  /** The first service of a compose YAML text the server produced. */
  function firstService(yaml: string): [string, Record<string, unknown>] {
    const data = parseYaml(yaml).toJS() as { services?: Record<string, Record<string, unknown>> };
    const entry = Object.entries(data.services ?? {})[0];
    if (!entry) throw new Error('The conversion produced no service');
    return entry;
  }

  async function convertRun() {
    busy = true;
    try {
      const parsed = await api.post<{ spec: ContainerSpec; warnings: string[] }>('/containers/parse-run', { command: runCommand });
      const r = await api.post<{ yaml: string }>('/containers/to-compose', parsed.spec);
      const [svcName, body] = firstService(r.yaml);
      converted = { body, from: 'the command', warnings: parsed.warnings };
      name = unique(svcName);
    } catch (err) {
      ui.error(err, "Couldn't read that command");
    } finally {
      busy = false;
    }
  }

  async function adopt(id: string, display: string) {
    busy = true;
    try {
      const s = await api.get<{ spec: ContainerSpec }>(`/containers/${seg(id)}/spec`);
      const r = await api.post<{ yaml: string }>('/containers/to-compose', s.spec);
      const [svcName, body] = firstService(r.yaml);
      converted = {
        body,
        from: display,
        warnings: [`The ${display} container keeps running. Remove it before deploying, or the two will clash over the name and ports.`],
      };
      name = unique(svcName);
    } catch (err) {
      ui.error(err, "Couldn't read that container");
    } finally {
      busy = false;
    }
  }

  function addConverted(e: Event) {
    e.preventDefault();
    if (!converted || !nameOk) return;
    const body = { ...converted.body };
    // The stack names its containers; keep a name only when it differs from the service.
    if (body.container_name === name) delete body.container_name;
    addToStack(body, {});
  }

  function addBlank(e: Event) {
    e.preventDefault();
    if (!nameOk || !blankImage.trim()) return;
    addToStack({ image: blankImage.trim(), restart: 'unless-stopped' }, {});
  }

  function back() {
    chosen = null;
    converted = null;
  }
</script>

<div class="add" class:compact>
  {#if chosen}
    <form class="setup" onsubmit={addTemplate}>
      <button type="button" class="btn small ghost back" onclick={back}><ArrowLeft size={15} />All apps</button>
      <div class="hero">
        <AppIcon name={chosen.name} slug={chosen.slug} size={44} />
        <div>
          <h3>{chosen.name}</h3>
          <span class="muted">{chosen.description} · <span class="mono">{chosen.service.image}</span></span>
        </div>
      </div>
      <label class="fld">
        <span>Service name</span>
        <input class="input mono" bind:value={name} spellcheck="false" aria-invalid={!nameOk} />
        {#if !nameOk}<span class="err">{studio.services.includes(name) ? 'Another service has this name.' : 'Use lowercase letters, digits, ., - and _.'}</span>{/if}
      </label>
      {#if Object.keys(ports).length}
        <div class="fld">
          <span>Ports on the server</span>
          <div class="ports">
            {#each Object.keys(ports) as p (p)}
              <label class="port">
                <input class="input mono" bind:value={ports[p]} inputmode="numeric" aria-label="Server port for {p}" />
                <span class="faint">→ {p}</span>
                {#if ports[p] !== p}<span class="faint small">{p} is taken</span>{/if}
              </label>
            {/each}
          </div>
        </div>
      {/if}
      {#if Object.keys(vars).length}
        <div class="fld">
          <span>Stack variables <span class="faint">(saved in {studio.envRel}, shared with other services)</span></span>
          {#each Object.keys(vars) as v (v)}
            {@const meta = stackVariables()[v]}
            <label class="var">
              <span class="mono">{v}</span>
              <input class="input mono" bind:value={vars[v]} spellcheck="false" aria-label={meta?.label ?? v} placeholder={meta?.label ?? ''} />
              {#if meta?.secret}<button type="button" class="btn ghost icon small" aria-label="Generate another" title="Generate another" onclick={() => (vars[v] = generatePassword())}><RefreshCw size={14} /></button>{/if}
            </label>
            {#if meta?.label && !meta.secret}<span class="faint small hint">{meta.label}</span>{/if}
          {/each}
        </div>
      {/if}
      {#if (chosen.vars ?? []).some((v) => env.has(v))}
        <p class="faint small">Uses {(chosen.vars ?? []).filter((v) => env.has(v)).join(', ')} from {studio.envRel} as they are.</p>
      {/if}
      <div class="go"><Button type="submit" variant="primary" disabled={!nameOk}>Add {name || 'service'}</Button></div>
    </form>
  {:else if converted}
    <form class="setup" onsubmit={addConverted}>
      <button type="button" class="btn small ghost back" onclick={back}><ArrowLeft size={15} />Back</button>
      <div class="hero">
        <AppIcon name={name} slug={appSlug(String(converted.body.image ?? ''))} size={44} />
        <div>
          <h3>From {converted.from}</h3>
          <span class="muted mono">{converted.body.image}</span>
        </div>
      </div>
      {#each converted.warnings as w (w)}<p class="warn"><TriangleAlert size={14} />{w}</p>{/each}
      <label class="fld">
        <span>Service name</span>
        <input class="input mono" bind:value={name} spellcheck="false" aria-invalid={!nameOk} />
      </label>
      <p class="faint small">Ports, folders and variables come over as they are; change them in the service's form after adding it.</p>
      <div class="go"><Button type="submit" variant="primary" disabled={!nameOk}>Add {name || 'service'}</Button></div>
    </form>
  {:else}
    <div class="ways" role="tablist" aria-label="How to add a service">
      <button type="button" role="tab" aria-selected={way === 'apps'} class:active={way === 'apps'} onclick={() => (way = 'apps')}><Box size={15} />Apps</button>
      <button type="button" role="tab" aria-selected={way === 'run'} class:active={way === 'run'} onclick={() => (way = 'run')}><SquareTerminal size={15} />docker run</button>
      <button type="button" role="tab" aria-selected={way === 'container'} class:active={way === 'container'} onclick={() => (way = 'container')}><Container size={15} />A container</button>
      <button type="button" role="tab" aria-selected={way === 'blank'} class:active={way === 'blank'} onclick={() => ((way = 'blank'), (name = name || unique('app')))}>Blank</button>
    </div>

    {#if way === 'apps'}
      <div class="filters">
        <div class="search">
          <Search size={15} />
          <input class="input" placeholder="Search apps" bind:value={query} aria-label="Search apps" />
        </div>
        <div class="cats">
          <button type="button" class="chip" class:on={!category} onclick={() => (category = '')}>All</button>
          {#each categories as c (c)}<button type="button" class="chip" class:on={category === c} onclick={() => (category = c)}>{c}</button>{/each}
        </div>
      </div>
      <div class="grid">
        {#each shown as t (t.slug)}
          <button type="button" class="app" onclick={() => choose(t)}>
            <AppIcon name={t.name} slug={t.slug} size={36} />
            <span class="txt">
              <span class="nm">{t.name}</span>
              <span class="ds">{t.description}</span>
            </span>
          </button>
        {:else}
          <p class="faint">No app matches. Try “Blank” and type the image.</p>
        {/each}
      </div>
    {:else if way === 'run'}
      <div class="pane">
        <p class="muted">Paste a <code>docker run</code> command from an app's documentation; it becomes a service.</p>
        <textarea class="input mono" rows="5" bind:value={runCommand} spellcheck="false" placeholder="docker run -d --name=sonarr -p 8989:8989 -v /opt/appdata/sonarr:/config lscr.io/linuxserver/sonarr:latest"></textarea>
        <div class="go"><Button variant="primary" busy={busy} disabled={!runCommand.trim()} onclick={convertRun}>Convert</Button></div>
      </div>
    {:else if way === 'container'}
      <div class="pane">
        <p class="muted">Bring a container you started by hand into this stack, with its settings.</p>
        {#if standalone.length}
          <div class="list">
            {#each standalone as c (c.id)}
              <button type="button" class="app" disabled={busy} onclick={() => adopt(c.id, c.display_name)}>
                <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={30} />
                <span class="txt"><span class="nm">{c.display_name}</span><span class="ds mono">{c.image}</span></span>
              </button>
            {/each}
          </div>
        {:else}
          <p class="faint">Every container on this server already belongs to a stack.</p>
        {/if}
      </div>
    {:else}
      <form class="pane" onsubmit={addBlank}>
        <label class="fld"><span>Service name</span><input class="input mono" bind:value={name} spellcheck="false" aria-invalid={!nameOk} /></label>
        <label class="fld"><span>Image</span><input class="input mono" bind:value={blankImage} placeholder="repository/image:tag" spellcheck="false" /></label>
        <div class="go"><Button type="submit" variant="primary" disabled={!nameOk || !blankImage.trim()}>Add {name || 'service'}</Button></div>
      </form>
    {/if}
  {/if}
</div>

<style>
  .add {
    display: grid;
    gap: 12px;
  }
  .ways {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .ways button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--line);
    border-radius: 16px;
    background: var(--panel);
    color: var(--text-2);
    font: 500 var(--fs-sm) / 1 var(--font-ui);
    cursor: pointer;
  }
  .ways button.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    width: min(260px, 100%);
  }
  .search :global(svg) {
    position: absolute;
    left: 9px;
    color: var(--text-3);
  }
  .search .input {
    min-height: 32px;
    padding: 4px 8px 4px 30px;
  }
  .cats {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .cats .chip {
    cursor: pointer;
  }
  .cats .chip.on {
    border-color: var(--accent);
    color: var(--text);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 8px;
    max-height: 460px;
    overflow-y: auto;
    padding: 2px;
  }
  .compact .grid {
    max-height: 380px;
  }
  .app {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms var(--ease);
  }
  .app:hover {
    border-color: var(--accent);
  }
  .txt {
    display: grid;
    min-width: 0;
  }
  .nm {
    font-weight: 600;
  }
  .ds {
    overflow: hidden;
    color: var(--text-2);
    font-size: var(--fs-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 8px;
  }
  .pane {
    display: grid;
    gap: 10px;
    max-width: 720px;
  }
  .pane p {
    margin: 0;
  }
  .setup {
    display: grid;
    gap: 14px;
    max-width: 620px;
  }
  .back {
    justify-self: start;
  }
  .hero {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .hero h3 {
    font-size: var(--fs-lg);
  }
  .fld {
    display: grid;
    gap: 6px;
  }
  .fld > span:first-child {
    font-weight: 500;
  }
  .err {
    color: var(--bad);
    font-size: var(--fs-sm);
  }
  .ports {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
  }
  .port {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .port .input {
    width: 90px;
    min-height: 32px;
    padding: 4px 8px;
  }
  .var {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
  }
  .var .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .hint {
    margin: -4px 0 0 208px;
  }
  .small {
    font-size: var(--fs-xs);
  }
  .warn {
    display: flex;
    gap: 8px;
    margin: 0;
    color: var(--warn);
    font-size: var(--fs-sm);
  }
  .warn :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .go {
    display: flex;
    justify-content: flex-end;
  }
  @media (max-width: 760px) {
    .var {
      grid-template-columns: 1fr auto;
    }
    .var .mono:first-child {
      grid-column: 1 / -1;
    }
    .hint {
      margin-left: 0;
    }
  }
</style>
