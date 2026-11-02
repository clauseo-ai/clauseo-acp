//! Randomness as an input: the caller seeds [`Rng`] once (desktop from [`os_seed`], a test from
//! a fixed number), and the send delays and the tally's hour come from it.

/// A small splitmix64 generator. Not for secrets: it only spreads send times.
#[derive(Debug, Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) const fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number from 0 to `n - 1` (0 when `n` is 0).
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next().checked_rem(n).unwrap_or(0)
    }
}

/// A seed from the OS's random source, for [`crate::Counts::open`].
pub fn os_seed() -> u64 {
    let (high, low) = uuid::Uuid::new_v4().as_u64_pair();
    high ^ low.rotate_left(32)
}
