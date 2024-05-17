#![no_main]

sp1_zkvm::entrypoint!(main);

use sp1_zkvm::syscalls::{
    syscall_sha256_extend,
    syscall_secp256k1_double,
    syscall_bn254_double,
    syscall_bls12381_double,
};

//extern "C" {
//    fn syscall_sha256_extend(w: *mut u32);
//    fn syscall_secp256k1_double(p: *mut u32);
//    fn syscall_bn254_double(p: *mut u32);
//    fn syscall_bls12381_double(p: *mut u32);

//    fn syscall_sha256_compress(w: *mut u32, state: *mut u32);
//    fn syscall_ed_add(p: *mut u32, q: *mut u32);

//    fn syscall_secp256k1_add(p: *mut u32, q: *const u32);
//    fn syscall_bn254_add(p: *mut u32, q: *const u32);
//    fn syscall_bls12381_add(p: *mut u32, q: *const u32);
//    fn syscall_blake3_compress_inner(p: *mut u32, q: *const u32);

//    fn syscall_ed_decompress(point: &mut [u8; 64]);
//    fn syscall_secp256k1_decompress(point: &mut [u8; 64], is_odd: bool);
//    fn syscall_bls12381_decompress(point: &mut [u8; 96], is_odd: bool);
//    fn syscall_keccak_permute(state: *mut u64);
//}

pub fn main() {
    // Index of precompile to invoke
    let index = sp1_zkvm::io::read::<usize>();
    // How many times selected precompile should be invoked
    let calls = sp1_zkvm::io::read::<usize>();

    let f: extern "C" fn(w: *mut u32);
    let valid_input: *mut u32;

    match index {
        0 => {
            f = syscall_sha256_extend;
            valid_input = [1u32; 64].as_mut_ptr();
        },
        1 => {
            f = syscall_secp256k1_double;
            let mut a: [u8; 64] = [
                152, 23, 248, 22, 91, 129, 242, 89, 217, 40, 206, 45, 219, 252, 155, 2, 7, 11, 135,
                206, 149, 98, 160, 85, 172, 187, 220, 249, 126, 102, 190, 121, 184, 212, 16, 251, 143,
                208, 71, 156, 25, 84, 133, 166, 72, 180, 23, 253, 168, 8, 17, 14, 252, 251, 164, 93,
                101, 196, 163, 38, 119, 218, 58, 72,
            ];
            valid_input = a.as_mut_ptr() as *mut u32;
        },
        2 => {
            f = syscall_bn254_double;
            let mut a: [u8; 64] = [
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0,
            ];
            valid_input = a.as_mut_ptr() as *mut u32;
        },
        3 => {
            f = syscall_bls12381_double;
            let mut a: [u8; 96] = [
                187, 198, 34, 219, 10, 240, 58, 251, 239, 26, 122, 249, 63, 232, 85, 108, 88, 172, 27,
                23, 63, 58, 78, 161, 5, 185, 116, 151, 79, 140, 104, 195, 15, 172, 169, 79, 140, 99,
                149, 38, 148, 215, 151, 49, 167, 211, 241, 23, 225, 231, 197, 70, 41, 35, 170, 12, 228,
                138, 136, 162, 68, 199, 60, 208, 237, 179, 4, 44, 203, 24, 219, 0, 246, 10, 208, 213,
                149, 224, 245, 252, 228, 138, 29, 116, 237, 48, 158, 160, 241, 160, 170, 227, 129, 244,
                179, 8,
            ];
            valid_input = a.as_mut_ptr() as *mut u32;
        },
        _ => { unimplemented!() }
    }

    for _i in 0..calls {
        f(valid_input);
    }
}