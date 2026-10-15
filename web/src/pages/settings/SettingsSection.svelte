<!--
  One section of the server settings as an editable form. Holds a copy of
  the section, shows a save bar when it changed, and saves it as a whole.
  Secrets come back as a placeholder and are kept unless replaced.
-->
<script lang="ts" generics="K extends keyof Settings">
  import { untrack, type Snippet } from 'svelte';
  import Button from '../../components/Button.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { session } from '../../lib/session.svelte';
  import type { Settings } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  interface Props {
    section: K;
    title: string;
    intro?: string;
    children: Snippet<[Settings[K]]>;
    /** Extra buttons in the header (tests, links). */
    actions?: Snippet<[Settings[K]]>;
    onsaved?: (s: Settings) => void;
  }

  let { section, title, intro = '', children, actions, onsaved }: Props = $props();

  function clone<T>(v: T): T {
    return JSON.parse(JSON.stringify(v));
  }

  let baseline = $state('');
  let form = $state<Settings[K] | null>(null);
  let saving = $state(false);
  let error = $state('');

  const dirty = $derived(!!form && JSON.stringify(form) !== baseline);

  function reset(value: Settings[K]) {
    form = clone(value);
    baseline = JSON.stringify(value);
    error = '';
  }

  // Load, and follow changes saved elsewhere while nothing is being edited here.
  $effect(() => {
    const current = session.settings?.[section];
    if (!current) return;
    untrack(() => {
      if (!form || !dirty) reset(current);
    });
  });

  async function save() {
    if (!form || saving) return;
    saving = true;
    error = '';
    try {
      const r = await api.put<{ settings: Settings }>(`/settings/${section}`, form);
      session.settings = r.settings;
      session.applyLook();
      reset(r.settings[section]);
      ui.toast(`${title} settings saved`, 'success');
      onsaved?.(r.settings);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      saving = false;
    }
  }

  function discard() {
    if (session.settings) reset(session.settings[section]);
    // Undo any live preview (accent, density) of the discarded values.
    session.applyLook();
  }
</script>

<form
  class="section"
  onsubmit={(e) => {
    e.preventDefault();
    void save();
  }}
>
  <header>
    <div class="titles">
      <h2>{title}</h2>
      {#if intro}<p class="muted">{intro}</p>{/if}
    </div>
    {#if actions && form}<div class="actions">{@render actions(form)}</div>{/if}
  </header>
  {#if form}
    <fieldset disabled={!session.isAdmin}>
      {@render children(form)}
    </fieldset>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if session.isAdmin && dirty}
    <div class="savebar">
      <span>Unsaved changes</span>
      <span class="grow"></span>
      <button type="button" class="btn ghost" onclick={discard}>Discard</button>
      <Button type="submit" variant="primary" busy={saving}>Save</Button>
    </div>
  {/if}
</form>

<style>
  .section {
    display: grid;
    gap: 14px;
    max-width: 900px;
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .titles {
    flex: 1;
  }
  .titles p {
    margin: 4px 0 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  fieldset {
    display: grid;
    gap: 16px;
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .error {
    margin: 0;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--bad) 50%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
    color: var(--text);
  }
  .savebar {
    position: sticky;
    bottom: 12px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
</style>
