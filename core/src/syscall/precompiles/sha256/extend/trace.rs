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

                cols.w_i_minus_15
                    .populate(event.w_i_minus_15_reads[j], &mut new_byte_lookup_events);
                cols.w_i_minus_16
                    .populate(event.w_i_minus_16_reads[j], &mut new_byte_lookup_events);
                cols.w_i_minus_7
                    .populate(event.w_i_minus_7_reads[j], &mut new_byte_lookup_events);

                // `s0 := w[i-15] rightshift 1`.
                let w_i_minus_15 = event.w_i_minus_15_reads[j].value;
                let s0 = cols.s0.populate(output, w_i_minus_15, 1);

                // `s1 := w[i-2] rightshift 2`.
                let s1 = cols.s1.populate(output, w_i_minus_15, 2);

                // Compute `s2`.
                let w_i_minus_7 = event.w_i_minus_7_reads[j].value;
                let w_i_minus_16 = event.w_i_minus_16_reads[j].value;
                cols.s2.populate(output, w_i_minus_16, s0, w_i_minus_7, s1);

                cols.w_i
                    .populate(event.w_i_writes[j], &mut new_byte_lookup_events);

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
