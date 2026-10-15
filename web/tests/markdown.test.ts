import assert from 'node:assert/strict';
import { test } from 'node:test';

import { renderMarkdown } from '../src/lib/markdown.ts';

test('HTML in release notes is escaped', () => {
  const html = renderMarkdown('<script>alert(1)</script> <img src=x onerror=alert(1)>');
  assert.ok(!html.includes('<script'));
  assert.ok(!html.includes('<img'));
  assert.ok(html.includes('&lt;script&gt;'));
});

test('only http(s) links become anchors', () => {
  assert.equal(renderMarkdown('[x](javascript:alert(1))').includes('<a'), false);
  assert.equal(renderMarkdown('[x](data:text/html,hi)').includes('<a'), false);
  const ok = renderMarkdown('[notes](https://example.com/a?b=1&c=2)');
  assert.match(ok, /<a href="https:\/\/example\.com\/a\?b=1&amp;c=2" target="_blank" rel="noopener noreferrer">notes<\/a>/);
});

test('quotes cannot break out of link attributes', () => {
  const html = renderMarkdown('[x](https://example.com/"onmouseover="alert(1))');
  assert.ok(!/href="[^"]*"onmouseover/.test(html));
});

test('headings, lists, code and emphasis', () => {
  const html = renderMarkdown('# Changes\n\n- **Fixed** a bug\n- Added `--flag`\n\n1. one\n2. two\n\n```\n<b>raw</b>\n```\n\nPlain *text*.');
  assert.match(html, /<h3>Changes<\/h3>/);
  assert.match(html, /<ul>\n<li><strong>Fixed<\/strong> a bug<\/li>\n<li>Added <code>--flag<\/code><\/li>\n<\/ul>/);
  assert.match(html, /<ol>\n<li>one<\/li>\n<li>two<\/li>\n<\/ol>/);
  assert.match(html, /<pre><code>&lt;b&gt;raw&lt;\/b&gt;<\/code><\/pre>/);
  assert.match(html, /<p>Plain <em>text<\/em>\.<\/p>/);
});

test('bare URLs are linked', () => {
  assert.match(renderMarkdown('See https://example.com/x for more'), /<a href="https:\/\/example\.com\/x"/);
});
