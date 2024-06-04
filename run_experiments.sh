#!/usr/bin/env bash

CALLS_INNER=1000
FRI_QUERIES_INNER=1

echo "CALLS: $CALLS_INNER"
echo "FRI_QUERIES: $FRI_QUERIES_INNER"

for reconstruct_commitment in "true" "false"
do
  for shard_chunking_multiplier in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20;
  do
    #2 4 8 16 32 64 128 256 512 1024 2048 4096 8192 16384 32768 65536 131072 262144 524288 1048576 2097152 4194304
    for shard_batch_size in 2 4 8 16 32 64 128 256 512 1024 2048 4096 8192 16384;
    do
      for shard_size in 16384 32768 65536 131072 262144 524288 1048576 2097152 4194304;
      do
        echo "SHARD_SIZE: ${shard_size}"
        echo "SHARD_BATCH_SIZE: ${shard_batch_size}"
        echo "RECONSTRUCT_COMMITMENT: ${reconstruct_commitment}"
        echo "SHARD_CHUNKING_MULTIPLIER: ${shard_chunking_multiplier}"
        for i in {0..5}; do
            CALLS=$CALLS_INNER \
            FRI_QUERIES=$FRI_QUERIES_INNER \
            SHARD_SIZE=$shard_size \
            RECONSTRUCT_COMMITMENT=$reconstruct_commitment \
            SHARD_BATCH_SIZE=$shard_batch_size \
            SHARD_CHUNKING_MULTIPLIER=$shard_chunking_multiplier \
            RUSTFLAGS='-C target-cpu=native' \
            cargo test test_multi_precompile_program_with_patched_stark_machine_sha_extend --release --package sp1-prover -- --nocapture
        done
      done
    done
  done
done

# To run this script you need to specify what Rust test to execute. Ideally it should print single line as a result, for example: [sha-extend] prove_core took: 1.226047583s, compress took: 4.652434084s
# > bash run_experiments.sh >> sha_extend.txt
#
# Then to clean up the results:
# > cat sha_extend.txt |  grep -v '^[running]' | grep -v '^[test result]' > sha_extend_clean.txt && sed '/^$/N;/^\n$/D' sha_extend_clean.txt > sha_extend_results.txt