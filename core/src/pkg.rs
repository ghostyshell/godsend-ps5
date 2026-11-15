//! PKG inspection for PS4 and PS5 packages.
//!
//! The app does not decrypt or repack packages; it identifies them so the
//! library and the uploader can name them, rename them and place them where
//! PS5Upload or PKG Manager installs them.

use crate::error::Result;
use serde::Serialize;
use std::path::Path;

pub const MAGIC: [u8; 4] = [0x7f, 0x43, 0x4e, 0x54]; // "\x7fCNT"

/// Where a PKG can be left for an installer, in preference order.
pub const PKG_DIRS: &[&str] = &[
    "/data/ps5upload/pkg_library",
    "/user/data/ps5upload/pkg_library",
    "/data/pkg",
];

#[derive(Debug, Clone, Serialize)]
pub struct PkgInfo {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub magic_ok: bool,
    /// Package type field at offset 0x04.
    pub pkg_type: u32,
    /// Nine-character title id (`PPSA01234`, `CUSA12345`).
    pub title_id: Option<String>,
    /// Full content id (`UP0000-PPSA01234_00-...`) when present.
    pub content_id: Option<String>,
}

impl PkgInfo {
    /// `base`, `update`, `dlc`, or `type <n>`.
    pub fn type_label(&self) -> String {
        match self.pkg_type {
            1 => "base".into(),
            2 => "update".into(),
            3 => "dlc".into(),
            other => format!("type {other}"),
        }
    }
}

pub fn is_pkg(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("pkg"))
        .unwrap_or(false)
}

pub fn is_pkg_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pkg")
}

/// Inspect a PKG using its first 4 KiB plus the file size.
pub fn info(path: &Path) -> Result<PkgInfo> {
    let size = std::fs::metadata(path)?.len();
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut head = vec![0u8; 4096];
    let read = file.read(&mut head)?;
    head.truncate(read);
    Ok(info_from_head(
        path.to_string_lossy().to_string(),
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        size,
        &head,
    ))
}

pub fn info_from_head(path: String, name: String, size_bytes: u64, head: &[u8]) -> PkgInfo {
    let magic_ok = head.len() >= 4 && head[0..4] == MAGIC;
    let pkg_type = if head.len() >= 8 {
        u32::from_be_bytes([head[4], head[5], head[6], head[7]])
    } else {
        0
    };
    let (content_id, title_id) = find_content_id(head);
    PkgInfo {
        path,
        name,
        size_bytes,
        magic_ok,
        pkg_type,
        title_id,
        content_id,
    }
}

/// The content id sits at offset 0x30 in both PS4 and PS5 headers. Scan for it
/// anyway, because older tools wrote it in slightly different places.
fn find_content_id(head: &[u8]) -> (Option<String>, Option<String>) {
    let mut run = Vec::new();
    for &byte in head.iter().chain(std::iter::once(&0u8)) {
        if byte.is_ascii_graphic() {
            run.push(byte);
            continue;
        }
        if run.len() >= 24 {
            if let Ok(text) = std::str::from_utf8(&run) {
                if let Some(id) = title_id_from_content_id(text) {
                    return (Some(text.to_string()), Some(id));
                }
            }
        }
        run.clear();
    }
    for window in head.windows(9) {
        if let Ok(text) = std::str::from_utf8(window) {
            if is_title_id(text) {
                return (None, Some(text.to_string()));
            }
        }
    }
    (None, None)
}

/// `UP0000-PPSA01234_00-XXXXXXXXXXXXXXXX` -> `PPSA01234`.
pub fn title_id_from_content_id(text: &str) -> Option<String> {
    let mut parts = text.split('-');
    let _distributor = parts.next()?;
    let rest = parts.next()?;
    let id = rest.split('_').next().unwrap_or(rest).trim();
    if is_title_id(id) {
        Some(id.to_string())
    } else {
        None
    }
}

/// `PPSA01234`, `CUSA12345`, `BLES01234`: four letters then five digits.
pub fn is_title_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 9
        && bytes[..4].iter().all(|b| b.is_ascii_uppercase())
        && bytes[4..].iter().all(|b| b.is_ascii_digit())
}

/// Candidate folders for a package, configured path first.
pub fn pkg_dirs(configured: &str) -> Vec<String> {
    let mut dirs = vec![configured.trim_end_matches('/').to_string()];
    for dir in PKG_DIRS {
        if !dirs.iter().any(|d| d == dir) {
            dirs.push((*dir).to_string());
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_title_ids() {
        assert!(is_title_id("PPSA01234"));
        assert!(is_title_id("CUSA12345"));
        assert!(is_title_id("BLES01234"));
        assert!(!is_title_id("PPSA1234"));
        assert!(!is_title_id("ppsa01234"));
    }

    #[test]
    fn parses_a_ps5_header() {
        let mut head = vec![0u8; 512];
        head[0..4].copy_from_slice(&MAGIC);
        head[4..8].copy_from_slice(&1u32.to_be_bytes());
        head[0x30..0x30 + 36].copy_from_slice(b"UP0000-PPSA01234_00-0000000000000000");
        let info = info_from_head("/x.pkg".into(), "x.pkg".into(), 1234, &head);
        assert!(info.magic_ok);
        assert_eq!(info.pkg_type, 1);
        assert_eq!(info.type_label(), "base");
        assert_eq!(info.title_id.as_deref(), Some("PPSA01234"));
        assert_eq!(
            info.content_id.as_deref(),
            Some("UP0000-PPSA01234_00-0000000000000000")
        );
    }

    #[test]
    fn reports_non_pkg_files() {
        let info = info_from_head("/x".into(), "x".into(), 10, b"not a pkg");
        assert!(!info.magic_ok);
        assert_eq!(info.title_id, None);
    }

    #[test]
    fn keeps_the_configured_pkg_dir_first() {
        let dirs = pkg_dirs("/data/ps5upload/pkg_library");
        assert_eq!(dirs[0], "/data/ps5upload/pkg_library");
        assert_eq!(dirs.iter().filter(|d| *d == "/data/ps5upload/pkg_library").count(), 1);
        assert!(dirs.contains(&"/data/pkg".to_string()));
    }
}
