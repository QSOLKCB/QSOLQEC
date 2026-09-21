# QSOLQEC Roadmap

This roadmap is intentionally modular. A research module can advance without forcing unrelated modules to move with it.

## Experimental maturity

```text
E0  sketch
E1  executes
E2  deterministic fixture
E3  oracle-compared
E4  benchmarked
E5  independently replicated
E6  candidate for bridge evaluation
```

Maturity is evidence metadata, not a quality score.

## R0 - Modular research foundation

**Complete: PR #1**

- Define the experimental scope and claim boundaries.
- Establish QSOL-QEC-BRIDGE as the only intended QEC integration path.
- Create a small Rust workspace.
- Define `SystemSpec = Q(d,n)`.
- Define module capabilities, typed data kinds, and maturity metadata.
- Validate experiment graphs without executing arbitrary modules.
- Add tests for invalid system sizes and incompatible pipeline edges.

No decoder, statevector, QEC code, GPU backend, or sonifier is implemented in R0.

## R1 - Native qudit dense oracle

**Complete: PR #2**

- Complex dense state for `Q(d,n)`.
- Explicit basis ordering.
- Checked `d^n` sizing and fallible allocation.
- Norm calculation.
- Basis-state construction.
- Explicit amplitude construction without silent normalization.
- Probability extraction.
- Deterministic machine-checked fixtures for d=2, d=3, d=4.
- Dense module descriptor at E2 deterministic-fixture maturity.

The dense engine is a correctness oracle, not the scalability strategy.

## R2 - Generalized qudit operations

**Complete: PR #3**

- Representation-independent operation definitions.
- Weyl `X_d`.
- Weyl `Z_d`.
- Positive-exponent discrete Fourier transform `F_d`.
- SUM-style controlled shift.
- Swap and local permutation operations.
- Validated generic local unitary fallback.
- Dense scalar local kernels without global `d^n x d^n` matrices.
- Full batch validation before mutation.
- Algebraic property tests for d=2, d=3, d=4.

## R3 - Glass Box

- Representation-neutral `ObservableState` snapshot contract.
- Typed pre/post execution events.
- Representation ID/version and semantic state digest.
- Explicit IEEE-754 numerical comparison contract.
- Exact/approximate representation declaration.
- Wall-clock timing observation.
- Logical representation-byte observation.
- Stable SHA-256 operation identity.
- Stable SHA-256 receipt artifact identity.
- Artifact identity excludes wall-clock timing, event sequence, and diagnostic text.
- Failed execution remains observable rather than being hidden or retried.

The observer wraps kernels rather than serializing from inside hot loops. Timing is an observation, not a benchmark claim.

## R4 - First alternate representation

Initial candidate: stabilizer/tableau where the mathematical domain permits it.

- Same experiment contract as dense oracle.
- Explicit exact/approximate/unsupported capability.
- Oracle comparison on tractable fixtures.
- Representation-size accounting.

## R5 - Benchmark harness

Compare representations using:

- runtime;
- memory;
- allocation count;
- representation size;
- oracle fidelity;
- maximum feasible `n`;
- operation family;
- compute environment.

Benchmark observations are not correctness proofs.

## R6 - Noise and decoder modules

- Replayable error/noise specifications.
- Syndrome data kind.
- Decoder module contract.
- Exact decoder where tractable.
- Candidate decoder comparisons.
- No oracle rescue unless explicitly part of the candidate algorithm.

## R7 - Flagship ququart experiment

Compare:

```text
native Q(4,n)
vs
packed Q(2,2n)
```

under explicitly equivalent workloads where equivalence can be defined.

Measure runtime, memory, representation growth, numerical agreement, and decoder behavior.

## R8 - Sonification observer

- Deterministic event-to-audio mapping.
- No correction authority.
- Preserve provenance to the source event.
- Optionally test whether audio improves human anomaly detection versus logs alone.

Historical QC64 audio work may inform interface ideas, with original authorship preserved.

## R9 - Compute acceleration

Potential modules:

- scalar reference;
- threaded CPU;
- SIMD;
- GPU;
- multi-GPU;
- QSOL-MESH distributed compute.

Every optimized backend remains subordinate to a declared numerical contract.

## R10 - Experimental representations

Potential research:

- sparse amplitudes;
- tensor networks / MPS;
- decision diagrams;
- Pauli/Weyl expansions;
- spectral representations;
- low-rank models;
- learned compression;
- new QSOL representations.

Failure to outperform a baseline is a valid result.

## R11 - AI observer

Only after deterministic experiment evidence exists:

- receipt interpretation;
- anomaly classification;
- hypothesis suggestion;
- strategy recommendation.

Compare AI, deterministic heuristics, and human observers on the same hidden corpus.

## R12 - Bridge candidate

A mature module may be exported to QSOL-QEC-BRIDGE only when there is a bounded integration reason.

The bridge, not QSOLQEC, owns conformance with QEC.
