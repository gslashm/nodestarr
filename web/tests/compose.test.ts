import assert from 'node:assert/strict';
import { test } from 'node:test';

import { deleteDotenv, dotenvIssues, evaluateDotenv, parseDotenv, renameDotenv, renderValue, setDotenv } from '../src/lib/compose/dotenv.ts';
import { serviceEnv, variableUsage } from '../src/lib/compose/environment.ts';
import { escapeDollars, escapeReferences, hasStrayDollar, interpolate, pureReference, references } from '../src/lib/compose/interpolate.ts';
import { merge3 } from '../src/lib/compose/merge3.ts';
import {
  addService,
  declareTopLevel,
  deleteKeyValue,
  editService,
  formatMount,
  formatPort,
  getDependencies,
  getHealthcheck,
  getMounts,
  getPorts,
  parseMount,
  parsePort,
  removeItem,
  removeService,
  renameService,
  set,
  setDependencies,
  setHealthcheck,
  setKeyValue,
  setLimit,
  setMount,
  setPort,
} from '../src/lib/compose/service.ts';
import { detectStyle, editYaml, parsePairText, parseYaml, pairText } from '../src/lib/compose/yamldoc.ts';

const MEDIA = `# media stack
name: media

x-common: &common
  restart: unless-stopped
  environment: &env
    PUID: 1000
    TZ: Europe/Berlin   # local time

services:
  plex:
    <<: *common
    image: lscr.io/linuxserver/plex:latest   # keep latest
    network_mode: host


  sonarr:
    image: "lscr.io/linuxserver/sonarr:develop"
    environment:
      - TZ=Europe/Berlin
      - PASSWORD=\${SONARR_PASS}
    ports:
    - "8989:8989"
    volumes:
      - ./config:/config
      - type: bind
        source: /mnt/tv
        target: /tv
        read_only: true
    command: >-
      --some long folded command
      that continues
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8989/ping"]
      interval: 30s
`;

function changedLines(a: string, b: string): string[] {
  const x = a.split('\n');
  const y = b.split('\n');
  const out: string[] = [];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if (x[i] !== y[i]) out.push(`${i + 1}: ${JSON.stringify(x[i])} -> ${JSON.stringify(y[i])}`);
  return out;
}

test('merge3 takes their changes and our layout', () => {
  const base = ['a', 'b', 'c', 'd'];
  const ours = ['a', 'b  ', 'c', 'd'];
  const theirs = ['a', 'b', 'c', 'X', 'd'];
  assert.deepEqual(merge3(base, ours, theirs), { lines: ['a', 'b  ', 'c', 'X', 'd'], conflicts: 0 });
  // Both changed the same line: theirs wins and it counts as a conflict.
  assert.deepEqual(merge3(['a', 'b'], ['a', 'B'], ['a', 'c']), { lines: ['a', 'c'], conflicts: 1 });
  assert.deepEqual(merge3([], [], ['x']).lines, ['x']);
  assert.deepEqual(merge3(['x'], ['x'], []).lines, []);
});

test('detectStyle reads indentation conventions', () => {
  assert.deepEqual(detectStyle('a:\n  b:\n    - x\n'), { indent: 2, indentSeq: true, flowPadding: false });
  assert.equal(detectStyle('a:\n    b: 1\n').indent, 4);
  assert.equal(detectStyle('a:\n  ports:\n  - x\n  - y\n').indentSeq, false);
  assert.equal(detectStyle('t: [ "a", "b" ]\n').flowPadding, true);
});

test('editing one value leaves the rest of the file alone', () => {
  const out = editYaml(MEDIA, (doc) => {
    const svc = editService(doc, 'plex')!;
    set(doc, svc, 'image', 'lscr.io/linuxserver/plex:1.41.0');
  });
  assert.deepEqual(changedLines(MEDIA, out), [
    `13: "    image: lscr.io/linuxserver/plex:latest   # keep latest" -> "    image: lscr.io/linuxserver/plex:1.41.0   # keep latest"`,
  ]);
});

test('no-op edits return the file unchanged', () => {
  assert.equal(
    editYaml(MEDIA, () => {}),
    MEDIA,
  );
  const crlf = MEDIA.replace(/\n/g, '\r\n');
  assert.equal(
    editYaml(crlf, () => {}),
    crlf,
  );
});

test('adding a port follows the flush sequence style of the file', () => {
  const out = editYaml(MEDIA, (doc) => {
    const svc = editService(doc, 'sonarr')!;
    setPort(doc, svc, -1, { hostIp: '127.0.0.1', published: '9000', target: '9000', protocol: 'udp', long: false });
  });
  assert.deepEqual(changedLines(MEDIA, out).slice(0, 1), [`24: "    volumes:" -> "    - \\"127.0.0.1:9000:9000/udp\\""`]);
  const ports = getPorts(parseYaml(out), editService(parseYaml(out), 'sonarr')!);
  assert.equal(ports.length, 2);
  assert.equal(formatPort(ports[1]), '127.0.0.1:9000:9000/udp');
  // Folded scalars, comments and blank lines elsewhere survive.
  assert.ok(out.includes('    command: >-\n      --some long folded command\n      that continues\n'));
  assert.ok(out.includes('    network_mode: host\n\n\n  sonarr:'));
});

test('environment entries keep the list form and quoting', () => {
  const out = editYaml(MEDIA, (doc) => {
    const svc = editService(doc, 'sonarr')!;
    setKeyValue(doc, svc, 'environment', 'TZ', 'America/New_York');
    setKeyValue(doc, svc, 'environment', 'PUID', '1000');
    deleteKeyValue(doc, svc, 'environment', 'PASSWORD');
  });
  assert.ok(out.includes('      - TZ=America/New_York\n      - PUID=1000\n    ports:'), out);
});

test('inherited environment is copied before changing it', () => {
  const out = editYaml(MEDIA, (doc) => {
    const svc = editService(doc, 'plex')!;
    setKeyValue(doc, svc, 'environment', 'PUID', '1001');
  });
  const data = parseYaml(out).toJS() as { services: { plex: { environment: Record<string, unknown> } }; 'x-common': { environment: Record<string, unknown> } };
  assert.deepEqual(data.services.plex.environment, { PUID: 1001, TZ: 'Europe/Berlin' });
  assert.equal(data['x-common'].environment.PUID, 1000, 'the shared anchor is untouched');
});

test('numbers stay numbers when edited', () => {
  const out = editYaml('services:\n  a:\n    environment:\n      PUID: 1000\n', (doc) => {
    setKeyValue(doc, editService(doc, 'a')!, 'environment', 'PUID', '1001');
  });
  assert.equal(out, 'services:\n  a:\n    environment:\n      PUID: 1001\n');
});

test('new keys go in the usual place', () => {
  const out = editYaml('services:\n  a:\n    image: nginx\n    ports:\n      - "80:80"\n', (doc) => {
    set(doc, editService(doc, 'a')!, 'restart', 'unless-stopped');
  });
  assert.equal(out, 'services:\n  a:\n    image: nginx\n    restart: unless-stopped\n    ports:\n      - "80:80"\n');
});

test('ports in short and long syntax', () => {
  assert.deepEqual(parsePort('8080:80'), { hostIp: '', published: '8080', target: '80', protocol: '', long: false });
  assert.deepEqual(parsePort('127.0.0.1::5000/udp'), { hostIp: '127.0.0.1', published: '', target: '5000', protocol: 'udp', long: false });
  assert.deepEqual(parsePort('[::1]:8080:80'), { hostIp: '::1', published: '8080', target: '80', protocol: '', long: false });
  assert.deepEqual(parsePort('3000'), { hostIp: '', published: '', target: '3000', protocol: '', long: false });
  for (const s of ['8080:80', '127.0.0.1:8080:80/udp', '[::1]:8080:80', '6000-6005:6000-6005', '3000', '127.0.0.1::5000']) {
    assert.equal(formatPort(parsePort(s)!), s);
  }
  const doc = parseYaml('services:\n  a:\n    ports:\n      - target: 80\n        published: "8080"\n        mode: host\n');
  const [p] = getPorts(doc, editService(doc, 'a')!);
  assert.equal(p.long, true);
  assert.equal(p.published, '8080');
  assert.equal(p.mode, 'host');
});

test('long-syntax ports stay long when edited', () => {
  const out = editYaml('services:\n  a:\n    ports:\n      - target: 80\n        published: "8080"\n        mode: host\n', (doc) => {
    const svc = editService(doc, 'a')!;
    const [p] = getPorts(doc, svc);
    setPort(doc, svc, 0, { ...p, published: '9090' });
  });
  assert.equal(out, 'services:\n  a:\n    ports:\n      - target: 80\n        published: "9090"\n        mode: host\n');
});

test('volumes in short and long syntax', () => {
  assert.deepEqual(parseMount('./config:/config'), { type: 'bind', source: './config', target: '/config', readOnly: false, options: [], long: false });
  assert.deepEqual(parseMount('data:/var/lib/data:ro,z'), { type: 'volume', source: 'data', target: '/var/lib/data', readOnly: true, options: ['z'], long: false });
  assert.deepEqual(parseMount('/cache'), { type: 'volume', source: '', target: '/cache', readOnly: false, options: [], long: false });
  assert.equal(parseMount('${MEDIA}/tv:/tv').type, 'bind');
  for (const s of ['./config:/config', 'data:/data:ro', '/mnt/x:/x:ro,z', '/cache']) assert.equal(formatMount(parseMount(s)), s);
  const doc = parseYaml(MEDIA);
  const mounts = getMounts(doc, editService(doc, 'sonarr')!);
  assert.equal(mounts.length, 2);
  assert.deepEqual(mounts[1], { type: 'bind', source: '/mnt/tv', target: '/tv', readOnly: true, options: [], long: true });
  const out = editYaml(MEDIA, (d) => {
    const svc = editService(d, 'sonarr')!;
    setMount(d, svc, 0, { ...mounts[0], readOnly: true });
    removeItem(d, svc, 'volumes', 1);
  });
  assert.ok(out.includes('    volumes:\n      - ./config:/config:ro\n    command'), out);
});

test('depends_on switches to the long form only when needed', () => {
  let text = 'services:\n  app:\n    image: x\n    depends_on:\n      - db\n  db:\n    image: postgres\n';
  text = editYaml(text, (d) => {
    const svc = editService(d, 'app')!;
    setDependencies(d, svc, [...getDependencies(d, svc), { service: 'cache', condition: 'service_started', restart: false, required: true }]);
  });
  assert.ok(text.includes('    depends_on:\n      - db\n      - cache\n'), text);
  text = editYaml(text, (d) => {
    const svc = editService(d, 'app')!;
    setDependencies(d, svc, getDependencies(d, svc).map((x) => (x.service === 'db' ? { ...x, condition: 'service_healthy' } : x)));
  });
  const data = parseYaml(text).toJS() as { services: { app: { depends_on: unknown } } };
  assert.deepEqual(data.services.app.depends_on, { db: { condition: 'service_healthy' }, cache: { condition: 'service_started' } });
});

test('healthchecks read and write every form', () => {
  const doc = parseYaml(MEDIA);
  const h = getHealthcheck(doc, editService(doc, 'sonarr')!)!;
  assert.equal(h.kind, 'exec');
  assert.deepEqual(h.args, ['curl', '-f', 'http://localhost:8989/ping']);
  const out = editYaml(MEDIA, (d) => {
    setHealthcheck(d, editService(d, 'sonarr')!, { ...h, args: ['curl', '-fs', 'http://localhost:8989/ping'], timeout: '5s' });
  });
  assert.ok(out.includes('      test: ["CMD", "curl", "-fs", "http://localhost:8989/ping"]\n      interval: 30s\n      timeout: 5s\n'), out);
});

test('limits go where the file keeps them', () => {
  const short = editYaml('services:\n  a:\n    image: x\n', (d) => setLimit(d, editService(d, 'a')!, 'memory', '512m'));
  assert.ok(short.includes('mem_limit: 512m'));
  const deploy = editYaml('services:\n  a:\n    image: x\n    deploy:\n      resources:\n        limits:\n          memory: 1g\n', (d) => setLimit(d, editService(d, 'a')!, 'memory', ''));
  assert.equal(deploy, 'services:\n  a:\n    image: x\n');
});

test('services can be added, renamed and removed with their references', () => {
  let text = 'services:\n  app:\n    image: x\n    depends_on: [db]\n  db:\n    image: postgres\n';
  text = editYaml(text, (d) => renameService(d, 'db', 'database'));
  assert.ok(text.includes('depends_on: [ database ]') || text.includes('depends_on: [database]'), text);
  assert.ok(text.includes('  database:\n    image: postgres'));
  text = editYaml(text, (d) => addService(d, 'cache', { image: 'redis:7', ports: ['6379:6379'], restart: 'unless-stopped' }));
  assert.ok(text.endsWith('  cache:\n    image: redis:7\n    restart: unless-stopped\n    ports:\n      - "6379:6379"\n'), text);
  text = editYaml(text, (d) => removeService(d, 'database'));
  const data = parseYaml(text).toJS() as { services: Record<string, Record<string, unknown>> };
  assert.deepEqual(Object.keys(data.services), ['app', 'cache']);
  assert.equal(data.services.app.depends_on, undefined);
});

test('new keys and services follow the blank lines around them', () => {
  const text = 'services:\n  a:\n    image: x\n\n  b:\n    image: y\n\nvolumes:\n  data: {}\n';
  const out = editYaml(text, (d) => {
    declareTopLevel(d, 'networks', 'proxy', true);
    addService(d, 'c', { image: 'z' });
  });
  assert.equal(out, 'services:\n  a:\n    image: x\n\n  b:\n    image: y\n\n  c:\n    image: z\n\nnetworks:\n  proxy:\n    external: true\n\nvolumes:\n  data: {}\n');
  // A new file gets a blank line between services; a packed one stays packed.
  const fresh = editYaml('services:\n', (d) => {
    addService(d, 'a', { image: 'nginx' });
    addService(d, 'b', { image: 'redis' });
  });
  assert.equal(fresh, 'services:\n  a:\n    image: nginx\n\n  b:\n    image: redis\n');
  const packed = editYaml('services:\n  a:\n    image: x\n  b:\n    image: y\n', (d) => addService(d, 'c', { image: 'z' }));
  assert.equal(packed, 'services:\n  a:\n    image: x\n  b:\n    image: y\n  c:\n    image: z\n');
});

test('single settings round-trip through raw YAML', () => {
  const doc = parseYaml(MEDIA);
  const svc = editService(doc, 'sonarr')!;
  const text = pairText(doc, svc, 'healthcheck');
  assert.equal(text, 'healthcheck:\n  test: [ "CMD", "curl", "-f", "http://localhost:8989/ping" ]\n  interval: 30s\n');
  const back = parsePairText(text.replace('30s', '1m'), 'healthcheck');
  assert.ok('node' in back);
  assert.ok('error' in parsePairText('other: 1', 'healthcheck'));
  assert.ok('error' in parsePairText('healthcheck: [', 'healthcheck'));
});

test('interpolation follows compose rules', () => {
  const env = new Map(Object.entries({ A: 'a', EMPTY: '' }));
  const look = (n: string) => env.get(n);
  assert.equal(interpolate('$A-${A}', look).value, 'a-a');
  assert.equal(interpolate('${EMPTY:-d}|${EMPTY-d}|${UNSET-d}', look).value, 'd||d');
  assert.equal(interpolate('${A:+yes}|${EMPTY:+yes}|${EMPTY+yes}|${UNSET+yes}', look).value, 'yes||yes|');
  assert.equal(interpolate('${UNSET:-${A}x}', look).value, 'ax');
  assert.equal(interpolate('$$A costs $$5', look).value, '$A costs $5');
  const r = interpolate('${UNSET}/${NEED:?set NEED first}', look);
  assert.deepEqual(r.missing, ['UNSET']);
  assert.deepEqual(r.errors, ['set NEED first']);
  assert.ok(interpolate('${1BAD}', look).issues.length > 0);
  assert.equal(pureReference('${TZ:-UTC}')?.name, 'TZ');
  assert.equal(pureReference('x${TZ}'), null);
  assert.deepEqual(
    references('${A:-${B}}/$C').map((v) => v.name),
    ['A', 'B', 'C'],
  );
  assert.equal(escapeDollars('pa$s'), 'pa$$s');
  assert.equal(escapeReferences('abc$def${TZ}${x}', ['def', 'x']), 'abc$$def${TZ}$${x}');
  assert.equal(interpolate(escapeReferences('abc$def', ['def']), look).value, 'abc$def');
  assert.equal(hasStrayDollar('pa$ 1'), true);
  assert.equal(hasStrayDollar('${A} $$ $B'), false);
});

test('dotenv parsing matches compose', () => {
  const text = `# media\nexport PUID=1000\nTZ="Europe/Berlin" # zone\nMSG='it''s'\nRAW='$HOME'\nPLAIN=a b # c\nMULTI="line1\nline2"\nBARE\n`;
  const e = parseDotenv(text);
  assert.deepEqual(
    e.map((x) => [x.key, x.value, x.quote, x.line]),
    [
      ['PUID', '1000', '', 2],
      ['TZ', 'Europe/Berlin', '"', 3],
      ['MSG', 'it', "'", 4],
      ['RAW', '$HOME', "'", 5],
      ['PLAIN', 'a b', '', 6],
      ['MULTI', 'line1\nline2', '"', 7],
      ['BARE', null, '', 9],
    ],
  );
  assert.equal(e[0].exported, true);
  assert.equal(e[1].comment, '# zone');
  const vals = evaluateDotenv('A=1\nB=${A}2\nC=\'${A}\'\nD=${OUT}\n', (n) => (n === 'OUT' ? 'o' : undefined));
  assert.deepEqual([...vals], [['A', '1'], ['B', '12'], ['C', '${A}'], ['D', 'o']]);
});

test('dotenv edits keep everything else', () => {
  const text = '# media\nexport PUID=1000\nTZ="Europe/Berlin" # zone\n\nOTHER=1\n';
  assert.equal(setDotenv(text, 'TZ', 'America/New_York'), '# media\nexport PUID=1000\nTZ="America/New_York" # zone\n\nOTHER=1\n');
  assert.equal(setDotenv(text, 'PUID', '1001'), '# media\nexport PUID=1001\nTZ="Europe/Berlin" # zone\n\nOTHER=1\n');
  assert.equal(setDotenv(text, 'NEW', 'two words'), `${text}NEW="two words"\n`);
  assert.equal(setDotenv('A=1', 'B', '2'), 'A=1\nB=2\n');
  assert.equal(deleteDotenv(text, 'TZ'), '# media\nexport PUID=1000\n\nOTHER=1\n');
  assert.equal(renameDotenv(text, 'OTHER', 'SECOND'), text.replace('OTHER', 'SECOND'));
  assert.equal(setDotenv('A=1\r\n', 'B', '2'), 'A=1\r\nB=2\r\n');
});

test('dotenv values are quoted so they read back the same', () => {
  assert.equal(renderValue('simple'), 'simple');
  assert.equal(renderValue('two words'), '"two words"');
  assert.equal(renderValue('p$ss', { literal: true }), "'p$ss'");
  assert.equal(renderValue('${BASE}/data'), '${BASE}/data');
  assert.equal(renderValue('say "hi"'), '"say \\"hi\\""');
  for (const v of ['simple', 'two words', 'p$ss', 'say "hi"', "it's", 'a#b', ' lead', 'multi\nline']) {
    const text = setDotenv('', 'K', v, { literal: true });
    assert.equal(evaluateDotenv(text).get('K'), v, `${v} -> ${text}`);
  }
  assert.deepEqual(dotenvIssues('A=1\nnot a line\nB=${1}\n').map((i) => i.line), [2, 3]);
});

test('the effective environment of a service, with where each value comes from', () => {
  const compose = parseYaml(`services:
  app:
    image: x
    env_file: [common.env, app.env]
    environment:
      TZ: \${TZ:-UTC}
      PASSWORD: \${APP_PASS}
      LEVEL: debug
      PUID:
`);
  const r = serviceEnv({
    composes: [{ file: 'compose.yaml', doc: compose }],
    service: 'app',
    project: new Map(Object.entries({ TZ: 'Europe/Berlin', PUID: '1000', APP_PASS: 's3cret', HOST: 'nas' })),
    envFiles: { 'common.env': 'LEVEL=info\nURL=http://${HOST}\n', 'app.env': 'URL=http://app\nEXTRA=1\n' },
    image: [
      { key: 'PATH', value: '/usr/bin' },
      { key: 'LEVEL', value: 'warn' },
    ],
  });
  const byKey = Object.fromEntries(r.rows.map((x) => [x.key, x]));
  assert.deepEqual(r.rows.map((x) => x.key), ['TZ', 'PASSWORD', 'LEVEL', 'PUID', 'URL', 'EXTRA', 'PATH']);
  assert.equal(byKey.TZ.def.value, 'Europe/Berlin');
  assert.equal(byKey.TZ.via, 'TZ');
  assert.equal(byKey.TZ.viaDefault, 'UTC');
  assert.equal(byKey.PASSWORD.secret, true);
  assert.equal(byKey.PASSWORD.via, 'APP_PASS');
  assert.equal(byKey.LEVEL.def.value, 'debug');
  assert.deepEqual(byKey.LEVEL.shadowed.map((d) => d.origin.kind), ['env_file', 'image']);
  assert.equal(byKey.PUID.def.raw, null);
  assert.equal(byKey.PUID.def.value, '1000');
  assert.equal(byKey.PUID.via, 'PUID');
  assert.equal(byKey.URL.def.value, 'http://app');
  assert.equal(byKey.URL.shadowed[0].value, 'http://nas');
  assert.equal(byKey.PATH.def.origin.kind, 'image');
});

test('variable usage across services', () => {
  const doc = parseYaml(`services:
  a:
    image: "img:\${TAG:-latest}"
    environment:
      - TZ=\${TZ}
      - PUID
  b:
    environment:
      DB: \${DB_PASS:?set it}
`);
  const u = variableUsage([{ file: 'compose.yaml', doc }]);
  assert.deepEqual(u.get('TAG'), { services: ['a'], defaults: ['latest'], bare: false, required: false });
  assert.deepEqual(u.get('TZ')?.services, ['a']);
  assert.equal(u.get('TZ')?.bare, true);
  assert.deepEqual(u.get('PUID')?.services, ['a']);
  assert.equal(u.get('DB_PASS')?.required, true);
});
