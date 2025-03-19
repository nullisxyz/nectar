//! WASM bindings for BMT functionality.
//!
//! This module provides JavaScript-friendly wrappers around BMT types.

use super::{BMTHasher, BMTProof, BmtProver};
use crate::chunk::ChunkAddress;
use alloy_primitives::B256;
use digest::Digest;
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;

/// WASM-friendly wrapper for the BMTHasher
#[wasm_bindgen(js_name = BMTHasher)]
pub struct WasmBMTHasher(BMTHasher);

#[wasm_bindgen(js_class = BMTHasher)]
impl WasmBMTHasher {
    /// Create a new BMT hasher
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(BMTHasher::new())
    }

    /// Set the span of data to be hashed
    #[wasm_bindgen]
    pub fn set_span(&mut self, span: u64) {
        self.0.set_span(span);
    }

    /// Add a prefix to the hash calculation
    #[wasm_bindgen(js_name = prefixWith)]
    pub fn prefix_with(&mut self, prefix: &Uint8Array) {
        self.0.prefix_with(&prefix.to_vec());
    }

    /// Update the hasher with more data
    #[wasm_bindgen]
    pub fn update(&mut self, data: &Uint8Array) {
        let data_vec = data.to_vec();
        self.0.update(&data_vec);
    }

    /// Get the current hash value without modifying the hasher
    #[wasm_bindgen(js_name = sum)]
    pub fn sum(&self) -> Uint8Array {
        let hash = self.0.sum();
        let result = Uint8Array::new_with_length(32);
        result.copy_from(hash.as_slice());
        result
    }

    /// Calculate the chunk address for the given data
    #[wasm_bindgen(js_name = chunkAddress)]
    pub fn chunk_address(
        &self,
        data: &Uint8Array,
    ) -> Result<crate::wasm::WasmChunkAddress, JsValue> {
        match self.0.chunk_address(&data.to_vec()) {
            Ok(addr) => Ok(crate::wasm::WasmChunkAddress(addr)),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
    }

    /// Generate a proof for a specific segment
    #[wasm_bindgen(js_name = generateProof)]
    pub fn generate_proof(
        &self,
        data: &Uint8Array,
        segment_index: usize,
    ) -> Result<WasmBMTProof, JsValue> {
        match self.0.generate_proof(&data.to_vec(), segment_index) {
            Ok(proof) => Ok(WasmBMTProof(proof)),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
    }

    /// Verify a proof against a root hash
    #[wasm_bindgen(js_name = verifyProof, static_method_of = BMTHasher)]
    pub fn verify_proof(proof: &WasmBMTProof, root_hash: &Uint8Array) -> Result<bool, JsValue> {
        match BMTHasher::verify_proof(&proof.0, &root_hash.to_vec()) {
            Ok(result) => Ok(result),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
    }
}

/// WASM-friendly wrapper for BMT proofs
#[wasm_bindgen(js_name = BMTProof)]
pub struct WasmBMTProof(pub(crate) BMTProof);

#[wasm_bindgen(js_class = BMTProof)]
impl WasmBMTProof {
    /// Get the segment index this proof is for
    #[wasm_bindgen(js_name = segmentIndex)]
    pub fn segment_index(&self) -> usize {
        self.0.segment_index
    }

    /// Get the segment being proven
    #[wasm_bindgen]
    pub fn segment(&self) -> Uint8Array {
        let result = Uint8Array::new_with_length(32);
        result.copy_from(self.0.segment.as_slice());
        result
    }

    /// Get the proof segments (sibling hashes)
    #[wasm_bindgen(js_name = proofSegments)]
    pub fn proof_segments(&self) -> Array {
        let result = Array::new();
        for segment in &self.0.proof_segments {
            let segment_array = Uint8Array::new_with_length(32);
            segment_array.copy_from(segment.as_slice());
            result.push(&segment_array);
        }
        result
    }

    /// Get the span of the data
    #[wasm_bindgen]
    pub fn span(&self) -> u64 {
        self.0.span
    }

    /// Verify this proof against a root hash
    #[wasm_bindgen]
    pub fn verify(&self, root_hash: &Uint8Array) -> Result<bool, JsValue> {
        match self.0.verify(&root_hash.to_vec()) {
            Ok(result) => Ok(result),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
    }
}
