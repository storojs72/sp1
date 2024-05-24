mod commit;
mod halt;
mod hint;
pub mod precompiles;
mod unconstrained;
mod verify;
mod write;

pub use commit::*;
pub use halt::*;
pub use hint::*;
pub use unconstrained::*;
pub use verify::*;
pub use write::*;

#[cfg(test)]
mod tests {
    use crate::io::SP1Stdin;
    use crate::utils::{run_test_io, run_test_io_custom, setup_logger};
    use crate::Program;

    #[test]
    fn test_multi_precompile_program_with_patched_stark_machine() {
        setup_logger();

        fn run_experiment(chips_to_deactivate: Vec<&str>, input: SP1Stdin) {
            let multi_precompile_program = include_bytes!(
                "../../../tests/multi-precompile-program/elf/riscv32im-succinct-zkvm-elf"
            );

            let program = Program::from(multi_precompile_program);

            // run test with all precompiles/chips enabled
            run_test_io(program.clone(), input.clone()).unwrap();
            // run test with some precompiles/chips deactivated
            run_test_io_custom(program, input, chips_to_deactivate).unwrap();
        }

        let chips_to_deactivate = vec![
            //"ShaExtend",
            "ShaCompress",
            "EdAddAssign",
            "EdDecompress",
            "Secp256k1Decompress",
            "Secp256k1AddAssign",
            "Secp256k1DoubleAssign",
            "KeccakPermute",
            "Bn254AddAssign",
            "Bn254DoubleAssign",
            "Bls12381AddAssign",
            "Bls12381DoubleAssign",
            "Uint256MulMod",
            "Bls12381Decompress",
        ];

        let calls = 100_000usize;
        let mut input = SP1Stdin::new();
        input.write(&0usize);
        input.write(&calls);
        run_experiment(chips_to_deactivate, input); // invoking sha_extend precompile

        let chips_to_deactivate = vec![
            "ShaExtend",
            "ShaCompress",
            "EdAddAssign",
            "EdDecompress",
            "Secp256k1Decompress",
            "Secp256k1AddAssign",
            //"Secp256k1DoubleAssign",
            "KeccakPermute",
            "Bn254AddAssign",
            "Bn254DoubleAssign",
            "Bls12381AddAssign",
            "Bls12381DoubleAssign",
            "Uint256MulMod",
            "Bls12381Decompress",
        ];

        let calls = 100_000usize;
        let mut input = SP1Stdin::new();
        input.write(&1usize);
        input.write(&calls);
        run_experiment(chips_to_deactivate, input); // invoking Secp256k1Double precompile

        let chips_to_deactivate = vec![
            "ShaExtend",
            "ShaCompress",
            "EdAddAssign",
            "EdDecompress",
            "Secp256k1Decompress",
            "Secp256k1AddAssign",
            "Secp256k1DoubleAssign",
            "KeccakPermute",
            "Bn254AddAssign",
            //"Bn254DoubleAssign",
            "Bls12381AddAssign",
            "Bls12381DoubleAssign",
            "Uint256MulMod",
            "Bls12381Decompress",
        ];

        let calls = 100_000usize;
        let mut input = SP1Stdin::new();
        input.write(&2usize);
        input.write(&calls);
        run_experiment(chips_to_deactivate, input); // invoking Bn254Double precompile

        let chips_to_deactivate = vec![
            "ShaExtend",
            "ShaCompress",
            "EdAddAssign",
            "EdDecompress",
            "Secp256k1Decompress",
            "Secp256k1AddAssign",
            "Secp256k1DoubleAssign",
            "KeccakPermute",
            "Bn254AddAssign",
            "Bn254DoubleAssign",
            "Bls12381AddAssign",
            //"Bls12381DoubleAssign",
            "Uint256MulMod",
            "Bls12381Decompress",
        ];

        let calls = 100_000usize;
        let mut input = SP1Stdin::new();
        input.write(&3usize);
        input.write(&calls);
        run_experiment(chips_to_deactivate, input); // invoking Bls12381Double precompile
    }
}
