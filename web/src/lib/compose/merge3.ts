// Line-based three-way merge (diff3).
//
// The visual editors change files by re-serializing a parsed document, which
// normalizes some formatting (comment spacing, blank lines, indentation of
// sequences). To keep the user's formatting, the edit is replayed as a merge:
// `base` is the normalized original, `ours` the original as written, and
// `theirs` the normalized edited file. Lines the edit didn't touch come from
// `ours`; changed lines come from `theirs`.

import { diffSeq } from '../diff';

export interface MergeResult {
  lines: string[];
  /** Chunks both sides changed differently (settled by the resolver, else `theirs`). */
  conflicts: number;
}

/** For each line of `a`, the index of the line it matches in `b`, or -1. */
function matching(a: string[], b: string[]): Int32Array {
  const m = new Int32Array(a.length).fill(-1);
  let i = 0;
  let j = 0;
  for (const d of diffSeq(a, b)) {
    if (d.op === ' ') m[i++] = j++;
    else if (d.op === '-') i++;
    else j++;
  }
  return m;
}

function same(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((l, i) => l === b[i]);
}

/**
 * Settles a chunk both sides changed; returns null to take `theirs`.
 * Receives the base, our and their lines of the chunk.
 */
export type Resolver = (base: string[], ours: string[], theirs: string[]) => string[] | null;

export function merge3(base: string[], ours: string[], theirs: string[], resolve?: Resolver): MergeResult {
  const mo = matching(base, ours);
  const mt = matching(base, theirs);
  const lines: string[] = [];
  let conflicts = 0;
  let b = 0;
  let o = 0;
  let t = 0;
  while (b < base.length || o < ours.length || t < theirs.length) {
    // A stable run: the same lines, unchanged on both sides.
    let n = 0;
    while (b + n < base.length && mo[b + n] === o + n && mt[b + n] === t + n) n++;
    if (n > 0) {
      for (let k = 0; k < n; k++) lines.push(ours[o + k]);
      b += n;
      o += n;
      t += n;
      continue;
    }
    // An unstable chunk ends at the next base line both sides still have.
    let k = b;
    while (k < base.length && (mo[k] < 0 || mt[k] < 0)) k++;
    const oEnd = k < base.length ? mo[k] : ours.length;
    const tEnd = k < base.length ? mt[k] : theirs.length;
    const B = base.slice(b, k);
    const O = ours.slice(o, oEnd);
    const T = theirs.slice(t, tEnd);
    if (same(O, B)) lines.push(...T);
    else if (same(T, B) || same(O, T)) lines.push(...O);
    else {
      lines.push(...(resolve?.(B, O, T) ?? T));
      conflicts++;
    }
    b = k;
    o = oEnd;
    t = tEnd;
  }
  return { lines, conflicts };
}
