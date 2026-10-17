import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { AutoInstall, Change, ContainerView } from '../src/lib/types.ts';
import { autoPlan, blockedBy, changeOf, countTypes, rulesSummary, typeRank, typesOf } from '../src/lib/updates.ts';

const DEFAULTS: AutoInstall = { rebuild: true, patch: true, minor: true, major: false, unknown: true, security: true, breaking: false, switch_tags: false };
const NOW = 1_000_000;

function change(over: Partial<Change> = {}): Change {
  return { bump: 'minor', estimated: false, from_version: null, to_version: null, prerelease: false, security: false, breaking: false, notes: null, ...over };
}

function container(update: Record<string, unknown> | null, over: Record<string, unknown> = {}): ContainerView {
  return {
    policy: 'auto',
    auto_install: DEFAULTS,
    min_age_hours: 0,
    prefs: { held: false, skipped_digest: '', rolled_back_digest: '', dismissed: {} },
    update: update && { status: 'available', current_digest: 'a', latest_digest: 'b', first_seen: NOW - 10 * 3600, newer_tag: null, change: null, build_change: null, ...update },
    ...over,
  } as unknown as ContainerView;
}

test('an update counts as its size plus its release-notes signals', () => {
  assert.deepEqual(typesOf(change({ security: true, breaking: true })), ['breaking', 'security', 'minor']);
  assert.deepEqual(typesOf(null), []);
  const counts = countTypes([change({ security: true }), change({ bump: 'patch' }), null]);
  assert.equal(counts.minor, 1);
  assert.equal(counts.security, 1);
  assert.equal(counts.patch, 1);
  assert.equal(counts.major, 0);
});

test('sorting puts what needs a look first', () => {
  const order = [change({ bump: 'rebuild' }), change({ bump: 'major' }), change({ security: true, bump: 'patch' }), change({ breaking: true, bump: 'patch' })]
    .sort((a, b) => typeRank(a) - typeRank(b))
    .map((c) => typesOf(c)[0]);
  assert.deepEqual(order, ['breaking', 'security', 'major', 'rebuild']);
});

test('updates found before types existed still get one', () => {
  assert.equal(changeOf({ status: 'available', change: null, newer_tag: null } as never)?.bump, 'unknown');
  assert.equal(changeOf({ status: 'available', change: null, newer_tag: { tag: '2', scope: 'major' } } as never)?.bump, 'major');
  assert.equal(changeOf({ status: 'up_to_date', change: change() } as never), null);
  assert.equal(changeOf({ status: 'skipped', change: change() } as never), null);
});

test('the rules match the server', () => {
  assert.equal(blockedBy(DEFAULTS, change(), false), null);
  assert.match(blockedBy(DEFAULTS, change({ bump: 'major' }), false) ?? '', /^Major versions/);
  assert.match(blockedBy(DEFAULTS, change({ breaking: true }), false) ?? '', /breaking/);
  assert.match(blockedBy(DEFAULTS, change({ bump: 'patch' }), true) ?? '', /tag/);
  const strict = { ...DEFAULTS, minor: false };
  assert.equal(blockedBy(strict, change({ security: true }), false), null);
  assert.notEqual(blockedBy(strict, change({ bump: 'major', security: true }), false), null);
  assert.match(blockedBy(strict, change({ bump: 'patch', security: true, breaking: true }), false) ?? '', /breaking/);
});

test('the plan says what automatic updates will do', () => {
  assert.equal(autoPlan(container({ change: change() }, { policy: 'notify' }), NOW), null);
  assert.equal(autoPlan(container(null), NOW), null);
  assert.deepEqual(autoPlan(container({ change: change() }), NOW), { install: true, reason: '', after: null, tag: null });
  assert.equal(autoPlan(container({ change: change() }, { prefs: { held: true, skipped_digest: '', rolled_back_digest: '' } }), NOW)?.reason, 'On hold');
  assert.match(autoPlan(container({ change: change({ bump: 'major' }) }), NOW)?.reason ?? '', /Major/);
  assert.match(autoPlan(container({ change: change() }, { prefs: { held: false, skipped_digest: '', rolled_back_digest: 'b' } }), NOW)?.reason ?? '', /rolled back/);
  // A build that was skipped is no update at all.
  assert.equal(autoPlan(container({ status: 'skipped', change: change() }, { prefs: { held: false, skipped_digest: 'b', rolled_back_digest: '' } }), NOW), null);
  // Too new: installs once it's old enough.
  const young = autoPlan(container({ change: change(), first_seen: NOW - 3600 }, { min_age_hours: 6 }), NOW);
  assert.equal(young?.install, true);
  assert.equal(young?.after, NOW + 5 * 3600);
});

test('a newer tag needs tag switching, else the new build of the tag installs', () => {
  const newer = { newer_tag: { tag: '9.0.0', scope: 'major', first_seen: NOW }, change: change({ bump: 'major' }), build_change: change({ bump: 'rebuild' }) };
  // Not allowed to switch tags: falls back to the rebuild.
  assert.deepEqual(autoPlan(container(newer), NOW), { install: true, reason: '', after: null, tag: null });
  const tags = { ...DEFAULTS, switch_tags: true, major: true };
  assert.equal(autoPlan(container(newer, { auto_install: tags }), NOW)?.tag, '9.0.0');
  // No new build of the tag: the reason is the tag's.
  const onlyTag = { ...newer, latest_digest: 'a', build_change: null };
  assert.match(autoPlan(container(onlyTag), NOW)?.reason ?? '', /tag/);
  // The new build was skipped: it isn't installed instead, so the newer tag waits, for its own reason.
  const skippedBuild = autoPlan(container(newer, { prefs: { held: false, skipped_digest: 'b', rolled_back_digest: '' } }), NOW);
  assert.equal(skippedBuild?.install, false);
  assert.match(skippedBuild?.reason ?? '', /tag/);
});

test('rules read as a sentence', () => {
  assert.equal(
    rulesSummary(DEFAULTS),
    'Rebuilds, patch and minor versions, updates of unknown size and other security fixes, unless the release notes mention breaking changes; tags stay as they are.',
  );
  const none = { rebuild: false, patch: false, minor: false, major: false, unknown: false, security: false, breaking: false, switch_tags: false };
  assert.equal(rulesSummary(none), 'Nothing installs on its own.');
  assert.match(rulesSummary({ ...none, security: true }), /^Security fixes/);
});
