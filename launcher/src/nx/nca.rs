//! NCA (switchbrew "NCA"): a 0xC00-byte header encrypted with AES-128-XTS (`header_key`, 0x200-byte
//! sectors numbered from 0, tweak = sector number as a *big-endian* 128-bit value, unlike standard
//! XTS), followed by up to four sections, each encrypted with AES-128-CTR.
//!
//! Decrypted header: 0x200 "NCA3", 0x205 content type (0 = program), 0x206/0x220 key generation (the
//! larger one, minus one if non-zero), 0x207 key area key index, 0x210 title ID, 0x230 rights ID,
//! 0x240 section table (start/end in 0x200-byte units), 0x280 SHA-256 of each FS header,
//! 0x300 key area (entry 2 is the CTR key), 0x400 four 0x200-byte FS headers.

use aes::cipher::{BlockDecrypt, KeyInit, KeyIvInit, StreamCipher};
use aes::Aes128;
use sha2::{Digest, Sha256};
use std::io;

use super::{invalid, le_u32, le_u64, FileRegion, Keys, Storage};

/// FS header byte 0x2.
pub const FS_ROMFS: u8 = 0;
pub const FS_PFS0: u8 = 1;
/// FS header byte 0x4: plain AES-CTR, and AES-CTR with per-range generations (BKTR patches).
const ENC_CTR: u8 = 3;
const ENC_CTR_EX: u8 = 4;

pub struct Nca {
    raw: FileRegion,
    h: Vec<u8>,
}

/// Decrypts an NCA3 header in place.
fn decrypt_header(h: &mut [u8], header_key: &[u8; 32]) {
    let xts = xts_mode::Xts128::new(
        Aes128::new(header_key[..16].into()),
        Aes128::new(header_key[16..].into()),
    );
    xts.decrypt_area(h, 0x200, 0, |sector: u128| sector.to_be_bytes());
}

/// CTR block for a byte `offset` inside the NCA: the section's 8-byte upper IV, then offset / 16 big-endian.
pub(crate) fn ctr_iv(upper: [u8; 8], offset: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[..8].copy_from_slice(&upper);
    iv[8..].copy_from_slice(&(offset >> 4).to_be_bytes());
    iv
}

impl Nca {
    pub fn open(mut raw: FileRegion, keys: &Keys) -> io::Result<Nca> {
        let mut h = vec![0u8; 0xC00];
        raw.read_at(0, &mut h)?;
        decrypt_header(&mut h, &keys.get("header_key")?);
        if &h[0x200..0x204] != b"NCA3" {
            return Err(invalid(
                "The NCA header could not be decrypted. Is header_key in prod.keys correct?",
            ));
        }
        Ok(Nca { raw, h })
    }

    pub fn content_type(&self) -> u8 {
        self.h[0x205]
    }

    pub fn title_id(&self) -> u64 {
        le_u64(&self.h, 0x210)
    }

    pub fn rights_id(&self) -> Option<[u8; 16]> {
        let r: [u8; 16] = self.h[0x230..0x240].try_into().unwrap();
        (r != [0; 16]).then_some(r)
    }

    pub fn fs_header(&self, i: usize) -> &[u8] {
        &self.h[0x400 + i * 0x200..][..0x200]
    }

    /// The first present section of an FS type ([`FS_ROMFS`], [`FS_PFS0`]).
    pub fn find_section(&self, fs_type: u8) -> Option<usize> {
        (0..4).find(|&i| le_u32(&self.h, 0x240 + i * 0x10) != 0 && self.fs_header(i)[2] == fs_type)
    }

    /// The section key: the title key (encrypted with `titlekek_XX`) when the NCA has a rights ID,
    /// otherwise key area entry 2 (encrypted with `key_area_key_<kind>_XX`).
    pub fn content_key(&self, keys: &Keys, title_key: Option<[u8; 16]>) -> io::Result<[u8; 16]> {
        let generation = self.h[0x206].max(self.h[0x220]).saturating_sub(1);
        let (kek, mut key) = match self.rights_id() {
            Some(_) => (
                format!("titlekek_{generation:02x}"),
                title_key.ok_or_else(|| invalid("no title key for this NCA"))?,
            ),
            None => {
                let kind = ["application", "ocean", "system"]
                    .get(self.h[0x207] as usize)
                    .ok_or_else(|| invalid("unknown key area key index"))?;
                (
                    format!("key_area_key_{kind}_{generation:02x}"),
                    self.h[0x320..0x330].try_into().unwrap(),
                )
            }
        };
        let kek: [u8; 16] = keys.get(&kek)?;
        Aes128::new(&kek.into()).decrypt_block((&mut key).into());
        Ok(key)
    }

    /// Section `i`, decrypted with its own counter (a BKTR section's tables are read this way too).
    pub fn section(&self, i: usize, key: [u8; 16]) -> io::Result<CtrSection> {
        let fh = self.fs_header(i);
        if Sha256::digest(fh).as_slice() != &self.h[0x280 + i * 0x20..][..0x20] {
            return Err(invalid(format!("NCA section {i} header is corrupt")));
        }
        if fh[4] != ENC_CTR && fh[4] != ENC_CTR_EX {
            return Err(invalid(format!(
                "NCA section {i} uses unsupported encryption type {}",
                fh[4]
            )));
        }
        let start = le_u32(&self.h, 0x240 + i * 0x10) as u64 * 0x200;
        let end = le_u32(&self.h, 0x244 + i * 0x10) as u64 * 0x200;
        Ok(CtrSection {
            raw: self.raw.sub(start, end.saturating_sub(start))?,
            nca_offset: start,
            key,
            // generation (0x140) and secure value (0x144) as one little-endian u64, used big-endian
            upper: le_u64(fh, 0x140).to_be_bytes(),
        })
    }
}

/// An AES-CTR encrypted NCA section, readable at any offset.
pub struct CtrSection {
    pub(crate) raw: FileRegion,
    nca_offset: u64,
    key: [u8; 16],
    pub(crate) upper: [u8; 8],
}

impl CtrSection {
    /// Reads with a given upper IV (BKTR ranges replace the generation half of it).
    pub(crate) fn read_ctr(
        &mut self,
        upper: [u8; 8],
        offset: u64,
        buf: &mut [u8],
    ) -> io::Result<()> {
        let skip = (offset % 16) as usize;
        let start = offset - skip as u64;
        let mut block = vec![0u8; (skip + buf.len()).next_multiple_of(16)];
        self.raw.read_at(start, &mut block)?;
        let iv = ctr_iv(upper, self.nca_offset + start);
        ctr::Ctr64BE::<Aes128>::new(&self.key.into(), &iv.into()).apply_keystream(&mut block);
        buf.copy_from_slice(&block[skip..][..buf.len()]);
        Ok(())
    }
}

impl Storage for CtrSection {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.read_ctr(self.upper, offset, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_xts_uses_big_endian_sector_tweaks() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let mut plain = vec![0u8; 0xC00];
        plain[0x200..0x204].copy_from_slice(b"NCA3");
        plain[0xBFF] = 0x5A;
        let mut h = plain.clone();
        let xts =
            xts_mode::Xts128::new(Aes128::new(key[..16].into()), Aes128::new(key[16..].into()));
        // encrypt sector by sector with explicit tweaks: sector n -> n as a big-endian u128
        for (n, sector) in h.chunks_mut(0x200).enumerate() {
            xts.encrypt_sector(sector, (n as u128).to_be_bytes());
        }
        decrypt_header(&mut h, &key);
        assert_eq!(h, plain);
    }

    #[test]
    fn ctr_counter() {
        let upper = le_u64(&[1, 0, 0, 0, 0xdd, 0xcc, 0xbb, 0xaa], 0).to_be_bytes();
        let iv = ctr_iv(upper, 0x0C00_1234_5670);
        assert_eq!(iv[..8], [0xaa, 0xbb, 0xcc, 0xdd, 0, 0, 0, 1]);
        assert_eq!(iv[8..], [0, 0, 0, 0xC0, 0x01, 0x23, 0x45, 0x67]);
        assert_eq!(ctr_iv(upper, 0xF)[8..], [0; 8]);
    }
}
