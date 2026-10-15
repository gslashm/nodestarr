//! Working out how Nodestarr can reach a container's HTTP port.
//!
//! Depends on where Nodestarr runs: on the host (or with host networking) the
//! published port on 127.0.0.1 works; inside a bridge network Nodestarr can use
//! the container's IP when they share a network, otherwise the published
//! port on the Docker host (via the default gateway).

use std::net::Ipv4Addr;

use crate::app::App;
use crate::docker::Entry;

/// How Nodestarr itself is attached to the network.
#[derive(Debug, Clone)]
pub enum SelfNet {
    /// Not in a container, or `network_mode: host`.
    Host,
    /// In a container on these networks; `gateway` reaches the Docker host.
    Bridge { networks: Vec<String>, gateway: Option<Ipv4Addr> },
}

pub fn self_net(app: &App) -> SelfNet {
    let Some(id) = app.docker.self_id() else { return SelfNet::Host };
    let Some(me) = app.docker.get(id) else { return SelfNet::Host };
    if me.summary.network_mode == "host" {
        return SelfNet::Host;
    }
    SelfNet::Bridge { networks: me.summary.networks.clone(), gateway: default_gateway() }
}

/// Default IPv4 gateway from /proc/net/route.
pub fn default_gateway() -> Option<Ipv4Addr> {
    let text = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() > 2 && cols[1] == "00000000" {
            let raw = u32::from_str_radix(cols[2], 16).ok()?;
            return Some(Ipv4Addr::from(raw.swap_bytes()));
        }
    }
    None
}

/// Base URL (scheme://host:port) for a container port, as reachable from Nodestarr.
pub fn base_url(app: &App, entry: &Entry, private_port: u16, https: bool) -> Option<String> {
    let s = &entry.summary;
    let scheme = if https { "https" } else { "http" };
    let published =
        s.ports.iter().find(|p| p.private_port == private_port && p.protocol == "tcp").and_then(|p| p.public_port);
    match self_net(app) {
        SelfNet::Host => {
            if s.network_mode == "host" {
                return Some(format!("{scheme}://127.0.0.1:{private_port}"));
            }
            if let Some(pp) = published {
                return Some(format!("{scheme}://127.0.0.1:{pp}"));
            }
            // Not published: the host can still reach bridge IPs directly.
            container_ip(entry, None).map(|ip| format!("{scheme}://{ip}:{private_port}"))
        }
        SelfNet::Bridge { networks, gateway } => {
            if s.network_mode != "host"
                && let Some(ip) = container_ip(entry, Some(&networks))
            {
                return Some(format!("{scheme}://{ip}:{private_port}"));
            }
            let host_port = if s.network_mode == "host" { Some(private_port) } else { published };
            let host = gateway.map(|g| g.to_string()).or_else(|| {
                let configured = app.settings.get().general.public_host.clone();
                (!configured.is_empty()).then_some(configured)
            })?;
            host_port.map(|p| format!("{scheme}://{host}:{p}"))
        }
    }
}

/// The container's IP, optionally restricted to networks Nodestarr is on.
fn container_ip(entry: &Entry, only: Option<&[String]>) -> Option<String> {
    let nets = entry.inspect.network_settings.as_ref()?.networks.as_ref()?;
    nets.iter()
        .filter(|(name, _)| only.is_none_or(|o| o.contains(name)))
        .filter_map(|(_, ep)| ep.ip_address.clone())
        .find(|ip| !ip.is_empty())
}

/// The port to use for an app: its catalog web port if the container
/// exposes it, otherwise the first published TCP port.
pub fn app_port(entry: &Entry) -> Option<u16> {
    let s = &entry.summary;
    let app = s.app.as_deref().and_then(crate::docker::apps::by_slug);
    if let Some(port) = app.and_then(|a| a.web_port)
        && (s.network_mode == "host" || s.ports.iter().any(|p| p.private_port == port))
    {
        return Some(port);
    }
    s.ports
        .iter()
        .find(|p| p.protocol == "tcp" && p.public_port.is_some())
        .map(|p| p.private_port)
        .or_else(|| app.and_then(|a| a.web_port))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_route_gateway_format() {
        // 0101A8C0 little-endian = 192.168.1.1
        let raw = u32::from_str_radix("0101A8C0", 16).unwrap();
        assert_eq!(std::net::Ipv4Addr::from(raw.swap_bytes()).to_string(), "192.168.1.1");
    }
}
