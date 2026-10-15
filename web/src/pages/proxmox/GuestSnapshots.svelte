<!-- Snapshots of one Proxmox guest: take, roll back to, and delete. -->
<script lang="ts">
  import Camera from '@lucide/svelte/icons/camera';
  import History from '@lucide/svelte/icons/history';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Button from '../../components/Button.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { dateTime } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import type { PveGuest, PveSnapshot } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { guest }: { guest: PveGuest } = $props();

  let snapshots = $state<PveSnapshot[] | null>(null);
  let error = $state('');
  let name = $state(defaultName());
  let description = $state('');
  let includeRam = $state(false);
  let creating = $state(false);

  function defaultName(): string {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, '0');
    return `berth_${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}_${p(d.getHours())}${p(d.getMinutes())}`;
  }

  const nameOk = $derived(/^[A-Za-z][A-Za-z0-9_-]{0,39}$/.test(name));

  async function load() {
    try {
      const r = await api.get<{ snapshots: PveSnapshot[] }>(`/proxmox/guests/${guest.vmid}/snapshots`);
      snapshots = r.snapshots;
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  $effect(() => {
    void guest.vmid;
    void load();
  });

  // Snapshot tasks change the list when they finish.
  let running = 0;
  $effect(() => {
    const n = live.activeTasks.filter((t) => t.kind === 'proxmox').length;
    if (n < running) void load();
    running = n;
  });

  async function create() {
    if (!nameOk || creating) return;
    creating = true;
    try {
      const r = await api.post<{ task_id: string }>(`/proxmox/guests/${guest.vmid}/snapshots`, {
        name,
        description,
        include_ram: includeRam,
      });
      ui.taskStarted(r.task_id, `Taking snapshot ${name} of ${guest.name}`);
      name = defaultName();
      description = '';
    } catch (err) {
      ui.error(err, "Couldn't take the snapshot");
    } finally {
      creating = false;
    }
  }

  async function rollback(s: PveSnapshot) {
    const r = await ui.ask({
      title: `Roll ${guest.name} back to ${s.name}?`,
      message: `Everything on ${guest.name} since ${dateTime(s.snaptime)} is lost.${guest.is_self ? ' This guest runs Berth, so Berth restarts and is unavailable for a moment.' : ''}`,
      confirmLabel: 'Roll back',
      danger: true,
      typeToConfirm: guest.is_self ? guest.name : undefined,
    });
    if (!r.ok) return;
    try {
      const res = await api.post<{ task_id: string }>(`/proxmox/guests/${guest.vmid}/snapshots/${encodeURIComponent(s.name)}/rollback`, {
        confirm_self: guest.is_self,
      });
      ui.taskStarted(res.task_id, `Rolling ${guest.name} back to ${s.name}`);
    } catch (err) {
      ui.error(err, "Couldn't roll back");
    }
  }

  async function remove(s: PveSnapshot) {
    const ok = await ui.confirm({ title: `Delete snapshot ${s.name}?`, confirmLabel: 'Delete', danger: true });
    if (!ok) return;
    try {
      const res = await api.del<{ task_id: string }>(`/proxmox/guests/${guest.vmid}/snapshots/${encodeURIComponent(s.name)}`);
      ui.taskStarted(res.task_id, `Deleting snapshot ${s.name}`);
    } catch (err) {
      ui.error(err, "Couldn't delete it");
    }
  }
</script>

<form
  class="create"
  onsubmit={(e) => {
    e.preventDefault();
    void create();
  }}
>
  <input class="input mono" bind:value={name} aria-label="Snapshot name" class:invalid={!nameOk} />
  <input class="input" bind:value={description} placeholder="Description (optional)" aria-label="Description" />
  {#if guest.kind === 'qemu' && guest.status === 'running'}
    <label class="ram"><input type="checkbox" bind:checked={includeRam} /> Include memory</label>
  {/if}
  <Button type="submit" icon={Camera} busy={creating} disabled={!nameOk}>Take snapshot</Button>
</form>
{#if !nameOk}<p class="text-bad small">Use up to 40 letters, digits, - or _, starting with a letter.</p>{/if}

{#if error}
  <p class="text-bad">{error}</p>
{:else if !snapshots}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else if snapshots.length === 0}
  <p class="muted">No snapshots yet.</p>
{:else}
  <ul class="snaps">
    {#each snapshots as s (s.name)}
      <li>
        <div class="what">
          <span class="mono">{s.name}</span>
          <span class="faint small">{dateTime(s.snaptime)}{s.vmstate ? ', with memory' : ''}{s.description ? `: ${s.description}` : ''}</span>
        </div>
        <button type="button" class="btn small" onclick={() => rollback(s)}><History size={14} />Roll back</button>
        <button type="button" class="btn small ghost icon" onclick={() => remove(s)} aria-label="Delete {s.name}" title="Delete"><Trash2 size={15} /></button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .create {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .create .input {
    flex: 1 1 200px;
  }
  .ram {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-sm);
  }
  .small {
    font-size: var(--fs-sm);
  }
  .snaps {
    display: grid;
    gap: 2px;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }
  .snaps li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }
  .what {
    flex: 1;
    display: grid;
    min-width: 0;
  }
</style>
