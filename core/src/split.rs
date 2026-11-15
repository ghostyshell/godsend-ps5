//! FAT32 split and merge for USB drives.
//!
//! Internal HDD installs never need this, but a USB drive formatted FAT32
//! cannot hold a file above 4 GiB. The console's file managers (multiMAN,
//! and mount tools) reassemble `<name>.0`, `<name>.1`, ... when mounting.
//! PKG sets from PSN use the same convention.

use crate::error::{Error, Result};
use crate::transfer::JobContext;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Default chunk: 4 GiB minus 1 MiB, safely under the FAT32 limit.
pub const DEFAULT_CHUNK: u64 = 4 * 1024 * 1024 * 1024 - 1024 * 1024;

pub fn part_name(src: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", src.to_string_lossy(), index))
}

/// Split `src` into `src.0`, `src.1`, ... and return the part paths.
pub fn split(src: &Path, chunk: u64, ctx: &JobContext) -> Result<Vec<PathBuf>> {
    if chunk == 0 {
        return Err(Error::Invalid(
            "split size must be greater than zero".into(),
        ));
    }
    let total = std::fs::metadata(src)?.len();
    let mut input = File::open(src)?;
    let mut parts = Vec::new();
    let mut done = 0u64;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut index = 0usize;
    ctx.progress(0, total);
    while done < total {
        ctx.check_cancel()?;
        let part = part_name(src, index);
        let mut output = File::create(&part)?;
        let mut left = (total - done).min(chunk);
        while left > 0 {
            ctx.check_cancel()?;
            let want = left.min(buf.len() as u64) as usize;
            input.read_exact(&mut buf[..want])?;
            output.write_all(&buf[..want])?;
            left -= want as u64;
            done += want as u64;
            ctx.progress(done, total);
        }
        output.flush()?;
        parts.push(part);
        index += 1;
    }
    Ok(parts)
}

/// Merge `name.0`, `name.1`, ... into `dest`.
pub fn merge(parts: &[PathBuf], dest: &Path, ctx: &JobContext) -> Result<u64> {
    let total: u64 = parts
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
        .sum();
    let mut output = File::create(dest)?;
    let mut done = 0u64;
    let mut buf = vec![0u8; 1024 * 1024];
    ctx.progress(0, total);
    for part in parts {
        let mut input = File::open(part)?;
        loop {
            ctx.check_cancel()?;
            let read = input.read(&mut buf)?;
            if read == 0 {
                break;
            }
            output.write_all(&buf[..read])?;
            done += read as u64;
            ctx.progress(done, total);
        }
    }
    output.flush()?;
    ctx.progress(done, total);
    Ok(done)
}

/// Find `base.0`, `base.1`, ... in order (stops at the first gap).
pub fn find_parts(base: &Path) -> Vec<PathBuf> {
    let mut parts = Vec::new();
    let mut index = 0;
    loop {
        let part = part_name(base, index);
        if !part.is_file() {
            break;
        }
        parts.push(part);
        index += 1;
        if index > 9999 {
            break;
        }
    }
    parts
}

/// Read the joined bytes of a split set as one file, for header inspection.
pub fn read_head(parts: &[PathBuf], max: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for part in parts {
        if out.len() >= max {
            break;
        }
        let mut file = File::open(part)?;
        let mut buf = vec![0u8; (max - out.len()).min(1024 * 1024)];
        let read = file.read(&mut buf)?;
        out.extend_from_slice(&buf[..read]);
    }
    Ok(out)
}

/// Rewrite the first bytes of a split set (used to patch a split file header).
pub fn patch_head(parts: &[PathBuf], offset: u64, bytes: &[u8]) -> Result<()> {
    let mut file = File::options().write(true).open(&parts[0])?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::JobManager;

    #[test]
    fn splits_and_rejoins() {
        let dir = std::env::temp_dir().join(format!("godsend-ps5-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("game.iso");
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        let mgr = JobManager::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let src2 = src.clone();
        mgr.spawn(
            "test",
            "split",
            |_| {},
            move |ctx| {
                let parts = split(&src2, 4096, ctx)?;
                assert_eq!(parts.len(), 3);
                let dest = src2.with_file_name("joined.iso");
                merge(&parts, &dest, ctx)?;
                assert_eq!(std::fs::read(&dest)?, std::fs::read(&src2)?);
                assert_eq!(find_parts(&src2).len(), 3);
                tx.send(()).unwrap();
                Ok(())
            },
        );
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("split/merge finished");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
