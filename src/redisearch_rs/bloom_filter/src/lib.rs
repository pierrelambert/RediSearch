/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv3); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Bloom filter implementation for fast term rejection in RediSearch.
//!
//! A Bloom filter is a space-efficient probabilistic data structure that can test
//! whether an element is a member of a set. False positives are possible, but false
//! negatives are not. This makes it ideal for quickly rejecting queries for terms
//! that definitely don't exist in the index.
//!
//! # Memory Overhead
//!
//! The filter uses approximately 10 bits per unique term with a 1% false positive rate.
//!
//! # Example
//!
//! ```
//! use bloom_filter::BloomFilter;
//!
//! let mut filter = BloomFilter::new(1000, 0.01);
//! filter.insert(b"hello");
//! filter.insert(b"world");
//!
//! assert!(filter.contains(b"hello"));
//! assert!(filter.contains(b"world"));
//! assert!(!filter.contains(b"nonexistent")); // Probably false
//! ```

use xxhash_rust::xxh3::Xxh3;

/// A Bloom filter for fast membership testing.
///
/// Uses multiple hash functions to minimize false positive rate while maintaining
/// O(1) insertion and lookup time.
pub struct BloomFilter {
    /// Bit array for the filter
    bits: Vec<u64>,
    /// Number of hash functions to use
    num_hashes: u32,
    /// Number of bits in the filter
    num_bits: usize,
    /// Number of items inserted
    count: usize,
}

impl BloomFilter {
    /// Create a new Bloom filter with the specified capacity and false positive rate.
    ///
    /// # Arguments
    ///
    /// * `expected_items` - Expected number of unique items to be inserted
    /// * `false_positive_rate` - Desired false positive rate (e.g., 0.01 for 1%)
    ///
    /// # Panics
    ///
    /// Panics if `false_positive_rate` is not in the range (0.0, 1.0).
    pub fn new(expected_items: usize, false_positive_rate: f64) -> Self {
        assert!(
            false_positive_rate > 0.0 && false_positive_rate < 1.0,
            "False positive rate must be between 0 and 1"
        );

        // Calculate optimal number of bits: m = -n * ln(p) / (ln(2)^2)
        let num_bits = (-(expected_items as f64) * false_positive_rate.ln()
            / (std::f64::consts::LN_2.powi(2)))
        .ceil() as usize;

        // Calculate optimal number of hash functions: k = (m/n) * ln(2)
        let num_hashes = ((num_bits as f64 / expected_items as f64) * std::f64::consts::LN_2)
            .ceil()
            .max(1.0) as u32;

        let num_u64s = (num_bits + 63) / 64;

        Self {
            bits: vec![0u64; num_u64s],
            num_hashes,
            num_bits,
            count: 0,
        }
    }

    /// Insert an item into the Bloom filter.
    ///
    /// # Arguments
    ///
    /// * `item` - The item to insert (as a byte slice)
    pub fn insert(&mut self, item: &[u8]) {
        for i in 0..self.num_hashes {
            let bit_index = self.hash(item, i);
            let word_index = bit_index / 64;
            let bit_offset = bit_index % 64;
            self.bits[word_index] |= 1u64 << bit_offset;
        }
        self.count += 1;
    }

    /// Check if an item might be in the Bloom filter.
    ///
    /// Returns `true` if the item might be in the set (with possible false positives),
    /// or `false` if the item is definitely not in the set.
    ///
    /// # Arguments
    ///
    /// * `item` - The item to check (as a byte slice)
    pub fn contains(&self, item: &[u8]) -> bool {
        for i in 0..self.num_hashes {
            let bit_index = self.hash(item, i);
            let word_index = bit_index / 64;
            let bit_offset = bit_index % 64;
            if (self.bits[word_index] & (1u64 << bit_offset)) == 0 {
                return false;
            }
        }
        true
    }

    /// Get the number of items inserted into the filter.
    pub const fn count(&self) -> usize {
        self.count
    }

    /// Get the memory usage of the filter in bytes.
    pub const fn mem_usage(&self) -> usize {
        self.bits.len() * 8 + std::mem::size_of::<Self>()
    }

    /// Hash function using xxHash3 with seed variation for multiple hashes.
    fn hash(&self, item: &[u8], seed: u32) -> usize {
        let mut hasher = Xxh3::with_seed(u64::from(seed));
        hasher.update(item);
        let hash = hasher.digest();
        (hash as usize) % self.num_bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut filter = BloomFilter::new(100, 0.01);
        
        filter.insert(b"hello");
        filter.insert(b"world");
        
        assert!(filter.contains(b"hello"));
        assert!(filter.contains(b"world"));
        assert_eq!(filter.count(), 2);
    }
}

