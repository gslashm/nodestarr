//! Container log reading and streaming.
//!
//! Docker returns logs as chunks that do not respect line boundaries (TTY
//! containers especially), so chunks are re-split into lines per stream.

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::query_parameters::LogsOptions;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    /// RFC 3339 timestamp from Docker (when requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    /// "out", "err" or "tty".
    pub s: &'static str,
    pub t: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogQuery {
    /// Lines from the end ("all" or a number). Default 500.
    pub tail: Option<String>,
    /// Unix seconds.
    pub since: Option<i64>,
    pub until: Option<i64>,
    #[serde(deserialize_with = "crate::util::flag", default = "default_true")]
    pub timestamps: bool,
    #[serde(deserialize_with = "crate::util::flag", default = "default_true")]
    pub stdout: bool,
    #[serde(deserialize_with = "crate::util::flag", default = "default_true")]
    pub stderr: bool,
}

fn default_true() -> bool {
    true
}

impl Default for LogQuery {
    fn default() -> Self {
        Self { tail: Some("500".into()), since: None, until: None, timestamps: true, stdout: true, stderr: true }
    }
}

impl LogQuery {
    fn options(&self, follow: bool) -> LogsOptions {
        let tail = match self.tail.as_deref() {
            Some("all") => "all".to_string(),
            Some(n) => n.parse::<u32>().map(|n| n.min(100_000).to_string()).unwrap_or_else(|_| "500".into()),
            None => "500".to_string(),
        };
        LogsOptions {
            follow,
            stdout: self.stdout,
            stderr: self.stderr,
            since: self.since.unwrap_or(0).clamp(0, i32::MAX as i64) as i32,
            until: self.until.unwrap_or(0).clamp(0, i32::MAX as i64) as i32,
            timestamps: self.timestamps,
            tail,
        }
    }
}

/// Splits chunked output into lines, keeping partial lines until completed.
#[derive(Default)]
struct Splitter {
    out: String,
    err: String,
    tty: String,
}

impl Splitter {
    fn push(&mut self, chunk: LogOutput, timestamps: bool, lines: &mut Vec<LogLine>) {
        let (buf, kind, bytes) = match chunk {
            LogOutput::StdOut { message } => (&mut self.out, "out", message),
            LogOutput::StdErr { message } => (&mut self.err, "err", message),
            LogOutput::Console { message } => (&mut self.tty, "tty", message),
            LogOutput::StdIn { .. } => return,
        };
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(pos) = buf.find('\n') {
            let mut line: String = buf.drain(..=pos).collect();
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
            lines.push(make_line(kind, line, timestamps));
        }
        // Guard against unbounded partial lines (binary output).
        if buf.len() > 64 * 1024 {
            let line = std::mem::take(buf);
            lines.push(make_line(kind, line, timestamps));
        }
    }

    fn finish(&mut self, timestamps: bool, lines: &mut Vec<LogLine>) {
        for (kind, buf) in [("out", &mut self.out), ("err", &mut self.err), ("tty", &mut self.tty)] {
            if !buf.is_empty() {
                let line = std::mem::take(buf);
                lines.push(make_line(kind, line, timestamps));
            }
        }
    }
}

fn make_line(kind: &'static str, raw: String, timestamps: bool) -> LogLine {
    if timestamps {
        if let Some((ts, rest)) = raw.split_once(' ') {
            if ts.len() >= 20 && ts.as_bytes().get(4) == Some(&b'-') {
                return LogLine { ts: Some(ts.to_string()), s: kind, t: rest.to_string() };
            }
        }
    }
    LogLine { ts: None, s: kind, t: raw }
}

/// Reads logs without following.
pub async fn fetch(api: &Docker, id: &str, q: &LogQuery) -> crate::error::Result<Vec<LogLine>> {
    let mut stream = api.logs(id, Some(q.options(false)));
    let mut splitter = Splitter::default();
    let mut lines = Vec::new();
    while let Some(chunk) = stream.next().await {
        splitter.push(chunk?, q.timestamps, &mut lines);
    }
    splitter.finish(q.timestamps, &mut lines);
    Ok(lines)
}

/// Follows logs, yielding batches of lines as they arrive.
pub fn follow(api: &Docker, id: &str, q: &LogQuery) -> impl Stream<Item = Result<Vec<LogLine>, String>> + Send + use<> {
    let inner = api.logs(id, Some(q.options(true)));
    let timestamps = q.timestamps;
    futures_util::stream::unfold(
        (inner, Splitter::default(), false),
        move |(mut inner, mut splitter, done)| async move {
            if done {
                return None;
            }
            match inner.next().await {
                Some(Ok(chunk)) => {
                    let mut lines = Vec::new();
                    splitter.push(chunk, timestamps, &mut lines);
                    Some((Ok(lines), (inner, splitter, false)))
                }
                Some(Err(err)) => Some((Err(err.to_string()), (inner, splitter, true))),
                None => {
                    let mut lines = Vec::new();
                    splitter.finish(timestamps, &mut lines);
                    Some((Ok(lines), (inner, splitter, true)))
                }
            }
        },
    )
    .filter(|r| futures_util::future::ready(!matches!(r, Ok(v) if v.is_empty())))
}

/// Renders lines as plain text for download/copy.
pub fn to_text(lines: &[LogLine], with_ts: bool) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.t.len() + 32).sum());
    for l in lines {
        if with_ts {
            if let Some(ts) = &l.ts {
                out.push_str(ts);
                out.push(' ');
            }
        }
        out.push_str(&strip_ansi(&l.t));
        out.push('\n');
    }
    out
}

/// Removes ANSI escape sequences (colors) for plain-text exports.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn splits_partial_chunks() {
        let mut s = Splitter::default();
        let mut lines = Vec::new();
        s.push(LogOutput::StdOut { message: Bytes::from("2024-01-01T00:00:00.000000000Z hel") }, true, &mut lines);
        assert!(lines.is_empty());
        s.push(
            LogOutput::StdOut { message: Bytes::from("lo\r\n2024-01-01T00:00:01.000000000Z world\n") },
            true,
            &mut lines,
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].t, "hello");
        assert_eq!(lines[0].ts.as_deref(), Some("2024-01-01T00:00:00.000000000Z"));
        assert_eq!(lines[1].t, "world");
        s.push(LogOutput::StdErr { message: Bytes::from("no newline") }, false, &mut lines);
        s.finish(false, &mut lines);
        assert_eq!(lines[2].t, "no newline");
        assert_eq!(lines[2].s, "err");
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[32mINFO\u{1b}[0m ok"), "INFO ok");
    }
}
