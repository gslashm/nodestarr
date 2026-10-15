<!-- Proxmox VE connection: address, API token, certificate trust, and defaults. -->
<script lang="ts">
  import PlugZap from '@lucide/svelte/icons/plug-zap';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import SecretInput from '../../components/SecretInput.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage } from '../../lib/api';
  import type { Settings } from '../../lib/types';
  import SettingsSection from './SettingsSection.svelte';

  let testing = $state(false);
  let result = $state<{ ok: boolean; text: string; nodes?: string[] } | null>(null);

  async function test(p: Settings['proxmox']) {
    testing = true;
    result = null;
    try {
      const r = await api.post<{ version: string; release: string; nodes: string[]; fingerprint: string | null }>('/settings/test/proxmox', p);
      if (r.fingerprint) p.fingerprint = r.fingerprint;
      if (!p.node && r.nodes.length === 1) p.node = r.nodes[0];
      result = {
        ok: true,
        text: `Connected to Proxmox VE ${r.version} (nodes: ${r.nodes.join(', ')}).${r.fingerprint ? ' The certificate fingerprint was filled in; compare it with the one shown in Proxmox before saving.' : ''}`,
        nodes: r.nodes,
      };
    } catch (err) {
      result = { ok: false, text: errorMessage(err) };
    } finally {
      testing = false;
    }
  }

  const SETUP = `pveum user add berth@pve --comment "Berth"
pveum role add BerthOps --privs "VM.Audit VM.PowerMgmt VM.Snapshot VM.Snapshot.Rollback VM.Backup Datastore.Audit Datastore.AllocateSpace Sys.Audit Sys.Modify"
pveum acl modify / --users berth@pve --roles BerthOps
pveum user token add berth@pve berth --privsep 0`;
</script>

<SettingsSection section="proxmox" title="Proxmox" intro="Connect to the Proxmox VE node that runs Berth's LXC or VM to see and manage its guests.">
  {#snippet actions(p)}
    <Button icon={PlugZap} busy={testing} onclick={() => test(p)} disabled={!p.url || !p.token_id}>Test connection</Button>
  {/snippet}
  {#snippet children(p)}
    <Group>
      <Field label="Proxmox">
        <Toggle bind:checked={p.enabled} label="Connect to Proxmox VE" />
      </Field>
      <Field label="Address" id="px-url" hint="The node's API address, usually port 8006.">
        <input id="px-url" class="input mono" bind:value={p.url} placeholder="https://192.168.1.10:8006" spellcheck="false" />
      </Field>
      <Field label="API token ID" id="px-tid" hint="user@realm!tokenname">
        <input id="px-tid" class="input mono" bind:value={p.token_id} placeholder="berth@pve!berth" spellcheck="false" autocomplete="off" />
      </Field>
      <Field label="Token secret" id="px-secret">
        <SecretInput id="px-secret" bind:value={p.token_secret} placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" label="Token secret" />
      </Field>
      <Field label="Certificate">
        <Segmented
          label="Certificate check"
          bind:value={p.tls_mode}
          options={[
            { value: 'fingerprint', label: 'Pin its fingerprint' },
            { value: 'verify', label: 'Trusted certificate' },
            { value: 'insecure', label: "Don't check" },
          ]}
        />
      </Field>
      {#if p.tls_mode === 'fingerprint'}
        <Field label="Fingerprint" id="px-fp" hint="SHA-256 of the node's certificate. Leave empty and test the connection to fill it in.">
          <input id="px-fp" class="input mono" bind:value={p.fingerprint} placeholder="AB:CD:…" spellcheck="false" />
        </Field>
      {:else if p.tls_mode === 'insecure'}
        <p class="warn">Without a certificate check, someone on your network could impersonate Proxmox and capture the token. Pinning the fingerprint is just as easy and safe.</p>
      {/if}
      {#if result}<p class={result.ok ? 'text-ok' : 'text-bad'}>{result.text}</p>{/if}
    </Group>

    <Group title="Defaults">
      <Field label="Node" id="px-node" hint="Empty uses the only node, or the one Berth runs on.">
        <input id="px-node" class="input mono" bind:value={p.node} list="px-nodes" placeholder="pve" spellcheck="false" />
        <datalist id="px-nodes">{#each result?.nodes ?? [] as n (n)}<option value={n}></option>{/each}</datalist>
      </Field>
      <Field label="Berth's guest ID" id="px-self" hint="The VMID of the LXC or VM Berth runs in, so Berth asks twice before stopping it. Empty: the guest named like this Docker host.">
        <input
          id="px-self"
          class="input num short"
          type="number"
          min="100"
          value={p.self_vmid ?? ''}
          oninput={(e) => (p.self_vmid = e.currentTarget.value === '' ? null : Number(e.currentTarget.value))}
          placeholder="100"
        />
      </Field>
      <Field label="Backup storage" id="px-store" hint="Where vzdump backups started from Berth go.">
        <input id="px-store" class="input mono" bind:value={p.backup_storage} placeholder="backups" spellcheck="false" />
      </Field>
    </Group>

    <details class="panel help">
      <summary class="panel-head"><h3>Creating the API token</h3></summary>
      <div class="panel-body">
        <p class="muted">Run these on the Proxmox host (as root). The last command prints the token secret once.</p>
        <pre class="cmds">{SETUP}</pre>
        <CopyButton text={SETUP} label="Copy commands" showLabel />
        <p class="muted small">For read-only monitoring, give the token the built-in PVEAuditor role instead.</p>
      </div>
    </details>
  {/snippet}
</SettingsSection>

<style>
  .short {
    max-width: 140px;
  }
  .warn {
    margin: 4px 0 8px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--warn) 10%, var(--panel));
    font-size: var(--fs-sm);
  }
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
  .cmds {
    margin: 8px 0;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 12.5px;
    white-space: pre-wrap;
  }
  .small {
    font-size: var(--fs-sm);
  }
</style>
