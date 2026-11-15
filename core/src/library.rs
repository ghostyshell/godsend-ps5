//! Remote library scan: what is installed on the PS5.
//!
//! A jailbroken PS5 exposes a stable layout over FTP:
//!
//! - `/user/app/<TITLEID>` installed app content, metadata in
//!   `/user/appmeta/<TITLEID>/{param.sfo,icon0.png}`
//! - `/user/patch/<TITLEID>` installed updates
//! - `/data/homebrew/<name>/` game images and folder dumps that ShadowMount+
//!   mounts (`*.exfat`, `*.ffpkg`, `*.ffpfs`, `*.ffpfsc`)
//! - `/data/etaHEN/games/` the older drop folder
//! - `/mnt/usb*/homebrew`, `/mnt/ext*/homebrew` the same on external drives
//!
//! The scan tolerates missing directories and per-entry failures, so one
//! unreadable PARAM.SFO never empties the library.

use crate::config::{Config, Host};
use crate::error::Result;
use crate::ftp;
use crate::pkg;
use crate::sfo::Sfo;
use serde::Serialize;
use std::path::PathBuf;

pub const APP_ROOT: &str = "/user/app";
pub const APPMETA_ROOT: &str = "/user/appmeta";

/// Extensions ShadowMount+ mounts, plus the raw PS4/PS5 ISO nobody expects on
/// a PS5 but people still keep on a USB drive.
pub const IMAGE_EXTS: &[&str] = &["exfat", "ffpkg", "ffpfs", "ffpfsc", "iso", "img"];

#[derive(Debug, Clone, Serialize)]
pub struct GameEntry {
    /// Stable id: `<kind>:<remote path>`.
    pub id: String,
    pub title: String,
    pub title_id: Option<String>,
    /// `ps5`, `ps4`, `homebrew`, `pkg`, `unknown`.
    pub system: String,
    /// `app`, `image`, `homebrew`, `pkg`.
    pub kind: String,
    /// `digital`, `homebrew`, `image`, `package`.
    pub category: String,
    pub path: String,
    pub size_bytes: u64,
    pub has_icon: bool,
    pub icon_path: Option<String>,
    pub version: Option<String>,
    /// `internal`, `usb0`, `ext0`, ...
    pub device: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LibraryView {
    pub games: Vec<GameEntry>,
    pub packages: Vec<GameEntry>,
    /// Directories that were reachable, for the empty state.
    pub roots: Vec<String>,
}

pub fn scan(host: &Host) -> Result<LibraryView> {
    scan_with(host, &Config::load())
}

/// Scan with explicit settings, so tests and the CLI can point at a fixture.
pub fn scan_with(host: &Host, config: &Config) -> Result<LibraryView> {
    let mut conn = ftp::connect(host)?;
    let mut view = LibraryView::default();

    scan_apps(&mut conn, &mut view);
    scan_homebrew(&mut conn, &config.homebrew_dir, "internal", &mut view);
    scan_homebrew(&mut conn, "/data/etaHEN/games", "internal", &mut view);
    for device in list_external_devices(&mut conn) {
        scan_homebrew(&mut conn, &format!("/mnt/{device}/homebrew"), &device, &mut view);
    }
    scan_packages(&mut conn, &config.pkg_dir, "internal", &mut view);

    view.games
        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    view.packages
        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    view.games.dedup_by(|a, b| a.path == b.path);
    view.roots.sort();
    view.roots.dedup();
    let _ = conn.quit();
    Ok(view)
}

/// Installed apps, read from appmeta.
fn scan_apps(conn: &mut suppaftp::FtpStream, view: &mut LibraryView) {
    let Ok(entries) = ftp::list(conn, APP_ROOT) else {
        return;
    };
    view.roots.push(APP_ROOT.to_string());
    for entry in entries.iter().filter(|e| e.is_dir) {
        let title_id = entry.name.trim().to_string();
        if !pkg::is_title_id(&title_id) {
            continue;
        }
        let sfo_path = format!("{APPMETA_ROOT}/{title_id}/param.sfo");
        let sfo = ftp::read_head(conn, &sfo_path, 512 * 1024)
            .ok()
            .and_then(|head| Sfo::parse(&head).ok());
        let title = sfo
            .as_ref()
            .and_then(|s| s.title())
            .map(|s| s.to_string())
            .unwrap_or_else(|| title_from_filename(&title_id));
        let version = sfo.as_ref().and_then(|s| s.version()).map(|s| s.to_string());
        let system = system_for_id(&title_id);
        view.games.push(GameEntry {
            id: format!("app:{}", entry.path),
            title,
            title_id: Some(title_id.clone()),
            system: system.into(),
            kind: "app".into(),
            category: "digital".into(),
            path: entry.path.clone(),
            size_bytes: entry.size_bytes,
            has_icon: true,
            icon_path: Some(format!("{APPMETA_ROOT}/{title_id}/icon0.png")),
            version,
            device: "internal".into(),
        });
    }
    if !view.roots.contains(&APPMETA_ROOT.to_string()) && !view.games.is_empty() {
        view.roots.push(APPMETA_ROOT.to_string());
    }
}

/// Images and folder dumps in one homebrew root.
fn scan_homebrew(
    conn: &mut suppaftp::FtpStream,
    root: &str,
    device: &str,
    view: &mut LibraryView,
) {
    let Ok(entries) = ftp::list(conn, root) else {
        return;
    };
    view.roots.push(root.to_string());
    for entry in entries {
        if entry.is_dir {
            if let Some(mut game) = read_homebrew_folder(conn, &entry, root, device) {
                game.system = "homebrew".into();
                view.games.push(game);
            }
            continue;
        }
        if let Some(game) = image_entry(&entry, device) {
            view.games.push(game);
        }
    }
}

fn read_homebrew_folder(
    conn: &mut suppaftp::FtpStream,
    entry: &ftp::RemoteEntry,
    root: &str,
    device: &str,
) -> Option<GameEntry> {
    // An image file inside the folder wins: that is what ShadowMount+ mounts.
    let children = ftp::list(conn, &entry.path).unwrap_or_default();
    let image = children
        .iter()
        .find(|child| !child.is_dir && is_image(&child.name));
    let sfo = ["sce_sys/param.sfo", "param.sfo"]
        .iter()
        .find_map(|relative| {
            let head = ftp::read_head(conn, &format!("{}/{relative}", entry.path), 512 * 1024).ok()?;
            Sfo::parse(&head).ok()
        });
    let title = sfo
        .as_ref()
        .and_then(|s| s.title())
        .map(|s| s.to_string())
        .unwrap_or_else(|| title_from_filename(&entry.name));
    let title_id = sfo
        .as_ref()
        .and_then(|s| s.title_id())
        .map(|s| s.to_string())
        .or_else(|| game_id_from_filename(&entry.name));
    let version = sfo.as_ref().and_then(|s| s.version()).map(|s| s.to_string());
    let (path, size, icon_path) = match image {
        Some(image) => (
            image.path.clone(),
            image.size_bytes,
            Some(format!("{}/sce_sys/icon0.png", entry.path)),
        ),
        None => (entry.path.clone(), 0, None),
    };
    let _ = root;
    Some(GameEntry {
        id: format!("image:{path}"),
        title,
        title_id,
        system: "homebrew".into(),
        kind: if image.is_some() { "image" } else { "homebrew" }.into(),
        category: "homebrew".into(),
        path,
        size_bytes: size,
        has_icon: icon_path.is_some(),
        icon_path,
        version,
        device: device.into(),
    })
}

fn image_entry(entry: &ftp::RemoteEntry, device: &str) -> Option<GameEntry> {
    if !is_image(&entry.name) {
        return None;
    }
    Some(GameEntry {
        id: format!("image:{}", entry.path),
        title: title_from_filename(&entry.name),
        title_id: game_id_from_filename(&entry.name),
        system: "homebrew".into(),
        kind: "image".into(),
        category: "homebrew".into(),
        path: entry.path.clone(),
        size_bytes: entry.size_bytes,
        has_icon: false,
        icon_path: None,
        version: None,
        device: device.into(),
    })
}

/// Packages waiting for PS5Upload or PKG Manager to install.
fn scan_packages(
    conn: &mut suppaftp::FtpStream,
    dir: &str,
    device: &str,
    view: &mut LibraryView,
) {
    let Ok(entries) = ftp::list(conn, dir) else {
        return;
    };
    view.roots.push(dir.to_string());
    for entry in entries.iter().filter(|e| !e.is_dir && pkg::is_pkg_name(&e.name)) {
        let head = ftp::read_head(conn, &entry.path, 4096).unwrap_or_default();
        let info = pkg::info_from_head(
            entry.path.clone(),
            entry.name.clone(),
            entry.size_bytes,
            &head,
        );
        view.packages.push(GameEntry {
            id: format!("pkg:{}", entry.path),
            title: info.title_id.clone().unwrap_or_else(|| title_from_filename(&entry.name)),
            title_id: info.title_id,
            system: "pkg".into(),
            kind: "pkg".into(),
            category: "package".into(),
            path: entry.path.clone(),
            size_bytes: entry.size_bytes,
            has_icon: false,
            icon_path: None,
            version: None,
            device: device.into(),
        });
    }
}

/// Which `/mnt` roots look like removable drives.
fn list_external_devices(conn: &mut suppaftp::FtpStream) -> Vec<String> {
    let Ok(entries) = ftp::list(conn, "/mnt") else {
        return Vec::new();
    };
    let mut devices: Vec<String> = entries
        .iter()
        .filter(|e| e.is_dir)
        .map(|e| e.name.clone())
        .filter(|name| name.starts_with("usb") || name.starts_with("ext"))
        .collect();
    devices.sort();
    devices
}

pub fn is_image(name: &str) -> bool {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    IMAGE_EXTS.contains(&ext.as_str())
}

/// PPSA / PPSX / EPPA and friends are PS5; CUSA is PS4 content running on a
/// PS5. Everything else is reported as homebrew-neutral.
pub fn system_for_id(title_id: &str) -> &'static str {
    let upper = title_id.to_ascii_uppercase();
    if upper.starts_with("CUSA") {
        "ps4"
    } else if ["PPSA", "PPSX", "EPPA", "ELJM", "ELAS", "ELAM"]
        .iter()
        .any(|prefix| upper.starts_with(prefix))
    {
        "ps5"
    } else {
        "unknown"
    }
}

/// `PPSA01234 - God of War Ragnarok.exfat` -> `God of War Ragnarok`.
pub fn title_from_filename(name: &str) -> String {
    let stem = name.rsplit('/').next().unwrap_or(name);
    let mut stem = stem.to_string();
    for ext in ["iso", "pkg", "exfat", "ffpkg", "ffpfs", "ffpfsc", "zip", "elf", "bin", "img"] {
        let suffix = format!(".{ext}");
        if stem.to_lowercase().ends_with(&suffix) {
            stem.truncate(stem.len() - suffix.len());
            break;
        }
    }
    let before_tag = stem.split(" (").next().unwrap_or(&stem);
    let without_id = before_tag
        .split_once(" - ")
        .map(|(head, tail)| {
            if pkg::is_title_id(head.trim()) {
                tail
            } else {
                before_tag
            }
        })
        .unwrap_or(before_tag);
    let cleaned = without_id.trim().trim_end_matches('.').to_string();
    if cleaned.is_empty() {
        title_id_or(stem)
    } else {
        cleaned
    }
}

fn title_id_or(text: String) -> String {
    game_id_from_filename(&text).unwrap_or(text)
}

/// Pull `PPSA01234` out of `PPSA01234 - Title.exfat` or `Title [PPSA01234]`.
pub fn game_id_from_filename(name: &str) -> Option<String> {
    let upper = name.to_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() < 9 {
        return None;
    }
    for index in 0..=bytes.len() - 9 {
        let window = &bytes[index..index + 9];
        let Ok(text) = std::str::from_utf8(window) else {
            continue;
        };
        if !pkg::is_title_id(text) {
            continue;
        }
        let before_ok = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
        let after_ok = index + 9 >= bytes.len() || !bytes[index + 9].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(text.to_string());
        }
    }
    None
}

/// Download and cache ICON0 for a library entry.
pub fn icon_bytes(host: &Host, icon_path: &str) -> Option<Vec<u8>> {
    let cache = icon_cache_path(icon_path);
    if let Ok(bytes) = std::fs::read(&cache) {
        return Some(bytes);
    }
    let miss = cache.with_extension("miss");
    if miss.exists() {
        return None;
    }
    let mut conn = ftp::connect(host).ok()?;
    let bytes = ftp::read_head(&mut conn, icon_path, 2 * 1024 * 1024).ok()?;
    let _ = conn.quit();
    if bytes.is_empty() {
        // A missing icon is a stable answer; remember it. A connection failure
        // returns above without touching the cache.
        if let Some(parent) = miss.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&miss, b"");
        return None;
    }
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &bytes);
    Some(bytes)
}

fn icon_cache_path(icon_path: &str) -> PathBuf {
    crate::config::icons_cache_dir().join(format!("{:016x}.png", fnv1a(icon_path)))
}

fn fnv1a(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Roots a delete may touch. Nothing else on the console is ours to remove.
pub const DELETE_ROOTS: &[&str] = &[
    "/user/app",
    "/user/appmeta",
    "/data/homebrew",
    "/data/etaHEN/games",
    "/data/ps5upload/pkg_library",
    "/user/data/ps5upload/pkg_library",
    "/data/pkg",
];

/// True when `path` sits below a known root and is not the root itself.
pub fn deletable(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    if path.contains("..") {
        return false;
    }
    DELETE_ROOTS.iter().any(|root| {
        path.strip_prefix(root)
            .map(|rest| rest.starts_with('/') && rest.len() > 1)
            .unwrap_or(false)
            || (path.starts_with("/mnt/") && path.contains("/homebrew/"))
    })
}

/// Delete a game folder, image or package from the console.
pub fn delete_entry(host: &Host, entry_path: &str, is_dir: bool) -> Result<()> {
    if !deletable(entry_path) {
        return Err(crate::error::Error::Invalid(format!(
            "refusing to delete {entry_path}; it is outside the managed folders"
        )));
    }
    let mut conn = ftp::connect(host)?;
    let result = ftp::remove(&mut conn, entry_path, is_dir);
    let _ = conn.quit();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_filenames() {
        assert_eq!(
            title_from_filename("PPSA01234 - God of War Ragnarok.exfat"),
            "God of War Ragnarok"
        );
        assert_eq!(
            title_from_filename("/data/homebrew/Demon's Souls.exfat"),
            "Demon's Souls"
        );
        assert_eq!(title_from_filename("Game (USA).ffpkg"), "Game");
        assert_eq!(title_from_filename("PPSA01234"), "PPSA01234");
    }

    #[test]
    fn ids_from_filenames() {
        assert_eq!(
            game_id_from_filename("PPSA01234 - Game.ffpkg").as_deref(),
            Some("PPSA01234")
        );
        assert_eq!(
            game_id_from_filename("Game [CUSA12345].exfat").as_deref(),
            Some("CUSA12345")
        );
        assert_eq!(game_id_from_filename("No Id Here.exfat"), None);
    }

    #[test]
    fn systems_follow_the_title_id() {
        assert_eq!(system_for_id("PPSA01234"), "ps5");
        assert_eq!(system_for_id("CUSA12345"), "ps4");
        assert_eq!(system_for_id("GDS00001"), "unknown");
    }

    #[test]
    fn image_extensions() {
        assert!(is_image("Game.exfat"));
        assert!(is_image("Game.FFPKG"));
        assert!(is_image("Game.ffpfsc"));
        assert!(!is_image("Game.pkg"));
        assert!(!is_image("Game.elf"));
    }

    #[test]
    fn only_managed_folders_are_deletable() {
        assert!(deletable("/user/app/PPSA01234"));
        assert!(deletable("/data/homebrew/Game/x.exfat"));
        assert!(deletable("/mnt/usb0/homebrew/Game"));
        assert!(!deletable("/user/app"));
        assert!(!deletable("/"));
        assert!(!deletable("/user/app/../system"));
        assert!(!deletable("/system/x"));
        assert!(!deletable(""));
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(fnv1a("abc"), fnv1a("abc"));
        assert_ne!(fnv1a("abc"), fnv1a("abd"));
    }
}
