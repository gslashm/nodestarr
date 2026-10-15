import assert from 'node:assert/strict';
import { test } from 'node:test';

import { ansiToHtml, logLevel, stripAnsi } from '../src/lib/ansi.ts';

const ESC = '\u001b';

test('plain lines are escaped', () => {
  assert.equal(ansiToHtml('<b>&</b>'), '&lt;b&gt;&amp;&lt;/b&gt;');
  assert.equal(ansiToHtml('carriage\r'), 'carriage');
});

test('colors and bold become spans', () => {
  assert.equal(ansiToHtml(`${ESC}[31mred${ESC}[0m plain`), '<span class="a-r">red</span> plain');
  assert.equal(ansiToHtml(`${ESC}[1;32mok${ESC}[22m still green${ESC}[39m`), '<span class="a-g a-bold">ok</span><span class="a-g"> still green</span>');
  assert.equal(ansiToHtml(`${ESC}[33m<warn>`), '<span class="a-y">&lt;warn&gt;</span>');
});

test('other escape sequences are dropped', () => {
  assert.equal(ansiToHtml(`${ESC}[2K${ESC}[1Gprogress`), 'progress');
  assert.equal(stripAnsi(`${ESC}[31mred${ESC}[0m ${ESC}]0;title\u0007done`), 'red done');
});

test('log levels from common markers', () => {
  assert.equal(logLevel('2026-10-01 12:00:00 ERROR something broke'), 'error');
  assert.equal(logLevel('[Fatal] crashed'), 'error');
  assert.equal(logLevel('time=1 level=error msg=x'), 'error');
  assert.equal(logLevel('[warn] disk almost full'), 'warn');
  assert.equal(logLevel('level=warning msg=slow'), 'warn');
  assert.equal(logLevel('[Info] all good'), '');
  // Whole words only: no match inside other words or plurals.
  assert.equal(logLevel('Scan finished with 0 errors, 0 warnings'), '');
  assert.equal(logLevel('Indexed "Terror Firma" (1997)'), '');
});
