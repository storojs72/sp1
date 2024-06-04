//! An end-to-end-prover implementation for the SP1 RISC-V zkVM.
//!
//! Seperates the proof generation process into multiple stages:
//!
//! 1. Generate shard proofs which split up and prove the valid execution of a RISC-V program.
//! 2. Compress shard proofs into a single shard proof.
//! 3. Wrap the shard proof into a SNARK-friendly field.
//! 4. Wrap the last shard proof, proven over the SNARK-friendly field, into a PLONK proof.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::new_without_default)]

pub mod build;
pub mod install;
pub mod types;
pub mod utils;
pub mod verify;

use std::borrow::Borrow;
use std::path::Path;

use p3_baby_bear::BabyBear;
use p3_challenger::CanObserve;
use p3_field::{AbstractField, PrimeField};
use rayon::prelude::*;
use sp1_core::air::{PublicValues, Word};
use sp1_core::runtime::{ExecutionError, ExecutionReport, Runtime};
use sp1_core::stark::{Challenger, MachineVerificationError};
use sp1_core::utils::{SP1CoreOpts, DIGEST_SIZE};
use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};
use serde::de::DeserializeOwned;
use serde::Serialize;
use size::Size;
use sp1_core::air::{MachineAir, SP1_PROOF_NUM_PV_ELTS};
pub use sp1_core::io::{SP1PublicValues, SP1Stdin};
use sp1_core::stark::{
    Challenge, Chip, Com, Domain, PcsProverData, Prover, ShardMainData, StarkProvingKey,
};
use sp1_core::{
    runtime::Program,
    stark::{
        LocalProver, RiscvAir, ShardProof, StarkGenericConfig, StarkMachine, StarkVerifyingKey, Val,
    },
    utils::{BabyBearPoseidon2, SP1CoreProverError},
};
use sp1_primitives::hash_deferred_proof;
use sp1_recursion_circuit::witness::Witnessable;
use sp1_recursion_compiler::config::InnerConfig;
use sp1_recursion_compiler::ir::Witness;
use sp1_recursion_core::{
    air::RecursionPublicValues,
    runtime::{RecursionProgram, Runtime as RecursionRuntime},
    stark::{config::BabyBearPoseidon2Outer, RecursionAir},
};
pub use sp1_recursion_gnark_ffi::plonk_bn254::PlonkBn254Proof;
use sp1_recursion_gnark_ffi::plonk_bn254::PlonkBn254Prover;
use sp1_recursion_program::hints::Hintable;
pub use sp1_recursion_program::machine::ReduceProgramType;
use sp1_recursion_program::machine::{
    SP1CompressVerifier, SP1DeferredVerifier, SP1RecursiveVerifier, SP1RootVerifier,
};
pub use sp1_recursion_program::machine::{
    SP1DeferredMemoryLayout, SP1RecursionMemoryLayout, SP1ReduceMemoryLayout, SP1RootMemoryLayout,
};
use tracing::instrument;
use sp1_recursion_core::stark::{RecursionAirSkinnyDeg7, RecursionAirWideDeg3};
pub use types::*;
use utils::words_to_bytes;

/// The configuration for the core prover.
pub type CoreSC = BabyBearPoseidon2;

/// The configuration for the inner prover.
pub type InnerSC = BabyBearPoseidon2;

/// The configuration for the outer prover.
pub type OuterSC = BabyBearPoseidon2Outer;

const REDUCE_DEGREE: usize = 3;
const COMPRESS_DEGREE: usize = 9;
const WRAP_DEGREE: usize = 9;

pub type ReduceAir<F> = RecursionAir<F, REDUCE_DEGREE>;
pub type CompressAir<F> = RecursionAir<F, COMPRESS_DEGREE>;
pub type WrapAir<F> = RecursionAir<F, WRAP_DEGREE>;

/// A end-to-end prover implementation for the SP1 RISC-V zkVM.
pub struct SP1Prover {
    /// The program that can recursively verify a set of proofs into a single proof.
    pub recursion_program: RecursionProgram<BabyBear>,

    /// The proving key for the recursion step.
    pub rec_pk: StarkProvingKey<InnerSC>,

    /// The verification key for the recursion step.
    pub rec_vk: StarkVerifyingKey<InnerSC>,

    /// The program that recursively verifies deferred proofs and accumulates the digests.
    pub deferred_program: RecursionProgram<BabyBear>,

    /// The proving key for the reduce step.
    pub deferred_pk: StarkProvingKey<InnerSC>,

    /// The verification key for the reduce step.
    pub deferred_vk: StarkVerifyingKey<InnerSC>,

    /// The program that reduces a set of recursive proofs into a single proof.
    pub compress_program: RecursionProgram<BabyBear>,

    /// The proving key for the reduce step.
    pub compress_pk: StarkProvingKey<InnerSC>,

    /// The verification key for the reduce step.
    pub compress_vk: StarkVerifyingKey<InnerSC>,

    /// The shrink program that compresses a proof into a succinct proof.
    pub shrink_program: RecursionProgram<BabyBear>,

    /// The proving key for the compress step.
    pub shrink_pk: StarkProvingKey<InnerSC>,

    /// The verification key for the compress step.
    pub shrink_vk: StarkVerifyingKey<InnerSC>,

    /// The wrap program that wraps a proof into a SNARK-friendly field.
    pub wrap_program: RecursionProgram<BabyBear>,

    /// The proving key for the wrap step.
    pub wrap_pk: StarkProvingKey<OuterSC>,

    /// The verification key for the wrapping step.
    pub wrap_vk: StarkVerifyingKey<OuterSC>,

    /// The machine used for proving the core step.
    pub core_machine: StarkMachine<CoreSC, RiscvAir<<CoreSC as StarkGenericConfig>::Val>>,

    /// The machine used for proving the recursive and reduction steps.
    pub compress_machine: StarkMachine<InnerSC, ReduceAir<<InnerSC as StarkGenericConfig>::Val>>,

    /// The machine used for proving the shrink step.
    pub shrink_machine: StarkMachine<InnerSC, CompressAir<<InnerSC as StarkGenericConfig>::Val>>,

    /// The machine used for proving the wrapping step.
    pub wrap_machine: StarkMachine<OuterSC, WrapAir<<OuterSC as StarkGenericConfig>::Val>>,
}

impl SP1Prover {
    /// Initializes a new [SP1Prover].
    #[instrument(name = "initialize prover", level = "debug", skip_all)]
    pub fn new() -> Self {
        let core_machine = RiscvAir::machine(CoreSC::default());

        // Get the recursive verifier and setup the proving and verifying keys.
        let recursion_program = SP1RecursiveVerifier::<InnerConfig, _>::build(&core_machine);
        let compress_machine = ReduceAir::machine(InnerSC::default());
        let (rec_pk, rec_vk) = compress_machine.setup(&recursion_program);

        // Get the deferred program and keys.
        let deferred_program = SP1DeferredVerifier::<InnerConfig, _, _>::build(&compress_machine);
        let (deferred_pk, deferred_vk) = compress_machine.setup(&deferred_program);

        // Make the reduce program and keys.
        let compress_program = SP1CompressVerifier::<InnerConfig, _, _>::build(
            &compress_machine,
            &rec_vk,
            &deferred_vk,
        );
        let (compress_pk, compress_vk) = compress_machine.setup(&compress_program);

        // Get the compress program, machine, and keys.
        let shrink_program =
            SP1RootVerifier::<InnerConfig, _, _>::build(&compress_machine, &compress_vk, true);
        let shrink_machine = CompressAir::wrap_machine_dyn(InnerSC::compressed());
        let (shrink_pk, shrink_vk) = shrink_machine.setup(&shrink_program);

        // Get the wrap program, machine, and keys.
        let wrap_program =
            SP1RootVerifier::<InnerConfig, _, _>::build(&shrink_machine, &shrink_vk, false);
        let wrap_machine = WrapAir::wrap_machine(OuterSC::default());
        let (wrap_pk, wrap_vk) = wrap_machine.setup(&wrap_program);

        Self {
            recursion_program,
            rec_pk,
            rec_vk,
            deferred_program,
            deferred_pk,
            deferred_vk,
            compress_program,
            compress_pk,
            compress_vk,
            shrink_program,
            shrink_pk,
            shrink_vk,
            wrap_program,
            wrap_pk,
            wrap_vk,
            core_machine,
            compress_machine,
            shrink_machine,
            wrap_machine,
        }
    }

    /// Initializes a new [SP1Prover].
    #[instrument(name = "initialize prover", level = "info", skip_all)]
    pub fn new_with_some_chips_disabled(chips_to_disable: Vec<&str>) -> Self {
        //let core_machine = RiscvAir::machine(CoreSC::default());
        let core_machine = {
            let mut chips = RiscvAir::get_all()
                .into_iter()
                .map(Chip::new)
                .collect::<Vec<_>>();

            // deactivate chips specified by names from input
            for chip_name in chips_to_disable.into_iter() {
                chips.retain(|chip| chip.name() != chip_name.to_string())
            }

            StarkMachine::new(CoreSC::default(), chips, SP1_PROOF_NUM_PV_ELTS)
        };


        // Get the recursive verifier and setup the proving and verifying keys.
        let recursion_program = SP1RecursiveVerifier::<InnerConfig, _>::build(&core_machine);
        let compress_machine = ReduceAir::machine(InnerSC::default());
        let (rec_pk, rec_vk) = compress_machine.setup(&recursion_program);

        // Get the deferred program and keys.
        let deferred_program = SP1DeferredVerifier::<InnerConfig, _, _>::build(&compress_machine);
        let (deferred_pk, deferred_vk) = compress_machine.setup(&deferred_program);

        // Make the reduce program and keys.
        let compress_program = SP1CompressVerifier::<InnerConfig, _, _>::build(
            &compress_machine,
            &rec_vk,
            &deferred_vk,
        );
        let (compress_pk, compress_vk) = compress_machine.setup(&compress_program);

        // Get the compress program, machine, and keys.
        let shrink_program =
            SP1RootVerifier::<InnerConfig, _, _>::build(&compress_machine, &compress_vk, true);
        let shrink_machine = CompressAir::machine(InnerSC::compressed());
        let (shrink_pk, shrink_vk) = shrink_machine.setup(&shrink_program);

        // Get the wrap program, machine, and keys.
        let wrap_program =
            SP1RootVerifier::<InnerConfig, _, _>::build(&shrink_machine, &shrink_vk, false);
        let wrap_machine = WrapAir::wrap_machine(OuterSC::default());
        let (wrap_pk, wrap_vk) = wrap_machine.setup(&wrap_program);

        Self {
            recursion_program,
            rec_pk,
            rec_vk,
            deferred_program,
            deferred_pk,
            deferred_vk,
            compress_program,
            compress_pk,
            compress_vk,
            shrink_program,
            shrink_pk,
            shrink_vk,
            wrap_program,
            wrap_pk,
            wrap_vk,
            core_machine,
            compress_machine,
            shrink_machine,
            wrap_machine,
        }
    }

    /// Creates a proving key and a verifying key for a given RISC-V ELF.
    #[instrument(name = "setup", level = "debug", skip_all)]
    pub fn setup(&self, elf: &[u8]) -> (SP1ProvingKey, SP1VerifyingKey) {
        let program = Program::from(elf);
        let (pk, vk) = self.core_machine.setup(&program);
        let vk = SP1VerifyingKey { vk };
        let pk = SP1ProvingKey {
            pk,
            elf: elf.to_vec(),
            vk: vk.clone(),
        };
        (pk, vk)
    }

    /// Generate a proof of an SP1 program with the specified inputs.
    #[instrument(name = "execute", level = "info", skip_all)]
    pub fn execute(
        elf: &[u8],
        stdin: &SP1Stdin,
    ) -> Result<(SP1PublicValues, ExecutionReport), ExecutionError> {
        let program = Program::from(elf);
        let opts = SP1CoreOpts::default();
        let mut runtime = Runtime::new(program, opts);
        runtime.write_vecs(&stdin.buffer);
        for (proof, vkey) in stdin.proofs.iter() {
            runtime.write_proof(proof.clone(), vkey.clone());
        }
        runtime.run_untraced()?;
        Ok((
            SP1PublicValues::from(&runtime.state.public_values_stream),
            runtime.report,
        ))
    }

    /// Generate shard proofs which split up and prove the valid execution of a RISC-V program with
    /// the core prover.
    #[instrument(name = "prove_core", level = "info", skip_all)]
    pub fn prove_core(
        &self,
        pk: &SP1ProvingKey,
        stdin: &SP1Stdin,
    ) -> Result<SP1CoreProof, SP1CoreProverError> {
        let config = CoreSC::default();
        let program = Program::from(&pk.elf);
        let opts = SP1CoreOpts::default();
        let (proof, public_values_stream) = sp1_core::utils::prove(program, stdin, config, opts)?;
        let public_values = SP1PublicValues::from(&public_values_stream);
        Ok(SP1CoreProof {
            proof: SP1CoreProofData(proof.shard_proofs),
            stdin: stdin.clone(),
            public_values,
        })
    }

    pub fn get_recursion_core_inputs<'a>(
        &'a self,
        vk: &'a StarkVerifyingKey<CoreSC>,
        leaf_challenger: &'a Challenger<CoreSC>,
        shard_proofs: &[ShardProof<CoreSC>],
        batch_size: usize,
        is_complete: bool,
    ) -> Vec<SP1RecursionMemoryLayout<'a, CoreSC, RiscvAir<BabyBear>>> {
        let mut core_inputs = Vec::new();
        let mut reconstruct_challenger = self.core_machine.config().challenger();
        vk.observe_into(&mut reconstruct_challenger);

        // Prepare the inputs for the recursion programs.
        for batch in shard_proofs.chunks(batch_size) {
            let proofs = batch.to_vec();

            core_inputs.push(SP1RecursionMemoryLayout {
                vk,
                machine: &self.core_machine,
                shard_proofs: proofs,
                leaf_challenger,
                initial_reconstruct_challenger: reconstruct_challenger.clone(),
                is_complete,
            });

            for proof in batch.iter() {
                reconstruct_challenger.observe(proof.commitment.main_commit);
                reconstruct_challenger
                    .observe_slice(&proof.public_values[0..self.core_machine.num_pv_elts()]);
            }
        }

        // Check that the leaf challenger is the same as the reconstruct challenger.
        assert_eq!(
            reconstruct_challenger.sponge_state,
            leaf_challenger.sponge_state
        );
        assert_eq!(
            reconstruct_challenger.input_buffer,
            leaf_challenger.input_buffer
        );
        assert_eq!(
            reconstruct_challenger.output_buffer,
            leaf_challenger.output_buffer
        );
        core_inputs
    }

    pub fn get_recursion_deferred_inputs<'a>(
        &'a self,
        vk: &'a StarkVerifyingKey<CoreSC>,
        leaf_challenger: &'a Challenger<InnerSC>,
        last_proof_pv: &PublicValues<Word<BabyBear>, BabyBear>,
        deferred_proofs: &[ShardProof<InnerSC>],
        batch_size: usize,
    ) -> Vec<SP1DeferredMemoryLayout<'a, InnerSC, RecursionAir<BabyBear, 3>>> {
        // Prepare the inputs for the deferred proofs recursive verification.
        let mut deferred_digest = [Val::<InnerSC>::zero(); DIGEST_SIZE];
        let mut deferred_inputs = Vec::new();

        for batch in deferred_proofs.chunks(batch_size) {
            let proofs = batch.to_vec();

            deferred_inputs.push(SP1DeferredMemoryLayout {
                compress_vk: &self.compress_vk,
                machine: &self.compress_machine,
                proofs,
                start_reconstruct_deferred_digest: deferred_digest.to_vec(),
                is_complete: false,
                sp1_vk: vk,
                sp1_machine: &self.core_machine,
                end_pc: Val::<InnerSC>::zero(),
                end_shard: last_proof_pv.shard,
                leaf_challenger: leaf_challenger.clone(),
                committed_value_digest: last_proof_pv.committed_value_digest.to_vec(),
                deferred_proofs_digest: last_proof_pv.deferred_proofs_digest.to_vec(),
            });

            deferred_digest = Self::hash_deferred_proofs(deferred_digest, batch);
        }
        deferred_inputs
    }

    /// Generate the inputs for the first layer of recursive proofs.
    #[allow(clippy::type_complexity)]
    pub fn get_first_layer_inputs<'a>(
        &'a self,
        vk: &'a SP1VerifyingKey,
        leaf_challenger: &'a Challenger<InnerSC>,
        shard_proofs: &[ShardProof<InnerSC>],
        deferred_proofs: &[ShardProof<InnerSC>],
        batch_size: usize,
    ) -> (
        Vec<SP1RecursionMemoryLayout<'a, InnerSC, RiscvAir<BabyBear>>>,
        Vec<SP1DeferredMemoryLayout<'a, InnerSC, RecursionAir<BabyBear, 3>>>,
    ) {
        let is_complete = shard_proofs.len() == 1 && deferred_proofs.is_empty();
        let core_inputs = self.get_recursion_core_inputs(
            &vk.vk,
            leaf_challenger,
            shard_proofs,
            batch_size,
            is_complete,
        );
        let last_proof_pv =
            PublicValues::from_vec(shard_proofs.last().unwrap().public_values.clone());
        let deferred_inputs = self.get_recursion_deferred_inputs(
            &vk.vk,
            leaf_challenger,
            &last_proof_pv,
            deferred_proofs,
            batch_size,
        );
        (core_inputs, deferred_inputs)
    }

    /// Reduce shards proofs to a single shard proof using the recursion prover.
    #[instrument(name = "compress", level = "info", skip_all)]
    pub fn compress(
        &self,
        vk: &SP1VerifyingKey,
        proof: SP1CoreProof,
        deferred_proofs: Vec<ShardProof<InnerSC>>,
    ) -> Result<SP1ReduceProof<InnerSC>, SP1RecursionProverError> {
        // Set the batch size for the reduction tree.
        let batch_size = 2;

        let shard_proofs = &proof.proof.0;
        // Get the leaf challenger.
        let mut leaf_challenger = self.core_machine.config().challenger();
        vk.vk.observe_into(&mut leaf_challenger);
        shard_proofs.iter().for_each(|proof| {
            leaf_challenger.observe(proof.commitment.main_commit);
            leaf_challenger.observe_slice(&proof.public_values[0..self.core_machine.num_pv_elts()]);
        });

        // Run the recursion and reduce programs.
        let (core_inputs, deferred_inputs) = self.get_first_layer_inputs(
            vk,
            &leaf_challenger,
            shard_proofs,
            &deferred_proofs,
            batch_size,
        );

        let mut first_layer_proofs = Vec::new();
        let opts = SP1CoreOpts::recursion();
        let shard_batch_size = opts.shard_batch_size;
        for inputs in core_inputs.chunks(shard_batch_size) {
            let proofs = inputs
                .into_par_iter()
                .map(|input| {
                    let mut runtime = RecursionRuntime::<Val<InnerSC>, Challenge<InnerSC>, _>::new(
                        &self.recursion_program,
                        self.compress_machine.config().perm.clone(),
                    );

                    let mut witness_stream = Vec::new();
                    witness_stream.extend(input.write());

                    runtime.witness_stream = witness_stream.into();
                    runtime.run();
                    runtime.print_stats();

                    let pk = &self.rec_pk;
                    let mut recursive_challenger = self.compress_machine.config().challenger();
                    (
                        self.compress_machine.prove::<LocalProver<_, _>>(
                            pk,
                            runtime.record,
                            &mut recursive_challenger,
                            opts,
                        ),
                        ReduceProgramType::Core,
                    )
                })
                .collect::<Vec<_>>();
            first_layer_proofs.extend(proofs);
        }

        // Run the deferred proofs programs.
        for inputs in deferred_inputs.chunks(shard_batch_size) {
            let proofs = inputs
                .into_par_iter()
                .map(|input| {
                    let mut runtime = RecursionRuntime::<Val<InnerSC>, Challenge<InnerSC>, _>::new(
                        &self.deferred_program,
                        self.compress_machine.config().perm.clone(),
                    );

                    let mut witness_stream = Vec::new();
                    witness_stream.extend(input.write());

                    runtime.witness_stream = witness_stream.into();
                    runtime.run();
                    runtime.print_stats();

                    let pk = &self.deferred_pk;
                    let mut recursive_challenger = self.compress_machine.config().challenger();
                    (
                        self.compress_machine.prove::<LocalProver<_, _>>(
                            pk,
                            runtime.record,
                            &mut recursive_challenger,
                            opts,
                        ),
                        ReduceProgramType::Deferred,
                    )
                })
                .collect::<Vec<_>>();
            first_layer_proofs.extend(proofs);
        }

        // Chain all the individual shard proofs.
        let mut reduce_proofs = first_layer_proofs
            .into_iter()
            .flat_map(|(proof, kind)| proof.shard_proofs.into_iter().map(move |p| (p, kind)))
            .collect::<Vec<_>>();

        // Iterate over the recursive proof batches until there is one proof remaining.
        let mut is_complete;
        loop {
            tracing::debug!("Recursive proof layer size: {}", reduce_proofs.len());
            is_complete = reduce_proofs.len() <= batch_size;

            let compress_inputs = reduce_proofs.chunks(batch_size).collect::<Vec<_>>();
            let batched_compress_inputs =
                compress_inputs.chunks(shard_batch_size).collect::<Vec<_>>();
            reduce_proofs = batched_compress_inputs
                .into_iter()
                .flat_map(|batches| {
                    batches
                        .par_iter()
                        .map(|batch| {
                            let (shard_proofs, kinds) =
                                batch.iter().cloned().unzip::<_, _, Vec<_>, Vec<_>>();

                            let input = SP1ReduceMemoryLayout {
                                compress_vk: &self.compress_vk,
                                recursive_machine: &self.compress_machine,
                                shard_proofs,
                                kinds,
                                is_complete,
                            };

                            let proof = self.compress_machine_proof(
                                input,
                                &self.compress_program,
                                &self.compress_pk,
                            );
                            (proof, ReduceProgramType::Reduce)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();

            if reduce_proofs.len() == 1 {
                break;
            }
        }
        debug_assert_eq!(reduce_proofs.len(), 1);
        let reduce_proof = reduce_proofs.pop().unwrap();

        Ok(SP1ReduceProof {
            proof: reduce_proof.0,
        })
    }

    pub fn compress_machine_proof(
        &self,
        input: impl Hintable<InnerConfig>,
        program: &RecursionProgram<BabyBear>,
        pk: &StarkProvingKey<InnerSC>,
    ) -> ShardProof<InnerSC> {
        let mut runtime = RecursionRuntime::<Val<InnerSC>, Challenge<InnerSC>, _>::new(
            program,
            self.compress_machine.config().perm.clone(),
        );

        let mut witness_stream = Vec::new();
        witness_stream.extend(input.write());

        runtime.witness_stream = witness_stream.into();
        runtime.run();
        runtime.print_stats();

        let opts = SP1CoreOpts::recursion();
        let mut recursive_challenger = self.compress_machine.config().challenger();
        self.compress_machine
            .prove::<LocalProver<_, _>>(pk, runtime.record, &mut recursive_challenger, opts)
            .shard_proofs
            .pop()
            .unwrap()
    }

    /// Wrap a reduce proof into a STARK proven over a SNARK-friendly field.
    #[instrument(name = "shrink", level = "info", skip_all)]
    pub fn shrink(
        &self,
        reduced_proof: SP1ReduceProof<InnerSC>,
    ) -> Result<SP1ReduceProof<InnerSC>, SP1RecursionProverError> {
        // Make the compress proof.
        let input = SP1RootMemoryLayout {
            machine: &self.compress_machine,
            proof: reduced_proof.proof,
            is_reduce: true,
        };

        // Run the compress program.
        let mut runtime = RecursionRuntime::<Val<InnerSC>, Challenge<InnerSC>, _>::new(
            &self.shrink_program,
            self.shrink_machine.config().perm.clone(),
        );

        let mut witness_stream = Vec::new();
        witness_stream.extend(input.write());

        runtime.witness_stream = witness_stream.into();
        runtime.run();
        runtime.print_stats();
        tracing::debug!("Compress program executed successfully");

        // Prove the compress program.
        let opts = SP1CoreOpts::recursion();
        let mut compress_challenger = self.shrink_machine.config().challenger();
        let mut compress_proof = self.shrink_machine.prove::<LocalProver<_, _>>(
            &self.shrink_pk,
            runtime.record,
            &mut compress_challenger,
            opts,
        );

        Ok(SP1ReduceProof {
            proof: compress_proof.shard_proofs.pop().unwrap(),
        })
    }

    /// Wrap a reduce proof into a STARK proven over a SNARK-friendly field.
    #[instrument(name = "wrap_bn254", level = "info", skip_all)]
    pub fn wrap_bn254(
        &self,
        compressed_proof: SP1ReduceProof<InnerSC>,
    ) -> Result<SP1ReduceProof<OuterSC>, SP1RecursionProverError> {
        let input = SP1RootMemoryLayout {
            machine: &self.shrink_machine,
            proof: compressed_proof.proof,
            is_reduce: false,
        };

        // Run the compress program.
        let mut runtime = RecursionRuntime::<Val<InnerSC>, Challenge<InnerSC>, _>::new(
            &self.wrap_program,
            self.shrink_machine.config().perm.clone(),
        );

        let mut witness_stream = Vec::new();
        witness_stream.extend(input.write());

        runtime.witness_stream = witness_stream.into();
        runtime.run();
        runtime.print_stats();
        tracing::debug!("Wrap program executed successfully");

        // Prove the wrap program.
        let opts = SP1CoreOpts::recursion();
        let mut wrap_challenger = self.wrap_machine.config().challenger();
        let time = std::time::Instant::now();
        let mut wrap_proof = self.wrap_machine.prove::<LocalProver<_, _>>(
            &self.wrap_pk,
            runtime.record,
            &mut wrap_challenger,
            opts,
        );
        let elapsed = time.elapsed();
        tracing::debug!("Wrap proving time: {:?}", elapsed);
        let mut wrap_challenger = self.wrap_machine.config().challenger();
        let result = self
            .wrap_machine
            .verify(&self.wrap_vk, &wrap_proof, &mut wrap_challenger);
        match result {
            Ok(_) => tracing::info!("Proof verified successfully"),
            Err(MachineVerificationError::NonZeroCumulativeSum) => {
                tracing::info!("Proof verification failed: NonZeroCumulativeSum")
            }
            e => panic!("Proof verification failed: {:?}", e),
        }
        tracing::info!("Wrapping successful");

        Ok(SP1ReduceProof {
            proof: wrap_proof.shard_proofs.pop().unwrap(),
        })
    }

    /// Wrap the STARK proven over a SNARK-friendly field into a PLONK proof.
    #[instrument(name = "wrap_plonk_bn254", level = "info", skip_all)]
    pub fn wrap_plonk_bn254(
        &self,
        proof: SP1ReduceProof<OuterSC>,
        build_dir: &Path,
    ) -> PlonkBn254Proof {
        let vkey_digest = proof.sp1_vkey_digest_bn254();
        let commited_values_digest = proof.sp1_commited_values_digest_bn254();

        let mut witness = Witness::default();
        proof.proof.write(&mut witness);
        witness.write_commited_values_digest(commited_values_digest);
        witness.write_vkey_hash(vkey_digest);

        let prover = PlonkBn254Prover::new();
        let proof = prover.prove(witness, build_dir.to_path_buf());

        // Verify the proof.
        prover.verify(
            &proof,
            &vkey_digest.as_canonical_biguint(),
            &commited_values_digest.as_canonical_biguint(),
            build_dir,
        );

        proof
    }

    /// Accumulate deferred proofs into a single digest.
    pub fn hash_deferred_proofs(
        prev_digest: [Val<CoreSC>; DIGEST_SIZE],
        deferred_proofs: &[ShardProof<InnerSC>],
    ) -> [Val<CoreSC>; 8] {
        let mut digest = prev_digest;
        for proof in deferred_proofs.iter() {
            let pv: &RecursionPublicValues<Val<CoreSC>> = proof.public_values.as_slice().borrow();
            let committed_values_digest = words_to_bytes(&pv.committed_value_digest);
            digest = hash_deferred_proof(
                &digest,
                &pv.sp1_vk_digest,
                &committed_values_digest.try_into().unwrap(),
            );
        }
        digest
    }
}

#[cfg(test)]
mod tests {

    use std::fs::File;
    use std::io::{Read, Write};
    use std::time::Instant;

    use self::build::try_build_plonk_bn254_artifacts_dev;
    use super::*;

    use anyhow::Result;
    use p3_field::PrimeField32;
    use serial_test::serial;
    use sp1_core::io::SP1Stdin;
    use sp1_core::utils::setup_logger;

    /// Tests an end-to-end workflow of proving a program across the entire proof generation
    /// pipeline.
    ///
    /// Add `FRI_QUERIES`=1 to your environment for faster execution. Should only take a few minutes
    /// on a Mac M2. Note: This test always re-builds the plonk bn254 artifacts, so setting SP1_DEV is
    /// not needed.
    #[test]
    #[serial]
    fn test_e2e() -> Result<()> {
        setup_logger();
        let elf = include_bytes!("../../tests/fibonacci/elf/riscv32im-succinct-zkvm-elf");

        tracing::info!("initializing prover");
        let prover = SP1Prover::new();

        tracing::info!("setup elf");
        let (pk, vk) = prover.setup(elf);

        tracing::info!("prove core");
        let stdin = SP1Stdin::new();
        let core_proof = prover.prove_core(&pk, &stdin)?;
        let public_values = core_proof.public_values.clone();

        tracing::info!("verify core");
        prover.verify(&core_proof.proof, &vk)?;

        tracing::info!("compress");
        let compressed_proof = prover.compress(&vk, core_proof, vec![])?;

        tracing::info!("verify compressed");
        prover.verify_compressed(&compressed_proof, &vk)?;

        tracing::info!("shrink");
        let shrink_proof = prover.shrink(compressed_proof)?;

        tracing::info!("verify shrink");
        prover.verify_shrink(&shrink_proof, &vk)?;

        tracing::info!("wrap bn254");
        let wrapped_bn254_proof = prover.wrap_bn254(shrink_proof)?;
        let bytes = bincode::serialize(&wrapped_bn254_proof).unwrap();

        // Save the proof.
        let mut file = File::create("proof-with-pis.json").unwrap();
        file.write_all(bytes.as_slice()).unwrap();

        // Load the proof.
        let mut file = File::open("proof-with-pis.json").unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();

        let wrapped_bn254_proof = bincode::deserialize(&bytes).unwrap();

        tracing::info!("verify wrap bn254");
        prover.verify_wrap_bn254(&wrapped_bn254_proof, &vk).unwrap();

        tracing::info!("checking vkey hash babybear");
        let vk_digest_babybear = wrapped_bn254_proof.sp1_vkey_digest_babybear();
        assert_eq!(vk_digest_babybear, vk.hash_babybear());

        tracing::info!("checking vkey hash bn254");
        let vk_digest_bn254 = wrapped_bn254_proof.sp1_vkey_digest_bn254();
        assert_eq!(vk_digest_bn254, vk.hash_bn254());

        tracing::info!("generate plonk bn254 proof");
        let artifacts_dir =
            try_build_plonk_bn254_artifacts_dev(&prover.wrap_vk, &wrapped_bn254_proof.proof);
        let plonk_bn254_proof = prover.wrap_plonk_bn254(wrapped_bn254_proof, &artifacts_dir);
        println!("{:?}", plonk_bn254_proof);

        prover.verify_plonk_bn254(&plonk_bn254_proof, &vk, &public_values, &artifacts_dir)?;

        Ok(())
    }

    /// Tests an end-to-end workflow of proving a program across the entire proof generation
    /// pipeline in addition to verifying deferred proofs.
    #[test]
    #[serial]
    fn test_e2e_with_deferred_proofs() -> Result<()> {
        setup_logger();

        // Test program which proves the Keccak-256 hash of various inputs.
        let keccak_elf = include_bytes!("../../tests/keccak256/elf/riscv32im-succinct-zkvm-elf");

        // Test program which verifies proofs of a vkey and a list of committed inputs.
        let verify_elf = include_bytes!("../../tests/verify-proof/elf/riscv32im-succinct-zkvm-elf");

        tracing::info!("initializing prover");
        let prover = SP1Prover::new();

        tracing::info!("setup keccak elf");
        let (keccak_pk, keccak_vk) = prover.setup(keccak_elf);

        tracing::info!("setup verify elf");
        let (verify_pk, verify_vk) = prover.setup(verify_elf);

        tracing::info!("prove subproof 1");
        let mut stdin = SP1Stdin::new();
        stdin.write(&1usize);
        stdin.write(&vec![0u8, 0, 0]);
        let deferred_proof_1 = prover.prove_core(&keccak_pk, &stdin)?;
        let pv_1 = deferred_proof_1.public_values.as_slice().to_vec().clone();

        // Generate a second proof of keccak of various inputs.
        tracing::info!("prove subproof 2");
        let mut stdin = SP1Stdin::new();
        stdin.write(&3usize);
        stdin.write(&vec![0u8, 1, 2]);
        stdin.write(&vec![2, 3, 4]);
        stdin.write(&vec![5, 6, 7]);
        let deferred_proof_2 = prover.prove_core(&keccak_pk, &stdin)?;
        let pv_2 = deferred_proof_2.public_values.as_slice().to_vec().clone();

        // Generate recursive proof of first subproof.
        tracing::info!("compress subproof 1");
        let deferred_reduce_1 = prover.compress(&keccak_vk, deferred_proof_1, vec![])?;

        // Generate recursive proof of second subproof.
        tracing::info!("compress subproof 2");
        let deferred_reduce_2 = prover.compress(&keccak_vk, deferred_proof_2, vec![])?;

        // Run verify program with keccak vkey, subproofs, and their committed values.
        let mut stdin = SP1Stdin::new();
        let vkey_digest = keccak_vk.hash_babybear();
        let vkey_digest: [u32; 8] = vkey_digest
            .iter()
            .map(|n| n.as_canonical_u32())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        stdin.write(&vkey_digest);
        stdin.write(&vec![pv_1.clone(), pv_2.clone(), pv_2.clone()]);
        stdin.write_proof(deferred_reduce_1.proof.clone(), keccak_vk.vk.clone());
        stdin.write_proof(deferred_reduce_2.proof.clone(), keccak_vk.vk.clone());
        stdin.write_proof(deferred_reduce_2.proof.clone(), keccak_vk.vk.clone());

        tracing::info!("proving verify program (core)");
        let verify_proof = prover.prove_core(&verify_pk, &stdin)?;

        // Generate recursive proof of verify program
        tracing::info!("compress verify program");
        let verify_reduce = prover.compress(
            &verify_vk,
            verify_proof,
            vec![
                deferred_reduce_1.proof,
                deferred_reduce_2.proof.clone(),
                deferred_reduce_2.proof,
            ],
        )?;
        let reduce_pv: &RecursionPublicValues<_> =
            verify_reduce.proof.public_values.as_slice().borrow();
        println!("deferred_hash: {:?}", reduce_pv.deferred_proofs_digest);
        println!("complete: {:?}", reduce_pv.is_complete);

        tracing::info!("verify verify program");
        prover.verify_compressed(&verify_reduce, &verify_vk)?;

        Ok(())
    }

    #[test]
    fn test_multi_precompile_program_with_deferred_proofs() {
        fn test_inner(
            program_elf: &[u8],
            deferred_proofs_num: usize,
            program_inputs: Vec<&SP1Stdin>,
        ) {
            assert_eq!(deferred_proofs_num, program_inputs.len());
            setup_logger();

            // verify program which verifies proofs of a vkey and a list of committed inputs
            let verify_elf =
                include_bytes!("../../tests/verify-proof/elf/riscv32im-succinct-zkvm-elf");

            tracing::info!("initializing prover");
            let prover = SP1Prover::new();

            tracing::info!("setup elf");
            let (program_pk, program_vk) = prover.setup(program_elf);
            let (verify_pk, verify_vk) = prover.setup(verify_elf);

            // Generate deferred proofs
            let mut public_values = vec![];
            let mut deferred_reduce_proofs = vec![];
            program_inputs
                .into_iter()
                .enumerate()
                .for_each(|(index, input)| {
                    tracing::info!("prove subproof {}", index);
                    let deferred_proof = prover.prove_core(&program_pk, input).unwrap();
                    let pv = deferred_proof.public_values.as_slice().to_vec().clone();
                    public_values.push(pv);
                    let deferred_reduce = prover.compress(&program_vk, deferred_proof, vec![]).unwrap();
                    deferred_reduce_proofs.push(deferred_reduce.proof);
                });

            // Aggregate deferred proofs
            let mut stdin = SP1Stdin::new();
            let vkey_digest = program_vk.hash_babybear();
            let vkey_digest: [u32; 8] = vkey_digest
                .iter()
                .map(|n| n.as_canonical_u32())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            stdin.write(&vec![vkey_digest; deferred_proofs_num]);
            stdin.write(&public_values);
            for drp in deferred_reduce_proofs.iter() {
                stdin.write_proof(drp.clone(), program_vk.vk.clone());
            }

            // Generate aggregated proof
            let verify_proof = prover.prove_core(&verify_pk, &stdin).unwrap();
            let verify_reduce =
                prover.compress(&verify_vk, verify_proof.clone(), deferred_reduce_proofs).unwrap();

            let reduce_pv: &RecursionPublicValues<_> =
                verify_reduce.proof.public_values.as_slice().borrow();
            println!("deferred_hash: {:?}", reduce_pv.deferred_proofs_digest);
            println!("complete: {:?}", reduce_pv.is_complete);

            tracing::info!("verify verify program");
            prover.verify_compressed(&verify_reduce, &verify_vk)?;
        }

        //env::set_var("RECONSTRUCT_COMMITMENTS", "false");
        //env::set_var("FRI_QUERIES", "1");
        //env::set_var("SHARD_SIZE", "262144");
        //env::set_var("MAX_RECURSION_PROGRAM_SIZE", "1");

        // Programs that we will use to produce deferred proofs while testing
        let multi_precompile_program =
            include_bytes!("../../tests/multi-precompile-program/elf/riscv32im-succinct-zkvm-elf");

        let calls = 100_000usize;

        let mut inputs = vec![];
        let mut input = SP1Stdin::new();
        input.write(&0usize);
        input.write(&calls);
        inputs.push(&input);
        let mut input = SP1Stdin::new();
        input.write(&1usize);
        input.write(&calls);
        inputs.push(&input);
        let mut input = SP1Stdin::new();
        input.write(&2usize);
        input.write(&calls);
        inputs.push(&input);
        let mut input = SP1Stdin::new();
        input.write(&3usize);
        input.write(&calls);
        inputs.push(&input);

        test_inner(multi_precompile_program, inputs.len(), inputs);
    }

    #[test]
    fn test_multi_precompile_program_with_patched_stark_machine() {
        setup_logger();

        fn run_experiment(name: String, chips_to_deactivate: Vec<&str>, input: SP1Stdin) {
            let multi_precompile_program = include_bytes!(
                "../../tests/multi-precompile-program/elf/riscv32im-succinct-zkvm-elf"
            );

            // run test with all precompiles/chips enabled
            let prover = SP1Prover::new();
            let (pk, vk) = prover.setup(multi_precompile_program);
            let start = Instant::now();
            let core_proof = prover.prove_core(&pk, &input).unwrap();
            let prove_core_took = start.elapsed();
            let start = Instant::now();
            let _reduce_proof = prover.compress(&vk, core_proof, vec![]);
            println!(
                "[{}] prove_core took: {:?}, compress took: {:?}",
                name,
                prove_core_took,
                start.elapsed()
            );

            // run test with some precompiles/chips deactivated
            let prover = SP1Prover::new_with_some_chips_disabled(chips_to_deactivate);
            let start = Instant::now();
            let core_proof = prover.prove_core(&pk, &input).unwrap();
            let prove_core_took = start.elapsed();
            let start = Instant::now();
            let _reduce_proof = prover.compress(&vk, core_proof, vec![]);
            println!(
                "[{}] prove_core took: {:?}, compress took: {:?}",
                name,
                prove_core_took,
                start.elapsed()
            );
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

        let calls = 200_000usize;
        let mut input = SP1Stdin::new();
        input.write(&0usize);
        input.write(&calls);
        run_experiment("sha-extend".to_string(), chips_to_deactivate, input); // invoking sha_extend precompile

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

        let calls = 1_000_000usize;
        let mut input = SP1Stdin::new();
        input.write(&1usize);
        input.write(&calls);
        run_experiment("secp256-k1-double".to_string(), chips_to_deactivate, input); // invoking Secp256k1Double precompile

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

        let calls = 1_000_000usize;
        let mut input = SP1Stdin::new();
        input.write(&2usize);
        input.write(&calls);
        run_experiment("bn254-double".to_string(), chips_to_deactivate, input); // invoking Bn254Double precompile

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

        let calls = 1_000_000usize;
        let mut input = SP1Stdin::new();
        input.write(&3usize);
        input.write(&calls);
        run_experiment("bls12381-double".to_string(), chips_to_deactivate, input); // invoking Bls12381Double precompile
    }
}
