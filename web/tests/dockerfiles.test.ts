import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  insertInstruction,
  instructions,
  lintDockerfile,
  moveInstruction,
  parseCopy,
  parseDockerfile,
  parseExec,
  parseFrom,
  parseHealth,
  parsePairs,
  removeInstruction,
  renderCopy,
  renderExec,
  renderFrom,
  renderHealth,
  renderInstruction,
  renderPairs,
  serializeDockerfile,
  setInstruction,
  stages,
} from '../src/lib/compose/dockerfiles.ts';

const DF = `# syntax=docker/dockerfile:1
# build stage
FROM node:22-alpine AS build
WORKDIR /app
COPY package*.json ./
RUN npm ci && \\
    # the comment is skipped
    npm run build

FROM nginx:latest
COPY --from=build /app/dist /usr/share/nginx/html
RUN <<EOF
echo hi
EOF
CMD nginx -g "daemon off;"
`;

test('parsing is lossless', () => {
  const df = parseDockerfile(DF);
  assert.equal(serializeDockerfile(df), DF);
  assert.equal(serializeDockerfile(parseDockerfile('FROM a')), 'FROM a');
  const kinds = df.items.map((i) => (i.kind === 'instruction' ? i.keyword : i.kind));
  assert.deepEqual(kinds, ['directive', 'comment', 'FROM', 'WORKDIR', 'COPY', 'RUN', 'blank', 'FROM', 'COPY', 'RUN', 'CMD']);
  const run = instructions(df)[3];
  assert.equal(run.args, 'npm ci &&\nnpm run build');
  assert.equal(run.line, 6);
  const heredoc = instructions(df)[6];
  assert.equal(heredoc.heredoc, true);
  assert.equal(heredoc.args, '<<EOF\necho hi\nEOF');
});

test('escape directive switches the continuation character', () => {
  const df = parseDockerfile('# escape=`\nFROM windows\nRUN dir `\n  c:\\\n');
  assert.equal(df.escape, '`');
  assert.equal(instructions(df)[1].args, 'dir\nc:\\');
});

test('edits change only the edited instruction', () => {
  const df = parseDockerfile(DF);
  const idx = df.items.findIndex((i) => i.kind === 'instruction' && i.keyword === 'CMD');
  const out = serializeDockerfile(setInstruction(df, idx, 'CMD', renderExec(['nginx', '-g', 'daemon off;'])));
  assert.equal(out, DF.replace('CMD nginx -g "daemon off;"', 'CMD ["nginx", "-g", "daemon off;"]'));
  assert.equal(renderInstruction('RUN', 'apt-get update &&\napt-get install -y curl'), 'RUN apt-get update && \\\n    apt-get install -y curl\n');
});

test('instructions move and go with their comments', () => {
  const df = parseDockerfile('FROM a\n# about b\nRUN b\nRUN c\n');
  const b = 2;
  const c = 3;
  assert.equal(serializeDockerfile(moveInstruction(df, c, b)), 'FROM a\nRUN c\n# about b\nRUN b\n');
  assert.equal(serializeDockerfile(moveInstruction(df, b, c)), 'FROM a\nRUN c\n# about b\nRUN b\n');
  assert.equal(serializeDockerfile(removeInstruction(df, b)), 'FROM a\nRUN c\n');
  assert.equal(serializeDockerfile(insertInstruction(parseDockerfile('FROM a'), 1, 'USER', 'app')), 'FROM a\nUSER app\n');
});

test('argument forms', () => {
  assert.deepEqual(parseFrom('--platform=$BUILDPLATFORM golang:1.23 AS build'), { platform: '$BUILDPLATFORM', image: 'golang:1.23', alias: 'build' });
  assert.equal(renderFrom({ platform: '', image: 'alpine:3.20', alias: '' }), 'alpine:3.20');
  assert.deepEqual(parseCopy('--from=build --chown=1000:1000 /app/dist "/srv/my site"'), {
    flags: [
      { name: 'from', value: 'build' },
      { name: 'chown', value: '1000:1000' },
    ],
    sources: ['/app/dist'],
    dest: '/srv/my site',
  });
  assert.equal(renderCopy(parseCopy('--link a b ./c/')), '--link a b ./c/');
  assert.deepEqual(parseCopy('["a b", "/c"]'), { flags: [], sources: ['a b'], dest: '/c' });
  assert.deepEqual(parsePairs('ENV', 'A=1 B="two words"'), [
    { key: 'A', value: '1' },
    { key: 'B', value: 'two words' },
  ]);
  assert.deepEqual(parsePairs('ENV', 'LEGACY some value'), [{ key: 'LEGACY', value: 'some value' }]);
  assert.equal(renderPairs('ENV', [{ key: 'A', value: '1' }, { key: 'B', value: 'two words' }]), 'A=1\nB="two words"');
  assert.equal(renderPairs('ARG', [{ key: 'VERSION', value: '' }]), 'VERSION');
  assert.deepEqual(parseExec('["a", "b"]'), ['a', 'b']);
  assert.equal(parseExec('a b'), null);
  const h = parseHealth('--interval=30s --retries=3 CMD curl -f http://localhost/ || exit 1');
  assert.equal(h.interval, '30s');
  assert.equal(h.retries, '3');
  assert.equal(h.shell, 'curl -f http://localhost/ || exit 1');
  assert.equal(renderHealth(h), '--interval=30s --retries=3 CMD curl -f http://localhost/ || exit 1');
  assert.equal(parseHealth('NONE').none, true);
});

test('stages group instructions under their FROM', () => {
  const s = stages(parseDockerfile(DF));
  assert.deepEqual(
    s.map((x) => [x.name, x.image, x.members.length]),
    [
      ['build', 'node:22-alpine', 4],
      ['', 'nginx:latest', 4],
    ],
  );
});

test('hints point at common mistakes', () => {
  const hints = lintDockerfile(parseDockerfile('FROM ubuntu\nRUN apt-get install -y curl\nADD app.py /app/\nCMD python app.py\n'));
  const text = hints.map((h) => `${h.line}:${h.level}`);
  assert.ok(text.includes('1:tip'), 'untagged base image');
  assert.ok(hints.some((h) => h.line === 2 && h.message.includes('apt-get update')));
  assert.ok(hints.some((h) => h.line === 3 && h.message.startsWith('COPY')));
  assert.ok(hints.some((h) => h.line === 4 && h.message.includes('exec form')));
  assert.ok(hints.some((h) => h.message.includes('runs as root')));
  assert.equal(lintDockerfile(parseDockerfile(DF)).filter((h) => h.message.includes('latest')).length, 1);
});
