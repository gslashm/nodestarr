<!--
  Host access setup: Nodestarr's SSH key, the commands that install the
  helper and authorize the key on a node, and a connection test per node,
  which also pins the node's SSH host key.
-->
<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import PlugZap from '@lucide/svelte/icons/plug-zap';
  import { onMount } from 'svelte';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import { api, apiUrl, errorMessage } from '../../lib/api';
  import { ago } from '../../lib/format';
  import { session } from '../../lib/session.svelte';
  import type { HostAccessInfo, NodeAccess, Settings } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { p }: { p: Settings['proxmox'] } = $props();

  let info = $state<HostAccessInfo | null>(null);
  let error = $state('');
  let creating = $state(false);
  let testing = $state<string | null>(null);
  let results = $state<Record<string, { ok: boolean; text: string }>>({});

  async function load() {
    try {
      info = await api.get<HostAccessInfo>('/proxmox/host-access');
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(() => void load());

  async function createKey() {
    if (info?.public_key) {
      const ok = await ui.confirm({
        title: 'Create a new key?',
        message: 'The current key stops working. Run the setup commands on every node again to authorize the new one (and remove the old line from authorized_keys).',
        confirmLabel: 'Create new key',
        danger: true,
      });
      if (!ok) return;
    }
    creating = true;
    try {
      await api.post('/proxmox/host-access/key');
      await load();
    } catch (err) {
      ui.error(err, "Couldn't create the key");
    } finally {
      creating = false;
    }
  }

  function describe(n: NodeAccess): string {
    const parts = [`helper v${n.helper}`];
    if (n.smartctl) parts.push(`smartctl ${n.smartctl}`);
    if (n.zfs) parts.push(`ZFS ${n.zfs.split('-')[0]}`);
    if (n.md_arrays) parts.push('mdadm arrays');
    if (n.snapraid_conf) parts.push(`SnapRAID${n.snapraid ? ` ${n.snapraid}` : ''}`);
    return parts.join(' · ');
  }

  async function test(node: string) {
    testing = node;
    try {
      const r = await api.post<{ node: NodeAccess }>('/proxmox/host-access/test', { node });
      const n = r.node;
      results[node] = n.error ? { ok: false, text: n.error } : { ok: true, text: `Connected to ${n.hostname ?? node}: ${describe(n)}` };
      await load();
    } catch (err) {
      results[node] = { ok: false, text: errorMessage(err) };
    } finally {
      testing = null;
    }
  }

  async function forgetHostKey(node: string) {
    const ok = await ui.confirm({
      title: `Forget ${node}'s host key?`,
      message: 'Do this after reinstalling the node. The next test accepts and pins whatever key the node presents, so make sure you are on a network you trust.',
      confirmLabel: 'Forget host key',
      danger: true,
    });
    if (!ok) return;
    try {
      await api.del(`/proxmox/host-access/host-keys/${encodeURIComponent(node)}`);
      delete results[node];
      await load();
    } catch (err) {
      ui.error(err);
    }
  }

  const saved = $derived(session.settings?.proxmox.host_access.addresses ?? {});
</script>

<details class="panel help" open={!!info && !info.nodes.some((n) => n.status?.ok)}>
  <summary class="panel-head"><h3>Setting up host access</h3></summary>
  <div class="panel-body body">
    {#if error}<p class="text-bad">{error}</p>{/if}
    <p class="muted">
      The Proxmox API can read SMART data, but it can't run self-tests, and every read through it wakes disks that are spun down. With host access,
      Nodestarr connects to each node over SSH and runs a small helper script there. The key Nodestarr uses can do nothing else: it is limited to the
      helper, which only reads disk and pool information, starts or aborts SMART self-tests, controls ZFS scrubs and blinks a disk's activity light.
    </p>
    <ol class="steps">
      <li>
        <strong>Nodestarr's SSH key</strong>
        {#if info?.public_key}
          <pre class="key">{info.public_key}</pre>
          <div class="row">
            <CopyButton text={info.public_key} label="Copy key" showLabel />
            {#if session.isAdmin}<Button small variant="ghost" icon={KeyRound} busy={creating} onclick={createKey}>Create a new key</Button>{/if}
          </div>
        {:else if info}
          <p class="muted small">Nodestarr doesn't have a key yet. It's kept in its data folder (<span class="mono">ssh/id_ed25519</span>).</p>
          {#if session.isAdmin}<div><Button variant="primary" icon={KeyRound} busy={creating} onclick={createKey}>Create key</Button></div>{/if}
        {/if}
      </li>
      <li>
        <strong>Install the helper on each node</strong>
        {#if info?.install}
          <p class="muted small">Run these as root in the node's shell (in the Proxmox web interface: the node › Shell). They install the helper as <span class="mono">{info.helper_path}</span> and authorize the key for it alone.</p>
          <details class="cmds-wrap">
            <summary class="small">Show the commands</summary>
            <pre class="cmds">{info.install}</pre>
          </details>
          <div class="row">
            <CopyButton text={info.install} label="Copy commands" showLabel />
            <a class="btn small ghost" href={apiUrl('/proxmox/host-access/helper.sh')} download="nodestarr-host"><Download size={15} /> Download the helper</a>
          </div>
          <p class="faint small">
            In a cluster the key line lands in the shared <span class="mono">authorized_keys</span>, but the helper lives on each node: run the commands on every
            node. smartctl comes with Proxmox; mdadm and SnapRAID arrays show up when they're installed.
          </p>
        {:else}
          <p class="faint small">Create the key first.</p>
        {/if}
      </li>
      <li>
        <strong>Test each node</strong>
        <p class="muted small">The first successful test pins the node's SSH host key; later connections refuse a different one. Compare the fingerprint with <span class="mono">ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</span> on the node.</p>
        {#if info?.nodes.length}
          <div class="table-wrap">
            <table class="table">
              <thead><tr><th>Node</th><th>SSH address</th><th>Status</th><th class="shrink"></th></tr></thead>
              <tbody>
                {#each info.nodes as n (n.node)}
                  {@const res = results[n.node]}
                  {@const st = n.status}
                  <tr>
                    <td class="mono">{n.node}</td>
                    <td>
                      <input
                        class="input mono addr"
                        bind:value={p.host_access.addresses[n.node]}
                        placeholder={n.configured ? '' : (n.address ?? 'host or IP')}
                        spellcheck="false"
                        aria-label="SSH address of {n.node}"
                        disabled={!session.isAdmin}
                      />
                      {#if (p.host_access.addresses[n.node] ?? '') !== (saved[n.node] ?? '')}<div class="faint small">Save to use this address.</div>{/if}
                    </td>
                    <td class="small">
                      {#if res}
                        <span class={res.ok ? 'text-ok' : 'text-bad'}>{res.text}</span>
                      {:else if st?.ok && st.helper !== null && !st.error}
                        <span class="text-ok">Working</span> <span class="faint">· {describe(st)} · checked {ago(st.checked_at)}</span>
                      {:else if st?.error}
                        <span class="text-bad">{st.error}</span>
                      {:else}
                        <span class="faint">Not tested yet</span>
                      {/if}
                      {#each n.host_keys as k (k)}<div class="faint mono fp" title="Pinned host key">{k}</div>{/each}
                    </td>
                    <td class="shrink">
                      {#if session.isAdmin}
                        <div class="row">
                          <Button small icon={PlugZap} busy={testing === n.node} disabled={!info.has_key || !p.host_access.enabled} onclick={() => test(n.node)}>Test</Button>
                          {#if n.host_keys.length}<Button small variant="ghost" onclick={() => forgetHostKey(n.node)}>Forget key</Button>{/if}
                        </div>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if !p.host_access.enabled}<p class="faint small">Turn host access on (and save) to test.</p>{/if}
        {:else}
          <p class="faint small">Connect to Proxmox first; its nodes show up here.</p>
        {/if}
      </li>
    </ol>
  </div>
</details>

<style>
  .help > summary {
    cursor: pointer;
    list-style: none;
  }
  .help > summary::-webkit-details-marker {
    display: none;
  }
  .help:not([open]) > summary {
    border-bottom: 0;
  }
  .body {
    display: grid;
    gap: 10px;
  }
  .body > p {
    margin: 0;
  }
  .steps {
    display: grid;
    gap: 18px;
    margin: 4px 0 0;
    padding-left: 20px;
  }
  .steps li {
    display: grid;
    gap: 8px;
  }
  .steps p {
    margin: 0;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .key,
  .cmds {
    margin: 0;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 12.5px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cmds {
    max-height: 340px;
    margin-top: 6px;
    overflow: auto;
  }
  .cmds-wrap summary {
    cursor: pointer;
    color: var(--text-2);
  }
  .addr {
    min-width: 160px;
  }
  .fp {
    font-size: 11.5px;
    overflow-wrap: anywhere;
  }
</style>
