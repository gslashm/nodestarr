// `.env` files the way Compose reads them, and edits that leave every other
// line (comments, blank lines, order, quoting) exactly as it was.
//
// Syntax: `KEY=value`, optional `export `, `#` comments, unquoted values with
// trailing ` # comments`, single quotes (literal), double quotes (escapes,
// may span lines). Unquoted and double-quoted values interpolate `${VAR}`
// from earlier lines and the environment; single-quoted ones don't.

import { interpolate, parseTemplate } from './interpolate';

export type Quote = '' | '"' | "'";

export interface DotenvEntry {
  key: string;
  /** Decoded value before interpolation; null for a bare `KEY` line (taken from the environment). */
  value: string | null;
  quote: Quote;
  exported: boolean;
  /** Offsets of the whole statement (without its line break). */
  start: number;
  end: number;
  /** 1-based line of the statement. */
  line: number;
  /** Trailing comment, including the `#`. */
  comment: string;
}

const KEY = /^[A-Za-z_][A-Za-z0-9_.-]*$/;

function unescapeDouble(s: string): string {
  return s.replace(/\\(.)/gs, (_, c: string) => {
    switch (c) {
      case 'n':
        return '\n';
      case 'r':
        return '\r';
      case 't':
        return '\t';
      default:
        return c;
    }
  });
}

/** Finds the closing quote, honoring backslash escapes in double quotes. */
function closingQuote(text: string, from: number, q: string): number {
  for (let i = from; i < text.length; i++) {
    if (q === '"' && text[i] === '\\') {
      i++;
      continue;
    }
    if (text[i] === q) return i;
  }
  return -1;
}

export function parseDotenv(text: string): DotenvEntry[] {
  const out: DotenvEntry[] = [];
  let pos = 0;
  let line = 1;
  while (pos < text.length) {
    let eol = text.indexOf('\n', pos);
    if (eol < 0) eol = text.length;
    const raw = text.slice(pos, eol).replace(/\r$/, '');
    const lead = raw.length - raw.trimStart().length;
    let body = raw.trim();
    const startLine = line;
    const start = pos + lead;
    let next = eol + 1;
    line++;
    if (!body || body.startsWith('#')) {
      pos = next;
      continue;
    }
    let exported = false;
    if (/^export\s/.test(body)) {
      exported = true;
      body = body.replace(/^export\s+/, '');
    }
    const sep = body.search(/[=:]/);
    const key = (sep < 0 ? body : body.slice(0, sep)).trim();
    if (!KEY.test(key)) {
      pos = next;
      continue;
    }
    if (sep < 0) {
      out.push({ key, value: null, quote: '', exported, start, end: pos + raw.length, line: startLine, comment: '' });
      pos = next;
      continue;
    }
    // Offset of the value within the file.
    const bodyStart = pos + raw.indexOf(body);
    let v = bodyStart + sep + 1;
    while (v < text.length && (text[v] === ' ' || text[v] === '\t')) v++;
    const q = text[v];
    if (q === '"' || q === "'") {
      const close = closingQuote(text, v + 1, q);
      if (close >= 0) {
        const inner = text.slice(v + 1, close);
        let lineEnd = text.indexOf('\n', close);
        if (lineEnd < 0) lineEnd = text.length;
        const rest = text.slice(close + 1, lineEnd).replace(/\r$/, '');
        const hash = rest.indexOf('#');
        for (let i = v; i < close; i++) if (text[i] === '\n') line++;
        out.push({
          key,
          value: q === '"' ? unescapeDouble(inner) : inner,
          quote: q,
          exported,
          start,
          end: close + 1 + rest.trimEnd().length,
          line: startLine,
          comment: hash >= 0 ? rest.slice(hash).trimEnd() : '',
        });
        pos = lineEnd + 1;
        continue;
      }
    }
    const valueText = text.slice(v, pos + raw.length);
    const hash = valueText.search(/\s#/);
    const value = (hash >= 0 ? valueText.slice(0, hash) : valueText).trim();
    out.push({
      key,
      value,
      quote: '',
      exported,
      start,
      end: pos + raw.length,
      line: startLine,
      comment: hash >= 0 ? valueText.slice(hash).trim() : '',
    });
    pos = next;
  }
  return out;
}

/** The last assignment of each key (later lines win), in file order of first appearance. */
export function dotenvMap(text: string): Map<string, DotenvEntry> {
  const m = new Map<string, DotenvEntry>();
  for (const e of parseDotenv(text)) m.set(e.key, e);
  return m;
}

/**
 * Values after interpolation, as Compose sees them. Each line can use the
 * keys before it and `outer` (for env files: the stack's own variables).
 */
export function evaluateDotenv(text: string, outer: (name: string) => string | undefined = () => undefined): Map<string, string> {
  const vals = new Map<string, string>();
  const look = (n: string) => (vals.has(n) ? vals.get(n) : outer(n));
  for (const e of parseDotenv(text)) {
    if (e.value === null) {
      const v = outer(e.key);
      if (v !== undefined) vals.set(e.key, v);
      continue;
    }
    vals.set(e.key, e.quote === "'" ? e.value : interpolate(e.value, look).value);
  }
  return vals;
}

function needsQuoting(v: string): boolean {
  return /[\s#"'\\]|^[=:]/.test(v);
}

/**
 * Renders a value. `literal` values must come back exactly as typed, so a
 * `$` is protected from interpolation; otherwise `${VAR}` stays live.
 * `prefer` keeps the quoting the line already used when it still works.
 */
export function renderValue(value: string, opts: { literal?: boolean; prefer?: Quote } = {}): string {
  const literal = opts.literal ?? false;
  const dollar = value.includes('$');
  const canSingle = !value.includes("'") && !value.includes('\n');
  const asDouble = () => `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t')}"`;
  if (literal && dollar) {
    // Single quotes stop interpolation entirely.
    if (canSingle) return `'${value}'`;
    return asDouble().replace(/\$/g, '$$$$');
  }
  if (opts.prefer === "'" && canSingle && (!dollar || literal)) return `'${value}'`;
  if (opts.prefer === '"') return asDouble();
  if (!needsQuoting(value)) return value;
  return asDouble();
}

function lineBreak(text: string): string {
  return text.includes('\r\n') ? '\r\n' : '\n';
}

/**
 * Sets `key` to `value`: replaces the last assignment in place (keeping its
 * `export`, comment and, when possible, its quotes) or appends a new line.
 */
export function setDotenv(text: string, key: string, value: string, opts: { literal?: boolean } = {}): string {
  const entries = parseDotenv(text).filter((e) => e.key === key);
  const last = entries[entries.length - 1];
  if (last) {
    const rendered = renderValue(value, { literal: opts.literal, prefer: last.quote });
    const line = `${last.exported ? 'export ' : ''}${key}=${rendered}${last.comment ? ` ${last.comment}` : ''}`;
    return text.slice(0, last.start) + line + text.slice(last.end);
  }
  const nl = lineBreak(text);
  const line = `${key}=${renderValue(value, { literal: opts.literal })}`;
  if (!text) return line + nl;
  return text.endsWith('\n') ? text + line + nl : text + nl + line + nl;
}

/** Removes every assignment of `key`, with its line break. */
export function deleteDotenv(text: string, key: string): string {
  const entries = parseDotenv(text).filter((e) => e.key === key);
  let out = text;
  for (const e of entries.reverse()) {
    let s = e.start;
    while (s > 0 && out[s - 1] !== '\n') s--;
    let end = e.end;
    if (out[end] === '\r') end++;
    if (out[end] === '\n') end++;
    out = out.slice(0, s) + out.slice(end);
  }
  return out;
}

/** Renames a key everywhere it's assigned. */
export function renameDotenv(text: string, from: string, to: string): string {
  const entries = parseDotenv(text).filter((e) => e.key === from);
  let out = text;
  for (const e of entries.reverse()) {
    const seg = out.slice(e.start, e.end);
    const i = seg.indexOf(from);
    out = out.slice(0, e.start + i) + to + out.slice(e.start + i + from.length);
  }
  return out;
}

/** Problems with a .env text: lines that aren't assignments, and bad `${...}` references. */
export function dotenvIssues(text: string): { line: number; message: string }[] {
  const out: { line: number; message: string }[] = [];
  const covered = new Set<number>();
  for (const e of parseDotenv(text)) {
    const span = text.slice(e.start, e.end).split('\n').length;
    for (let l = e.line; l < e.line + span; l++) covered.add(l);
    if (e.value !== null && e.quote !== "'") {
      for (const issue of parseTemplate(e.value).issues) out.push({ line: e.line, message: issue });
    }
  }
  text.split(/\r?\n/).forEach((l, i) => {
    const t = l.trim();
    if (!t || t.startsWith('#') || covered.has(i + 1)) return;
    out.push({ line: i + 1, message: `"${t.length > 30 ? `${t.slice(0, 30)}…` : t}" isn't a KEY=value line` });
  });
  return out.sort((a, b) => a.line - b.line);
}
