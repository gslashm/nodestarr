// Line diff for showing what changed between two versions of a file.
//
// Trims the common prefix and suffix, then runs a longest-common-subsequence
// table over the rest. Config files are small and changes are local, so the
// table stays small; very large rewrites fall back to "replace everything".

export interface DiffLine {
  op: ' ' | '+' | '-';
  text: string;
}

export interface Hunk {
  /** 1-based first line in the old text. */
  oldStart: number;
  /** 1-based first line in the new text. */
  newStart: number;
  lines: DiffLine[];
}

/** Largest table (old lines x new lines) the LCS step will build. */
const MAX_CELLS = 2_500_000;

function splitLines(s: string): string[] {
  if (s === '') return [];
  const lines = s.replace(/\r\n?/g, '\n').split('\n');
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

export function diffLines(before: string, after: string): DiffLine[] {
  const a = splitLines(before);
  const b = splitLines(after);
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const out: DiffLine[] = a.slice(0, start).map((text) => ({ op: ' ', text }));
  const midA = a.slice(start, endA);
  const midB = b.slice(start, endB);
  const n = midA.length;
  const m = midB.length;

  if (n * m > MAX_CELLS) {
    for (const text of midA) out.push({ op: '-', text });
    for (const text of midB) out.push({ op: '+', text });
  } else if (n > 0 || m > 0) {
    // lcs[i][j] = length of the LCS of midA[i..] and midB[j..].
    const width = m + 1;
    const lcs = n < 65_535 && m < 65_535 ? new Uint16Array((n + 1) * width) : new Uint32Array((n + 1) * width);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        lcs[i * width + j] = midA[i] === midB[j] ? lcs[(i + 1) * width + j + 1] + 1 : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (midA[i] === midB[j]) {
        out.push({ op: ' ', text: midA[i] });
        i++;
        j++;
      } else if (lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]) {
        out.push({ op: '-', text: midA[i++] });
      } else {
        out.push({ op: '+', text: midB[j++] });
      }
    }
    while (i < n) out.push({ op: '-', text: midA[i++] });
    while (j < m) out.push({ op: '+', text: midB[j++] });
  }

  for (const text of a.slice(endA)) out.push({ op: ' ', text });
  return out;
}

/** Groups a diff into hunks with `context` unchanged lines around each change. */
export function hunks(lines: DiffLine[], context = 3): Hunk[] {
  // Line ranges to show, merged when they touch or overlap.
  const ranges: [number, number][] = [];
  lines.forEach((l, k) => {
    if (l.op === ' ') return;
    const from = Math.max(0, k - context);
    const to = Math.min(lines.length - 1, k + context);
    const last = ranges[ranges.length - 1];
    if (last && from <= last[1] + 1) last[1] = Math.max(last[1], to);
    else ranges.push([from, to]);
  });
  const out: Hunk[] = [];
  let k = 0;
  let oldLine = 1;
  let newLine = 1;
  for (const [from, to] of ranges) {
    for (; k < from; k++) {
      if (lines[k].op !== '+') oldLine++;
      if (lines[k].op !== '-') newLine++;
    }
    out.push({ oldStart: oldLine, newStart: newLine, lines: lines.slice(from, to + 1) });
  }
  return out;
}


/** One displayed row of a diff. */
export interface DiffRow {
  kind: 'add' | 'del' | 'ctx' | 'hunk';
  text: string;
  /** Line number in the old text (absent for added lines and hunk headers). */
  old?: number;
  /** Line number in the new text (absent for removed lines and hunk headers). */
  new?: number;
}

/** Display rows for a computed diff: hunks with `context` unchanged lines. */
export function diffRows(lines: DiffLine[], context = 3): DiffRow[] {
  const out: DiffRow[] = [];
  for (const h of hunks(lines, context)) {
    out.push({ kind: 'hunk', text: `Line ${h.newStart}` });
    let o = h.oldStart;
    let n = h.newStart;
    for (const l of h.lines) {
      if (l.op === '+') out.push({ kind: 'add', text: l.text, new: n++ });
      else if (l.op === '-') out.push({ kind: 'del', text: l.text, old: o++ });
      else out.push({ kind: 'ctx', text: l.text, old: o++, new: n++ });
    }
  }
  return out;
}

/**
 * Display rows for a unified diff (the "diff -u" format the server sends).
 * File headers ("--- a/x", "+++ b/x") and "\ No newline at end of file"
 * markers are dropped, since callers show the path, and each hunk header
 * becomes "Line N" so it reads the same as a diff computed here.
 */
export function parseUnified(text: string): DiffRow[] {
  const out: DiffRow[] = [];
  let o = 0;
  let n = 0;
  // Lines left in the current hunk, from its header. Outside a hunk, lines
  // are file headers.
  let oldLeft = 0;
  let newLeft = 0;
  for (const l of text.replace(/\n$/, '').split('\n')) {
    const h = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/.exec(l);
    if (h) {
      o = Number(h[1]);
      oldLeft = h[2] === undefined ? 1 : Number(h[2]);
      n = Number(h[3]);
      newLeft = h[4] === undefined ? 1 : Number(h[4]);
      out.push({ kind: 'hunk', text: `Line ${n}${h[5] ? `  ${h[5]}` : ''}` });
    } else if (oldLeft + newLeft === 0 || l.startsWith('\\')) {
      continue;
    } else if (l.startsWith('+')) {
      out.push({ kind: 'add', text: l.slice(1), new: n++ });
      newLeft--;
    } else if (l.startsWith('-')) {
      out.push({ kind: 'del', text: l.slice(1), old: o++ });
      oldLeft--;
    } else {
      out.push({ kind: 'ctx', text: l.slice(1), old: o++, new: n++ });
      oldLeft--;
      newLeft--;
    }
  }
  return out;
}
