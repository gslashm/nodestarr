<!--
  A service's environment as one list: variables from the compose file,
  from env files and from the image, each with where it comes from and the
  value the container ends up with. Edits go back to where the value lives:
  `${VAR}` references change the variable in .env, env file entries change
  that file, everything else changes the compose file.
-->
<script lang="ts">
  import ClipboardPaste from '@lucide/svelte/icons/clipboard-paste';
  import Copy from '@lucide/svelte/icons/copy';
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import FileCode from '@lucide/svelte/icons/file-code';
  import FileText from '@lucide/svelte/icons/file-text';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Link from '@lucide/svelte/icons/link';
  import Package from '@lucide/svelte/icons/package';
  import Plus from '@lucide/svelte/icons/plus';
  import Search from '@lucide/svelte/icons/search';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Unlink from '@lucide/svelte/icons/unlink';
  import Dialog from '../Dialog.svelte';
  import Menu, { type MenuEntry } from '../Menu.svelte';
  import { api, seg } from '../../lib/api';
  import { deleteDotenv, dotenvMap, parseDotenv, renameDotenv, setDotenv } from '../../lib/compose/dotenv';
  import { envFilesOf, serviceEnv, looksSecret, type EnvRow } from '../../lib/compose/environment';
  import { images } from '../../lib/compose/images.svelte';
  import { escapeDollars, escapeReferences, hasStrayDollar, interpolate, references } from '../../lib/compose/interpolate';
  import { deleteKeyValue, editService, get, renameKeyValue, set, setKeyValue } from '../../lib/compose/service';
  import { joinPath, useStudio } from '../../lib/compose/studio.svelte';
  import { lookup, type YAMLMap, type YDoc } from '../../lib/compose/yamldoc';
  import { live } from '../../lib/live.svelte';
  import { plural } from '../../lib/format';
  import type { ContainerSpec } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();

  let filter = $state('');
  let reveal = $state(false);
  let revealed = $state<Record<string, boolean>>({});
  let showImage = $state(false);
  let pasteOpen = $state(false);
  let pasteText = $state('');
  let newKey = $state('');
  let newValue = $state('');
  let newTarget = $state('compose');

  const envFileTexts = $derived(studio.envFileTexts(name));
  const envFileRefs = $derived(studio.docs.flatMap(({ doc: d }) => envFilesOf(d, name)));

  const imageRef = $derived(interpolate(String(get(doc, svc, 'image') ?? ''), (n) => studio.project.get(n)).value);
  $effect(() => {
    if (imageRef) images.loadInspect(imageRef);
  });
  const imageEnv = $derived(images.env(imageRef));

  const env = $derived(serviceEnv({ composes: studio.docs, service: name, project: studio.project, envFiles: envFileTexts, image: imageEnv }));
  const inherited = $derived(lookup(doc, svc, 'environment').inherited);
  const envText = $derived(studio.text(studio.envRel));
  const envEntries = $derived(dotenvMap(envText));

  const visible = $derived(
    env.rows.filter((r) => {
      if (!showImage && r.def.origin.kind === 'image') return false;
      const q = filter.trim().toLowerCase();
      return !q || r.key.toLowerCase().includes(q) || (r.def.value ?? '').toLowerCase().includes(q);
    }),
  );
  const imageOnly = $derived(env.rows.filter((r) => r.def.origin.kind === 'image').length);

  // What the running container has, to show what a deploy would change.
  const container = $derived(live.list.find((c) => c.stack === studio.name && c.service === name) ?? null);
  let running = $state<Map<string, string> | null>(null);
  let runningFor = '';
  $effect(() => {
    const id = container?.id;
    if (!id || id === runningFor) return;
    runningFor = id;
    api
      .get<{ spec: ContainerSpec }>(`/containers/${seg(id)}/spec`)
      .then((r) => (running = new Map(r.spec.env.map((kv) => [kv.key, kv.value]))))
      .catch(() => (running = null));
  });
  const goneFromFiles = $derived(running ? [...running.keys()].filter((k) => !env.rows.some((r) => r.key === k)) : []);
  // The container's settings leave out variables identical to the image's own.
  const imageValues = $derived(new Map((imageEnv ?? []).map((kv) => [kv.key, kv.value])));
  const runningValue = (key: string) => running?.get(key) ?? imageValues.get(key);

  /** Text shown in a row's value field, and what editing it changes. */
  function field(row: EnvRow): { text: string; placeholder: string; readonly: boolean } {
    const o = row.def.origin;
    if (o.kind === 'image') return { text: row.def.raw ?? '', placeholder: '', readonly: true };
    if (o.kind === 'compose' && row.via) {
      const e = envEntries.get(row.via);
      return { text: e?.value ?? '', placeholder: row.viaDefault !== null ? `default: ${row.viaDefault || '(empty)'}` : 'Not set', readonly: false };
    }
    return { text: row.def.raw ?? '', placeholder: '', readonly: false };
  }

  const label = (row: EnvRow, what: string) => `env:${name}:${row.key}:${what}`;

  function guard(fn: () => void) {
    try {
      fn();
    } catch (err) {
      ui.error(err);
    }
  }

  /** In dotenv files a `$` that isn't a reference is kept literal (single quotes). */
  const literal = (v: string) => hasStrayDollar(v) && !references(v).length;

  function composeEdit(file: string, fn: (d: YDoc, s: YAMLMap) => void, what: string) {
    studio.editCompose(
      file,
      (d) => {
        const s = editService(d, name);
        if (s) fn(d, s);
      },
      what,
    );
  }

  function setValue(row: EnvRow, value: string) {
    guard(() => {
      const o = row.def.origin;
      if (o.kind === 'compose' && row.via) {
        studio.setText(studio.envRel, setDotenv(envText, row.via, value, { literal: literal(value) }), label(row, 'value'));
      } else if (o.kind === 'compose') {
        composeEdit(o.file, (d, s) => setKeyValue(d, s, 'environment', row.key, value), label(row, 'value'));
      } else if (o.kind === 'env_file') {
        const rel = joinPath(o.file);
        studio.setText(rel, setDotenv(studio.text(rel), row.key, value, { literal: literal(value) }), label(row, 'value'));
      }
    });
  }

  function rename(row: EnvRow, to: string) {
    to = to.trim();
    if (!to || to === row.key) return;
    if (env.rows.some((r) => r.key === to && r.def.origin.kind !== 'image')) {
      ui.toast(`${to} is already set for ${name}`, 'warning');
      return;
    }
    guard(() => {
      const o = row.def.origin;
      if (o.kind === 'compose') composeEdit(o.file, (d, s) => renameKeyValue(d, s, 'environment', row.key, to), label(row, 'rename'));
      else if (o.kind === 'env_file') studio.setText(joinPath(o.file), renameDotenv(studio.text(joinPath(o.file)), row.key, to), label(row, 'rename'));
    });
  }

  function remove(row: EnvRow) {
    guard(() => {
      const o = row.def.origin;
      if (o.kind === 'compose') composeEdit(o.file, (d, s) => deleteKeyValue(d, s, 'environment', row.key), label(row, 'remove'));
      else if (o.kind === 'env_file') studio.setText(joinPath(o.file), deleteDotenv(studio.text(joinPath(o.file)), row.key), label(row, 'remove'));
    });
  }

  /** A name for a new .env variable that doesn't clash with an unrelated one. */
  function variableFor(key: string, value: string): string {
    const existing = envEntries.get(key);
    if (!existing || existing.value === value) return key;
    const prefixed = `${name.toUpperCase().replace(/[^A-Z0-9]+/g, '_')}_${key}`;
    return envEntries.has(prefixed) && envEntries.get(prefixed)?.value !== value ? `${prefixed}_${Date.now() % 1000}` : prefixed;
  }

  /** Keeps a compose value in .env and references it (for secrets). */
  function moveToEnv(row: EnvRow) {
    const o = row.def.origin;
    if (o.kind !== 'compose' || row.via) return;
    const value = row.def.value ?? '';
    const v = variableFor(row.key, value);
    guard(() => {
      const what = label(row, 'move');
      studio.setText(studio.envRel, setDotenv(envText, v, value, { literal: hasStrayDollar(value) }), what);
      composeEdit(o.file, (d, s) => setKeyValue(d, s, 'environment', row.key, `\${${v}}`), what);
      ui.toast(`${row.key} now comes from ${v} in ${studio.envRel}`, 'success');
    });
  }

  /** Gives the service its own value instead of the shared variable. */
  function detach(row: EnvRow) {
    const o = row.def.origin;
    if (o.kind !== 'compose' || !row.via) return;
    guard(() => composeEdit(o.file, (d, s) => setKeyValue(d, s, 'environment', row.key, escapeDollars(row.def.value ?? '')), label(row, 'detach')));
  }

  function override(row: EnvRow) {
    guard(() => studio.editService(name, (d, s) => setKeyValue(d, s, 'environment', row.key, row.def.raw ?? ''), label(row, 'override')));
  }

  function fixDollars(row: EnvRow) {
    const o = row.def.origin;
    if (o.kind !== 'compose' || row.def.raw === null) return;
    const fixed = row.def.raw.replace(/\$(?!\{|[A-Za-z_]|\$)/g, '$$$$');
    guard(() => composeEdit(o.file, (d, s) => setKeyValue(d, s, 'environment', row.key, fixed), label(row, 'fix')));
  }

  /** A `$` that was meant as part of the value (a password, say): escape it. */
  function keepDollar(row: EnvRow, variable: string) {
    const o = row.def.origin;
    if (o.kind !== 'compose' || row.def.raw === null) return;
    const fixed = escapeReferences(row.def.raw, [variable]);
    guard(() => composeEdit(o.file, (d, s) => setKeyValue(d, s, 'environment', row.key, fixed), label(row, 'fix')));
  }

  function setMissing(nameOfVar: string) {
    studio.select({ kind: 'variables' });
    setTimeout(() => document.getElementById(`var-${nameOfVar}`)?.focus(), 50);
  }

  /** Adds a variable where it should live. */
  function add(key: string, value: string, target: string): boolean {
    key = key.trim();
    if (!/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(key)) {
      ui.toast('Names use letters, digits and _, and start with a letter or _', 'warning');
      return false;
    }
    if (env.rows.some((r) => r.key === key && r.def.origin.kind !== 'image')) {
      ui.toast(`${key} is already set; change it in its row`, 'warning');
      return false;
    }
    let ok = true;
    const what = `env:${name}:add:${key}`;
    try {
      if (target === 'compose') {
        studio.editService(name, (d, s) => setKeyValue(d, s, 'environment', key, value), what);
      } else if (target === 'dotenv') {
        const v = variableFor(key, value);
        if (value || !envEntries.has(v)) studio.setText(studio.envRel, setDotenv(envText, v, value, { literal: literal(value) }), what);
        studio.editService(name, (d, s) => setKeyValue(d, s, 'environment', key, `\${${v}}`), what);
      } else {
        const rel = joinPath(target);
        studio.setText(rel, setDotenv(studio.text(rel), key, value, { literal: literal(value) }), what);
      }
    } catch (err) {
      ui.error(err);
      ok = false;
    }
    return ok;
  }

  function submitNew(e: Event) {
    e.preventDefault();
    if (add(newKey, newValue, newTarget)) {
      newKey = '';
      newValue = '';
      document.getElementById(`env-new-${name}`)?.focus();
    }
  }

  // Secrets go to .env by default.
  $effect(() => {
    if (looksSecret(newKey)) newTarget = 'dotenv';
  });

  /** KEY=value lines, `-e KEY=value` flags or `export KEY=value`. */
  const pasted = $derived.by(() => {
    const text = pasteText.replace(/(?:^|\s)(?:-e|--env)[\s=]+/g, '\n');
    return parseDotenv(text).filter((e) => e.value !== null);
  });

  function addPasted() {
    let n = 0;
    for (const e of pasted) {
      if (env.rows.some((r) => r.key === e.key && r.def.origin.kind !== 'image')) {
        const row = env.rows.find((r) => r.key === e.key)!;
        setValue(row, e.value ?? '');
      } else if (!add(e.key, e.value ?? '', looksSecret(e.key) ? 'dotenv' : 'compose')) continue;
      n++;
    }
    pasteOpen = false;
    pasteText = '';
    ui.toast(`${plural(n, 'variable')} added`, 'success');
  }

  function asEnvFile(): string {
    return env.rows
      .filter((r) => r.def.origin.kind !== 'image' && r.def.value !== null)
      .map((r) => `${r.key}=${r.def.value}`)
      .join('\n');
  }

  function asRunFlags(): string {
    return env.rows
      .filter((r) => r.def.origin.kind !== 'image' && r.def.value !== null)
      .map((r) => `-e ${r.key}=${/^[\w@%+=:,./-]*$/.test(r.def.value ?? '') ? r.def.value : `'${(r.def.value ?? '').replace(/'/g, `'"'"'`)}'`}`)
      .join(' ');
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      ui.toast(`Copied ${what}`, 'success');
    } catch {
      ui.toast('Copying is blocked by the browser here', 'warning');
    }
  }

  const headerMenu = $derived<MenuEntry[]>([
    { label: 'Paste variables…', icon: ClipboardPaste, run: () => (pasteOpen = true) },
    { label: 'Copy as a .env file', icon: Copy, run: () => copy(asEnvFile(), 'the variables') },
    { label: 'Copy as docker run flags', icon: Copy, run: () => copy(asRunFlags(), 'the flags') },
  ]);

  function rowMenu(row: EnvRow): MenuEntry[] {
    const o = row.def.origin;
    const items: MenuEntry[] = [];
    if (o.kind === 'compose' && !row.via) items.push({ label: `Keep the value in ${studio.envRel}`, icon: KeyRound, hint: 'Good for passwords and tokens', run: () => moveToEnv(row) });
    if (o.kind === 'compose' && row.via) {
      items.push({ label: `Show ${row.via} in Variables`, icon: Link, run: () => setMissing(row.via!) });
      items.push({ label: 'Use a value just for this service', icon: Unlink, run: () => detach(row) });
    }
    if (o.kind === 'env_file') items.push({ label: `Open ${o.file}`, icon: FileText, run: () => studio.select({ kind: 'file', rel: joinPath(o.file) }) });
    if (o.kind === 'image') items.push({ label: 'Override for this service', icon: Plus, run: () => override(row) });
    items.push({ label: 'Copy value', icon: Copy, run: () => copy(row.def.value ?? '', row.key) });
    if (o.kind !== 'image') items.push('separator', { label: o.kind === 'env_file' ? `Remove from ${o.file}` : 'Remove', icon: Trash2, danger: true, run: () => remove(row) });
    return items;
  }

  function sourceLabel(row: EnvRow): string {
    const o = row.def.origin;
    if (o.kind === 'compose') return row.via ? `${studio.envRel} · ${row.via}` : o.file;
    if (o.kind === 'env_file') return o.file;
    return 'image';
  }

  function shadowText(row: EnvRow): string {
    return row.shadowed
      .map((d) => {
        const o = d.origin;
        const where = o.kind === 'image' ? 'the image' : o.kind === 'env_file' ? o.file : o.file;
        return `${where}${d.value !== null ? ` (${d.value})` : ''}`;
      })
      .join(', ');
  }

  const masked = (row: EnvRow) => row.secret && !reveal && !revealed[row.key];
  const usedElsewhere = (v: string) => (studio.usage.get(v)?.services ?? []).filter((s) => s && s !== name);

  function setEnvFiles(paths: string[]) {
    guard(() => studio.editService(name, (d, s) => set(d, s, 'env_file', paths.length === 1 ? paths[0] : paths), `env:${name}:env_file`));
  }

  let newFile = $state('');
  function addEnvFile(e: Event) {
    e.preventDefault();
    const p = newFile.trim();
    if (!p) return;
    const paths = envFileRefs.map((r) => r.path);
    if (paths.includes(p)) return;
    setEnvFiles([...paths, p]);
    void studio.open(joinPath(p), 'env_file');
    newFile = '';
  }
</script>

<div class="env">
  <div class="toolbar">
    <div class="search">
      <Search size={15} />
      <input class="input" placeholder="Filter variables" bind:value={filter} aria-label="Filter variables" />
    </div>
    <span class="grow"></span>
    <div class="acts">
      {#if imageOnly}
        <button type="button" class="btn small ghost" class:on={showImage} onclick={() => (showImage = !showImage)} title="Variables the image sets by itself">
          <Package size={15} />{showImage ? 'Hide' : 'Show'} {imageOnly} from the image
        </button>
      {/if}
      {#if env.rows.some((r) => r.secret)}
        <button type="button" class="btn small ghost" onclick={() => (reveal = !reveal)}>
          {#if reveal}<EyeOff size={15} />Hide secrets{:else}<Eye size={15} />Show secrets{/if}
        </button>
      {/if}
      <Menu items={headerMenu} label="More environment actions" small />
    </div>
  </div>

  {#if inherited}
    <p class="note"><Link size={14} /> These variables are shared with other services through a YAML anchor. Changing one gives {name} its own copy.</p>
  {/if}
  {#if env.unreadable.length}
    <p class="note warn"><TriangleAlert size={14} /> {env.unreadable.join(', ')} can't be read. It's created when you save, or check the path.</p>
  {/if}

  {#if visible.length}
    <div class="rows" role="table" aria-label="Environment variables of {name}">
      {#each visible as row (row.key + row.def.origin.kind)}
        {@const f = field(row)}
        {@const o = row.def.origin}
        {@const was = runningValue(row.key)}
        {@const drift = running && row.def.value !== null && o.kind !== 'image' && was !== row.def.value}
        <div class="row" class:image={o.kind === 'image'} role="row">
          <div class="key" role="cell">
            {#if o.kind === 'image'}
              <span class="mono keytext">{row.key}</span>
            {:else}
              <input
                class="input mono"
                value={row.key}
                aria-label="Name"
                spellcheck="false"
                onchange={(e) => rename(row, e.currentTarget.value)}
                onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
              />
            {/if}
          </div>
          <div class="value" role="cell">
            <div class="value-line">
              <input
                class="input mono"
                type={masked(row) ? 'password' : 'text'}
                value={f.text}
                placeholder={f.placeholder}
                readonly={f.readonly}
                aria-label="Value of {row.key}"
                spellcheck="false"
                autocomplete="off"
                oninput={(e) => setValue(row, e.currentTarget.value)}
              />
              {#if row.secret && !reveal}
                <button type="button" class="btn ghost icon small" aria-label={revealed[row.key] ? 'Hide value' : 'Show value'} onclick={() => (revealed[row.key] = !revealed[row.key])}>
                  {#if revealed[row.key]}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
                </button>
              {/if}
            </div>
            {#if row.def.value === null}
              <span class="sub faint">Not set{row.via ? `: add ${row.via} to ${studio.envRel}` : ''}. The container won't have this variable.</span>
            {:else if row.def.value !== f.text && !masked(row)}
              <span class="sub"><span class="faint">becomes</span> <span class="mono resolved">{row.def.value === '' ? '(empty)' : row.def.value}</span></span>
            {/if}
            {#each row.def.missing as m (m)}
              <span class="sub warn">
                <TriangleAlert size={13} />{m} isn't set.
                <button type="button" class="linkish" onclick={() => setMissing(m)}>Set it in Variables</button>
                {#if o.kind === 'compose' && !row.via && row.def.raw !== null}
                  or <button type="button" class="linkish" onclick={() => keepDollar(row, m)}>keep “${m}” as text</button>
                {/if}
              </span>
            {/each}
            {#each row.def.errors as er (er)}
              <span class="sub bad"><TriangleAlert size={13} />Compose stops here: {er}</span>
            {/each}
            {#each row.def.issues as is (is)}
              <span class="sub bad"><TriangleAlert size={13} />{is}</span>
            {/each}
            {#if o.kind === 'compose' && row.def.raw !== null && hasStrayDollar(row.def.raw)}
              <span class="sub warn"><TriangleAlert size={13} />A lone $ starts a variable in compose. <button type="button" class="linkish" onclick={() => fixDollars(row)}>Make it a plain $</button></span>
            {/if}
            {#if row.via && usedElsewhere(row.via).length}
              <span class="sub faint">Shared with {usedElsewhere(row.via).join(', ')}</span>
            {/if}
            {#if drift}
              <span class="sub drift" title="Deploy to apply">The running container has {was === undefined ? 'no value' : masked(row) ? 'another value' : `“${was}”`}; deploy to apply.</span>
            {/if}
          </div>
          <div class="source" role="cell">
            <span class="chip src {o.kind}{row.via ? ' via' : ''}" title={row.shadowed.length ? `Overrides ${shadowText(row)}` : ''}>
              {#if o.kind === 'image'}<Package size={12} />{:else if o.kind === 'env_file'}<FileText size={12} />{:else if row.via}<KeyRound size={12} />{:else}<FileCode size={12} />{/if}
              <span class="ellipsis">{sourceLabel(row)}</span>
            </span>
            {#if row.shadowed.length}<span class="over faint" title="Overrides {shadowText(row)}">overrides {row.shadowed.length}</span>{/if}
          </div>
          <div class="act" role="cell">
            <Menu items={rowMenu(row)} label="Actions for {row.key}" small />
          </div>
        </div>
      {/each}
    </div>
  {:else if env.rows.length}
    <p class="empty faint">No variables match “{filter}”.</p>
  {:else}
    <p class="empty muted">No environment variables yet. Most images document theirs; common ones are TZ (time zone) and PUID/PGID (the user files are written as).</p>
  {/if}

  {#if goneFromFiles.length}
    <p class="note"><TriangleAlert size={14} /> The running container still has {goneFromFiles.join(', ')}. Deploy to remove {goneFromFiles.length === 1 ? 'it' : 'them'}.</p>
  {/if}

  <form class="new" onsubmit={submitNew}>
    <input id="env-new-{name}" class="input mono" placeholder="NAME" bind:value={newKey} aria-label="New variable name" spellcheck="false" autocomplete="off" />
    <input class="input mono" placeholder="value" bind:value={newValue} aria-label="New variable value" spellcheck="false" autocomplete="off" type={looksSecret(newKey) && !reveal ? 'password' : 'text'} />
    <select class="input" bind:value={newTarget} aria-label="Where to keep it">
      <option value="compose">In {studio.serviceFile(name)}</option>
      <option value="dotenv">In {studio.envRel} (shared, private)</option>
      {#each envFileRefs as r (r.path)}<option value={r.path}>In {r.path}</option>{/each}
    </select>
    <button type="submit" class="btn small" disabled={!newKey.trim()}><Plus size={15} />Add</button>
  </form>

  <details class="files">
    <summary>Env files {#if envFileRefs.length}<span class="faint">({envFileRefs.map((r) => r.path).join(', ')})</span>{/if}</summary>
    <p class="faint small">Files of KEY=value lines loaded into the container. Values set above win over them.</p>
    {#each envFileRefs as r, i (r.path)}
      <div class="file-row">
        <button type="button" class="btn small ghost mono" onclick={() => studio.select({ kind: 'file', rel: joinPath(r.path) })}><FileText size={14} />{r.path}</button>
        <span class="grow"></span>
        <button type="button" class="btn ghost icon small" aria-label="Stop loading {r.path}" onclick={() => setEnvFiles(envFileRefs.filter((_, j) => j !== i).map((x) => x.path))}><Trash2 size={14} /></button>
      </div>
    {/each}
    <form class="file-row" onsubmit={addEnvFile}>
      <input class="input mono" placeholder="./{name}.env" bind:value={newFile} aria-label="Env file to load" spellcheck="false" />
      <button type="submit" class="btn small" disabled={!newFile.trim()}><Plus size={15} />Load a file</button>
    </form>
  </details>
</div>

<Dialog open={pasteOpen} title="Paste variables" width="640px" onclose={() => (pasteOpen = false)}>
  <p class="muted">Paste KEY=value lines (a .env file) or the -e flags of a docker run command. Names that look like passwords go to {studio.envRel}.</p>
  <textarea class="input mono" rows="8" bind:value={pasteText} placeholder="TZ=Europe/Berlin&#10;PUID=1000" spellcheck="false"></textarea>
  {#if pasted.length}<p class="faint small">{plural(pasted.length, 'variable')}: {pasted.map((p) => p.key).join(', ')}</p>{/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (pasteOpen = false)}>Cancel</button>
    <button type="button" class="btn primary" disabled={!pasted.length} onclick={addPasted}>Add {plural(pasted.length, 'variable')}</button>
  {/snippet}
</Dialog>

<style>
  .env {
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
    flex: 1 1 160px;
    align-items: center;
    max-width: 280px;
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 2px;
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
  .btn.on {
    color: var(--text);
    background: var(--raised);
  }
  .note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .note.warn :global(svg) {
    color: var(--warn);
  }
  .rows {
    display: grid;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(150px, 0.9fr) minmax(0, 2fr) minmax(120px, 0.8fr) 30px;
    gap: 8px;
    align-items: start;
    padding: 8px 10px;
    border-top: 1px solid var(--line);
  }
  .row:first-child {
    border-top: 0;
  }
  .row.image {
    background: color-mix(in srgb, var(--raised) 45%, transparent);
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .keytext {
    display: block;
    padding: 6px 0;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .value {
    display: grid;
    gap: 3px;
    min-width: 0;
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
  .value-line .input[readonly] {
    border-style: dashed;
    color: var(--text-2);
  }
  .sub {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
    font-size: var(--fs-xs);
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  .sub.warn {
    color: var(--warn);
  }
  .sub.bad {
    color: var(--bad);
  }
  .sub.drift {
    color: var(--info);
  }
  .resolved {
    color: var(--text);
  }
  .linkish {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
  .linkish:hover {
    text-decoration: underline;
  }
  .source {
    display: grid;
    gap: 2px;
    justify-items: start;
    padding-top: 5px;
    min-width: 0;
  }
  .src {
    max-width: 100%;
    gap: 5px;
    overflow: hidden;
  }
  .src :global(svg) {
    flex: none;
  }
  .src .ellipsis {
    min-width: 0;
  }
  .src.via {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
    color: var(--text);
  }
  .src.image {
    border-style: dashed;
  }
  .over {
    font-size: var(--fs-xs);
  }
  .act {
    padding-top: 2px;
  }
  .empty {
    margin: 0;
    padding: 6px 2px;
  }
  .new {
    display: grid;
    grid-template-columns: minmax(150px, 0.9fr) minmax(0, 1.4fr) minmax(0, 1fr) auto;
    gap: 6px;
    align-items: center;
  }
  .new .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .files > summary {
    cursor: pointer;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .files[open] {
    display: grid;
    gap: 6px;
  }
  .file-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .file-row .input {
    max-width: 320px;
    min-height: 32px;
    padding: 4px 8px;
  }
  .small {
    margin: 0;
    font-size: var(--fs-sm);
  }
  textarea {
    margin: 8px 0;
  }
  @media (max-width: 860px) {
    .row {
      grid-template-columns: minmax(0, 1fr) 30px;
    }
    .row .key {
      grid-column: 1;
    }
    .row .value,
    .row .source {
      grid-column: 1 / -1;
    }
    .row .act {
      grid-column: 2;
      grid-row: 1;
    }
    .new {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
