use p3_air::{Air, AirBuilder, BaseAir};

use super::{ShaExtendChip, ShaExtendCols, NUM_SHA_EXTEND_COLS};
use crate::air::{BaseAirBuilder, SP1AirBuilder};
use crate::memory::MemoryCols;
use crate::operations::{
    Add4Operation, FixedRotateRightOperation, FixedShiftRightOperation, XorOperation,
};
use crate::runtime::SyscallCode;
use core::borrow::Borrow;
use p3_field::AbstractField;
use p3_matrix::MatrixRowSlices;

impl<F> BaseAir<F> for ShaExtendChip {
    fn width(&self) -> usize {
        NUM_SHA_EXTEND_COLS
    }
}

impl<AB> Air<AB> for ShaExtendChip
where
    AB: SP1AirBuilder,
{
    fn eval(&self, builder: &mut AB) {
        // Initialize columns.
        let main = builder.main();
        let local: &ShaExtendCols<AB::Var> = main.row_slice(0).borrow();
        let next: &ShaExtendCols<AB::Var> = main.row_slice(1).borrow();
        let i_start = AB::F::from_canonical_u32(16);
        let nb_bytes_in_word = AB::F::from_canonical_u32(4);

        // Evaluate the control flags.
        self.eval_flags(builder);

        // Copy over the inputs until the result has been computed (every 48 rows).
        builder
            .when_transition()
            .when_not(local.cycle_16_end.result * local.cycle_48[2])
            .assert_eq(local.shard, next.shard);
        builder
            .when_transition()
            .when_not(local.cycle_16_end.result * local.cycle_48[2])
            .assert_eq(local.clk, next.clk);
        builder
            .when_transition()
            .when_not(local.cycle_16_end.result * local.cycle_48[2])
            .assert_eq(local.w_ptr, next.w_ptr);

        // Read w[i-15].
        builder.constraint_memory_access(
            local.shard,
            local.clk + (local.i - i_start),
            local.w_ptr + (local.i - AB::F::from_canonical_u32(15)) * nb_bytes_in_word,
            &local.w_i_minus_15,
            local.is_real,
        );

        // Compute `s0`.
        // s0 := w[i-15] rightshift 1
        FixedShiftRightOperation::<AB::F>::eval(
            builder,
            *local.w_i_minus_15.value(),
            1,
            local.s0,
            local.is_real,
        );

        // Compute `s1`.
        // s1 := w[i-15] rightshift 2
        FixedShiftRightOperation::<AB::F>::eval(
            builder,
            *local.w_i_minus_15.value(),
            2,
            local.s1,
            local.is_real,
        );

        // Compute `s`.
        // s := w[i-15] rightshift 3
        FixedShiftRightOperation::<AB::F>::eval(
            builder,
            *local.w_i_minus_15.value(),
            3,
            local.s,
            local.is_real,
        );

        // s2_intermediate_1 := w[i-15] ^ s0
        XorOperation::<AB::F>::eval(
            builder,
            *local.w_i_minus_15.value(),
            local.s0.value,
            local.s2_intermediate_1,
            local.is_real
        );

        // s2_intermediate_2 := s1 ^ s
        XorOperation::<AB::F>::eval(
            builder,
            local.s1.value,
            local.s.value,
            local.s2_intermediate_2,
            local.is_real
        );

        // s2 := s2_intermediate_1 ^ s2_intermediate_2.
        XorOperation::<AB::F>::eval(
            builder,
            local.s2_intermediate_1.value,
            local.s2_intermediate_2.value,
            local.s2,
            local.is_real
        );

        // Write `s2` to `w[i]`.
        builder.constraint_memory_access(
            local.shard,
            local.clk + (local.i - i_start),
            local.w_ptr + local.i * nb_bytes_in_word,
            &local.w_i,
            local.is_real,
        );

        // Receive syscall event in first row of 48-cycle.
        builder.receive_syscall(
            local.shard,
            local.clk,
            AB::F::from_canonical_u32(SyscallCode::SHA_EXTEND.syscall_id()),
            local.w_ptr,
            AB::Expr::zero(),
            local.cycle_48_start,
        );

        // If this row is real and not the last cycle, then next row should also be real.
        builder
            .when_transition()
            .when(local.is_real - local.cycle_48_end)
            .assert_one(next.is_real);

        // Assert that the table ends in nonreal columns. Since each extend ecall is 48 cycles and
        // the table is padded to a power of 2, the last row of the table should always be padding.
        builder.when_last_row().assert_zero(local.is_real);
    }
}
