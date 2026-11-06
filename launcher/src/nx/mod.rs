//! Native reader for Nintendo Switch dumps: NSP (PFS0) -> NCA -> ExeFS (PFS0) / RomFS, including
//! BKTR update patches (an update's RomFS stored as a patch over the base game's RomFS).
//!
//! Formats follow switchbrew.org (NCA, PFS0, RomFS, Ticket pages) and hactool by SciresM (ISC licence,
//! `nca.c`/`bktr.c`), plus LibHac's names for the newer BKTR fields. Everything is read through
//! [`Storage`] on demand, so multi-GB files are streamed, never loaded whole.

mod bktr;
mod keys;
mod nca;
mod romfs;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

pub use bktr::open_patch;
pub use keys::{hex, parse_ticket, parse_title_keys, Keys};
pub use nca::{Nca, FS_PFS0, FS_ROMFS};
pub use romfs::find_root_file;

/// Random-access byte source (a file region, a decrypted section, a patched RomFS, ...).
pub trait Storage {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

/// `len` bytes of a file starting at `start`.
pub struct FileRegion {
    file: File,
    start: u64,
    len: u64,
}

impl FileRegion {
    pub fn new(file: File, start: u64, len: u64) -> Self {
        Self { file, start, len }
    }

    pub fn whole(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self::new(file, 0, len))
    }

    pub fn sub(&self, start: u64, len: u64) -> io::Result<Self> {
        if start.checked_add(len).is_none_or(|end| end > self.len) {
            return Err(invalid("region out of bounds"));
        }
        Ok(Self::new(self.file.try_clone()?, self.start + start, len))
    }
}

impl Storage for FileRegion {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        if offset + buf.len() as u64 > self.len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read past the end of the file region",
            ));
        }
        self.file.seek(SeekFrom::Start(self.start + offset))?;
        self.file.read_exact(buf)
    }
}

pub(crate) fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

pub(crate) fn le_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

pub(crate) fn le_u64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// A file inside a PFS0, with its offset relative to the storage.
pub struct PfsFile {
    pub name: String,
    pub offset: u64,
    pub size: u64,
}

/// PFS0 (an NSP, or an ExeFS): "PFS0", file count, string table size, reserved; then 0x18-byte
/// entries (data offset, size, name offset in the string table, reserved), the string table, and the data.
pub fn pfs0_files(s: &mut dyn Storage, at: u64) -> io::Result<Vec<PfsFile>> {
    let mut head = [0u8; 0x10];
    s.read_at(at, &mut head)?;
    if &head[..4] != b"PFS0" {
        return Err(invalid("not a PFS0 container"));
    }
    let (count, strings) = (le_u32(&head, 4) as usize, le_u32(&head, 8) as usize);
    if count > 0x10000 || strings > 0x100000 {
        return Err(invalid("PFS0 header is implausible"));
    }
    let mut table = vec![0u8; count * 0x18 + strings];
    s.read_at(at + 0x10, &mut table)?;
    let data = at + 0x10 + table.len() as u64;
    let names = &table[count * 0x18..];
    (0..count)
        .map(|i| {
            let e = &table[i * 0x18..][..0x18];
            let name = names
                .get(le_u32(e, 0x10) as usize..)
                .ok_or_else(|| invalid("PFS0 name out of range"))?;
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            Ok(PfsFile {
                name: String::from_utf8_lossy(name).into_owned(),
                offset: data + le_u64(e, 0),
                size: le_u64(e, 8),
            })
        })
        .collect()
}

/// Streams `size` bytes at `offset` into `dest`, returning their SHA-256 (lowercase hex).
/// `progress` gets the bytes written so far, every 64 MiB and at the end.
pub fn copy_out(
    s: &mut dyn Storage,
    offset: u64,
    size: u64,
    dest: &Path,
    progress: &mut dyn FnMut(u64),
) -> io::Result<String> {
    let mut out = io::BufWriter::new(File::create(dest)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    while done < size {
        let n = buf.len().min((size - done) as usize);
        s.read_at(offset + done, &mut buf[..n])?;
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done += n as u64;
        if done % (64 << 20) < n as u64 || done == size {
            progress(done);
        }
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Storage for Vec<u8> {
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            buf.copy_from_slice(&self[offset as usize..][..buf.len()]);
            Ok(())
        }
    }

    #[test]
    fn pfs0_listing() {
        let mut p = b"PFS0".to_vec();
        p.extend(2u32.to_le_bytes());
        p.extend(8u32.to_le_bytes());
        p.extend(0u32.to_le_bytes());
        for (off, size, name) in [(0u64, 3u64, 0u32), (3, 2, 2)] {
            p.extend(off.to_le_bytes());
            p.extend(size.to_le_bytes());
            p.extend(name.to_le_bytes());
            p.extend(0u32.to_le_bytes());
        }
        p.extend(b"a\0bc\0\0\0\0");
        p.extend(b"xyzuv");
        let files = pfs0_files(&mut p, 0).unwrap();
        assert_eq!(files[1].name, "bc");
        assert_eq!((files[1].offset, files[1].size), (0x48 + 3, 2));
        let mut b = [0u8; 2];
        p.read_at(files[1].offset, &mut b).unwrap();
        assert_eq!(&b, b"uv");
    }
}
