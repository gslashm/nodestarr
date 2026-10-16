<!-- Less common settings: user, entrypoint, switches, capabilities, kernel settings, labels, logging, stopping and pulling. -->
<script lang="ts">
  import Field from '../../Field.svelte';
  import Toggle from '../../Toggle.svelte';
  import KeyValueList from '../KeyValueList.svelte';
  import StringList from '../StringList.svelte';
  import { deleteKeyValue, get, getCommand, getList, readKeyValues, set, setBool, setKeyValue, setKeyValues, setList } from '../../../lib/compose/service';
  import { useStudio } from '../../../lib/compose/studio.svelte';
  import { createNode, ensureMap, lookup, pairOf, setKey, type YAMLMap, type YDoc } from '../../../lib/compose/yamldoc';
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

  const entrypoint = $derived(getCommand(doc, svc, 'entrypoint'));
  const labels = $derived(readKeyValues(doc, lookup(doc, svc, 'labels').node).entries);
  const label = (k: string) => labels.find((l) => l.key === k)?.value ?? '';
  const sysctls = $derived(readKeyValues(doc, lookup(doc, svc, 'sysctls').node).entries);
  const logging = $derived((get(doc, svc, 'logging') as { driver?: string; options?: Record<string, unknown> } | undefined) ?? {});
  const logOptions = $derived(Object.entries(logging.options ?? {}).map(([key, value]) => ({ key, value: value === null ? null : String(value) })));

  function setLabel(key: string, value: string) {
    edit(`label:${key}`, (d, s) => (value ? setKeyValue(d, s, 'labels', key, value) : deleteKeyValue(d, s, 'labels', key)));
  }

  function setLogging(driver: string, options: { key: string; value: string | null }[]) {
    edit('logging', (d, s) => {
      if (!driver && !options.length) {
        set(d, s, 'logging', undefined);
        return;
      }
      const m = ensureMap(d, s, 'logging');
      setKey(d, m, 'driver', driver || undefined, ['driver', 'options']);
      const opts: Record<string, string> = {};
      for (const o of options) opts[o.key] = o.value ?? '';
      setKey(d, m, 'options', options.length ? createNode(d, opts) : undefined, ['driver', 'options']);
      if (!pairOf(m, 'driver') && !pairOf(m, 'options')) set(d, s, 'logging', undefined);
    });
  }

  const PULL = [
    { value: '', label: 'When missing (default)' },
    { value: 'always', label: 'Always, on every deploy' },
    { value: 'daily', label: 'Once a day' },
    { value: 'weekly', label: 'Once a week' },
    { value: 'never', label: 'Never (use local images)' },
    { value: 'build', label: 'Always build' },
  ];
  const pull = $derived(String(get(doc, svc, 'pull_policy') ?? ''));
  const CAPS = ['NET_ADMIN', 'NET_RAW', 'SYS_ADMIN', 'SYS_TIME', 'SYS_NICE', 'SYS_MODULE', 'CHOWN', 'SETUID', 'SETGID', 'DAC_OVERRIDE', 'MKNOD', 'ALL'];
</script>

<Field label="Nodestarr" hint="How Nodestarr shows this app: a display name and the address its Open button goes to.">
  <div class="pair">
    <input class="input" value={label('nodestarr.name')} placeholder="Display name" aria-label="Display name" oninput={(e) => setLabel('nodestarr.name', e.currentTarget.value)} />
    <input class="input mono" value={label('nodestarr.url')} placeholder="https://app.example.com" aria-label="Web address" spellcheck="false" oninput={(e) => setLabel('nodestarr.url', e.currentTarget.value.trim())} />
  </div>
</Field>

<Field label="User" hint="user or user:group, by name or number. Empty uses the image's default.">
  <input class="input mono short" value={String(get(doc, svc, 'user') ?? '')} placeholder="Image default" spellcheck="false" aria-label="User" oninput={(e) => edit('user', (d, s) => set(d, s, 'user', e.currentTarget.value))} />
</Field>

<Field label="Working folder">
  <input class="input mono" value={String(get(doc, svc, 'working_dir') ?? '')} placeholder="Image default" spellcheck="false" aria-label="Working folder" oninput={(e) => edit('workdir', (d, s) => set(d, s, 'working_dir', e.currentTarget.value))} />
</Field>

<Field label="Entrypoint" hint="Replaces the program the image starts. Rarely needed.">
  <input
    class="input mono"
    value={Array.isArray(entrypoint.value) ? shellJoin(entrypoint.value) : entrypoint.value}
    placeholder="Image default"
    spellcheck="false"
    aria-label="Entrypoint"
    oninput={(e) => edit('entrypoint', (d, s) => set(d, s, 'entrypoint', entrypoint.form === 'list' ? shellSplit(e.currentTarget.value) : e.currentTarget.value))}
  />
</Field>

<Field label="Options">
  <div class="toggles">
    <Toggle checked={get(doc, svc, 'init') === true} label="Run an init process" hint="Reaps zombie processes and passes stop signals on." onchange={(v) => edit('init', (d, s) => setBool(d, s, 'init', v))} />
    <Toggle checked={get(doc, svc, 'read_only') === true} label="Read-only file system" hint="Only folders, volumes and tmpfs can be written." onchange={(v) => edit('ro', (d, s) => setBool(d, s, 'read_only', v))} />
    <Toggle checked={get(doc, svc, 'tty') === true} label="Allocate a terminal (TTY)" onchange={(v) => edit('tty', (d, s) => setBool(d, s, 'tty', v))} />
    <Toggle checked={get(doc, svc, 'stdin_open') === true} label="Keep standard input open" onchange={(v) => edit('stdin', (d, s) => setBool(d, s, 'stdin_open', v))} />
    <Toggle checked={get(doc, svc, 'privileged') === true} label="Privileged" hint="Full access to the server's devices. Avoid unless the app needs it." onchange={(v) => edit('privileged', (d, s) => setBool(d, s, 'privileged', v))} />
  </div>
</Field>

<Field label="Add capabilities" hint="Like NET_ADMIN for VPN containers.">
  <StringList items={getList(doc, svc, 'cap_add')} onchange={(items) => edit('cap_add', (d, s) => setList(d, s, 'cap_add', items))} placeholder="NET_ADMIN" suggestions={CAPS} label="Capability" />
</Field>
<Field label="Drop capabilities">
  <StringList items={getList(doc, svc, 'cap_drop')} onchange={(items) => edit('cap_drop', (d, s) => setList(d, s, 'cap_drop', items))} placeholder="ALL" suggestions={CAPS} label="Capability" />
</Field>
<Field label="Security options">
  <StringList items={getList(doc, svc, 'security_opt')} onchange={(items) => edit('secopt', (d, s) => setList(d, s, 'security_opt', items))} placeholder="no-new-privileges:true" suggestions={['no-new-privileges:true', 'apparmor:unconfined', 'seccomp:unconfined']} label="Option" />
</Field>
<Field label="Kernel settings" hint="sysctls, like net.ipv4.ip_forward=1.">
  <KeyValueList entries={sysctls} onchange={(entries) => edit('sysctls', (d, s) => setKeyValues(d, s, 'sysctls', entries))} keyPlaceholder="net.ipv4.ip_forward" addLabel="Add setting" />
</Field>
<Field label="Labels" hint="Read by tools like Traefik, Watchtower or Nodestarr.">
  <KeyValueList entries={labels} onchange={(entries) => edit('labels', (d, s) => setKeyValues(d, s, 'labels', entries))} keyPlaceholder="traefik.enable" addLabel="Add label" />
</Field>

<Field label="Logging" hint="json-file (the default) keeps logs on disk; max-size and max-file stop them from filling it.">
  <div class="log">
    <input class="input mono short" value={logging.driver ?? ''} placeholder="json-file" list="log-drivers" spellcheck="false" aria-label="Log driver" onchange={(e) => setLogging(e.currentTarget.value.trim(), logOptions)} />
    <datalist id="log-drivers">{#each ['json-file', 'local', 'journald', 'syslog', 'none'] as d (d)}<option value={d}></option>{/each}</datalist>
    <KeyValueList entries={logOptions} onchange={(entries) => setLogging(logging.driver ?? '', entries)} keyPlaceholder="max-size" valuePlaceholder="10m" addLabel="Add option" />
  </div>
</Field>

<Field label="Stopping" hint="How long compose waits after the stop signal before killing it, like 30s.">
  <div class="pair">
    <input class="input mono" value={String(get(doc, svc, 'stop_grace_period') ?? '')} placeholder="10s" spellcheck="false" aria-label="Stop grace period" oninput={(e) => edit('stop', (d, s) => set(d, s, 'stop_grace_period', e.currentTarget.value))} />
    <input class="input mono" value={String(get(doc, svc, 'stop_signal') ?? '')} placeholder="SIGTERM" spellcheck="false" aria-label="Stop signal" oninput={(e) => edit('stopsig', (d, s) => set(d, s, 'stop_signal', e.currentTarget.value))} />
  </div>
</Field>

<Field label="Pull the image">
  <select class="input short" value={PULL.some((p) => p.value === pull) ? pull : 'custom'} aria-label="Pull policy" onchange={(e) => edit('pull', (d, s) => set(d, s, 'pull_policy', e.currentTarget.value))}>
    {#each PULL as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
    {#if !PULL.some((p) => p.value === pull)}<option value="custom">{pull}</option>{/if}
  </select>
</Field>

<Field label="Platform" hint="Only to run an image made for another architecture, like linux/amd64.">
  <input class="input mono short" value={String(get(doc, svc, 'platform') ?? '')} placeholder="This server's" spellcheck="false" aria-label="Platform" oninput={(e) => edit('platform', (d, s) => set(d, s, 'platform', e.currentTarget.value))} />
</Field>

<Field label="Profiles" hint="A service with profiles only starts when one of them is enabled (COMPOSE_PROFILES in .env).">
  <StringList items={getList(doc, svc, 'profiles')} onchange={(items) => edit('profiles', (d, s) => setList(d, s, 'profiles', items))} placeholder="debug" label="Profile" />
</Field>

<style>
  .short {
    max-width: 260px;
  }
  .pair {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 6px;
  }
  .toggles {
    display: grid;
    gap: 14px;
    padding-top: 6px;
  }
  .log {
    display: grid;
    gap: 8px;
  }
</style>
