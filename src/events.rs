//! In-process event bus.
//!
//! Services publish what changed; the browser's live connection (SSE) and
//! internal consumers (Home Assistant bridge, alert engine) subscribe. A
//! broadcast channel means slow consumers drop old events instead of
//! stalling publishers; they resynchronize with a fresh snapshot.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;

use crate::activity::ActivityEntry;
use crate::docker::model::ContainerSummary;
use crate::integrations::AppStatus;
use crate::monitor::alerts::AlertView;
use crate::monitor::host::HostStats;
use crate::monitor::stats::LiveStat;
use crate::monitor::uptime::UptimeStatus;
use crate::settings::Severity;
use crate::tasks::TaskSnapshot;
use crate::updates::UpdateInfo;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// A container appeared or changed.
    Container(Arc<ContainerSummary>),
    /// A container was removed.
    ContainerRemoved { id: String, identity: String },
    /// Compose stacks changed on disk or in Docker; clients should refetch.
    StacksChanged,
    Task(Arc<TaskSnapshot>),
    TaskLog { id: String, lines: Vec<String> },
    Activity(Arc<ActivityEntry>),
    Update { identity: String, info: Arc<UpdateInfo> },
    /// An update check of all containers started or finished.
    UpdateCheck { running: bool, last_check: Option<i64> },
    Stats(Arc<Vec<LiveStat>>),
    /// Docker host CPU, memory and load (sent with live stats).
    Host(Arc<HostStats>),
    Uptime(Arc<UptimeStatus>),
    App { identity: String, status: Arc<AppStatus> },
    Alert(Arc<AlertView>),
    Toast { level: Severity, message: String },
    /// Settings were saved; clients refetch what they display.
    Settings,
    /// Per-container preferences changed.
    Prefs { identity: String },
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    /// Publishes an event; a no-op when nobody listens.
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub fn toast(&self, level: Severity, message: impl Into<String>) {
        self.publish(Event::Toast {
            level,
            message: message.into(),
        });
    }
}
