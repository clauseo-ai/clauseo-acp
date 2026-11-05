//! The schema's tests: closed on the wire (the committed JSON Schema, the walk over it, and the
//! property test over arbitrary days), the strict reads, and the agreement with the settings.

pub(crate) mod sample;

mod settings;
mod strict;
mod wire;

/// A splitmix64 step: a well-mixed number from any number.
pub(crate) fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
