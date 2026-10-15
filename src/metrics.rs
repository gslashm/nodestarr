//! Prometheus metrics in the text exposition format.

use std::fmt::Write as _;

use crate::app::App;

fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

struct Out(String);

impl Out {
    fn help(&mut self, name: &str, kind: &str, help: &str) {
        let _ = writeln!(self.0, "# HELP {name} {help}\n# TYPE {name} {kind}");
    }

    fn sample(&mut self, name: &str, labels: &[(&str, &str)], value: f64) {
        self.0.push_str(name);
        if !labels.is_empty() {
            self.0.push('{');
            for (i, (k, v)) in labels.iter().enumerate() {
                if i > 0 {
                    self.0.push(',');
                }
                let _ = write!(self.0, "{k}=\"{}\"", esc(v));
            }
            self.0.push('}');
        }
        let _ = writeln!(self.0, " {value}");
    }
}

pub async fn render(app: &App) -> String {
    let mut o = Out(String::with_capacity(16 * 1024));
    o.help("nodestarr_info", "gauge", "Nodestarr version");
    o.sample("nodestarr_info", &[("version", crate::VERSION)], 1.0);
    o.help("nodestarr_docker_connected", "gauge", "Whether Nodestarr can reach Docker");
    o.sample("nodestarr_docker_connected", &[], if app.docker.is_connected() { 1.0 } else { 0.0 });

    let containers = app.docker.list();
    let stats = crate::monitor::stats::latest(app);
    let updates = app.updates.all();
    let now = crate::util::now();

    o.help("nodestarr_container_running", "gauge", "1 when the container is running");
    o.help("nodestarr_container_healthy", "gauge", "1 healthy, 0 unhealthy (only containers with a health check)");
    o.help("nodestarr_container_restarts", "gauge", "Restart count reported by Docker");
    o.help("nodestarr_container_uptime_seconds", "gauge", "Seconds since the container started");
    o.help("nodestarr_container_update_available", "gauge", "1 when an image update is available");
    o.help("nodestarr_container_cpu_percent", "gauge", "CPU usage, 100 = one core");
    o.help("nodestarr_container_memory_bytes", "gauge", "Memory usage excluding page cache");
    o.help("nodestarr_container_network_receive_bytes_per_second", "gauge", "Network receive rate");
    o.help("nodestarr_container_network_transmit_bytes_per_second", "gauge", "Network transmit rate");
    for c in &containers {
        let stack = c.stack.clone().unwrap_or_default();
        let labels = [("name", c.name.as_str()), ("stack", stack.as_str()), ("image", c.image.as_str())];
        o.sample("nodestarr_container_running", &labels, if c.is_running() { 1.0 } else { 0.0 });
        if let Some(h) = &c.health
            && h != "starting"
        {
            o.sample("nodestarr_container_healthy", &labels, if h == "healthy" { 1.0 } else { 0.0 });
        }
        o.sample("nodestarr_container_restarts", &labels, c.restart_count as f64);
        if c.is_running()
            && let Some(started) = c.started_at
        {
            o.sample("nodestarr_container_uptime_seconds", &labels, (now - started).max(0) as f64);
        }
        let upd = updates.get(&c.identity).is_some_and(|u| u.has_update());
        o.sample("nodestarr_container_update_available", &labels, if upd { 1.0 } else { 0.0 });
        if let Some(s) = stats.get(&c.identity) {
            o.sample("nodestarr_container_cpu_percent", &labels, s.cpu);
            o.sample("nodestarr_container_memory_bytes", &labels, s.mem as f64);
            o.sample("nodestarr_container_network_receive_bytes_per_second", &labels, s.net_rx);
            o.sample("nodestarr_container_network_transmit_bytes_per_second", &labels, s.net_tx);
        }
    }

    o.help("nodestarr_uptime_check_up", "gauge", "1 when the check is answering");
    o.help("nodestarr_uptime_check_latency_milliseconds", "gauge", "Last response time");
    for u in app.uptime.all() {
        if u.state == "paused" || u.state == "pending" {
            continue;
        }
        let labels = [("check", u.id.as_str()), ("name", u.name.as_str())];
        o.sample("nodestarr_uptime_check_up", &labels, if u.state == "up" { 1.0 } else { 0.0 });
        if let Some(l) = u.latency_ms {
            o.sample("nodestarr_uptime_check_latency_milliseconds", &labels, l as f64);
        }
    }

    if let Ok(latest) = app.backups.latest_by_identity().await {
        o.help("nodestarr_backup_last_timestamp_seconds", "gauge", "Time of the newest backup");
        for (identity, ts) in latest {
            o.sample("nodestarr_backup_last_timestamp_seconds", &[("identity", identity.as_str())], ts as f64);
        }
    }
    if let Ok(active) = crate::monitor::alerts::list(app, true, 1000).await {
        o.help("nodestarr_alerts_active", "gauge", "Number of firing alerts");
        o.sample("nodestarr_alerts_active", &[], active.len() as f64);
    }
    if let Some((total, avail)) = crate::monitor::host::memory() {
        o.help("nodestarr_host_memory_total_bytes", "gauge", "Docker host memory");
        o.sample("nodestarr_host_memory_total_bytes", &[], total as f64);
        o.help("nodestarr_host_memory_available_bytes", "gauge", "Docker host available memory");
        o.sample("nodestarr_host_memory_available_bytes", &[], avail as f64);
    }
    o.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_labels() {
        let mut o = Out(String::new());
        o.sample("m", &[("a", "x\"y\\z\n")], 1.5);
        assert_eq!(o.0, "m{a=\"x\\\"y\\\\z\\n\"} 1.5\n");
    }
}
