import assert from 'node:assert/strict';
import { test } from 'node:test';

import { cronPresets, describeCron } from '../src/lib/cron.ts';
import { setTimeFormat } from '../src/lib/format.ts';

test('common schedules read as plain language', () => {
  setTimeFormat('24h');
  assert.equal(describeCron('*/15 * * * *'), 'Every 15 minutes');
  assert.equal(describeCron('*/1 * * * *'), 'Every minute');
  assert.equal(describeCron('17 * * * *'), 'Every hour at :17');
  assert.equal(describeCron('17 */6 * * *'), 'Every 6 hours, at :17');
  assert.equal(describeCron('30 3 * * *'), 'Every day at 03:30');
  assert.equal(describeCron('0 3,15 * * *'), 'Every day at 03:00, 15:00');
  assert.equal(describeCron('0 4 * * 0'), 'Every Sunday at 04:00');
  assert.equal(describeCron('0 2 * * 1-5'), 'Weekdays at 02:00');
  assert.equal(describeCron('0 9 * * 6,0'), 'Weekends at 09:00');
  assert.equal(describeCron('0 1 15 * *'), 'On day 15 of every month at 01:00');
  assert.equal(describeCron('@daily'), 'Every day at midnight');
});

test('times follow the 12-hour setting', () => {
  setTimeFormat('12h');
  try {
    assert.equal(describeCron('30 3 * * *'), 'Every day at 3:30 AM');
    assert.equal(describeCron('0 0 * * *'), 'Every day at 12:00 AM');
    assert.equal(describeCron('5 12 * * *'), 'Every day at 12:05 PM');
    assert.equal(describeCron('0 23 * * 1-5'), 'Weekdays at 11:00 PM');
    assert.ok(cronPresets('daily')[0].label.endsWith('3:30 AM'));
  } finally {
    setTimeFormat('24h');
  }
});

test('unusual shapes are not described', () => {
  assert.equal(describeCron('0 4 * 6 *'), null);
  assert.equal(describeCron('0 4 1-7 * 1'), null);
  assert.equal(describeCron('not a schedule'), null);
  assert.equal(describeCron(''), null);
});

test('presets describe themselves', () => {
  for (const kind of ['check', 'daily'] as const) {
    for (const p of cronPresets(kind)) assert.notEqual(describeCron(p.value), null, p.value);
  }
});
