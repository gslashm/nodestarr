// Converts ANSI color codes in log output to HTML spans (input is escaped
// first, so log content can't inject markup).

const FG: Record<number, string> = {
  30: 'k', 31: 'r', 32: 'g', 33: 'y', 34: 'b', 35: 'm', 36: 'c', 37: 'w',
  90: 'K', 91: 'R', 92: 'G', 93: 'Y', 94: 'B', 95: 'M', 96: 'C', 97: 'W',
};

export function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// eslint-disable-next-line no-control-regex
const SGR = /\u001b\[([0-9;]*)m/g;
// eslint-disable-next-line no-control-regex
const OTHER_ESC = /\u001b\[[0-9;?]*[A-Za-z]|\u001b\][^\u0007]*\u0007|\r/g;

export function hasAnsi(s: string): boolean {
  return s.includes('\u001b[');
}

/** HTML for one line, with spans for colors and bold. */
export function ansiToHtml(line: string): string {
  if (!hasAnsi(line)) return escapeHtml(line.replace(/\r/g, ''));
  let out = '';
  let last = 0;
  let open = false;
  let fg = '';
  let bold = false;
  const text = line.replace(OTHER_ESC, (m) => (m.startsWith('\u001b[') && m.endsWith('m') ? m : ''));
  for (const m of text.matchAll(SGR)) {
    out += escapeHtml(text.slice(last, m.index));
    last = (m.index ?? 0) + m[0].length;
    const codes = m[1] === '' ? [0] : m[1].split(';').map(Number);
    for (const c of codes) {
      if (c === 0) {
        fg = '';
        bold = false;
      } else if (c === 1) bold = true;
      else if (c === 22) bold = false;
      else if (c === 39) fg = '';
      else if (FG[c]) fg = FG[c];
    }
    if (open) out += '</span>';
    open = !!fg || bold;
    if (open) out += `<span class="${[fg && `a-${fg}`, bold && 'a-bold'].filter(Boolean).join(' ')}">`;
  }
  out += escapeHtml(text.slice(last));
  if (open) out += '</span>';
  return out;
}

/** Strips ANSI codes (for copying and searching). */
export function stripAnsi(s: string): string {
  return s.replace(SGR, '').replace(OTHER_ESC, '');
}

/**
 * Rough severity of a log line, from common level markers near its start
 * ("ERROR", "[warn]", "level=error", ...). Used for coloring and for the
 * "problems only" filter.
 */
export function logLevel(t: string): 'error' | 'warn' | '' {
  const head = t.slice(0, 160);
  if (/\b(error|fatal|panic|critical|exception)\b|\[(err|error|fatal)\]|level=(error|fatal)|\bERR\b/i.test(head)) return 'error';
  if (/\b(warn|warning)\b|\[warn\]|level=warn/i.test(head)) return 'warn';
  return '';
}
