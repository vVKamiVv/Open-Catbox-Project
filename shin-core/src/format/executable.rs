//! The game's executable (`exefs/main`, an NSO), read for the few tables of text the engine keeps there rather than
//! in the romfs: the culprit game's testimonies and evidence, and the system dialogs.
//!
//! Translation patches change those strings with an IPS32 patch (applied by Atmosphère when the game loads), so the
//! patch is applied here too. Addresses are the ones the decompiled code uses: the image is based at
//! [`Executable::BASE`] with its relative relocations applied.

use anyhow::{Context, Result, bail, ensure};

pub struct Executable {
    image: Vec<u8>,
}

impl Executable {
    pub const BASE: u64 = 0x71_0000_0000;

    /// Loads the NSO `nso` and applies the IPS32 patches `patches` (which address the NSO's header plus image)
    pub fn load(nso: &[u8], patches: &[&[u8]]) -> Result<Self> {
        let mut image = load_nso(nso)?;
        relocate(&mut image)?;
        for patch in patches {
            apply_ips(&mut image, patch)?;
        }
        Ok(Self { image })
    }

    fn slice(&self, address: u64, len: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(Self::BASE)?).ok()?;
        self.image.get(offset..offset.checked_add(len)?)
    }

    pub fn u32(&self, address: u64) -> Option<u32> {
        Some(u32::from_le_bytes(self.slice(address, 4)?.try_into().ok()?))
    }

    pub fn u64(&self, address: u64) -> Option<u64> {
        Some(u64::from_le_bytes(self.slice(address, 8)?.try_into().ok()?))
    }

    /// The NUL-terminated string at `address`, without the NUL
    pub fn c_string(&self, address: u64) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(Self::BASE)?).ok()?;
        let rest = self.image.get(offset..)?;
        Some(&rest[..rest.iter().position(|&b| b == 0)?])
    }

    /// The string a pointer at `address` points to
    pub fn string_at_pointer(&self, address: u64) -> Option<&[u8]> {
        self.c_string(self.u64(address)?)
    }
}

/// The NSO's three segments (text, rodata, data) decompressed into one image, followed by its bss
fn load_nso(nso: &[u8]) -> Result<Vec<u8>> {
    ensure!(nso.get(..4) == Some(b"NSO0"), "not an NSO file");
    let u32_at = |offset: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            nso.get(offset..offset + 4).context("NSO header cut short")?.try_into()?,
        ))
    };
    let flags = u32_at(0xc)?;
    let bss = u32_at(0x3c)? as usize;
    let mut segments = Vec::new();
    for i in 0..3 {
        let file_offset = u32_at(0x10 + i * 0x10)? as usize;
        let memory_offset = u32_at(0x14 + i * 0x10)? as usize;
        let size = u32_at(0x18 + i * 0x10)? as usize;
        let file_size = u32_at(0x60 + i * 4)? as usize;
        let stored = nso
            .get(file_offset..file_offset + file_size)
            .context("NSO segment outside the file")?;
        let data = if flags & (1 << i) != 0 {
            lz4_block_decompress(stored, size)?
        } else {
            stored.to_vec()
        };
        segments.push((memory_offset, data));
    }
    let end = segments.iter().map(|(offset, data)| offset + data.len()).max().unwrap_or(0) + bss;
    let mut image = vec![0; end];
    for (offset, data) in segments {
        image[offset..offset + data.len()].copy_from_slice(&data);
    }
    Ok(image)
}

/// LZ4 block format (no frame): sequences of literals and back-references
fn lz4_block_decompress(input: &[u8], size: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut i = 0;
    let read_length = |i: &mut usize, mut length: usize| -> Result<usize> {
        if length == 15 {
            loop {
                let byte = *input.get(*i).context("LZ4 length cut short")?;
                *i += 1;
                length += byte as usize;
                if byte != 255 {
                    break;
                }
            }
        }
        Ok(length)
    };
    while i < input.len() {
        let token = input[i];
        i += 1;
        let literals = read_length(&mut i, (token >> 4) as usize)?;
        out.extend_from_slice(input.get(i..i + literals).context("LZ4 literals cut short")?);
        i += literals;
        if i >= input.len() {
            break;
        }
        let distance = u16::from_le_bytes([input[i], *input.get(i + 1).context("LZ4 offset cut short")?]);
        i += 2;
        let length = read_length(&mut i, (token & 15) as usize)? + 4;
        let start = out
            .len()
            .checked_sub(distance as usize)
            .filter(|_| distance != 0)
            .context("LZ4 offset outside the output")?;
        // copies may overlap their own output
        for k in 0..length {
            out.push(out[start + k]);
        }
    }
    ensure!(out.len() == size, "LZ4 segment decompressed to {} bytes, expected {size}", out.len());
    Ok(out)
}

/// Applies the `R_AARCH64_RELATIVE` relocations listed by the MOD0 header's dynamic section
fn relocate(image: &mut [u8]) -> Result<()> {
    let read_u32 = |image: &[u8], offset: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(image.get(offset..offset + 4).context("MOD0 outside the image")?.try_into()?))
    };
    let mod0 = read_u32(image, 4)? as usize;
    ensure!(image.get(mod0..mod0 + 4) == Some(b"MOD0"), "no MOD0 header");
    let mut dynamic = mod0.wrapping_add(read_u32(image, mod0 + 4)? as i32 as isize as usize);
    let (mut rela, mut rela_size) = (0usize, 0usize);
    loop {
        let entry = image.get(dynamic..dynamic + 16).context("dynamic section outside the image")?;
        let tag = i64::from_le_bytes(entry[..8].try_into()?);
        let value = u64::from_le_bytes(entry[8..].try_into()?) as usize;
        match tag {
            0 => break,
            7 => rela = value,
            8 => rela_size = value,
            _ => {}
        }
        dynamic += 16;
    }
    for entry in (rela..rela + rela_size).step_by(24) {
        let record = image.get(entry..entry + 24).context("relocation outside the image")?;
        let offset = u64::from_le_bytes(record[..8].try_into()?) as usize;
        let info = u64::from_le_bytes(record[8..16].try_into()?);
        let addend = i64::from_le_bytes(record[16..].try_into()?);
        const R_AARCH64_RELATIVE: u64 = 1027;
        if info & 0xffff_ffff == R_AARCH64_RELATIVE {
            let value = (Executable::BASE as i64 + addend) as u64;
            image
                .get_mut(offset..offset + 8)
                .context("relocation target outside the image")?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

/// An IPS32 patch: records of (u32 offset, u16 size, data), or a run (size 0, u16 count, byte); offsets count the
/// NSO's 0x100-byte header, as Atmosphère's patches do
fn apply_ips(image: &mut [u8], patch: &[u8]) -> Result<()> {
    const HEADER: usize = 0x100;
    ensure!(patch.starts_with(b"IPS32"), "not an IPS32 patch");
    let mut i = 5;
    loop {
        let tag = patch.get(i..i + 4).context("IPS patch cut short")?;
        if tag == b"EEOF" {
            return Ok(());
        }
        let offset = u32::from_be_bytes(tag.try_into()?) as usize;
        let size = u16::from_be_bytes(patch.get(i + 4..i + 6).context("IPS record cut short")?.try_into()?);
        i += 6;
        let data = if size == 0 {
            let count = u16::from_be_bytes(patch.get(i..i + 2).context("IPS run cut short")?.try_into()?);
            let byte = *patch.get(i + 2).context("IPS run cut short")?;
            i += 3;
            vec![byte; count as usize]
        } else {
            let data = patch.get(i..i + size as usize).context("IPS data cut short")?.to_vec();
            i += size as usize;
            data
        };
        let Some(target) = offset.checked_sub(HEADER) else {
            bail!("IPS record at {offset:#x} patches the NSO header");
        };
        image
            .get_mut(target..target + data.len())
            .context("IPS record outside the image")?
            .copy_from_slice(&data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_copies_literals_and_overlapping_matches() {
        // "abc" as literals, then a match 3 back of length 6 (4 + 2): "abcabcabc"
        let block = [0x32, b'a', b'b', b'c', 3, 0];
        assert_eq!(lz4_block_decompress(&block, 9).unwrap(), b"abcabcabc");
    }

    #[test]
    fn ips_records_and_runs_skip_the_header() {
        let mut image = vec![0u8; 8];
        let mut patch = b"IPS32".to_vec();
        patch.extend_from_slice(&[0, 0, 1, 2, 0, 2, b'h', b'i']); // 0x102: "hi"
        patch.extend_from_slice(&[0, 0, 1, 5, 0, 0, 0, 3, b'z']); // 0x105: "zzz"
        patch.extend_from_slice(b"EEOF");
        apply_ips(&mut image, &patch).unwrap();
        assert_eq!(&image, b"\0\0hi\0zzz");
    }

    /// Needs the game's executable: SHIN_TEST_EXEFS = a folder with `main` and the English patch's `.ips`
    #[test]
    #[ignore = "needs the game's executable (SHIN_TEST_EXEFS)"]
    fn reads_the_patched_testimonies() {
        let dir = std::path::PathBuf::from(std::env::var_os("SHIN_TEST_EXEFS").unwrap());
        let nso = std::fs::read(dir.join("main")).unwrap();
        let patches = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ips")))
            .map(|path| std::fs::read(path).unwrap())
            .collect::<Vec<_>>();
        let patches = patches.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let executable = Executable::load(&nso, &patches).unwrap();
        // the first testimony (Krauss), through the table at 0x71001db528
        let text = executable.string_at_pointer(0x71_001d_b528).unwrap();
        assert!(text.starts_with(b"@z110."), "{:?}", String::from_utf8_lossy(text));
        if !patches.is_empty() {
            assert!(String::from_utf8_lossy(text).contains("dining hall"));
        }
    }
}
