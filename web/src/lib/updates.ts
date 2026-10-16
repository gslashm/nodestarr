// Update types: what kind of update each available update is (a major,
// minor or patch version, a rebuild, with security fixes or breaking
// changes). The server works the type out (src/updates/classify.rs); this
// module names and orders the types for the Updates page, the container's
// Updates tab, settings and confirmations, and `autoPlan` mirrors the
// automatic-update decision (src/updates/auto.rs) so each update can say
// whether it installs on its own.

import { hasUpdate } from './format';
import type { AutoInstall, Bump, Change, ContainerView, TagScope, UpdateInfo } from './types';

export type UpdateType = 'breaking' | 'security' | Bump;

export interface TypeInfo {
  key: UpdateType;
  label: string;
  /** For sentences: "major versions", "rebuilds". */
  plural: string;
  hint: string;
  tone: 'bad' | 'warn' | 'update' | '';
}

/** Every type, in the order that needs a look first. */
export const UPDATE_TYPES: TypeInfo[] = [
  { key: 'breaking', label: 'Breaking', plural: 'breaking changes', hint: 'The release notes mention breaking changes or steps to take', tone: 'bad' },
  { key: 'security', label: 'Security', plural: 'security fixes', hint: 'The release notes mention security fixes', tone: 'warn' },
  { key: 'major', label: 'Major', plural: 'major versions', hint: 'A new major version, like 1.x to 2.0', tone: 'update' },
  { key: 'minor', label: 'Minor', plural: 'minor versions', hint: 'A new minor version, like 1.2 to 1.3', tone: '' },
  { key: 'patch', label: 'Patch', plural: 'patch versions', hint: 'A new patch version, like 1.2.3 to 1.2.4', tone: '' },
  { key: 'rebuild', label: 'Rebuild', plural: 'rebuilds', hint: 'The same version in a new image, usually base image or packaging fixes', tone: '' },
  { key: 'unknown', label: 'Unknown', plural: 'updates of unknown size', hint: "The image doesn't say which version it is, so the size of the change is unknown", tone: '' },
];

export const TYPE = Object.fromEntries(UPDATE_TYPES.map((t) => [t.key, t])) as Record<UpdateType, TypeInfo>;

const UNKNOWN: Change = {
  bump: 'unknown',
  estimated: false,
  from_version: null,
  to_version: null,
  prerelease: false,
  security: false,
  breaking: false,
  notes: null,
};

/** The offered update's type, or null without an update. Updates found
 * before types existed count as unknown until the next check. */
export function changeOf(u: UpdateInfo | null | undefined): Change | null {
  if (!u || !hasUpdate(u)) return null;
  return u.change ?? (u.newer_tag ? { ...UNKNOWN, bump: scopeBump(u.newer_tag.scope) } : UNKNOWN);
}

function scopeBump(s: TagScope): Bump {
  return s === 'digest' ? 'rebuild' : s;
}

/** Every type an update counts as: its size, plus breaking and security. */
export function typesOf(ch: Change | null): UpdateType[] {
  if (!ch) return [];
  const out: UpdateType[] = [];
  if (ch.breaking) out.push('breaking');
  if (ch.security) out.push('security');
  out.push(ch.bump);
  return out;
}

/** Sort key: its most significant type first (breaking, then security, then size). */
export function typeRank(ch: Change | null): number {
  if (!ch) return UPDATE_TYPES.length;
  return UPDATE_TYPES.findIndex((t) => t.key === typesOf(ch)[0]);
}

/** Counts per type (an update counts once for each type it has). */
export function countTypes(changes: (Change | null)[]): Record<UpdateType, number> {
  const counts = Object.fromEntries(UPDATE_TYPES.map((t) => [t.key, 0])) as Record<UpdateType, number>;
  for (const ch of changes) for (const t of typesOf(ch)) counts[t]++;
  return counts;
}

/** What the size of the change means for this update, in a sentence. */
export function describeChange(ch: Change, tag: string): string {
  const span = ch.from_version && ch.to_version ? `: ${ch.from_version} → ${ch.to_version}` : '';
  if (ch.estimated) {
    return `At most a ${ch.bump === 'rebuild' ? 'rebuild' : `${ch.bump} version`}: the image doesn't say which version it is, and new builds of the tag ${tag} stay within it.`;
  }
  switch (ch.bump) {
    case 'rebuild':
      return ch.to_version ? `The same version (${ch.to_version}) in a new image.` : 'The same version in a new image.';
    case 'unknown':
      return "The image doesn't say which version it is, so Nodestarr can't tell how big the change is.";
    default:
      return `A ${ch.bump} version${span}.`;
  }
}

const SIZES: Bump[] = ['rebuild', 'patch', 'minor', 'major', 'unknown'];

/** The rules in a sentence: "Rebuilds, patch and minor versions, ...". */
export function rulesSummary(r: AutoInstall): string {
  const versions = (['patch', 'minor', 'major'] as const).filter((k) => r[k]);
  const parts: string[] = [];
  if (r.rebuild) parts.push('rebuilds');
  if (versions.length) parts.push(`${list(versions)} versions`);
  if (r.unknown) parts.push('updates of unknown size');
  const allSizes = SIZES.every((k) => r[k]);
  if (r.security && !allSizes) parts.push(parts.length ? 'other security fixes' : 'security fixes');
  if (!parts.length) return 'Nothing installs on its own.';
  let s = `${capitalize(list(parts))}`;
  s += r.breaking ? ', breaking changes included' : ', unless the release notes mention breaking changes';
  s += r.switch_tags ? '; switches to newer tags.' : '; tags stay as they are.';
  return s;
}

function list(items: readonly string[]): string {
  return items.length <= 1 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** Why `rules` don't let `ch` install on its own, or null when they do. */
export function blockedBy(rules: AutoInstall, ch: Change, switchesTag: boolean): string | null {
  if (switchesTag && !rules.switch_tags) return "It changes the tag, and automatic updates don't switch tags";
  if (ch.breaking && !rules.breaking) return 'Its release notes mention breaking changes';
  if (rules[ch.bump] || (ch.security && rules.security && ch.bump !== 'major')) return null;
  return `${capitalize(TYPE[ch.bump].plural)} aren't installed automatically`;
}

export interface AutoPlan {
  /** It installs at an automatic update run (once `after` has passed). */
  install: boolean;
  /** Why it doesn't. */
  reason: string;
  /** Unix seconds when it's old enough to install. */
  after: number | null;
  /** The tag it switches to, when the update is a newer tag. */
  tag: string | null;
}

/**
 * What automatic updates will do with a container's update, or null when
 * the container isn't set to update automatically or has no update.
 */
export function autoPlan(c: ContainerView, now = Date.now() / 1000): AutoPlan | null {
  const u = c.update;
  if (c.policy !== 'auto' || !u || !hasUpdate(u)) return null;
  const no = (reason: string): AutoPlan => ({ install: false, reason, after: null, tag: null });
  if (c.prefs.held) return no('On hold');
  const rules = c.auto_install;
  let blocked: string | null = null;
  if (u.newer_tag) {
    blocked = blockedBy(rules, changeOf(u)!, true);
    if (!blocked) return waitFor(u.newer_tag.first_seen, c.min_age_hours, now, u.newer_tag.tag);
  }
  const newBuild = (!!u.current_digest && !!u.latest_digest && u.current_digest !== u.latest_digest) || u.status === 'pending_recreate';
  if (!newBuild) return no(blocked ?? 'There is no new build of its tag');
  const build = (u.newer_tag ? u.build_change : u.change) ?? UNKNOWN;
  const why = blockedBy(rules, build, false);
  if (why) return no(blocked ?? why);
  if (c.prefs.skipped_digest && u.latest_digest === c.prefs.skipped_digest) return no('This build was skipped');
  return waitFor(u.first_seen, c.min_age_hours, now, null);
}

function waitFor(firstSeen: number | null, hours: number, now: number, tag: string | null): AutoPlan {
  const after = hours > 0 ? (firstSeen ?? now) + hours * 3600 : 0;
  return { install: true, reason: '', after: after > now ? after : null, tag };
}
