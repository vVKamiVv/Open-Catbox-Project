/// `std::minstd_rand` (multiplier 48271, modulus 2^31 - 1) mapped to [0, 1), the generator the original engine uses
/// for its random effects (glass shards, dissolve pieces, rain)
#[derive(Debug, Clone)]
pub struct MinStdRand(u32);

impl MinStdRand {
    /// Seeds like the original: reduced modulo 2^31 - 1, with 0 replaced by 1
    pub fn new(seed: u32) -> Self {
        Self(match seed % 0x7fff_ffff {
            0 => 1,
            seed => seed,
        })
    }

    pub fn next(&mut self) -> f32 {
        self.0 = ((self.0 as u64 * 48271) % 0x7fff_ffff) as u32;
        (self.0 - 1) as f32 / 2_147_483_648.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_std_minstd_rand() {
        // std::minstd_rand from seed 1 produces 48271, 182605794, 1291394886
        let mut rng = MinStdRand::new(1);
        let expected = [48271u32, 182605794, 1291394886].map(|x| (x - 1) as f32 / 2_147_483_648.0);
        assert_eq!([rng.next(), rng.next(), rng.next()], expected);
    }
}
