<!--
  Form for a container's settings (image, ports, volumes, environment,
  resources and the advanced options). Used to add a container and to edit
  a standalone one. Edits `spec` in place.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import { api } from '../lib/api';
  import { formatSize, parseSize, shellJoin, shellSplit } from '../lib/spec';
  import type { ContainerSpec, NetworkRow, VolumeRow } from '../lib/types';
  import Field from './Field.svelte';
  import KeyValueEditor from './KeyValueEditor.svelte';
  import ListEditor from './ListEditor.svelte';
  import Toggle from './Toggle.svelte';

  let { spec = $bindable(), editing = false }: { spec: ContainerSpec; editing?: boolean } = $props();

  let networks = $state<string[]>([]);
  let volumes = $state<string[]>([]);
  let sizeErrors = $state<Record<string, string>>({});

  const RESTART = [
    { value: 'unless-stopped', label: 'Unless stopped by hand' },
    { value: 'always', label: 'Always' },
    { value: 'on-failure', label: 'Only when it fails' },
    { value: 'no', label: 'Never' },
  ];

  onMount(() => {
    // Suggestions only; the form works without them.
    api
      .get<{ networks: NetworkRow[] }>('/networks')
      .then((r) => (networks = r.networks.map((x) => x.name)))
      .catch(() => {});
    api
      .get<{ volumes: VolumeRow[] }>('/volumes')
      .then((r) => (volumes = r.volumes.filter((x) => !x.anonymous).map((x) => x.name)))
      .catch(() => {});
  });

  function setSize(field: 'memory' | 'shm_size', text: string) {
    const v = parseSize(text);
    if (v === 'invalid') {
      sizeErrors[field] = 'Use a number with a unit, like 512m or 2g.';
      return;
    }
    delete sizeErrors[field];
    spec[field] = v;
  }

  function setHealth(on: boolean) {
    spec.healthcheck = on ? { command: '', interval_secs: 30, timeout_secs: 10, retries: 3, start_period_secs: 0 } : null;
  }

  const customRestart = $derived(!RESTART.some((r) => r.value === spec.restart));
</script>

<div class="spec">
  <section class="panel">
    <div class="panel-head"><h2>Basics</h2></div>
    <div class="panel-body">
      <Field label="Image" id="s-image" hint="Like lscr.io/linuxserver/sonarr:latest. It's pulled if it isn't on this machine.">
        <input id="s-image" class="input mono" bind:value={spec.image} placeholder="repository/image:tag" required spellcheck="false" />
      </Field>
      <Field label="Name" id="s-name" hint={editing ? 'Renaming recreates the container under the new name.' : 'Letters, digits, dots, dashes and underscores. Empty lets Docker pick one.'}>
        <input id="s-name" class="input mono" bind:value={spec.name} placeholder="sonarr" spellcheck="false" pattern={'[A-Za-z0-9_.\\-]*'} />
      </Field>
      <Field label="Restart" id="s-restart" hint="What Docker does when the container exits or the machine reboots.">
        <select id="s-restart" class="input" bind:value={spec.restart}>
          {#each RESTART as r (r.value)}<option value={r.value}>{r.label}</option>{/each}
          {#if customRestart}<option value={spec.restart}>{spec.restart}</option>{/if}
        </select>
      </Field>
    </div>
  </section>

  <section class="panel">
    <div class="panel-head"><h2>Network</h2></div>
    <div class="panel-body">
      <Field label="Network" id="s-netmode" hint="bridge (the default), host, none, a network name, or container:name to share another container's network.">
        <input id="s-netmode" class="input mono" bind:value={spec.network_mode} list="s-networks" placeholder="bridge" spellcheck="false" />
        <datalist id="s-networks">
          {#each ['bridge', 'host', 'none', ...networks.filter((n) => !['bridge', 'host', 'none'].includes(n))] as n (n)}<option value={n}></option>{/each}
        </datalist>
      </Field>
      {#if spec.network_mode !== 'host' && !spec.network_mode.startsWith('container:')}
        <Field label="Ports" hint="Host port empty publishes on a random free port. Host address empty listens on all addresses.">
          <div class="rows">
            {#if spec.ports.length}
              <div class="row ports head">
                <span>Host address</span><span>Host port</span><span>Container port</span><span>Protocol</span><span></span>
              </div>
            {/if}
            {#each spec.ports as p, i (i)}
              <div class="row ports">
                <input class="input mono" bind:value={p.host_ip} placeholder="0.0.0.0" aria-label="Host address {i + 1}" />
                <input class="input mono" bind:value={p.host_port} placeholder="8080" aria-label="Host port {i + 1}" inputmode="numeric" />
                <input class="input mono" bind:value={p.container_port} placeholder="80" aria-label="Container port {i + 1}" inputmode="numeric" required />
                <select class="input" bind:value={p.protocol} aria-label="Protocol {i + 1}">
                  <option value="tcp">TCP</option>
                  <option value="udp">UDP</option>
                </select>
                <button type="button" class="btn ghost icon small" aria-label="Remove port" onclick={() => (spec.ports = spec.ports.filter((_, j) => j !== i))}><X size={15} /></button>
              </div>
            {/each}
            <div>
              <button type="button" class="btn small" onclick={() => (spec.ports = [...spec.ports, { host_ip: '', host_port: '', container_port: '', protocol: 'tcp' }])}><Plus size={15} />Add port</button>
            </div>
          </div>
        </Field>
        <Field label="Also connect to" hint="More networks, besides the one above.">
          <ListEditor bind:items={spec.networks} placeholder="proxy" mono addLabel="Add network" />
        </Field>
      {/if}
      <Field label="Hostname" id="s-hostname">
        <input id="s-hostname" class="input mono" bind:value={spec.hostname} placeholder="Container ID" spellcheck="false" />
      </Field>
    </div>
  </section>

  <section class="panel">
    <div class="panel-head"><h2>Storage</h2></div>
    <div class="panel-body">
      <Field label="Volumes and folders" hint="Folder: a host path (absolute). Volume: a named Docker volume, created if missing; empty name makes an anonymous one.">
        <div class="rows">
          {#if spec.volumes.length}
            <div class="row vols head"><span>Type</span><span>On the host</span><span>In the container</span><span>Read-only</span><span></span></div>
          {/if}
          {#each spec.volumes as v, i (i)}
            <div class="row vols">
              <select class="input" bind:value={v.kind} aria-label="Type {i + 1}">
                <option value="bind">Folder</option>
                <option value="volume">Volume</option>
                <option value="tmpfs">Memory (tmpfs)</option>
              </select>
              {#if v.kind === 'tmpfs'}
                <span class="faint small">–</span>
              {:else}
                <input
                  class="input mono"
                  bind:value={v.source}
                  placeholder={v.kind === 'bind' ? '/opt/appdata/app' : 'app-data'}
                  list={v.kind === 'volume' ? 's-volumes' : undefined}
                  aria-label="Source {i + 1}"
                  spellcheck="false"
                />
              {/if}
              <input class="input mono" bind:value={v.target} placeholder="/config" aria-label="Target {i + 1}" required spellcheck="false" />
              <label class="check"><input type="checkbox" bind:checked={v.read_only} aria-label="Read-only {i + 1}" /></label>
              <button type="button" class="btn ghost icon small" aria-label="Remove volume" onclick={() => (spec.volumes = spec.volumes.filter((_, j) => j !== i))}><X size={15} /></button>
            </div>
          {/each}
          <datalist id="s-volumes">{#each volumes as n (n)}<option value={n}></option>{/each}</datalist>
          <div>
            <button type="button" class="btn small" onclick={() => (spec.volumes = [...spec.volumes, { kind: 'bind', source: '', target: '', read_only: false }])}><Plus size={15} />Add volume or folder</button>
          </div>
        </div>
      </Field>
    </div>
  </section>

  <section class="panel">
    <div class="panel-head"><h2>Environment</h2></div>
    <div class="panel-body">
      <Field label="Variables" hint="Common ones: PUID and PGID (user and group IDs), TZ (time zone, like America/New_York).">
        <KeyValueEditor bind:items={spec.env} keyPlaceholder="TZ" addLabel="Add variable" />
      </Field>
      <Field label="Labels">
        <KeyValueEditor bind:items={spec.labels} keyLabel="Label" keyPlaceholder="com.example.label" addLabel="Add label" />
      </Field>
    </div>
  </section>

  <section class="panel">
    <div class="panel-head"><h2>Resources</h2></div>
    <div class="panel-body">
      <Field label="Memory limit" id="s-mem" hint="Like 512m or 2g. Empty means no limit." error={sizeErrors.memory}>
        <input id="s-mem" class="input mono short" value={formatSize(spec.memory)} onchange={(e) => setSize('memory', e.currentTarget.value)} placeholder="No limit" />
      </Field>
      <Field label="CPU limit" id="s-cpu" hint="Number of cores, like 1.5. Empty means no limit.">
        <input
          id="s-cpu"
          class="input num short"
          type="number"
          min="0"
          step="0.1"
          value={spec.cpus ?? ''}
          oninput={(e) => (spec.cpus = e.currentTarget.value === '' ? null : Number(e.currentTarget.value))}
          placeholder="No limit"
        />
      </Field>
      <Field label="Devices" hint="Hardware to pass through, like /dev/dri for video transcoding.">
        <div class="rows">
          {#each spec.devices as d, i (i)}
            <div class="row devs">
              <input class="input mono" bind:value={d.host} placeholder="/dev/dri" aria-label="Host device {i + 1}" />
              <input class="input mono" bind:value={d.container} placeholder="Same path" aria-label="Container path {i + 1}" />
              <input class="input mono" bind:value={d.permissions} placeholder="rwm" aria-label="Permissions {i + 1}" />
              <button type="button" class="btn ghost icon small" aria-label="Remove device" onclick={() => (spec.devices = spec.devices.filter((_, j) => j !== i))}><X size={15} /></button>
            </div>
          {/each}
          <div>
            <button type="button" class="btn small" onclick={() => (spec.devices = [...spec.devices, { host: '', container: '', permissions: '' }])}><Plus size={15} />Add device</button>
          </div>
        </div>
      </Field>
      <Field label="NVIDIA GPUs" id="s-gpus" hint="Needs the NVIDIA container toolkit on the host.">
        <select id="s-gpus" class="input short" bind:value={spec.gpus}>
          <option value="">None</option>
          <option value="all">All GPUs</option>
          {#if spec.gpus && spec.gpus !== 'all'}<option value={spec.gpus}>{spec.gpus}</option>{/if}
        </select>
      </Field>
    </div>
  </section>

  <details class="panel advanced">
    <summary class="panel-head"><h2>Advanced</h2><span class="faint small">Command, user, health check, capabilities, logging</span></summary>
    <div class="panel-body">
      <Field label="Command" id="s-cmd" hint="Arguments for the image's program. Empty uses the image default. Quote arguments with spaces.">
        <input id="s-cmd" class="input mono" value={shellJoin(spec.command)} onchange={(e) => (spec.command = shellSplit(e.currentTarget.value))} placeholder="Image default" spellcheck="false" />
      </Field>
      <Field label="Entrypoint" id="s-entry" hint="Replaces the program the image starts. Rarely needed.">
        <input id="s-entry" class="input mono" value={shellJoin(spec.entrypoint)} onchange={(e) => (spec.entrypoint = shellSplit(e.currentTarget.value))} placeholder="Image default" spellcheck="false" />
      </Field>
      <Field label="User" id="s-user" hint="user or user:group, by name or number. Empty uses the image default.">
        <input id="s-user" class="input mono short" bind:value={spec.user} placeholder="1000:1000" spellcheck="false" />
      </Field>
      <Field label="Working folder" id="s-wd">
        <input id="s-wd" class="input mono" bind:value={spec.working_dir} placeholder="Image default" spellcheck="false" />
      </Field>
      <Field label="Health check">
        <div class="health">
          <Toggle checked={!!spec.healthcheck} label="Use a custom health check" hint="Otherwise the image's own check (if any) is used." onchange={setHealth} />
          {#if spec.healthcheck}
            <input class="input mono" bind:value={spec.healthcheck.command} placeholder="curl -fs http://localhost:8080/ || exit 1" aria-label="Health check command" spellcheck="false" />
            <div class="health-nums">
              <label>Every <input class="input num" type="number" min="1" bind:value={spec.healthcheck.interval_secs} /> s</label>
              <label>Timeout <input class="input num" type="number" min="1" bind:value={spec.healthcheck.timeout_secs} /> s</label>
              <label>Retries <input class="input num" type="number" min="1" bind:value={spec.healthcheck.retries} /></label>
              <label>Grace period <input class="input num" type="number" min="0" bind:value={spec.healthcheck.start_period_secs} /> s</label>
            </div>
          {/if}
        </div>
      </Field>
      <Field label="Options">
        <div class="toggles">
          <Toggle bind:checked={spec.init} label="Run an init process" hint="Reaps zombie processes and forwards signals (docker run --init)." />
          <Toggle bind:checked={spec.tty} label="Allocate a terminal (TTY)" />
          <Toggle bind:checked={spec.stdin_open} label="Keep standard input open" />
          <Toggle bind:checked={spec.privileged} label="Privileged" hint="Full access to the host's devices. Avoid unless the app needs it." />
        </div>
      </Field>
      <Field label="Add capabilities" hint="Like NET_ADMIN for VPN containers.">
        <ListEditor bind:items={spec.cap_add} placeholder="NET_ADMIN" mono addLabel="Add" />
      </Field>
      <Field label="Drop capabilities">
        <ListEditor bind:items={spec.cap_drop} placeholder="ALL" mono addLabel="Add" />
      </Field>
      <Field label="Extra groups" hint="Group names or IDs, like render or video for GPU access.">
        <ListEditor bind:items={spec.group_add} placeholder="video" mono addLabel="Add" />
      </Field>
      <Field label="Shared memory" id="s-shm" hint="Size of /dev/shm, like 256m. Empty uses Docker's 64m." error={sizeErrors.shm_size}>
        <input id="s-shm" class="input mono short" value={formatSize(spec.shm_size)} onchange={(e) => setSize('shm_size', e.currentTarget.value)} placeholder="64m" />
      </Field>
      <Field label="Memory folders (tmpfs)" hint="Paths mounted in memory, like /tmp.">
        <ListEditor bind:items={spec.tmpfs} placeholder="/tmp" mono addLabel="Add" />
      </Field>
      <Field label="DNS servers">
        <ListEditor bind:items={spec.dns} placeholder="1.1.1.1" mono addLabel="Add" />
      </Field>
      <Field label="Extra hosts" hint="host:ip entries added to /etc/hosts. host-gateway means the Docker host.">
        <ListEditor bind:items={spec.extra_hosts} placeholder="host.docker.internal:host-gateway" mono addLabel="Add" />
      </Field>
      <Field label="Security options">
        <ListEditor bind:items={spec.security_opt} placeholder="no-new-privileges:true" mono addLabel="Add" />
      </Field>
      <Field label="Kernel settings (sysctls)">
        <KeyValueEditor bind:items={spec.sysctls} keyLabel="Setting" keyPlaceholder="net.ipv4.ip_forward" addLabel="Add setting" textMode={false} />
      </Field>
      <Field label="Log driver" id="s-logdrv" hint="Empty uses Docker's default (usually json-file).">
        <input id="s-logdrv" class="input mono short" bind:value={spec.log_driver} placeholder="json-file" spellcheck="false" />
      </Field>
      <Field label="Log options" hint="For json-file: max-size (like 10m) and max-file keep logs from filling the disk.">
        <KeyValueEditor bind:items={spec.log_opts} keyLabel="Option" keyPlaceholder="max-size" addLabel="Add option" textMode={false} />
      </Field>
    </div>
  </details>
</div>

<style>
  .spec {
    display: grid;
    gap: 16px;
  }
  .spec .panel-body {
    padding-top: 4px;
    padding-bottom: 8px;
  }
  .rows {
    display: grid;
    gap: 6px;
  }
  .row {
    display: grid;
    gap: 6px;
    align-items: center;
  }
  .row.head {
    color: var(--text-3);
    font-size: var(--fs-xs);
  }
  .ports {
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) minmax(0, 1fr) 84px 28px;
  }
  .vols {
    grid-template-columns: 130px minmax(0, 2fr) minmax(0, 1.5fr) 64px 28px;
  }
  .devs {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 80px 28px;
  }
  .check {
    display: flex;
    justify-content: center;
  }
  .short {
    max-width: 200px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .toggles {
    display: grid;
    gap: 14px;
    padding-top: 6px;
  }
  .health {
    display: grid;
    gap: 10px;
    padding-top: 6px;
  }
  .health-nums {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
  }
  .health-nums label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .health-nums .input {
    width: 76px;
    min-height: 32px;
    padding: 4px 8px;
  }
  .advanced > summary {
    cursor: pointer;
    list-style: none;
  }
  .advanced > summary::-webkit-details-marker {
    display: none;
  }
  .advanced > summary h2 {
    flex: none;
  }
  .advanced > summary::after {
    content: '▸';
    margin-left: auto;
    color: var(--text-3);
    transition: transform 120ms var(--ease);
  }
  .advanced[open] > summary::after {
    transform: rotate(90deg);
  }
  .advanced:not([open]) > summary {
    border-bottom: 0;
  }
  @media (max-width: 760px) {
    .ports,
    .vols,
    .devs {
      grid-template-columns: 1fr 1fr;
    }
    .row.head {
      display: none;
    }
  }
</style>
