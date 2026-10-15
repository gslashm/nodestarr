import assert from 'node:assert/strict';
import { test } from 'node:test';

import { formatSize, parseSize, shellJoin, shellSplit } from '../src/lib/spec.ts';

test('shellSplit handles quotes and escapes', () => {
  assert.deepEqual(shellSplit('nginx -g "daemon off;"'), ['nginx', '-g', 'daemon off;']);
  assert.deepEqual(shellSplit(`sh -c 'echo "hi there"'`), ['sh', '-c', 'echo "hi there"']);
  assert.deepEqual(shellSplit('a\\ b c'), ['a b', 'c']);
  assert.deepEqual(shellSplit('"say \\"hi\\""'), ['say "hi"']);
  assert.deepEqual(shellSplit(`''`), ['']);
  assert.deepEqual(shellSplit('  spaced   out  '), ['spaced', 'out']);
  assert.deepEqual(shellSplit(''), []);
});

test('shellJoin round-trips through shellSplit', () => {
  const cases = [['nginx', '-g', 'daemon off;'], ['sh', '-c', `echo 'quoted' "and" $HOME`], [''], ['a=b', '--flag=1,2', '/path/to/x.conf']];
  for (const args of cases) assert.deepEqual(shellSplit(shellJoin(args)), args);
  assert.equal(shellJoin(['plain', 'two words']), `plain 'two words'`);
});

test('parseSize reads Docker-style sizes', () => {
  assert.equal(parseSize(''), null);
  assert.equal(parseSize('  '), null);
  assert.equal(parseSize('1024'), 1024);
  assert.equal(parseSize('512m'), 512 * 1024 ** 2);
  assert.equal(parseSize('2g'), 2 * 1024 ** 3);
  assert.equal(parseSize('1.5G'), 1.5 * 1024 ** 3);
  assert.equal(parseSize('256MiB'), 256 * 1024 ** 2);
  assert.equal(parseSize('64kb'), 64 * 1024);
  assert.equal(parseSize('lots'), 'invalid');
  assert.equal(parseSize('-1g'), 'invalid');
});

test('formatSize writes sizes parseSize reads back', () => {
  assert.equal(formatSize(null), '');
  assert.equal(formatSize(0), '');
  assert.equal(formatSize(512 * 1024 ** 2), '512m');
  assert.equal(formatSize(2 * 1024 ** 3), '2g');
  assert.equal(formatSize(1536 * 1024 ** 2), '1536m');
  assert.equal(formatSize(1000), '1000');
  for (const s of ['512m', '2g', '64k', '3t']) assert.equal(formatSize(parseSize(s) as number), s);
});
