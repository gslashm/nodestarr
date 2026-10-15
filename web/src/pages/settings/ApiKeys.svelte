<!-- API keys for scripts and other apps (Home Assistant, Homepage, scripts). -->
<script lang="ts">
  import { onMount } from 'svelte';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { api, basePath, errorMessage } from '../../lib/api';
  import type { ApiKeyInfo, Role } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let keys = $state<ApiKeyInfo[] | null>(null);
  let error = $state('');
  let name = $state('');
  let role = $state<Role>('admin');
  let creating = $state(false);
  let created = $state<{ key: string; name: string } | null>(null);

  async function load() {
    try {
      keys = await api.get<ApiKeyInfo[]>('/apikeys');
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(load);

  async function create() {
    creating = true;
    try {
      const r = await api.post<{ key: string; info: ApiKeyInfo }>('/apikeys', { name: name.trim(), role });
      created = { key: r.key, name: r.info.name };
      name = '';
      await load();
    } catch (err) {
      ui.error(err, "Couldn't create the key");
    } finally {
      creating = false;
    }
  }

  async function revoke(k: ApiKeyInfo) {
    const ok = await ui.confirm({ title: `Revoke "${k.name}"?`, message: 'Anything using this key stops working right away.', confirmLabel: 'Revoke', danger: true });
    if (!ok) return;
    try {
      await api.del(`/apikeys/${k.id}`);
      await load();
    } catch (err) {
      ui.error(err);
    }
  }

  const origin = `${location.origin}${basePath()}`;
</script>

<div class="keys">
  <header>
    <h2>API keys</h2>
    <p class="muted">For scripts and dashboards. Send the key in an <code>X-Api-Key</code> header (or <code>Authorization: Bearer</code>).</p>
  </header>

  {#if created}
    <div class="created">
      <p><strong>Copy the key for “{created.name}” now.</strong> It won't be shown again.</p>
      <div class="keyline">
        <code class="key">{created.key}</code>
        <CopyButton text={created.key} label="Copy key" showLabel small={false} />
      </div>
      <p class="small muted">Try it: <code>curl -H "X-Api-Key: {created.key.slice(0, 8)}…" {origin}/api/v1/containers</code></p>
      <button type="button" class="btn small done" onclick={() => (created = null)}>I've copied it</button>
    </div>
  {/if}

  {#if error}
    <p class="text-bad">{error}</p>
  {:else if keys && keys.length}
    <section class="panel">
      <div class="table-wrap">
        <table class="table">
          <thead><tr><th>Name</th><th>Key</th><th>Role</th><th>Last used</th><th class="shrink"></th></tr></thead>
          <tbody>
            {#each keys as k (k.id)}
              <tr>
                <td>{k.name}</td>
                <td class="mono small">{k.prefix}…</td>
                <td>{k.role === 'admin' ? 'Admin' : 'Read-only'}</td>
                <td>{#if k.last_used_at}<TimeAgo ts={k.last_used_at} />{:else}<span class="faint">never</span>{/if}</td>
                <td class="shrink"><button type="button" class="btn small ghost icon" onclick={() => revoke(k)} aria-label="Revoke {k.name}" title="Revoke"><Trash2 size={15} /></button></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {:else if keys}
    <p class="muted">No API keys yet.</p>
  {/if}

  <Group title="Create a key">
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void create();
      }}
    >
      <Field label="Name" id="k-name" hint="What uses it, so you know which key to revoke later.">
        <input id="k-name" class="input" bind:value={name} placeholder="Home Assistant" />
      </Field>
      <Field label="Access" id="k-role">
        <select id="k-role" class="input" bind:value={role}>
          <option value="admin">Full control</option>
          <option value="viewer">Read-only</option>
        </select>
      </Field>
      <div class="row-end">
        <Button type="submit" icon={KeyRound} busy={creating} disabled={!name.trim()}>Create key</Button>
      </div>
    </form>
  </Group>
</div>

<style>
  .keys {
    display: grid;
    gap: 16px;
    max-width: 900px;
  }
  header p {
    margin: 4px 0 0;
  }
  .created {
    display: grid;
    gap: 8px;
    padding: 14px 16px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-lg);
    background: var(--accent-soft);
  }
  .created p {
    margin: 0;
  }
  .keyline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .done {
    justify-self: start;
  }
  .key {
    padding: 6px 10px;
    font-size: 14px;
    word-break: break-all;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .row-end {
    display: flex;
    justify-content: flex-end;
    padding: 4px 0 8px;
  }
</style>
