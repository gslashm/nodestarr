<!--
  A .env file as a list of variables. For the stack's own .env it also
  shows which services use each variable, the ones compose needs that
  aren't set, and the ones nothing uses. Renaming a variable renames its
  ${...} references in the compose files too.
-->
<script lang="ts">
  import Code from '@lucide/svelte/icons/code';
  import Copy from '@lucide/svelte/icons/copy';
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Plus from '@lucide/svelte/icons/plus';
  import Search from '@lucide/svelte/icons/search';
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import CodeEditor from '../CodeEditor.svelte';
  import Menu, { type MenuEntry } from '../Menu.svelte';
  import { deleteDotenv, dotenvIssues, parseDotenv, renameDotenv, setDotenv } from '../../lib/compose/dotenv';
  import { envFilesOf, looksSecret, renameVariable } from '../../lib/compose/environment';
  import { hasStrayDollar, interpolate, references } from '../../lib/compose/interpolate';
  import { joinPath, useStudio } from '../../lib/compose/studio.svelte';
  import { generatePassword } from '../../lib/compose/templates';
  import { plural } from '../../lib/format';
  import { ui } from '../../lib/ui.svelte';

  let { rel }: { rel: string } = $props();
  const studio = useStudio();

  const isProject = $derived(rel === studio.envRel);
  let asText = $state(false);
  let filter = $state('');
  let reveal = $state(false);
  let revealed = $state<Record<string, boolean>>({});
  let newKey = $state('');
  let newValue = $state('');

  const text = $derived(studio.text(rel));
  const lines = $derived(text.split(/\r?\n/));
  const issues = $derived(dotenvIssues(text));

  // Compose settings that live in .env but aren't ${...} variables.
  const COMPOSE_VARS = new Set(['COMPOSE_PROJECT_NAME', 'COMPOSE_PROFILES', 'COMPOSE_FILE', 'COMPOSE_PATH_SEPARATOR', 'COMPOSE_PARALLEL_LIMIT', 'COMPOSE_IGNORE_ORPHANS', 'COMPOSE_REMOVE_ORPHANS', 'COMPOSE_BAKE', 'DOCKER_HOST', 'DOCKER_DEFAULT_PLATFORM']);

  interface Row {
    key: string;
    value: string;
    line: number;
    /** Comment lines right above it. */
    about: string;
  }

  const rows = $derived.by((): Row[] => {
    const byKey = new Map<string, Row>();
    for (const e of parseDotenv(text)) {
      const about: string[] = [];
      for (let l = e.line - 2; l >= 0; l--) {
        const t = lines[l]?.trim() ?? '';
        if (!t.startsWith('#')) break;
        about.unshift(t.replace(/^#+\s?/, ''));
      }
      byKey.set(e.key, { key: e.key, value: e.value ?? '', line: e.line, about: about.join(' ') });
    }
    return [...byKey.values()];
  });

  /** Who uses each variable (only for the stack's .env). */
  const usage = $derived(isProject ? studio.usage : new Map());
  /** Services that load this file through env_file. */
  const loadedBy = $derived(
    isProject
      ? []
      : studio.services.filter((s) => studio.docs.some(({ doc }) => envFilesOf(doc, s).some((r) => joinPath(r.path) === rel))),
  );

  const missing = $derived(
    isProject ? [...usage.entries()].filter(([k, u]) => !rows.some((r) => r.key === k) && (u.bare || u.required)).map(([k, u]) => ({ key: k, use: u })) : [],
  );
  const defaulted = $derived(
    isProject ? [...usage.entries()].filter(([k, u]) => !rows.some((r) => r.key === k) && !u.bare && !u.required && u.defaults.length).map(([k, u]) => ({ key: k, use: u })) : [],
  );
  const visible = $derived(rows.filter((r) => !filter.trim() || r.key.toLowerCase().includes(filter.trim().toLowerCase())));

  const label = (key: string, what: string) => `var:${rel}:${key}:${what}`;
  const literal = (v: string) => hasStrayDollar(v) && !references(v).length;

  function write(next: string, what: string) {
    if (studio.files[rel]) studio.setText(rel, next, what);
    else studio.addFile(rel, next, 'env');
  }

  function setValue(key: string, value: string) {
    write(setDotenv(text, key, value, { literal: literal(value) }), label(key, 'value'));
  }

  function rename(row: Row, to: string) {
    to = to.trim();
    if (!to || to === row.key) return;
    if (!/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(to)) {
      ui.toast('Names use letters, digits and _, and start with a letter or _', 'warning');
      return;
    }
    if (rows.some((r) => r.key === to)) {
      ui.toast(`${to} already exists`, 'warning');
      return;
    }
    const what = label(row.key, 'rename');
    write(renameDotenv(text, row.key, to), what);
    // Keep the compose files pointing at it.
    if (isProject && usage.has(row.key)) {
      let n = 0;
      for (const f of studio.composeRels) {
        try {
          studio.editCompose(f, (doc) => (n += renameVariable(doc, row.key, to)), what);
        } catch (err) {
          ui.error(err);
        }
      }
      if (n) ui.toast(`Renamed ${row.key} in ${plural(n, 'place')} of the compose files too`, 'success');
    }
  }

  function remove(row: Row) {
    write(deleteDotenv(text, row.key), label(row.key, 'remove'));
  }

  function add(e: Event) {
    e.preventDefault();
    const key = newKey.trim();
    if (!/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(key)) {
      ui.toast('Names use letters, digits and _, and start with a letter or _', 'warning');
      return;
    }
    write(setDotenv(text, key, newValue, { literal: literal(newValue) }), label(key, 'add'));
    newKey = '';
    newValue = '';
  }

  function generate(key: string) {
    setValue(key, generatePassword());
    revealed[key] = true;
  }

  function resolved(v: string): string | null {
    if (!v.includes('$')) return null;
    const r = interpolate(v, (n) => studio.project.get(n));
    return r.value === v ? null : r.value;
  }

  function rowMenu(row: Row): MenuEntry[] {
    const items: MenuEntry[] = [{ label: 'Copy value', icon: Copy, run: () => navigator.clipboard?.writeText(row.value) }];
    if (looksSecret(row.key)) items.push({ label: 'Generate a strong password', icon: Sparkles, run: () => generate(row.key) });
    items.push('separator', { label: 'Remove', icon: Trash2, danger: true, run: () => remove(row) });
    return items;
  }

  const masked = (key: string) => looksSecret(key) && !reveal && !revealed[key];

  function goTo(service: string) {
    if (service) studio.select({ kind: 'service', name: service, section: 'environment' });
  }
</script>

<div class="vars">
  <div class="toolbar">
    {#if !asText}
      <div class="search">
        <Search size={15} />
        <input class="input" placeholder="Filter" bind:value={filter} aria-label="Filter variables" />
      </div>
    {/if}
    <span class="grow"></span>
    {#if !asText && rows.some((r) => looksSecret(r.key))}
      <button type="button" class="btn small ghost" onclick={() => (reveal = !reveal)}>
        {#if reveal}<EyeOff size={15} />Hide secrets{:else}<Eye size={15} />Show secrets{/if}
      </button>
    {/if}
    <button type="button" class="btn small ghost" onclick={() => (asText = !asText)}><Code size={15} />{asText ? 'Edit as a list' : 'Edit as text'}</button>
  </div>

  {#if isProject}
    <p class="intro muted">
      Variables for the compose files: <code>$&#123;NAME&#125;</code> anywhere in them is replaced by the value here. Keep passwords and tokens here rather than in the compose file.
    </p>
  {:else if loadedBy.length}
    <p class="intro muted">Loaded into {loadedBy.join(', ')} as environment variables.</p>
  {:else}
    <p class="intro muted">No service loads this file yet. Add it under a service's Environment, in Env files.</p>
  {/if}

  {#if asText}
    <CodeEditor bind:value={() => text, (v) => write(v, `code:${rel}`)} language="ini" height="420px" label="Contents of {rel}" onsave={() => studio.requestSave()} />
  {:else}
    {#if issues.length}
      <div class="issues">
        {#each issues as i (i.line + i.message)}<p><TriangleAlert size={14} />Line {i.line}: {i.message}</p>{/each}
      </div>
    {/if}

    {#if missing.length}
      <section class="group need">
        <h3><TriangleAlert size={15} />Needed by the compose files</h3>
        {#each missing as m (m.key)}
          <div class="row">
            <div class="name">
              <span class="mono keytext">{m.key}</span>
              <span class="faint small">{m.use.required ? 'Required. ' : ''}Used by {m.use.services.filter(Boolean).join(', ') || 'the stack'}</span>
            </div>
            <div class="value-line">
              <input id="var-{m.key}" class="input mono" placeholder="Not set" aria-label="Value of {m.key}" spellcheck="false" autocomplete="off" type={masked(m.key) ? 'password' : 'text'} onchange={(e) => setValue(m.key, e.currentTarget.value)} />
              {#if looksSecret(m.key)}
                <button type="button" class="btn small ghost" onclick={() => generate(m.key)} title="Generate a strong password"><Sparkles size={14} />Generate</button>
              {/if}
            </div>
          </div>
        {/each}
      </section>
    {/if}

    {#if visible.length}
      <div class="rows">
        {#each visible as row (row.key)}
          {@const use = usage.get(row.key)}
          {@const r = masked(row.key) ? null : resolved(row.value)}
          <div class="row">
            <div class="name">
              <input class="input mono" value={row.key} aria-label="Name" spellcheck="false" onchange={(e) => rename(row, e.currentTarget.value)} onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />
              {#if row.about}<span class="about faint" title={row.about}>{row.about}</span>{/if}
            </div>
            <div class="value">
              <div class="value-line">
                <input
                  id="var-{row.key}"
                  class="input mono"
                  value={row.value}
                  type={masked(row.key) ? 'password' : 'text'}
                  aria-label="Value of {row.key}"
                  spellcheck="false"
                  autocomplete="off"
                  oninput={(e) => setValue(row.key, e.currentTarget.value)}
                />
                {#if looksSecret(row.key) && !reveal}
                  <button type="button" class="btn ghost icon small" aria-label={revealed[row.key] ? 'Hide value' : 'Show value'} onclick={() => (revealed[row.key] = !revealed[row.key])}>
                    {#if revealed[row.key]}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
                  </button>
                {/if}
              </div>
              {#if r !== null}<span class="sub"><span class="faint">becomes</span> <span class="mono">{r || '(empty)'}</span></span>{/if}
            </div>
            <div class="used">
              {#if isProject}
                {#if use}
                  {#each use.services as s (s)}
                    {#if s}<button type="button" class="chip link" onclick={() => goTo(s)}>{s}</button>{:else}<span class="chip">stack</span>{/if}
                  {/each}
                {:else if COMPOSE_VARS.has(row.key)}
                  <span class="chip">compose setting</span>
                {:else}
                  <span class="faint small">Not used</span>
                {/if}
              {/if}
            </div>
            <div class="act"><Menu items={rowMenu(row)} label="Actions for {row.key}" small /></div>
          </div>
        {/each}
      </div>
    {:else if rows.length}
      <p class="faint">No variables match “{filter}”.</p>
    {/if}

    {#if defaulted.length && !filter}
      <section class="group">
        <h3 class="muted">Using their defaults</h3>
        {#each defaulted as d (d.key)}
          <div class="row">
            <div class="name">
              <span class="mono keytext">{d.key}</span>
              <span class="faint small">{d.use.services.filter(Boolean).join(', ')}</span>
            </div>
            <div class="value-line">
              <input id="var-{d.key}" class="input mono" placeholder="default: {d.use.defaults.join(' / ') || '(empty)'}" aria-label="Value of {d.key}" spellcheck="false" onchange={(e) => setValue(d.key, e.currentTarget.value)} />
            </div>
          </div>
        {/each}
      </section>
    {/if}

    <form class="new" onsubmit={add}>
      <input class="input mono" placeholder="NAME" bind:value={newKey} aria-label="New variable name" spellcheck="false" autocomplete="off" />
      <input class="input mono" placeholder="value" bind:value={newValue} aria-label="New variable value" spellcheck="false" autocomplete="off" />
      <button type="submit" class="btn small" disabled={!newKey.trim()}><Plus size={15} />Add variable</button>
    </form>
    {#if !rows.length && !missing.length}
      <p class="faint small"><KeyRound size={13} /> No variables yet.</p>
    {/if}
  {/if}
</div>

<style>
  .vars {
    display: grid;
    gap: 10px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
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
  .intro {
    margin: 0;
    font-size: var(--fs-sm);
  }
  .issues p {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 4px;
    color: var(--warn);
    font-size: var(--fs-sm);
  }
  .rows,
  .group {
    display: grid;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .group h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 4px;
    font-size: var(--fs-sm);
  }
  .group.need {
    border-color: color-mix(in srgb, var(--warn) 45%, var(--line));
  }
  .group.need h3 :global(svg) {
    color: var(--warn);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(150px, 0.9fr) minmax(0, 1.6fr) minmax(120px, 0.9fr) 30px;
    gap: 8px;
    align-items: start;
    padding: 8px 12px;
    border-top: 1px solid var(--line);
  }
  .group .row {
    grid-template-columns: minmax(150px, 0.9fr) minmax(0, 2.5fr);
  }
  .rows .row:first-child {
    border-top: 0;
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .name,
  .value {
    display: grid;
    gap: 3px;
    min-width: 0;
  }
  .keytext {
    padding-top: 6px;
    overflow-wrap: anywhere;
  }
  .about {
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .value-line {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .value-line .input {
    flex: 1;
    min-width: 0;
  }
  .sub {
    font-size: var(--fs-xs);
    overflow-wrap: anywhere;
  }
  .used {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding-top: 5px;
  }
  .chip.link {
    cursor: pointer;
    font-family: var(--font-data);
  }
  .chip.link:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }
  .small {
    font-size: var(--fs-xs);
  }
  .new {
    display: grid;
    grid-template-columns: minmax(150px, 0.9fr) minmax(0, 2fr) auto;
    gap: 6px;
  }
  .new .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  @media (max-width: 860px) {
    .row,
    .group .row {
      grid-template-columns: minmax(0, 1fr) 30px;
    }
    .row .value,
    .row .used,
    .group .row .value-line {
      grid-column: 1 / -1;
    }
    .new {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
