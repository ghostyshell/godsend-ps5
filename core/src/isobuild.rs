//! ISO9660 writer: turn a folder into a mountable ISO.
//!
//! ISO creation normally shells out to `genisoimage -iso-level 3
//! -allow-lowercase -allow-multidot -joliet`. This module does the same job in
//! process, with no external tool: an ISO9660 tree with uppercase 8.3-safe
//! names, a Joliet supplementary descriptor that carries the real file names,
//! path tables, and multi-extent records for files larger than 4 GiB.

use crate::error::{Error, Result};
use crate::iso::{Extent, SECTOR};
use crate::transfer::JobContext;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const PVD_LBA: u64 = 16;
const SVD_LBA: u64 = 17;
const FIRST_FREE: u64 = 18;
/// Largest single extent: 2047 sectors, keeping the byte count under u32::MAX.
const MAX_EXTENT: u64 = 2047 * SECTOR;

struct Entry {
    name: String,
    src: PathBuf,
    is_dir: bool,
    size: u64,
    children: Vec<usize>,
}

fn build_arena(root: &Path) -> Result<Vec<Entry>> {
    fn walk(dir: &Path, name: String, arena: &mut Vec<Entry>) -> Result<usize> {
        let id = arena.len();
        arena.push(Entry {
            name,
            src: dir.to_path_buf(),
            is_dir: true,
            size: 0,
            children: Vec::new(),
        });
        let mut children: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
        children.sort_by_key(|e| e.file_name());
        let mut ids = Vec::new();
        for child in children {
            let child_name = child.file_name().to_string_lossy().to_string();
            if child_name.starts_with('.') || child_name.starts_with("._") {
                continue;
            }
            let path = child.path();
            if path.is_dir() {
                ids.push(walk(&path, child_name, arena)?);
            } else {
                let size = child.metadata().map(|m| m.len()).unwrap_or(0);
                let id = arena.len();
                arena.push(Entry {
                    name: child_name,
                    src: path,
                    is_dir: false,
                    size,
                    children: Vec::new(),
                });
                ids.push(id);
            }
        }
        arena[id].children = ids;
        Ok(id)
    }
    if !root.is_dir() {
        return Err(Error::Invalid(format!(
            "{} is not a folder",
            root.display()
        )));
    }
    let mut arena = Vec::new();
    walk(root, String::new(), &mut arena)?;
    Ok(arena)
}

fn bfs_dirs(arena: &[Entry]) -> Vec<usize> {
    let mut out = vec![0usize];
    let mut i = 0;
    while i < out.len() {
        let id = out[i];
        for &child in &arena[id].children {
            if arena[child].is_dir {
                out.push(child);
            }
        }
        i += 1;
    }
    out
}

fn iso_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    truncate_name(&out, 30)
}

fn joliet_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    let mut units: Vec<u16> = out.encode_utf16().collect();
    units.truncate(64);
    String::from_utf16_lossy(&units)
}

fn truncate_name(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    if let Some(dot) = name.rfind('.') {
        let ext = &name[dot..];
        if ext.len() < max {
            let keep = max - ext.len();
            let mut base: String = name[..dot].chars().take(keep).collect();
            base.push_str(ext);
            return base;
        }
    }
    name.chars().take(max).collect()
}

/// ISO9660 records must be even length; 33 header bytes + identifier.
fn record_len(id_len: usize) -> u32 {
    let len = 33 + id_len;
    if len % 2 == 1 {
        len as u32 + 1
    } else {
        len as u32
    }
}

fn name_bytes(name: &str, joliet: bool) -> Vec<u8> {
    if joliet {
        name.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    } else {
        name.as_bytes().to_vec()
    }
}

fn extents_for(size: u64) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut left = size;
    if left == 0 {
        return vec![(0, 0)];
    }
    while left > 0 {
        let take = left.min(MAX_EXTENT);
        out.push((0, take as u32));
        left -= take;
    }
    out
}

fn dir_sectors(arena: &[Entry], id: usize, joliet: bool) -> u64 {
    let mut bytes = 0u64;
    bytes += 2 * record_len(1) as u64; // "." and ".."
    for &child in &arena[id].children {
        let name = if joliet {
            joliet_name(&arena[child].name)
        } else {
            iso_name(&arena[child].name)
        };
        let len = record_len(name_bytes(&name, joliet).len()) as u64;
        if arena[child].is_dir {
            bytes += len;
        } else {
            bytes += len * extents_for(arena[child].size).len() as u64;
        }
    }
    (bytes + SECTOR - 1) / SECTOR
}

fn put_record(out: &mut Vec<u8>, id: &[u8], lba: u32, size: u32, is_dir: bool, multi: bool) {
    let start = out.len();
    let mut flags = 0u8;
    if is_dir {
        flags |= 0x02;
    }
    if multi {
        flags |= 0x80;
    }
    let len = record_len(id.len());
    out.push(len as u8);
    out.push(0); // extended attribute length
    out.extend_from_slice(&lba.to_le_bytes());
    out.extend_from_slice(&lba.to_be_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&date7());
    out.push(flags);
    out.push(0); // file unit size
    out.push(0); // interleave gap
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.push(id.len() as u8);
    out.extend_from_slice(id);
    if (out.len() - start) % 2 == 1 {
        out.push(0);
    }
}

fn date7() -> [u8; 7] {
    // Recording date: years since 1900, month, day, hour, minute, second, GMT.
    [126, 1, 1, 0, 0, 0, 0]
}

fn date17() -> [u8; 17] {
    let mut out = *b"2026010100000000\0";
    out[16] = 0;
    out
}

fn write_pvd(
    bytes: &mut [u8],
    is_joliet: bool,
    total_sectors: u32,
    path_table_size: u32,
    l_path_lba: u32,
    m_path_lba: u32,
    root_lba: u32,
    root_size: u32,
    volume_id: &str,
) {
    bytes.fill(0);
    bytes[0] = if is_joliet { 2 } else { 1 };
    bytes[1..6].copy_from_slice(b"CD001");
    bytes[6] = 1;
    put_str(&mut bytes[8..40], "PS5");
    if is_joliet {
        let units: Vec<u16> = volume_id.encode_utf16().take(16).collect();
        bytes[40..40 + units.len() * 2].copy_from_slice(
            &units
                .iter()
                .flat_map(|u| u.to_be_bytes())
                .collect::<Vec<u8>>(),
        );
        bytes[88..91].copy_from_slice(b"%/E");
    } else {
        put_str(&mut bytes[40..72], volume_id);
    }
    put_both32(&mut bytes[80..88], total_sectors);
    put_both16(&mut bytes[120..124], 1);
    put_both16(&mut bytes[124..128], 1);
    put_both16(&mut bytes[128..132], 2048);
    put_both32(&mut bytes[132..140], path_table_size);
    bytes[140..144].copy_from_slice(&l_path_lba.to_le_bytes());
    bytes[148..152].copy_from_slice(&m_path_lba.to_be_bytes());
    // Root directory record.
    let mut rec = Vec::new();
    put_record(&mut rec, &[0], root_lba, root_size, true, false);
    let n = rec.len().min(34);
    bytes[156..156 + n].copy_from_slice(&rec[..n]);
    put_str(&mut bytes[190..318], "GODsend PS5");
    put_str(&mut bytes[318..446], "");
    put_str(&mut bytes[446..574], "");
    put_str(&mut bytes[574..702], "GODsend PS5 ISO builder");
    put_str(&mut bytes[702..739], "");
    put_str(&mut bytes[739..776], "");
    put_str(&mut bytes[776..813], "");
    bytes[813..830].copy_from_slice(&date17());
    bytes[830..847].copy_from_slice(&date17());
    bytes[847..864].fill(b'0');
    bytes[864..881].fill(b'0');
    bytes[881] = 1;
}

fn put_str(dst: &mut [u8], text: &str) {
    dst.fill(b' ');
    let bytes = text.as_bytes();
    let n = bytes.len().min(dst.len());
    dst[..n].copy_from_slice(&bytes[..n]);
}

fn put_both16(dst: &mut [u8], value: u16) {
    dst[0..2].copy_from_slice(&value.to_le_bytes());
    dst[2..4].copy_from_slice(&value.to_be_bytes());
}

fn put_both32(dst: &mut [u8], value: u32) {
    dst[0..4].copy_from_slice(&value.to_le_bytes());
    dst[4..8].copy_from_slice(&value.to_be_bytes());
}

fn path_table(arena: &[Entry], dirs: &[usize], joliet: bool, little_endian: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, &id) in dirs.iter().enumerate() {
        let name = if id == 0 {
            String::new()
        } else if joliet {
            joliet_name(&arena[id].name)
        } else {
            iso_name(&arena[id].name)
        };
        let id_bytes = if id == 0 {
            vec![0u8]
        } else {
            name_bytes(&name, joliet)
        };
        let parent = if index == 0 {
            1u16
        } else {
            parent_number(arena, dirs, id)
        };
        out.push(id_bytes.len() as u8);
        out.push(0);
        let lba = 0u32; // filled by caller
        if little_endian {
            out.extend_from_slice(&lba.to_le_bytes());
            out.extend_from_slice(&parent.to_le_bytes());
        } else {
            out.extend_from_slice(&lba.to_be_bytes());
            out.extend_from_slice(&parent.to_be_bytes());
        }
        out.extend_from_slice(&id_bytes);
        if id_bytes.len() % 2 == 1 {
            out.push(0);
        }
    }
    out
}

fn parent_number(arena: &[Entry], dirs: &[usize], id: usize) -> u16 {
    for &candidate in dirs {
        if arena[candidate].children.contains(&id) {
            return (dirs.iter().position(|&d| d == candidate).unwrap_or(0) + 1) as u16;
        }
    }
    1
}

fn fill_path_table_lbas(table: &mut [u8], dirs: &[usize], lobas: &[u32], little_endian: bool) {
    let mut pos = 0usize;
    for (index, id) in dirs.iter().enumerate() {
        let id_len = table[pos] as usize;
        let lba = lobas[index];
        let bytes = if little_endian {
            lba.to_le_bytes()
        } else {
            lba.to_be_bytes()
        };
        table[pos + 2..pos + 6].copy_from_slice(&bytes);
        pos += 8 + id_len + (id_len % 2);
        let _ = id;
    }
}

/// Build `src` into `out`.
pub fn build_iso(src: &Path, out: &Path, volume_id: &str, ctx: &JobContext) -> Result<u64> {
    let arena = build_arena(src)?;
    let dirs = bfs_dirs(&arena);

    let iso_sizes: Vec<u64> = dirs
        .iter()
        .map(|&id| dir_sectors(&arena, id, false))
        .collect();
    let joliet_sizes: Vec<u64> = dirs
        .iter()
        .map(|&id| dir_sectors(&arena, id, true))
        .collect();
    let iso_path = path_table(&arena, &dirs, false, true);
    let iso_path_m = path_table(&arena, &dirs, false, false);
    let joliet_path = path_table(&arena, &dirs, true, true);
    let joliet_path_m = path_table(&arena, &dirs, true, false);
    let iso_path_sectors = (iso_path.len() as u64 + SECTOR - 1) / SECTOR;
    let joliet_path_sectors = (joliet_path.len() as u64 + SECTOR - 1) / SECTOR;

    let mut next = FIRST_FREE;
    let iso_l_path_lba = next as u32;
    next += iso_path_sectors;
    let iso_m_path_lba = next as u32;
    next += iso_path_sectors;
    let joliet_l_path_lba = next as u32;
    next += joliet_path_sectors;
    let joliet_m_path_lba = next as u32;
    next += joliet_path_sectors;

    let iso_dir_lba_start = next;
    let mut iso_dir_lbas = Vec::with_capacity(dirs.len());
    for &size in &iso_sizes {
        iso_dir_lbas.push(next as u32);
        next += size;
    }
    let joliet_dir_lba_start = next;
    let mut joliet_dir_lbas = Vec::with_capacity(dirs.len());
    for &size in &joliet_sizes {
        joliet_dir_lbas.push(next as u32);
        next += size;
    }
    let _ = (iso_dir_lba_start, joliet_dir_lba_start);

    // Assign file extents after every directory.
    let mut file_extents: Vec<Vec<Extent>> = vec![Vec::new(); arena.len()];
    for (id, entry) in arena.iter().enumerate() {
        if entry.is_dir {
            continue;
        }
        let mut extents = Vec::new();
        for (_, size) in extents_for(entry.size) {
            extents.push(Extent {
                lba: next as u32,
                size,
            });
            next += (size as u64 + SECTOR - 1) / SECTOR;
        }
        file_extents[id] = extents;
    }
    let total_sectors = next;
    if total_sectors > u32::MAX as u64 {
        return Err(Error::Invalid(
            "image would exceed the 4 TiB ISO9660 limit".into(),
        ));
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(out)?;
    file.set_len(total_sectors * SECTOR)?;

    // Volume descriptors.
    let mut pvd = vec![0u8; SECTOR as usize];
    write_pvd(
        &mut pvd,
        false,
        total_sectors as u32,
        iso_path.len() as u32,
        iso_l_path_lba,
        iso_m_path_lba,
        iso_dir_lbas[0],
        (iso_sizes[0] * SECTOR) as u32,
        volume_id,
    );
    write_at(&mut file, PVD_LBA * SECTOR, &pvd)?;
    let mut svd = vec![0u8; SECTOR as usize];
    write_pvd(
        &mut svd,
        true,
        total_sectors as u32,
        joliet_path.len() as u32,
        joliet_l_path_lba,
        joliet_m_path_lba,
        joliet_dir_lbas[0],
        (joliet_sizes[0] * SECTOR) as u32,
        volume_id,
    );
    write_at(&mut file, SVD_LBA * SECTOR, &svd)?;

    // Path tables.
    let mut iso_path_l = iso_path;
    let mut iso_path_pt = iso_path_m;
    let mut joliet_path_l = joliet_path;
    let mut joliet_path_pt = joliet_path_m;
    fill_path_table_lbas(&mut iso_path_l, &dirs, &iso_dir_lbas, true);
    fill_path_table_lbas(&mut iso_path_pt, &dirs, &iso_dir_lbas, false);
    fill_path_table_lbas(&mut joliet_path_l, &dirs, &joliet_dir_lbas, true);
    fill_path_table_lbas(&mut joliet_path_pt, &dirs, &joliet_dir_lbas, false);
    write_at(&mut file, iso_l_path_lba as u64 * SECTOR, &iso_path_l)?;
    write_at(&mut file, iso_m_path_lba as u64 * SECTOR, &iso_path_pt)?;
    write_at(&mut file, joliet_l_path_lba as u64 * SECTOR, &joliet_path_l)?;
    write_at(
        &mut file,
        joliet_m_path_lba as u64 * SECTOR,
        &joliet_path_pt,
    )?;

    // Directories, one tree per descriptor.
    for tree in [false, true] {
        let sizes = if tree { &joliet_sizes } else { &iso_sizes };
        let lbas = if tree {
            &joliet_dir_lbas
        } else {
            &iso_dir_lbas
        };
        for (index, &id) in dirs.iter().enumerate() {
            let mut bytes = Vec::new();
            // "." and ".."
            put_record(
                &mut bytes,
                &[0],
                lbas[index],
                (sizes[index] * SECTOR) as u32,
                true,
                false,
            );
            let parent = if index == 0 {
                lbas[0]
            } else {
                let parent_id = parent_dir(&arena, &dirs, id);
                let parent_index = dirs.iter().position(|&d| d == parent_id).unwrap_or(0);
                lbas[parent_index]
            };
            let parent_size = if index == 0 {
                sizes[0]
            } else {
                let parent_id = parent_dir(&arena, &dirs, id);
                let parent_index = dirs.iter().position(|&d| d == parent_id).unwrap_or(0);
                sizes[parent_index]
            };
            put_record(
                &mut bytes,
                &[1],
                parent,
                (parent_size * SECTOR) as u32,
                true,
                false,
            );

            let mut children = arena[id].children.clone();
            children.sort_by_key(|&c| {
                let name = if tree {
                    joliet_name(&arena[c].name)
                } else {
                    iso_name(&arena[c].name)
                };
                name
            });
            for child in children {
                let name = if tree {
                    joliet_name(&arena[child].name)
                } else {
                    iso_name(&arena[child].name)
                };
                let id_bytes = name_bytes(&name, tree);
                if arena[child].is_dir {
                    let ci = dirs.iter().position(|&d| d == child).unwrap_or(0);
                    let size = if tree {
                        joliet_sizes[ci]
                    } else {
                        iso_sizes[ci]
                    };
                    put_record(
                        &mut bytes,
                        &id_bytes,
                        lbas[ci],
                        (size * SECTOR) as u32,
                        true,
                        false,
                    );
                } else {
                    let extents = &file_extents[child];
                    for (n, extent) in extents.iter().enumerate() {
                        let multi = n + 1 < extents.len();
                        put_record(&mut bytes, &id_bytes, extent.lba, extent.size, false, multi);
                    }
                }
            }
            let expected = (sizes[index] * SECTOR) as usize;
            if bytes.len() > expected {
                return Err(Error::Invalid(format!(
                    "ISO directory overflow ({} > {} bytes)",
                    bytes.len(),
                    expected
                )));
            }
            bytes.resize(expected, 0);
            write_at(&mut file, lbas[index] as u64 * SECTOR, &bytes)?;
        }
    }

    // File data.
    let total_bytes: u64 = arena.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
    let mut done = 0u64;
    ctx.progress(0, total_bytes);
    let mut buf = vec![0u8; 1024 * 1024];
    for (id, entry) in arena.iter().enumerate() {
        if entry.is_dir {
            continue;
        }
        let mut src = File::open(&entry.src)?;
        for extent in &file_extents[id] {
            ctx.check_cancel()?;
            file.seek(SeekFrom::Start(extent.lba as u64 * SECTOR))?;
            let mut left = extent.size as u64;
            while left > 0 {
                ctx.check_cancel()?;
                let want = left.min(buf.len() as u64) as usize;
                src.read_exact(&mut buf[..want])?;
                file.write_all(&buf[..want])?;
                left -= want as u64;
                done += want as u64;
                ctx.progress(done, total_bytes);
            }
        }
    }
    file.flush()?;
    ctx.progress(done, total_bytes);
    Ok(total_sectors * SECTOR)
}

fn parent_dir(arena: &[Entry], dirs: &[usize], id: usize) -> usize {
    for &candidate in dirs {
        if arena[candidate].children.contains(&id) {
            return candidate;
        }
    }
    0
}

fn write_at(file: &mut File, offset: u64, bytes: &[u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::JobManager;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("godsend-ps3-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trips_a_game_folder() {
        let root = temp_dir("iso");
        std::fs::create_dir_all(root.join("PS3_GAME/USRDIR/movies")).unwrap();
        std::fs::write(root.join("PS3_DISC.SFB"), b"PS3_DISC.SFB").unwrap();
        std::fs::write(root.join("PS3_GAME/PARAM.SFO"), b"\0PSFfake").unwrap();
        std::fs::write(root.join("PS3_GAME/ICON0.PNG"), b"\x89PNG fake").unwrap();
        std::fs::write(root.join("PS3_GAME/USRDIR/EBOOT.BIN"), vec![7u8; 5000]).unwrap();
        std::fs::write(root.join("PS3_GAME/USRDIR/movies/intro.pam"), b"movie").unwrap();

        let iso_path = root.join("out.iso");
        let mgr = JobManager::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx2 = tx.clone();
        let id = mgr.spawn("convert", "iso", move |_| {}, {
            let root = root.clone();
            let iso_path = iso_path.clone();
            move |ctx| {
                build_iso(&root, &iso_path, "TEST", ctx)?;
                tx2.send(()).unwrap();
                Ok(())
            }
        });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("build completed");

        let mut iso = crate::iso::Iso::open(&iso_path).expect("built ISO is readable");
        assert!(iso.has("/PS3_GAME"));
        assert!(
            iso.has("/PS3_GAME/USRDIR/MOVIES/INTRO.PAM")
                || iso.has("/PS3_GAME/USRDIR/movies/intro.pam")
        );
        let boot = iso
            .read_file("PS3_GAME/USRDIR/EBOOT.BIN", 10_000)
            .expect("EBOOT readable");
        assert_eq!(boot.len(), 5000);
        assert!(boot.iter().all(|b| *b == 7));

        let out = root.join("extracted");
        let mgr2 = JobManager::new();
        mgr2.spawn("extract", "x", |_| {}, {
            let out = out.clone();
            move |ctx| {
                let mut iso = crate::iso::Iso::open(&iso_path)?;
                iso.extract(&out, ctx).map(|_| ())
            }
        });
        for _ in 0..100 {
            if out.join("PS3_GAME/USRDIR/EBOOT.BIN").exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let got = std::fs::read(out.join("PS3_GAME/USRDIR/EBOOT.BIN")).expect("extracted EBOOT");
        assert_eq!(got.len(), 5000);
        assert!(got.iter().all(|b| *b == 7));

        let _ = std::fs::remove_dir_all(&root);
        let _ = id;
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(iso_name("EBOOT.BIN"), "EBOOT.BIN");
        assert_eq!(iso_name("a b/c"), "A_B_C");
        assert!(joliet_name("Game (USA).iso").contains("(USA)"));
    }

    #[test]
    fn splits_extents_over_four_gigabytes() {
        let extents = extents_for(MAX_EXTENT * 2 + 5);
        assert_eq!(extents.len(), 3);
        assert_eq!(
            extents.iter().map(|(_, s)| *s as u64).sum::<u64>(),
            MAX_EXTENT * 2 + 5
        );
    }
}
