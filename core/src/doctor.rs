//! Preflight checks shown on the Home page.

use crate::config::{Config, Host};
use crate::device;
use crate::error::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

pub fn run(config: &Config) -> Vec<Check> {
    run_with_statuses(config, &[])
}

/// Same checks, reusing statuses the caller already probed so a snapshot does
/// not probe the active console twice.
pub fn run_with_statuses(
    config: &Config,
    statuses: &[crate::device::DeviceStatus],
) -> Vec<Check> {
    let mut checks = Vec::new();

    match config.active() {
        Some(host) => checks.push(Check {
            name: "Console configured".into(),
            ok: true,
            detail: format!("{} at {}", host.name, host.addr),
        }),
        None => checks.push(Check {
            name: "Console configured".into(),
            ok: false,
            detail: "Add a PS5 on the Devices page".into(),
        }),
    }

    if let Some(host) = config.active() {
        let status = statuses
            .iter()
            .find(|s| s.host_id == host.id)
            .cloned()
            .unwrap_or_else(|| device::probe(&host));
        let port = status
            .ftp_port
            .map(|p| format!(" on {p}"))
            .unwrap_or_default();
        checks.push(Check {
            name: "FTP reachable".into(),
            ok: status.ftp_ok,
            detail: if status.ftp_ok {
                format!("{}{}", human_ms(status.ftp_ms), port)
            } else {
                "start ftpsrv or etaHEN on the console".into()
            },
        });
        checks.push(Check {
            name: "etaHEN".into(),
            ok: status.etahen_ok,
            detail: if status.etahen_ok {
                status
                    .etahen_version
                    .map(|v| format!("v{v}"))
                    .unwrap_or_else(|| "config found at /data/etaHEN".into())
            } else {
                "optional: etaHEN adds PKG Manager and its payload menu".into()
            },
        });
        checks.push(Check {
            name: "Ghostpad".into(),
            ok: status.pad_ok,
            detail: if status.pad_ok {
                format!("answering on {}", host.pad_port)
            } else {
                "the virtual controller needs the Ghostpad payload".into()
            },
        });
        checks.push(Check {
            name: "elfldr".into(),
            ok: status.payload_ok,
            detail: if status.payload_ok {
                format!("answering on {}", host.payload_port)
            } else {
                "payload sending needs elfldr on port 9021".into()
            },
        });
        checks.push(Check {
            name: "PKG Manager".into(),
            ok: status.pkg_manager_ok,
            detail: if status.pkg_manager_ok {
                host.pkg_manager_base()
            } else {
                "optional: the etaHEN PKG Manager web UI".into()
            },
        });
    }

    let download_dir = crate::aria2::download_dir(config);
    let writable = std::fs::create_dir_all(&download_dir).is_ok()
        && std::fs::write(download_dir.join(".probe"), b"ok").is_ok();
    if writable {
        let _ = std::fs::remove_file(download_dir.join(".probe"));
    }
    checks.push(Check {
        name: "Download folder".into(),
        ok: writable,
        detail: download_dir.display().to_string(),
    });

    checks.push(Check {
        name: "aria2c".into(),
        ok: crate::aria2::find_aria2(&config.aria2_path).is_some(),
        detail: match crate::aria2::find_aria2(&config.aria2_path) {
            Some(path) => path.display().to_string(),
            None => "optional: downloads fall back to a single connection".into(),
        },
    });

    checks.push(Check {
        name: "Debrid".into(),
        ok: !config.rd_api_key.is_empty() || !config.tb_api_key.is_empty(),
        detail: match (config.rd_api_key.is_empty(), config.tb_api_key.is_empty()) {
            (false, false) => "Real-Debrid and TorBox connected".into(),
            (false, true) => "Real-Debrid connected".into(),
            (true, false) => "TorBox connected".into(),
            (true, true) => "optional: keys unlock debrid download links".into(),
        },
    });

    checks
}

pub fn host_checks(host: &Host) -> Result<Vec<Check>> {
    let status = device::probe(host);
    Ok(vec![
        Check {
            name: "FTP".into(),
            ok: status.ftp_ok,
            detail: human_ms(status.ftp_ms),
        },
        Check {
            name: "etaHEN".into(),
            ok: status.etahen_ok,
            detail: status.etahen_version.unwrap_or_default(),
        },
        Check {
            name: "Ghostpad".into(),
            ok: status.pad_ok,
            detail: format!("port {}", host.pad_port),
        },
        Check {
            name: "elfldr".into(),
            ok: status.payload_ok,
            detail: format!("port {}", host.payload_port),
        },
    ])
}

fn human_ms(ms: u64) -> String {
    if ms == 0 {
        "connected".into()
    } else {
        format!("{ms} ms")
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
