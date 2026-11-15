//! PS5 probing: FTP reachability, etaHEN detection, and the optional services
//! the app drives (Ghostpad, elfldr, PKG Manager).

use crate::config::{Host, FTP_PORT_ETAHEN, FTP_PORT_FTPSRV};
use crate::error::Result;
use crate::ftp;
use crate::payload;
use serde::Serialize;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceStatus {
    pub host_id: String,
    pub name: String,
    pub addr: String,
    pub ftp_ok: bool,
    pub ftp_ms: u64,
    /// The port that answered (2121 ftpsrv or 1337 etaHEN).
    pub ftp_port: Option<u16>,
    /// `/data/etaHEN/config.ini` was readable, so etaHEN is running.
    pub etahen_ok: bool,
    pub etahen_version: Option<String>,
    /// The homebrew install folder is reachable.
    pub homebrew_ok: bool,
    /// Ghostpad payload answers on the pad port.
    pub pad_ok: bool,
    /// elfldr answers on the payload port.
    pub payload_ok: bool,
    /// PKG Manager web UI answers.
    pub pkg_manager_ok: bool,
    pub pkg_manager_ms: u64,
    /// First line of the etaHEN config or the first failure, for the inspector.
    pub message: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(4))
        .build()
}

/// Probe a console. Never errors: unreachable pieces are reported as false.
pub fn probe(host: &Host) -> DeviceStatus {
    let mut status = DeviceStatus {
        host_id: host.id.clone(),
        name: host.name.clone(),
        addr: host.addr.clone(),
        ..Default::default()
    };

    for port in host.ftp_port_candidates() {
        if tcp_open(&host.addr, port, 400) {
            status.ftp_port = Some(port);
            break;
        }
    }

    let started = Instant::now();
    match ftp::connect(host) {
        Ok(mut conn) => {
            status.ftp_ok = conn.pwd().is_ok();
            status.ftp_ms = started.elapsed().as_millis() as u64;
            if let Ok(bytes) = ftp::read_head(&mut conn, "/data/etaHEN/config.ini", 64 * 1024) {
                if !bytes.is_empty() {
                    status.etahen_ok = true;
                    let text = String::from_utf8_lossy(&bytes);
                    status.etahen_version = parse_etahen_version(&text);
                    if status.message.is_empty() {
                        status.message = first_line(&text);
                    }
                }
            }
            status.homebrew_ok = ftp::list(&mut conn, "/data/homebrew").is_ok();
            let _ = conn.quit();
        }
        Err(e) => status.message = e.to_string(),
    }

    status.pad_ok = tcp_open(&host.addr, host.pad_port, 400);
    status.payload_ok = payload::probe(host);

    let started = Instant::now();
    match agent().get(&format!("{}/", host.pkg_manager_base())).call() {
        Ok(_) => {
            status.pkg_manager_ok = true;
            status.pkg_manager_ms = started.elapsed().as_millis() as u64;
        }
        Err(ureq::Error::Status(_, _)) => {
            // Any HTTP answer means the server is up, even a 404 on `/`.
            status.pkg_manager_ok = true;
            status.pkg_manager_ms = started.elapsed().as_millis() as u64;
        }
        Err(e) => {
            if status.message.is_empty() {
                status.message = format!("PKG Manager: {e}");
            }
        }
    }
    status
}

fn tcp_open(host: &str, port: u16, timeout_ms: u64) -> bool {
    let Ok(addr) = format!("{host}:{port}").parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)).is_ok()
}

/// etaHEN writes a small ini. Versions show up as `2.5B`, `2.4B`, or a plain
/// `2.4`. Keep the first such token after a `version` key, else the first
/// version-looking token anywhere.
pub fn parse_etahen_version(text: &str) -> Option<String> {
    // ASCII lowering keeps the byte offsets, so slicing text is always valid.
    let lower = text.to_ascii_lowercase();
    if let Some(index) = lower.find("version") {
        if let Some(found) = version_token(&text[index..]) {
            return Some(found);
        }
    }
    version_token(text)
}

fn version_token(text: &str) -> Option<String> {
    for token in text.split(|c: char| c.is_whitespace() || c == '=' || c == ';' || c == '"') {
        let token = token.trim();
        let bytes = token.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_digit()
            && token.contains('.')
            && token.len() <= 8
            && token
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.' || b == b'B' || b == b'b')
        {
            return Some(token.to_string());
        }
    }
    None
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Probe every configured host in parallel.
pub fn probe_all(hosts: &[Host]) -> Vec<DeviceStatus> {
    let mut handles = Vec::new();
    for host in hosts {
        let host = host.clone();
        handles.push(std::thread::spawn(move || probe(&host)));
    }
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

/// LAN discovery: a host answering FTP on 2121 or 1337 and exposing `user` or
/// `data` at the root is a PS5.
pub fn discover(subnet_prefix: &str) -> Result<Vec<Host>> {
    let mut found = Vec::new();
    let mut handles = Vec::new();
    for last in 1..=254u8 {
        let addr = format!("{subnet_prefix}.{last}");
        handles.push(std::thread::spawn(move || {
            for port in [FTP_PORT_FTPSRV, FTP_PORT_ETAHEN] {
                if tcp_open(&addr, port, 220) {
                    return Some((addr, port));
                }
            }
            None
        }));
        if handles.len() >= 64 {
            drain(&mut handles, &mut found);
        }
    }
    drain(&mut handles, &mut found);
    found.sort_by(|a, b| a.addr.cmp(&b.addr));
    let local = local_ipv4().unwrap_or_default();
    found.retain(|h| h.addr != local);

    let mut consoles = Vec::new();
    for mut host in found {
        if ftp::connect(&host).is_ok() {
            // ftp::connect already resolved the working port; ask the cache.
            host.name = format!("PS5 {}", host.addr);
            consoles.push(host);
        }
    }
    Ok(consoles)
}

fn drain(
    handles: &mut Vec<std::thread::JoinHandle<Option<(String, u16)>>>,
    found: &mut Vec<Host>,
) {
    for handle in handles.drain(..) {
        if let Ok(Some((addr, port))) = handle.join() {
            let mut host = Host::new("", &addr);
            host.ftp_port = port;
            host.refresh_id();
            found.push(host);
        }
    }
}

fn local_ipv4() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_etahen_version() {
        let ini = "# etaHEN config\nFTP=1\nVERSION=2.5B\n";
        assert_eq!(parse_etahen_version(ini).as_deref(), Some("2.5B"));
        let ini = "etaHEN 2.4B\nFTP=1\n";
        assert_eq!(parse_etahen_version(ini).as_deref(), Some("2.4B"));
        assert_eq!(parse_etahen_version("nothing here"), None);
    }

    #[test]
    fn ignores_non_versions() {
        assert_eq!(parse_etahen_version("PORT=1337"), None);
        assert_eq!(parse_etahen_version("path=/data/homebrew"), None);
    }
}
