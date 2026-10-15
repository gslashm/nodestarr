<!-- People who can sign in to Berth, and what they may do. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import UserPlus from '@lucide/svelte/icons/user-plus';
  import Button from '../../components/Button.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { session } from '../../lib/session.svelte';
  import type { Role, UserInfo } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let users = $state<UserInfo[] | null>(null);
  let error = $state('');

  let username = $state('');
  let password = $state('');
  let role = $state<Role>('viewer');
  let creating = $state(false);

  let resetFor = $state<UserInfo | null>(null);
  let newPassword = $state('');

  async function load() {
    try {
      users = await api.get<UserInfo[]>('/users');
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(load);

  async function create() {
    creating = true;
    try {
      await api.post('/users', { username: username.trim(), password, role });
      ui.toast(`${username} can sign in now`, 'success');
      username = password = '';
      role = 'viewer';
      await load();
    } catch (err) {
      ui.error(err, "Couldn't add the user");
    } finally {
      creating = false;
    }
  }

  async function setRole(u: UserInfo, r: Role) {
    try {
      await api.put(`/users/${u.id}/role`, { role: r });
      await load();
    } catch (err) {
      ui.error(err);
      await load();
    }
  }

  async function remove(u: UserInfo) {
    const ok = await ui.confirm({ title: `Remove ${u.username}?`, message: 'They are signed out everywhere and can no longer sign in.', confirmLabel: 'Remove', danger: true });
    if (!ok) return;
    try {
      await api.del(`/users/${u.id}`);
      await load();
    } catch (err) {
      ui.error(err);
    }
  }

  async function resetPassword() {
    if (!resetFor) return;
    try {
      await api.put(`/users/${resetFor.id}/password`, { password: newPassword });
      ui.toast(`Password of ${resetFor.username} changed; they were signed out`, 'success');
      resetFor = null;
      newPassword = '';
    } catch (err) {
      ui.error(err);
    }
  }
</script>

<div class="users">
  <header>
    <h2>Users</h2>
    <p class="muted">Admins can change anything. Viewers see everything except secrets and can't change anything.</p>
  </header>

  {#if error}
    <p class="text-bad">{error}</p>
  {:else if users}
    <section class="panel">
      <div class="table-wrap">
        <table class="table">
          <thead><tr><th>User</th><th>Role</th><th>Two-step</th><th>Last sign-in</th><th class="shrink"></th></tr></thead>
          <tbody>
            {#each users as u (u.id)}
              {@const self = u.username === session.me?.username}
              <tr>
                <td>{u.username} {#if self}<span class="chip">you</span>{/if}</td>
                <td>
                  <select class="input role" value={u.role} onchange={(e) => setRole(u, e.currentTarget.value as Role)} disabled={self} aria-label="Role of {u.username}">
                    <option value="admin">Admin</option>
                    <option value="viewer">Viewer</option>
                  </select>
                </td>
                <td>{#if u.totp_enabled}<span class="text-ok">On</span>{:else}<span class="faint">Off</span>{/if}</td>
                <td>{#if u.last_login_at}<TimeAgo ts={u.last_login_at} />{:else}<span class="faint">never</span>{/if}</td>
                <td class="shrink">
                  <div class="row-actions">
                    {#if !self}
                      <button type="button" class="btn small ghost" onclick={() => ((resetFor = u), (newPassword = ''))}>Set password</button>
                      <button type="button" class="btn small ghost icon" onclick={() => remove(u)} aria-label="Remove {u.username}" title="Remove"><Trash2 size={15} /></button>
                    {/if}
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}

  <Group title="Add a user">
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void create();
      }}
    >
      <Field label="Username" id="u-name">
        <input id="u-name" class="input" bind:value={username} autocomplete="off" spellcheck="false" />
      </Field>
      <Field label="Password" id="u-pass" hint="At least 8 characters. They can change it in their account settings.">
        <input id="u-pass" class="input" type="password" bind:value={password} autocomplete="new-password" />
      </Field>
      <Field label="Role" id="u-role">
        <select id="u-role" class="input" bind:value={role}>
          <option value="viewer">Viewer (read-only)</option>
          <option value="admin">Admin</option>
        </select>
      </Field>
      <div class="row-end">
        <Button type="submit" icon={UserPlus} busy={creating} disabled={!username.trim() || password.length < 8}>Add user</Button>
      </div>
    </form>
  </Group>
</div>

<Dialog open={!!resetFor} title="New password for {resetFor?.username ?? ''}" width="440px" onclose={() => (resetFor = null)}>
  <form
    id="reset-form"
    onsubmit={(e) => {
      e.preventDefault();
      void resetPassword();
    }}
  >
    <input class="input" type="password" bind:value={newPassword} autocomplete="new-password" aria-label="New password" placeholder="At least 8 characters" />
  </form>
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (resetFor = null)}>Cancel</button>
    <button class="btn primary" form="reset-form" type="submit" disabled={newPassword.length < 8}>Set password</button>
  {/snippet}
</Dialog>

<style>
  .users {
    display: grid;
    gap: 16px;
    max-width: 900px;
  }
  header p {
    margin: 4px 0 0;
  }
  .role {
    width: auto;
    min-height: 30px;
    padding-top: 3px;
    padding-bottom: 3px;
  }
  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }
  .row-end {
    display: flex;
    justify-content: flex-end;
    padding: 4px 0 8px;
  }
</style>
