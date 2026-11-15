//! App configuration: PS5 consoles, download folders, debrid keys and service
//! bases. Persisted as JSON under the platform data directory.

use crate::error::{Error, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Default FTP port of ftpsrv, the standalone PS5 FTP payload.
pub const FTP_PORT_FTPSRV: u16 = 2121;
/// Default FTP port of etaHEN's built-in server.
pub const FTP_PORT_ETAHEN: u16 = 1337;

/// One PS5 the app can talk to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Host {
    /// Stable id: `addr:ftp_port`.
    pub id: String,
    pub name: String,
    /// IP address or hostname.
    pub addr: String,
    #[serde(default = "default_ftp_port")]
    pub ftp_port: u16,
    #[serde(default = "default_ftp_user")]
    pub ftp_user: String,
    #[serde(default)]
    pub ftp_pass: String,
    /// Ghostpad payload port (virtual controller).
    #[serde(default = "default_pad_port")]
    pub pad_port: u16,
    /// elfldr payload loader port.
    #[serde(default = "default_payload_port")]
    pub payload_port: u16,
    /// etaHEN PKG Manager web UI port.
    #[serde(default = "default_pkg_manager_port")]
    pub pkg_manager_port: u16,
}

fn default_ftp_port() -> u16 {
    FTP_PORT_FTPSRV
}
fn default_ftp_user() -> String {
    "anonymous".into()
}
fn default_pad_port() -> u16 {
    6967
}
fn default_payload_port() -> u16 {
    9021
}
fn default_pkg_manager_port() -> u16 {
    8844
}

impl Host {
    pub fn new(name: &str, addr: &str) -> Self {
        let addr = addr.trim().to_string();
        Self {
            id: format!("{addr}:{}", default_ftp_port()),
            name: if name.trim().is_empty() {
                addr.clone()
            } else {
                name.trim().to_string()
            },
            addr,
            ftp_port: default_ftp_port(),
            ftp_user: default_ftp_user(),
            ftp_pass: String::new(),
            pad_port: default_pad_port(),
            payload_port: default_payload_port(),
            pkg_manager_port: default_pkg_manager_port(),
        }
    }

    /// Recompute the id after the address or port changed.
    pub fn refresh_id(&mut self) {
        self.id = format!("{}:{}", self.addr.trim(), self.ftp_port);
    }

    pub fn socket(&self, port: u16) -> String {
        format!("{}:{}", self.addr, port)
    }

    pub fn pkg_manager_base(&self) -> String {
        format!("http://{}:{}", self.addr, self.pkg_manager_port)
    }

    /// FTP ports to try, in order: the configured one first, then the two
    /// servers PS5 homebrew actually ships.
    pub fn ftp_port_candidates(&self) -> Vec<u16> {
        let mut ports = vec![self.ftp_port];
        for port in [FTP_PORT_FTPSRV, FTP_PORT_ETAHEN, 21] {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
        ports
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Every configured console.
    pub hosts: Vec<Host>,
    /// Id of the host the UI works against.
    pub active_host: Option<String>,
    /// Where downloads land. Empty = `<Downloads>/godsend-ps5`.
    pub download_dir: String,
    /// Real-Debrid API key. Empty = not connected.
    pub rd_api_key: String,
    /// TorBox API key. Empty = not connected.
    pub tb_api_key: String,
    /// game-search service base URL.
    pub gamesearch_base: String,
    /// Public R2 base for cover art.
    pub covers_base: String,
    /// Optional explicit aria2c path. Empty = auto-detect.
    pub aria2_path: String,
    /// Chunk size for FAT32 splits in bytes (default 4 GiB - 1 MiB).
    pub split_bytes: u64,
    /// Where a PKG is left for PS5Upload or PKG Manager to install.
    pub pkg_dir: String,
    /// Where game images and folder dumps are installed for ShadowMount+.
    pub homebrew_dir: String,
    /// Queue the package for install right after the upload.
    pub auto_install_pkg: bool,
    /// Runtime payload catalogue (itsPLK mirror).
    pub payload_catalog_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            active_host: None,
            download_dir: String::new(),
            rd_api_key: String::new(),
            tb_api_key: String::new(),
            gamesearch_base: "http://localhost:3011".into(),
            covers_base: "https://covers.debrid.in".into(),
            aria2_path: String::new(),
            split_bytes: 4 * 1024 * 1024 * 1024 - 1024 * 1024,
            pkg_dir: "/data/ps5upload/pkg_library".into(),
            homebrew_dir: "/data/homebrew".into(),
            auto_install_pkg: true,
            payload_catalog_url:
                "https://raw.githubusercontent.com/itsPLK/ps5-payloads-mirror/main/payloads_v2.json"
                    .into(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let path = config_path();
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!(
                    "config at {} is corrupt ({}), using defaults",
                    path.display(),
                    e
                );
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            restrict_dir(parent);
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        // Write beside the target and rename, so a crash mid-write cannot
        // leave invalid JSON that load() would quietly replace with defaults.
        let temp = path.with_extension("json.tmp");
        write_private(&temp, &bytes)?;
        fs::rename(&temp, &path)?;
        Ok(())
    }

    pub fn active(&self) -> Option<Host> {
        let id = self.active_host.as_ref()?;
        self.hosts.iter().find(|h| &h.id == id).cloned()
    }

    pub fn require_active(&self) -> Result<Host> {
        self.active().ok_or(Error::NoDevice)
    }

    /// Add or replace a host by id, then make it active when it is the first.
    pub fn upsert_host(&mut self, host: Host) {
        self.upsert_host_replacing(host, None)
    }

    /// Add or replace a host, first removing `replace_id` (the id the editor
    /// started from, which changes when the address changes).
    pub fn upsert_host_replacing(&mut self, mut host: Host, replace_id: Option<&str>) {
        let replaced_active = replace_id
            .map(|id| self.active_host.as_deref() == Some(id))
            .unwrap_or(false);
        if let Some(old) = replace_id {
            self.hosts.retain(|h| h.id != old);
            if replaced_active {
                self.active_host = None;
            }
        }
        host.refresh_id();
        host.name = host.name.trim().to_string();
        host.addr = host.addr.trim().to_string();
        if host.name.is_empty() {
            host.name = host.addr.clone();
        }
        if let Some(existing) = self.hosts.iter_mut().find(|h| h.id == host.id) {
            *existing = host.clone();
        } else {
            self.hosts.push(host.clone());
        }
        if self.active_host.is_none() || replaced_active {
            self.active_host = Some(host.id);
        }
    }

    pub fn remove_host(&mut self, id: &str) {
        self.hosts.retain(|h| h.id != id);
        if self.active_host.as_deref() == Some(id) {
            self.active_host = self.hosts.first().map(|h| h.id.clone());
        }
    }

    /// Overwrite only the fields present in `patch` (a partial JSON object).
    pub fn patch_from_json(&mut self, patch: &serde_json::Value) -> Result<()> {
        let merged = merge_json(&serde_json::to_value(&*self)?, patch);
        let next: Config = serde_json::from_value(merged)
            .map_err(|e| Error::Invalid(format!("config patch rejected: {}", e)))?;
        *self = next;
        self.save()
    }
}

fn merge_json(base: &serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            let mut out = b.clone();
            for (k, v) in p {
                let merged = match out.get(k) {
                    Some(existing) => merge_json(existing, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            serde_json::Value::Object(out)
        }
        (_, v) => v.clone(),
    }
}

/// Qualifier so the data dir is `~/Library/Application Support/godsend-ps5`
/// on macOS and friends elsewhere.
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("click", "godsendps5", "godsend-ps5")
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("godsend-ps5"))
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

pub fn icons_cache_dir() -> PathBuf {
    cache_dir().join("icons")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn jobs_path() -> PathBuf {
    data_dir().join("jobs.json")
}

pub fn ensure_dirs() -> Result<()> {
    for dir in [data_dir(), cache_dir(), icons_cache_dir()] {
        fs::create_dir_all(&dir)?;
        restrict_dir(&dir);
        probe_writable(&dir)?;
    }
    Ok(())
}

/// `config.json` holds FTP passwords and debrid keys; keep it owner-only.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)?;
        Ok(())
    }
}

fn restrict_dir(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
}

fn probe_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(".write-probe");
    fs::write(&probe, b"ok")?;
    fs::remove_file(&probe)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ids_and_active_selection() {
        let mut cfg = Config::default();
        let mut host = Host::new("Living room", "192.168.1.50");
        cfg.upsert_host(host.clone());
        assert_eq!(cfg.active_host.as_deref(), Some("192.168.1.50:2121"));
        host.addr = "192.168.1.51".into();
        cfg.upsert_host(host);
        assert_eq!(cfg.hosts.len(), 2);
        assert_eq!(cfg.require_active().unwrap().addr, "192.168.1.50");
        cfg.remove_host("192.168.1.50:2121");
        assert_eq!(cfg.require_active().unwrap().addr, "192.168.1.51");
    }

    #[test]
    fn ftp_falls_back_to_etahen_and_ftpsrv() {
        let host = Host::new("", "10.0.0.2");
        let ports = host.ftp_port_candidates();
        assert_eq!(ports[0], 2121);
        assert!(ports.contains(&1337));
        assert!(ports.contains(&21));
    }

    #[test]
    fn editing_an_address_replaces_instead_of_duplicating() {
        let mut cfg = Config::default();
        let mut host = Host::new("Den", "192.168.1.50");
        cfg.upsert_host(host.clone());
        let old_id = host.id.clone();
        host.addr = "192.168.1.60".into();
        cfg.upsert_host_replacing(host.clone(), Some(&old_id));
        assert_eq!(cfg.hosts.len(), 1);
        assert_eq!(cfg.hosts[0].addr, "192.168.1.60");
        assert_eq!(cfg.active_host.as_deref(), Some("192.168.1.60:2121"));
    }

    #[test]
    fn patch_merges_fields() {
        let mut cfg = Config::default();
        cfg.patch_from_json(&serde_json::json!({ "auto_install_pkg": false, "split_bytes": 123 }))
            .expect("patch must apply");
        assert!(!cfg.auto_install_pkg);
        assert_eq!(cfg.split_bytes, 123);
    }
}
