//! BKTR: an update's RomFS section stored as a patch over the base game's RomFS section.
//!
//! The patch section's FS header has two bucket-tree tables (hactool `bktr.c`; LibHac
//! `IndirectStorage` / `AesCtrCounterExtendedStorage`), each described at 0x100 / 0x120 by
//! (offset u64, size u64, "BKTR", version u32, entry count u32, reserved) and stored inside the
//! section, encrypted with the section's normal counter:
//! - IndirectStorage: maps virtual offsets of the patched RomFS to a physical offset in the base
//!   RomFS section (storage 0) or in this section (storage 1). Entry: virtual u64, physical u64, storage u32.
//! - AesCtrEx: splits this section's physical bytes into ranges with their own counter generation.
//!   Entry: offset u64, encryption u8 (0 encrypted, 1 plain), 3 reserved, generation u32.
//!   The generation replaces the low half of the upper IV; the secure value (FS header 0x144) stays.
//!
//! Bucket tree layout: 0x4000-byte nodes. Node 0 lists the entry sets (header: index u32, set count u32,
//! end offset u64; then each set's start offset); then one node per entry set (header: index u32,
//! entry count u32, end offset u64; then the entries). Trees too big for one list node add a second
//! list level, which a patch of this size never needs.

use std::io;

use super::{invalid, le_u32, le_u64, nca::CtrSection, Nca, Storage};

const NODE: usize = 0x4000;

/// Flattens a bucket tree into its entries; also returns the end offset of the last range.
fn bucket_entries(table: &[u8], entry_size: usize, count: usize) -> io::Result<(Vec<&[u8]>, u64)> {
    let bad = || invalid("BKTR table is malformed");
    let list = table.get(..NODE).ok_or_else(bad)?;
    let sets = le_u32(list, 4) as usize;
    if sets > (NODE - 0x10) / 8 {
        return Err(invalid(
            "BKTR tables with two list levels are not supported",
        ));
    }
    let mut entries = Vec::with_capacity(count);
    for s in 0..sets {
        let node = table.get(NODE * (1 + s)..NODE * (2 + s)).ok_or_else(bad)?;
        let n = le_u32(node, 4) as usize;
        if n > (NODE - 0x10) / entry_size {
            return Err(bad());
        }
        entries.extend((0..n).map(|j| &node[0x10 + j * entry_size..][..entry_size]));
    }
    if entries.len() != count {
        return Err(bad());
    }
    Ok((entries, le_u64(list, 8)))
}

/// Splits a read at `offset` into the ranges of a sorted start list (last range ends at `end`),
/// calling `f(range index, offset, part)` for each part.
fn for_each_range(
    starts: &[u64],
    end: u64,
    mut offset: u64,
    mut buf: &mut [u8],
    mut f: impl FnMut(usize, u64, &mut [u8]) -> io::Result<()>,
) -> io::Result<()> {
    while !buf.is_empty() {
        let i = starts.partition_point(|&s| s <= offset);
        if i == 0 || offset >= end {
            return Err(invalid(format!(
                "offset {offset:#x} is outside the BKTR table"
            )));
        }
        let range_end = starts.get(i).copied().unwrap_or(end);
        let n = buf.len().min((range_end - offset) as usize);
        let (part, rest) = buf.split_at_mut(n);
        f(i - 1, offset, part)?;
        offset += n as u64;
        buf = rest;
    }
    Ok(())
}

struct AesCtrEx {
    section: CtrSection,
    starts: Vec<u64>,
    /// (encrypted, generation)
    ranges: Vec<(bool, u32)>,
    end: u64,
}

impl Storage for AesCtrEx {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let secure_value = u32::from_be_bytes(self.section.upper[..4].try_into().unwrap());
        let (section, ranges) = (&mut self.section, &self.ranges);
        for_each_range(&self.starts, self.end, offset, buf, |i, off, part| {
            let (encrypted, generation) = ranges[i];
            if encrypted {
                section.read_ctr(ctr_ex_upper(secure_value, generation), off, part)
            } else {
                section.raw.read_at(off, part)
            }
        })
    }
}

fn ctr_ex_upper(secure_value: u32, generation: u32) -> [u8; 8] {
    (((secure_value as u64) << 32) | generation as u64).to_be_bytes()
}

/// The patched RomFS section: reads go to the base section or to the patch data.
pub struct Indirect {
    base: Box<dyn Storage>,
    patch: Box<dyn Storage>,
    starts: Vec<u64>,
    /// (physical offset, storage index)
    targets: Vec<(u64, u32)>,
    end: u64,
}

impl Storage for Indirect {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let (starts, targets) = (&self.starts, &self.targets);
        let (base, patch) = (&mut self.base, &mut self.patch);
        for_each_range(starts, self.end, offset, buf, |i, off, part| {
            let (physical, storage) = targets[i];
            let at = physical + (off - starts[i]);
            match storage {
                0 => base.read_at(at, part),
                1 => patch.read_at(at, part),
                _ => Err(invalid("BKTR entry names an unknown storage")),
            }
        })
    }
}

/// Reads one bucket tree described at FS header offset `at`.
fn read_table(section: &mut CtrSection, fh: &[u8], at: usize) -> io::Result<(Vec<u8>, usize)> {
    if &fh[at + 0x10..at + 0x14] != b"BKTR" {
        return Err(invalid("The update's RomFS section is not a BKTR patch"));
    }
    let size = le_u64(fh, at + 8);
    if size > 64 << 20 {
        return Err(invalid("BKTR table is implausibly large"));
    }
    let mut table = vec![0u8; size as usize];
    section.read_at(le_u64(fh, at), &mut table)?;
    Ok((table, le_u32(fh, at + 0x18) as usize))
}

/// Opens section `i` of an update NCA as its full (patched) RomFS section, reading unchanged parts from `base`.
pub fn open_patch(
    nca: &Nca,
    i: usize,
    key: [u8; 16],
    base: Box<dyn Storage>,
) -> io::Result<Indirect> {
    let fh = nca.fs_header(i);
    let mut section = nca.section(i, key)?;

    let (table, count) = read_table(&mut section, fh, 0x100)?;
    let (entries, end) = bucket_entries(&table, 0x14, count)?;
    let starts = entries.iter().map(|e| le_u64(e, 0)).collect();
    let targets = entries
        .iter()
        .map(|e| (le_u64(e, 8), le_u32(e, 0x10)))
        .collect();

    let (table, count) = read_table(&mut section, fh, 0x120)?;
    let (entries, ctr_end) = bucket_entries(&table, 0x10, count)?;
    let ctr_starts = entries.iter().map(|e| le_u64(e, 0)).collect();
    let ranges = entries
        .iter()
        .map(|e| (e[8] == 0, le_u32(e, 0xC)))
        .collect();

    Ok(Indirect {
        base,
        patch: Box::new(AesCtrEx {
            section,
            starts: ctr_starts,
            ranges,
            end: ctr_end,
        }),
        starts,
        targets,
        end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-set tree of indirect entries (virtual, physical, storage).
    fn tree(sets: &[&[(u64, u64, u32)]], end: u64) -> Vec<u8> {
        let mut t = vec![0u8; NODE * (1 + sets.len())];
        t[4..8].copy_from_slice(&(sets.len() as u32).to_le_bytes());
        t[8..16].copy_from_slice(&end.to_le_bytes());
        for (s, set) in sets.iter().enumerate() {
            t[0x10 + s * 8..][..8].copy_from_slice(&set[0].0.to_le_bytes());
            let node = &mut t[NODE * (1 + s)..];
            node[..4].copy_from_slice(&(s as u32).to_le_bytes());
            node[4..8].copy_from_slice(&(set.len() as u32).to_le_bytes());
            for (j, (v, p, st)) in set.iter().enumerate() {
                let e = &mut node[0x10 + j * 0x14..];
                e[..8].copy_from_slice(&v.to_le_bytes());
                e[8..16].copy_from_slice(&p.to_le_bytes());
                e[16..20].copy_from_slice(&st.to_le_bytes());
            }
        }
        t
    }

    #[test]
    fn bucket_tree_flattens_sets() {
        let t = tree(&[&[(0, 100, 0), (10, 0, 1)], &[(30, 120, 0)]], 50);
        let (entries, end) = bucket_entries(&t, 0x14, 3).unwrap();
        assert_eq!(end, 50);
        assert_eq!(
            entries.iter().map(|e| le_u64(e, 0)).collect::<Vec<_>>(),
            [0, 10, 30]
        );
        assert_eq!(le_u32(entries[1], 0x10), 1);
        assert!(bucket_entries(&t, 0x14, 4).is_err());
        assert!(bucket_entries(&t[..NODE], 0x14, 3).is_err());
    }

    #[test]
    fn ranges_split_reads() {
        let mut seen = Vec::new();
        let mut buf = [0u8; 25];
        for_each_range(&[0, 10, 30], 50, 5, &mut buf, |i, off, part| {
            seen.push((i, off, part.len()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, [(0, 5, 5), (1, 10, 20)]);
        assert!(for_each_range(&[0, 10], 20, 15, &mut [0; 10], |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn indirect_reads_from_both_storages() {
        let base: Vec<u8> = (0..200).collect();
        let patch: Vec<u8> = (0..50).map(|x| 200 + x as u8).collect::<Vec<_>>();
        let mut s = Indirect {
            base: Box::new(base),
            patch: Box::new(patch),
            starts: vec![0, 10, 30],
            targets: vec![(100, 0), (0, 1), (120, 0)],
            end: 50,
        };
        let mut buf = [0u8; 4];
        s.read_at(8, &mut buf).unwrap();
        assert_eq!(buf, [108, 109, 200, 201]);
        s.read_at(28, &mut buf).unwrap();
        assert_eq!(buf, [218, 219, 120, 121]);
    }

    #[test]
    fn ctr_ex_counter() {
        assert_eq!(
            ctr_ex_upper(0xAABBCCDD, 7),
            [0xaa, 0xbb, 0xcc, 0xdd, 0, 0, 0, 7]
        );
    }
}
