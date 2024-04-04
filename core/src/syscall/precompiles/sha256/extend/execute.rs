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
        let mut w_i_minus_15_reads = Vec::new();
        let mut w_i_writes = Vec::new();
        for i in 16..64 {
            // Read w[i-15].
            let (record, w_i_minus_15) = rt.mr(w_ptr + (i - 15) * 4);
            w_i_minus_15_reads.push(record);

            // Compute `s0`.
            let s0 = w_i_minus_15 >> 1;

            // Compute `s1`.
            let s1 = w_i_minus_15 >> 2;

            // Compute `s`.
            let s = w_i_minus_15 >> 3;

            // Compute `w_i`.
            let w_i = s1
                .bitxor(w_i_minus_15)
                .bitxor(s0)
                .bitxor(s);

            // Write w[i].
            w_i_writes.push(rt.mw(w_ptr + i * 4, w_i));
            rt.clk += 1;
        }

        // Push the SHA extend event.
        let shard = rt.current_shard();
        rt.record_mut().sha_extend_events.push(ShaExtendEvent {
            shard,
            clk: clk_init,
            w_ptr: w_ptr_init,
            w_i_minus_15_reads,
            w_i_writes,
        });

        None
    }
}
