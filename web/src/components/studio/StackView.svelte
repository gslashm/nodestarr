<!--
  The stack as a whole: its services at a glance, problems found while
  editing (with fixes), and the networks and volumes it declares.
-->
<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Plus from '@lucide/svelte/icons/plus';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Wrench from '@lucide/svelte/icons/wrench';
  import AppIcon from '../AppIcon.svelte';
  import StateDot from '../StateDot.svelte';
  import { stackProblems, type Problem } from '../../lib/compose/problems';
  import { declareTopLevel, get, getMounts, getNetworks, getPorts, serviceMap, TOP_ORDER } from '../../lib/compose/service';
  import { useStudio } from '../../lib/compose/studio.svelte';
  import { appSlug } from '../../lib/compose/templates';
  import { deref, ensureMap, isMap, keysOf, pairOf, setKey, toJs, type YAMLMap, type YDoc } from '../../lib/compose/yamldoc';
  import { plural, stateOf } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { ui } from '../../lib/ui.svelte';

  const studio = useStudio();

  const problems = $derived(stackProblems(studio.docs, studio.project, studio.usage));
  const main = $derived(studio.composeRels[0] ?? 'compose.yaml');
  const mainDoc = $derived(studio.doc(main));

  interface Tile {
    name: string;
    image: string;
    ports: string[];
    mounts: number;
    slug: string | null;
  }

  const tiles = $derived.by((): Tile[] =>
    studio.services.map((name) => {
      const file = studio.serviceFile(name);
      const doc = studio.doc(file);
      const svc = doc ? serviceMap(doc, name) : null;
      const image = doc && svc ? String(get(doc, svc, 'image') ?? (get(doc, svc, 'build') !== undefined ? 'built here' : '')) : '';
      const container = live.list.find((c) => c.stack === studio.name && c.service === name);
      return {
        name,
        image,
        ports: doc && svc ? getPorts(doc, svc).filter((p) => p.published).map((p) => p.published) : [],
        mounts: doc && svc ? getMounts(doc, svc).length : 0,
        slug: container?.app ?? appSlug(image),
      };
    }),
  );

  interface TopEntry {
    name: string;
    external: boolean;
    driver: string;
    usedBy: string[];
  }

  function top(kind: 'networks' | 'volumes'): TopEntry[] {
    if (!mainDoc || !isMap(mainDoc.contents)) return [];
    const m = deref(mainDoc, pairOf(mainDoc.contents as unknown as YAMLMap, kind)?.value);
    if (!isMap(m)) return [];
    return keysOf(m).map((name) => {
      const v = (toJs(mainDoc, pairOf(m, name)?.value) as Record<string, unknown> | null) ?? {};
      const usedBy = studio.services.filter((s) => {
        const doc = studio.doc(studio.serviceFile(s));
        const svc = doc ? serviceMap(doc, s) : null;
        if (!doc || !svc) return false;
        return kind === 'volumes' ? getMounts(doc, svc).some((x) => x.type === 'volume' && x.source === name) : getNetworks(doc, svc).some((x) => x.name === name);
      });
      return { name, external: v.external === true, driver: String(v.driver ?? ''), usedBy };
    });
  }

  const networks = $derived(top('networks'));
  const volumes = $derived(top('volumes'));

  function editTop(kind: 'networks' | 'volumes', fn: (m: YAMLMap, doc: YDoc) => void, what: string) {
    try {
      studio.editCompose(
        main,
        (doc) => {
          const root = doc.contents as unknown as YAMLMap;
          const m = ensureMap(doc, root, kind, TOP_ORDER);
          fn(m, doc);
          if (!m.items.length) setKey(doc, root, kind, undefined);
        },
        `top:${kind}:${what}`,
      );
    } catch (err) {
      ui.error(err);
    }
  }

  function setExternal(kind: 'networks' | 'volumes', name: string, external: boolean) {
    editTop(
      kind,
      (m, doc) => {
        const p = pairOf(m, name);
        if (!p) return;
        if (!isMap(p.value)) p.value = external ? doc.createNode({ external: true }) : null;
        else {
          setKey(doc, p.value, 'external', external ? true : undefined);
          if (!p.value.items.length) p.value = null;
        }
      },
      name,
    );
  }

  function remove(kind: 'networks' | 'volumes', name: string) {
    editTop(
      kind,
      (m) => {
        const p = pairOf(m, name);
        if (p) m.items = m.items.filter((x) => x !== p);
      },
      name,
    );
  }

  let newNetwork = $state('');
  let newVolume = $state('');

  function add(kind: 'networks' | 'volumes', e: Event) {
    e.preventDefault();
    const name = (kind === 'networks' ? newNetwork : newVolume).trim();
    if (!name) return;
    try {
      studio.editCompose(main, (doc) => declareTopLevel(doc, kind, name), `top:${kind}:add`);
    } catch (err) {
      ui.error(err);
    }
    if (kind === 'networks') newNetwork = '';
    else newVolume = '';
  }

  function fix(p: Problem) {
    const f = p.fix;
    if (!f) return;
    if (f.kind === 'declare') {
      try {
        studio.editCompose(f.file, (doc) => declareTopLevel(doc, f.what, f.name), `fix:${f.name}`);
      } catch (err) {
        ui.error(err);
      }
    } else {
      studio.select({ kind: 'variables' });
      setTimeout(() => document.getElementById(`var-${f.name}`)?.focus(), 60);
    }
  }

  function goTo(p: Problem) {
    if (p.service) studio.select({ kind: 'service', name: p.service, section: p.section });
  }

  const containerOf = (name: string) => live.list.find((c) => c.stack === studio.name && c.service === name);
</script>

<div class="stack">
  <section>
    <h2>{studio.services.length ? plural(studio.services.length, 'service') : 'No services yet'}</h2>
    <div class="tiles">
      {#each tiles as t (t.name)}
        {@const c = containerOf(t.name)}
        <button type="button" class="tile" onclick={() => studio.select({ kind: 'service', name: t.name })}>
          <AppIcon name={t.name} slug={t.slug} size={34} />
          <span class="info">
            <span class="name">{#if c}<StateDot state={stateOf(c)} />{/if}{t.name}</span>
            <span class="img mono">{t.image || 'no image'}</span>
            <span class="meta faint">{t.ports.length ? `port ${t.ports.join(', ')}` : 'no published ports'} · {plural(t.mounts, 'folder')}</span>
          </span>
        </button>
      {/each}
    </div>
  </section>

  <section>
    <h2>Checks</h2>
    {#if problems.length}
      <ul class="problems">
        {#each problems as p, i (i)}
          <li class={p.level}>
            <TriangleAlert size={15} />
            <span class="msg">{p.message}</span>
            {#if p.service}<button type="button" class="btn small ghost" onclick={() => goTo(p)}>Show</button>{/if}
            {#if p.fix}<button type="button" class="btn small" onclick={() => fix(p)}><Wrench size={14} />{p.fix.kind === 'declare' ? 'Declare it' : 'Set it'}</button>{/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="ok"><CircleCheck size={16} /> Nothing looks wrong. Saving still checks the files with docker compose.</p>
    {/if}
  </section>

  <div class="two">
    <section>
      <h2>Networks</h2>
      <p class="faint small">The stack gets its own network either way. Declare others to share them with other stacks; “external” joins one that already exists.</p>
      {#each networks as n (n.name)}
        <div class="row">
          <span class="mono name">{n.name}</span>
          <label class="ext"><input type="checkbox" checked={n.external} onchange={(e) => setExternal('networks', n.name, e.currentTarget.checked)} /> external</label>
          <span class="faint small used">{n.usedBy.length ? n.usedBy.join(', ') : 'not used'}</span>
          <button type="button" class="btn ghost icon small" aria-label="Remove {n.name}" onclick={() => remove('networks', n.name)}><Trash2 size={14} /></button>
        </div>
      {/each}
      <form class="row add" onsubmit={(e) => add('networks', e)}>
        <input class="input mono" placeholder="proxy" bind:value={newNetwork} aria-label="New network" spellcheck="false" />
        <button type="submit" class="btn small" disabled={!newNetwork.trim()}><Plus size={15} />Declare</button>
      </form>
    </section>
    <section>
      <h2>Volumes</h2>
      <p class="faint small">Named storage Docker manages. Removing a declaration doesn't delete the data.</p>
      {#each volumes as v (v.name)}
        <div class="row">
          <span class="mono name">{v.name}</span>
          <label class="ext"><input type="checkbox" checked={v.external} onchange={(e) => setExternal('volumes', v.name, e.currentTarget.checked)} /> external</label>
          <span class="faint small used">{v.usedBy.length ? v.usedBy.join(', ') : 'not used'}</span>
          <button type="button" class="btn ghost icon small" aria-label="Remove {v.name}" onclick={() => remove('volumes', v.name)}><Trash2 size={14} /></button>
        </div>
      {/each}
      <form class="row add" onsubmit={(e) => add('volumes', e)}>
        <input class="input mono" placeholder="app_data" bind:value={newVolume} aria-label="New volume" spellcheck="false" />
        <button type="submit" class="btn small" disabled={!newVolume.trim()}><Plus size={15} />Declare</button>
      </form>
    </section>
  </div>
</div>

<style>
  .stack {
    display: grid;
    gap: 22px;
  }
  section {
    display: grid;
    gap: 8px;
    align-content: start;
  }
  h2 {
    font-size: var(--fs-md);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 8px;
  }
  .tile {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel) var(--starfield);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms var(--ease);
  }
  .tile:hover {
    border-color: var(--accent);
  }
  .info {
    display: grid;
    min-width: 0;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }
  .img,
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
  }
  .img {
    color: var(--text-2);
  }
  .problems {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .problems li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--warn);
    border-radius: var(--radius);
    background: var(--panel);
  }
  .problems li.error {
    border-left-color: var(--bad);
  }
  .problems li > :global(svg) {
    color: var(--warn);
  }
  .problems li.error > :global(svg) {
    color: var(--bad);
  }
  .msg {
    flex: 1;
    min-width: 200px;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: var(--text-2);
  }
  .ok :global(svg) {
    color: var(--ok);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 22px;
  }
  .small {
    margin: 0;
    font-size: var(--fs-sm);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr) 30px;
    gap: 8px;
    align-items: center;
    padding: 4px 0;
    border-bottom: 1px solid var(--line);
  }
  .row.add {
    grid-template-columns: minmax(0, 1fr) auto;
    border-bottom: 0;
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .ext {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .used {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
