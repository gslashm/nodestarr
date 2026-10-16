// Things in a stack compose would refuse or that would surprise you, found
// while editing (before `docker compose config` runs on save).

import type { VariableUse } from './environment';
import { get, getDependencies, getMounts, getNetworks, getPorts, serviceMap, servicesMap, topLevelNames } from './service';
import { keysOf, yamlError, type YDoc } from './yamldoc';

export type Fix =
  | { kind: 'declare'; what: 'volumes' | 'networks'; name: string; file: string }
  | { kind: 'variable'; name: string };

export interface Problem {
  level: 'error' | 'warn';
  message: string;
  service?: string;
  section?: string;
  fix?: Fix;
}

export function stackProblems(docs: { file: string; doc: YDoc }[], project: Map<string, string>, usage: Map<string, VariableUse>): Problem[] {
  const out: Problem[] = [];
  for (const { file, doc } of docs) {
    const e = yamlError(doc);
    if (e) out.push({ level: 'error', message: `${file}: ${e}` });
  }
  if (out.length) return out;

  const main = docs[0]?.file ?? 'compose.yaml';
  const declared = (kind: 'volumes' | 'networks') => new Set(docs.flatMap(({ doc }) => topLevelNames(doc, kind)));
  const volumes = declared('volumes');
  const networks = declared('networks');
  const names = new Set(docs.flatMap(({ doc }) => (servicesMap(doc) ? keysOf(servicesMap(doc)!) : [])));
  const published = new Map<string, string>();

  for (const name of names) {
    const own = docs.map(({ doc }) => ({ doc, svc: serviceMap(doc, name) })).filter((x) => x.svc);
    const hasImage = own.some(({ doc, svc }) => get(doc, svc!, 'image') !== undefined || get(doc, svc!, 'build') !== undefined || get(doc, svc!, 'extends') !== undefined);
    if (!hasImage) out.push({ level: 'error', message: `${name} has neither an image nor a Dockerfile to build.`, service: name, section: 'general' });
    for (const { doc, svc } of own) {
      for (const m of getMounts(doc, svc!)) {
        if (m.type === 'volume' && m.source && !m.source.includes('$') && !volumes.has(m.source)) {
          out.push({ level: 'error', message: `${name} uses the volume ${m.source}, which isn't declared.`, service: name, section: 'storage', fix: { kind: 'declare', what: 'volumes', name: m.source, file: main } });
          volumes.add(m.source);
        }
      }
      const mode = String(get(doc, svc!, 'network_mode') ?? '');
      if (mode.startsWith('service:') && !names.has(mode.slice(8))) {
        out.push({ level: 'error', message: `${name} shares the network of ${mode.slice(8)}, which isn't in the stack.`, service: name, section: 'network' });
      }
      for (const n of getNetworks(doc, svc!)) {
        if (n.name !== 'default' && !networks.has(n.name)) {
          out.push({ level: 'error', message: `${name} joins the network ${n.name}, which isn't declared.`, service: name, section: 'network', fix: { kind: 'declare', what: 'networks', name: n.name, file: main } });
          networks.add(n.name);
        }
      }
      for (const d of getDependencies(doc, svc!)) {
        if (!names.has(d.service)) out.push({ level: 'error', message: `${name} starts after ${d.service}, which isn't in the stack.`, service: name, section: 'general' });
      }
      for (const p of getPorts(doc, svc!)) {
        if (!p.published || p.published.includes('$')) continue;
        const key = `${p.hostIp || '*'}:${p.published}/${p.protocol || 'tcp'}`;
        const other = published.get(key);
        if (other && other !== name) out.push({ level: 'warn', message: `${name} and ${other} both publish port ${p.published}; only one can start.`, service: name, section: 'ports' });
        else published.set(key, name);
      }
    }
  }

  for (const [v, use] of usage) {
    if (project.has(v)) continue;
    if (use.required) out.push({ level: 'error', message: `${v} must be set (${use.services.filter(Boolean).join(', ') || 'the stack'} requires it).`, fix: { kind: 'variable', name: v } });
    else if (use.bare) out.push({ level: 'warn', message: `${v} isn't set, so ${use.services.filter(Boolean).join(', ') || 'the stack'} won't get a value for it.`, fix: { kind: 'variable', name: v } });
  }
  return out;
}
