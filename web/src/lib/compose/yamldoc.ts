// YAML documents for the visual editors: parsing, small node helpers, and
// writing an edited document back without reformatting the user's file.
//
// Edits are made on a parsed Document (comments, quoting, anchors and key
// order survive in the tree). Serializing a tree normalizes some layout,
// so the result is merged back into the original text line by line (see
// merge3.ts) and kept only if it still means exactly the same data.

import { Document, isAlias, isMap, isPair, isScalar, isSeq, parseDocument, Scalar, YAMLMap, YAMLSeq, type Node, type Pair, type ToStringOptions } from 'yaml';
import { diffSeq } from '../diff';
import { merge3 } from './merge3';

export type YDoc = Document.Parsed;
export { Scalar, YAMLMap, YAMLSeq, isAlias, isMap, isPair, isScalar, isSeq };
export type { Node, Pair };

const PARSE = { merge: true, prettyErrors: true, uniqueKeys: true } as const;

export function parseYaml(text: string): YDoc {
  return parseDocument(text, PARSE);
}

/** First error of a document, with its line, for messages. */
export function yamlError(doc: YDoc): string | null {
  const e = doc.errors[0];
  if (!e) return null;
  return e.message.split('\n')[0];
}

// ---------------------------------------------------------------- layout

export interface YamlStyle {
  indent: number;
  /** Block sequences under a key are indented (`key:\n  - a`), not flush. */
  indentSeq: boolean;
  /** Flow collections are written `[ a, b ]` rather than `[a, b]`. */
  flowPadding: boolean;
}

/** Guesses the layout conventions of a file, so new lines match it. */
export function detectStyle(text: string): YamlStyle {
  let indent = 0;
  let seqIndented = 0;
  let seqFlush = 0;
  let padded = 0;
  let tight = 0;
  let prev: { ind: number; opens: boolean } | null = null;
  for (const line of text.split(/\r?\n/)) {
    const body = line.trimStart();
    if (!body || body.startsWith('#')) continue;
    const ind = line.length - body.length;
    const item = body.startsWith('- ') || body === '-';
    if (prev?.opens) {
      if (item) {
        if (ind > prev.ind) seqIndented++;
        else seqFlush++;
      } else if (!indent && ind > prev.ind) {
        indent = ind - prev.ind;
      }
    }
    // A key whose value starts on the next line: `key:` (maybe `- key:`).
    const content = item ? body.slice(2).trimStart() : body;
    const opens = /^[^\s#'"[{][^#]*?:\s*(#.*)?$/.test(content) || /^(["'])[^"']*\1:\s*(#.*)?$/.test(content);
    prev = { ind: item ? ind + (body.length - content.length) : ind, opens };
    for (const m of body.matchAll(/[[{](.)/g)) {
      if (m[1] === ']' || m[1] === '}') continue;
      if (m[1] === ' ') padded++;
      else tight++;
    }
  }
  return { indent: indent >= 1 && indent <= 8 ? indent : 2, indentSeq: seqFlush <= seqIndented, flowPadding: padded > tight };
}

function stringOptions(style: YamlStyle): ToStringOptions {
  return { indent: style.indent, indentSeq: style.indentSeq, flowCollectionPadding: style.flowPadding, lineWidth: 0, minContentWidth: 0 };
}

function splitLines(text: string): { lines: string[]; eol: boolean } {
  const lines = text.split('\n');
  const eol = lines.length > 0 && lines[lines.length - 1] === '';
  if (eol) lines.pop();
  return { lines, eol };
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== typeof b || a === null || b === null || typeof a !== 'object') {
    return typeof a === 'number' && typeof b === 'number' && Number.isNaN(a) && Number.isNaN(b);
  }
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (a instanceof Map || b instanceof Map || a instanceof Set || b instanceof Set) return false;
  const ka = Object.keys(a as object);
  const kb = Object.keys(b as object);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    if (!deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k])) return false;
  }
  return true;
}

const indentOf = (l: string) => l.length - l.trimStart().length;

function shift(line: string, delta: number): string {
  if (!delta || !line.trim()) return line;
  return delta > 0 ? ' '.repeat(delta) + line : line.slice(Math.min(-delta, indentOf(line)));
}

/** Gives an edited line the spacing before its trailing comment that the old line had. */
function keepCommentGap(old: string, edited: string): string {
  const a = /^(.*?\S)(\s+)(#.*)$/.exec(old);
  const b = /^(.*?\S)(\s+)(#.*)$/.exec(edited);
  if (!a || !b || a[3] !== b[3]) return edited;
  return b[1] + a[2] + b[3];
}

/**
 * Settles a chunk where the edit touched lines whose layout differs from the
 * normalized form: lines with the same content keep ours, new lines take the
 * indentation of their neighbours, and changed lines keep their comment gap.
 */
function resolveLayout(_base: string[], ours: string[], theirs: string[]): string[] | null {
  const norm = (l: string) => l.trim().replace(/\s+#/, ' #');
  const ops = diffSeq(ours.map(norm), theirs.map(norm));
  // Indentation difference (ours - theirs) at each matched pair, to shift new lines by.
  const deltas: (number | null)[] = [];
  {
    let i = 0;
    let j = 0;
    for (const op of ops) {
      if (op.op === ' ') deltas.push(indentOf(ours[i++]) - indentOf(theirs[j++]));
      else {
        deltas.push(null);
        if (op.op === '-') i++;
        else j++;
      }
    }
  }
  const near = (k: number) => {
    for (let d = 1; d < ops.length; d++) {
      if (deltas[k - d] !== undefined && deltas[k - d] !== null) return deltas[k - d]!;
      if (deltas[k + d] !== undefined && deltas[k + d] !== null) return deltas[k + d]!;
    }
    return null;
  };
  const out: string[] = [];
  let i = 0;
  let j = 0;
  for (let k = 0; k < ops.length; k++) {
    const op = ops[k];
    if (op.op === ' ') {
      out.push(ours[i++]);
      j++;
    } else if (op.op === '-') {
      // A removed line directly replaced by a new one is a changed line.
      const next = ops[k + 1];
      if (next?.op === '+') {
        const o = ours[i];
        const t = theirs[j];
        out.push(keepCommentGap(o, shift(t, near(k) ?? indentOf(o) - indentOf(t))));
        k++;
        j++;
      }
      i++;
    } else {
      out.push(shift(theirs[j++], near(k) ?? 0));
    }
  }
  return out;
}

/** The data a YAML text describes, or undefined when it doesn't parse. */
function dataOf(text: string): { ok: boolean; data: unknown } {
  const doc = parseYaml(text);
  if (doc.errors.length) return { ok: false, data: undefined };
  return { ok: true, data: doc.toJS({ maxAliasCount: -1 }) };
}

/**
 * Writes an edited document as text, keeping the layout of `original`
 * wherever the edit didn't change anything.
 */
export function serialize(doc: Document, original: string): string {
  const crlf = original.includes('\r\n');
  const orig = crlf ? original.replace(/\r\n/g, '\n') : original;
  const opts = stringOptions(detectStyle(orig));
  const edited = doc.toString(opts);
  let out = edited;
  if (orig.trim()) {
    const before = parseYaml(orig);
    if (!before.errors.length) {
      const base = before.toString(opts);
      if (base === edited) {
        out = orig;
      } else {
        const o = splitLines(orig);
        const want = dataOf(edited);
        // Most careful merge first; each result must mean exactly what the edit does.
        for (const resolver of [resolveLayout, undefined]) {
          const merged = merge3(splitLines(base).lines, o.lines, splitLines(edited).lines, resolver);
          const text = merged.lines.join('\n') + (o.eol || !merged.lines.length ? '\n' : '');
          const got = dataOf(text);
          if (got.ok && want.ok && deepEqual(got.data, want.data)) {
            out = text;
            break;
          }
        }
      }
    }
  }
  return crlf ? out.replace(/\n/g, '\r\n') : out;
}

/** Parses, applies `mutate`, and writes the result keeping the layout. */
export function editYaml(text: string, mutate: (doc: YDoc) => void): string {
  const doc = parseYaml(text);
  if (doc.errors.length) throw new Error(`The file has a YAML error: ${yamlError(doc)}`);
  mutate(doc);
  return serialize(doc, text);
}

// ----------------------------------------------------------------- nodes

/** `<<` merge keys (YAML anchors shared between services). */
export function isMergeKey(key: unknown): boolean {
  if (!isScalar(key)) return false;
  const v = key.value;
  return v === '<<' || (typeof v === 'symbol' && v.description === '<<');
}

function keyText(key: unknown): string | null {
  if (isScalar(key)) {
    if (typeof key.value === 'symbol') return null;
    return key.value === null ? '' : String(key.value);
  }
  return typeof key === 'string' ? key : null;
}

export function pairOf(map: YAMLMap, key: string): Pair | undefined {
  return (map.items as Pair[]).find((p) => !isMergeKey(p.key) && keyText(p.key) === key);
}

/** Keys of a mapping in order (merge keys left out). */
export function keysOf(map: YAMLMap): string[] {
  const out: string[] = [];
  for (const p of map.items as Pair[]) {
    if (isMergeKey(p.key)) continue;
    const k = keyText(p.key);
    if (k !== null) out.push(k);
  }
  return out;
}

/** Resolves an alias to the node it points at. */
export function deref(doc: Document, node: unknown): Node | undefined {
  if (isAlias(node)) return (node.resolve(doc) as Node | undefined) ?? undefined;
  return (node as Node | null | undefined) ?? undefined;
}

/** Mappings merged into `map` through `<<` keys, nearest first. */
export function mergeSources(doc: Document, map: YAMLMap): YAMLMap[] {
  const out: YAMLMap[] = [];
  for (const p of map.items as Pair[]) {
    if (!isMergeKey(p.key)) continue;
    const v = deref(doc, p.value);
    if (isMap(v)) out.push(v);
    else if (isSeq(v)) for (const it of v.items) {
      const m = deref(doc, it);
      if (isMap(m)) out.push(m);
    }
  }
  return out;
}

export interface Lookup {
  node: Node | undefined;
  /** Comes from a `<<` merge key rather than the mapping itself. */
  inherited: boolean;
}

/** A key's value, looking through merge keys like Compose does. */
export function lookup(doc: Document, map: YAMLMap, key: string): Lookup {
  const own = pairOf(map, key);
  if (own) return { node: deref(doc, own.value), inherited: false };
  for (const src of mergeSources(doc, map)) {
    const found = lookup(doc, src, key);
    if (found.node !== undefined || pairOf(src, key)) return { node: found.node, inherited: true };
  }
  return { node: undefined, inherited: false };
}

export function toJs(doc: Document, node: unknown): unknown {
  if (node === undefined || node === null) return node;
  const n = deref(doc, node);
  if (!n) return undefined;
  if (isScalar(n)) return n.value;
  return (n as YAMLMap | YAMLSeq).toJS(doc, { maxAliasCount: -1 });
}

/** The text of a scalar value ('' for null), or null when it isn't a scalar. */
export function scalarText(doc: Document, node: unknown): string | null {
  const n = deref(doc, node);
  if (!isScalar(n)) return null;
  const v = n.value;
  if (v === null || v === undefined) return '';
  return typeof v === 'symbol' ? null : String(v);
}

// YAML 1.1 parsers (and people) read these as booleans, numbers or null.
const AMBIGUOUS = /^(?:y|Y|yes|Yes|YES|n|N|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF|null|Null|NULL|~|[-+]?\d[\d_]*(?::[0-5]?\d)+(?:\.\d*)?|0[0-7]+|0o[0-7]+|0x[0-9a-fA-F]+|[-+]?(?:\d[\d_]*)?\.?\d+(?:[eE][-+]?\d+)?|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$/;

/** A string scalar, quoted when a YAML reader could take it for something else. */
export function stringNode(value: string, quote = false): Scalar {
  const s = new Scalar(value);
  if (quote || value === '' || AMBIGUOUS.test(value) || /^\s|\s$/.test(value)) s.type = Scalar.QUOTE_DOUBLE;
  return s;
}

/** Turns plain data into nodes, quoting ambiguous strings. */
export function createNode(doc: Document, value: unknown): Node {
  if (typeof value === 'string') return stringNode(value);
  if (Array.isArray(value)) {
    const seq = new YAMLSeq(doc.schema);
    for (const v of value) seq.items.push(createNode(doc, v));
    return seq;
  }
  if (value && typeof value === 'object' && !(value instanceof Scalar) && !isMap(value) && !isSeq(value)) {
    const map = new YAMLMap(doc.schema);
    for (const [k, v] of Object.entries(value)) {
      if (v === undefined) continue;
      map.items.push(doc.createPair(k, createNode(doc, v)));
    }
    return map;
  }
  if (isMap(value) || isSeq(value) || isScalar(value)) return value as Node;
  return doc.createNode(value) as Node;
}

/**
 * Sets a scalar's text, keeping its quoting and its kind: a number stays a
 * number when the new text is a number too (`1000` → `1001`, not `"1001"`).
 */
export function setScalarText(s: Scalar, text: string): void {
  const old = s.value;
  if (typeof old === 'number' && /^-?\d+(\.\d+)?$/.test(text.trim()) && s.type !== Scalar.QUOTE_DOUBLE && s.type !== Scalar.QUOTE_SINGLE) {
    s.value = Number(text);
  } else if (typeof old === 'boolean' && (text === 'true' || text === 'false')) {
    s.value = text === 'true';
  } else {
    s.value = text;
    if (s.type === Scalar.PLAIN || !s.type) {
      if (text === '' || AMBIGUOUS.test(text)) s.type = Scalar.QUOTE_DOUBLE;
    } else if (s.type === Scalar.BLOCK_FOLDED || s.type === Scalar.BLOCK_LITERAL) {
      if (!text.includes('\n')) s.type = Scalar.PLAIN;
    }
  }
}

/**
 * Gives a new pair a blank line before it when its siblings are separated
 * by blank lines (`fallback` decides when there are none to go by).
 */
export function matchSpacing(map: YAMLMap, pair: Pair, fallback = false): void {
  const others = (map.items as Pair[]).filter((p) => p !== pair).slice(1);
  const spaced = others.filter((p) => (p.key as Scalar | null)?.spaceBefore).length;
  const blank = others.length ? spaced * 2 >= others.length && spaced > 0 : fallback;
  if (blank && isScalar(pair.key) && (map.items as Pair[])[0] !== pair) pair.key.spaceBefore = true;
}

/**
 * Sets `key` in a mapping. Existing keys keep their place (and comments);
 * new keys go after the closest key that comes before them in `order`.
 * `undefined` removes the key.
 */
export function setKey(doc: Document, map: YAMLMap, key: string, value: unknown, order: readonly string[] = []): void {
  const existing = pairOf(map, key);
  if (value === undefined) {
    if (existing) map.items = (map.items as Pair[]).filter((p) => p !== existing);
    return;
  }
  const node = createNode(doc, value);
  if (existing) {
    if (isScalar(existing.value) && isScalar(node) && !isAlias(existing.value)) {
      const old = existing.value;
      const v = node.value;
      if (typeof v === 'string') setScalarText(old, v);
      else old.value = v;
      return;
    }
    existing.value = node;
    return;
  }
  const pair = doc.createPair(key, node);
  const rank = order.indexOf(key);
  const items = map.items as Pair[];
  let at = items.length - 1;
  if (rank >= 0) {
    let after = -1;
    for (let i = 0; i < items.length; i++) {
      const k = keyText(items[i].key);
      const r = k === null ? -1 : order.indexOf(k);
      if (isMergeKey(items[i].key) || (r >= 0 && r < rank)) after = i;
    }
    if (after >= 0 || items.some((p) => order.includes(keyText(p.key) ?? ''))) at = after;
  }
  items.splice(at + 1, 0, pair);
  matchSpacing(map, pair);
}

/** Gets (or creates) a nested mapping under `map`. */
export function ensureMap(doc: Document, map: YAMLMap, key: string, order: readonly string[] = []): YAMLMap {
  const p = pairOf(map, key);
  const v = p ? deref(doc, p.value) : undefined;
  if (isMap(v) && !isAlias(p?.value)) return v;
  const fresh = new YAMLMap(doc.schema);
  if (isMap(v)) for (const it of v.items as Pair[]) fresh.items.push(doc.createPair(it.key, it.value));
  if (p) p.value = fresh;
  else setKey(doc, map, key, fresh, order);
  return fresh;
}

/** Removes `key` from `map`, then removes `map` from its parent too if it's left empty. */
export function deleteIn(doc: Document, path: string[]): void {
  const maps: YAMLMap[] = [];
  let cur: unknown = doc.contents;
  for (const k of path.slice(0, -1)) {
    if (!isMap(cur)) return;
    maps.push(cur);
    cur = deref(doc, pairOf(cur, k)?.value);
  }
  if (!isMap(cur)) return;
  setKey(doc, cur, path[path.length - 1], undefined);
  // Drop mappings this left empty, innermost first.
  let child: YAMLMap = cur;
  for (let i = maps.length - 1; i >= 0; i--) {
    if (child.items.length) break;
    setKey(doc, maps[i], path[i], undefined);
    child = maps[i];
  }
}

/** `key: value` text of one setting, for editing it as YAML. */
export function pairText(doc: Document, map: YAMLMap, key: string, indent = 2): string {
  const p = pairOf(map, key);
  if (!p) return '';
  const wrap = (value: unknown) => {
    const d = new Document();
    const m = new YAMLMap(d.schema);
    m.items.push(d.createPair(key, value));
    d.contents = m;
    return d.toString({ indent, lineWidth: 0, minContentWidth: 0 });
  };
  try {
    return wrap(p.value);
  } catch {
    // Aliases to anchors elsewhere in the file: show the resolved value.
    return wrap(createNode(doc, toJs(doc, p.value)));
  }
}

/** Reads back `key: value` text written in a raw editor. */
export function parsePairText(text: string, key: string): { node: unknown } | { error: string } {
  const d = parseYaml(text);
  if (d.errors.length) return { error: yamlError(d) ?? 'Not valid YAML' };
  if (!isMap(d.contents) || d.contents.items.length !== 1 || !pairOf(d.contents, key)) {
    return { error: `Write it as "${key}: …", with only this setting` };
  }
  return { node: pairOf(d.contents, key)!.value };
}

/** Line range (1-based, inclusive) a pair occupies, for highlighting. */
export function pairLines(text: string, pair: Pair): [number, number] | null {
  const k = (pair.key as Node | null)?.range;
  const v = (pair.value as Node | null)?.range;
  if (!k) return null;
  const end = v ? v[1] : k[1];
  const line = (offset: number) => {
    let n = 1;
    for (let i = 0; i < offset && i < text.length; i++) if (text.charCodeAt(i) === 10) n++;
    return n;
  };
  return [line(k[0]), line(Math.max(k[0], end - 1))];
}
