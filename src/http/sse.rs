//! Server-sent events: one stream per browser tab carrying container
//! changes, task progress, update state, alerts and (on request) live stats.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::Stream;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::app::App;
use crate::events::Event;
use crate::http::Auth;

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// Include live resource stats (and sample them quickly while connected).
    #[serde(deserialize_with = "crate::util::flag", default)]
    pub stats: bool,
}

pub async fn events(State(app): State<App>, Auth(_p): Auth, Query(q): Query<EventsQuery>) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = app.bus.subscribe();
    let guard = q.stats.then(|| app.monitor.watch());
    let hello = serde_json::json!({ "type": "hello", "data": { "version": crate::VERSION, "time": crate::util::now() } });
    let stream = futures_util::stream::unfold((rx, guard, Some(hello), q.stats), |(mut rx, guard, hello, stats)| async move {
        if let Some(h) = hello {
            let ev = SseEvent::default().data(h.to_string());
            return Some((Ok(ev), (rx, guard, None, stats)));
        }
        loop {
            match rx.recv().await {
                Ok(Event::Stats(_) | Event::Host(_)) if !stats => continue,
                Ok(ev) => {
                    let data = serde_json::to_string(&ev).unwrap_or_default();
                    return Some((Ok(SseEvent::default().data(data)), (rx, guard, None, stats)));
                }
                Err(RecvError::Lagged(_)) => {
                    // The client missed events; tell it to refetch everything.
                    let data = r#"{"type":"resync"}"#;
                    return Some((Ok(SseEvent::default().data(data)), (rx, guard, None, stats)));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
