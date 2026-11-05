//! [`Jitter`]: the seeded source of the update checks' delays. The caller seeds it (the actor
//! from the OS, a test from a constant), and the machine stays deterministic given the seed.

/// `SplitMix64` over a seed. Not for secrets: it only spreads the checks of many installs over
/// time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Jitter(u64);

impl Jitter {
    /// A source seeded with `seed`.
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next value.
    pub(crate) fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // The high half: SplitMix64's best bits.
        u32::try_from(z >> 32).unwrap_or(u32::MAX)
    }

    /// A value in `0..n`, for a positive `n`; 0 otherwise.
    pub(crate) fn below(&mut self, n: i64) -> i64 {
        if n <= 0 {
            return 0;
        }
        i64::from(self.next_u32()) % n
    }
}
