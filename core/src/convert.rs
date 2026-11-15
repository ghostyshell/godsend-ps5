//! Conversions the UI and CLI offer for local content.
//!
//! These are the ones that can be done correctly on a desktop without console
//! keys: ISO9660 build and extract, FAT32 split and merge, and ZIP pack and
//! unpack. ShadowMount+ image formats (`.exfat`, `.ffpkg`, `.ffpfs`,
//! `.ffpfsc`) are built by the console-side tooling and are inspected here,
//! not rewritten.

use crate::config::{Config, Host};
use crate::error::{Error, Result};
use crate::install;
use crate::iso;
use crate::isobuild;
use crate::library;
use crate::pkg;
use crate::split;
use crate::transfer::JobContext;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ConvertInfo {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    /// `iso` | `folder` | `pkg` | `image` | `elf` | `zip` | `split` | `other`
    pub kind: String,
    /// `ps5` | `ps4` | `homebrew` | `pkg` | `unknown`
    pub system: String,
    pub title_id: Option<String>,
    /// Number of parts found for a split set.
    pub parts: usize,
}

/// Inspect any local path the Convert page can act on.
pub fn inspect(path: &Path) -> Result<ConvertInfo> {
    let metadata = std::fs::metadata(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut info = ConvertInfo {
        path: path.to_string_lossy().to_string(),
        name: name.clone(),
        size_bytes: metadata.len(),
        kind: "other".into(),
        system: "unknown".into(),
        title_id: None,
        parts: 0,
    };

    // A `.0` file is the head of a split set: report the joined size.
    let parts = split::find_parts(path);
    if parts.len() > 1 {
        info.kind = "split".into();
        info.parts = parts.len();
        info.size_bytes = parts
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
            .sum();
        let head = split::read_head(&parts, 4096).unwrap_or_default();
        if head.len() >= 4 && head[0..4] == pkg::MAGIC {
            info.system = "pkg".into();
        }
        return Ok(info);
    }

    if path.is_dir() {
        info.kind = "folder".into();
        info.system = "homebrew".into();
        info.title_id = install::folder_title_id(path);
        return Ok(info);
    }

    let lower = name.to_lowercase();
    if lower.ends_with(".pkg") {
        info.kind = "pkg".into();
        info.system = "pkg".into();
        if let Ok(p) = pkg::info(path) {
            info.title_id = p.title_id;
        }
    } else if library::is_image(&name) && !lower.ends_with(".iso") {
        info.kind = "image".into();
        info.system = "homebrew".into();
    } else if lower.ends_with(".iso") {
        info.kind = "iso".into();
        if let Ok(mut image) = iso::Iso::open(path) {
            info.system = image.system();
            info.title_id = image.title_id();
        }
    } else if lower.ends_with(".zip") {
        info.kind = "zip".into();
    } else if lower.ends_with(".elf") || lower.ends_with(".bin") {
        info.kind = "elf".into();
    }
    info.title_id = info
        .title_id
        .or_else(|| library::game_id_from_filename(&name));
    Ok(info)
}

pub fn folder_to_iso(src: &Path, out: &Path, ctx: &JobContext) -> Result<u64> {
    if !src.is_dir() {
        return Err(Error::Invalid(format!("{} is not a folder", src.display())));
    }
    ctx.message("building ISO9660 + Joliet image");
    isobuild::build_iso(src, out, "PS5DATA", ctx)
}

pub fn iso_to_folder(src: &Path, out: &Path, ctx: &JobContext) -> Result<u64> {
    let mut image = iso::Iso::open(src)?;
    ctx.message(format!(
        "extracting {} entries{}",
        image.nodes.len(),
        if image.joliet { " (Joliet names)" } else { "" }
    ));
    std::fs::create_dir_all(out)?;
    image.extract(out, ctx)
}

pub fn split_file(src: &Path, chunk: u64, ctx: &JobContext) -> Result<Vec<PathBuf>> {
    ctx.message(format!("splitting into {} MiB parts", chunk / (1024 * 1024)));
    split::split(src, chunk, ctx)
}

pub fn merge_file(base: &Path, out: &Path, ctx: &JobContext) -> Result<u64> {
    let parts = split::find_parts(base);
    if parts.is_empty() {
        return Err(Error::NotFound(format!(
            "no parts found for {} (expected {}.0, {}.1, ...)",
            base.display(),
            base.display(),
            base.display()
        )));
    }
    ctx.message(format!("merging {} parts", parts.len()));
    split::merge(&parts, out, ctx)
}

/// Pack a folder into a ZIP, skipping macOS resource-fork noise.
pub fn folder_to_zip(src: &Path, out: &Path, ctx: &JobContext) -> Result<u64> {
    if !src.is_dir() {
        return Err(Error::Invalid(format!("{} is not a folder", src.display())));
    }
    let entries = iso::walk_tree(src)?;
    let total: u64 = entries
        .iter()
        .filter(|(_, is_dir)| !is_dir)
        .filter_map(|(path, _)| std::fs::metadata(path).ok().map(|m| m.len()))
        .sum();
    ctx.message(format!("packing {} files", entries.len()));
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut done = 0u64;
    for (path, is_dir) in entries {
        ctx.check_cancel()?;
        let rel = path.strip_prefix(src).unwrap_or(&path);
        let name = rel.to_string_lossy().replace('\\', "/");
        if name.starts_with("__MACOSX") || name.rsplit('/').next().unwrap_or("").starts_with("._") {
            continue;
        }
        if is_dir {
            zip.add_directory(format!("{name}/"), options)?;
            continue;
        }
        zip.start_file(name, options)?;
        let mut input = std::fs::File::open(&path)?;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let read = input.read(&mut buf)?;
            if read == 0 {
                break;
            }
            zip.write_all(&buf[..read])?;
            done += read as u64;
            ctx.progress(done, total.max(done));
        }
    }
    zip.finish()?;
    Ok(done)
}

/// Extract a ZIP into `out`, keeping every entry inside it.
pub fn zip_to_folder(src: &Path, out: &Path, ctx: &JobContext) -> Result<u64> {
    let file = std::fs::File::open(src)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.size()))
        .sum();
    std::fs::create_dir_all(out)?;
    let mut done = 0u64;
    for i in 0..archive.len() {
        ctx.check_cancel()?;
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        if name
            .components()
            .any(|c| c.as_os_str() == "__MACOSX" || c.as_os_str().to_string_lossy().starts_with("._"))
        {
            continue;
        }
        let target = out.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut outfile)?;
        done += entry.size();
        ctx.progress(done, total);
    }
    Ok(done)
}

/// One-shot: make a local file installable and push it to the console.
pub fn push_to_console(
    host: &Host,
    local: &Path,
    config: &Config,
    ctx: &JobContext,
) -> Result<install::InstallReport> {
    install::install_local(host, local, config, ctx)
}

/// Destination folder on the console for a local path, for the UI preview.
pub fn install_destination(local: &Path, config: &Config) -> String {
    let homebrew = config.homebrew_dir.trim_end_matches('/');
    let pkg_dir = config.pkg_dir.trim_end_matches('/');
    match install::classify(local) {
        install::LocalKind::Pkg => pkg_dir.to_string(),
        install::LocalKind::Image | install::LocalKind::Folder => {
            format!("{homebrew}/{}", install::install_folder_name(local))
        }
        install::LocalKind::Elf => "elfldr (payload port)".to_string(),
        install::LocalKind::Zip => "(unpacked, then its contents)".to_string(),
        install::LocalKind::Other => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zips_and_unzips_a_folder() {
        let base = std::env::temp_dir().join(format!("godsend-ps5-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::write(src.join("sub/b.bin"), vec![7u8; 1024]).unwrap();
        let archive = base.join("out.zip");
        let extracted = base.join("out");
        let manager = crate::transfer::JobManager::new();
        let job_src = src.clone();
        let job_archive = archive.clone();
        let job_extracted = extracted.clone();
        manager.spawn(
            "convert",
            "zip",
            |_| {},
            move |ctx| {
                folder_to_zip(&job_src, &job_archive, ctx)?;
                zip_to_folder(&job_archive, &job_extracted, ctx)?;
                Ok(())
            },
        );
        // Wait for the job thread.
        for _ in 0..100 {
            if extracted.join("sub/b.bin").exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(std::fs::read(extracted.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(extracted.join("sub/b.bin")).unwrap().len(), 1024);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn inspects_a_folder() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-inspect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sce_sys")).unwrap();
        std::fs::write(
            dir.join("sce_sys/PARAM.SFO"),
            crate::sfo::build_sfo(&[
                ("TITLE", crate::sfo::SfoValue::Utf8("Demo".into())),
                ("TITLE_ID", crate::sfo::SfoValue::Utf8("PPSA01234".into())),
            ]),
        )
        .unwrap();
        let info = inspect(&dir).unwrap();
        assert_eq!(info.kind, "folder");
        assert_eq!(info.title_id.as_deref(), Some("PPSA01234"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_destination_matches_the_console_layout() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-dest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config::default();
        let pkg = dir.join("a.pkg");
        std::fs::write(&pkg, b"x").unwrap();
        assert_eq!(install_destination(&pkg, &config), "/data/ps5upload/pkg_library");
        let image = dir.join("PPSA01234 - Game.ffpkg");
        std::fs::write(&image, b"x").unwrap();
        assert_eq!(install_destination(&image, &config), "/data/homebrew/PPSA01234");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
