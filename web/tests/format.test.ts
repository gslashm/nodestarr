import assert from 'node:assert/strict';
import { test } from 'node:test';

import { actorLabel, bytes, clockTime, duration, imageRepo, imageTag, setTimeFormat, shortImage, stateOf } from '../src/lib/format.ts';

test('bytes uses binary units', () => {
  assert.equal(bytes(null), '–');
  assert.equal(bytes(512), '512 B');
  assert.equal(bytes(1536), '1.5 KiB');
  assert.equal(bytes(250 * 1024 ** 2), '250 MiB');
  assert.equal(bytes(3.25 * 1024 ** 3), '3.3 GiB');
});

test('duration is compact', () => {
  assert.equal(duration(42), '42s');
  assert.equal(duration(600), '10m');
  assert.equal(duration(3600 * 3 + 60 * 20), '3h 20m');
  assert.equal(duration(86400 * 4 + 3600 * 6), '4d 6h');
  assert.equal(duration(86400 * 2), '2d');
  assert.equal(duration(-5), '0s');
});

test('clock times follow the format setting', () => {
  setTimeFormat('24h');
  assert.equal(clockTime(3, 5), '03:05');
  assert.equal(clockTime(15, 30), '15:30');
  setTimeFormat('12h');
  assert.equal(clockTime(0, 0), '12:00 AM');
  assert.equal(clockTime(12, 0), '12:00 PM');
  assert.equal(clockTime(15, 30), '3:30 PM');
  setTimeFormat('24h');
});

test('image names', () => {
  assert.equal(imageTag('lscr.io/linuxserver/sonarr:develop'), 'develop');
  assert.equal(imageTag('registry:5000/app'), 'latest');
  assert.equal(imageTag('nginx@sha256:abc'), 'latest');
  assert.equal(imageTag('nginx:1.27@sha256:abc'), '1.27');
  assert.equal(imageRepo('registry:5000/app:2'), 'registry:5000/app');
  assert.equal(shortImage('docker.io/library/nginx:1'), 'nginx:1');
});

test('container states', () => {
  const base = { state: 'running', health: null, exit_code: null, oom_killed: false };
  assert.equal(stateOf(base), 'running');
  assert.equal(stateOf({ ...base, health: 'unhealthy' }), 'unhealthy');
  assert.equal(stateOf({ ...base, state: 'exited', exit_code: 0 }), 'stopped');
  assert.equal(stateOf({ ...base, state: 'exited', exit_code: 143 }), 'stopped');
  assert.equal(stateOf({ ...base, state: 'exited', exit_code: 1 }), 'crashed');
  assert.equal(stateOf({ ...base, state: 'exited', exit_code: 137, oom_killed: true }), 'crashed');
});

test('actors', () => {
  assert.equal(actorLabel('user:jake'), 'jake');
  assert.equal(actorLabel('api:ha'), 'API key "ha"');
  assert.equal(actorLabel('auto-update'), 'Automatic update');
  assert.equal(actorLabel(null), 'Berth');
});
