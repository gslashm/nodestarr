<!-- The services of a stack with their live state and per-service actions. -->
<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Play from '@lucide/svelte/icons/play';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Square from '@lucide/svelte/icons/square';
  import AppIcon from '../../components/AppIcon.svelte';
  import StatePill from '../../components/StatePill.svelte';
  import { stackAction } from '../../lib/actions';
  import { clock } from '../../lib/clock.svelte';
  import { bytes, duration, hasUpdate, isUp, pct, shortImage, stateOf, updateLabel } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { StackDetail } from '../../lib/types';

  let { name, detail }: { name: string; detail: StackDetail; reload: () => Promise<void> } = $props();

  const rows = $derived(
    detail.services.map((s) => {
      const c = s.container ? live.byIdentity.get(s.container.identity) : undefined;
      return { s, c, svc: s.name ?? s.container?.service ?? '' };
    }),
  );
</script>

{#if detail.config_error}
  <div class="alert">
    <CircleAlert size={18} />
    <div>
      <strong>The compose files have a problem</strong>
      <pre>{detail.config_error}</pre>
    </div>
  </div>
{/if}

<section class="panel">
  <div class="table-wrap">
    <table class="table">
      <thead>
        <tr>
          <th>Service</th>
          <th>State</th>
          <th class="hide-sm">Image</th>
          <th>Version</th>
          <th class="num hide-sm">CPU</th>
          <th class="num hide-sm">Memory</th>
          <th class="shrink"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as { s, c, svc }, i (svc || s.container?.id || i)}
          {@const st = c ? stateOf(c) : null}
          <tr>
            <td>
              <div class="svc">
                {#if c}
                  <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={26} />
                  <div class="names">
                    <a href={href(`/containers/${encodeURIComponent(c.name)}`)}>{svc}</a>
                    {#if c.display_name.toLowerCase() !== svc.toLowerCase()}<span class="faint small">{c.display_name}</span>{/if}
                  </div>
                {:else}
                  <span class="ghost-icon"></span>
                  <div class="names">
                    <span>{svc}</span>
                    {#if s.profiles?.length}<span class="faint small">profile {s.profiles.join(', ')}</span>{/if}
                  </div>
                {/if}
              </div>
            </td>
            <td>
              {#if st && c}
                <StatePill state={st} />
                {#if isUp(st) && c.started_at}<span class="faint small up">{duration(clock.now - c.started_at)}</span>{/if}
              {:else}
                <span class="faint">Not created</span>
              {/if}
            </td>
            <td class="hide-sm mono small ellipsis image" title={s.image ?? c?.image ?? ''}>
              {s.built ? 'built here' : shortImage(s.image ?? c?.image ?? '')}
            </td>
            <td class="ver">
              {c?.version ?? '–'}
              {#if c && hasUpdate(c.update)}<a class="chip upd" href={href(`/containers/${encodeURIComponent(c.name)}/updates`)}>{updateLabel(c.update)}</a>{/if}
            </td>
            <td class="num hide-sm">{c?.stats && st && isUp(st) ? pct(c.stats.cpu, 1) : '–'}</td>
            <td class="num hide-sm">{c?.stats && st && isUp(st) ? bytes(c.stats.mem) : '–'}</td>
            <td class="shrink">
              {#if session.isAdmin && svc}
                <div class="actions">
                  {#if c && hasUpdate(c.update)}
                    <button type="button" class="btn small ghost icon" title="Pull and update {svc}" aria-label="Update {svc}" onclick={() => stackAction(name, 'update', [svc])}><ArrowUp size={15} /></button>
                  {/if}
                  {#if !c}
                    <button type="button" class="btn small" onclick={() => stackAction(name, 'up', [svc])}><Play size={14} />Create</button>
                  {:else if st && (isUp(st) || st === 'restarting')}
                    <button type="button" class="btn small ghost icon" title="Restart {svc}" aria-label="Restart {svc}" onclick={() => stackAction(name, 'restart', [svc])}><RotateCw size={15} /></button>
                    <button type="button" class="btn small ghost icon" title="Stop {svc}" aria-label="Stop {svc}" onclick={() => stackAction(name, 'stop', [svc])} disabled={c.is_self}><Square size={15} /></button>
                  {:else}
                    <button type="button" class="btn small ghost icon" title="Start {svc}" aria-label="Start {svc}" onclick={() => stackAction(name, 'start', [svc])}><Play size={15} /></button>
                  {/if}
                </div>
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td colspan="7" class="faint">No services found in the compose files.</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<section class="panel files">
  <div class="panel-head"><h2>Files</h2></div>
  <div class="panel-body">
    {#if detail.stack.dir}<p class="mono small">{detail.stack.dir}</p>{/if}
    <ul>
      {#each detail.stack.files as f (f)}<li class="mono small">{f}</li>{/each}
    </ul>
    {#if !detail.stack.editable}
      <p class="muted small">Berth can't read these files, so the stack can be controlled but not edited. Mount the folder into Berth at the same path to edit it here.</p>
    {:else if !detail.stack.in_stacks_dir}
      <p class="faint small">This stack lives outside the stacks folders set in Settings > Docker. It works the same; it's just not grouped with the others.</p>
    {/if}
  </div>
</section>

<style>
  .alert {
    display: flex;
    gap: 12px;
    margin-bottom: 16px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--bad);
    border-radius: var(--radius);
    background: var(--panel);
  }
  .alert :global(svg) {
    flex: none;
    color: var(--bad);
  }
  .alert pre {
    margin-top: 4px;
    color: var(--text-2);
    font-size: 12.5px;
  }
  .svc {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .names {
    display: grid;
    min-width: 0;
  }
  .names a {
    color: var(--text);
    font-weight: 500;
  }
  .ghost-icon {
    flex: none;
    width: 26px;
    height: 26px;
    border: 1px dashed var(--line-strong);
    border-radius: 7px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .up {
    margin-left: 6px;
  }
  .image {
    max-width: 260px;
    color: var(--text-2);
  }
.ver {
    font-family: var(--font-data);
    font-variant-numeric: tabular-nums;
  }
  .upd {
    margin-left: 6px;
    color: var(--update);
    text-decoration: none;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }
  .files {
    margin-top: 16px;
  }
  .files ul {
    margin: 6px 0 10px;
    padding-left: 18px;
  }
  .files p {
    margin: 0 0 6px;
  }
  @media (max-width: 760px) {
    .hide-sm {
      display: none;
    }
  }
</style>
