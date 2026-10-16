<!--
  A Dockerfile as cards, one per instruction, grouped by build stage. Each
  kind of instruction gets fitting fields (FROM: image and stage name, COPY:
  sources and destination, ENV: a list...). Drag cards to reorder them.
  Instructions nobody touched are written back exactly as they were.
-->
<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ChevronUp from '@lucide/svelte/icons/chevron-up';
  import Code from '@lucide/svelte/icons/code';
  import GripVertical from '@lucide/svelte/icons/grip-vertical';
  import Lightbulb from '@lucide/svelte/icons/lightbulb';
  import Plus from '@lucide/svelte/icons/plus';
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import CodeEditor from '../CodeEditor.svelte';
  import Menu, { type MenuEntry } from '../Menu.svelte';
  import KeyValueList from './KeyValueList.svelte';
  import {
    insertInstruction,
    KEYWORD_HELP,
    KEYWORDS,
    lintDockerfile,
    moveInstruction,
    parseCopy,
    parseDockerfile,
    parseExec,
    parseFrom,
    parseHealth,
    parsePairs,
    removeInstruction,
    renderCopy,
    renderExec,
    renderFrom,
    renderHealth,
    renderPairs,
    serializeDockerfile,
    setInstruction,
    stages,
    type DfInstruction,
    type Dockerfile,
  } from '../../lib/compose/dockerfiles';
  import { buildFiles, joinPath, useStudio } from '../../lib/compose/studio.svelte';
  import { DOCKERFILE_TEMPLATES } from '../../lib/compose/templates';
  import { get, serviceMap } from '../../lib/compose/service';
  import { shellJoin, shellSplit } from '../../lib/spec';
  import { ui } from '../../lib/ui.svelte';

  let { rel }: { rel: string } = $props();
  const studio = useStudio();

  let asCode = $state(false);
  let showHints = $state(true);
  let dragFrom = $state<number | null>(null);
  let dragOver = $state<number | null>(null);

  const text = $derived(studio.text(rel));
  const df = $derived(parseDockerfile(text));
  const groups = $derived(stages(df));
  const hints = $derived(lintDockerfile(df));
  const hintsByLine = $derived(new Map(hints.map((h) => [h.line, h])));

  /** Services built from this Dockerfile. */
  const usedBy = $derived(
    studio.services.filter((s) =>
      studio.docs.some(({ doc }) => {
        const svc = serviceMap(doc, s);
        const b = svc ? buildFiles(get(doc, svc, 'build')) : null;
        return b?.dockerfile === rel;
      }),
    ),
  );

  function write(next: Dockerfile, what: string) {
    studio.setText(rel, serializeDockerfile(next), `df:${rel}:${what}`);
  }

  const update = (index: number, ins: DfInstruction, args: string) => write(setInstruction(df, index, ins.keyword, args), `ins${index}`);

  function useTemplate(key: string) {
    const t = DOCKERFILE_TEMPLATES.find((x) => x.key === key);
    if (!t) return;
    studio.setText(rel, t.content(''), `df:${rel}:template`);
    const ignore = joinPath(rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '.', '.dockerignore');
    void studio.open(ignore, 'dockerignore').then((f) => {
      if (!f.text && f.original === null) studio.setText(ignore, t.ignore, `df:${rel}:template`);
    });
  }

  /** Where a new instruction goes: after `after` (an item index), or at the end of a stage. */
  function insert(after: number, keyword: string) {
    const defaults: Record<string, string> = {
      FROM: 'alpine:3.22',
      RUN: '',
      COPY: '. /app',
      ADD: '',
      ENV: 'KEY=value',
      ARG: 'NAME',
      WORKDIR: '/app',
      USER: 'app',
      EXPOSE: '8080',
      VOLUME: '/data',
      LABEL: 'org.opencontainers.image.title=app',
      CMD: renderExec(['app']),
      ENTRYPOINT: renderExec(['app']),
      HEALTHCHECK: 'CMD curl -fsS http://localhost:8080/ || exit 1',
      STOPSIGNAL: 'SIGTERM',
      SHELL: renderExec(['/bin/sh', '-c']),
    };
    write(insertInstruction(df, after + 1, keyword, defaults[keyword] ?? ''), `insert`);
    setTimeout(() => document.querySelector<HTMLElement>(`[data-df-item="${after + 1}"] input, [data-df-item="${after + 1}"] textarea`)?.focus(), 30);
  }

  function addMenu(after: number): MenuEntry[] {
    return KEYWORDS.filter((k) => k !== 'MAINTAINER' && k !== 'ONBUILD').map((k) => ({ label: k, hint: KEYWORD_HELP[k], run: () => insert(after, k) }));
  }

  /** Item index of the previous / next instruction, for moving. */
  function neighbour(index: number, dir: -1 | 1): number | null {
    for (let i = index + dir; i >= 0 && i < df.items.length; i += dir) if (df.items[i].kind === 'instruction') return i;
    return null;
  }

  function move(index: number, dir: -1 | 1) {
    const to = neighbour(index, dir);
    if (to === null) return;
    write(moveInstruction(df, index, to), 'move');
  }

  function rowMenu(index: number): MenuEntry[] {
    return [
      { label: 'Move up', icon: ChevronUp, run: () => move(index, -1), disabled: neighbour(index, -1) === null },
      { label: 'Move down', icon: ChevronDown, run: () => move(index, 1), disabled: neighbour(index, 1) === null },
      'separator',
      { label: 'Remove', icon: Trash2, danger: true, run: () => write(removeInstruction(df, index), 'remove') },
    ];
  }

  function onKey(e: KeyboardEvent, index: number) {
    if (!e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return;
    e.preventDefault();
    move(index, e.key === 'ArrowUp' ? -1 : 1);
  }

  function drop(e: DragEvent, index: number) {
    e.preventDefault();
    if (dragFrom !== null && dragFrom !== index) write(moveInstruction(df, dragFrom, index), 'move');
    dragFrom = null;
    dragOver = null;
  }

  const CATEGORY: Record<string, string> = {
    FROM: 'base',
    RUN: 'run',
    COPY: 'files',
    ADD: 'files',
    ENV: 'config',
    ARG: 'config',
    LABEL: 'config',
    WORKDIR: 'config',
    USER: 'config',
    EXPOSE: 'config',
    VOLUME: 'config',
    CMD: 'start',
    ENTRYPOINT: 'start',
    HEALTHCHECK: 'start',
    STOPSIGNAL: 'start',
    SHELL: 'start',
  };

  function stageItems(members: number[]): number[] {
    // Comments and blank lines between this stage's instructions are shown too.
    if (!members.length) return [];
    const out: number[] = [];
    for (let i = members[0]; i <= members[members.length - 1]; i++) out.push(i);
    return out;
  }

  function lead(): number[] {
    const first = df.items.findIndex((i) => i.kind === 'instruction');
    return df.items.map((_, i) => i).filter((i) => first < 0 || i < first);
  }

  function setPairs(index: number, ins: DfInstruction, pairs: { key: string; value: string | null }[]) {
    update(
      index,
      ins,
      renderPairs(
        ins.keyword,
        pairs.map((p) => ({ key: p.key, value: p.value ?? '' })),
      ),
    );
  }
</script>

<div class="df">
  <div class="toolbar">
    {#if usedBy.length}<span class="muted small">Builds {usedBy.join(', ')}</span>{:else}<span class="faint small">No service builds from this file yet.</span>{/if}
    <span class="grow"></span>
    {#if !asCode && text.trim()}
      <Menu
        label="Start from a template"
        small
        items={DOCKERFILE_TEMPLATES.map((t) => ({
          label: t.name,
          hint: t.description,
          icon: Sparkles,
          run: async () => {
            if (await ui.confirm({ title: `Replace ${rel} with the ${t.name} template?`, confirmLabel: 'Replace', danger: true })) useTemplate(t.key);
          },
        }))}
      />
    {/if}
    <button type="button" class="btn small ghost" onclick={() => (asCode = !asCode)}><Code size={15} />{asCode ? 'Edit as cards' : 'Edit as text'}</button>
  </div>

  {#if asCode}
    <CodeEditor bind:value={() => text, (v) => studio.setText(rel, v, `df:${rel}:code`)} language="dockerfile" height="560px" label="Contents of {rel}" onsave={() => studio.requestSave()} />
  {:else if !text.trim()}
    <div class="empty">
      <h3>Start the Dockerfile</h3>
      <p class="muted">Pick a starting point; every line can be changed afterwards.</p>
      <div class="templates">
        {#each DOCKERFILE_TEMPLATES as t (t.key)}
          <button type="button" class="template" onclick={() => useTemplate(t.key)}>
            <strong>{t.name}</strong>
            <span class="muted">{t.description}</span>
          </button>
        {/each}
      </div>
    </div>
  {:else}
    {#if hints.length}
      <div class="hints">
        <button type="button" class="hints-head" onclick={() => (showHints = !showHints)} aria-expanded={showHints}>
          <Lightbulb size={15} />{hints.length} {hints.length === 1 ? 'suggestion' : 'suggestions'}<ChevronDown size={14} class={showHints ? 'flip' : ''} />
        </button>
        {#if showHints}
          <ul>
            {#each hints as h, i (i)}
              <li class={h.level}>
                <button type="button" onclick={() => document.querySelector(`[data-df-line="${h.line}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' })}>Line {h.line}</button>
                {h.message}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}

    {#snippet item(index: number)}
      {@const it = df.items[index]}
      {#if it.kind === 'comment'}
        <p class="comment">{it.text.trim()}</p>
      {:else if it.kind === 'directive'}
        <p class="comment directive">{it.text.trim()}</p>
      {:else if it.kind === 'instruction'}
        {@const ins = it}
        {@const hint = hintsByLine.get(ins.line)}
        <!-- A drop target for dragged cards; Alt+arrow presses bubble up from its fields. -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          class="card {CATEGORY[ins.keyword] ?? 'other'}"
          class:over={dragOver === index && dragFrom !== index}
          class:dragging={dragFrom === index}
          data-df-item={index}
          data-df-line={ins.line}
          role="group"
          aria-label="{ins.keyword} instruction"
          ondragover={(e) => {
            if (dragFrom === null) return;
            e.preventDefault();
            dragOver = index;
          }}
          ondragleave={() => dragOver === index && (dragOver = null)}
          ondrop={(e) => drop(e, index)}
          onkeydown={(e) => onKey(e, index)}
        >
          <span
            class="grip"
            draggable="true"
            role="button"
            tabindex="-1"
            aria-label="Drag to move"
            ondragstart={(e) => {
              dragFrom = index;
              e.dataTransfer?.setData('text/plain', String(index));
            }}
            ondragend={() => ((dragFrom = null), (dragOver = null))}><GripVertical size={15} /></span
          >
          <span class="kw" title={KEYWORD_HELP[ins.keyword] ?? ''}>{ins.keyword}</span>
          <div class="args">
            {#if ins.heredoc}
              <textarea class="input mono" rows={Math.min(14, ins.args.split('\n').length + 1)} value={ins.args} spellcheck="false" aria-label="{ins.keyword} arguments" oninput={(e) => update(index, ins, e.currentTarget.value)}></textarea>
            {:else if ins.keyword === 'FROM'}
              {@const f = parseFrom(ins.args)}
              <div class="from">
                <input class="input mono" value={f.image} placeholder="image:tag" spellcheck="false" aria-label="Base image" oninput={(e) => update(index, ins, renderFrom({ ...f, image: e.currentTarget.value.trim() }))} />
                <span class="faint">as</span>
                <input class="input mono stage" value={f.alias} placeholder="stage name" spellcheck="false" aria-label="Stage name" oninput={(e) => update(index, ins, renderFrom({ ...f, alias: e.currentTarget.value.trim() }))} />
              </div>
            {:else if ins.keyword === 'RUN' && !parseExec(ins.args)}
              <textarea class="input mono" rows={Math.min(14, Math.max(1, ins.args.split('\n').length))} value={ins.args} placeholder="apt-get update" spellcheck="false" aria-label="Commands" oninput={(e) => update(index, ins, e.currentTarget.value)}></textarea>
              <span class="faint tiny">One line per continued line; they're joined with \ when written.</span>
            {:else if ins.keyword === 'COPY' || ins.keyword === 'ADD'}
              {@const c = parseCopy(ins.args)}
              <div class="copy">
                <input class="input mono" value={c.sources.join(' ')} placeholder="source files" spellcheck="false" aria-label="Copy from" oninput={(e) => update(index, ins, renderCopy({ ...c, sources: shellSplit(e.currentTarget.value) }))} />
                <span class="faint">→</span>
                <input class="input mono" value={c.dest} placeholder="/app/" spellcheck="false" aria-label="Copy to" oninput={(e) => update(index, ins, renderCopy({ ...c, dest: e.currentTarget.value }))} />
              </div>
              <input
                class="input mono flags"
                value={c.flags.map((f) => (f.value ? `--${f.name}=${f.value}` : `--${f.name}`)).join(' ')}
                placeholder="Options, like --chown=1000:1000 or --from=build"
                spellcheck="false"
                aria-label="Options"
                onchange={(e) =>
                  update(
                    index,
                    ins,
                    renderCopy({
                      ...c,
                      flags: e.currentTarget.value
                        .split(/\s+/)
                        .filter(Boolean)
                        .map((w) => {
                          const m = /^--([^=]+)(?:=(.*))?$/.exec(w);
                          return m ? { name: m[1], value: m[2] ?? '' } : { name: w.replace(/^-+/, ''), value: '' };
                        }),
                    }),
                  )}
              />
            {:else if ins.keyword === 'ENV' || ins.keyword === 'ARG' || ins.keyword === 'LABEL'}
              <KeyValueList entries={parsePairs(ins.keyword, ins.args)} onchange={(p) => setPairs(index, ins, p)} keyPlaceholder="NAME" valuePlaceholder={ins.keyword === 'ARG' ? 'default (optional)' : 'value'} addLabel="Add" />
            {:else if ins.keyword === 'CMD' || ins.keyword === 'ENTRYPOINT'}
              {@const exec = parseExec(ins.args)}
              <div class="cmd">
                <select class="input form" value={exec ? 'exec' : 'shell'} aria-label="Form" onchange={(e) => update(index, ins, e.currentTarget.value === 'exec' ? renderExec(shellSplit(ins.args.replace(/\n/g, ' '))) : shellJoin(exec ?? []))}>
                  <option value="exec">Program and arguments</option>
                  <option value="shell">Shell command</option>
                </select>
                <input
                  class="input mono"
                  value={exec ? shellJoin(exec) : ins.args.replace(/\n/g, ' ')}
                  spellcheck="false"
                  aria-label="Command"
                  oninput={(e) => update(index, ins, exec ? renderExec(shellSplit(e.currentTarget.value)) : e.currentTarget.value)}
                />
              </div>
            {:else if ins.keyword === 'HEALTHCHECK'}
              {@const h = parseHealth(ins.args)}
              {#if h.none}
                <span class="muted">Turns off the base image's health check.</span>
              {:else}
                <input class="input mono" value={h.exec ? shellJoin(h.exec) : h.shell} placeholder="curl -fsS http://localhost/ || exit 1" spellcheck="false" aria-label="Health check command" oninput={(e) => update(index, ins, renderHealth({ ...h, exec: null, shell: e.currentTarget.value }))} />
                <div class="health">
                  <label>Every <input class="input mono" value={h.interval} placeholder="30s" oninput={(e) => update(index, ins, renderHealth({ ...h, interval: e.currentTarget.value.trim() }))} /></label>
                  <label>Timeout <input class="input mono" value={h.timeout} placeholder="30s" oninput={(e) => update(index, ins, renderHealth({ ...h, timeout: e.currentTarget.value.trim() }))} /></label>
                  <label>Retries <input class="input mono" value={h.retries} placeholder="3" oninput={(e) => update(index, ins, renderHealth({ ...h, retries: e.currentTarget.value.trim() }))} /></label>
                  <label>Grace <input class="input mono" value={h.startPeriod} placeholder="0s" oninput={(e) => update(index, ins, renderHealth({ ...h, startPeriod: e.currentTarget.value.trim() }))} /></label>
                </div>
              {/if}
            {:else}
              <input class="input mono" value={ins.args.replace(/\n/g, ' ')} spellcheck="false" aria-label="{ins.keyword} arguments" oninput={(e) => update(index, ins, e.currentTarget.value)} />
            {/if}
            {#if hint}<span class="hint {hint.level}"><TriangleAlert size={13} />{hint.message}</span>{/if}
          </div>
          <div class="tools"><Menu items={rowMenu(index)} label="Actions for {ins.keyword}" small /></div>
        </div>
        <div class="gap">
          <Menu items={addMenu(index)} label="Add an instruction here" small>
            {#snippet trigger()}<span class="add-here"><Plus size={13} /></span>{/snippet}
          </Menu>
        </div>
      {/if}
    {/snippet}

    {#each lead() as i (i)}{@render item(i)}{/each}
    {#each groups as g, gi (g.from + ':' + gi)}
      <section class="stage">
        <h3>
          {groups.length > 1 ? `Stage ${gi + 1}` : 'Build'}{#if g.name}: <span class="mono">{g.name}</span>{/if}
          {#if g.image}<span class="faint small">from <span class="mono">{g.image}</span></span>{/if}
          {#if gi === groups.length - 1 && groups.length > 1}<span class="chip">the image</span>{/if}
        </h3>
        {#each stageItems(g.members) as i (i)}{@render item(i)}{/each}
      </section>
    {/each}
    <div class="end">
      <Menu items={addMenu(df.items.length - 1)} label="Add an instruction" small>
        {#snippet trigger()}<span class="btn small"><Plus size={15} />Add instruction</span>{/snippet}
      </Menu>
      <span class="faint small">Drag cards by their handle, or press Alt+↑/↓ inside one, to reorder.</span>
    </div>
  {/if}
</div>

<style>
  .df {
    display: grid;
    gap: 10px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .tiny {
    font-size: var(--fs-xs);
  }
  .empty {
    display: grid;
    gap: 8px;
    padding: 18px;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
  }
  .empty p {
    margin: 0;
  }
  .templates {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    gap: 8px;
  }
  .template {
    display: grid;
    gap: 4px;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .template:hover {
    border-color: var(--accent);
  }
  .template span {
    font-size: var(--fs-sm);
  }
  .hints {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
  }
  .hints-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    border: 0;
    background: none;
    color: var(--text-2);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .hints-head :global(.flip) {
    transform: rotate(180deg);
  }
  .hints ul {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0 12px 10px 36px;
    font-size: var(--fs-sm);
  }
  .hints li.warn {
    color: var(--warn);
  }
  .hints li button {
    margin-right: 6px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
  .stage {
    display: grid;
    gap: 0;
  }
  .stage h3 {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    margin: 6px 0 8px;
    font-size: var(--fs-sm);
    color: var(--text-2);
  }
  .comment {
    margin: 4px 0 2px 34px;
    color: var(--text-3);
    font: italic 400 12.5px / 1.4 var(--font-code);
    white-space: pre-wrap;
  }
  .comment.directive {
    font-style: normal;
  }
  .card {
    --kw: var(--text-3);
    display: grid;
    grid-template-columns: 18px 104px minmax(0, 1fr) 30px;
    gap: 8px;
    align-items: start;
    padding: 8px 8px 8px 6px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--kw);
    border-radius: var(--radius);
    background: var(--panel);
  }
  .card.base {
    --kw: var(--accent);
  }
  .card.run {
    --kw: #5ab0f0;
  }
  .card.files {
    --kw: #2a8f86;
  }
  .card.config {
    --kw: #a88426;
  }
  .card.start {
    --kw: #c26a2e;
  }
  .card.over {
    box-shadow: 0 -2px 0 var(--accent);
  }
  .card.dragging {
    opacity: 0.5;
  }
  .grip {
    padding-top: 7px;
    color: var(--text-3);
    cursor: grab;
  }
  .kw {
    padding-top: 6px;
    color: var(--text);
    font: 600 12.5px / 1.4 var(--font-code);
    letter-spacing: 0.02em;
  }
  .args {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .args .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .args textarea.input {
    min-height: 34px;
    resize: vertical;
  }
  .from,
  .copy,
  .cmd {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .from .input,
  .copy .input,
  .cmd .input {
    flex: 1;
    min-width: 0;
  }
  .from .stage {
    flex: 0 1 180px;
  }
  .cmd .form {
    flex: 0 0 auto;
    width: auto;
  }
  .flags {
    font-size: 12.5px;
  }
  .health {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
  }
  .health label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .health .input {
    width: 70px;
  }
  .hint {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-xs);
  }
  .hint.warn {
    color: var(--warn);
  }
  .gap {
    display: flex;
    justify-content: center;
    height: 14px;
    opacity: 0;
    transition: opacity 120ms var(--ease);
  }
  .gap:hover,
  .gap:focus-within {
    opacity: 1;
  }
  .add-here {
    display: grid;
    place-items: center;
    width: 22px;
    height: 14px;
    border-radius: 7px;
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .end {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
  }
  @media (max-width: 760px) {
    .card {
      grid-template-columns: 18px minmax(0, 1fr) 30px;
    }
    .kw {
      grid-column: 2;
    }
    .args {
      grid-column: 1 / -1;
    }
  }
</style>
