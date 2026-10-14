//! Network helpers: CIDR lists, client address resolution behind proxies,
//! and discovering this machine's LAN address for links.

use std::net::{IpAddr, SocketAddr, UdpSocket};

use ipnet::IpNet;

/// A parsed list of CIDR ranges ("192.168.1.0/24", "100.64.0.0/10", single IPs).
#[derive(Debug, Clone, Default)]
pub struct CidrList(Vec<IpNet>);

impl CidrList {
    /// Parses entries, returning an error naming the first invalid one.
    pub fn parse(entries: &[String]) -> Result<Self, String> {
        let mut nets = Vec::with_capacity(entries.len());
        for raw in entries {
            let s = raw.trim();
            if s.is_empty() {
                continue;
            }
            let net = if s.contains('/') {
                s.parse::<IpNet>().map_err(|_| format!("invalid network: {s}"))?
            } else {
                let ip: IpAddr = s.parse().map_err(|_| format!("invalid address: {s}"))?;
                IpNet::from(ip)
            };
            nets.push(net);
        }
        Ok(Self(nets))
    }

    /// Parses leniently, skipping invalid entries (settings are validated on save).
    pub fn parse_lossy(entries: &[String]) -> Self {
        Self(
            entries
                .iter()
                .filter_map(|e| Self::parse(std::slice::from_ref(e)).ok())
                .flat_map(|l| l.0)
                .collect(),
        )
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        self.0.iter().any(|n| n.contains(&ip))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Maps IPv4-mapped IPv6 addresses (::ffff:1.2.3.4) back to IPv4.
pub fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

/// Default "local networks" used by the optional auth bypass: loopback,
/// RFC 1918 private ranges, link-local, and the Tailscale/CGNAT range.
pub fn default_local_networks() -> Vec<String> {
    [
        "127.0.0.0/8",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "169.254.0.0/16",
        "100.64.0.0/10",
        "::1/128",
        "fc00::/7",
        "fe80::/10",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Determines the real client address. If the TCP peer is a trusted proxy,
/// the right-most untrusted entry of X-Forwarded-For (or X-Real-IP) is used.
pub fn client_ip(
    peer: SocketAddr,
    forwarded_for: Option<&str>,
    real_ip: Option<&str>,
    trusted: &CidrList,
) -> IpAddr {
    let peer_ip = canonical(peer.ip());
    if trusted.is_empty() || !trusted.contains(peer_ip) {
        return peer_ip;
    }
    if let Some(xff) = forwarded_for {
        // Walk from the right: the last hop not added by a trusted proxy is the client.
        for part in xff.rsplit(',') {
            if let Ok(ip) = part.trim().parse::<IpAddr>() {
                let ip = canonical(ip);
                if !trusted.contains(ip) {
                    return ip;
                }
            }
        }
    }
    if let Some(ip) = real_ip.and_then(|s| s.trim().parse::<IpAddr>().ok()) {
        return canonical(ip);
    }
    peer_ip
}

/// Best-effort guess of this machine's primary LAN IPv4 address, used for
/// quick links when no public host is configured. No packets are sent: a UDP
/// "connect" only asks the kernel which source address it would use.
pub fn primary_ipv4() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_unspecified() || ip.is_loopback() {
        None
    } else {
        Some(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(v: &[&str]) -> CidrList {
        CidrList::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn cidr_contains() {
        let l = list(&["192.168.1.0/24", "10.0.0.5"]);
        assert!(l.contains("192.168.1.50".parse().unwrap()));
        assert!(l.contains("10.0.0.5".parse().unwrap()));
        assert!(!l.contains("10.0.0.6".parse().unwrap()));
        assert!(l.contains("::ffff:192.168.1.60".parse().unwrap()));
        assert!(CidrList::parse(&["nope".to_string()]).is_err());
    }

    #[test]
    fn client_ip_through_proxy() {
        let trusted = list(&["172.17.0.0/16"]);
        let proxy: SocketAddr = "172.17.0.2:5000".parse().unwrap();
        let direct: SocketAddr = "192.168.1.50:5000".parse().unwrap();
        // Untrusted peer: header ignored (prevents spoofing).
        assert_eq!(
            client_ip(direct, Some("1.2.3.4"), None, &trusted),
            "192.168.1.50".parse::<IpAddr>().unwrap()
        );
        // Trusted peer: right-most untrusted hop wins.
        assert_eq!(
            client_ip(proxy, Some("8.8.8.8, 1.2.3.4, 172.17.0.9"), None, &trusted),
            "1.2.3.4".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            client_ip(proxy, None, Some("5.6.7.8"), &trusted),
            "5.6.7.8".parse::<IpAddr>().unwrap()
        );
    }
}
