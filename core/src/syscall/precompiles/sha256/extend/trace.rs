use std::borrow::BorrowMut;

use p3_field::PrimeField32;
use p3_matrix::dense::RowMajorMatrix;

use crate::{air::MachineAir, runtime::ExecutionRecord};

use super::{ShaExtendChip, ShaExtendCols, NUM_SHA_EXTEND_COLS};

impl<F: PrimeField32> MachineAir<F> for ShaExtendChip {
    type Record = ExecutionRecord;

    fn name(&self) -> String {
        "ShaExtend".to_string()
    }

    fn generate_trace(
        &self,
        input: &ExecutionRecord,
        output: &mut ExecutionRecord,
    ) -> RowMajorMatrix<F> {
        let mut rows = Vec::new();

        let mut new_byte_lookup_events = Vec::new();
        for i in 0..input.sha_extend_events.len() {
            let event = input.sha_extend_events[i].clone();
            for j in 0..48usize {
                let mut row = [F::zero(); NUM_SHA_EXTEND_COLS];
                let cols: &mut ShaExtendCols<F> = row.as_mut_slice().borrow_mut();
                cols.is_real = F::one();
                cols.populate_flags(j);
                cols.shard = F::from_canonical_u32(event.shard);
                cols.clk = F::from_canonical_u32(event.clk);
                cols.w_ptr = F::from_canonical_u32(event.w_ptr);

                cols.w_0
                    .populate(event.w_0_reads[j], &mut new_byte_lookup_events);

                // `s0 := w[0] rightshift 1`.
                let w_0 = event.w_0_reads[j].value;
                let s0 = cols.s0.populate(output, w_0, 1);

                // `s1 := w[0] rightshift 2`.
                let s1 = cols.s1.populate(output, w_0, 2);

                // `s := w[i-15] rightshift 3`.
                let s = cols.s.populate(output, w_0, 3);

                // Compute `s2`.
                let s2_intermediate_1 = cols.s2_intermediate_1.populate(output, w_0, s0);
                let s2_intermediate_2 = cols.s2_intermediate_2.populate(output, s1, s);
                cols.s2.populate(output, s2_intermediate_1, s2_intermediate_2);

                cols.w_1
                    .populate(event.w_1_writes[j], &mut new_byte_lookup_events);

                rows.push(row);
            }
        }

        output.add_byte_lookup_events(new_byte_lookup_events);

        let nb_rows = rows.len();
        let mut padded_nb_rows = nb_rows.next_power_of_two();
        if padded_nb_rows == 2 || padded_nb_rows == 1 {
            padded_nb_rows = 4;
        }
        for i in nb_rows..padded_nb_rows {
            let mut row = [F::zero(); NUM_SHA_EXTEND_COLS];
            let cols: &mut ShaExtendCols<F> = row.as_mut_slice().borrow_mut();
            cols.populate_flags(i);
            rows.push(row);
        }

        // Convert the trace to a row major matrix.
        RowMajorMatrix::new(
            rows.into_iter().flatten().collect::<Vec<_>>(),
            NUM_SHA_EXTEND_COLS,
        )
    }

    fn included(&self, shard: &Self::Record) -> bool {
        !shard.sha_extend_events.is_empty()
    }
}
