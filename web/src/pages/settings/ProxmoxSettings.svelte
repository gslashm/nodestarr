<!-- Proxmox VE connection: address, API token, certificate trust, defaults, disk health and SSH host access. -->
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
  import HostAccessSetup from './HostAccessSetup.svelte';
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

  const SETUP = `pveum user add nodestarr@pve --comment "Nodestarr"
pveum role add NodestarrOps --privs "VM.Audit VM.PowerMgmt VM.Snapshot VM.Snapshot.Rollback VM.Backup Datastore.Audit Datastore.AllocateSpace Sys.Audit Sys.Modify"
pveum acl modify / --users nodestarr@pve --roles NodestarrOps
pveum user token add nodestarr@pve nodestarr --privsep 0`;
</script>

<SettingsSection section="proxmox" title="Proxmox" intro="Connect to the Proxmox VE node that runs Nodestarr's LXC or VM to see and manage its guests.">
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
        <input id="px-tid" class="input mono" bind:value={p.token_id} placeholder="nodestarr@pve!nodestarr" spellcheck="false" autocomplete="off" />
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
      <Field label="Node" id="px-node" hint="Empty uses the only node, or the one Nodestarr runs on.">
        <input id="px-node" class="input mono" bind:value={p.node} list="px-nodes" placeholder="pve" spellcheck="false" />
        <datalist id="px-nodes">{#each result?.nodes ?? [] as n (n)}<option value={n}></option>{/each}</datalist>
      </Field>
      <Field label="Nodestarr's guest ID" id="px-self" hint="The VMID of the LXC or VM Nodestarr runs in, so Nodestarr asks twice before stopping it. Empty: the guest named like this Docker host.">
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
      <Field label="Backup storage" id="px-store" hint="Where vzdump backups started from Nodestarr go.">
        <input id="px-store" class="input mono" bind:value={p.backup_storage} placeholder="backups" spellcheck="false" />
      </Field>
    </Group>

    <Group title="Disk health" hint="SMART data, temperatures and pools of every node, with history.">
      <Field label="Disk health">
        <Toggle bind:checked={p.disks.enabled} label="Read SMART data and pool status in the background" />
      </Field>
      <Field label="Read SMART data every" id="px-dh-min" hint="Minutes. Pool status (scrubs, resilvers, degraded vdevs) is read every 5 minutes regardless.">
        <input id="px-dh-min" class="input num short" type="number" min="5" step="5" bind:value={p.disks.sample_minutes} />
      </Field>
      <Field label="Sleeping disks">
        <Toggle
          bind:checked={p.disks.wake_sleeping}
          label="Wake disks that are spun down to read them"
          hint="Off: with host access, a sleeping disk is left alone and keeps its last reading. Through the Proxmox API, reading always wakes it."
        />
      </Field>
      <Field label="Keep history" id="px-dh-keep" hint="Days of SMART readings, self-test results and events.">
        <input id="px-dh-keep" class="input num short" type="number" min="7" bind:value={p.disks.retention_days} />
      </Field>
      <Field label="Self-tests at once" id="px-dh-par" hint="Per node. 0 runs every scheduled test right away; 1 tests one disk at a time, which keeps a pool fast meanwhile.">
        <input id="px-dh-par" class="input num short" type="number" min="0" bind:value={p.disks.max_parallel_tests} />
      </Field>
      <Field label="Notifications">
        <Toggle bind:checked={p.disks.notify_passed} label="Also notify about passed self-tests" hint="Failed and unfinished tests always notify (event “Disk self-test”). Disk and pool problems are alerts: Settings › Monitoring and alerts." />
      </Field>
    </Group>

    <Group title="Host access" hint="SSH to the nodes, for self-tests and what the API can't see.">
      <Field label="Host access">
        <Toggle bind:checked={p.host_access.enabled} label="Connect to the nodes over SSH" hint="Runs SMART self-tests, reads mdadm and SnapRAID arrays, controls ZFS scrubs and leaves sleeping disks asleep. Set it up below." />
      </Field>
      <Field label="SSH user" id="px-ha-user" hint="The helper needs root to talk to the disks.">
        <input id="px-ha-user" class="input mono short" bind:value={p.host_access.user} spellcheck="false" autocomplete="off" />
      </Field>
      <Field label="SSH port" id="px-ha-port">
        <input id="px-ha-port" class="input num short" type="number" min="1" max="65535" bind:value={p.host_access.port} />
      </Field>
    </Group>

    <HostAccessSetup {p} />

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
