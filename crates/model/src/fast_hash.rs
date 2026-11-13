//! A fast hasher for small keys (ids, addresses), for the maps and sets that hold tens of
//! thousands of them: the standard one (SipHash) resists flooding by chosen keys, a threat a
//! desktop modeller does not face, at several times the cost.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// A multiplicative hash (as rustc's `FxHasher`): each word folded in by a rotation, a xor
/// and a product; finished by a folded multiply (the 128-bit product's halves xored), so
/// that every bit of the key reaches the table's low bits.
#[derive(Clone, Copy, Default)]
pub struct FastHasher(u64);

const K: u64 = 0xf135_7aea_2e62_a9c5;
const K2: u64 = 0x9e37_79b9_7f4a_7c15;

impl FastHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().expect("8 bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut last = [0u8; 8];
            last[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(last) ^ (rest.len() as u64) << 56);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        let p = (self.0 as u128) * (K2 as u128);
        (p as u64) ^ (p >> 64) as u64
    }
}

pub type FastBuild = BuildHasherDefault<FastHasher>;
/// A `HashMap` with [`FastHasher`]: made by `FastMap::default()`.
pub type FastMap<K, V> = HashMap<K, V, FastBuild>;
/// A `HashSet` with [`FastHasher`]: made by `FastSet::default()`.
pub type FastSet<T> = HashSet<T, FastBuild>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Id;

    /// Ids in a run, ids differing only in their high bits, aligned addresses: spread over
    /// the table's buckets (the low bits of the hash).
    #[test]
    fn keys_alike_spread() {
        let hash = |k: u64| {
            let mut h = FastHasher::default();
            h.write_u64(k);
            h.finish()
        };
        for keys in [
            (0..4096u64).collect::<Vec<_>>(),
            (0..4096u64).map(|k| k << 40).collect(),
            (0..4096u64).map(|k| 0x7f00_0000_0000 + k * 64).collect(),
        ] {
            let buckets: FastSet<u64> = keys.iter().map(|k| hash(*k) & 4095).collect();
            // Random hashes fill about 63 %; keys left in a few buckets would fail far below.
            assert!(buckets.len() > 1800, "{} buckets of 4096", buckets.len());
        }
        let set: FastSet<Id> = (0..1000).map(Id).collect();
        assert!((0..1000).all(|k| set.contains(&Id(k))) && !set.contains(&Id(1000)));
    }
}
