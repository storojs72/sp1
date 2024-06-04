//! This is a test program that takes in a sp1_core vkey and a list of inputs, and then verifies the
//! SP1 proof for each input.

#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};
use sp1_zkvm::precompiles::verify::verify_sp1_proof;

fn words_to_bytes(words: &[u32; 8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for i in 0..8 {
        let word_bytes = words[i].to_le_bytes();
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&word_bytes);
    }
    bytes
}

pub fn main() {
    let v_keys = sp1_zkvm::io::read::<Vec<[u32; 8]>>();
    let inputs = sp1_zkvm::io::read::<Vec<Vec<u8>>>();
    assert_eq!(v_keys.len(), inputs.len());

    inputs.iter().zip(v_keys.iter()).for_each(|(input, v_key)| {
        // Get expected pv_digest hash: sha256(input)
        let pv_digest = Sha256::digest(input);
        verify_sp1_proof(&v_key, &pv_digest.into());

        println!("Verified proof for digest: {:?}", hex::encode(pv_digest));
        println!("Verified input: {:?}", hex::encode(input));
    });
}
