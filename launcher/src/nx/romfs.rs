//! RomFS inside an NCA section (switchbrew "RomFS"; hactool `ivfc.h`).
//!
//! The FS header's hash data (at 0x8) is an IVFC hash tree: "IVFC", version, master hash size,
//! level count (7 including the master hash), then 0x18-byte level headers (offset u64, size u64,
//! log2 block size u32, reserved) at 0x18. The last level is the RomFS itself: a 0x50-byte header
//! (header size, then offset/size of the directory hash, directory meta, file hash and file meta
//! tables, then the file data offset). File meta entries: parent dir u32, sibling u32, data offset u64,
//! size u64, hash-bucket next u32, name length u32, name padded to 4 bytes. The root directory is at 0.

use std::io;

use super::{invalid, le_u32, le_u64, Storage};

/// Offset of the RomFS (the last IVFC level) inside its section.
fn romfs_offset(fs_header: &[u8]) -> io::Result<u64> {
    let levels = le_u32(fs_header, 0x14) as usize;
    if &fs_header[0x8..0xC] != b"IVFC" || !(2..=7).contains(&levels) {
        return Err(invalid("The RomFS section has no IVFC header"));
    }
    Ok(le_u64(fs_header, 0x18 + (levels - 2) * 0x18))
}

struct RomFile {
    parent: u32,
    name: String,
    offset: u64,
    size: u64,
}

fn parse_file_table(t: &[u8]) -> Vec<RomFile> {
    let mut files = Vec::new();
    let mut pos = 0;
    while pos + 0x20 <= t.len() {
        let e = &t[pos..];
        let len = le_u32(e, 0x1C) as usize;
        let Some(name) = e.get(0x20..0x20 + len) else {
            break;
        };
        files.push(RomFile {
            parent: le_u32(e, 0),
            name: String::from_utf8_lossy(name).into_owned(),
            offset: le_u64(e, 8),
            size: le_u64(e, 0x10),
        });
        pos += 0x20 + len.next_multiple_of(4);
    }
    files
}

/// Finds a file in the RomFS root of a section; returns its (offset in the section, size).
pub fn find_root_file(
    s: &mut dyn Storage,
    fs_header: &[u8],
    name: &str,
) -> io::Result<Option<(u64, u64)>> {
    let romfs = romfs_offset(fs_header)?;
    let mut h = [0u8; 0x50];
    s.read_at(romfs, &mut h)?;
    let size = le_u64(&h, 0x40);
    if le_u64(&h, 0) != 0x50 || size > 64 << 20 {
        return Err(invalid("The RomFS header is invalid (wrong keys?)"));
    }
    let mut table = vec![0u8; size as usize];
    s.read_at(romfs + le_u64(&h, 0x38), &mut table)?;
    Ok(parse_file_table(&table)
        .into_iter()
        .find(|f| f.parent == 0 && f.name == name)
        .map(|f| (romfs + le_u64(&h, 0x48) + f.offset, f.size)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(parent: u32, offset: u64, size: u64, name: &str) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend(parent.to_le_bytes());
        e.extend(u32::MAX.to_le_bytes());
        e.extend(offset.to_le_bytes());
        e.extend(size.to_le_bytes());
        e.extend(u32::MAX.to_le_bytes());
        e.extend((name.len() as u32).to_le_bytes());
        e.extend(name.as_bytes());
        e.resize(e.len().next_multiple_of(4), 0);
        e
    }

    #[test]
    fn file_table() {
        let mut t = entry(0x18, 0, 5, "patch.rom");
        t.extend(entry(0, 0x10, 7, "data.rom"));
        let files = parse_file_table(&t);
        assert_eq!(files.len(), 2);
        assert_eq!(
            (files[0].parent, files[0].name.as_str()),
            (0x18, "patch.rom")
        );
        assert_eq!((files[1].offset, files[1].size), (0x10, 7));
    }

    #[test]
    fn root_file_lookup() {
        // section: IVFC level 5 (of 7) puts the RomFS at 0x100
        let mut fh = vec![0u8; 0x200];
        fh[0x8..0xC].copy_from_slice(b"IVFC");
        fh[0x14] = 7;
        fh[0x90..0x98].copy_from_slice(&0x100u64.to_le_bytes());
        let table = [entry(1, 0, 1, "data.rom"), entry(0, 3, 4, "data.rom")].concat();
        let mut s = vec![0u8; 0x100];
        for v in [0x50u64, 0, 0, 0, 0, 0, 0, 0x50, table.len() as u64, 0x200] {
            s.extend(v.to_le_bytes());
        }
        s.extend(&table);
        assert_eq!(
            find_root_file(&mut s, &fh, "data.rom").unwrap(),
            Some((0x303, 4))
        );
        assert_eq!(find_root_file(&mut s, &fh, "patch.rom").unwrap(), None);
        fh[0x8] = 0;
        assert!(find_root_file(&mut s, &fh, "data.rom").is_err());
    }
}
