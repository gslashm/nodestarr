<!-- Published ports, with warnings when another service or container already uses a host port. -->
<script lang="ts">
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Plus from '@lucide/svelte/icons/plus';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import Field from '../../Field.svelte';
  import StringList from '../StringList.svelte';
  import { get, getList, getPorts, removeItem, serviceMap, setList, setPort, type PortSpec } from '../../../lib/compose/service';
  import { useStudio } from '../../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../../lib/compose/yamldoc';
  import { resolveLink } from '../../../lib/format';
  import { live } from '../../../lib/live.svelte';
  import { ui } from '../../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();

  function edit(what: string, fn: (d: YDoc, s: YAMLMap) => void) {
    try {
      studio.editService(name, fn, `svc:${name}:${what}`);
    } catch (err) {
      ui.error(err);
    }
  }

  const ports = $derived(getPorts(doc, svc));
  const mode = $derived(String(get(doc, svc, 'network_mode') ?? ''));
  const shared = $derived(mode === 'host' || mode.startsWith('service:') || mode.startsWith('container:'));
  const showIp = $derived(ports.some((p) => p.hostIp));
  let ipColumn = $state(false);

  /** Host ports other services of the stack publish. */
  const stackPorts = $derived.by(() => {
    const out = new Map<string, string>();
    for (const { doc: d } of studio.docs) {
      for (const other of studio.services) {
        if (other === name) continue;
        const s = serviceMap(d, other);
        if (!s) continue;
        for (const p of getPorts(d, s)) if (p.published) out.set(`${p.published}/${p.protocol || 'tcp'}`, other);
      }
    }
    return out;
  });

  /** Host ports running containers outside this service use. */
  const serverPorts = $derived.by(() => {
    const out = new Map<string, string>();
    for (const c of live.list) {
      if (c.stack === studio.name && c.service === name) continue;
      for (const p of c.ports) if (p.public_port) out.set(`${p.public_port}/${p.protocol}`, c.display_name);
    }
    return out;
  });

  function clash(p: PortSpec): string | null {
    if (!p.published || p.published.includes('$')) return null;
    const key = `${p.published}/${p.protocol || 'tcp'}`;
    const s = stackPorts.get(key);
    if (s) return `${s} in this stack publishes ${p.published} too`;
    const c = serverPorts.get(key);
    return c ? `${c} already uses port ${p.published} on this server` : null;
  }

  function update(i: number, patch: Partial<PortSpec>) {
    edit(`port${i}`, (d, s) => setPort(d, s, i, { ...ports[i], ...patch }));
  }

  /** A host port nothing uses yet, near `from`. */
  function freePort(from: number): string {
    const used = new Set([...stackPorts.keys(), ...serverPorts.keys(), ...ports.map((p) => `${p.published}/${p.protocol || 'tcp'}`)]);
    for (let n = from; n < from + 200; n++) if (!used.has(`${n}/tcp`)) return String(n);
    return String(from);
  }

  function add() {
    const last = ports[ports.length - 1];
    const base = last ? Number(last.published || last.target) + 1 || 8080 : 8080;
    edit('ports-add', (d, s) => setPort(d, s, -1, { hostIp: '', published: freePort(base), target: '', protocol: '', long: false }));
    setTimeout(() => document.querySelectorAll<HTMLInputElement>(`[data-port-target="${name}"]`).item(ports.length)?.focus(), 30);
  }

  const expose = $derived(getList(doc, svc, 'expose'));
  const host = $derived(location.hostname);
</script>

{#if shared}
  <p class="note">
    {#if mode === 'host'}This service uses the server's network directly, so it listens on the server's ports as they are. Publishing ports doesn't apply.
    {:else}This service shares the network of <code>{mode.split(':')[1]}</code>; publish ports on that one instead.{/if}
  </p>
{:else}
  <Field label="Published ports" hint="Open a port on the server and send it to the container. An empty server port picks a random free one." wide>
    <div class="ports">
      {#if ports.length}
        <div class="row head" class:ip={showIp || ipColumn}>
          {#if showIp || ipColumn}<span>Address</span>{/if}<span>Server port</span><span></span><span>Container port</span><span>Protocol</span><span></span>
        </div>
      {/if}
      {#each ports as p, i (i)}
        {@const warn = clash(p)}
        <div class="row" class:ip={showIp || ipColumn}>
          {#if showIp || ipColumn}
            <input class="input mono" value={p.hostIp} placeholder="all" aria-label="Listen address {i + 1}" spellcheck="false" oninput={(e) => update(i, { hostIp: e.currentTarget.value.trim() })} />
          {/if}
          <input class="input mono" class:invalid={!!warn} value={p.published} placeholder="random" inputmode="numeric" aria-label="Server port {i + 1}" oninput={(e) => update(i, { published: e.currentTarget.value.trim() })} />
          <span class="arrow" aria-hidden="true">→</span>
          <input class="input mono" data-port-target={name} value={p.target} placeholder="80" inputmode="numeric" aria-label="Container port {i + 1}" oninput={(e) => update(i, { target: e.currentTarget.value.trim() })} />
          <select class="input" value={p.protocol || 'tcp'} aria-label="Protocol {i + 1}" onchange={(e) => update(i, { protocol: e.currentTarget.value === 'tcp' ? '' : e.currentTarget.value })}>
            <option value="tcp">TCP</option>
            <option value="udp">UDP</option>
            <option value="sctp">SCTP</option>
          </select>
          <div class="tail">
            {#if p.published && (p.protocol || 'tcp') === 'tcp' && /^\d+$/.test(p.published)}
              <a class="btn ghost icon small" href={resolveLink(`http://{host}:${p.published}`)} target="_blank" rel="noopener" aria-label="Open port {p.published}" title="Open http://{host}:{p.published}"><ExternalLink size={14} /></a>
            {/if}
            <button type="button" class="btn ghost icon small" aria-label="Remove port {i + 1}" onclick={() => edit('ports-remove', (d, s) => removeItem(d, s, 'ports', i))}><X size={15} /></button>
          </div>
          {#if warn}<span class="warn"><TriangleAlert size={13} />{warn}</span>{/if}
        </div>
      {/each}
      <div class="actions">
        <button type="button" class="btn small" onclick={add}><Plus size={15} />Add port</button>
        {#if !showIp}<button type="button" class="btn small ghost" onclick={() => (ipColumn = !ipColumn)}>{ipColumn ? 'Hide' : 'Listen on one'} address</button>{/if}
      </div>
    </div>
  </Field>
{/if}

<Field label="Ports for other containers" hint="Reachable only by other containers on the same network, not from outside. Most images declare these already.">
  <StringList items={expose} onchange={(items) => edit('expose', (d, s) => setList(d, s, 'expose', items))} placeholder="3000" addLabel="Add" label="Port" />
</Field>

<style>
  .note {
    margin: 0 0 8px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text-2);
  }
  .ports {
    display: grid;
    gap: 6px;
    max-width: 720px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 18px minmax(0, 1fr) 96px 64px;
    gap: 6px;
    align-items: center;
  }
  .row.ip {
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr) 18px minmax(0, 1fr) 96px 64px;
  }
  .row.head {
    color: var(--text-3);
    font-size: var(--fs-xs);
  }
  .row .input {
    min-height: 32px;
    padding: 4px 8px;
  }
  .arrow {
    color: var(--text-3);
    text-align: center;
  }
  .tail {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }
  .warn {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--warn);
    font-size: var(--fs-xs);
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  @media (max-width: 760px) {
    .row,
    .row.ip {
      grid-template-columns: minmax(0, 1fr) 18px minmax(0, 1fr);
    }
    .row.head {
      display: none;
    }
  }
</style>
