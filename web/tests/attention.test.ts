import assert from 'node:assert/strict';
import { test } from 'node:test';

import { ALWAYS, concerns, hasProblem } from '../src/lib/attention.ts';
import type { ContainerView } from '../src/lib/types.ts';

function container(over: Record<string, unknown> = {}, dismissed: Record<string, string> = {}): ContainerView {
  return {
    id: 'abc',
    state: 'exited',
    health: null,
    exit_code: 1,
    oom_killed: false,
    finished_at: 1000,
    policy: 'notify',
    update: null,
    uptime: null,
    app_status: null,
    prefs: { held: false, dismissed },
    ...over,
  } as unknown as ContainerView;
}

test('a crash is a problem until dismissed', () => {
  const c = container();
  const [crash] = concerns(c);
  assert.equal(crash.kind, 'crash');
  assert.equal(crash.hidden, false);
  assert.ok(hasProblem(c));

  const dismissed = container({}, { crash: crash.fingerprint });
  assert.equal(concerns(dismissed)[0].hidden, true);
  assert.ok(!hasProblem(dismissed));
});

test('a dismissal lapses when the situation changes', () => {
  const first = concerns(container())[0].fingerprint;
  const again = container({ finished_at: 2000 }, { crash: first });
  assert.equal(concerns(again)[0].hidden, false);
});

test('ignoring a kind hides every occurrence of it, and only it', () => {
  const update = { status: 'available', latest_digest: 'sha256:new' };
  const c = container({ finished_at: 5000, update }, { crash: ALWAYS });
  const byKind = Object.fromEntries(concerns(c).map((x) => [x.kind, x.hidden]));
  assert.deepEqual(byKind, { crash: true, update: false });
});

test('updates on ignored policy are not concerns', () => {
  const c = container({ state: 'running', policy: 'ignore', update: { status: 'available', latest_digest: 'x' } });
  assert.deepEqual(concerns(c), []);
});
