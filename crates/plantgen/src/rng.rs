//! Counter-based randomness and lineage identities.
//!
//! Nothing in plant generation keeps a mutable random-number generator. Every
//! random draw is a pure function of named inputs: the variant seed, the
//! lineage of the module that asks, the derivation step and a salt. Adding or
//! removing a branch therefore never shifts the random numbers seen by any other
//! branch, which is what keeps edited plants recognisable (the "index-free
//! identity" lesson of Lindenmayer's 2025 retrospective).

/// `SplitMix64` finaliser: a fast bijective 64-bit mixer with full avalanche.
#[must_use]
pub const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Order-sensitive combination of two words.
#[must_use]
pub const fn combine(a: u64, b: u64) -> u64 {
    mix64(a ^ mix64(b.wrapping_add(0x9e37_79b9_7f4a_7c15)))
}

/// Order-sensitive hash of a word sequence.
#[must_use]
pub fn hash_words(words: &[u64]) -> u64 {
    words
        .iter()
        .fold(0x6a09_e667_f3bc_c908, |state, word| combine(state, *word))
}

/// Stable 64-bit FNV-1a hash of text, used to salt draws by name.
#[must_use]
pub const fn hash_str(text: &str) -> u64 {
    let bytes = text.as_bytes();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

/// Uniform double in `[0, 1)` from the top 53 bits of a hash.
#[must_use]
pub fn unit(hash: u64) -> f64 {
    // Exact: 53 bits fit the mantissa.
    #[allow(clippy::cast_precision_loss)]
    let top = (hash >> 11) as f64;
    top * (1.0 / 9_007_199_254_740_992.0)
}

/// Approximately standard-normal draw from two hashes (Box-Muller with libm).
#[must_use]
pub fn normal(hash: u64) -> f64 {
    let u1 = unit(mix64(hash)).max(1e-300);
    let u2 = unit(mix64(hash ^ 0x5851_f42d_4c95_7f2d));
    crate::math::sqrt(-2.0 * crate::math::ln(u1)) * crate::math::cos(2.0 * crate::math::PI * u2)
}

/// Stable identity of one module lineage within one grown plant.
///
/// The axiom's modules are roots. A production that continues its predecessor
/// (the first successor module with the predecessor's symbol) keeps the
/// predecessor's lineage; every other successor module gets a child lineage
/// derived from the predecessor, the derivation step and its ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Lineage(pub u64);

impl Lineage {
    #[must_use]
    pub fn root(seed: u64, ordinal: u32) -> Self {
        Self(hash_words(&[0x726f_6f74, seed, u64::from(ordinal)]))
    }

    #[must_use]
    pub fn child(self, step: u32, ordinal: u32) -> Self {
        Self(hash_words(&[self.0, u64::from(step), u64::from(ordinal)]))
    }

    /// Identity of the `index`-th geometric element (segment or organ)
    /// produced by interpreting the module with this lineage.
    #[must_use]
    pub fn element(self, kind: u64, index: u32) -> u64 {
        hash_words(&[self.0, kind, u64::from(index)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_draws_stay_in_range_and_are_stable() {
        for value in [0, 1, u64::MAX, 0x1234_5678_9abc_def0] {
            let draw = unit(mix64(value));
            assert!((0.0..1.0).contains(&draw));
        }
        // Pinned so a change to the generator is a deliberate revision bump.
        assert_eq!(mix64(1), 0x5692_161d_100b_05e5);
        assert_eq!(hash_str("after"), 0xbf82_010f_6f71_eae9);
    }

    #[test]
    fn child_lineages_differ_by_step_and_ordinal() {
        let root = Lineage::root(7, 0);
        assert_ne!(root.child(1, 0), root.child(2, 0));
        assert_ne!(root.child(1, 0), root.child(1, 1));
        assert_eq!(root.child(3, 2), Lineage::root(7, 0).child(3, 2));
    }

    #[test]
    fn normal_draws_have_roughly_unit_variance() {
        let n = 20_000_u32;
        let (mut sum, mut sum_sq) = (0.0, 0.0);
        for i in 0..n {
            let x = normal(combine(99, u64::from(i)));
            sum += x;
            sum_sq += x * x;
        }
        let mean = sum / f64::from(n);
        let variance = sum_sq / f64::from(n) - mean * mean;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((variance - 1.0).abs() < 0.05, "variance {variance}");
    }
}
