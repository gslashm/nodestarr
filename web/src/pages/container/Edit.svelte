<!--
  Edit a container's settings. A stack's container is edited as its service
  in the stack's compose files (saving can recreate just this service). A
  standalone container is recreated with the new settings (data in volumes
  and folders is kept); if the new one doesn't stay up, the old one is put
  back.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import FileCode from '@lucide/svelte/icons/file-code';
  import Layers from '@lucide/svelte/icons/layers';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import SpecForm from '../../components/SpecForm.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import StudioView from '../../components/studio/Studio.svelte';
  import { api, errorMessage, seg } from '../../lib/api';
  import { Studio } from '../../lib/compose/studio.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail, ContainerSpec, ContainerView } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { c }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  let spec = $state<ContainerSpec | null>(null);
  let original = $state('');
  let loadError = $state('');
  let applying = $state(false);
  let compose = $state<string | null>(null);

  const dirty = $derived(!!spec && JSON.stringify(spec) !== original);

  // A stack's container: edit its service in the stack's files.
  // svelte-ignore state_referenced_locally
  const studio = c.stack && c.service && !c.is_self ? new Studio(c.stack) : null;

  onMount(async () => {
    if (studio) {
      await studio.load();
      return;
    }
    if (c.stack || c.is_self) return;
    try {
      const r = await api.get<{ spec: ContainerSpec; compose: string }>(`/containers/${seg(c.id)}/spec`);
      spec = r.spec;
      original = JSON.stringify(r.spec);
    } catch (err) {
      loadError = errorMessage(err);
    }
  });

  async function apply() {
    if (!spec || applying) return;
    const ok = await ui.confirm({
      title: `Apply changes to ${c.display_name}?`,
      message:
        'The container is replaced with a new one using these settings. Data in volumes and folders is kept. If the new container does not stay up, the old one is put back.',
      confirmLabel: 'Apply and recreate',
    });
    if (!ok) return;
    applying = true;
    try {
      const r = await api.put<{ task_id: string }>(`/containers/${seg(c.id)}/spec`, spec);
      original = JSON.stringify(spec);
      ui.taskStarted(r.task_id, `Applying changes to ${c.display_name}`);
    } catch (err) {
      ui.error(err, "Couldn't apply the changes");
    } finally {
      applying = false;
    }
  }

  async function showCompose() {
    if (!spec) return;
    try {
      const r = await api.post<{ yaml: string }>('/containers/to-compose', spec);
      compose = r.yaml;
    } catch (err) {
      ui.error(err);
    }
  }

  function discard() {
    if (original) spec = JSON.parse(original);
  }
</script>

{#if studio && session.isAdmin}
  {#if studio.loadError}
    <EmptyState icon={Layers} title="Managed by the {c.stack} stack">
      {studio.loadError}
      {#snippet actions()}<a class="btn" href={href(`/stacks/${encodeURIComponent(c.stack ?? '')}`)}>Open the stack</a>{/snippet}
    </EmptyState>
  {:else if !studio.loaded}
    <p class="faint"><Spinner size={13} /> Loading the {c.stack} stack…</p>
  {:else}
    <p class="muted intro-stack">
      {c.display_name} is the <span class="mono">{c.service}</span> service of the
      <a href={href(`/stacks/${encodeURIComponent(c.stack ?? '')}`)}>{c.stack}</a> stack; changes go into its compose files.
      <a href={href(`/stacks/${encodeURIComponent(c.stack ?? '')}/compose?service=${encodeURIComponent(c.service ?? '')}`)}>Open the whole stack in the editor</a>
    </p>
    <StudioView {studio} mode="service" service={c.service ?? ''} />
  {/if}
{:else if c.stack}
  <EmptyState icon={Layers} title="Managed by the {c.stack} stack">
    Change its settings in the stack's compose file, then deploy the stack.
    {#snippet actions()}<a class="btn primary" href={href(`/stacks/${encodeURIComponent(c.stack ?? '')}/compose`)}>Open the stack editor</a>{/snippet}
  </EmptyState>
{:else if c.is_self}
  <EmptyState icon={Layers} title="Nodestarr can't edit its own container">Change Nodestarr's settings where you deployed it (its compose file or run command).</EmptyState>
{:else if !session.isAdmin}
  <EmptyState icon={Layers} title="Only admins can edit containers" />
{:else if loadError}
  <p class="text-bad">{loadError}</p>
{:else if !spec}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else}
  <form
    class="edit"
    onsubmit={(e) => {
      e.preventDefault();
      void apply();
    }}
  >
    <div class="intro">
      <p class="muted">
        Changes are applied by recreating the container. Prefer managing it as code? Copy it as a compose service and create a stack from it.
      </p>
      <button type="button" class="btn small ghost" onclick={showCompose}><FileCode size={15} />Copy as compose</button>
    </div>
    <SpecForm bind:spec editing />
    <div class="savebar" class:dirty>
      <span class="muted">{dirty ? 'You have unsaved changes.' : 'No changes yet.'}</span>
      <span class="grow"></span>
      {#if dirty}<button type="button" class="btn ghost" onclick={discard}>Discard</button>{/if}
      <Button type="submit" variant="primary" busy={applying} disabled={!dirty || c.busy}>Apply and recreate</Button>
    </div>
  </form>
{/if}

<Dialog open={compose !== null} title="{c.display_name} as a compose service" width="720px" onclose={() => (compose = null)}>
  <pre class="yaml">{compose}</pre>
  {#snippet footer()}
    <a class="btn ghost" href={href('/stacks/new')}>Create a stack</a>
    <CopyButton text={compose ?? ''} label="Copy" showLabel small={false} />
  {/snippet}
</Dialog>

<style>
  .edit {
    display: grid;
    gap: 16px;
    max-width: 1080px;
  }
  .intro {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
  }
  .intro p {
    margin: 0;
  }
  .intro-stack {
    margin: 0 0 14px;
  }
  .savebar {
    position: sticky;
    bottom: 12px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .savebar.dirty {
    border-color: var(--accent);
  }
  .yaml {
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre;
    overflow: auto;
  }
</style>
