<!--
  App integrations: Berth talks to apps it recognizes (the *arrs, Plex,
  SABnzbd, qBittorrent, Jellyfin, Immich, ...) for versions, health warnings
  and activity. Shows what was found and whether each one answers.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Button from '../../components/Button.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import SecretInput from '../../components/SecretInput.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api } from '../../lib/api';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { IntegrationApp } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';
  import SettingsSection from './SettingsSection.svelte';

  let apps = $state<IntegrationApp[] | null>(null);
  let refreshing = $state(false);

  async function load() {
    try {
      const r = await api.get<{ apps: IntegrationApp[] }>('/integrations');
      apps = r.apps;
    } catch {
      apps = [];
    }
  }

  onMount(load);

  async function refresh() {
    refreshing = true;
    try {
      await api.post('/integrations/refresh');
      await new Promise((r) => setTimeout(r, 1500));
      await load();
      ui.toast('Asked every app again', 'success');
    } catch (err) {
      ui.error(err);
    } finally {
      refreshing = false;
    }
  }
</script>

<SettingsSection section="integrations" title="App integrations" intro="API keys are read from each app's own config file when Berth can see it, so most apps need no setup.">
  {#snippet children(s)}
    <Group>
      <Field label="Integrations">
        <Toggle bind:checked={s.enabled} label="Talk to the apps Berth recognizes" hint="Versions, health warnings (like Sonarr's), queue sizes and active streams." />
      </Field>
      <Field label="Find them">
        <Toggle bind:checked={s.auto_discover} label="Connect to recognized apps on their own" hint="Off: only apps you switch on in their container settings." />
      </Field>
      <Field label="Ask every" id="i-poll" hint="Seconds between checks. Apps are only asked while their container runs.">
        <input id="i-poll" class="input num short" type="number" min="15" bind:value={s.poll_secs} />
      </Field>
      <Field label="qBittorrent login" id="i-qb" hint="qBittorrent has no API key; Berth signs in with these if its web UI asks for a password.">
        <div class="inline">
          <input id="i-qb" class="input" bind:value={s.qbittorrent_username} placeholder="User name" autocomplete="off" />
          <SecretInput bind:value={s.qbittorrent_password} placeholder="Password" label="qBittorrent password" />
        </div>
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<section class="found">
  <div class="head">
    <h3>Found</h3>
    {#if session.isAdmin}<Button small variant="ghost" icon={RefreshCw} busy={refreshing} onclick={refresh}>Ask again</Button>{/if}
  </div>
  {#if !apps}
    <p class="faint">Loading…</p>
  {:else if apps.length === 0}
    <p class="muted">No recognized apps among your containers.</p>
  {:else}
    <div class="panel table-wrap">
      <table class="table">
        <thead><tr><th>Container</th><th>App</th><th>Answers</th><th>Version</th><th>API access</th></tr></thead>
        <tbody>
          {#each apps as a (a.identity)}
            {@const st = a.status}
            <tr>
              <td><a href={href(`/containers/${encodeURIComponent(a.name)}/settings`)}>{a.name}</a></td>
              <td>{a.kind ?? a.app ?? '–'}{#if a.overridden}<span class="faint small"> (set by you)</span>{/if}</td>
              <td>
                {#if !a.enabled}<span class="faint">Off</span>
                {:else if !a.running}<span class="faint">Not running</span>
                {:else if !st}<span class="faint">Not asked yet</span>
                {:else if st.reachable}<span class="text-ok">Yes</span> <span class="faint small"><TimeAgo ts={st.checked_at} /></span>
                {:else}<span class="text-bad" title={st.error ?? ''}>No</span>{#if st.error}<span class="faint small err"> {st.error}</span>{/if}{/if}
              </td>
              <td class="num">{st?.version ?? '–'}</td>
              <td class="small">
                {#if st?.credentials === 'discovered'}From its config file{:else if st?.credentials === 'configured'}Set by you{:else if st?.credentials === 'missing'}<span class="text-warn">Missing</span>{:else if st?.credentials === 'not_needed'}Not needed{:else}–{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .inline {
    display: flex;
    gap: 8px;
  }
  .short {
    max-width: 140px;
  }
  .found {
    display: grid;
    gap: 10px;
    max-width: 900px;
    margin-top: 20px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .head h3 {
    flex: 1;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .err {
    display: block;
    max-width: 320px;
  }
</style>
