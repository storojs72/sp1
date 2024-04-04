mod air;
mod columns;
mod execute;
mod flags;
mod trace;

pub use columns::*;

use crate::runtime::{MemoryReadRecord, MemoryWriteRecord};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaExtendEvent {
    pub shard: u32,
    pub clk: u32,
    pub w_ptr: u32,
    pub w_i_minus_15_reads: Vec<MemoryReadRecord>,
    pub w_i_minus_16_reads: Vec<MemoryReadRecord>,
    pub w_i_writes: Vec<MemoryWriteRecord>,
}

/// Implements the SHA extension operation which loops over i = [16, 63] and modifies w[i] in each
/// iteration. The only input to the syscall is the 4byte-aligned pointer to the w array.
///
/// In the AIR, each SHA extend syscall takes up 48 rows, where each row corresponds to a single
/// iteration of the loop.
#[derive(Default)]
pub struct ShaExtendChip;

impl ShaExtendChip {
    pub fn new() -> Self {
        Self {}
    }
}

pub fn sha_extend(w: &mut [u32]) {
    /*
    let a = 800;
    let a_right_shift_1 = a >> 1;
    let a_right_shift_2 = a >> 2;
    let a_right_shift_3 = a >> 3;

    let output = a ^ a_right_shift_1 ^ a_right_shift_2 ^ a_right_shift_3;
    */

    for i in 16..64 {
        let s0 = w[i - 15] >> 1;
        let s1 = w[i - 15] >> 2;
        let s = w[i - 15] >> 3;

        w[i] = w[i - 16] + s0 + s1 + s;
    }
}

#[cfg(test)]
pub mod extend_tests {

    use p3_baby_bear::BabyBear;

    use p3_matrix::dense::RowMajorMatrix;

    use crate::{
        air::MachineAir,
        alu::AluEvent,
        runtime::{ExecutionRecord, Instruction, Opcode, Program, SyscallCode},
        utils::{
            self, run_test,
            tests::{SHA2_ELF, SHA_EXTEND_ELF},
        },
    };
    use crate::syscall::precompiles::sha256::sha_extend;
    use crate::utils::run_test_custom;

    use super::ShaExtendChip;

    pub fn sha_extend_program_custom(w_ptr: u32, value: u32) -> Program {
        let mut instructions = vec![Instruction::new(Opcode::ADD, 29, 0, value, false, true)];
        for i in 0..64 {
            instructions.extend(vec![
                Instruction::new(Opcode::ADD, 30, 0, w_ptr + i * 4, false, true),
                Instruction::new(Opcode::SW, 29, 30, 0, false, true),
            ]);
        }
        instructions.extend(vec![
            Instruction::new(
                Opcode::ADD,
                5,
                0,
                SyscallCode::SHA_EXTEND as u32,
                false,
                true,
            ),
            Instruction::new(Opcode::ADD, 10, 0, w_ptr, false, true),
            Instruction::new(Opcode::ADD, 11, 0, 0, false, true),
            Instruction::new(Opcode::ECALL, 5, 10, 11, false, false),
        ]);
        Program::new(instructions, 0, 0)
    }

    #[test]
    fn test_sha_prove() {
        utils::setup_logger();
        let w_ptr = 100;
        let a = 800;


        let program = sha_extend_program_custom(w_ptr, a);
        let (_, memory) = run_test_custom(program);

        let mut w_computed = vec![];
        for i in 0..64 {
            w_computed.push(memory.get(&(w_ptr + i * 4)).unwrap().value.clone());
        }

        let mut w_expected = vec![a; 64];
        sha_extend(&mut w_expected);

        println!("expected: {:?}", w_expected);
        println!("computed: {:?}", w_computed);

        assert_eq!(w_computed, w_expected);
    }

    #[test]
    fn test_sha256_program() {
        utils::setup_logger();
        let program = Program::from(SHA2_ELF);
        run_test(program).unwrap();
    }

    #[test]
    fn test_sha_extend_program() {
        utils::setup_logger();
        let program = Program::from(SHA_EXTEND_ELF);
        run_test(program).unwrap();
    }

    pub fn sha_extend_program() -> Program {
        let w_ptr = 100;
        let mut instructions = vec![Instruction::new(Opcode::ADD, 29, 0, 5, false, true)];
        for i in 0..64 {
            instructions.extend(vec![
                Instruction::new(Opcode::ADD, 30, 0, w_ptr + i * 4, false, true),
                Instruction::new(Opcode::SW, 29, 30, 0, false, true),
            ]);
        }
        instructions.extend(vec![
            Instruction::new(
                Opcode::ADD,
                5,
                0,
                SyscallCode::SHA_EXTEND as u32,
                false,
                true,
            ),
            Instruction::new(Opcode::ADD, 10, 0, w_ptr, false, true),
            Instruction::new(Opcode::ADD, 11, 0, 0, false, true),
            Instruction::new(Opcode::ECALL, 5, 10, 11, false, false),
        ]);
        Program::new(instructions, 0, 0)
    }

    #[test]
    fn generate_trace() {
        let mut shard = ExecutionRecord::default();
        shard.add_events = vec![AluEvent::new(0, Opcode::ADD, 14, 8, 6)];
        let chip = ShaExtendChip::new();
        let trace: RowMajorMatrix<BabyBear> =
            chip.generate_trace(&shard, &mut ExecutionRecord::default());
        println!("{:?}", trace.values)
    }
}
