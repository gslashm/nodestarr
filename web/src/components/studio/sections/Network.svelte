<!-- How a service is connected: its network mode, the networks it joins, hostname, DNS and extra host entries. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import Field from '../../Field.svelte';
  import StringList from '../StringList.svelte';
  import { api } from '../../../lib/api';
  import { declareTopLevel, get, getList, getNetworks, set, setList, setNetworks, topLevelNames } from '../../../lib/compose/service';
  import { useStudio } from '../../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../../lib/compose/yamldoc';
  import type { NetworkRow } from '../../../lib/types';
  import { ui } from '../../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();
  const uid = $props.id();

  function edit(what: string, fn: (d: YDoc, s: YAMLMap) => void) {
    try {
      studio.editService(name, fn, `svc:${name}:${what}`);
    } catch (err) {
      ui.error(err);
    }
  }

  let dockerNetworks = $state<string[]>([]);
  onMount(() => {
    api
      .get<{ networks: NetworkRow[] }>('/networks')
      .then((r) => (dockerNetworks = r.networks.map((n) => n.name).filter((n) => !['bridge', 'host', 'none'].includes(n))))
      .catch(() => {});
  });

  const mode = $derived(String(get(doc, svc, 'network_mode') ?? ''));
  const modeKind = $derived(mode === '' ? 'default' : mode === 'host' || mode === 'none' || mode === 'bridge' ? mode : mode.startsWith('service:') ? 'service' : 'custom');
  const nets = $derived(getNetworks(doc, svc));
  const declared = $derived(topLevelNames(doc, 'networks'));
  const others = $derived(studio.services.filter((s) => s !== name));
  let newNet = $state('');

  function setMode(kind: string) {
    const value = kind === 'default' ? '' : kind === 'service' ? `service:${others[0] ?? ''}` : kind === 'custom' ? mode || 'container:' : kind;
    edit('network_mode', (d, s) => set(d, s, 'network_mode', value));
  }

  function addNetwork() {
    const n = newNet.trim();
    if (!n || nets.some((x) => x.name === n)) return;
    // A network that already exists outside this stack is joined, not created.
    const external = dockerNetworks.includes(n) && !n.startsWith(`${studio.name}_`);
    edit('networks', (d, s) => {
      setNetworks(d, s, [...nets, { name: n, aliases: [], ipv4: '' }]);
      if (n !== 'default') declareTopLevel(d, 'networks', n, external);
    });
    newNet = '';
  }

  const hosts = $derived(getList(doc, svc, 'extra_hosts'));
</script>

<Field label="Network" hint={modeKind === 'default' ? "The stack's own network: services reach each other by name." : modeKind === 'host' ? "The server's network directly: no isolation, no published ports needed." : modeKind === 'none' ? 'No network at all.' : modeKind === 'service' ? "Uses another service's network (VPN containers, for example)." : ''}>
  <div class="inline">
    <select class="input" value={modeKind} aria-label="Network mode" onchange={(e) => setMode(e.currentTarget.value)}>
      <option value="default">The stack's network</option>
      <option value="host">The server's network (host)</option>
      <option value="bridge">Docker's default bridge</option>
      {#if others.length}<option value="service">Another service's network</option>{/if}
      <option value="none">None</option>
      <option value="custom">Other…</option>
    </select>
    {#if modeKind === 'service'}
      <select class="input" value={mode.slice(8)} aria-label="Service" onchange={(e) => edit('network_mode', (d, s) => set(d, s, 'network_mode', `service:${e.currentTarget.value}`))}>
        {#each others as o (o)}<option value={o}>{o}</option>{/each}
      </select>
    {:else if modeKind === 'custom'}
      <input class="input mono" value={mode} placeholder="container:name" spellcheck="false" aria-label="Network mode" oninput={(e) => edit('network_mode', (d, s) => set(d, s, 'network_mode', e.currentTarget.value))} />
    {/if}
  </div>
</Field>

{#if modeKind === 'default' || modeKind === 'bridge'}
  <Field label="Networks" hint="Join other networks too, like a reverse proxy's. Networks that already exist on the server are joined, not created.">
    <div class="nets">
      {#each nets as n, i (n.name)}
        <div class="net">
          <span class="mono name">{n.name}</span>
          <input class="input mono" value={n.aliases.join(', ')} placeholder="Other names (optional)" aria-label="Aliases on {n.name}" spellcheck="false" onchange={(e) => edit('networks', (d, s) => setNetworks(d, s, nets.map((x, j) => (j === i ? { ...x, aliases: e.currentTarget.value.split(',').map((a) => a.trim()).filter(Boolean) } : x))))} />
          <input class="input mono ip" value={n.ipv4} placeholder="Fixed IP" aria-label="Fixed IP on {n.name}" spellcheck="false" onchange={(e) => edit('networks', (d, s) => setNetworks(d, s, nets.map((x, j) => (j === i ? { ...x, ipv4: e.currentTarget.value.trim() } : x))))} />
          <button type="button" class="btn ghost icon small" aria-label="Leave {n.name}" onclick={() => edit('networks', (d, s) => setNetworks(d, s, nets.filter((_, j) => j !== i)))}><X size={15} /></button>
        </div>
      {/each}
      <div class="net add">
        <input
          class="input mono"
          placeholder="proxy"
          bind:value={newNet}
          list="{uid}-nets"
          aria-label="Network to join"
          spellcheck="false"
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              addNetwork();
            }
          }}
        />
        <button type="button" class="btn small" onclick={addNetwork} disabled={!newNet.trim()}><Plus size={15} />Join</button>
      </div>
      <datalist id="{uid}-nets">{#each [...new Set(['default', ...declared, ...dockerNetworks])] as n (n)}<option value={n}></option>{/each}</datalist>
    </div>
  </Field>
{/if}

{#if modeKind !== 'service' && modeKind !== 'custom'}
  <Field label="Hostname" hint="The name the container sees for itself.">
    <input class="input mono short" value={String(get(doc, svc, 'hostname') ?? '')} placeholder="Container ID" spellcheck="false" aria-label="Hostname" oninput={(e) => edit('hostname', (d, s) => set(d, s, 'hostname', e.currentTarget.value))} />
  </Field>
  <Field label="DNS servers" hint="Instead of the server's own.">
    <StringList items={getList(doc, svc, 'dns')} onchange={(items) => edit('dns', (d, s) => setList(d, s, 'dns', items))} placeholder="1.1.1.1" label="DNS server" />
  </Field>
  <Field label="Extra hosts" hint="Lines added to /etc/hosts, as name:address. host-gateway is the server itself.">
    <StringList items={hosts} onchange={(items) => edit('extra_hosts', (d, s) => setList(d, s, 'extra_hosts', items))} placeholder="host.docker.internal:host-gateway" suggestions={['host.docker.internal:host-gateway']} label="Host" />
  </Field>
{/if}

<style>
  .inline {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .inline .input {
    width: auto;
    min-width: 200px;
  }
  .nets {
    display: grid;
    gap: 6px;
  }
  .net {
    display: grid;
    grid-template-columns: minmax(90px, 0.8fr) minmax(0, 1.4fr) minmax(0, 0.8fr) 30px;
    gap: 6px;
    align-items: center;
  }
  .net.add {
    grid-template-columns: minmax(0, 1fr) auto;
  }
  .net .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .name {
    overflow-wrap: anywhere;
  }
  .short {
    max-width: 280px;
  }
</style>
