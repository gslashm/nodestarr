// Compose's variable interpolation: `$VAR`, `${VAR}`, `${VAR:-default}`,
// `${VAR-default}`, `${VAR:?error}`, `${VAR?error}`, `${VAR:+alt}`,
// `${VAR+alt}`, nesting inside defaults, and `$$` for a literal `$`.

export type Op = '' | ':-' | '-' | ':?' | '?' | ':+' | '+';

export interface VarExpr {
  name: string;
  op: Op;
  /** The text after the operator (default, error message or alternate). */
  arg: Part[];
  argRaw: string;
  braced: boolean;
  /** Offsets of the whole expression in the source. */
  start: number;
  end: number;
}

export type Part = string | VarExpr;

export interface Parsed {
  parts: Part[];
  /** Malformed expressions (compose rejects these). */
  issues: string[];
}

const NAME = /[A-Za-z_][A-Za-z0-9_]*/y;
const OPS: Op[] = [':-', ':?', ':+', '-', '?', '+'];

function parseAt(s: string, from: number, inBraces: boolean, issues: string[]): { parts: Part[]; end: number } {
  const parts: Part[] = [];
  let text = '';
  let i = from;
  const flush = () => {
    if (text) parts.push(text);
    text = '';
  };
  while (i < s.length) {
    const c = s[i];
    if (inBraces && c === '}') return (flush(), { parts, end: i });
    if (c !== '$') {
      text += c;
      i++;
      continue;
    }
    const next = s[i + 1];
    if (next === '$') {
      text += '$';
      i += 2;
      continue;
    }
    if (next === '{') {
      NAME.lastIndex = i + 2;
      const m = NAME.exec(s);
      if (!m) {
        issues.push(`"${s.slice(i, Math.min(s.length, i + 12))}" is not a valid variable`);
        text += '${';
        i += 2;
        continue;
      }
      let j = i + 2 + m[0].length;
      let op: Op = '';
      if (s[j] !== '}') {
        const found = OPS.find((o) => s.startsWith(o, j));
        if (!found) {
          issues.push(`"\${${m[0]}${s[j] ?? ''}" is not a valid variable`);
          text += s.slice(i, j);
          i = j;
          continue;
        }
        op = found;
        j += found.length;
      }
      if (op) {
        const inner = parseAt(s, j, true, issues);
        if (inner.end >= s.length) {
          issues.push(`\${${m[0]} is missing its closing }`);
          text += s.slice(i);
          i = s.length;
          continue;
        }
        flush();
        parts.push({ name: m[0], op, arg: inner.parts, argRaw: s.slice(j, inner.end), braced: true, start: i, end: inner.end + 1 });
        i = inner.end + 1;
      } else {
        flush();
        parts.push({ name: m[0], op, arg: [], argRaw: '', braced: true, start: i, end: j + 1 });
        i = j + 1;
      }
      continue;
    }
    NAME.lastIndex = i + 1;
    const m = NAME.exec(s);
    if (m) {
      flush();
      parts.push({ name: m[0], op: '', arg: [], argRaw: '', braced: false, start: i, end: i + 1 + m[0].length });
      i += 1 + m[0].length;
      continue;
    }
    text += '$';
    i++;
  }
  flush();
  return { parts, end: s.length };
}

export function parseTemplate(s: string): Parsed {
  const issues: string[] = [];
  return { parts: parseAt(s, 0, false, issues).parts, issues };
}

export type Lookup = (name: string) => string | undefined;

export interface Evaluated {
  value: string;
  /** Variables used without a default that aren't set. */
  missing: string[];
  /** `${VAR:?message}` failures. */
  errors: string[];
  /** Every variable the value depends on. */
  used: string[];
}

function run(parts: Part[], lookup: Lookup, acc: Omit<Evaluated, 'value'>): string {
  let out = '';
  for (const p of parts) {
    if (typeof p === 'string') {
      out += p;
      continue;
    }
    if (!acc.used.includes(p.name)) acc.used.push(p.name);
    const v = lookup(p.name);
    const unset = v === undefined;
    const empty = unset || v === '';
    switch (p.op) {
      case '':
        if (unset && !acc.missing.includes(p.name)) acc.missing.push(p.name);
        out += v ?? '';
        break;
      case ':-':
        out += empty ? run(p.arg, lookup, acc) : v;
        break;
      case '-':
        out += unset ? run(p.arg, lookup, acc) : v;
        break;
      case ':?':
      case '?':
        if (p.op === ':?' ? empty : unset) acc.errors.push(run(p.arg, lookup, acc) || `${p.name} is required`);
        out += v ?? '';
        break;
      case ':+':
        out += empty ? '' : run(p.arg, lookup, acc);
        break;
      case '+':
        out += unset ? '' : run(p.arg, lookup, acc);
        break;
    }
  }
  return out;
}

export function evaluate(parts: Part[], lookup: Lookup): Evaluated {
  const acc = { missing: [] as string[], errors: [] as string[], used: [] as string[] };
  const value = run(parts, lookup, acc);
  return { value, ...acc };
}

export function interpolate(s: string, lookup: Lookup): Evaluated & { issues: string[] } {
  const { parts, issues } = parseTemplate(s);
  return { ...evaluate(parts, lookup), issues };
}

/** Every variable expression in a value, nested ones included. */
export function references(s: string): VarExpr[] {
  const out: VarExpr[] = [];
  const walk = (parts: Part[]) => {
    for (const p of parts) {
      if (typeof p === 'string') continue;
      out.push(p);
      walk(p.arg);
    }
  };
  walk(parseTemplate(s).parts);
  return out;
}

/** The variable when the whole value is one reference (`${TZ}`, `${TZ:-UTC}`, `$TZ`). */
export function pureReference(s: string): VarExpr | null {
  const { parts, issues } = parseTemplate(s);
  if (issues.length || parts.length !== 1 || typeof parts[0] === 'string') return null;
  const p = parts[0];
  return p.start === 0 && p.end === s.length ? p : null;
}

/** The default a reference falls back to, if it has one. */
export function defaultOf(v: VarExpr): string | null {
  return v.op === ':-' || v.op === '-' ? v.argRaw : null;
}

/**
 * Escapes the `$` of plain references to these variables, so `pa$word`
 * stays `pa$word` instead of becoming "pa" plus an unset variable.
 */
export function escapeReferences(s: string, names: string[]): string {
  const starts = references(s)
    .filter((r) => r.op === '' && names.includes(r.name))
    .map((r) => r.start)
    .sort((a, b) => b - a);
  let out = s;
  for (const at of starts) out = `${out.slice(0, at)}$${out.slice(at)}`;
  return out;
}

/** Escapes `$` so a literal value survives interpolation. */
export function escapeDollars(s: string): string {
  return s.replace(/\$/g, '$$$$');
}

/** True when the text contains a `$` that isn't part of a reference or `$$`. */
export function hasStrayDollar(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    if (s[i] !== '$') continue;
    const n = s[i + 1];
    if (n === '$') {
      i++;
      continue;
    }
    if (n === '{' || (n && /[A-Za-z_]/.test(n))) continue;
    return true;
  }
  return false;
}
