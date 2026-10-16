<!--
  Create a compose stack: name it, add services (from the app gallery, a
  docker run command, an existing container, or blank) and shape them in
  the same editor existing stacks use. The files go into a new folder in
  the first stacks directory. A ready compose file can be pasted too.
-->
<script lang="ts">
  import FileCode from '@lucide/svelte/icons/file-code';
  import PageHeader from '../components/PageHeader.svelte';
  import StudioView from '../components/studio/Studio.svelte';
  import { Studio } from '../lib/compose/studio.svelte';
  import { href, router } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();

  const stacksDir = $derived(session.settings?.docker.stacks_dirs[0] ?? '');
  const studio = new Studio('', true);
  studio.startNew('');

  $effect(() => {
    studio.dir = stacksDir && studio.name ? `${stacksDir.replace(/\/$/, '')}/${studio.name}` : '';
  });

  const nameOk = $derived(/^[a-z0-9][a-z0-9_-]*$/.test(studio.name));
</script>

<PageHeader title="Create a stack" back={{ label: 'Stacks', path: '/stacks' }} />

<div class="page-body">
  {#if !session.isAdmin}
    <p class="muted">Only admins can create stacks.</p>
  {:else if !stacksDir}
    <p class="muted">Set a stacks folder in <a href={href('/settings/docker')}>Settings > Docker</a> first; new stacks get a folder in it.</p>
  {:else}
    <div class="naming">
      <label for="stack-name">Name</label>
      <input
        id="stack-name"
        class="input mono"
        value={studio.name}
        placeholder="media"
        autocomplete="off"
        spellcheck="false"
        aria-invalid={!!studio.name && !nameOk}
        oninput={(e) => (studio.name = e.currentTarget.value.toLowerCase().replace(/[^a-z0-9_-]/g, '-'))}
      />
      <span class="where" class:bad={!!studio.name && !nameOk}>
        {#if studio.name && !nameOk}Start with a letter or digit; use lowercase letters, digits, - and _.{:else}Files go in <span class="mono">{stacksDir.replace(/\/$/, '')}/{studio.name || 'name'}</span>{/if}
      </span>
      <span class="grow"></span>
      <button type="button" class="btn small ghost" onclick={() => studio.select({ kind: 'file', rel: 'compose.yaml' })}><FileCode size={15} />Paste a compose file instead</button>
    </div>
    <StudioView {studio} mode="new" oncreated={(n) => router.navigate(`/stacks/${encodeURIComponent(n)}`)} />
  {/if}
</div>

<style>
  .naming {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    margin-bottom: 18px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .naming label {
    font-weight: 600;
  }
  .naming .input {
    width: 220px;
  }
  .where {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .where.bad {
    color: var(--bad);
  }
</style>
