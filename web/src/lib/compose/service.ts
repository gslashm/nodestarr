// Compose services on a parsed document: reading their settings for the
// visual editor and writing changes back in the form the file already uses
// (short or long syntax, list or mapping), so a small change stays small.

import {
  YAMLMap,
  YAMLSeq,
  createNode,
  deref,
  ensureMap,
  isMap,
  isScalar,
  isSeq,
  keysOf,
  lookup,
  matchSpacing,
  pairOf,
  scalarText,
  setKey,
  setScalarText,
  stringNode,
  toJs,
  type Node,
  type Pair,
  type YDoc,
} from './yamldoc';

/** Where new service keys go, so generated files read in the usual order. */
export const SERVICE_ORDER = [
  'image',
  'build',
  'container_name',
  'hostname',
  'restart',
  'user',
  'working_dir',
  'entrypoint',
  'command',
  'env_file',
  'environment',
  'ports',
  'expose',
  'volumes',
  'tmpfs',
  'network_mode',
  'networks',
  'dns',
  'extra_hosts',
  'depends_on',
  'healthcheck',
  'devices',
  'group_add',
  'cap_add',
  'cap_drop',
  'security_opt',
  'privileged',
  'mem_limit',
  'cpus',
  'shm_size',
  'deploy',
  'logging',
  'labels',
] as const;

export const TOP_ORDER = ['name', 'services', 'networks', 'volumes', 'configs', 'secrets'] as const;

// --------------------------------------------------------------- services

export function servicesMap(doc: YDoc): YAMLMap | null {
  if (!isMap(doc.contents)) return null;
  const v = deref(doc, pairOf(doc.contents, 'services')?.value);
  return isMap(v) ? v : null;
}

export function serviceNames(doc: YDoc): string[] {
  const s = servicesMap(doc);
  return s ? keysOf(s) : [];
}

export function serviceMap(doc: YDoc, name: string): YAMLMap | null {
  const s = servicesMap(doc);
  if (!s) return null;
  const v = deref(doc, pairOf(s, name)?.value);
  return isMap(v) ? v : null;
}

/** The services mapping, created when the file has none yet. */
export function ensureServices(doc: YDoc): YAMLMap {
  if (!isMap(doc.contents)) doc.contents = new YAMLMap(doc.schema) as never;
  return ensureMap(doc, doc.contents as unknown as YAMLMap, 'services', TOP_ORDER);
}

/** A service's mapping for editing; null when it's missing or not a mapping. */
export function editService(doc: YDoc, name: string): YAMLMap | null {
  const s = servicesMap(doc);
  if (!s) return null;
  const p = pairOf(s, name);
  if (!p) return null;
  if (isMap(p.value)) return p.value;
  const v = deref(doc, p.value);
  if (!isMap(v)) return null;
  // An alias: give the service its own copy before changing it.
  const copy = createNode(doc, v.toJS(doc)) as YAMLMap;
  p.value = copy;
  return copy;
}

/** Reads a plain value (string, number, list, mapping) of a service key, merges included. */
export function get(doc: YDoc, svc: YAMLMap, key: string): unknown {
  return toJs(doc, lookup(doc, svc, key).node);
}

export function getText(doc: YDoc, svc: YAMLMap, key: string): string {
  const v = get(doc, svc, key);
  if (v === undefined || v === null) return '';
  return typeof v === 'object' ? JSON.stringify(v) : String(v);
}

/** Sets a service key in its usual place; empty values remove it. */
export function set(doc: YDoc, svc: YAMLMap, key: string, value: unknown): void {
  const empty = value === undefined || value === null || value === '' || (Array.isArray(value) && value.length === 0);
  setKey(doc, svc, key, empty ? undefined : value, SERVICE_ORDER);
}

export function setBool(doc: YDoc, svc: YAMLMap, key: string, on: boolean): void {
  setKey(doc, svc, key, on ? true : undefined, SERVICE_ORDER);
}

/** A list of strings (a lone string counts as a one-item list). */
export function getList(doc: YDoc, svc: YAMLMap, key: string): string[] {
  const v = get(doc, svc, key);
  if (v === undefined || v === null || v === '') return [];
  if (Array.isArray(v)) return v.map((x) => (x === null ? '' : typeof x === 'object' ? JSON.stringify(x) : String(x)));
  return [String(v)];
}

/**
 * Writes a list of strings, reusing the existing item nodes where the text
 * didn't change (so their comments and quotes stay).
 */
export function setList(doc: YDoc, svc: YAMLMap, key: string, items: string[]): void {
  if (!items.length) {
    setKey(doc, svc, key, undefined);
    return;
  }
  const p = pairOf(svc, key);
  if (p && isSeq(p.value)) {
    const old = p.value.items as unknown[];
    const quoted = old.some((n) => isScalar(n) && n.type === 'QUOTE_DOUBLE');
    p.value.items = items.map((text, i) => {
      const n = old[i];
      if (isScalar(n) && scalarText(doc, n) === text) return n;
      if (isScalar(n)) {
        setScalarText(n, text);
        return n;
      }
      return stringNode(text, quoted);
    });
    return;
  }
  set(doc, svc, key, items);
}

/** Text of a key/value collection written as a mapping or as `key=value` items. */
export interface KeyValueEntry {
  key: string;
  /** null for `KEY` (list) or `KEY:` (mapping) without a value. */
  value: string | null;
}

export type CollectionForm = 'map' | 'list';

export function readKeyValues(doc: YDoc, node: Node | undefined): { entries: KeyValueEntry[]; form: CollectionForm | null } {
  const n = deref(doc, node);
  if (isMap(n)) {
    const entries: KeyValueEntry[] = [];
    for (const p of n.items as Pair[]) {
      const k = scalarText(doc, p.key);
      if (k === null) continue;
      const v = deref(doc, p.value);
      const value = v === undefined || v === null ? null : isScalar(v) ? (v.value === null ? null : String(v.value)) : JSON.stringify(toJs(doc, v));
      entries.push({ key: k, value });
    }
    return { entries, form: 'map' };
  }
  if (isSeq(n)) {
    const entries: KeyValueEntry[] = [];
    for (const it of n.items) {
      const t = scalarText(doc, it);
      if (t === null) continue;
      const eq = t.indexOf('=');
      entries.push(eq < 0 ? { key: t, value: null } : { key: t.slice(0, eq), value: t.slice(eq + 1) });
    }
    return { entries, form: 'list' };
  }
  return { entries: [], form: null };
}

/** The form other services in the file use for `key`, to match it. */
function preferredForm(doc: YDoc, key: string): CollectionForm {
  const s = servicesMap(doc);
  let map = 0;
  let list = 0;
  for (const name of s ? keysOf(s) : []) {
    const svc = serviceMap(doc, name);
    const v = svc ? deref(doc, pairOf(svc, key)?.value) : undefined;
    if (isMap(v)) map++;
    else if (isSeq(v)) list++;
  }
  return list > map ? 'list' : 'map';
}

/**
 * Gives `svc` its own copy of `key` when it only inherits it through a merge
 * key, and returns the collection node to edit.
 */
function ownCollection(doc: YDoc, svc: YAMLMap, key: string): YAMLMap | YAMLSeq | null {
  const p = pairOf(svc, key);
  if (p) {
    if (isMap(p.value) || isSeq(p.value)) return p.value;
    const v = deref(doc, p.value);
    if (isMap(v) || isSeq(v)) {
      const copy = createNode(doc, v.toJS(doc)) as YAMLMap | YAMLSeq;
      p.value = copy;
      return copy;
    }
    return null;
  }
  const inherited = lookup(doc, svc, key);
  if (inherited.node && (isMap(inherited.node) || isSeq(inherited.node))) {
    const copy = createNode(doc, (inherited.node as YAMLMap | YAMLSeq).toJS(doc)) as YAMLMap | YAMLSeq;
    setKey(doc, svc, key, copy, SERVICE_ORDER);
    return copy;
  }
  return null;
}

function kvItemText(key: string, value: string | null): string {
  return value === null ? key : `${key}=${value}`;
}

/** Sets one entry of a key/value collection (environment, labels, sysctls...). */
export function setKeyValue(doc: YDoc, svc: YAMLMap, key: string, entryKey: string, value: string | null): void {
  let coll = ownCollection(doc, svc, key);
  if (!coll) {
    coll = preferredForm(doc, key) === 'list' ? new YAMLSeq(doc.schema) : new YAMLMap(doc.schema);
    setKey(doc, svc, key, coll, SERVICE_ORDER);
  }
  if (isMap(coll)) {
    const p = pairOf(coll, entryKey);
    if (p) {
      if (value === null) p.value = null;
      else if (isScalar(p.value)) setScalarText(p.value, value);
      else p.value = stringNode(value);
    } else {
      coll.items.push(doc.createPair(entryKey, value === null ? null : stringNode(value)));
    }
    return;
  }
  const items = coll.items as unknown[];
  const quoted = items.some((n) => isScalar(n) && n.type === 'QUOTE_DOUBLE');
  const at = items.findIndex((n) => {
    const t = scalarText(doc, n);
    return t !== null && (t === entryKey || t.startsWith(`${entryKey}=`));
  });
  const text = kvItemText(entryKey, value);
  if (at >= 0) {
    const n = items[at];
    if (isScalar(n)) setScalarText(n, text);
    else items[at] = stringNode(text, quoted);
  } else {
    items.push(stringNode(text, quoted));
  }
}

export function deleteKeyValue(doc: YDoc, svc: YAMLMap, key: string, entryKey: string): void {
  const coll = ownCollection(doc, svc, key);
  if (!coll) return;
  if (isMap(coll)) {
    coll.items = (coll.items as Pair[]).filter((p) => scalarText(doc, p.key) !== entryKey);
  } else {
    coll.items = coll.items.filter((n) => {
      const t = scalarText(doc, n);
      return !(t !== null && (t === entryKey || t.startsWith(`${entryKey}=`)));
    });
  }
  if (!coll.items.length) setKey(doc, svc, key, undefined);
}

export function renameKeyValue(doc: YDoc, svc: YAMLMap, key: string, from: string, to: string): void {
  const coll = ownCollection(doc, svc, key);
  if (!coll || from === to) return;
  if (isMap(coll)) {
    const p = pairOf(coll, from);
    if (p) {
      if (isScalar(p.key)) p.key.value = to;
      else p.key = stringNode(to);
    }
    return;
  }
  for (const n of coll.items) {
    const t = scalarText(doc, n);
    if (t === null || !isScalar(n)) continue;
    if (t === from) setScalarText(n, to);
    else if (t.startsWith(`${from}=`)) setScalarText(n, to + t.slice(from.length));
  }
}

/** Replaces a whole key/value collection, keeping its form. */
export function setKeyValues(doc: YDoc, svc: YAMLMap, key: string, entries: KeyValueEntry[]): void {
  const current = readKeyValues(doc, lookup(doc, svc, key).node);
  const form = current.form ?? preferredForm(doc, key);
  if (!entries.length) {
    setKey(doc, svc, key, undefined);
    return;
  }
  for (const e of current.entries) if (!entries.some((x) => x.key === e.key)) deleteKeyValue(doc, svc, key, e.key);
  if (!pairOf(svc, key)) setKey(doc, svc, key, form === 'list' ? new YAMLSeq(doc.schema) : new YAMLMap(doc.schema), SERVICE_ORDER);
  for (const e of entries) {
    const old = current.entries.find((x) => x.key === e.key);
    if (!old || old.value !== e.value) setKeyValue(doc, svc, key, e.key, e.value);
  }
}

// ------------------------------------------------------------------ ports

export interface PortSpec {
  hostIp: string;
  /** Host port or range; empty publishes on a random port. */
  published: string;
  target: string;
  /** '' means tcp. */
  protocol: string;
  /** Long-syntax extras, kept as they were. */
  mode?: string;
  name?: string;
  appProtocol?: string;
  long: boolean;
}

export function parsePort(text: string): PortSpec | null {
  let rest = text.trim();
  if (!rest) return null;
  let protocol = '';
  const slash = rest.lastIndexOf('/');
  if (slash > 0 && !/[:\]]/.test(rest.slice(slash))) {
    protocol = rest.slice(slash + 1);
    rest = rest.slice(0, slash);
  }
  let hostIp = '';
  if (rest.startsWith('[')) {
    const close = rest.indexOf(']');
    if (close < 0) return null;
    hostIp = rest.slice(1, close);
    rest = rest.slice(close + 1).replace(/^:/, '');
    const parts = rest.split(':');
    if (parts.length === 1) return { hostIp, published: '', target: parts[0], protocol, long: false };
    return { hostIp, published: parts[0], target: parts.slice(1).join(':'), protocol, long: false };
  }
  const parts = rest.split(':');
  if (parts.length === 1) return { hostIp: '', published: '', target: parts[0], protocol, long: false };
  if (parts.length === 2) return { hostIp: '', published: parts[0], target: parts[1], protocol, long: false };
  return { hostIp: parts.slice(0, -2).join(':'), published: parts[parts.length - 2], target: parts[parts.length - 1], protocol, long: false };
}

export function formatPort(p: PortSpec): string {
  let s = '';
  if (p.hostIp) s = `${p.hostIp.includes(':') ? `[${p.hostIp}]` : p.hostIp}:${p.published}:`;
  else if (p.published) s = `${p.published}:`;
  s += p.target;
  if (p.protocol && p.protocol !== 'tcp') s += `/${p.protocol}`;
  return s;
}

function portFromNode(doc: YDoc, n: unknown): PortSpec | null {
  const node = deref(doc, n);
  if (isScalar(node)) return parsePort(String(node.value ?? ''));
  if (isMap(node)) {
    const g = (k: string) => {
      const v = toJs(doc, pairOf(node, k)?.value);
      return v === undefined || v === null ? '' : String(v);
    };
    return {
      hostIp: g('host_ip'),
      published: g('published'),
      target: g('target'),
      protocol: g('protocol') === 'tcp' ? '' : g('protocol'),
      mode: g('mode') || undefined,
      name: g('name') || undefined,
      appProtocol: g('app_protocol') || undefined,
      long: true,
    };
  }
  return null;
}

export function getPorts(doc: YDoc, svc: YAMLMap): PortSpec[] {
  const n = lookup(doc, svc, 'ports').node;
  if (!isSeq(n)) return [];
  return n.items.map((it) => portFromNode(doc, it) ?? { hostIp: '', published: '', target: String(toJs(doc, it) ?? ''), protocol: '', long: false });
}

function portNode(doc: YDoc, p: PortSpec, old: unknown): unknown {
  if (p.long || p.mode || p.name || p.appProtocol) {
    const map = isMap(old) ? old : new YAMLMap(doc.schema);
    const put = (k: string, v: string | undefined, numeric = false) => {
      if (!v) {
        setKey(doc, map, k, undefined);
        return;
      }
      setKey(doc, map, k, numeric && /^\d+$/.test(v) ? Number(v) : v, ['name', 'target', 'published', 'host_ip', 'protocol', 'app_protocol', 'mode']);
    };
    put('target', p.target, true);
    put('published', p.published);
    put('host_ip', p.hostIp);
    put('protocol', p.protocol && p.protocol !== 'tcp' ? p.protocol : isMap(old) && pairOf(old, 'protocol') ? p.protocol || 'tcp' : '');
    put('mode', p.mode);
    put('name', p.name);
    put('app_protocol', p.appProtocol);
    return map;
  }
  const text = formatPort(p);
  if (isScalar(old)) {
    if (typeof old.value === 'number' && /^\d+$/.test(text)) old.value = Number(text);
    else setScalarText(old, text);
    return old;
  }
  // Ports are conventionally quoted ("8080:80").
  return stringNode(text, true);
}

/** Changes one port; index -1 appends. */
export function setPort(doc: YDoc, svc: YAMLMap, index: number, p: PortSpec): void {
  const seq = ownCollection(doc, svc, 'ports');
  if (!seq || !isSeq(seq)) {
    const fresh = new YAMLSeq(doc.schema);
    fresh.items.push(portNode(doc, p, null));
    setKey(doc, svc, 'ports', fresh, SERVICE_ORDER);
    return;
  }
  if (index < 0 || index >= seq.items.length) seq.items.push(portNode(doc, p, null));
  else seq.items[index] = portNode(doc, p, deref(doc, seq.items[index]));
}

export function removeItem(doc: YDoc, svc: YAMLMap, key: string, index: number): void {
  const seq = ownCollection(doc, svc, key);
  if (!seq || !isSeq(seq)) return;
  seq.items.splice(index, 1);
  if (!seq.items.length) setKey(doc, svc, key, undefined);
}

export function moveItem(doc: YDoc, svc: YAMLMap, key: string, from: number, to: number): void {
  const seq = ownCollection(doc, svc, key);
  if (!seq || !isSeq(seq) || to < 0 || to >= seq.items.length) return;
  const [it] = seq.items.splice(from, 1);
  seq.items.splice(to, 0, it);
}

// ---------------------------------------------------------------- volumes

export type MountType = 'bind' | 'volume' | 'tmpfs' | 'npipe' | 'image' | 'cluster';

export interface MountSpec {
  type: MountType;
  /** Host path (bind), volume name (volume), image (image); '' for anonymous volumes and tmpfs. */
  source: string;
  target: string;
  readOnly: boolean;
  /** Other short-syntax options (z, Z, cached, nocopy...). */
  options: string[];
  long: boolean;
}

/** Bind sources look like paths; anything else is a named volume. */
export function sourceKind(source: string): 'bind' | 'volume' {
  return /^(?:[./~]|\$\{?|[A-Za-z]:[\\/])/.test(source) || source.includes('/') ? 'bind' : 'volume';
}

export function parseMount(text: string): MountSpec {
  const parts = text.split(':');
  if (parts.length === 1) return { type: 'volume', source: '', target: parts[0], readOnly: false, options: [], long: false };
  const [source, target, mode = ''] = parts.length >= 3 ? [parts[0], parts[1], parts.slice(2).join(':')] : [parts[0], parts[1]];
  const opts = mode ? mode.split(',').filter(Boolean) : [];
  return {
    type: sourceKind(source),
    source,
    target,
    readOnly: opts.includes('ro'),
    options: opts.filter((o) => o !== 'ro' && o !== 'rw'),
    long: false,
  };
}

export function formatMount(m: MountSpec): string {
  if (!m.source) return m.target;
  const opts = [...(m.readOnly ? ['ro'] : []), ...m.options];
  return `${m.source}:${m.target}${opts.length ? `:${opts.join(',')}` : ''}`;
}

function mountFromNode(doc: YDoc, n: unknown): MountSpec | null {
  const node = deref(doc, n);
  if (isScalar(node)) return parseMount(String(node.value ?? ''));
  if (isMap(node)) {
    const v = toJs(doc, node) as Record<string, unknown>;
    return {
      type: (String(v.type ?? 'volume') as MountType) || 'volume',
      source: v.source === undefined || v.source === null ? '' : String(v.source),
      target: String(v.target ?? ''),
      readOnly: v.read_only === true || v.read_only === 'true',
      options: [],
      long: true,
    };
  }
  return null;
}

export function getMounts(doc: YDoc, svc: YAMLMap): MountSpec[] {
  const n = lookup(doc, svc, 'volumes').node;
  if (!isSeq(n)) return [];
  return n.items.map((it) => mountFromNode(doc, it) ?? parseMount(String(toJs(doc, it) ?? '')));
}

function mountNode(doc: YDoc, m: MountSpec, old: unknown): unknown {
  // tmpfs and image mounts only exist in the long syntax.
  if (m.long || m.type === 'tmpfs' || m.type === 'image' || m.type === 'npipe' || m.type === 'cluster') {
    const map = isMap(old) ? old : new YAMLMap(doc.schema);
    const order = ['type', 'source', 'target', 'read_only'];
    setKey(doc, map, 'type', m.type, order);
    setKey(doc, map, 'source', m.type === 'tmpfs' || !m.source ? undefined : m.source, order);
    setKey(doc, map, 'target', m.target, order);
    setKey(doc, map, 'read_only', m.readOnly ? true : undefined, order);
    return map;
  }
  const text = formatMount(m);
  if (isScalar(old)) {
    setScalarText(old, text);
    return old;
  }
  return stringNode(text);
}

export function setMount(doc: YDoc, svc: YAMLMap, index: number, m: MountSpec): void {
  const seq = ownCollection(doc, svc, 'volumes');
  if (!seq || !isSeq(seq)) {
    const fresh = new YAMLSeq(doc.schema);
    fresh.items.push(mountNode(doc, m, null));
    setKey(doc, svc, 'volumes', fresh, SERVICE_ORDER);
    return;
  }
  if (index < 0 || index >= seq.items.length) seq.items.push(mountNode(doc, m, null));
  else seq.items[index] = mountNode(doc, m, deref(doc, seq.items[index]));
}

// ------------------------------------------------------------- depends_on

export type DependCondition = 'service_started' | 'service_healthy' | 'service_completed_successfully';

export interface Dependency {
  service: string;
  condition: DependCondition;
  restart: boolean;
  required: boolean;
}

export function getDependencies(doc: YDoc, svc: YAMLMap): Dependency[] {
  const v = get(doc, svc, 'depends_on');
  if (Array.isArray(v)) return v.map((s) => ({ service: String(s), condition: 'service_started', restart: false, required: true }));
  if (v && typeof v === 'object') {
    return Object.entries(v as Record<string, Record<string, unknown> | null>).map(([service, o]) => ({
      service,
      condition: (o?.condition as DependCondition) ?? 'service_started',
      restart: o?.restart === true,
      required: o?.required !== false,
    }));
  }
  return [];
}

/** Writes depends_on, in the short list form when nothing needs the long one. */
export function setDependencies(doc: YDoc, svc: YAMLMap, deps: Dependency[]): void {
  if (!deps.length) {
    setKey(doc, svc, 'depends_on', undefined);
    return;
  }
  const simple = deps.every((d) => d.condition === 'service_started' && !d.restart && d.required);
  const cur = deref(doc, pairOf(svc, 'depends_on')?.value);
  if (simple && !isMap(cur)) {
    setList(doc, svc, 'depends_on', deps.map((d) => d.service));
    return;
  }
  const map = isMap(cur) ? cur : new YAMLMap(doc.schema);
  for (const p of [...(map.items as Pair[])]) {
    const k = scalarText(doc, p.key);
    if (k !== null && !deps.some((d) => d.service === k)) map.items = (map.items as Pair[]).filter((x) => x !== p);
  }
  for (const d of deps) {
    const opts = ensureMap(doc, map, d.service);
    setKey(doc, opts, 'condition', d.condition, ['condition', 'restart', 'required']);
    setKey(doc, opts, 'restart', d.restart ? true : undefined, ['condition', 'restart', 'required']);
    setKey(doc, opts, 'required', d.required ? undefined : false, ['condition', 'restart', 'required']);
  }
  if (!isMap(cur)) setKey(doc, svc, 'depends_on', map, SERVICE_ORDER);
}

// ------------------------------------------------------------ healthcheck

export interface Healthcheck {
  /** 'shell' (CMD-SHELL / a string), 'exec' (CMD + arguments), 'none' (disabled). */
  kind: 'shell' | 'exec' | 'none';
  command: string;
  args: string[];
  interval: string;
  timeout: string;
  retries: string;
  startPeriod: string;
  startInterval: string;
}

export function getHealthcheck(doc: YDoc, svc: YAMLMap): Healthcheck | null {
  const v = get(doc, svc, 'healthcheck') as Record<string, unknown> | undefined;
  if (!v || typeof v !== 'object') return null;
  const str = (k: string) => (v[k] === undefined || v[k] === null ? '' : String(v[k]));
  const base = { interval: str('interval'), timeout: str('timeout'), retries: str('retries'), startPeriod: str('start_period'), startInterval: str('start_interval') };
  if (v.disable === true) return { kind: 'none', command: '', args: [], ...base };
  const test = v.test;
  if (typeof test === 'string') return { kind: 'shell', command: test, args: [], ...base };
  if (Array.isArray(test)) {
    const [head, ...rest] = test.map(String);
    if (head === 'NONE') return { kind: 'none', command: '', args: [], ...base };
    if (head === 'CMD-SHELL') return { kind: 'shell', command: rest.join(' '), args: [], ...base };
    if (head === 'CMD') return { kind: 'exec', command: '', args: rest, ...base };
  }
  return { kind: 'shell', command: '', args: [], ...base };
}

export function setHealthcheck(doc: YDoc, svc: YAMLMap, h: Healthcheck | null): void {
  if (!h) {
    setKey(doc, svc, 'healthcheck', undefined);
    return;
  }
  const map = ensureMap(doc, svc, 'healthcheck', SERVICE_ORDER);
  const order = ['test', 'disable', 'interval', 'timeout', 'retries', 'start_period', 'start_interval'];
  const testNode = deref(doc, pairOf(map, 'test')?.value);
  if (h.kind === 'none') {
    setKey(doc, map, 'test', undefined);
    setKey(doc, map, 'disable', true, order);
  } else {
    setKey(doc, map, 'disable', undefined);
    if (h.kind === 'shell') {
      // Keep the list form if the file uses it.
      if (isSeq(testNode)) {
        const seq = new YAMLSeq(doc.schema);
        seq.flow = testNode.flow;
        seq.items.push(stringNode('CMD-SHELL', true), stringNode(h.command, true));
        setKey(doc, map, 'test', seq, order);
      } else setKey(doc, map, 'test', h.command, order);
    } else {
      const seq = new YAMLSeq(doc.schema);
      seq.flow = isSeq(testNode) ? testNode.flow : true;
      for (const a of ['CMD', ...h.args]) seq.items.push(stringNode(a, true));
      setKey(doc, map, 'test', seq, order);
    }
  }
  const put = (k: string, v: string) => setKey(doc, map, k, v.trim() ? (/^\d+$/.test(v.trim()) && k === 'retries' ? Number(v) : v.trim()) : undefined, order);
  put('interval', h.interval);
  put('timeout', h.timeout);
  put('retries', h.retries);
  put('start_period', h.startPeriod);
  put('start_interval', h.startInterval);
}

// --------------------------------------------------------- command lines

/** A command or entrypoint as written: a string (shell form) or a list. */
export function getCommand(doc: YDoc, svc: YAMLMap, key: 'command' | 'entrypoint'): { form: 'string' | 'list' | null; value: string | string[] } {
  const v = get(doc, svc, key);
  if (Array.isArray(v)) return { form: 'list', value: v.map(String) };
  if (v === undefined || v === null || v === '') return { form: null, value: '' };
  return { form: 'string', value: String(v) };
}

// --------------------------------------------------------------- resources

/** Reads a resource limit from `deploy.resources.limits` or the short key. */
export function getLimit(doc: YDoc, svc: YAMLMap, kind: 'memory' | 'cpus'): { value: string; where: 'deploy' | 'short' | null } {
  const deploy = get(doc, svc, 'deploy') as { resources?: { limits?: Record<string, unknown> } } | undefined;
  const fromDeploy = deploy?.resources?.limits?.[kind];
  if (fromDeploy !== undefined && fromDeploy !== null) return { value: String(fromDeploy), where: 'deploy' };
  const short = get(doc, svc, kind === 'memory' ? 'mem_limit' : 'cpus');
  if (short !== undefined && short !== null && short !== '') return { value: String(short), where: 'short' };
  return { value: '', where: null };
}

/** Writes a limit where the file already keeps it (the short key for new ones). */
export function setLimit(doc: YDoc, svc: YAMLMap, kind: 'memory' | 'cpus', value: string): void {
  const cur = getLimit(doc, svc, kind);
  const v = value.trim();
  if (cur.where === 'deploy') {
    const deploy = ensureMap(doc, svc, 'deploy', SERVICE_ORDER);
    const res = ensureMap(doc, deploy, 'resources');
    const limits = ensureMap(doc, res, 'limits');
    setKey(doc, limits, kind, v || undefined);
    if (!limits.items.length) setKey(doc, res, 'limits', undefined);
    if (!res.items.length) setKey(doc, deploy, 'resources', undefined);
    if (!deploy.items.length) setKey(doc, svc, 'deploy', undefined);
    return;
  }
  const key = kind === 'memory' ? 'mem_limit' : 'cpus';
  setKey(doc, svc, key, v ? (kind === 'cpus' && /^\d+(\.\d+)?$/.test(v) ? Number(v) : v) : undefined, SERVICE_ORDER);
}

/** NVIDIA GPUs requested through `deploy.resources.reservations.devices` (or `gpus`). */
export function getGpus(doc: YDoc, svc: YAMLMap): string {
  const direct = get(doc, svc, 'gpus');
  if (direct !== undefined && direct !== null) return Array.isArray(direct) ? 'custom' : String(direct);
  const deploy = get(doc, svc, 'deploy') as { resources?: { reservations?: { devices?: Record<string, unknown>[] } } } | undefined;
  const dev = deploy?.resources?.reservations?.devices?.find((d) => d.driver === 'nvidia' || (Array.isArray(d.capabilities) && d.capabilities.includes('gpu')));
  if (!dev) return '';
  if (dev.count !== undefined) return String(dev.count);
  if (Array.isArray(dev.device_ids)) return 'custom';
  return 'all';
}

export function setGpus(doc: YDoc, svc: YAMLMap, value: string): void {
  if (pairOf(svc, 'gpus')) {
    setKey(doc, svc, 'gpus', value ? (/^\d+$/.test(value) ? Number(value) : value) : undefined, SERVICE_ORDER);
    return;
  }
  const deployNode = deref(doc, pairOf(svc, 'deploy')?.value);
  const devicesPath = () => {
    const deploy = ensureMap(doc, svc, 'deploy', SERVICE_ORDER);
    const res = ensureMap(doc, deploy, 'resources');
    const resv = ensureMap(doc, res, 'reservations');
    return { deploy, res, resv };
  };
  if (!value) {
    if (!isMap(deployNode)) return;
    const { deploy, res, resv } = devicesPath();
    const devs = deref(doc, pairOf(resv, 'devices')?.value);
    if (isSeq(devs)) {
      devs.items = devs.items.filter((d) => {
        const o = toJs(doc, d) as Record<string, unknown> | null;
        return !(o && (o.driver === 'nvidia' || (Array.isArray(o.capabilities) && o.capabilities.includes('gpu'))));
      });
      if (!devs.items.length) setKey(doc, resv, 'devices', undefined);
    }
    if (!resv.items.length) setKey(doc, res, 'reservations', undefined);
    if (!res.items.length) setKey(doc, deploy, 'resources', undefined);
    if (!deploy.items.length) setKey(doc, svc, 'deploy', undefined);
    return;
  }
  const { resv } = devicesPath();
  let devs = deref(doc, pairOf(resv, 'devices')?.value);
  if (!isSeq(devs)) {
    devs = new YAMLSeq(doc.schema);
    setKey(doc, resv, 'devices', devs);
  }
  const seq = devs as YAMLSeq;
  const count = value === 'all' ? 'all' : Number(value);
  const existing = seq.items.find((d) => {
    const o = toJs(doc, d) as Record<string, unknown> | null;
    return o && (o.driver === 'nvidia' || (Array.isArray(o.capabilities) && o.capabilities.includes('gpu')));
  });
  if (isMap(existing)) {
    setKey(doc, existing, 'count', count, ['driver', 'count', 'capabilities']);
    return;
  }
  const entry = createNode(doc, { driver: 'nvidia', count, capabilities: ['gpu'] }) as YAMLMap;
  const caps = deref(doc, pairOf(entry, 'capabilities')?.value);
  if (isSeq(caps)) caps.flow = true;
  seq.items.push(entry);
}

// ----------------------------------------------------------- top level

/** Declares a top-level named volume or network if the file doesn't have it yet. */
export function declareTopLevel(doc: YDoc, kind: 'volumes' | 'networks', name: string, external = false): void {
  if (!isMap(doc.contents)) return;
  const top = ensureMap(doc, doc.contents as unknown as YAMLMap, kind, TOP_ORDER);
  if (pairOf(top, name)) return;
  top.items.push(doc.createPair(name, external ? createNode(doc, { external: true }) : null));
}

/** Names declared under top-level `volumes:` / `networks:`. */
export function topLevelNames(doc: YDoc, kind: 'volumes' | 'networks' | 'secrets' | 'configs'): string[] {
  if (!isMap(doc.contents)) return [];
  const v = deref(doc, pairOf(doc.contents as unknown as YAMLMap, kind)?.value);
  return isMap(v) ? keysOf(v) : [];
}

/** Service networks as a list of names (from either form). */
export function getNetworks(doc: YDoc, svc: YAMLMap): { name: string; aliases: string[]; ipv4: string }[] {
  const v = get(doc, svc, 'networks');
  if (Array.isArray(v)) return v.map((n) => ({ name: String(n), aliases: [], ipv4: '' }));
  if (v && typeof v === 'object') {
    return Object.entries(v as Record<string, Record<string, unknown> | null>).map(([name, o]) => ({
      name,
      aliases: Array.isArray(o?.aliases) ? o.aliases.map(String) : [],
      ipv4: o?.ipv4_address ? String(o.ipv4_address) : '',
    }));
  }
  return [];
}

export function setNetworks(doc: YDoc, svc: YAMLMap, nets: { name: string; aliases: string[]; ipv4: string }[]): void {
  if (!nets.length) {
    setKey(doc, svc, 'networks', undefined);
    return;
  }
  const cur = deref(doc, pairOf(svc, 'networks')?.value);
  const simple = nets.every((n) => !n.aliases.length && !n.ipv4);
  if (simple && !isMap(cur)) {
    setList(doc, svc, 'networks', nets.map((n) => n.name));
    return;
  }
  const map = isMap(cur) ? cur : new YAMLMap(doc.schema);
  map.items = (map.items as Pair[]).filter((p) => nets.some((n) => n.name === scalarText(doc, p.key)));
  for (const n of nets) {
    let p = pairOf(map, n.name);
    if (!p) {
      map.items.push(doc.createPair(n.name, null));
      p = pairOf(map, n.name)!;
    }
    if (!n.aliases.length && !n.ipv4) {
      if (isMap(p.value)) {
        setKey(doc, p.value, 'aliases', undefined);
        setKey(doc, p.value, 'ipv4_address', undefined);
        if (!p.value.items.length) p.value = null;
      }
      continue;
    }
    const opts = isMap(p.value) ? p.value : (p.value = new YAMLMap(doc.schema));
    setKey(doc, opts, 'aliases', n.aliases.length ? n.aliases : undefined, ['aliases', 'ipv4_address']);
    setKey(doc, opts, 'ipv4_address', n.ipv4 || undefined, ['aliases', 'ipv4_address']);
  }
  if (!isMap(cur)) setKey(doc, svc, 'networks', map, SERVICE_ORDER);
}

// ------------------------------------------------------------- whole service

/** Adds a service from plain data (template, converted run command...). */
export function addService(doc: YDoc, name: string, body: Record<string, unknown>): void {
  const services = ensureServices(doc);
  if (pairOf(services, name)) throw new Error(`A service named ${name} already exists`);
  const ordered: Record<string, unknown> = {};
  for (const k of SERVICE_ORDER) if (k in body) ordered[k] = body[k];
  for (const [k, v] of Object.entries(body)) if (!(k in ordered)) ordered[k] = v;
  const node = createNode(doc, ordered) as YAMLMap;
  // Ports read best quoted; healthcheck tests and capability lists inline.
  const ports = deref(doc, pairOf(node, 'ports')?.value);
  if (isSeq(ports)) for (const it of ports.items) if (isScalar(it) && typeof it.value === 'string') it.type = 'QUOTE_DOUBLE';
  const hc = deref(doc, pairOf(node, 'healthcheck')?.value);
  const test = isMap(hc) ? deref(doc, pairOf(hc, 'test')?.value) : undefined;
  if (isSeq(test)) {
    test.flow = true;
    for (const it of test.items) if (isScalar(it)) it.type = 'QUOTE_DOUBLE';
  }
  const pair = doc.createPair(name, node);
  services.items.push(pair);
  // Services read best with a blank line between them, unless the file packs them.
  matchSpacing(services, pair, true);
}

export function removeService(doc: YDoc, name: string): void {
  const services = servicesMap(doc);
  if (!services) return;
  services.items = (services.items as Pair[]).filter((p) => scalarText(doc, p.key) !== name);
  // Others can't depend on a service that's gone.
  for (const other of keysOf(services)) {
    const svc = serviceMap(doc, other);
    if (!svc) continue;
    const deps = getDependencies(doc, svc);
    if (deps.some((d) => d.service === name)) setDependencies(doc, svc, deps.filter((d) => d.service !== name));
  }
}

/** Renames a service and every depends_on / network_mode reference to it. */
export function renameService(doc: YDoc, from: string, to: string): void {
  const services = servicesMap(doc);
  if (!services || from === to) return;
  if (pairOf(services, to)) throw new Error(`A service named ${to} already exists`);
  const p = pairOf(services, from);
  if (!p) return;
  if (isScalar(p.key)) p.key.value = to;
  for (const other of keysOf(services)) {
    const svc = serviceMap(doc, other);
    if (!svc) continue;
    const deps = getDependencies(doc, svc);
    if (deps.some((d) => d.service === from)) {
      const dn = deref(doc, pairOf(svc, 'depends_on')?.value);
      if (isMap(dn)) {
        const dp = pairOf(dn, from);
        if (dp && isScalar(dp.key)) dp.key.value = to;
      } else setDependencies(doc, svc, deps.map((d) => (d.service === from ? { ...d, service: to } : d)));
    }
    const nm = pairOf(svc, 'network_mode');
    if (nm && isScalar(nm.value) && nm.value.value === `service:${from}`) nm.value.value = `service:${to}`;
  }
}

/** Copies a service under a new name (container_name left out, it must be unique). */
export function duplicateService(doc: YDoc, from: string, to: string): void {
  const svc = serviceMap(doc, from);
  if (!svc) return;
  const body = svc.toJS(doc, { maxAliasCount: -1 }) as Record<string, unknown>;
  delete body.container_name;
  addService(doc, to, body);
}

/** Keys of a service in the file (merge keys left out). */
export function ownKeys(svc: YAMLMap): string[] {
  return keysOf(svc);
}

/** Service settings the forms edit. */
export const FORM_KEYS = new Set([
  'image',
  'build',
  'container_name',
  'restart',
  'command',
  'depends_on',
  'environment',
  'env_file',
  'ports',
  'expose',
  'volumes',
  'tmpfs',
  'network_mode',
  'networks',
  'hostname',
  'dns',
  'extra_hosts',
  'healthcheck',
  'mem_limit',
  'cpus',
  'pids_limit',
  'gpus',
  'devices',
  'group_add',
  'shm_size',
  'deploy',
  'user',
  'working_dir',
  'entrypoint',
  'init',
  'tty',
  'stdin_open',
  'privileged',
  'read_only',
  'cap_add',
  'cap_drop',
  'security_opt',
  'sysctls',
  'labels',
  'logging',
  'stop_grace_period',
  'stop_signal',
  'pull_policy',
  'platform',
  'profiles',
]);

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

/** True when a form-handled setting uses parts the form can't show. */
function beyondForm(key: string, v: unknown): boolean {
  switch (key) {
    case 'build':
      return isObject(v) && Object.keys(v).some((k) => !['context', 'dockerfile', 'target', 'args'].includes(k));
    case 'deploy': {
      if (!isObject(v)) return false;
      if (Object.keys(v).some((k) => k !== 'resources')) return true;
      const r = v.resources;
      if (!isObject(r)) return false;
      if (Object.keys(r).some((k) => k !== 'limits' && k !== 'reservations')) return true;
      if (isObject(r.limits) && Object.keys(r.limits).some((k) => k !== 'memory' && k !== 'cpus')) return true;
      if (isObject(r.reservations)) {
        const res = r.reservations;
        if (Object.keys(res).some((k) => k !== 'devices')) return true;
        return Array.isArray(res.devices) && res.devices.some((d) => !isObject(d) || d.driver !== 'nvidia' || 'device_ids' in d);
      }
      return false;
    }
    case 'devices':
    case 'gpus':
      return Array.isArray(v) && v.some((d) => d !== null && typeof d === 'object');
    case 'extra_hosts':
      return isObject(v);
    case 'logging':
      return isObject(v) && Object.keys(v).some((k) => k !== 'driver' && k !== 'options');
    case 'networks':
      return isObject(v) && Object.values(v).some((o) => isObject(o) && Object.keys(o).some((k) => k !== 'aliases' && k !== 'ipv4_address'));
    case 'healthcheck':
      return isObject(v) && Object.keys(v).some((k) => !['test', 'disable', 'interval', 'timeout', 'retries', 'start_period', 'start_interval'].includes(k));
    default:
      return false;
  }
}

/** Settings shown in the raw YAML editors: unknown ones and those beyond the forms. */
export function rawKeys(doc: YDoc, svc: YAMLMap): string[] {
  return keysOf(svc).filter((k) => !FORM_KEYS.has(k) || beyondForm(k, get(doc, svc, k)));
}
