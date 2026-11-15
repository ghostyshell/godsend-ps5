//! Payload and homebrew catalogue.
//!
//! The catalogue is not baked into the app: it comes from the public
//! `itsPLK/ps5-payloads-mirror` payload list, which is refreshed as the scene
//! ships new builds. Every row is an ELF (sent to elfldr) or a PKG (uploaded
//! to the install folder), and the same install path handles both.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::install::{install_local, InstallReport};
use crate::transfer::JobContext;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Stable id: the asset filename.
    pub id: String,
    pub name: String,
    pub description: String,
    /// `Networking & Servers`, `Utilities & Tools`, ...
    pub category: String,
    pub filename: String,
    pub url: String,
    /// Upstream release page.
    pub source: String,
    pub version: String,
    pub published_at: String,
    pub size_bytes: u64,
    pub checksum: String,
}

/// How long a cached catalogue is trusted without a refresh.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Refreshed catalogue rows for the UI.
pub fn catalog(config: &Config, refresh: bool) -> Result<Vec<CatalogEntry>> {
    let cache = cache_path();
    if !refresh && fresh(&cache) {
        if let Ok(bytes) = std::fs::read(&cache) {
            if let Ok(entries) = serde_json::from_slice::<Vec<CatalogEntry>>(&bytes) {
                if !entries.is_empty() {
                    return Ok(entries);
                }
            }
        }
    }
    match fetch(&config.payload_catalog_url) {
        Ok(entries) if !entries.is_empty() => {
            if let Some(parent) = cache.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(bytes) = serde_json::to_vec_pretty(&entries) {
                let _ = std::fs::write(&cache, bytes);
            }
            Ok(entries)
        }
        Ok(_) => Err(Error::Http("the payload catalogue is empty".into())),
        Err(error) => {
            // A stale cache beats an empty page when the mirror is down.
            if let Ok(bytes) = std::fs::read(&cache) {
                if let Ok(entries) = serde_json::from_slice::<Vec<CatalogEntry>>(&bytes) {
                    if !entries.is_empty() {
                        return Ok(entries);
                    }
                }
            }
            Err(error)
        }
    }
}

fn fetch(url: &str) -> Result<Vec<CatalogEntry>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(25))
        .build();
    let value: serde_json::Value = agent
        .get(url)
        .set("User-Agent", "godsend-ps5")
        .call()?
        .into_json()
        .map_err(|e| Error::Http(format!("payload catalogue: {e}")))?;
    Ok(parse_catalog(&value))
}

/// Parse both mirror shapes: `{"payloads": [...]}` and a bare array.
pub fn parse_catalog(value: &serde_json::Value) -> Vec<CatalogEntry> {
    let list = value
        .get("payloads")
        .and_then(|p| p.as_array())
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut entries = Vec::new();
    for row in list {
        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
        let url = row.get("url").and_then(|v| v.as_str()).unwrap_or("").trim();
        let filename = row
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if name.is_empty() || url.is_empty() || filename.is_empty() {
            continue;
        }
        // A catalogue is remote input. Only a plain asset name with a payload
        // extension may become a local file name.
        let lower = filename.to_ascii_lowercase();
        let plain = !filename.contains('/')
            && !filename.contains('\\')
            && !filename.starts_with('.')
            && [".elf", ".bin", ".pkg"].iter().any(|ext| lower.ends_with(ext));
        if !plain {
            continue;
        }
        entries.push(CatalogEntry {
            id: filename.to_string(),
            name: name.to_string(),
            description: row
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            category: row
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("Payloads")
                .to_string(),
            filename: filename.to_string(),
            url: url.to_string(),
            source: row
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            version: row
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            published_at: row
                .get("last_update")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            size_bytes: 0,
            checksum: row
                .get("checksum")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        });
    }
    entries.sort_by(|a, b| {
        a.category
            .to_lowercase()
            .cmp(&b.category.to_lowercase())
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

fn cache_path() -> PathBuf {
    crate::config::cache_dir().join("payloads.json")
}

fn fresh(path: &PathBuf) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age < CACHE_TTL)
        .unwrap_or(false)
}

/// Download a catalogue row, then deliver it: ELFs go to elfldr, PKGs and
/// images go to the console's install folders.
pub fn install(
    host: &crate::config::Host,
    config: &Config,
    entry: &CatalogEntry,
    ctx: &JobContext,
) -> Result<InstallReport> {
    ctx.message(format!("downloading {}", entry.name));
    let local = crate::store::download_url(&entry.url, &entry.filename, config, ctx)?;
    // The catalogue carries a SHA-256. A payload handed to elfldr runs in the
    // console's kernel context, so verify before delivering.
    if entry.checksum.trim().is_empty() {
        return Err(Error::Invalid(format!(
            "{} has no checksum; refusing to send it",
            entry.name
        )));
    }
    let actual = sha256_file(&local)?;
    if !actual.eq_ignore_ascii_case(entry.checksum.trim()) {
        return Err(Error::Invalid(format!(
            "{} failed its checksum (expected {}, got {})",
            entry.name, entry.checksum, actual
        )));
    }
    ctx.message(format!("installing {}", entry.name));
    install_local(host, &local, config, ctx)
}

fn sha256_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_v2_catalogue() {
        let value = serde_json::json!({
            "payloads": [
                {
                    "name": "ftpsrv",
                    "filename": "ftpsrv_v0.21.1.elf",
                    "url": "https://example/ftpsrv.elf",
                    "source": "https://github.com/ps5-payload-dev/ftpsrv/releases",
                    "description": "FTP server",
                    "last_update": "2026-11-01",
                    "version": "v0.21.1",
                    "category": "Networking & Servers",
                    "checksum": "abc"
                },
                {"name": "broken"}
            ]
        });
        let entries = parse_catalog(&value);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ftpsrv_v0.21.1.elf");
        assert_eq!(entries[0].category, "Networking & Servers");
        assert_eq!(entries[0].version, "v0.21.1");
    }

    #[test]
    fn drops_catalogue_names_that_are_paths() {
        let value = serde_json::json!({
            "payloads": [
                {"name": "bad", "filename": "../../x.elf", "url": "https://example/x.elf"},
                {"name": "also bad", "filename": "x.zip", "url": "https://example/x.zip"},
                {"name": "ok", "filename": "x.elf", "url": "https://example/x.elf"}
            ]
        });
        let entries = parse_catalog(&value);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "x.elf");
    }

    #[test]
    fn parses_a_bare_array() {
        let value = serde_json::json!([
            {"name": "a", "filename": "a.elf", "url": "https://example/a.elf"}
        ]);
        assert_eq!(parse_catalog(&value).len(), 1);
    }
}
