//! Logging setup: human-readable logs on stdout plus an in-memory ring of
//! recent entries that the System > Logs page can show without shell access.

use std::fmt::Write as _;
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

use crate::util::ring::Ring;

const RING_CAPACITY: usize = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Unix milliseconds.
    pub ts: i64,
    pub level: &'static str,
    pub target: String,
    pub message: String,
}

fn ring() -> &'static Mutex<Ring<LogEntry>> {
    static RING: OnceLock<Mutex<Ring<LogEntry>>> = OnceLock::new();
    RING.get_or_init(|| Mutex::new(Ring::new(RING_CAPACITY)))
}

/// Most recent log entries, oldest first, optionally filtered by minimum level.
pub fn recent(limit: usize, min_level: Option<Level>) -> Vec<LogEntry> {
    let guard = ring().lock();
    let iter = guard.iter().filter(|e| match min_level {
        Some(min) => level_rank(e.level) >= level_rank(min.as_str()),
        None => true,
    });
    let all: Vec<_> = iter.cloned().collect();
    let skip = all.len().saturating_sub(limit);
    all.into_iter().skip(skip).collect()
}

fn level_rank(level: &str) -> u8 {
    match level {
        "TRACE" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "WARN" => 3,
        "ERROR" => 4,
        _ => 2,
    }
}

/// Installs the global subscriber. Safe to call once.
pub fn init(filter: &str) {
    let env_filter = EnvFilter::try_new(filter).or_else(|_| EnvFilter::try_new("info")).expect("valid default filter");
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_ansi(std::env::var_os("NO_COLOR").is_none() && atty_stdout())
        .compact();
    let _ = tracing_subscriber::registry().with(env_filter).with(stdout_layer).with(RingLayer).try_init();
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

struct RingLayer;

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let entry = LogEntry {
            ts: crate::util::now_ms(),
            level: meta.level().as_str(),
            target: meta.target().to_string(),
            message: visitor.finish(),
        };
        ring().lock().push(entry);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl MessageVisitor {
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else if self.message.is_empty() {
            self.fields.trim_start().to_string()
        } else {
            format!("{}{}", self.message, self.fields)
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={:?}", field.name(), value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={}", field.name(), value);
        }
    }
}
