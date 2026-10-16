// Dockerfiles as a list of instructions, for the visual editor.
//
// Parsing is lossless: every item keeps its original text, so instructions
// nobody touched are written back byte for byte. Edited instructions are
// rendered fresh. Continuation lines, heredocs, comments and parser
// directives (`# syntax=`, `# escape=`) are understood.

export interface DfInstruction {
  kind: 'instruction';
  /** Upper case: FROM, RUN, COPY... */
  keyword: string;
  /**
   * Arguments as written; continuation lines are joined with newlines (one
   * line per continued line, indentation and the escape character removed).
   */
  args: string;
  /** Original text, including continuation lines and the line break. */
  text: string;
  /** 1-based line of the keyword. */
  line: number;
  /** Uses a heredoc (`RUN <<EOF`); edited as raw text. */
  heredoc: boolean;
}

export interface DfOther {
  kind: 'comment' | 'blank' | 'directive';
  text: string;
  line: number;
}

export type DfItem = DfInstruction | DfOther;

export interface Dockerfile {
  items: DfItem[];
  escape: string;
}

export const KEYWORDS = [
  'FROM',
  'RUN',
  'CMD',
  'LABEL',
  'EXPOSE',
  'ENV',
  'ADD',
  'COPY',
  'ENTRYPOINT',
  'VOLUME',
  'USER',
  'WORKDIR',
  'ARG',
  'ONBUILD',
  'STOPSIGNAL',
  'HEALTHCHECK',
  'SHELL',
  'MAINTAINER',
] as const;

const HEREDOC = /<<(-?)(["']?)([A-Za-z_][A-Za-z0-9_]*)\2/g;

export function parseDockerfile(text: string): Dockerfile {
  const lines = text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const items: DfItem[] = [];
  let escape = '\\';
  let directives = true;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const body = raw.replace(/\r?\n$/, '');
    const t = body.trim();
    if (!t) {
      directives = false;
      items.push({ kind: 'blank', text: raw, line: i + 1 });
      continue;
    }
    if (t.startsWith('#')) {
      const d = directives ? /^#\s*([a-zA-Z]+)\s*=\s*(\S+)\s*$/.exec(t) : null;
      if (d) {
        if (d[1].toLowerCase() === 'escape' && (d[2] === '`' || d[2] === '\\')) escape = d[2];
        items.push({ kind: 'directive', text: raw, line: i + 1 });
      } else {
        directives = false;
        items.push({ kind: 'comment', text: raw, line: i + 1 });
      }
      continue;
    }
    directives = false;
    const m = /^(\s*)([A-Za-z]+)(\s+|$)/.exec(body);
    const keyword = (m?.[2] ?? t.split(/\s+/)[0]).toUpperCase();
    const start = i;
    const parts: string[] = [m ? body.slice(m[0].length) : ''];
    let text = raw;
    let heredoc = false;
    // Heredocs: everything up to the closing delimiter belongs to the instruction.
    const docs = [...parts[0].matchAll(HEREDOC)].map((h) => ({ strip: h[1] === '-', word: h[3] }));
    if (docs.length && ['RUN', 'COPY', 'ADD'].includes(keyword)) {
      heredoc = true;
      for (const d of docs) {
        while (i + 1 < lines.length) {
          i++;
          text += lines[i];
          const l = lines[i].replace(/\r?\n$/, '');
          if ((d.strip ? l.replace(/^\t+/, '') : l) === d.word) break;
        }
      }
      items.push({ kind: 'instruction', keyword, args: text.slice((m?.[0].length ?? 0)).replace(/\r?\n$/, ''), text, line: start + 1, heredoc });
      continue;
    }
    // Continuation lines.
    const continues = (s: string) => s.trimEnd().endsWith(escape);
    const strip = (s: string) => s.trimEnd().slice(0, -1).trimEnd();
    let cont = continues(parts[0]);
    if (cont) parts[0] = strip(parts[0]);
    while (cont && i + 1 < lines.length) {
      i++;
      text += lines[i];
      const next = lines[i].replace(/\r?\n$/, '').trim();
      // Comments and blank lines inside a continued instruction are skipped.
      if (!next || next.startsWith('#')) continue;
      cont = continues(next);
      parts.push(cont ? strip(next) : next);
    }
    items.push({ kind: 'instruction', keyword, args: parts.filter((p) => p !== '').join('\n'), text, line: start + 1, heredoc });
  }
  return { items, escape };
}

export function serializeDockerfile(df: Dockerfile): string {
  return df.items.map((i) => i.text).join('');
}

/** Text of an instruction: one line per argument line, continued with the escape character. */
export function renderInstruction(keyword: string, args: string, escape = '\\', heredoc = false): string {
  if (heredoc) return `${keyword} ${args}\n`;
  const lines = args
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
  if (lines.length <= 1) return `${keyword}${lines[0] ? ` ${lines[0]}` : ''}\n`;
  return `${keyword} ${lines.join(` ${escape}\n    `)}\n`;
}

export function instructions(df: Dockerfile): DfInstruction[] {
  return df.items.filter((i): i is DfInstruction => i.kind === 'instruction');
}

/** Replaces the instruction at `index` (an index into `items`). */
export function setInstruction(df: Dockerfile, index: number, keyword: string, args: string): Dockerfile {
  const items = [...df.items];
  const old = items[index];
  const heredoc = old?.kind === 'instruction' && old.heredoc && /<</.test(args);
  const text = renderInstruction(keyword, args, df.escape, heredoc);
  items[index] = { kind: 'instruction', keyword, args, text, line: old?.line ?? 0, heredoc };
  return { ...df, items };
}

export function insertInstruction(df: Dockerfile, at: number, keyword: string, args: string): Dockerfile {
  const items = [...df.items];
  // The file might not end with a line break yet.
  const prev = items[at - 1];
  if (prev && !prev.text.endsWith('\n')) items[at - 1] = { ...prev, text: `${prev.text}\n` };
  items.splice(at, 0, { kind: 'instruction', keyword, args, text: renderInstruction(keyword, args, df.escape), line: 0, heredoc: false });
  return { ...df, items };
}

/** First item of an instruction's block: the comment lines directly above it belong to it. */
function blockStart(items: DfItem[], index: number): number {
  let from = index;
  while (from > 0 && items[from - 1].kind === 'comment') from--;
  return from;
}

/** Removes an instruction with the comment lines directly above it. */
export function removeInstruction(df: Dockerfile, index: number): Dockerfile {
  const items = [...df.items];
  const from = blockStart(items, index);
  items.splice(from, index - from + 1);
  return { ...df, items };
}

/**
 * Moves an instruction (with its comments) next to another one: above it
 * when moving up, below it when moving down.
 */
export function moveInstruction(df: Dockerfile, index: number, target: number): Dockerfile {
  if (index === target) return df;
  const items = df.items.map((it, i, all) => (i === all.length - 1 && !it.text.endsWith('\n') ? { ...it, text: `${it.text}\n` } : it));
  const from = blockStart(items, index);
  const block = items.slice(from, index + 1);
  const rest = [...items.slice(0, from), ...items.slice(index + 1)];
  const t = target > index ? target - block.length : target;
  const at = target > index ? t + 1 : blockStart(rest, t);
  rest.splice(at, 0, ...block);
  return { ...df, items: rest };
}

// ---------------------------------------------------------- argument forms

/** `["a", "b"]` exec form, or null for the shell form. */
export function parseExec(args: string): string[] | null {
  const t = args.trim();
  if (!t.startsWith('[')) return null;
  try {
    const v = JSON.parse(t);
    return Array.isArray(v) && v.every((x) => typeof x === 'string') ? v : null;
  } catch {
    return null;
  }
}

export function renderExec(list: string[]): string {
  return `[${list.map((x) => JSON.stringify(x)).join(', ')}]`;
}

export interface FromArgs {
  platform: string;
  image: string;
  alias: string;
}

export function parseFrom(args: string): FromArgs {
  const words = args.trim().split(/\s+/);
  let platform = '';
  const rest: string[] = [];
  for (const w of words) {
    if (w.startsWith('--platform=')) platform = w.slice(11);
    else if (w) rest.push(w);
  }
  const as = rest.findIndex((w) => w.toUpperCase() === 'AS');
  return { platform, image: rest[0] ?? '', alias: as > 0 ? (rest[as + 1] ?? '') : '' };
}

export function renderFrom(f: FromArgs): string {
  return [f.platform ? `--platform=${f.platform}` : '', f.image, f.alias ? `AS ${f.alias}` : ''].filter(Boolean).join(' ');
}

/** Splits words the way the Dockerfile does: quotes group, backslash escapes. */
export function splitWords(s: string): string[] {
  const out: string[] = [];
  let cur = '';
  let q: string | null = null;
  let any = false;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) {
      if (c === q) q = null;
      else if (c === '\\' && q === '"' && i + 1 < s.length) cur += s[++i];
      else cur += c;
    } else if (c === '"' || c === "'") {
      q = c;
      any = true;
    } else if (c === '\\' && i + 1 < s.length) {
      cur += s[++i];
      any = true;
    } else if (/\s/.test(c)) {
      if (any) out.push(cur);
      cur = '';
      any = false;
    } else {
      cur += c;
      any = true;
    }
  }
  if (any) out.push(cur);
  return out;
}

function quoteWord(w: string): string {
  return w === '' || /[\s"'\\$]/.test(w) ? JSON.stringify(w) : w;
}

export interface CopyArgs {
  /** --from, --chown, --chmod, --link... (value '' for switches). */
  flags: { name: string; value: string }[];
  sources: string[];
  dest: string;
}

export function parseCopy(args: string): CopyArgs {
  const exec = parseExec(args);
  const words = exec ?? splitWords(args.replace(/\n/g, ' '));
  const flags: { name: string; value: string }[] = [];
  const rest: string[] = [];
  for (const w of words) {
    if (!exec && w.startsWith('--') && !rest.length) {
      const eq = w.indexOf('=');
      flags.push(eq < 0 ? { name: w.slice(2), value: '' } : { name: w.slice(2, eq), value: w.slice(eq + 1) });
    } else rest.push(w);
  }
  return { flags, sources: rest.slice(0, -1), dest: rest[rest.length - 1] ?? '' };
}

export function renderCopy(c: CopyArgs): string {
  const flags = c.flags.filter((f) => f.name).map((f) => (f.value ? `--${f.name}=${f.value}` : `--${f.name}`));
  return [...flags, ...c.sources.filter(Boolean).map(quoteWord), quoteWord(c.dest)].join(' ');
}

export interface Pair {
  key: string;
  value: string;
}

/** ENV / LABEL / ARG pairs, including the old `ENV KEY value` form. */
export function parsePairs(keyword: string, args: string): Pair[] {
  const flat = args.replace(/\n/g, ' ').trim();
  if (keyword === 'ENV' && /^[^\s=]+\s/.test(flat) && !/^[^\s=]+=/.test(flat)) {
    const sp = flat.search(/\s/);
    return [{ key: flat.slice(0, sp), value: flat.slice(sp).trim() }];
  }
  return splitWords(flat).map((w) => {
    const eq = w.indexOf('=');
    return eq < 0 ? { key: w, value: '' } : { key: w.slice(0, eq), value: w.slice(eq + 1) };
  });
}

/** One pair per line, so long lists stay readable. */
export function renderPairs(keyword: string, pairs: Pair[]): string {
  const items = pairs
    .filter((p) => p.key)
    .map((p) => (keyword === 'ARG' && p.value === '' ? p.key : `${p.key}=${p.value === '' || /[\s"'\\]/.test(p.value) ? JSON.stringify(p.value) : p.value}`));
  return items.join('\n');
}

export interface HealthArgs {
  none: boolean;
  interval: string;
  timeout: string;
  startPeriod: string;
  startInterval: string;
  retries: string;
  /** The command after CMD: exec list or shell text. */
  exec: string[] | null;
  shell: string;
}

export function parseHealth(args: string): HealthArgs {
  const flat = args.replace(/\n/g, ' ').trim();
  const h: HealthArgs = { none: false, interval: '', timeout: '', startPeriod: '', startInterval: '', retries: '', exec: null, shell: '' };
  if (/^NONE$/i.test(flat)) return { ...h, none: true };
  let rest = flat;
  for (;;) {
    const m = /^--([a-z-]+)=(\S+)\s*/.exec(rest);
    if (!m) break;
    const key = { interval: 'interval', timeout: 'timeout', 'start-period': 'startPeriod', 'start-interval': 'startInterval', retries: 'retries' }[m[1]] as keyof HealthArgs | undefined;
    if (key) (h as unknown as Record<string, string>)[key] = m[2];
    rest = rest.slice(m[0].length);
  }
  rest = rest.replace(/^CMD\s+/i, '');
  h.exec = parseExec(rest);
  if (!h.exec) h.shell = rest;
  return h;
}

export function renderHealth(h: HealthArgs): string {
  if (h.none) return 'NONE';
  const opts = [
    h.interval && `--interval=${h.interval}`,
    h.timeout && `--timeout=${h.timeout}`,
    h.startPeriod && `--start-period=${h.startPeriod}`,
    h.startInterval && `--start-interval=${h.startInterval}`,
    h.retries && `--retries=${h.retries}`,
  ].filter(Boolean);
  return [...opts, 'CMD', h.exec ? renderExec(h.exec) : h.shell].join(' ');
}

// ------------------------------------------------------------------ stages

export interface Stage {
  /** Index in `items` of the FROM instruction (-1 for instructions before any FROM). */
  from: number;
  name: string;
  image: string;
  /** Indexes in `items` of the instructions of this stage. */
  members: number[];
}

export function stages(df: Dockerfile): Stage[] {
  const out: Stage[] = [];
  df.items.forEach((it, idx) => {
    if (it.kind !== 'instruction') return;
    if (it.keyword === 'FROM') {
      const f = parseFrom(it.args);
      out.push({ from: idx, name: f.alias, image: f.image, members: [idx] });
    } else {
      if (!out.length) out.push({ from: -1, name: '', image: '', members: [] });
      out[out.length - 1].members.push(idx);
    }
  });
  return out;
}

// ------------------------------------------------------------------- hints

export interface DfHint {
  line: number;
  level: 'tip' | 'warn';
  message: string;
}

export function lintDockerfile(df: Dockerfile): DfHint[] {
  const hints: DfHint[] = [];
  const all = instructions(df);
  if (!all.length) return hints;
  if (all[0].keyword !== 'FROM' && all[0].keyword !== 'ARG') hints.push({ line: all[0].line, level: 'warn', message: 'A Dockerfile starts with FROM (only ARG can come before it).' });
  const names = new Set<string>();
  let lastStage: DfInstruction[] = [];
  let current: DfInstruction[] = [];
  for (const ins of all) {
    if (ins.keyword === 'FROM') {
      if (current.length) lastStage = current;
      current = [ins];
      const f = parseFrom(ins.args);
      if (f.alias) names.add(f.alias.toLowerCase());
      const image = f.image;
      const isStage = names.has(image.toLowerCase()) && image.toLowerCase() !== f.alias.toLowerCase();
      if (!isStage && image !== 'scratch' && !image.includes('$') && !image.includes('@')) {
        const tag = image.slice(image.lastIndexOf('/') + 1).split(':')[1];
        if (!tag) hints.push({ line: ins.line, level: 'tip', message: `${image} has no tag, so it means :latest. Pin a version to make builds repeatable.` });
        else if (tag === 'latest') hints.push({ line: ins.line, level: 'tip', message: 'Pin a version instead of :latest to make builds repeatable.' });
      }
      continue;
    }
    current.push(ins);
    const flat = ins.args.replace(/\n/g, ' ');
    if (ins.keyword === 'MAINTAINER') hints.push({ line: ins.line, level: 'tip', message: 'MAINTAINER is deprecated; use LABEL org.opencontainers.image.authors="…" instead.' });
    if (ins.keyword === 'ADD' && !ins.heredoc) {
      const c = parseCopy(ins.args);
      if (c.sources.length && c.sources.every((s) => !/^https?:\/\//.test(s) && !/\.(tar|tgz|tar\.gz|tar\.xz|tar\.bz2)$/.test(s) && !s.startsWith('git@'))) {
        hints.push({ line: ins.line, level: 'tip', message: 'COPY is clearer for plain files; ADD also downloads URLs and unpacks archives.' });
      }
    }
    if (ins.keyword === 'RUN') {
      if (/\bapt-get\s+(?:-\S+\s+)*install\b/.test(flat)) {
        if (!/--no-install-recommends/.test(flat)) hints.push({ line: ins.line, level: 'tip', message: 'apt-get install --no-install-recommends keeps the image smaller.' });
        if (!/rm\s+-rf\s+\/var\/lib\/apt\/lists/.test(flat) && !ins.heredoc) hints.push({ line: ins.line, level: 'tip', message: 'Remove /var/lib/apt/lists/* in the same RUN to keep the package index out of the image.' });
        if (!/apt-get\s+update/.test(flat)) hints.push({ line: ins.line, level: 'warn', message: 'Run apt-get update in the same RUN as install, or old package lists get cached.' });
      }
      if (/\bapt-get\s+(?:-\S+\s+)*(?:dist-)?upgrade\b/.test(flat)) hints.push({ line: ins.line, level: 'tip', message: 'Upgrading every package in a build is better done by moving to a newer base image.' });
      if (/(^|&&|;)\s*cd\s+\S+\s*($|&&|;)/.test(flat) && !/&&/.test(flat.split(/\bcd\b/)[1] ?? '')) hints.push({ line: ins.line, level: 'tip', message: 'Use WORKDIR to change folders; a cd only lasts for this RUN.' });
    }
    if ((ins.keyword === 'CMD' || ins.keyword === 'ENTRYPOINT') && !parseExec(ins.args) && ins.args.trim()) {
      hints.push({ line: ins.line, level: 'tip', message: `${ins.keyword} in exec form (["app", "--flag"]) lets the app receive stop signals directly.` });
    }
  }
  const final = current.length ? current : lastStage;
  const cmds = final.filter((i) => i.keyword === 'CMD');
  if (cmds.length > 1) hints.push({ line: cmds[0].line, level: 'warn', message: 'Only the last CMD counts; the earlier ones do nothing.' });
  if (final.length && !final.some((i) => i.keyword === 'USER')) {
    hints.push({ line: final[0].line, level: 'tip', message: 'The container runs as root. Add a USER if the app works without it.' });
  }
  return hints.sort((a, b) => a.line - b.line);
}

/** What each instruction does, for the "add" menu and tooltips. */
export const KEYWORD_HELP: Record<string, string> = {
  FROM: 'Start from a base image',
  RUN: 'Run commands while building',
  COPY: 'Copy files into the image',
  ADD: 'Copy files, URLs or archives',
  ENV: 'Set environment variables',
  ARG: 'Build-time variables',
  WORKDIR: 'Set the working folder',
  USER: 'Run as this user',
  EXPOSE: 'Document the ports the app listens on',
  VOLUME: 'Declare folders that hold data',
  LABEL: 'Add metadata',
  CMD: 'Default command',
  ENTRYPOINT: 'The program the container runs',
  HEALTHCHECK: 'How to check the app is healthy',
  SHELL: 'Shell used by RUN',
  STOPSIGNAL: 'Signal sent to stop the app',
  ONBUILD: 'Run when used as a base image',
  MAINTAINER: 'Deprecated author field',
};
