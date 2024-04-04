use std::mem::size_of;

use sp1_derive::AlignedBorrow;

use crate::memory::MemoryReadCols;
use crate::memory::MemoryWriteCols;
use crate::operations::Add4Operation;
use crate::operations::FixedRotateRightOperation;
use crate::operations::FixedShiftRightOperation;
use crate::operations::IsZeroOperation;
use crate::operations::XorOperation;

pub const NUM_SHA_EXTEND_COLS: usize = size_of::<ShaExtendCols<u8>>();

#[derive(AlignedBorrow, Default, Debug, Clone, Copy)]
#[repr(C)]
pub struct ShaExtendCols<T> {
    /// Inputs.
    pub shard: T,
    pub clk: T,
    pub w_ptr: T,

    /// Control flags.
    pub i: T,
    /// g^n where g is generator with order 16 and n is the row number.
    pub cycle_16: T,
    /// Checks whether current row is start of a 16-row cycle. Bool result is stored in `result`.
    pub cycle_16_start: IsZeroOperation<T>,
    /// Checks whether current row is end of a 16-row cycle. Bool result is stored in `result`.
    pub cycle_16_end: IsZeroOperation<T>,
    /// Flags for when in the first, second, or third 16-row cycle.
    pub cycle_48: [T; 3],
    /// Whether the current row is the first of a 48-row cycle.
    pub cycle_48_start: T,
    pub cycle_48_end: T,

    /// Inputs to `s0`, `s1`, `s2`.
    pub w_0: MemoryReadCols<T>,

    /// `s0 = w[i-15] >> 1`
    pub s0: FixedShiftRightOperation<T>,
    /// `s1 := w[i-15] >> 2`.
    pub s1: FixedShiftRightOperation<T>,
    /// `s := w[i-15] >> 3`.
    pub s: FixedShiftRightOperation<T>,

    /// `w[i] := w[i-15] ^ s0 ^ s1 ^ s`.
    pub s2_intermediate_1: XorOperation<T>,
    pub s2_intermediate_2: XorOperation<T>,
    pub s2: XorOperation<T>,

    /// Result.
    pub w_16: MemoryWriteCols<T>,

    /// Selector.
    pub is_real: T,
}
