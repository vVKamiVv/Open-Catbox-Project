use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

/// A fresh seed, as the original's `shin::randomSeed` (`0x71000fc2c0`) gives one from its engine-wide random source
/// (an MT19937 there) to every wobbler of a layer (`0x71000c7220`) and to the random effects. Here SplitMix64 over a
/// counter that starts at the clock: every call differs, which is all the seeds are for
pub fn random_seed() -> u32 {
    static START: OnceLock<u64> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let start = *START.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
    });
    let mut z = start.wrapping_add(NEXT.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) as u32
}

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
    fn seeds_differ_call_to_call() {
        let seeds: std::collections::HashSet<_> = (0..1000).map(|_| random_seed()).collect();
        assert_eq!(seeds.len(), 1000);
    }

    #[test]
    fn matches_std_minstd_rand() {
        // std::minstd_rand from seed 1 produces 48271, 182605794, 1291394886
        let mut rng = MinStdRand::new(1);
        let expected = [48271u32, 182605794, 1291394886].map(|x| (x - 1) as f32 / 2_147_483_648.0);
        assert_eq!([rng.next(), rng.next(), rng.next()], expected);
    }
}
