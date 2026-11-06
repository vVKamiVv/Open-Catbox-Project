//! `prod.keys` / `title.keys` (`name = hex` lines, as written by Lockpick_RCM) and tickets.
//! Key material is never formatted into messages: errors name the missing key only.

use std::collections::HashMap;
use std::io;

pub struct Keys(HashMap<String, Vec<u8>>);

impl Keys {
    /// Lines that are not `name = <hex>` are ignored.
    pub fn parse(text: &str) -> Keys {
        Keys(
            text.lines()
                .filter_map(|l| l.split_once('='))
                .filter_map(|(k, v)| Some((k.trim().to_ascii_lowercase(), unhex(v.trim())?)))
                .collect(),
        )
    }

    pub fn get<const N: usize>(&self, name: &str) -> io::Result<[u8; N]> {
        self.0
            .get(name)
            .and_then(|v| v.as_slice().try_into().ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("prod.keys has no {name}. Dump prod.keys again from a console whose firmware is at least as new as the game."),
                )
            })
    }
}

/// `title.keys`: rights ID -> title key (still encrypted with the `titlekek`).
pub fn parse_title_keys(text: &str) -> HashMap<[u8; 16], [u8; 16]> {
    let key = |s: &str| unhex(s.trim())?.try_into().ok();
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter_map(|(r, k)| Some((key(r)?, key(k)?)))
        .collect()
}

pub struct Ticket {
    pub rights_id: [u8; 16],
    /// Personalised tickets wrap the title key with a console-unique RSA key; `title.keys` must provide it.
    pub personalised: bool,
    /// Common tickets: the title key, encrypted with the `titlekek`.
    pub title_key: [u8; 16],
}

/// Ticket (switchbrew "Ticket"): a signature whose type decides where the body starts; in the body,
/// the title key block is at 0x40, the title key type (0 common, 1 personalised) at 0x141 and the
/// rights ID at 0x160. With the usual RSA-2048 signature that is 0x180 / 0x281 / 0x2A0 in the file.
pub fn parse_ticket(b: &[u8]) -> Option<Ticket> {
    let body = match u32::from_le_bytes(b.get(0..4)?.try_into().ok()?) {
        0x10000 | 0x10003 => 0x240,
        0x10001 | 0x10004 => 0x140,
        0x10002 | 0x10005 => 0x80,
        _ => return None,
    };
    Some(Ticket {
        title_key: b.get(body + 0x40..body + 0x50)?.try_into().ok()?,
        personalised: *b.get(body + 0x141)? != 0,
        rights_id: b.get(body + 0x160..body + 0x170)?.try_into().ok()?,
    })
}

pub fn hex(b: &[u8]) -> String {
    use std::fmt::Write;
    b.iter().fold(String::new(), |mut s, x| {
        let _ = write!(s, "{x:02x}");
        s
    })
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || s.is_empty() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_file() {
        let k = Keys::parse("header_key = 00112233445566778899AABBCCDDEEFF00112233445566778899aabbccddeeff\nbad = xyz\nmaster_kek_source_12 = 0011223344556677889900112233445566\n");
        let h: [u8; 32] = k.get("header_key").unwrap();
        assert_eq!(h[9], 0x99);
        assert!(k.get::<16>("header_key").is_err());
        assert!(k.get::<16>("bad").is_err());
        assert!(k.get::<17>("master_kek_source_12").is_ok());
        let err = k.get::<16>("titlekek_0a").err().unwrap().to_string();
        assert!(err.contains("titlekek_0a"));
    }

    #[test]
    fn title_keys_file() {
        let t = parse_title_keys(
            "01006a300ba2c000000000000000000b = 000102030405060708090a0b0c0d0e0f\nshort = 00\n",
        );
        assert_eq!(t.len(), 1);
        let rights: [u8; 16] = unhex("01006a300ba2c000000000000000000b")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(t[&rights][15], 0x0f);
    }

    #[test]
    fn ticket_fields() {
        let mut t = vec![0u8; 0x2c0];
        t[0..4].copy_from_slice(&0x10004u32.to_le_bytes());
        t[0x180] = 0xab;
        t[0x281] = 1;
        t[0x2a0..0x2a8].copy_from_slice(&[0x01, 0x00, 0x6a, 0x30, 0x0b, 0xa2, 0xc0, 0x00]);
        let tik = parse_ticket(&t).unwrap();
        assert!(hex(&tik.rights_id).starts_with("01006a300ba2c000"));
        assert!(tik.personalised);
        assert_eq!(tik.title_key[0], 0xab);
        assert!(parse_ticket(&[0; 8]).is_none());
    }
}
