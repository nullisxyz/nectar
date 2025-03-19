//! Reference implementation of a Binary Merkle Tree hasher.

use alloy_primitives::{B256, Keccak256};
use bytes::Bytes;
use digest::{FixedOutput, FixedOutputReset, OutputSizeUser, Reset, Update};
use generic_array::{GenericArray, typenum::U32};
use std::io::{self, Write};
use std::marker::PhantomData;

// Only include rayon on non-wasm platforms with the parallel feature enabled
#[cfg(not(target_arch = "wasm32"))]
use rayon;

use super::constants::*;
use crate::chunk::ChunkAddress;
use crate::error::Result;

// Precomputed power-of-2 values for bit shifting operations
const BMT_SEGMENT_SIZE_LOG2: usize = 5; // SEGMENT_SIZE = 32 = 2^5

/// Reference implementation of a BMT hasher that uses Keccak256
///
/// This implementation uses a fixed number of BMT branches (128) as defined by `BMT_BRANCHES`.
/// The Binary Merkle Tree is structured to efficiently hash data in parallel when supported.
#[derive(Debug, Clone)]
pub struct BMTHasher {
    span: u64,
    prefix: Option<Vec<u8>>,
    buffer: [u8; BMT_MAX_DATA_LENGTH],
    cursor: usize,
    _marker: PhantomData<Keccak256>,
}

impl Default for BMTHasher {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl BMTHasher {
    /// Create a new BMT hasher with `BMT_BRANCHES` (128) branches
    ///
    /// The hasher is optimized for data sized in multiples of SEGMENT_SIZE,
    /// with a maximum of BMT_BRANCHES * SEGMENT_SIZE bytes.
    #[inline]
    pub fn new() -> Self {
        Self {
            span: 0,
            prefix: None,
            buffer: [0u8; BMT_MAX_DATA_LENGTH], // Pre-initialized with zeros
            cursor: 0,
            _marker: PhantomData,
        }
    }

    /// Set the span of data to be hashed
    #[inline]
    pub fn set_span(&mut self, span: u64) {
        self.span = span;
    }

    /// Get the current span
    #[inline(always)]
    pub fn span(&self) -> u64 {
        self.span
    }

    /// Add a prefix to the hash calculation
    #[inline]
    pub fn prefix_with(&mut self, prefix: &[u8]) {
        self.prefix = Some(prefix.to_vec());
    }

    /// Get the current prefix
    #[inline(always)]
    pub fn prefix(&self) -> &[u8] {
        self.prefix.as_deref().unwrap_or(&[])
    }

    /// Get the current cursor position
    #[inline(always)]
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// Get the amount of data currently in the buffer
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.cursor
    }

    /// Check if the buffer is empty
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.cursor == 0
    }

    /// Update the hasher with more data (non-destructive)
    #[inline]
    pub fn update_data(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        // Calculate how much data we can actually copy
        let available_space = BMT_MAX_DATA_LENGTH - self.cursor;
        let bytes_to_copy = data.len().min(available_space);

        if bytes_to_copy > 0 {
            // Copy data at cursor position
            self.buffer[self.cursor..self.cursor + bytes_to_copy]
                .copy_from_slice(&data[..bytes_to_copy]);

            // Update cursor position
            self.cursor += bytes_to_copy;
        }
    }

    /// Compute the BMT hash and return the chunk address (non-destructive)
    #[inline]
    pub fn chunk_address(&self, data: &[u8]) -> Result<ChunkAddress> {
        // Optimize by avoiding cloning when possible
        let hash = if data.is_empty() {
            // Use existing data if no new data
            self.sum()
        } else {
            // Create a temporary hasher with same config
            let mut temp_hasher = BMTHasher::new();
            temp_hasher.span = self.span;
            temp_hasher.prefix = self.prefix.clone();

            // Add our existing data and the new data
            if self.cursor > 0 {
                temp_hasher.update_data(&self.buffer[..self.cursor]);
            }
            temp_hasher.update_data(data);

            temp_hasher.sum()
        };

        // Create address from hash
        ChunkAddress::from_slice(hash.as_slice()).map_err(|e| e.into())
    }

    /// Hash data using a binary merkle tree (internal implementation)
    #[inline(always)]
    fn hash_internal(&self) -> B256 {
        // Use parallel hashing only when supported by the platform and enabled
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.hash_helper_parallel(&self.buffer, BMT_MAX_DATA_LENGTH)
        }

        // Use sequential hashing for WASM or when parallel is disabled
        #[cfg(target_arch = "wasm32")]
        {
            self.hash_helper_sequential(&self.buffer, BMT_MAX_DATA_LENGTH)
        }
    }

    /// Sequential implementation for hash computation (always available)
    #[inline(always)]
    fn hash_helper_sequential(&self, data: &[u8], length: usize) -> B256 {
        if length == SEGMENT_PAIR_LENGTH {
            let mut hasher = Keccak256::new();
            hasher.update(&data[..length]);
            return B256::from_slice(hasher.finalize().as_slice());
        }

        let half = length / 2;

        // Split data and hash both halves sequentially
        let left = &data[..half];
        let right = &data[half..];

        let left_hash = self.hash_helper_sequential(left, half);
        let right_hash = self.hash_helper_sequential(right, half);

        // Create a new Keccak256 hasher for the parent node
        let mut hasher = Keccak256::new();

        // Update with both child hashes
        hasher.update(left_hash.as_slice());
        hasher.update(right_hash.as_slice());

        // Return the hash
        B256::from_slice(hasher.finalize().as_slice())
    }

    /// Parallel implementation for hash computation (native environments with parallel feature)
    #[cfg(not(target_arch = "wasm32"))]
    #[inline(always)]
    fn hash_helper_parallel(&self, data: &[u8], length: usize) -> B256 {
        if length == SEGMENT_PAIR_LENGTH {
            let mut hasher = Keccak256::new();
            hasher.update(&data[..length]);
            return B256::from_slice(hasher.finalize().as_slice());
        }

        let half = length / 2;

        // Split data and hash both halves in parallel
        let (left, right) = data.split_at(half);
        let (left_hash, right_hash) = rayon::join(
            || self.hash_helper_parallel(left, half),
            || self.hash_helper_parallel(right, half),
        );

        // Create a new Keccak256 hasher for the parent node
        let mut hasher = Keccak256::new();

        // Update with both child hashes
        hasher.update(left_hash.as_slice());
        hasher.update(right_hash.as_slice());

        // Return the hash
        B256::from_slice(hasher.finalize().as_slice())
    }

    /// Finalize with span and optional prefix
    #[inline(always)]
    fn finalize_with_prefix(&self, intermediate_hash: B256) -> B256 {
        let mut hasher = Keccak256::new();

        // Add prefix if present
        if let Some(prefix) = &self.prefix {
            hasher.update(prefix);
        }

        // Add span as little-endian bytes
        hasher.update(self.span.to_le_bytes());

        // Add the intermediate hash
        hasher.update(intermediate_hash.as_slice());

        // Finalize to get the result
        B256::from_slice(hasher.finalize().as_slice())
    }

    /// Compute the current hash value as B256 (non-destructive)
    /// This is similar to the Go sum() pattern
    #[inline]
    pub fn sum(&self) -> B256 {
        self.finalize_with_prefix(self.hash_internal())
    }

    /// Reset the hasher's internal state
    #[inline(always)]
    fn reset_internal(&mut self) {
        // Simply reset cursor - no need to clear the buffer as it will be overwritten
        self.cursor = 0;
        self.span = 0;
        // Don't reset prefix, as it's considered a configuration parameter
    }

    /// Get the current data as Bytes (immutable reference)
    #[inline]
    pub fn data(&self) -> Bytes {
        if self.cursor == 0 {
            return Bytes::new();
        }

        // Create Bytes from slice
        Bytes::copy_from_slice(&self.buffer[..self.cursor])
    }

    /// Get segments for the current level of data
    #[inline]
    pub fn get_level_segments(&self, data: &[u8]) -> Vec<B256> {
        // Use parallel processing only when available
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            (0..BMT_BRANCHES)
                .into_par_iter()
                .map(|i| self.compute_segment_hash(data, i))
                .collect()
        }

        // Sequential for WASM or when parallel is disabled
        #[cfg(target_arch = "wasm32")]
        {
            (0..BMT_BRANCHES)
                .map(|i| self.compute_segment_hash(data, i))
                .collect()
        }
    }

    /// Compute the hash for a single segment at given index
    #[inline(always)]
    fn compute_segment_hash(&self, data: &[u8], i: usize) -> B256 {
        let start = i << BMT_SEGMENT_SIZE_LOG2; // Equivalent to i * SEGMENT_SIZE
        let mut hasher = Keccak256::new();

        if start < data.len() {
            let end = (start + SEGMENT_SIZE).min(data.len());
            let segment_data = &data[start..end];

            // Update with segment data
            hasher.update(segment_data);

            // If segment is shorter than SEGMENT_SIZE, the remaining bytes are zeros
            if segment_data.len() < SEGMENT_SIZE {
                hasher.update(&[0u8; SEGMENT_SIZE][..(SEGMENT_SIZE - segment_data.len())]);
            }
        } else {
            // Empty segment (all zeros)
            hasher.update(&[0u8; SEGMENT_SIZE]);
        }

        B256::from_slice(hasher.finalize().as_slice())
    }
}

// Implement io::Write trait for BMTHasher
impl Write for BMTHasher {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Keep original behavior to ensure tests pass
        self.update_data(buf);
        Ok(buf.len())
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        // Nothing needs to be done for flush
        Ok(())
    }
}

// Implement the Digest trait methods to match the standard patterns
impl OutputSizeUser for BMTHasher {
    type OutputSize = U32; // 32-byte output size
}

impl Update for BMTHasher {
    #[inline]
    fn update(&mut self, data: &[u8]) {
        self.update_data(data);
    }
}

impl Reset for BMTHasher {
    #[inline]
    fn reset(&mut self) {
        self.reset_internal();
    }
}

impl FixedOutput for BMTHasher {
    #[inline]
    fn finalize_into(self, out: &mut GenericArray<u8, Self::OutputSize>) {
        // Just finalize without resetting
        let b256 = self.sum();
        out.copy_from_slice(b256.as_slice());
    }
}

impl FixedOutputReset for BMTHasher {
    #[inline]
    fn finalize_into_reset(&mut self, out: &mut GenericArray<u8, Self::OutputSize>) {
        // Compute the hash
        let b256 = self.sum();

        // Copy it to the output
        out.copy_from_slice(b256.as_slice());

        // Reset the hasher
        self.reset_internal();
    }
}

// Make BMTHasher a valid hash function
impl digest::HashMarker for BMTHasher {}

/// A factory that creates BMTHasher instances
#[derive(Debug, Default, Clone)]
pub struct BMTHasherFactory;

impl BMTHasherFactory {
    /// Create a new factory for BMTHasher instances
    #[inline]
    pub fn new() -> Self {
        Self
    }

    /// Create a new BMT hasher
    #[inline]
    pub fn create_hasher(&self) -> BMTHasher {
        BMTHasher::new()
    }
}
