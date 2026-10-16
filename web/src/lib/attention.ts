// What about a container needs attention, and whether the user put it away.
// Shared by the dashboard's "Needs attention" panel, the Containers page's
// filter and the sidebar badge, so all three agree.

import { api, seg } from './api';
import { hasUpdate, stateOf } from './format';
import type { ContainerView } from './types';

export type AttentionKind = 'crash' | 'health' | 'web' | 'app' | 'held' | 'update';

/** Fingerprint value that hides every occurrence of a kind. */
export const ALWAYS = '*';

export interface Concern {
  kind: AttentionKind;
  tone: 'bad' | 'warn' | 'update';
  /** Identifies this occurrence: a dismissal lapses when it changes. */
  fingerprint: string;
  hidden: boolean;
}

/** Every concern a container has right now, hidden ones included. */
export function concerns(c: ContainerView): Concern[] {
  const out: Concern[] = [];
  const add = (kind: AttentionKind, tone: Concern['tone'], fingerprint: string) => {
    const d = c.prefs.dismissed?.[kind];
    out.push({ kind, tone, fingerprint, hidden: d === ALWAYS || d === fingerprint });
  };
  const st = stateOf(c);
  if (st === 'crashed' || st === 'dead') add('crash', 'bad', `${c.id}:${c.finished_at ?? ''}`);
  else if (st === 'unhealthy' || st === 'restarting') add('health', 'bad', `${st}:${c.id}`);
  else if (c.uptime?.state === 'down') add('web', 'bad', String(c.uptime.since));
  const issues = c.app_status?.health.filter((h) => h.level !== 'notice') ?? [];
  if (issues.length) add('app', issues.some((h) => h.level === 'error') ? 'bad' : 'warn', issues.map((h) => h.message).join('\n'));
  if (c.prefs.held) add('held', 'warn', c.update?.latest_digest ?? '');
  if (hasUpdate(c.update) && c.policy !== 'ignore') add('update', 'update', c.update?.latest_digest ?? c.update?.latest_version ?? '');
  return out;
}

/** Whether the container has a problem (not just an update or a note) the user hasn't put away. */
export function hasProblem(c: ContainerView): boolean {
  return concerns(c).some((x) => !x.hidden && x.tone === 'bad');
}

/**
 * Hides a concern: this occurrence only, or with `always` every future one
 * of its kind for this container. Pass `null` to show it again.
 */
export async function dismiss(c: ContainerView, kind: AttentionKind, fingerprint: string | null): Promise<void> {
  const r = await api.put<{ dismissed: Record<string, string> }>(`/containers/${seg(c.id)}/dismiss`, { kind, fingerprint });
  c.prefs.dismissed = r.dismissed;
}
