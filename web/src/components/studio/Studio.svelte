<!--
  The stack editor: navigation on the left, the selected service, file or
  overview in the middle, and a save bar that checks, reviews, saves and
  deploys every changed file at once. Used for existing stacks, for new
  ones, and (scoped to one service) on a container's page.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import FileDiff from '@lucide/svelte/icons/file-diff';
  import Redo2 from '@lucide/svelte/icons/redo-2';
  import Rocket from '@lucide/svelte/icons/rocket';
  import Save from '@lucide/svelte/icons/save';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Undo2 from '@lucide/svelte/icons/undo-2';
  import X from '@lucide/svelte/icons/x';
  import Button from '../Button.svelte';
  import Dialog from '../Dialog.svelte';
  import AddService from './AddService.svelte';
  import FileView from './FileView.svelte';
  import Rail from './Rail.svelte';
  import ReviewDialog from './ReviewDialog.svelte';
  import ServiceEditor from './ServiceEditor.svelte';
  import StackView from './StackView.svelte';
  import VariablesEditor from './VariablesEditor.svelte';
  import { stackAction } from '../../lib/actions';
  import { api, errorMessage, seg } from '../../lib/api';
  import { provideStudio, type Selection, type Studio } from '../../lib/compose/studio.svelte';
  import { plural } from '../../lib/format';
  import { router } from '../../lib/router.svelte';
  import { ui } from '../../lib/ui.svelte';

  interface Props {
    studio: Studio;
    /** An existing stack, a new one, or one service of a stack. */
    mode: 'stack' | 'new' | 'service';
    service?: string;
    /** New stacks: called with the result of creating it. */
    oncreated?: (name: string) => void;
  }

  let { studio, mode, service = '', oncreated }: Props = $props();
  // A mounted editor keeps its studio and mode.
  untrack(() => provideStudio(studio));

  let validation = $state<{ ok: boolean; message?: string } | null>(null);
  let validating = $state(false);
  let saving = $state(false);
  let reviewing = $state(false);
  let adding = $state(false);
  let rebuild = $state(true);

  const changes = $derived(studio.changes);
  const needsBuild = $derived(changes.some((f) => f.kind === 'dockerfile' || f.kind === 'dockerignore'));
  const yamlBroken = $derived(Object.keys(studio.yamlErrors).length > 0);
  const nameOk = $derived(/^[a-z0-9][a-z0-9_-]*$/.test(studio.name));

  // Results go stale as soon as anything changes.
  $effect(() => {
    void changes.map((f) => f.text).join('\u0000');
    validation = null;
  });

  // ---------------------------------------------------------------- URL

  function fromQuery(q: URLSearchParams): Selection | null {
    if (q.get('service')) return { kind: 'service', name: q.get('service')!, section: q.get('section') ?? undefined };
    if (q.get('file')) return { kind: 'file', rel: q.get('file')! };
    if (q.get('view') === 'variables') return { kind: 'variables' };
    return null;
  }

  untrack(() => {
    if (mode === 'stack') {
      const initial = fromQuery(router.query);
      if (initial) studio.selection = initial;
    }
    if (mode === 'service') studio.selection = { kind: 'service', name: service };
  });

  // Keep the address in step with what's shown, so it can be bookmarked or shared.
  $effect(() => {
    if (mode !== 'stack') return;
    const s = studio.selection;
    router.setQuery({
      service: s.kind === 'service' ? s.name : null,
      section: s.kind === 'service' && s.section && s.section !== 'general' ? s.section : null,
      file: s.kind === 'file' ? s.rel : null,
      view: s.kind === 'variables' ? 'variables' : null,
    });
  });

  // Files a service refers to (env files, Dockerfiles) are loaded as they appear.
  let discovered = '';
  $effect(() => {
    const key = studio.docs.map((d) => studio.text(d.file)).join('\u0000');
    if (!studio.loaded || key === discovered) return;
    discovered = key;
    const t = setTimeout(() => void studio.discover(), 400);
    return () => clearTimeout(t);
  });

  // ------------------------------------------------------------- saving

  async function validate() {
    validating = true;
    try {
      validation = await api.post<{ ok: boolean; message?: string }>(`/stacks/${seg(studio.name)}/validate`, { files: studio.payload() });
    } catch (err) {
      validation = { ok: false, message: errorMessage(err) };
    } finally {
      validating = false;
    }
  }

  async function save(deploy: boolean) {
    if (saving) return;
    if (mode === 'new') return create(deploy);
    if (!changes.length) {
      if (deploy) await stackAction(studio.name, 'up', mode === 'service' ? [service] : []);
      return;
    }
    if (yamlBroken) {
      validation = { ok: false, message: Object.entries(studio.yamlErrors).map(([f, e]) => `${f}: ${e}`).join('\n') };
      return;
    }
    saving = true;
    try {
      const r = await api.put<{ task_id: string | null }>(`/stacks/${seg(studio.name)}/files`, {
        files: studio.payload(),
        deploy,
        services: mode === 'service' ? [service] : [],
        build: deploy && needsBuild && rebuild,
      });
      studio.markSaved();
      reviewing = false;
      validation = null;
      if (r.task_id) ui.taskStarted(r.task_id, mode === 'service' ? `Recreating ${service}` : `Deploying ${studio.name}`);
      else
        ui.toast('Saved. Deploy to apply the changes.', 'success', {
          label: 'Deploy now',
          run: () => void stackAction(studio.name, 'up', mode === 'service' ? [service] : []),
        });
    } catch (err) {
      // Invalid files come back as a 400 with compose's own message.
      validation = { ok: false, message: errorMessage(err) };
      ui.toast("Not saved: compose doesn't accept the files yet", 'error');
    } finally {
      saving = false;
    }
  }

  async function create(deploy: boolean) {
    if (!nameOk) {
      ui.toast('Give the stack a name first: lowercase letters, digits, - and _', 'warning');
      document.getElementById('stack-name')?.focus();
      return;
    }
    if (!studio.services.length) {
      ui.toast('Add a service first', 'warning');
      return;
    }
    if (yamlBroken) {
      validation = { ok: false, message: Object.entries(studio.yamlErrors).map(([f, e]) => `${f}: ${e}`).join('\n') };
      return;
    }
    saving = true;
    try {
      const env = studio.text(studio.envRel);
      const r = await api.post<{ task_id: string | null }>('/stacks', {
        name: studio.name,
        compose: studio.text(studio.composeRels[0]),
        env: env.trim() ? env : null,
        files: studio.extraFiles(),
        deploy,
      });
      studio.markSaved();
      reviewing = false;
      if (r.task_id) ui.taskStarted(r.task_id, `Deploying ${studio.name}`);
      else ui.toast(`Stack ${studio.name} created`, 'success');
      oncreated?.(studio.name);
    } catch (err) {
      validation = { ok: false, message: errorMessage(err) };
    } finally {
      saving = false;
    }
  }

  untrack(() => (studio.onsave = () => void save(false)));

  // ------------------------------------------------------------ keyboard

  function typing(el: EventTarget | null): boolean {
    const t = el as HTMLElement | null;
    return !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable || !!t.closest?.('.cm-editor'));
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;
    const k = e.key.toLowerCase();
    if (k === 's') {
      e.preventDefault();
      void save(false);
    } else if (k === 'z' && !typing(e.target)) {
      e.preventDefault();
      if (e.shiftKey) studio.redo();
      else studio.undo();
    } else if (k === 'y' && !typing(e.target)) {
      e.preventDefault();
      studio.redo();
    }
  }

  function onBeforeUnload(e: BeforeUnloadEvent) {
    if (changes.length) e.preventDefault();
  }

  // Toasts appear above the save bar instead of covering its buttons.
  let barHeight = $state(0);
  $effect(() => {
    document.documentElement.style.setProperty('--toast-lift', `${barHeight + 8}px`);
    return () => document.documentElement.style.removeProperty('--toast-lift');
  });

  const sel = $derived(studio.selection);
  const summary = $derived.by(() => {
    const names = changes.map((f) => f.rel).join(', ');
    if (mode === 'new') return studio.services.length ? `${plural(changes.length, 'file')} to create: ${names}` : 'Add a service to get started';
    return changes.length ? `${plural(changes.length, 'unsaved file')}: ${names}` : 'No changes';
  });

  // On narrow screens the navigation folds away behind the current selection.
  let railOpen = $state(false);
  const selectionLabel = $derived(
    sel.kind === 'service' ? sel.name : sel.kind === 'file' ? sel.rel : sel.kind === 'variables' ? 'Variables' : mode === 'new' && !studio.services.length ? 'Add a service' : 'Overview',
  );
  $effect(() => {
    void studio.selection;
    railOpen = false;
  });
</script>

<svelte:window onkeydown={onKey} onbeforeunload={onBeforeUnload} />

<div class="studio" class:scoped={mode === 'service'}>
  {#if mode !== 'service'}
    <aside class="side" class:open={railOpen}>
      <button type="button" class="rail-toggle btn" aria-expanded={railOpen} onclick={() => (railOpen = !railOpen)}>
        <span class="ellipsis">{selectionLabel}</span>
        {#if changes.length}<span class="dot"></span>{/if}
        <ChevronDown size={16} />
      </button>
      <div class="rail-wrap"><Rail onadd={() => (adding = true)} /></div>
    </aside>
  {/if}

  <div class="content">
    {#if validation}
      <div class="validation" class:ok={validation.ok}>
        {#if validation.ok}
          <CircleCheck size={16} /> Compose accepts the files as they are now.
        {:else}
          <TriangleAlert size={16} />
          <pre>{validation.message}</pre>
        {/if}
        <button type="button" class="btn ghost icon small" aria-label="Dismiss" onclick={() => (validation = null)}><X size={15} /></button>
      </div>
    {/if}

    {#if mode === 'service'}
      <ServiceEditor name={service} scoped />
    {:else if sel.kind === 'service' && studio.services.includes(sel.name)}
      {#key sel.name}<ServiceEditor name={sel.name} />{/key}
    {:else if sel.kind === 'variables'}
      <div class="pane">
        <h2>Variables</h2>
        <VariablesEditor rel={studio.envRel} />
      </div>
    {:else if sel.kind === 'file' && studio.files[sel.rel]}
      {#key sel.rel}<FileView rel={sel.rel} />{/key}
    {:else if mode === 'new' && !studio.services.length}
      <div class="pane">
        <h2>What should this stack run?</h2>
        <p class="muted">Pick apps to start from, paste a docker run command, or bring in a container you started by hand. Everything stays editable.</p>
        <AddService onadded={(n) => studio.select({ kind: 'service', name: n })} />
      </div>
    {:else}
      <StackView />
    {/if}

    <div class="savebar" class:dirty={changes.length > 0} bind:clientHeight={barHeight}>
      <div class="state" class:clean={!changes.length && mode !== 'new'}>
        {#if changes.length}<span class="dot"></span>{/if}
        <span class="summary" title={summary}>{summary}</span>
      </div>
      <span class="grow"></span>
      <Button small variant="ghost" icon={Undo2} label={studio.canUndo ? `Undo: ${studio.undoLabel.split(':').slice(0, 2).join(' ')}` : 'Undo'} disabled={!studio.canUndo} onclick={() => studio.undo()} />
      <Button small variant="ghost" icon={Redo2} label="Redo" disabled={!studio.canRedo} onclick={() => studio.redo()} />
      {#if mode !== 'new'}
        <Button small variant="ghost" icon={CircleCheck} label="Check the files with compose" busy={validating} onclick={validate} disabled={!changes.length}><span class="lbl">Check</span></Button>
      {/if}
      <Button small variant="ghost" icon={FileDiff} label="Review the changes" onclick={() => (reviewing = true)} disabled={!changes.length}><span class="lbl">Review</span></Button>
      {#if mode === 'new'}
        <Button small icon={Save} busy={saving} onclick={() => save(false)} disabled={!studio.services.length}>Create</Button>
        <Button small variant="primary" icon={Rocket} busy={saving} onclick={() => save(true)} disabled={!studio.services.length}>Create and deploy</Button>
      {:else}
        <Button small icon={Save} busy={saving} onclick={() => save(false)} disabled={!changes.length}>Save</Button>
        <Button small variant="primary" icon={Rocket} busy={saving} onclick={() => save(true)} disabled={!changes.length}>
          {mode === 'service' ? `Save and recreate ${service}` : 'Save and deploy'}
        </Button>
      {/if}
    </div>
  </div>
</div>

<ReviewDialog open={reviewing} onclose={() => (reviewing = false)}>
  {#snippet footer()}
    {#if needsBuild && mode !== 'new'}
      <label class="rebuild"><input type="checkbox" bind:checked={rebuild} /> Rebuild images while deploying</label>
    {/if}
    <button type="button" class="btn ghost" onclick={() => (reviewing = false)}>Keep editing</button>
    <Button icon={Save} busy={saving} onclick={() => save(false)} disabled={!changes.length}>{mode === 'new' ? 'Create' : 'Save'}</Button>
    <Button variant="primary" icon={Rocket} busy={saving} onclick={() => save(true)} disabled={!changes.length}>{mode === 'new' ? 'Create and deploy' : 'Save and deploy'}</Button>
  {/snippet}
</ReviewDialog>

<Dialog open={adding} title="Add a service" width="880px" onclose={() => (adding = false)}>
  {#if adding}
    <AddService
      compact
      onadded={(n) => {
        adding = false;
        studio.select({ kind: 'service', name: n });
      }}
    />
  {/if}
</Dialog>

<style>
  .studio {
    display: grid;
    grid-template-columns: 228px minmax(0, 1fr);
    gap: 20px;
    align-items: start;
  }
  .studio.scoped {
    grid-template-columns: minmax(0, 1fr);
  }
  .side {
    position: sticky;
    top: 118px;
    max-height: calc(100vh - 136px);
    overflow-y: auto;
    padding-bottom: 12px;
  }
  .content {
    display: grid;
    gap: 14px;
    min-width: 0;
  }
  .pane {
    display: grid;
    gap: 12px;
  }
  .pane h2 {
    font-size: var(--fs-lg);
  }
  .pane p {
    margin: 0;
  }
  .validation {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--bad) 50%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
  }
  .validation > :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--bad);
  }
  .validation pre {
    flex: 1;
    min-width: 0;
    color: var(--text);
    font-size: 12.5px;
  }
  .validation.ok {
    align-items: center;
    border-color: color-mix(in srgb, var(--ok) 50%, var(--line));
    background: color-mix(in srgb, var(--ok) 8%, var(--panel));
    color: var(--ok);
  }
  .validation.ok > :global(svg) {
    color: var(--ok);
  }
  .validation .btn {
    margin-left: auto;
  }
  .savebar {
    position: sticky;
    bottom: 12px;
    z-index: 15;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    padding: 8px 10px 8px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .savebar.dirty {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
  }
  .state {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    max-width: 50%;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }
  .summary {
    overflow: hidden;
    color: var(--text-2);
    font-size: var(--fs-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rebuild {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-right: auto;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .rail-toggle {
    display: none;
  }
  @media (max-width: 960px) {
    .studio {
      grid-template-columns: minmax(0, 1fr);
      gap: 12px;
    }
    .side {
      position: static;
      max-height: none;
      padding-bottom: 0;
    }
    .rail-toggle {
      display: flex;
      justify-content: space-between;
      width: 100%;
    }
    .rail-wrap {
      display: none;
      margin-top: 8px;
      padding: 8px;
      border: 1px solid var(--line);
      border-radius: var(--radius-lg);
      background: var(--panel);
    }
    .side.open .rail-wrap {
      display: block;
    }
    .state {
      max-width: 100%;
      flex-basis: 100%;
    }
  }
  @media (max-width: 600px) {
    .savebar {
      bottom: 6px;
      gap: 4px;
      padding: 6px 8px;
    }
    .savebar :global(.btn.ghost) {
      padding: 0 7px;
    }
    .state.clean,
    .lbl {
      display: none;
    }
  }
</style>
