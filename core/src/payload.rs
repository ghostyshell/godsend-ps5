//! Payload sender: hands an ELF to elfldr on TCP 9021.
//!
//! elfldr takes one connection per payload: the client sends the file and
//! closes its write side, the payload maps and runs it. The loader reports
//! only through the console's kernel log, so an empty reply means "accepted",
//! not "failed".

use crate::config::Host;
use crate::error::{Error, Result};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SendReport {
    pub bytes: u64,
    /// Whatever the loader wrote back, usually empty.
    pub reply: String,
}

/// Connect to elfldr. Returns false when the port is closed, which is the
/// normal state before the loader is started.
pub fn probe(host: &Host) -> bool {
    connect(host, Duration::from_millis(400)).is_ok()
}

fn connect(host: &Host, timeout: Duration) -> Result<TcpStream> {
    let addr = host.socket(host.payload_port);
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|e| Error::DeviceRequired(format!("bad loader address {addr}: {e}")))?
        .next()
        .ok_or_else(|| Error::DeviceRequired(format!("no address for {addr}")))?;
    let stream = TcpStream::connect_timeout(&socket_addr, timeout).map_err(|e| {
        Error::DeviceRequired(format!(
            "elfldr is not answering on {addr} ({e}). Start elfldr on the console."
        ))
    })?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    Ok(stream)
}

/// Send bytes to elfldr.
pub fn send(host: &Host, bytes: &[u8]) -> Result<SendReport> {
    if bytes.is_empty() {
        return Err(Error::Invalid("payload is empty".into()));
    }
    let mut stream = connect(host, Duration::from_secs(3))?;
    stream
        .write_all(bytes)
        .map_err(|e| Error::device(format!("sending the payload failed: {e}")))?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut reply = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                reply.extend_from_slice(&buf[..read]);
                if reply.len() >= 16 * 1024 {
                    break;
                }
            }
            // A timeout is expected: elfldr usually says nothing.
            Err(_) => break,
        }
    }
    Ok(SendReport {
        bytes: bytes.len() as u64,
        reply: String::from_utf8_lossy(&reply).trim().to_string(),
    })
}

/// Send a local file, refusing anything that is not an executable payload.
pub fn send_file(host: &Host, path: &Path) -> Result<SendReport> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !(name.ends_with(".elf") || name.ends_with(".bin")) {
        return Err(Error::Invalid(format!(
            "{} is not an ELF or BIN payload",
            path.display()
        )));
    }
    let metadata = std::fs::metadata(path)?;
    if metadata.len() == 0 {
        return Err(Error::Invalid(format!("{} is empty", path.display())));
    }
    let bytes = std::fs::read(path)?;
    send(host, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_payload_files() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-payload-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("notes.txt");
        std::fs::write(&file, b"hello").unwrap();
        let host = Host::new("", "127.0.0.1");
        assert!(send_file(&host, &file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_payloads() {
        let host = Host::new("", "127.0.0.1");
        assert!(send(&host, b"").is_err());
    }
}
