//! Long-running operations (updates, backups, restores, compose deploys)
//! run as tasks: they stream a log and progress to the UI, can be cancelled,
//! and are recorded for later inspection.
//!
//! Tasks that touch the same container share a lock key, so an update and a
//! scheduled backup of the same app never overlap.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::params;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::activity::Actor;
use crate::db::Db;
use crate::error::{AppError, Result};
use crate::events::{Event, EventBus};
use crate::util::ring::Ring;

const LOG_LINES_KEPT: usize = 3000;
const LOG_FLUSH_MS: i64 = 250;
const FINISHED_KEPT_IN_MEMORY_SECS: i64 = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_finished(self) -> bool {
        matches!(self, TaskStatus::Succeeded | TaskStatus::Failed | TaskStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub target: Option<String>,
    pub target_name: Option<String>,
    pub status: TaskStatus,
    pub actor: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// 0.0 to 1.0 when known.
    pub progress: Option<f32>,
    /// Most recent log line, shown in the task bar.
    pub message: Option<String>,
    pub error: Option<String>,
    pub result: Option<serde_json::Value>,
}

/// Describes a task to start.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub kind: &'static str,
    pub title: String,
    pub target: Option<String>,
    pub target_name: Option<String>,
    pub actor: Actor,
    /// Tasks with the same key run one at a time (usually the container identity).
    pub lock_key: Option<String>,
}

impl TaskSpec {
    pub fn new(kind: &'static str, title: impl Into<String>, actor: &Actor) -> Self {
        Self { kind, title: title.into(), target: None, target_name: None, actor: actor.clone(), lock_key: None }
    }

    pub fn target(mut self, identity: impl Into<String>, name: impl Into<String>) -> Self {
        let identity = identity.into();
        self.lock_key = Some(identity.clone());
        self.target = Some(identity);
        self.target_name = Some(name.into());
        self
    }

    pub fn lock(mut self, key: impl Into<String>) -> Self {
        self.lock_key = Some(key.into());
        self
    }
}

struct TaskState {
    snapshot: Mutex<TaskSnapshot>,
    log: Mutex<Ring<String>>,
    pending: Mutex<Vec<String>>,
    last_flush: Mutex<i64>,
    cancel: CancellationToken,
}

/// Handle given to the task body for reporting progress.
#[derive(Clone)]
pub struct TaskCtx {
    state: Arc<TaskState>,
    bus: EventBus,
    id: String,
}

impl TaskCtx {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Appends a log line (also mirrored to Nodestarr's own log at debug level).
    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::debug!(task = %self.id, "{line}");
        let stamped = format!("{} {}", chrono::Utc::now().format("%H:%M:%S"), line);
        self.state.log.lock().push(stamped.clone());
        self.state.pending.lock().push(stamped);
        {
            let mut snap = self.state.snapshot.lock();
            snap.message = Some(crate::util::truncate(&line, 200));
        }
        let now = crate::util::now_ms();
        let due = {
            let mut last = self.state.last_flush.lock();
            if now - *last >= LOG_FLUSH_MS {
                *last = now;
                true
            } else {
                false
            }
        };
        if due {
            self.flush();
        }
    }

    /// Sends pending log lines and the current snapshot to listeners.
    pub fn flush(&self) {
        let lines = std::mem::take(&mut *self.state.pending.lock());
        if !lines.is_empty() {
            self.bus.publish(Event::TaskLog { id: self.id.clone(), lines });
        }
        let snap = self.state.snapshot.lock().clone();
        self.bus.publish(Event::Task(Arc::new(snap)));
    }

    pub fn progress(&self, fraction: f32) {
        self.state.snapshot.lock().progress = Some(fraction.clamp(0.0, 1.0));
    }

    pub fn set_title(&self, title: impl Into<String>) {
        self.state.snapshot.lock().title = title.into();
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancel.is_cancelled()
    }

    /// Returns an error if the task was cancelled; call between steps.
    pub fn check_cancelled(&self) -> Result<()> {
        if self.is_cancelled() { Err(AppError::conflict("cancelled")) } else { Ok(()) }
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.state.cancel.clone()
    }
}

#[derive(Clone)]
pub struct TaskManager {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    bus: EventBus,
    tasks: Mutex<HashMap<String, Arc<TaskState>>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl TaskManager {
    pub fn new(db: Db, bus: EventBus) -> Self {
        Self {
            inner: Arc::new(Inner { db, bus, tasks: Mutex::new(HashMap::new()), locks: Mutex::new(HashMap::new()) }),
        }
    }

    fn lock_for(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.inner.locks.lock().entry(key.to_string()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone()
    }

    /// True while a task holding `key` is queued or running.
    pub fn is_busy(&self, key: &str) -> bool {
        self.inner.tasks.lock().values().any(|t| {
            let s = t.snapshot.lock();
            !s.status.is_finished() && s.target.as_deref() == Some(key)
        })
    }

    /// Starts a task in the background and returns its id.
    pub fn spawn<F, Fut>(&self, spec: TaskSpec, body: F) -> String
    where
        F: FnOnce(TaskCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<serde_json::Value>> + Send + 'static,
    {
        let (id, fut) = self.prepare(spec, body);
        tokio::spawn(fut);
        id
    }

    /// Runs a task to completion and returns its result (still recorded and
    /// visible in the UI). Used by schedulers that process items in order.
    pub async fn run<F, Fut>(&self, spec: TaskSpec, body: F) -> Result<serde_json::Value>
    where
        F: FnOnce(TaskCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<serde_json::Value>> + Send + 'static,
    {
        let (id, fut) = self.prepare(spec, body);
        fut.await;
        let state = self.inner.tasks.lock().get(&id).cloned();
        let snap = state.map(|s| s.snapshot.lock().clone());
        match snap {
            Some(s) if s.status == TaskStatus::Succeeded => Ok(s.result.unwrap_or_default()),
            Some(s) => Err(AppError::conflict(s.error.unwrap_or_else(|| "task failed".into()))),
            None => Err(AppError::conflict("task vanished")),
        }
    }

    fn prepare<F, Fut>(&self, spec: TaskSpec, body: F) -> (String, impl Future<Output = ()> + Send + 'static)
    where
        F: FnOnce(TaskCtx) -> Fut + Send + 'static,
        Fut: Future<Output = Result<serde_json::Value>> + Send + 'static,
    {
        let id = crate::util::short_id();
        let now = crate::util::now();
        let state = Arc::new(TaskState {
            snapshot: Mutex::new(TaskSnapshot {
                id: id.clone(),
                kind: spec.kind.to_string(),
                title: spec.title.clone(),
                target: spec.target.clone(),
                target_name: spec.target_name.clone(),
                status: TaskStatus::Queued,
                actor: Some(spec.actor.0.clone()),
                created_at: now,
                started_at: None,
                finished_at: None,
                progress: None,
                message: None,
                error: None,
                result: None,
            }),
            log: Mutex::new(Ring::new(LOG_LINES_KEPT)),
            pending: Mutex::new(Vec::new()),
            last_flush: Mutex::new(0),
            cancel: CancellationToken::new(),
        });
        self.inner.tasks.lock().insert(id.clone(), state.clone());
        let ctx = TaskCtx { state: state.clone(), bus: self.inner.bus.clone(), id: id.clone() };
        ctx.flush();

        let manager = self.clone();
        let lock = spec.lock_key.as_deref().map(|k| self.lock_for(k));
        let fut = async move {
            let _guard = match &lock {
                Some(l) => Some(l.lock().await),
                None => None,
            };
            {
                let mut s = state.snapshot.lock();
                s.status = TaskStatus::Running;
                s.started_at = Some(crate::util::now());
            }
            ctx.flush();
            let outcome = if state.cancel.is_cancelled() {
                Err(AppError::conflict("cancelled"))
            } else {
                // Run the body in its own task so a panic is contained and
                // reported as a failure instead of killing the scheduler.
                match tokio::spawn(body(ctx.clone())).await {
                    Ok(res) => res,
                    Err(join) => Err(AppError::Other(anyhow::anyhow!("task panicked: {join}"))),
                }
            };
            {
                let mut s = state.snapshot.lock();
                s.finished_at = Some(crate::util::now());
                match outcome {
                    Ok(v) => {
                        s.status = TaskStatus::Succeeded;
                        s.progress = Some(1.0);
                        s.result = if v.is_null() { None } else { Some(v) };
                    }
                    Err(err) => {
                        let cancelled = state.cancel.is_cancelled();
                        s.status = if cancelled { TaskStatus::Cancelled } else { TaskStatus::Failed };
                        s.error = Some(err.to_string());
                    }
                }
            }
            if let Some(err) = state.snapshot.lock().error.clone() {
                state.log.lock().push(format!("error: {err}"));
                state.pending.lock().push(format!("error: {err}"));
            }
            ctx.flush();
            manager.persist(&state).await;
            manager.evict_old();
        };
        (id, fut)
    }

    async fn persist(&self, state: &Arc<TaskState>) {
        let snap = state.snapshot.lock().clone();
        let log = {
            let lines = state.log.lock();
            let joined: Vec<String> = lines.iter().cloned().collect();
            let text = joined.join("\n");
            // Cap stored logs at 256 KiB, keeping the end.
            if text.len() > 262_144 {
                let mut start = text.len() - 262_144;
                while !text.is_char_boundary(start) {
                    start += 1;
                }
                text[start..].to_string()
            } else {
                text
            }
        };
        let res = self
            .inner
            .db
            .call(move |c| {
                c.execute(
                    "INSERT OR REPLACE INTO tasks (id, kind, title, target, target_name, status, actor, created_at, started_at, finished_at, log, result, error)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        snap.id,
                        snap.kind,
                        snap.title,
                        snap.target,
                        snap.target_name,
                        snap.status.as_str(),
                        snap.actor,
                        snap.created_at,
                        snap.started_at,
                        snap.finished_at,
                        log,
                        snap.result.as_ref().map(|r| r.to_string()),
                        snap.error,
                    ],
                )?;
                Ok(())
            })
            .await;
        if let Err(err) = res {
            tracing::warn!(%err, "failed to persist task");
        }
    }

    fn evict_old(&self) {
        let cutoff = crate::util::now() - FINISHED_KEPT_IN_MEMORY_SECS;
        self.inner.tasks.lock().retain(|_, t| {
            let s = t.snapshot.lock();
            !s.status.is_finished() || s.finished_at.unwrap_or(i64::MAX) > cutoff
        });
    }

    /// Requests cancellation; the task stops at its next checkpoint.
    pub fn cancel(&self, id: &str) -> bool {
        match self.inner.tasks.lock().get(id) {
            Some(t) if !t.snapshot.lock().status.is_finished() => {
                t.cancel.cancel();
                true
            }
            _ => false,
        }
    }

    /// Active and recently finished tasks, newest first.
    pub async fn list(&self, limit: usize) -> Result<Vec<TaskSnapshot>> {
        let mut live: Vec<TaskSnapshot> = self.inner.tasks.lock().values().map(|t| t.snapshot.lock().clone()).collect();
        let known: std::collections::HashSet<String> = live.iter().map(|t| t.id.clone()).collect();
        let stored = self
            .inner
            .db
            .call(move |c| {
                let mut stmt =
                    c.prepare(&format!("SELECT {STORED_COLUMNS} FROM tasks ORDER BY created_at DESC LIMIT ?1"))?;
                let rows = stmt.query_map([limit as i64], stored_snapshot)?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await?;
        live.extend(stored.into_iter().filter(|t| !known.contains(&t.id)));
        live.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
        live.truncate(limit);
        Ok(live)
    }

    /// One task with its full log.
    pub async fn get(&self, id: &str) -> Result<(TaskSnapshot, Vec<String>)> {
        if let Some(t) = self.inner.tasks.lock().get(id).cloned() {
            let snap = t.snapshot.lock().clone();
            let log = t.log.lock().iter().cloned().collect();
            return Ok((snap, log));
        }
        let id = id.to_string();
        let row = self
            .inner
            .db
            .call(move |c| {
                use rusqlite::OptionalExtension;
                Ok(c.query_row(&format!("SELECT {STORED_COLUMNS}, log FROM tasks WHERE id = ?1"), [&id], |r| {
                    let log: Option<String> = r.get(12)?;
                    Ok((stored_snapshot(r)?, log.unwrap_or_default()))
                })
                .optional()?)
            })
            .await?;
        match row {
            Some((snap, log)) => Ok((snap, log.lines().map(str::to_string).collect())),
            None => Err(AppError::not_found("task")),
        }
    }

    /// Number of queued or running tasks.
    pub fn active_count(&self) -> usize {
        self.inner.tasks.lock().values().filter(|t| !t.snapshot.lock().status.is_finished()).count()
    }
}

/// Columns read back for persisted (finished) tasks, in [`stored_snapshot`] order.
const STORED_COLUMNS: &str =
    "id, kind, title, target, target_name, status, actor, created_at, started_at, finished_at, result, error";

fn stored_snapshot(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskSnapshot> {
    let status: String = r.get(5)?;
    let result: Option<String> = r.get(10)?;
    Ok(TaskSnapshot {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        target: r.get(3)?,
        target_name: r.get(4)?,
        status: match status.as_str() {
            "succeeded" => TaskStatus::Succeeded,
            "cancelled" => TaskStatus::Cancelled,
            _ => TaskStatus::Failed,
        },
        actor: r.get(6)?,
        created_at: r.get(7)?,
        started_at: r.get(8)?,
        finished_at: r.get(9)?,
        progress: None,
        message: None,
        error: r.get(11)?,
        result: result.and_then(|s| serde_json::from_str(&s).ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn runs_and_records() {
        let db = Db::open_in_memory().unwrap();
        let tm = TaskManager::new(db, EventBus::new());
        let res = tm
            .run(TaskSpec::new("test", "Test task", &Actor::system()), |ctx| async move {
                ctx.log("step one");
                ctx.progress(0.5);
                Ok(serde_json::json!({"ok": true}))
            })
            .await
            .unwrap();
        assert_eq!(res["ok"], true);
        let list = tm.list(10).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, TaskStatus::Succeeded);
        let (_, log) = tm.get(&list[0].id).await.unwrap();
        assert!(log.iter().any(|l| l.ends_with("step one")));
    }

    #[tokio::test]
    async fn failures_and_locks() {
        let db = Db::open_in_memory().unwrap();
        let tm = TaskManager::new(db, EventBus::new());
        let err = tm
            .run(TaskSpec::new("test", "Failing", &Actor::system()), |_| async move { Err(AppError::conflict("boom")) })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("boom"));

        // Two tasks with the same lock run sequentially.
        let order = Arc::new(Mutex::new(Vec::new()));
        let (o1, o2) = (order.clone(), order.clone());
        let spec = TaskSpec::new("test", "A", &Actor::system()).lock("same");
        let id1 = tm.spawn(spec.clone(), move |_| async move {
            o1.lock().push("a-start");
            tokio::time::sleep(Duration::from_millis(50)).await;
            o1.lock().push("a-end");
            Ok(serde_json::Value::Null)
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        tm.run(spec, move |_| async move {
            o2.lock().push("b");
            Ok(serde_json::Value::Null)
        })
        .await
        .unwrap();
        assert_eq!(*order.lock(), vec!["a-start", "a-end", "b"]);
        assert!(!id1.is_empty());
    }

    #[tokio::test]
    async fn cancellation() {
        let db = Db::open_in_memory().unwrap();
        let tm = TaskManager::new(db, EventBus::new());
        let id = tm.spawn(TaskSpec::new("test", "Long", &Actor::system()), |ctx| async move {
            for _ in 0..100 {
                ctx.check_cancelled()?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok(serde_json::Value::Null)
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(tm.cancel(&id));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (snap, _) = tm.get(&id).await.unwrap();
        assert_eq!(snap.status, TaskStatus::Cancelled);
    }
}
