//! A small non-cryptographic hasher for cache keys. SipHash's DoS resistance
//! is not needed for bounded, self-verifying caches (a collision costs one
//! replaced entry), and it dominated the cost of hashing short keys.
use std::hash::Hasher;

#[derive(Default)]
pub(crate) struct FastHasher(u64);

const K: u64 = 0x9e37_79b9_7f4a_7c15;

impl FastHasher {
    #[inline]
    fn mix(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.mix(u64::from_le_bytes(c.try_into().expect("8-byte chunk")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut tail = [0u8; 8];
            tail[..rest.len()].copy_from_slice(rest);
            // The length keeps "a" + "\0" distinct from "a".
            self.mix(u64::from_le_bytes(tail) ^ ((rest.len() as u64) << 56));
        }
    }
    #[inline]
    fn write_u8(&mut self, n: u8) {
        self.mix(u64::from(n));
    }
    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.mix(u64::from(n));
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.mix(n);
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.mix(n as u64);
    }
    /// Final avalanche so both hashbrown's low (bucket) and high (control)
    /// bits are well mixed.
    #[inline]
    fn finish(&self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^ (h >> 33)
    }
}

#[cfg(test)]
mod tests {
    use super::FastHasher;
    use std::hash::{Hash, Hasher};

    fn hash<T: Hash>(v: T) -> u64 {
        let mut h = FastHasher::default();
        v.hash(&mut h);
        h.finish()
    }

    #[test]
    fn deterministic_and_input_sensitive() {
        assert_eq!(hash("Shodo"), hash("Shodo"));
        assert_ne!(hash("Shodo"), hash("shodo"));
        assert_ne!(hash(("a", "bc")), hash(("ab", "c")));
        assert_ne!(hash(1u64), hash(2u64));
        assert_ne!(hash("a"), hash("a\0"));
        assert_ne!(hash(""), hash("\0"));
    }

    #[test]
    fn spreads_sequential_keys_across_low_and_high_bits() {
        let hashes: Vec<u64> = (0u64..4096).map(hash).collect();
        let low: std::collections::HashSet<_> = hashes.iter().map(|h| h & 0xfff).collect();
        let high: std::collections::HashSet<_> = hashes.iter().map(|h| h >> 57).collect();
        assert!(low.len() > 2500, "low bits cluster: {}", low.len());
        assert_eq!(high.len(), 128, "high 7 bits should all appear");
    }
}
