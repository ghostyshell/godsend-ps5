//! Minimal ISO9660 reader for disc images and folder archives.
//!
//! Game ISOs are ISO9660 images whose directory tree is plaintext even in
//! Redump "encrypted" dumps (only file data in the encrypted regions is
//! scrambled). The reader here therefore works for both retail dumps and
//! folder-style images, and understands the multi-extent records used for
//! files larger than 4 GiB.
//!
//! It prefers the Joliet tree when the image has one, because most tooling
//! (multiMAN, genisoimage with `-allow-lowercase`) stores the real file names
//! there.

use crate::error::{Error, Result};
use crate::transfer::JobContext;
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const SECTOR: u64 = 2048;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IsoNode {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub extents: Vec<Extent>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Extent {
    pub lba: u32,
    pub size: u32,
}

impl IsoNode {
    pub fn total_size(&self) -> u64 {
        self.extents.iter().map(|e| e.size as u64).sum()
    }
}

pub struct Iso {
    file: File,
    pub nodes: Vec<IsoNode>,
    pub joliet: bool,
    pub volume_id: String,
}

impl Iso {
    pub fn open(path: &Path) -> Result<Iso> {
        let mut file = File::open(path)?;
        let pvd = read_sector(&mut file, 16)?;
        if &pvd[1..6] != b"CD001" || pvd[0] != 1 {
            return Err(Error::Invalid(format!(
                "{} is not an ISO9660 image (no primary volume descriptor)",
                path.display()
            )));
        }

        // Sector 17 is the Joliet supplementary descriptor when present.
        let mut descriptor = Sector::from(&pvd);
        let mut joliet = false;
        if let Ok(svd) = read_sector(&mut file, 17) {
            if &svd[1..6] == b"CD001" && svd[0] == 2 && &svd[88..91] == b"%/E" {
                descriptor = Sector::from(&svd);
                joliet = true;
            }
        }

        let volume_id = if joliet {
            decode_joliet_str(&descriptor.bytes[40..72])
        } else {
            String::from_utf8_lossy(&descriptor.bytes[40..72])
                .trim()
                .to_string()
        };

        let mut nodes = Vec::new();
        let root = parse_record(&descriptor.bytes[156..156 + 34], joliet, "/")
            .ok_or_else(|| Error::Invalid("ISO root directory record is malformed".into()))?;
        if !root.is_dir {
            return Err(Error::Invalid("ISO root entry is not a directory".into()));
        }
        let mut pending = vec![(root.path.clone(), root.extents[0].lba, root.extents[0].size)];
        let mut seen = std::collections::HashSet::new();
        while let Some((dir_path, lba, size)) = pending.pop() {
            if nodes.len() > 200_000 {
                return Err(Error::Invalid("ISO has too many entries".into()));
            }
            if !seen.insert(lba) {
                continue;
            }
            let bytes = read_extents(&mut file, &[Extent { lba, size }], 64 * 1024 * 1024)?;
            let parent = dir_path.trim_end_matches('/');
            let mut pos = 0usize;
            let mut chain: Option<Parsed> = None;
            while pos < bytes.len() {
                let len = bytes[pos] as usize;
                if len == 0 {
                    // Records never cross a sector boundary; jump to the next.
                    let next = ((pos / SECTOR as usize) + 1) * SECTOR as usize;
                    if next <= pos {
                        break;
                    }
                    pos = next;
                    continue;
                }
                if pos + len > bytes.len() || len < 34 {
                    break;
                }
                let parsed = parse_record_inner(&bytes[pos..pos + len], joliet, parent);
                pos += len;
                let Some(Parsed {
                    node,
                    raw_name,
                    multi_extent,
                }) = parsed
                else {
                    continue;
                };
                if node.name == "." || node.name == ".." {
                    continue;
                }
                if let Some(mut prev) = chain.take() {
                    if prev.raw_name == raw_name && !prev.node.is_dir {
                        prev.node = merge_extents(prev.node, node);
                        if multi_extent {
                            chain = Some(prev);
                        } else {
                            push_node(prev.node, &mut nodes, &mut pending);
                        }
                        continue;
                    }
                    push_node(prev.node, &mut nodes, &mut pending);
                }
                if multi_extent {
                    chain = Some(Parsed {
                        node,
                        raw_name,
                        multi_extent,
                    });
                } else {
                    push_node(node, &mut nodes, &mut pending);
                }
            }
            if let Some(prev) = chain.take() {
                push_node(prev.node, &mut nodes, &mut pending);
            }
        }
        nodes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Iso {
            file,
            nodes,
            joliet,
            volume_id,
        })
    }

    pub fn find(&self, path: &str) -> Option<&IsoNode> {
        let want = format!("/{}", path.trim_matches('/'));
        self.nodes
            .iter()
            .find(|n| n.path.eq_ignore_ascii_case(&want) || n.path == want)
    }

    pub fn has(&self, path: &str) -> bool {
        self.find(path).is_some()
    }

    /// Read a small file (PARAM.SFO, ICON0.PNG, SYSTEM.CNF) into memory.
    pub fn read_file(&mut self, path: &str, max: usize) -> Result<Vec<u8>> {
        let node = self
            .find(path)
            .ok_or_else(|| Error::NotFound(path.to_string()))?
            .clone();
        let bytes = read_extents(&mut self.file, &node.extents, max)?;
        Ok(bytes.into_iter().take(max).collect())
    }

    /// Extract the whole image into `dest`, preserving the tree.
    pub fn extract(&mut self, dest: &Path, ctx: &JobContext) -> Result<u64> {
        let total: u64 = self
            .nodes
            .iter()
            .filter(|n| !n.is_dir)
            .map(|n| n.total_size())
            .sum();
        let mut done = 0u64;
        ctx.progress(0, total);
        for node in self.nodes.clone() {
            ctx.check_cancel()?;
            let rel = node.path.trim_start_matches('/');
            if rel.split('/').any(|part| part == "..") {
                continue;
            }
            let out = dest.join(rel);
            if !out.starts_with(dest) {
                continue;
            }
            if node.is_dir {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&out)?;
            for extent in &node.extents {
                ctx.check_cancel()?;
                copy_extent(&mut self.file, extent, &mut file, &mut done, total, ctx)?;
            }
            done = done.max(0);
            ctx.progress(done, total);
        }
        Ok(done)
    }

    /// Best-effort content type: `ps5`, `ps4`, `ps2`, `ps1`, `psp` or
    /// `unknown`. The PARAM.SFO title id is the only reliable signal.
    pub fn system(&mut self) -> String {
        if let Some(id) = self.title_id() {
            return crate::library::system_for_id(&id).to_string();
        }
        if self.has("/PSP_GAME") {
            return "psp".into();
        }
        if let Ok(cnf) = self.read_file("/SYSTEM.CNF", 4096) {
            let text = String::from_utf8_lossy(&cnf).to_uppercase();
            if text.contains("BOOT2") {
                return "ps2".into();
            }
            if text.contains("BOOT") {
                return "ps1".into();
            }
        }
        "unknown".into()
    }

    /// Title id from `sce_sys/PARAM.SFO` or `PARAM.SFO`.
    pub fn title_id(&mut self) -> Option<String> {
        for candidate in ["/sce_sys/PARAM.SFO", "/PARAM.SFO"] {
            if let Ok(bytes) = self.read_file(candidate, 512 * 1024) {
                if let Ok(sfo) = crate::sfo::Sfo::parse(&bytes) {
                    if let Some(id) = sfo.title_id() {
                        return Some(id.to_string());
                    }
                }
            }
        }
        None
    }

    pub fn title(&mut self) -> Option<String> {
        for candidate in ["/sce_sys/PARAM.SFO", "/PARAM.SFO"] {
            if let Ok(bytes) = self.read_file(candidate, 512 * 1024) {
                if let Ok(sfo) = crate::sfo::Sfo::parse(&bytes) {
                    if let Some(title) = sfo.title() {
                        return Some(title.to_string());
                    }
                }
            }
        }
        None
    }

    pub fn icon(&mut self, max: usize) -> Option<Vec<u8>> {
        for candidate in ["/sce_sys/icon0.png", "/ICON0.PNG", "/icon0.png"] {
            if let Ok(bytes) = self.read_file(candidate, max) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
        None
    }
}

fn read_extents(file: &mut File, extents: &[Extent], limit: usize) -> Result<Vec<u8>> {
    let total = extents.iter().map(|e| e.size as u64).sum::<u64>();
    let mut out = Vec::with_capacity(total.min(limit as u64) as usize);
    let mut buf = vec![0u8; 256 * 1024];
    for extent in extents {
        file.seek(SeekFrom::Start(extent.lba as u64 * SECTOR))?;
        let mut left = extent.size as u64;
        while left > 0 {
            if out.len() >= limit {
                return Ok(out);
            }
            let want = left
                .min(buf.len() as u64)
                .min((limit - out.len()) as u64) as usize;
            file.read_exact(&mut buf[..want])?;
            out.extend_from_slice(&buf[..want]);
            left -= want as u64;
        }
    }
    Ok(out)
}

fn copy_extent(
    src: &mut File,
    extent: &Extent,
    dst: &mut File,
    done: &mut u64,
    total: u64,
    ctx: &JobContext,
) -> Result<()> {
    src.seek(SeekFrom::Start(extent.lba as u64 * SECTOR))?;
    let mut left = extent.size as u64;
    let mut buf = vec![0u8; 1024 * 1024];
    while left > 0 {
        ctx.check_cancel()?;
        let want = left.min(buf.len() as u64) as usize;
        src.read_exact(&mut buf[..want])?;
        dst.write_all(&buf[..want])?;
        left -= want as u64;
        *done += want as u64;
        ctx.progress(*done, total);
    }
    Ok(())
}

fn read_sector(file: &mut File, sector: u64) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(sector * SECTOR))?;
    let mut buf = vec![0u8; SECTOR as usize];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

struct Sector {
    bytes: Vec<u8>,
}

impl From<&Vec<u8>> for Sector {
    fn from(v: &Vec<u8>) -> Self {
        Self { bytes: v.clone() }
    }
}

struct Parsed {
    node: IsoNode,
    raw_name: String,
    multi_extent: bool,
}

fn parse_record(record: &[u8], joliet: bool, parent: &str) -> Option<IsoNode> {
    parse_record_inner(record, joliet, parent).map(|p| p.node)
}

fn parse_record_inner(record: &[u8], joliet: bool, parent: &str) -> Option<Parsed> {
    if record.len() < 34 {
        return None;
    }
    let flags = record[25];
    let id_len = record[32] as usize;
    if 33 + id_len > record.len() {
        return None;
    }
    let raw = &record[33..33 + id_len];
    let (name, raw_name) = if id_len == 1 && raw[0] == 0 {
        (".".to_string(), ".".to_string())
    } else if id_len == 1 && raw[0] == 1 {
        ("..".to_string(), "..".to_string())
    } else if joliet {
        let name = decode_joliet(raw);
        (name.clone(), name)
    } else {
        let name = String::from_utf8_lossy(raw).to_string();
        let name = name
            .split(';')
            .next()
            .unwrap_or(&name)
            .trim_end_matches('.')
            .to_string();
        (name.clone(), name)
    };
    let lba = u32::from_le_bytes([record[2], record[3], record[4], record[5]]);
    let size = u32::from_le_bytes([record[10], record[11], record[12], record[13]]);
    let is_dir = flags & 0x02 != 0;
    let multi = flags & 0x80 != 0;
    let path = if name == "." || name == ".." {
        parent.to_string()
    } else if parent == "/" || parent.is_empty() {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    };
    let node = IsoNode {
        path,
        name,
        is_dir,
        size_bytes: size as u64,
        extents: vec![Extent { lba, size }],
    };
    Some(Parsed {
        node,
        raw_name,
        multi_extent: multi,
    })
}

fn merge_extents(mut prev: IsoNode, next: IsoNode) -> IsoNode {
    prev.extents.extend(next.extents);
    prev.size_bytes = prev.extents.iter().map(|e| e.size as u64).sum();
    prev
}

fn push_node(node: IsoNode, nodes: &mut Vec<IsoNode>, pending: &mut Vec<(String, u32, u32)>) {
    if node.is_dir && !node.extents.is_empty() {
        pending.push((node.path.clone(), node.extents[0].lba, node.extents[0].size));
    }
    nodes.push(node);
}

fn decode_joliet(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let mut text = String::from_utf16_lossy(&units);
    text = text.split(';').next().unwrap_or(&text).to_string();
    text.trim_end_matches('.').to_string()
}

fn decode_joliet_str(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units).trim().to_string()
}

/// Convert a filesystem tree into a list of `(relative path, is_dir)` pairs,
/// used by the ISO builder. Kept here so iso and isobuild share one walker.
pub fn walk_tree(root: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.starts_with("._") {
                continue;
            }
            let path = entry.path();
            let is_dir = path.is_dir();
            out.push((path.clone(), is_dir));
            if is_dir {
                stack.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Quick check used by store/install to classify a local image.
pub fn probe_system(path: &Path) -> String {
    match Iso::open(path) {
        Ok(mut iso) => iso.system(),
        Err(_) => "unknown".into(),
    }
}
