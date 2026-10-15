import assert from 'node:assert/strict';
import { test } from 'node:test';

import { diffLines, diffRows, hunks, parseUnified } from '../src/lib/diff.ts';

const ops = (before: string, after: string) => diffLines(before, after).map((l) => `${l.op}${l.text}`);

test('identical texts have no changes', () => {
  assert.deepEqual(ops('a\nb\n', 'a\nb\n'), [' a', ' b']);
  assert.deepEqual(hunks(diffLines('a\nb', 'a\nb')), []);
  assert.deepEqual(diffRows(diffLines('same', 'same')), []);
});

test('additions, removals and replacements', () => {
  assert.deepEqual(ops('', 'x\ny\n'), ['+x', '+y']);
  assert.deepEqual(ops('x\ny\n', ''), ['-x', '-y']);
  assert.deepEqual(ops('a\nb\nc\n', 'a\nB\nc\n'), [' a', '-b', '+B', ' c']);
  assert.deepEqual(ops('a\nc\n', 'a\nb\nc\n'), [' a', '+b', ' c']);
});

test('line endings and a missing final newline do not count as changes', () => {
  assert.deepEqual(ops('a\r\nb\r\n', 'a\nb'), [' a', ' b']);
});

test('hunks keep the requested context and merge when close', () => {
  const before = 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\n';
  const after = 'a\nb\nX\nd\ne\nf\ng\nh\ni\nj\nk\nY\nm\nn\n';
  const hs = hunks(diffLines(before, after), 2);
  assert.equal(hs.length, 2);
  assert.equal(hs[0].oldStart, 1);
  assert.equal(hs[0].newStart, 1);
  assert.equal(hs[1].oldStart, 10);
  assert.equal(hs[1].newStart, 10);
  // With more context the two changes touch and become one hunk.
  assert.equal(hunks(diffLines(before, after), 5).length, 1);
});

test('rows carry old and new line numbers', () => {
  const rows = diffRows(diffLines('a\nb\nc\n', 'a\nB\nc\nd\n'), 1);
  assert.deepEqual(rows, [
    { kind: 'hunk', text: 'Line 1' },
    { kind: 'ctx', text: 'a', old: 1, new: 1 },
    { kind: 'del', text: 'b', old: 2 },
    { kind: 'add', text: 'B', new: 2 },
    { kind: 'ctx', text: 'c', old: 3, new: 3 },
    { kind: 'add', text: 'd', new: 4 },
  ]);
});

test('large rewrites fall back to replacing everything', () => {
  const before = Array.from({ length: 2000 }, (_, i) => `old ${i}`).join('\n');
  const after = Array.from({ length: 2000 }, (_, i) => `new ${i}`).join('\n');
  const lines = diffLines(before, after);
  assert.equal(lines.filter((l) => l.op === '-').length, 2000);
  assert.equal(lines.filter((l) => l.op === '+').length, 2000);
});

test('unified diffs: headers dropped, hunks numbered', () => {
  const unified = [
    '--- a/docker-compose.yml',
    '+++ b/docker-compose.yml',
    '@@ -1,3 +1,4 @@ services:',
    '+# note',
    ' services:',
    '-  web:',
    '+  app:',
    '   image: nginx',
    '\\ No newline at end of file',
    '',
  ].join('\n');
  assert.deepEqual(parseUnified(unified), [
    { kind: 'hunk', text: 'Line 1  services:' },
    { kind: 'add', text: '# note', new: 1 },
    { kind: 'ctx', text: 'services:', old: 1, new: 2 },
    { kind: 'del', text: '  web:', old: 2 },
    { kind: 'add', text: '  app:', new: 3 },
    { kind: 'ctx', text: '  image: nginx', old: 3, new: 4 },
  ]);
});

test('unified diffs: a removed line that looks like a header stays a change', () => {
  const rows = parseUnified('--- a/x\n+++ b/x\n@@ -1,2 +1 @@\n--- old rule\n keep\n');
  assert.deepEqual(rows.slice(1), [
    { kind: 'del', text: '-- old rule', old: 1 },
    { kind: 'ctx', text: 'keep', old: 2, new: 1 },
  ]);
});

test('unified diffs: several files in one text', () => {
  const rows = parseUnified('--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n--- a/y\n+++ b/y\n@@ -5 +5 @@\n-c\n+d\n');
  assert.deepEqual(
    rows.map((r) => `${r.kind}:${r.text}`),
    ['hunk:Line 1', 'del:a', 'add:b', 'hunk:Line 5', 'del:c', 'add:d'],
  );
});
