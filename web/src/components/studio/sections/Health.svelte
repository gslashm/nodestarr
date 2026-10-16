<!-- The health check: the image's own, a custom command, or none. -->
<script lang="ts">
  import Field from '../../Field.svelte';
  import Segmented from '../../Segmented.svelte';
  import { images } from '../../../lib/compose/images.svelte';
  import { interpolate } from '../../../lib/compose/interpolate';
  import { get, getHealthcheck, getPorts, setHealthcheck, type Healthcheck } from '../../../lib/compose/service';
  import { useStudio } from '../../../lib/compose/studio.svelte';
  import type { YAMLMap, YDoc } from '../../../lib/compose/yamldoc';
  import { shellJoin, shellSplit } from '../../../lib/spec';
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

  const hc = $derived(getHealthcheck(doc, svc));
  const mode = $derived(!hc ? 'image' : hc.kind === 'none' ? 'off' : 'custom');
  const image = $derived(interpolate(String(get(doc, svc, 'image') ?? ''), (n) => studio.project.get(n)).value.trim());
  const imageCheck = $derived.by(() => {
    const i = images.inspect[image];
    const t = i?.state === 'ok' ? i.data.defaults.healthcheck : null;
    if (!t?.length || t[0] === 'NONE') return null;
    return t[0] === 'CMD-SHELL' ? t.slice(1).join(' ') : shellJoin(t[0] === 'CMD' ? t.slice(1) : t);
  });

  const EMPTY: Healthcheck = { kind: 'shell', command: '', args: [], interval: '30s', timeout: '10s', retries: '3', startPeriod: '', startInterval: '' };

  function setMode(m: 'image' | 'custom' | 'off') {
    if (m === 'image') edit('health', (d, s) => setHealthcheck(d, s, null));
    else if (m === 'off') edit('health', (d, s) => setHealthcheck(d, s, { ...(hc ?? EMPTY), kind: 'none' }));
    else edit('health', (d, s) => setHealthcheck(d, s, hc && hc.kind !== 'none' ? hc : { ...EMPTY, command: imageCheck ?? '' }));
  }

  function update(patch: Partial<Healthcheck>) {
    if (!hc) return;
    edit('health', (d, s) => setHealthcheck(d, s, { ...hc, ...patch }));
  }

  const port = $derived(getPorts(doc, svc)[0]?.target.split('-')[0] || '80');
  const PRESETS = $derived([
    { label: 'Web page answers (curl)', command: `curl -fsS http://localhost:${port}/ || exit 1` },
    { label: 'Web page answers (wget)', command: `wget -q --spider http://localhost:${port}/ || exit 1` },
    { label: 'Port is open', command: `nc -z localhost ${port} || exit 1` },
    { label: 'PostgreSQL is ready', command: 'pg_isready -U "$${POSTGRES_USER:-postgres}"' },
  ]);
</script>

<Field label="Health check" hint="Docker runs the check regularly; services can wait for a healthy one, and Nodestarr shows when it fails.">
  <Segmented
    label="Health check"
    options={[
      { value: 'image', label: "The image's" },
      { value: 'custom', label: 'Custom' },
      { value: 'off', label: 'Off' },
    ]}
    value={mode}
    onchange={(v) => setMode(v)}
  />
</Field>

{#if mode === 'image'}
  <p class="note">{imageCheck ? `The image checks itself with: ${imageCheck}` : "The image has no health check of its own (or it hasn't been looked up yet)."}</p>
{:else if hc && hc.kind !== 'none'}
  <Field label="Command" hint={hc.kind === 'shell' ? 'Runs in a shell inside the container; exit code 0 means healthy.' : 'Runs directly, without a shell.'}>
    <div class="cmd">
      <select class="input kind" value={hc.kind} aria-label="How the command runs" onchange={(e) => update(e.currentTarget.value === 'exec' ? { kind: 'exec', args: shellSplit(hc.command) } : { kind: 'shell', command: shellJoin(hc.args) })}>
        <option value="shell">Shell</option>
        <option value="exec">Direct</option>
      </select>
      <input
        class="input mono"
        value={hc.kind === 'shell' ? hc.command : shellJoin(hc.args)}
        placeholder="curl -fsS http://localhost/ || exit 1"
        spellcheck="false"
        aria-label="Health check command"
        oninput={(e) => update(hc.kind === 'shell' ? { command: e.currentTarget.value } : { args: shellSplit(e.currentTarget.value) })}
      />
    </div>
    <div class="presets">
      {#each PRESETS as p (p.label)}
        <button type="button" class="chip preset" onclick={() => update({ kind: 'shell', command: p.command })}>{p.label}</button>
      {/each}
    </div>
  </Field>
  <Field label="Timing" hint="Durations like 30s, 1m or 1m30s.">
    <div class="nums">
      <label>Every <input class="input mono" value={hc.interval} placeholder="30s" oninput={(e) => update({ interval: e.currentTarget.value })} /></label>
      <label>Timeout <input class="input mono" value={hc.timeout} placeholder="30s" oninput={(e) => update({ timeout: e.currentTarget.value })} /></label>
      <label>Retries <input class="input mono" value={hc.retries} placeholder="3" inputmode="numeric" oninput={(e) => update({ retries: e.currentTarget.value })} /></label>
      <label>Grace period <input class="input mono" value={hc.startPeriod} placeholder="0s" oninput={(e) => update({ startPeriod: e.currentTarget.value })} /></label>
      <label>Check every, while starting <input class="input mono" value={hc.startInterval} placeholder="5s" oninput={(e) => update({ startInterval: e.currentTarget.value })} /></label>
    </div>
  </Field>
{:else}
  <p class="note">Health checks are off, including the image's own.</p>
{/if}

<style>
  .note {
    margin: 0 0 8px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .cmd {
    display: flex;
    gap: 6px;
  }
  .kind {
    width: 110px;
    flex: none;
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .preset {
    cursor: pointer;
  }
  .preset:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
  .nums {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
  }
  .nums label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .nums .input {
    width: 76px;
    min-height: 32px;
    padding: 4px 8px;
  }
</style>
