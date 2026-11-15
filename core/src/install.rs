//! Installing content on the PS5: upload over FTP to the folders the console's
//! own tools watch, or hand an ELF straight to elfldr.
//!
//! Destination follows the PS5 homebrew layout:
//!
//! - PKG                    -> the PS5Upload `pkg_library` folder, or `/data/pkg`
//! - image (`.exfat`, `.ffpkg`, `.ffpfs`, `.ffpfsc`) -> `<homebrew_dir>/<id>/`
//! - game folder dump       -> `<homebrew_dir>/<id>/`
//! - ELF payload            -> sent to elfldr on the payload port
//! - ZIP                    -> unpacked locally, then treated as a folder
//!
//! ShadowMount+ scans `/data/homebrew` every 15 seconds and mounts whatever
//! image appears there.

use crate::config::{Config, Host};
use crate::error::{Error, Result};
use crate::ftp;
use crate::iso;
use crate::library;
use crate::payload;
use crate::pkg;
use crate::sfo::Sfo;
use crate::split;
use crate::transfer::{dir_size, JobContext};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalKind {
    Pkg,
    Image,
    Folder,
    Elf,
    Zip,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub remote_path: String,
    pub kind: String,
    pub bytes: u64,
    /// True when the file landed in an install folder the console watches.
    pub install_requested: bool,
    pub message: String,
}

/// What kind of local content this path is.
pub fn classify(path: &Path) -> LocalKind {
    if path.is_dir() {
        return LocalKind::Folder;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pkg" => LocalKind::Pkg,
        "elf" | "bin" => LocalKind::Elf,
        "zip" => LocalKind::Zip,
        _ if library::is_image(&name) => LocalKind::Image,
        _ => LocalKind::Other,
    }
}

/// Folder name for an install: the title id when we can find one, else the
/// cleaned name.
pub fn install_folder_name(path: &Path) -> String {
    if let Some(id) = pkg::is_pkg(path)
        .then(|| pkg::info(path).ok())
        .flatten()
        .and_then(|info| info.title_id)
    {
        return id;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "homebrew".into());
    if let Some(id) = library::game_id_from_filename(&name) {
        return id;
    }
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(&name);
    clean_component(stem)
}

/// Title id from a local folder dump's PARAM.SFO, when it has one.
pub fn folder_title_id(path: &Path) -> Option<String> {
    for relative in ["sce_sys/PARAM.SFO", "param.sfo"] {
        let Ok(bytes) = std::fs::read(path.join(relative)) else {
            continue;
        };
        if let Ok(sfo) = Sfo::parse(&bytes) {
            if let Some(id) = sfo.title_id() {
                // Console content is untrusted: only a real title id may become
                // part of a remote path.
                if pkg::is_title_id(id) {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}

fn folder_name_for(path: &Path) -> String {
    if let Some(id) = folder_title_id(path) {
        return id;
    }
    install_folder_name(path)
}

/// Upload one file, splitting it when the destination is a FAT32 USB device
/// and the file is above the split limit.
pub fn push_file(
    conn: &mut suppaftp::FtpStream,
    local: &Path,
    remote_dir: &str,
    split_bytes: u64,
    ctx: &JobContext,
) -> Result<String> {
    let size = std::fs::metadata(local)?.len();
    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| Error::Invalid("file has no name".into()))?;
    let target = format!("{}/{}", remote_dir.trim_end_matches('/'), name);
    ftp::mkdir_all(conn, remote_dir)?;
    if remote_dir.starts_with("/mnt/") && split_bytes > 0 && size > split_bytes {
        ctx.message(format!("splitting {name} for FAT32"));
        let parts = split::split(local, split_bytes, ctx)?;
        let mut first = target.clone();
        for (index, part) in parts.iter().enumerate() {
            let part_name = part.file_name().unwrap_or_default().to_string_lossy();
            let remote = format!("{}/{}", remote_dir.trim_end_matches('/'), part_name);
            if index == 0 {
                first = remote.clone();
            }
            ftp::upload(conn, part, &remote, ctx)?;
            let _ = std::fs::remove_file(part);
        }
        return Ok(first);
    }
    ftp::upload(conn, local, &target, ctx)?;
    Ok(target)
}

/// Upload a folder tree under `remote_parent/<folder name>`.
pub fn push_folder(
    conn: &mut suppaftp::FtpStream,
    local: &Path,
    remote_parent: &str,
    ctx: &JobContext,
) -> Result<String> {
    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| Error::Invalid("folder has no name".into()))?;
    let base = format!("{}/{}", remote_parent.trim_end_matches('/'), name);
    let entries = iso::walk_tree(local)?;
    let total = dir_size(local);
    let mut done = 0u64;
    ctx.progress(0, total);
    ftp::mkdir_all(conn, &base)?;
    for (path, is_dir) in entries {
        ctx.check_cancel()?;
        let rel = path.strip_prefix(local).unwrap_or(&path);
        let remote = format!("{}/{}", base, rel.to_string_lossy().replace('\\', "/"));
        if is_dir {
            ftp::mkdir_all(conn, &remote)?;
            continue;
        }
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        ftp::upload(conn, &path, &remote, ctx)?;
        done += size;
        ctx.progress(done, total);
    }
    Ok(base)
}

/// An extracted archive: the folder to install and the temp root to remove.
pub struct Unpacked {
    pub source: PathBuf,
    pub temp_root: PathBuf,
}

/// Unpack a ZIP into a unique temp directory and return the folder that looks
/// like the content root.
pub fn unpack_zip(zip_path: &Path, ctx: &JobContext) -> Result<Unpacked> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let slot = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let temp_root = std::env::temp_dir().join(format!(
        "godsend-ps5-unzip-{}-{nanos}-{slot}",
        std::process::id()
    ));
    // create_dir, not create_dir_all: two jobs must never share a folder.
    std::fs::create_dir(&temp_root)?;
    let extract = temp_root.join("content");
    std::fs::create_dir_all(&extract)?;

    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.size()))
        .sum();
    let mut done = 0u64;
    for i in 0..archive.len() {
        ctx.check_cancel()?;
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        // Skip macOS resource forks, the same noise installs prune.
        if name
            .components()
            .any(|c| c.as_os_str() == "__MACOSX" || c.as_os_str().to_string_lossy().starts_with("._"))
        {
            continue;
        }
        let out = extract.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut outfile)?;
        done += entry.size();
        ctx.progress(done, total);
    }

    let root = find_game_root(&extract).unwrap_or_else(|| extract.clone());
    // A flat archive puts the game root at the extraction folder itself. Name
    // the installed folder after the archive, never after the temp directory.
    let source = if root == extract {
        let stem = zip_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "homebrew".into());
        let named = temp_root.join(clean_component(&stem));
        std::fs::rename(&extract, &named)?;
        named
    } else {
        root
    };
    Ok(Unpacked { source, temp_root })
}

fn find_game_root(dir: &Path) -> Option<PathBuf> {
    if dir.join("sce_sys").join("PARAM.SFO").is_file()
        || dir.join("PARAM.SFO").is_file()
        || dir.join("eboot.bin").is_file()
        || dir.join("EBOOT.BIN").is_file()
    {
        return Some(dir.to_path_buf());
    }
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    let dirs: Vec<_> = entries.iter().filter(|e| e.path().is_dir()).collect();
    if dirs.len() == 1 {
        return find_game_root(&dirs[0].path());
    }
    for entry in entries {
        if entry.path().is_dir() {
            if let Some(found) = find_game_root(&entry.path()) {
                return Some(found);
            }
        }
    }
    None
}

/// Full install flow for one local path.
pub fn install_local(
    host: &Host,
    local: &Path,
    config: &Config,
    ctx: &JobContext,
) -> Result<InstallReport> {
    let kind = classify(local);
    match kind {
        LocalKind::Elf => {
            ctx.message("sending payload to elfldr");
            let report = payload::send_file(host, local)?;
            Ok(InstallReport {
                remote_path: format!("elfldr:{}", host.payload_port),
                kind: "elf".into(),
                bytes: report.bytes,
                install_requested: true,
                message: if report.reply.is_empty() {
                    "payload sent; the console logs the result in its kernel log".into()
                } else {
                    format!("payload sent: {}", report.reply)
                },
            })
        }
        LocalKind::Zip => {
            ctx.message("unpacking archive");
            let unpacked = unpack_zip(local, ctx)?;
            let result = install_local(host, &unpacked.source, config, ctx);
            let _ = std::fs::remove_dir_all(&unpacked.temp_root);
            result
        }
        LocalKind::Other => Err(Error::Invalid(format!(
            "{} is not a PKG, image, game folder, ELF or ZIP",
            local.display()
        ))),
        LocalKind::Pkg | LocalKind::Image | LocalKind::Folder => {
            // Only these need FTP. An ELF install must work with elfldr alone,
            // and a ZIP must not hold an idle control connection while it unpacks.
            let mut conn = ftp::connect(host)?;
            let result = (|| -> Result<InstallReport> {
                match kind {
                    LocalKind::Pkg => {
                        let info = pkg::info(local).unwrap_or_else(|_| pkg::info_from_head(
                            local.to_string_lossy().to_string(),
                            local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                            std::fs::metadata(local).map(|m| m.len()).unwrap_or(0),
                            &[],
                        ));
                        ctx.message("uploading package");
                        let mut last_error = None;
                        let mut remote = None;
                        for dir in pkg::pkg_dirs(&config.pkg_dir) {
                            match push_file(&mut conn, local, &dir, 0, ctx) {
                                Ok(path) => {
                                    remote = Some(path);
                                    break;
                                }
                                Err(error) => last_error = Some(error),
                            }
                        }
                        let remote = remote.ok_or_else(|| {
                            last_error.unwrap_or_else(|| Error::Ftp("no package folder accepted the upload".into()))
                        })?;
                        Ok(InstallReport {
                            remote_path: remote.clone(),
                            kind: "pkg".into(),
                            bytes: info.size_bytes,
                            install_requested: config.auto_install_pkg,
                            message: if config.auto_install_pkg {
                                format!("uploaded to {remote}; PS5Upload or PKG Manager installs it from there")
                            } else {
                                format!("uploaded to {remote}; install it from the console")
                            },
                        })
                    }
                    LocalKind::Image => {
                        let folder = install_folder_name(local);
                        let base = format!("{}/{}", config.homebrew_dir.trim_end_matches('/'), folder);
                        ctx.message(format!("uploading image to {base}"));
                        let bytes = std::fs::metadata(local)?.len();
                        let remote = push_file(&mut conn, local, &base, config.split_bytes, ctx)?;
                        Ok(InstallReport {
                            remote_path: remote,
                            kind: "image".into(),
                            bytes,
                            install_requested: true,
                            message: format!(
                                "uploaded to {base}; ShadowMount+ mounts it within 15 seconds"
                            ),
                        })
                    }
                    LocalKind::Folder => {
                        let folder = folder_name_for(local);
                        let base = format!("{}/{}", config.homebrew_dir.trim_end_matches('/'), folder);
                        ctx.message(format!("uploading folder to {base}"));
                        let total = dir_size(local);
                        let entries = iso::walk_tree(local)?;
                        let mut done = 0u64;
                        ftp::mkdir_all(&mut conn, &base)?;
                        for (path, is_dir) in entries {
                            ctx.check_cancel()?;
                            let rel = path.strip_prefix(local).unwrap_or(&path);
                            let remote = format!("{}/{}", base, rel.to_string_lossy().replace('\\', "/"));
                            if is_dir {
                                ftp::mkdir_all(&mut conn, &remote)?;
                                continue;
                            }
                            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            ftp::upload(&mut conn, &path, &remote, ctx)?;
                            done += size;
                            ctx.progress(done, total);
                        }
                        Ok(InstallReport {
                            remote_path: base.clone(),
                            kind: "folder".into(),
                            bytes: total,
                            install_requested: true,
                            message: format!("uploaded to {base}; ShadowMount+ mounts it within 15 seconds"),
                        })
                    }
                    _ => unreachable!(),
                }
            })();
            let _ = conn.quit();
            result
        }
    }
}

/// One safe path component: no separators, no leading or trailing dots.
fn clean_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    if cleaned.is_empty() {
        "homebrew".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_local_paths() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sce_sys")).unwrap();
        std::fs::write(dir.join("sce_sys/PARAM.SFO"), b"\0PSF").unwrap();
        assert_eq!(classify(&dir), LocalKind::Folder);
        for (name, kind) in [
            ("game.pkg", LocalKind::Pkg),
            ("game.exfat", LocalKind::Image),
            ("game.ffpkg", LocalKind::Image),
            ("payload.elf", LocalKind::Elf),
            ("game.zip", LocalKind::Zip),
            ("game.rar", LocalKind::Other),
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
            assert_eq!(classify(&dir.join(name)), kind, "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_flat_zip_is_installed_under_the_archive_name() {
        let base = std::env::temp_dir().join(format!("godsend-ps5-flat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/sce_sys")).unwrap();
        std::fs::write(base.join("src/sce_sys/PARAM.SFO"), b"\0PSFfake").unwrap();
        let archive = base.join("My Dump.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("sce_sys/PARAM.SFO", options).unwrap();
        use std::io::Write;
        zip.write_all(b"\0PSFfake").unwrap();
        zip.finish().unwrap();

        let manager = crate::transfer::JobManager::new();
        let src = archive.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        manager.spawn(
            "test",
            "unzip",
            |_| {},
            move |ctx| {
                let unpacked = unpack_zip(&src, ctx)?;
                let _ = tx.send(unpacked);
                Ok(())
            },
        );
        let unpacked = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("unpacked");
        assert_eq!(
            unpacked.source.file_name().unwrap().to_string_lossy(),
            "My Dump"
        );
        let _ = std::fs::remove_dir_all(&unpacked.temp_root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_a_title_id_that_is_a_path() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-badid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sce_sys")).unwrap();
        std::fs::write(
            dir.join("sce_sys/PARAM.SFO"),
            crate::sfo::build_sfo(&[(
                "TITLE_ID",
                crate::sfo::SfoValue::Utf8("../../../user/app/PPSA00000".into()),
            )]),
        )
        .unwrap();
        assert_eq!(folder_title_id(&dir), None);
        assert!(!folder_name_for(&dir).contains('/'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_names_prefer_the_title_id() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image = dir.join("PPSA01234 - God of War.exfat");
        std::fs::write(&image, b"x").unwrap();
        assert_eq!(install_folder_name(&image), "PPSA01234");
        let plain = dir.join("My Homebrew.elf");
        std::fs::write(&plain, b"x").unwrap();
        assert_eq!(install_folder_name(&plain), "My Homebrew");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
