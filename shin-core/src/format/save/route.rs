//! The route stream of a save slot: the results of every SGET, SELECT and QUIZ in execution order, so loading can
//! replay the script to the save point without asking the player again (`0x7100034230` read, `0x7100034470` write).
//!
//! Bits are most significant first: stream bit `i` is bit `7 - i % 8` of byte `i / 8`. Reads and writes both grow
//! the buffer to whole KiB, so a touched route is a multiple of 1024 bytes; reading past the data gives zero bits.

use super::RouteData;

/// A position in a [`RouteData`] stream
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RouteCursor {
    pub bit: u64,
}

impl RouteData {
    fn grow_for(&mut self, cursor: RouteCursor, bits: u32) {
        let needed = ((cursor.bit + bits as u64 + 7) >> 3) as usize;
        if self.0.len() <= needed {
            self.0.resize((needed + 0x3ff) & !0x3ff, 0);
        }
    }

    /// Reads `bits` (at most 32) as an unsigned value
    pub fn read_bits(&mut self, cursor: &mut RouteCursor, bits: u32) -> u32 {
        self.grow_for(*cursor, bits);
        let mut value = 0;
        for _ in 0..bits {
            let byte = self.0[(cursor.bit >> 3) as usize];
            value = (value << 1) | (byte >> (7 - (cursor.bit & 7))) as u32 & 1;
            cursor.bit += 1;
        }
        value
    }

    /// Writes the low `bits` of `value`, overwriting only those bits
    pub fn write_bits(&mut self, cursor: &mut RouteCursor, bits: u32, value: u32) {
        self.grow_for(*cursor, bits);
        for i in (0..bits).rev() {
            let byte = &mut self.0[(cursor.bit >> 3) as usize];
            let mask = 0x80u8 >> (cursor.bit & 7);
            if value >> i & 1 != 0 {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
            cursor.bit += 1;
        }
    }

    /// An SGET result (`0x71000348e0`): a 1 bit then the value for 0 or 1, else a 0 bit then 16 bits sign-extended
    pub fn read_sget(&mut self, cursor: &mut RouteCursor) -> i32 {
        if self.read_bits(cursor, 1) == 1 {
            self.read_bits(cursor, 1) as i32
        } else {
            self.read_bits(cursor, 16) as u16 as i16 as i32
        }
    }

    /// `0x7100034930`
    pub fn write_sget(&mut self, cursor: &mut RouteCursor, value: i32) {
        if (value as u32) < 2 {
            self.write_bits(cursor, 1, 1);
            self.write_bits(cursor, 1, value as u32);
        } else {
            self.write_bits(cursor, 1, 0);
            self.write_bits(cursor, 16, value as u32 & 0xffff);
        }
    }

    /// A SELECT's pick: 4 bits
    pub const SELECT_BITS: u32 = 4;
    /// A QUIZ's answer: 6 bits
    pub const QUIZ_BITS: u32 = 6;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_most_significant_bit_first() {
        let mut route = RouteData::default();
        let mut cursor = RouteCursor::default();
        route.write_sget(&mut cursor, 1);
        route.write_bits(&mut cursor, RouteData::SELECT_BITS, 0b1010);
        route.write_sget(&mut cursor, -3);
        route.write_bits(&mut cursor, RouteData::QUIZ_BITS, 33);
        // the real save's route starts 0xc0: one SGET that returned 1
        assert_eq!(route.0[0] & 0xc0, 0xc0);
        assert_eq!(route.0[0] >> 2 & 0xf, 0b1010);
        assert_eq!(route.0.len(), 1024);

        let mut cursor = RouteCursor::default();
        assert_eq!(route.read_sget(&mut cursor), 1);
        assert_eq!(route.read_bits(&mut cursor, RouteData::SELECT_BITS), 0b1010);
        assert_eq!(route.read_sget(&mut cursor), -3);
        assert_eq!(route.read_bits(&mut cursor, RouteData::QUIZ_BITS), 33);
        // past the data: zeros
        assert_eq!(route.read_bits(&mut cursor, 32), 0);
    }

    #[test]
    fn writes_keep_the_bits_around_them() {
        let mut route = RouteData(vec![0xff; 1024]);
        let mut cursor = RouteCursor { bit: 2 };
        route.write_bits(&mut cursor, 4, 0);
        assert_eq!(route.0[0], 0b1100_0011);
    }
}
