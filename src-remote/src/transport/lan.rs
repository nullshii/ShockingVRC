use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

pub const MDNS_TYPE: &str = "_shockingvrc._tcp.local.";

pub fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub fn local_ipv4() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) if !v4.is_link_local() => Some(v4),
            _ => None,
        })
        .collect();
    out.sort_by_key(|ip| !ip.is_private());
    out.dedup();
    out
}

pub fn is_local_address(ip: &IpAddr) -> bool {
    ip.is_loopback() || local_ipv4().iter().any(|v4| IpAddr::V4(*v4) == *ip)
}

pub struct Advertisement {
    daemon: Option<mdns_sd::ServiceDaemon>,
    fullname: String,
}

impl Drop for Advertisement {
    fn drop(&mut self) {
        if let Some(d) = self.daemon.take() {
            let _ = d.unregister(&self.fullname);
            let _ = d.shutdown();
        }
    }
}

pub fn advertise(room: &str, port: u16, pin: &str) -> Option<Advertisement> {
    let ips: Vec<IpAddr> = local_ipv4().into_iter().map(IpAddr::V4).collect();
    if ips.is_empty() {
        return None;
    }
    let daemon = mdns_sd::ServiceDaemon::new().ok()?;
    let mut props = HashMap::new();
    props.insert("pin".to_string(), pin.to_string());
    props.insert("v".to_string(), "2".to_string());
    let host = format!("svrc-{}.local.", &room[..room.len().min(12)]);
    let info = mdns_sd::ServiceInfo::new(MDNS_TYPE, room, &host, &ips[..], port, props).ok()?;
    let fullname = info.get_fullname().to_string();
    if let Err(e) = daemon.register(info) {
        log::warn!("[remote] mDNS advertise failed: {e}");
        return None;
    }
    log::info!("[remote] Advertising on the LAN via mDNS");
    Some(Advertisement {
        daemon: Some(daemon),
        fullname,
    })
}

pub fn discover(room: &str, timeout: Duration) -> Vec<(SocketAddr, Option<String>)> {
    use mdns_sd::{ScopedIp, ServiceEvent};
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else {
        return Vec::new();
    };
    let Ok(rx) = daemon.browse(MDNS_TYPE) else {
        let _ = daemon.shutdown();
        return Vec::new();
    };
    let deadline = std::time::Instant::now() + timeout;
    let mut found = Vec::new();
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                if !info.get_fullname().starts_with(room) {
                    continue;
                }
                let pin = info.get_property_val_str("pin").map(str::to_string);
                for a in info.get_addresses() {
                    let ip = match a {
                        ScopedIp::V4(v4) => IpAddr::V4(*v4.addr()),
                        ScopedIp::V6(v6) => IpAddr::V6(*v6.addr()),
                        _ => continue,
                    };
                    found.push((SocketAddr::new(ip, info.get_port()), pin.clone()));
                }
                if !found.is_empty() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges_are_recognised() {
        for ip in ["192.168.1.2", "10.0.0.1", "172.16.5.4", "127.0.0.1", "fd00::1"] {
            assert!(is_private(&ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "2001:db8::1"] {
            assert!(!is_private(&ip.parse().unwrap()), "{ip}");
        }
    }
}
