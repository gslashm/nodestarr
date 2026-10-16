// The environment a container ends up with, worked out the way Compose does
// it, with the origin of every variable so the editor can show it as one
// list and write each change back to the right place.
//
// Lowest to highest priority: the image's own ENV, `env_file:` files in
// order, then `environment:` (later compose files override earlier ones).
// `${VAR}` in compose values comes from the stack's variables (.env).

import { parseDotenv, evaluateDotenv, type Quote } from './dotenv';
import { defaultOf, interpolate, pureReference, references } from './interpolate';
import { get, readKeyValues, serviceMap, servicesMap } from './service';
import { isPair, isScalar, keysOf, lookup, pairOf, type YDoc } from './yamldoc';
import { visit } from 'yaml';

export interface EnvFileRef {
  path: string;
  required: boolean;
}

export function envFilesOf(doc: YDoc, service: string): EnvFileRef[] {
  const svc = serviceMap(doc, service);
  if (!svc) return [];
  const v = get(doc, svc, 'env_file');
  const list = Array.isArray(v) ? v : v === undefined || v === null || v === '' ? [] : [v];
  return list
    .map((x) =>
      x && typeof x === 'object'
        ? { path: String((x as Record<string, unknown>).path ?? ''), required: (x as Record<string, unknown>).required !== false }
        : { path: String(x), required: true },
    )
    .filter((r) => r.path);
}

export type EnvOrigin =
  | { kind: 'compose'; file: string; inherited: boolean }
  | { kind: 'env_file'; file: string; line: number; quote: Quote }
  | { kind: 'image' };

export interface EnvDef {
  origin: EnvOrigin;
  /** As written; null for a bare `KEY` (its value comes from the stack's variables). */
  raw: string | null;
  /** What the container gets; null when it ends up unset. */
  value: string | null;
  /** Variables it uses that aren't set (and have no default). */
  missing: string[];
  /** `${VAR:?message}` failures, which stop compose. */
  errors: string[];
  /** Malformed `${...}` expressions. */
  issues: string[];
}

export interface EnvRow {
  key: string;
  /** The definition that wins. */
  def: EnvDef;
  /** Definitions it overrides, nearest first. */
  shadowed: EnvDef[];
  /** The stack variable that holds the value (`${TZ}`, or a bare `TZ`). */
  via: string | null;
  /** The reference's own fallback (`${TZ:-UTC}` → "UTC"). */
  viaDefault: string | null;
  secret: boolean;
}

const SECRET = /PASS|SECRET|TOKEN|KEY|CREDENTIAL|PRIVATE|CLAIM|SALT|AUTH/i;

export function looksSecret(key: string): boolean {
  return SECRET.test(key);
}

export interface EnvInputs {
  /** Compose files in order (overrides last). */
  composes: { file: string; doc: YDoc }[];
  service: string;
  /** The stack's variables (.env), already evaluated. */
  project: Map<string, string>;
  /** Contents of env files by the path written in compose; null when missing. */
  envFiles: Record<string, string | null | undefined>;
  /** ENV of the image, when known. */
  image?: { key: string; value: string }[] | null;
}

export interface ServiceEnv {
  rows: EnvRow[];
  envFiles: EnvFileRef[];
  /** env files that are referenced but couldn't be read. */
  unreadable: string[];
}

export function serviceEnv(inp: EnvInputs): ServiceEnv {
  // Definitions per key, lowest priority first.
  const defs = new Map<string, EnvDef[]>();
  const add = (key: string, def: EnvDef) => {
    if (!defs.has(key)) defs.set(key, []);
    defs.get(key)!.push(def);
  };
  const look = (n: string) => inp.project.get(n);

  const composeDefs: [string, EnvDef][] = [];
  const fileRefs: EnvFileRef[] = [];
  for (const { file, doc } of inp.composes) {
    const svc = serviceMap(doc, inp.service);
    if (!svc) continue;
    for (const r of envFilesOf(doc, inp.service)) if (!fileRefs.some((x) => x.path === r.path)) fileRefs.push(r);
    const found = lookup(doc, svc, 'environment');
    for (const e of readKeyValues(doc, found.node).entries) {
      const ev = e.value === null ? null : interpolate(e.value, look);
      composeDefs.push([
        e.key,
        {
          origin: { kind: 'compose', file, inherited: found.inherited },
          raw: e.value,
          value: e.value === null ? (inp.project.get(e.key) ?? null) : ev!.value,
          missing: ev?.missing ?? [],
          errors: ev?.errors ?? [],
          issues: ev?.issues ?? [],
        },
      ]);
    }
  }

  for (const kv of inp.image ?? []) {
    add(kv.key, { origin: { kind: 'image' }, raw: kv.value, value: kv.value, missing: [], errors: [], issues: [] });
  }
  const unreadable: string[] = [];
  for (const ref of fileRefs) {
    const text = inp.envFiles[ref.path];
    if (text === null || text === undefined) {
      if (text === null && ref.required) unreadable.push(ref.path);
      continue;
    }
    const values = evaluateDotenv(text, look);
    for (const e of parseDotenv(text)) {
      add(e.key, {
        origin: { kind: 'env_file', file: ref.path, line: e.line, quote: e.quote },
        raw: e.value,
        value: values.get(e.key) ?? null,
        missing: e.value !== null && e.quote !== "'" ? interpolate(e.value, (n) => values.get(n) ?? look(n)).missing : [],
        errors: [],
        issues: [],
      });
    }
  }
  // Compose wins over the files and the image, so it goes last per key.
  for (const [k, d] of composeDefs) add(k, d);
  // Display order: keys set in compose (as written), then env files, then the image.
  const order: string[] = [];
  for (const [k] of composeDefs) if (!order.includes(k)) order.push(k);
  for (const k of defs.keys()) if (!order.includes(k)) order.push(k);
  const fromImage = (k: string) => defs.get(k)!.every((d) => d.origin.kind === 'image');
  order.sort((a, b) => Number(fromImage(a)) - Number(fromImage(b)));

  const rows: EnvRow[] = order.map((key) => {
    const all = defs.get(key)!;
    const def = all[all.length - 1];
    let via: string | null = null;
    let viaDefault: string | null = null;
    if (def.origin.kind === 'compose') {
      if (def.raw === null) via = key;
      else {
        const ref = pureReference(def.raw);
        if (ref && (ref.op === '' || ref.op === ':-' || ref.op === '-' || ref.op === ':?' || ref.op === '?')) {
          via = ref.name;
          viaDefault = defaultOf(ref);
        }
      }
    }
    return { key, def, shadowed: all.slice(0, -1).reverse(), via, viaDefault, secret: looksSecret(key) };
  });
  return { rows, envFiles: fileRefs, unreadable };
}

export interface VariableUse {
  /** Services that use it (`''` for settings outside services). */
  services: string[];
  /** Defaults given where it's used. */
  defaults: string[];
  /** Used somewhere without a default. */
  bare: boolean;
  /** Used as `${VAR:?...}`. */
  required: boolean;
}

/** Which stack variables the compose files use, and where. */
export function variableUsage(composes: { file: string; doc: YDoc }[]): Map<string, VariableUse> {
  const out = new Map<string, VariableUse>();
  for (const { doc } of composes) {
    const services = servicesMap(doc);
    visit(doc, {
      Scalar(key, node, path) {
        if (key === 'key' || typeof node.value !== 'string' || !node.value.includes('$')) return;
        let service = '';
        if (services) {
          const at = path.indexOf(services as never);
          const pair = path[at + 1];
          if (at >= 0 && isPair(pair) && isScalar(pair.key)) service = String(pair.key.value);
        }
        for (const ref of references(node.value)) {
          let u = out.get(ref.name);
          if (!u) out.set(ref.name, (u = { services: [], defaults: [], bare: false, required: false }));
          if (!u.services.includes(service)) u.services.push(service);
          const d = defaultOf(ref);
          if (d !== null) {
            if (!u.defaults.includes(d)) u.defaults.push(d);
          } else if (ref.op === ':?' || ref.op === '?') u.required = true;
          else if (ref.op === '') u.bare = true;
        }
      },
    });
    // A bare `KEY` in environment takes the stack variable of the same name.
    for (const name of services ? keysOf(services) : []) {
      const svc = serviceMap(doc, name);
      if (!svc) continue;
      const env = lookup(doc, svc, 'environment').node;
      for (const e of readKeyValues(doc, env).entries) {
        if (e.value !== null) continue;
        let u = out.get(e.key);
        if (!u) out.set(e.key, (u = { services: [], defaults: [], bare: false, required: false }));
        if (!u.services.includes(name)) u.services.push(name);
      }
    }
  }
  return out;
}

/**
 * Renames a stack variable everywhere a compose document uses it: `${OLD}`,
 * `${OLD:-x}` and `$OLD` become the new name, and a bare `OLD` entry in an
 * environment list (which took the variable of the same name) now points
 * at it explicitly. Returns how many places changed.
 */
export function renameVariable(doc: YDoc, from: string, to: string): number {
  let changed = 0;
  visit(doc, {
    Scalar(key, node) {
      if (key === 'key' || typeof node.value !== 'string' || !node.value.includes('$')) return;
      const s = node.value;
      const refs = references(s)
        .filter((r) => r.name === from)
        .sort((a, b) => b.start - a.start);
      if (!refs.length) return;
      let out = s;
      for (const r of refs) {
        const at = out.indexOf(from, r.start);
        out = out.slice(0, at) + to + out.slice(at + from.length);
      }
      node.value = out;
      changed += refs.length;
    },
  });
  const services = servicesMap(doc);
  for (const name of services ? keysOf(services) : []) {
    const svc = serviceMap(doc, name);
    const env = svc ? pairOf(svc, 'environment')?.value : undefined;
    if (!env || !readKeyValues(doc, env as never).entries.some((e) => e.key === from && e.value === null)) continue;
    if (isScalar(env)) continue;
    const coll = env as { items: unknown[] };
    for (const it of coll.items) {
      if (isPair(it) && isScalar(it.key) && String(it.key.value) === from && (it.value === null || (isScalar(it.value) && it.value.value === null))) {
        it.value = doc.createNode(`\${${to}}`);
        changed++;
      } else if (isScalar(it) && it.value === from) {
        it.value = `${from}=\${${to}}`;
        changed++;
      }
    }
  }
  return changed;
}

/** Services in the files (first appearance order across files). */
export function allServices(composes: { doc: YDoc }[]): string[] {
  const out: string[] = [];
  for (const { doc } of composes) {
    const s = servicesMap(doc);
    for (const k of s ? keysOf(s) : []) if (!out.includes(k)) out.push(k);
  }
  return out;
}

/** True when the service sets `key` in this file's `environment:` itself. */
export function hasOwnEnv(doc: YDoc, service: string, key: string): boolean {
  const svc = serviceMap(doc, service);
  const env = svc ? pairOf(svc, 'environment')?.value : undefined;
  if (!env) return false;
  return readKeyValues(doc, env as never).entries.some((e) => e.key === key);
}
