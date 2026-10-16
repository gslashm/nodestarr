<!-- What a service runs (an image or a Dockerfile), its name, restart policy, command and start order. -->
<script lang="ts">
  import FilePlus from '@lucide/svelte/icons/file-plus';
  import FileCode from '@lucide/svelte/icons/file-code';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import Field from '../../Field.svelte';
  import Segmented from '../../Segmented.svelte';
  import ImageField from '../ImageField.svelte';
  import KeyValueList from '../KeyValueList.svelte';
  import { DOCKERFILE_TEMPLATES } from '../../../lib/compose/templates';
  import { get, getCommand, getDependencies, readKeyValues, set, setDependencies, type DependCondition } from '../../../lib/compose/service';
  import { buildFiles, joinPath, useStudio } from '../../../lib/compose/studio.svelte';
  import { createNode, deref, ensureMap, isMap, pairOf, setKey, type YAMLMap, type YDoc } from '../../../lib/compose/yamldoc';
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

  const build = $derived(get(doc, svc, 'build'));
  const mode = $derived(build !== undefined && build !== null ? 'build' : 'image');
  const files = $derived(buildFiles(build));
  const buildMap = $derived(build && typeof build === 'object' ? (build as Record<string, unknown>) : null);
  const dockerfile = $derived(files ? studio.files[files.dockerfile] : undefined);

  const RESTART: { value: string; label: string }[] = [
    { value: 'unless-stopped', label: 'Unless stopped' },
    { value: 'always', label: 'Always' },
    { value: 'on-failure', label: 'On failure' },
    { value: 'no', label: 'Never' },
  ];
  const restart = $derived(String(get(doc, svc, 'restart') ?? ''));

  const command = $derived(getCommand(doc, svc, 'command'));
  const commandText = $derived(Array.isArray(command.value) ? shellJoin(command.value) : command.value);

  function setCommand(text: string) {
    edit('command', (d, s) => set(d, s, 'command', command.form === 'list' ? shellSplit(text) : text));
  }

  /** Switching to a Dockerfile starts one from the current image. */
  async function setMode(m: 'image' | 'build') {
    if (m === mode) return;
    if (m === 'build') {
      const image = String(get(doc, svc, 'image') ?? '');
      const context = `./${name}`;
      edit('mode', (d, s) => {
        set(d, s, 'build', context);
        set(d, s, 'image', undefined);
      });
      const rel = joinPath(context, 'Dockerfile');
      await studio.open(rel, 'dockerfile');
      if (!studio.text(rel)) {
        const t = DOCKERFILE_TEMPLATES[0];
        studio.setText(rel, t.content(image), `svc:${name}:mode`);
        const ignore = joinPath(context, '.dockerignore');
        await studio.open(ignore, 'dockerignore');
        if (!studio.text(ignore)) studio.setText(ignore, t.ignore, `svc:${name}:mode`);
      }
      ui.toast(`${name} is now built from ${rel}`, 'success');
    } else {
      const ok = await ui.confirm({
        title: `Run ${name} from an image?`,
        message: 'The build settings are removed from the compose file. The Dockerfile itself stays where it is.',
        confirmLabel: 'Use an image',
      });
      if (ok) edit('mode', (d, s) => set(d, s, 'build', undefined));
    }
  }

  /** Changes one build setting, switching `build: ./dir` to the long form when needed. */
  function setBuild(key: string, value: unknown) {
    edit('build', (d, s) => {
      const p = pairOf(s, 'build');
      if (key === 'context' && p && typeof build === 'string') {
        setKey(d, s, 'build', value || '.');
        return;
      }
      const m = ensureMap(d, s, 'build');
      if (p && !buildMap && typeof build === 'string') setKey(d, m, 'context', build, ['context', 'dockerfile', 'target', 'args']);
      const empty = value === '' || value === undefined || (Array.isArray(value) && !value.length);
      setKey(d, m, key, empty ? undefined : value, ['context', 'dockerfile', 'dockerfile_inline', 'target', 'args', 'tags', 'platforms']);
    });
  }

  async function openDockerfile() {
    if (!files) return;
    const f = await studio.open(files.dockerfile, 'dockerfile');
    studio.select({ kind: 'file', rel: f.rel });
  }

  const buildArgs = $derived.by(() => {
    const b = deref(doc, pairOf(svc, 'build')?.value);
    return readKeyValues(doc, isMap(b) ? deref(doc, pairOf(b, 'args')?.value) : undefined).entries;
  });

  function setArgs(entries: { key: string; value: string | null }[]) {
    edit('build-args', (d, s) => {
      const m = ensureMap(d, s, 'build');
      if (typeof build === 'string') setKey(d, m, 'context', build, ['context', 'dockerfile', 'target', 'args']);
      if (!entries.length) {
        setKey(d, m, 'args', undefined);
        return;
      }
      const body: Record<string, string | null> = {};
      for (const e of entries) body[e.key] = e.value;
      setKey(d, m, 'args', createNode(d, body), ['context', 'dockerfile', 'target', 'args']);
    });
  }

  // Start order.
  const deps = $derived(getDependencies(doc, svc));
  const others = $derived(studio.services.filter((s) => s !== name && !deps.some((d) => d.service === s)));
  let newDep = $state('');
  const CONDITIONS: { value: DependCondition; label: string }[] = [
    { value: 'service_started', label: 'has started' },
    { value: 'service_healthy', label: 'is healthy' },
    { value: 'service_completed_successfully', label: 'has finished' },
  ];
</script>

<Field label="Runs">
  <Segmented
    label="What the service runs"
    options={[
      { value: 'image', label: 'An image' },
      { value: 'build', label: 'A Dockerfile' },
    ]}
    value={mode}
    onchange={(v) => setMode(v)}
  />
</Field>

{#if mode === 'image'}
  <Field label="Image" hint="Pulled from its registry when it isn't on this machine yet.">
    <ImageField {name} {doc} {svc} />
  </Field>
{:else}
  <Field label="Build folder" hint="What the build can see, relative to the compose file.">
    <input class="input mono" value={files?.context ?? '.'} spellcheck="false" aria-label="Build folder" onchange={(e) => setBuild('context', e.currentTarget.value)} />
  </Field>
  <Field label="Dockerfile">
    <div class="inline">
      <input class="input mono" value={String(buildMap?.dockerfile ?? 'Dockerfile')} spellcheck="false" aria-label="Dockerfile name" onchange={(e) => setBuild('dockerfile', e.currentTarget.value === 'Dockerfile' ? '' : e.currentTarget.value)} />
      <button type="button" class="btn" onclick={openDockerfile}>
        {#if dockerfile && !dockerfile.loading && dockerfile.original === null && !dockerfile.text}<FilePlus size={16} />Create it{:else}<FileCode size={16} />Edit it{/if}
      </button>
    </div>
  </Field>
  <Field label="Target stage" hint="For multi-stage Dockerfiles: the stage to build. Empty builds the last one.">
    <input class="input mono short" value={String(buildMap?.target ?? '')} spellcheck="false" aria-label="Target stage" onchange={(e) => setBuild('target', e.currentTarget.value)} />
  </Field>
  <Field label="Build arguments" hint="Values for ARG lines in the Dockerfile.">
    <KeyValueList entries={buildArgs} onchange={setArgs} keyPlaceholder="VERSION" addLabel="Add argument" />
  </Field>
  <Field label="Name the image" hint="Optional tag for the built image, like myapp:1.0.">
    <input class="input mono" value={String(get(doc, svc, 'image') ?? '')} spellcheck="false" aria-label="Image name" placeholder="{studio.name || 'stack'}-{name}" onchange={(e) => edit('image', (d, s) => set(d, s, 'image', e.currentTarget.value.trim()))} />
  </Field>
{/if}

<Field label="Container name" hint="Empty lets compose name it {studio.name || 'stack'}-{name}-1. Names must be unique on the server.">
  <input class="input mono" value={String(get(doc, svc, 'container_name') ?? '')} placeholder="{studio.name || 'stack'}-{name}-1" spellcheck="false" aria-label="Container name" oninput={(e) => edit('container_name', (d, s) => set(d, s, 'container_name', e.currentTarget.value))} />
</Field>

<Field label="Restart" hint={restart === 'no' || !restart ? "Docker won't start it again after it exits or the server reboots." : restart === 'on-failure' ? 'Restarted only when it exits with an error.' : restart === 'always' ? 'Restarted whenever it exits, even after you stop it and the server reboots.' : 'Restarted after crashes and reboots, unless you stopped it.'}>
  <div class="inline">
    <Segmented label="Restart policy" options={RESTART} value={restart || 'no'} onchange={(v) => edit('restart', (d, s) => set(d, s, 'restart', v))} />
    {#if restart && !RESTART.some((r) => r.value === restart)}<code>{restart}</code>{/if}
  </div>
</Field>

<Field label="Command" hint="Arguments for the image's program. Empty uses the image's default.">
  <input class="input mono" value={commandText} placeholder="Image default" spellcheck="false" aria-label="Command" oninput={(e) => setCommand(e.currentTarget.value)} />
</Field>

<Field label="Starts after" hint="Services compose starts first. “Is healthy” waits for their health check.">
  <div class="deps">
    {#each deps as d, i (d.service)}
      <div class="dep">
        <span class="mono">{d.service}</span>
        <select class="input" value={d.condition} aria-label="Wait until {d.service}" onchange={(e) => edit('depends', (doc2, s) => setDependencies(doc2, s, deps.map((x, j) => (j === i ? { ...x, condition: e.currentTarget.value as DependCondition } : x))))}>
          {#each CONDITIONS as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
        </select>
        <label class="check" title="Restart {name} when {d.service} is restarted or updated">
          <input type="checkbox" checked={d.restart} onchange={(e) => edit('depends', (doc2, s) => setDependencies(doc2, s, deps.map((x, j) => (j === i ? { ...x, restart: e.currentTarget.checked } : x))))} />
          restart with it
        </label>
        <button type="button" class="btn ghost icon small" aria-label="Remove {d.service}" onclick={() => edit('depends', (doc2, s) => setDependencies(doc2, s, deps.filter((_, j) => j !== i)))}><X size={15} /></button>
      </div>
    {/each}
    {#if others.length}
      <div class="dep add">
        <select class="input" bind:value={newDep} aria-label="Service to start first">
          <option value="">Choose a service…</option>
          {#each others as o (o)}<option value={o}>{o}</option>{/each}
        </select>
        <button
          type="button"
          class="btn small"
          disabled={!newDep}
          onclick={() => {
            const svcName = newDep;
            edit('depends', (doc2, s) => setDependencies(doc2, s, [...deps, { service: svcName, condition: 'service_started', restart: false, required: true }]));
            newDep = '';
          }}><Plus size={15} />Add</button
        >
      </div>
    {:else if !deps.length}
      <span class="faint small">No other services in this stack.</span>
    {/if}
  </div>
</Field>

<style>
  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .inline .input {
    flex: 1;
    min-width: 160px;
  }
  .short {
    max-width: 240px;
  }
  .deps {
    display: grid;
    gap: 6px;
  }
  .dep {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .dep .mono {
    min-width: 110px;
  }
  .dep .input {
    width: auto;
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .small {
    font-size: var(--fs-sm);
  }
</style>
