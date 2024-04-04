use std::ops::BitXor;
use crate::{
    runtime::Syscall,
    syscall::precompiles::{sha256::ShaExtendEvent, SyscallContext},
};

use super::ShaExtendChip;

impl Syscall for ShaExtendChip {
    fn num_extra_cycles(&self) -> u32 {
        48
    }

    fn execute(&self, rt: &mut SyscallContext, arg1: u32, arg2: u32) -> Option<u32> {
        let clk_init = rt.clk;
        let w_ptr = arg1;
        if arg2 != 0 {
            panic!("arg2 must be 0")
        }

        let w_ptr_init = w_ptr;
        let mut w_0_reads = Vec::new();
        let mut w_1_writes = Vec::new();
        for _ in 16..64 {
            // Read w[0].
            let (record, w_0) = rt.mr(w_ptr);
            w_0_reads.push(record);

            // Compute `s0`.
            let s0 = w_0 >> 1;

            // Compute `s1`.
            let s1 = w_0 >> 2;

            // Compute `s`.
            let s = w_0 >> 3;

            // Compute `w_i`.
            let w_1 = s1
                .bitxor(w_0)
                .bitxor(s0)
                .bitxor(s);

            // Write w[16].
            w_1_writes.push(rt.mw(w_ptr + 1 * 4, w_1));
            rt.clk += 1;
        }

        // Push the SHA extend event.
        let shard = rt.current_shard();
        rt.record_mut().sha_extend_events.push(ShaExtendEvent {
            shard,
            clk: clk_init,
            w_ptr: w_ptr_init,
            w_0_reads,
            w_1_writes,
        });

        None
    }
}
