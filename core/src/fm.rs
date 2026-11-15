//! File manager: local folders plus the console's filesystem over FTP.

use crate::config::Host;
use crate::error::{Error, Result};
use crate::ftp;
use crate::transfer::JobContext;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct FmEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    /// `iso` | `pkg` | `zip` | `folder` | `split` | `other`
    pub kind: String,
}

pub fn local_list(path: &Path) -> Result<Vec<FmEntry>> {
    if !path.is_dir() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(path)?.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with("._") {
            continue;
        }
        let is_dir = p.is_dir();
        out.push(FmEntry {
            kind: kind_for(&name, is_dir),
            name,
            path: p.to_string_lossy().to_string(),
            is_dir,
            size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub fn remote_list(host: &Host, path: &str) -> Result<Vec<FmEntry>> {
    let mut conn = ftp::connect(host)?;
    let entries = ftp::list(&mut conn, path)?;
    let _ = conn.quit();
    Ok(entries
        .into_iter()
        .map(|e| FmEntry {
            kind: kind_for(&e.name, e.is_dir),
            name: e.name,
            path: e.path,
            is_dir: e.is_dir,
            size_bytes: e.size_bytes,
        })
        .collect())
}

pub fn kind_for(name: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".into();
    }
    let lower = name.to_lowercase();
    if lower.ends_with(".iso") {
        "iso".into()
    } else if lower.ends_with(".pkg") {
        "pkg".into()
    } else if lower.ends_with(".zip") {
        "zip".into()
    } else if lower.ends_with(".iso.0") || lower.ends_with(".pkg.0") {
        "split".into()
    } else {
        "other".into()
    }
}

pub fn mkdir_local(parent: &Path, name: &str) -> Result<PathBuf> {
    let path = parent.join(safe_component(name)?);
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn mkdir_remote(host: &Host, path: &str) -> Result<()> {
    let mut conn = ftp::connect(host)?;
    let result = ftp::mkdir_all(&mut conn, path);
    let _ = conn.quit();
    result
}

pub fn delete_local(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn delete_remote(host: &Host, path: &str, is_dir: bool) -> Result<()> {
    let mut conn = ftp::connect(host)?;
    let result = ftp::remove(&mut conn, path, is_dir);
    let _ = conn.quit();
    result
}

/// Upload any local file or folder into `remote_dir`. A file going to a
/// `/dev_usb*` target larger than `split_bytes` is written as `.0`, `.1`, ...
/// parts, because console USB drives are FAT32.
pub fn upload(
    host: &Host,
    local: &Path,
    remote_dir: &str,
    split_bytes: u64,
    ctx: &JobContext,
) -> Result<String> {
    let mut conn = ftp::connect(host)?;
    let result = if local.is_dir() {
        crate::install::push_folder(&mut conn, local, remote_dir, ctx)
    } else {
        crate::install::push_file(&mut conn, local, remote_dir, split_bytes, ctx)
    };
    let _ = conn.quit();
    result
}

/// Download a remote file or folder into `local_dir`.
pub fn download(host: &Host, remote: &str, local_dir: &Path, ctx: &JobContext) -> Result<PathBuf> {
    let mut conn = ftp::connect(host)?;
    let name = remote
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("download");
    let name = safe_component(name)?;
    let target = local_dir.join(name);
    let result = (|| -> Result<PathBuf> {
        if ftp::is_dir(&mut conn, remote) {
            fs::create_dir_all(&target)?;
            download_tree(&mut conn, remote, &target, ctx)?;
        } else {
            ftp::download(&mut conn, remote, &target, ctx)?;
        }
        Ok(target)
    })();
    let _ = conn.quit();
    result
}

fn download_tree(
    conn: &mut suppaftp::FtpStream,
    remote: &str,
    local: &Path,
    ctx: &JobContext,
) -> Result<()> {
    for entry in ftp::list(conn, remote)? {
        let name = safe_component(&entry.name)?;
        let target = local.join(name);
        if entry.is_dir {
            fs::create_dir_all(&target)?;
            download_tree(conn, &entry.path, &target, ctx)?;
        } else {
            ftp::download(conn, &entry.path, &target, ctx)?;
        }
    }
    Ok(())
}

fn safe_component(name: &str) -> Result<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed == "."
        || trimmed == ".."
    {
        return Err(Error::Invalid(format!(
            "'{name}' is not a valid folder name"
        )));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_names() {
        assert_eq!(kind_for("game.iso", false), "iso");
        assert_eq!(kind_for("game.pkg", false), "pkg");
        assert_eq!(kind_for("game.iso.0", false), "split");
        assert_eq!(kind_for("folder", true), "folder");
        assert_eq!(kind_for("readme.txt", false), "other");
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_component("../etc").is_err());
        assert!(safe_component("a/b").is_err());
        assert_eq!(safe_component("My Game").unwrap(), "My Game");
    }

    #[test]
    fn lists_a_local_folder() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-fm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.iso"), b"x").unwrap();
        let entries = local_list(&dir).unwrap();
        assert_eq!(entries[0].name, "sub");
        assert_eq!(entries[1].kind, "iso");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
