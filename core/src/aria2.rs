//! aria2c-powered downloads with a plain HTTP fallback.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::transfer::JobContext;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find aria2c: configured path, app bundle resources, then PATH.
pub fn find_aria2(configured: &str) -> Option<PathBuf> {
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        let named_aria2 = p
            .file_name()
            .map(|n| n.eq_ignore_ascii_case("aria2c") || n.eq_ignore_ascii_case("aria2c.exe"))
            .unwrap_or(false);
        if p.is_file() && named_aria2 {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join("aria2c"), dir.join("../Resources/aria2c")] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    which("aria2c")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Default download directory: config override, else `<Downloads>/godsend-ps5`.
pub fn download_dir(config: &Config) -> PathBuf {
    if !config.download_dir.is_empty() {
        return PathBuf::from(&config.download_dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let downloads = if home.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(&home).join("Downloads")
    };
    downloads.join("godsend-ps5")
}

/// Download `url` to `dest_dir/dest_name`, reporting progress.
pub fn download(
    url: &str,
    dest_dir: &Path,
    dest_name: &str,
    config: &Config,
    ctx: &JobContext,
) -> Result<PathBuf> {
    let lower = url.trim().to_ascii_lowercase();
    if !["http://", "https://", "ftp://", "magnet:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Err(Error::Invalid(format!(
            "refusing a download URL with an unsupported scheme: {url}"
        )));
    }
    std::fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(dest_name);
    match find_aria2(&config.aria2_path) {
        Some(aria2) => {
            ctx.message("downloading (aria2c, 16 connections)");
            run_aria2(&aria2, url, dest_dir, &dest, ctx)
        }
        None => {
            ctx.message("downloading (single connection; install aria2c for speed)");
            run_fallback(url, &dest, ctx)
        }
    }
}

fn run_aria2(
    aria2: &Path,
    url: &str,
    dest_dir: &Path,
    dest: &Path,
    ctx: &JobContext,
) -> Result<PathBuf> {
    use std::io::BufRead;
    let name = dest
        .file_name()
        .ok_or_else(|| Error::Invalid("no file name".into()))?;
    let mut child = Command::new(aria2)
        .args([
            "-x",
            "16",
            "-s",
            "16",
            "-k",
            "1M",
            "--file-allocation=none",
            "--console-log-level=warn",
            "--summary-interval=2",
            "--continue=true",
            "--auto-file-renaming=false",
            "--allow-overwrite=true",
            "-d",
        ])
        .arg(dest_dir)
        .arg("-o")
        .arg(name)
        .arg("--")
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("aria2c failed to start: {e}")))?;
    // Drain stderr on its own thread: a full pipe would block aria2c forever.
    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
            text
        })
    });
    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(std::result::Result::ok) {
            if ctx.check_cancel().is_err() {
                let _ = child.kill();
                let _ = std::fs::remove_file(dest);
                return Err(Error::Cancelled(ctx.job_id()));
            }
            if let (Some(open), Some(close)) = (line.rfind('('), line.rfind('%')) {
                if close > open {
                    if let Ok(pct) = line[open + 1..close].parse::<u64>() {
                        ctx.progress(pct, 100);
                    }
                }
            }
        }
    }
    let status = child.wait()?;
    if !status.success() {
        let stderr = stderr_thread
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let tail: String = stderr.lines().rev().take(4).collect::<Vec<_>>().join("; ");
        if tail.contains("Range") || tail.contains("range") || tail.contains("416") {
            return run_fallback(url, dest, ctx);
        }
        return Err(Error::Other(format!("aria2c exited badly: {tail}")));
    }
    if let Some(handle) = stderr_thread {
        let _ = handle.join();
    }
    let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    ctx.progress(size, size);
    Ok(dest.to_path_buf())
}

fn run_fallback(url: &str, dest: &Path, ctx: &JobContext) -> Result<PathBuf> {
    use std::io::{Read, Write};
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(3600))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| Error::Http(format!("download failed: {e}")))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut out = std::fs::File::create(dest)?;
    let mut buf = [0u8; 256 * 1024];
    let mut done = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        ctx.check_cancel()?;
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        out.write_all(&buf[..read])?;
        done += read as u64;
        if last.elapsed().as_millis() > 200 {
            ctx.progress(done, total);
            last = std::time::Instant::now();
        }
    }
    ctx.progress(done, total);
    Ok(dest.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_dir_default() {
        let mut cfg = Config::default();
        cfg.download_dir = "/tmp/custom-dl".into();
        assert_eq!(download_dir(&cfg), PathBuf::from("/tmp/custom-dl"));
        cfg.download_dir = String::new();
        assert!(download_dir(&cfg).ends_with("godsend-ps5"));
    }
}
