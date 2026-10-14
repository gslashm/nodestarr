//! Monitoring: resource history, Docker host stats, alerts and uptime checks.

pub mod alerts;
pub mod host;
pub mod stats;
pub mod uptime;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::{Mutex, RwLock};
use tokio::sync::{Notify, mpsc};

use crate::app::App;
use crate::db::Db;
use crate::docker::stats::RawSample;
use crate::events::EventBus;

#[derive(Clone)]
pub struct MonitorService {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    #[allow(dead_code)]
    pub db: Db,
    #[allow(dead_code)]
    pub bus: EventBus,
    pub prev: Mutex<HashMap<String, RawSample>>,
    pub latest: RwLock<HashMap<String, stats::LiveStat>>,
    pub live_viewers: AtomicUsize,
    pub wake: Arc<Notify>,
    pub alerts: Mutex<alerts::AlertState>,
    /// Alert ids to dismiss, handled by the alert engine's own loop so
    /// dismissals never race with an evaluation in progress.
    pub dismiss_tx: mpsc::UnboundedSender<i64>,
    pub dismiss_rx: Mutex<Option<mpsc::UnboundedReceiver<i64>>>,
}

impl MonitorService {
    pub fn new(db: Db, bus: EventBus) -> Self {
        let (dismiss_tx, dismiss_rx) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(Inner {
                db,
                bus,
                prev: Mutex::new(HashMap::new()),
                latest: RwLock::new(HashMap::new()),
                live_viewers: AtomicUsize::new(0),
                wake: Arc::new(Notify::new()),
                alerts: Mutex::new(alerts::AlertState::default()),
                dismiss_tx,
                dismiss_rx: Mutex::new(Some(dismiss_rx)),
            }),
        }
    }

    /// Asks the alert engine to close an alert.
    pub fn dismiss_alert(&self, id: i64) {
        let _ = self.inner.dismiss_tx.send(id);
    }

    /// Registers a live viewer; stats are sampled quickly until the guard drops.
    pub fn watch(&self) -> ViewerGuard {
        self.inner.live_viewers.fetch_add(1, Ordering::Relaxed);
        self.inner.wake.notify_one();
        ViewerGuard { inner: self.inner.clone() }
    }

    pub fn viewers(&self) -> usize {
        self.inner.live_viewers.load(Ordering::Relaxed)
    }
}

pub struct ViewerGuard {
    inner: Arc<Inner>,
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.inner.live_viewers.fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn start(app: App) {
    tokio::spawn(stats::run(app.clone()));
    tokio::spawn(alerts::run(app));
}
