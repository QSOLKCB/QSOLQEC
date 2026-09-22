# QSOLQEC

**A modular experimental compute laboratory for classical emulation, observation, correction, compression, and analysis of quantum and qudit computation.**

QSOLQEC is deliberately less strict than [QSOLKCB/QEC](https://github.com/QSOLKCB/QEC). It is the workshop: ideas are allowed to be speculative, approximate, incomplete, or wrong as long as the experiment declares what it actually demonstrates.

Mature results do **not** move directly into QEC. Candidates intended for integration must pass through [QSOLKCB/QSOL-QEC-BRIDGE](https://github.com/QSOLKCB/QSOL-QEC-BRIDGE).

## Central research question

> **How much quantum computation can be reproduced, compressed, decoded, or otherwise made tractable by deterministic classical computation before the classical representation hits the exponential wall?**

QSOLQEC does not assume quantum advantage is impossible or inevitable. It tries to identify the boundary experimentally by making classical emulation as strong, modular, observable, and reproducible as practical.

A useful model is:

```text
Q(d, n)

d = local state dimension
n = number of subsystems

Q(2, n) -> qubits
Q(3, n) -> qutrits
Q(4, n) -> ququarts
```

The naive dense state requires `d^n` complex amplitudes. The dense representation is the small-system oracle, not the scalability strategy.

## What this repository is

QSOLQEC is intended to host interchangeable research modules for:

- native qubit, qutrit, ququart, and general-qudit representations;
- exact dense statevector reference execution;
- sparse, stabilizer, tensor, decision-diagram, spectral, or future experimental representations;
- generalized qudit operations;
- noise and error models;
- quantum-error-correction decoders;
- compression and approximation experiments;
- Glass Box observation and diagnostics;
- sonification of deterministic experiment events;
- CPU, SIMD, GPU, multi-GPU, and distributed compute backends;
- analysis and benchmarking;
- small AI observers or strategy recommenders with no implicit authority.

The core runtime should know as little quantum physics as possible. Modules advertise what capabilities they provide and what typed data they consume and produce.

## What this repository is not

QSOLQEC is not:

- quantum hardware;
- evidence of physical quantum behavior merely because a model was simulated;
- a quantum-advantage claimant on the basis of classical emulation;
- a Qiskit or Aer wrapper;
- an LLM decoder;
- a direct extension of QSOLKCB/QEC;
- a place where experimental output automatically becomes canonical evidence.

**Simulation is evidence about the declared model, not evidence about physical quantum hardware.**

If a classical emulator efficiently reproduces a computation proposed as a quantum-separation example, that weakens the separation for that task; it does not make the classical emulator a quantum computer.

See [CLAIM_BOUNDARIES.md](CLAIM_BOUNDARIES.md).

## Architecture

```text
                         QSOLQEC
                            |
                     experiment runtime
                            |
                     module registry
                            |
    +---------+-------------+------------+---------+
    |         |             |            |         |
  state    operation      decoder      observer   compute
 modules    modules       modules       modules    modules
    |         |             |            |         |
    +---------+-------------+------------+---------+
                            |
                       event stream
                            |
                    experiment artifacts
                            |
                    maturity / evidence
                            |
                   optional bridge export
                            |
                  QSOL-QEC-BRIDGE
                            |
                           QEC
```

The architecture is defined in [ARCHITECTURE.md](ARCHITECTURE.md).

## Experimental maturity

Modules and experiments may declare a lightweight maturity level:

```text
E0  sketch
E1  executes
E2  deterministic fixture
E3  oracle-compared
E4  benchmarked
E5  independently replicated
E6  candidate for bridge evaluation
```

This is not a quality score. It records what evidence exists.

## Rust workspace

```text
crates/
├── qsolqec-core/         Q(d,n), basis ordering, representation-independent geometry
├── qsolqec-module-api/   module descriptors and typed capability contract
├── qsolqec-runtime/      experiment graph validation
├── qsolqec-ops/          generalized qudit operation semantics
├── qsolqec-glassbox/     observation events and artifact identity
├── qsolqec-dense/        dense oracle + scalar operation execution
├── qsolqec-stabilizer/   R4 exact prime-d stabilizer/tableau candidate
├── qsolqec-memorywall/   R5 benchmark + memory-wall experiment runtime
├── qsolqec-storage/      R6 neutral structured logical-storage contract
├── qsolqec-fly-phi664/   R7 MaleCNS-bound Phi664 sparse storage prototype
├── qsolqec-fly-qdn/      R8A/R8B exact Q(d,n)-bound Fly-Phi664 paths
├── qsolqec-qec/          R10 replayable noise, syndrome, and decoder modules
└── qsolqec-cli/          minimal executable smoke path
```

R5 adds a provider-neutral memory-wall runtime that executes deterministic workloads through Dense and PrimeStabilizer, records logical representation bytes separately from process RSS, and emits machine-readable receipts suitable for repeated local/cloud experiments. Every benchmark receipt is built only from a clean Git checkout with the committed `Cargo.lock`, and its build-time SHA must be retrievable from the canonical public QSOLQEC repository before the executable is accepted.

R6 adds a representation-neutral `qsolqec-storage` contract for large logical address spaces with checked geometry-bound packed addresses, bounded materialization windows, sparse/dense backing declarations, deterministic tile ownership, explicit persistence versions, exact/approximate storage declarations, and a typed `ObservableStorage` snapshot that is deliberately separate from Glass Box quantum-state observation.

R7 adds `qsolqec-fly-phi664`, a concrete sparse substrate that binds an exact MaleCNS v1.0 `bodyId` set into source identity and gives every macro node three disjoint finite fibres: F27, N125, and R512. It round-trips structured addresses through a stable packed namespace, enforces bounded materialization, and emits the R6 storage snapshot without implementing `SystemSpec` or Glass Box `ObservableState`.

R8A adds `qsolqec-fly-qdn`, the first explicit Q(d,n)-bound adapter over Fly-Phi664. It maps basis index directly onto canonical packed Phi664 addresses, stores exact Complex64 payload bits sparsely, reconstructs the complete amplitude vector for the Gate-A baseline, implements Glass Box `ObservableState`, and executes the exact Clifford-style operation subset without a Dense runtime fallback.\n\nR8B adds the exact virtualized path inside `qsolqec-fly-qdn`: density-adaptive pages, sound sparse/permutation reduction, active-lane Fourier pruning, bounded reusable worker-local SoA scratch, invariant and signature-bound reuse, immutable shared tiles, duplicate in-flight coalescing, and static partitioned ownership. It preserves the Gate-A semantic digest and bit-level operation results. Approximation remains disabled and no portable performance claim is made before R9.

R9 adds the amplitude-memory-wall challenge, placing Dense, PrimeStabilizer, and virtualized Fly-Phi664 under one fresh-process benchmark contract with explicit logical/materialized/RSS evidence and bounded Dense verification.

R10 adds `qsolqec-qec`: deterministic replayable generalized Weyl noise, typed error patterns and syndromes, a representation-independent decoder contract, an exact prime-d repetition-X decoder, and a bounded lookup candidate exhaustively compared over the declared correctable corpus. Decoder runtime has no dependency on Dense, Stabilizer, Fly-Phi664, or the memory-wall harness.

See:

- [docs/QUDIT_DENSE_ORACLE.md](docs/QUDIT_DENSE_ORACLE.md)
- [docs/QUDIT_OPERATIONS.md](docs/QUDIT_OPERATIONS.md)
- [docs/GLASS_BOX.md](docs/GLASS_BOX.md)
- [docs/PRIME_STABILIZER.md](docs/PRIME_STABILIZER.md)
- [docs/MEMORY_WALL_RUNTIME.md](docs/MEMORY_WALL_RUNTIME.md)
- [docs/STRUCTURED_STORAGE_CONTRACT.md](docs/STRUCTURED_STORAGE_CONTRACT.md)
- [docs/FLY_PHI664.md](docs/FLY_PHI664.md)
- [docs/FLY_PHI664_QDN.md](docs/FLY_PHI664_QDN.md)\n- [docs/FLY_PHI664_VIRTUALIZED.md](docs/FLY_PHI664_VIRTUALIZED.md)
- [docs/AMPLITUDE_MEMORY_WALL_CHALLENGE.md](docs/AMPLITUDE_MEMORY_WALL_CHALLENGE.md)
- [docs/NOISE_DECODER_MODULES.md](docs/NOISE_DECODER_MODULES.md)

## Historical inspiration

The project has conceptual ancestry in experiments such as [EmergentMonk/qecaudioqc64](https://github.com/EmergentMonk/qecaudioqc64), a fork of Davide Gessa's QC64 Commodore 64 quantum simulator. The original BASIC quantum simulator is Davide Gessa's work; the fork added SID-based audio and visual tracking. Its relevance here is observational: simulated state transitions were exposed through a secondary deterministic signal channel.

QSOLQEC does not treat that historical program as a mathematical oracle.

## Donor repositories

Existing QSOL repositories may contribute ideas or future modules, but they are not automatically part of the QSOLQEC core.

Likely module-level donors include:

- QSOLKCB/QEC - replay, evidence, decoder-governance concepts;
- QSOLKCB/SPECTRAL - spectral analysis experiments;
- QSOLKCB/SONIFICATION - observation-to-audio experiments;
- QSOLKCB/GALAXY - bounded tiling, logical-vs-resident scaling, and GPU compute experiments;
- QSOLKCB/OPT - reusable optimization contracts for adaptive storage, tiling, reuse, incremental materialization, and coordination;
- QSOLKCB/QSOL-MESH - distributed compute experiments;
- QSOLKCB/QSOL-GEO-REASON, GLUBALL, UFT-ID-3.0, QSOLAI and others - experimental modules only when a bounded experiment justifies them.

Import the useful contract or method, not an entire repository by default.

## Roadmap

The numbered roadmap is defined canonically in [ROADMAP.md](ROADMAP.md). The current sequence is:

1. **R0 - Modular research foundation** - complete in PR #1.
2. **R1 - Native qudit dense oracle** - complete in PR #2.
3. **R2 - Generalized qudit operations** - complete in PR #3.
4. **R3 - Glass Box** - complete in PR #4.
5. **R4 - First alternate representation** - complete in PR #5; exact prime-d stabilizer/tableau.
6. **R5 - Representation benchmark and memory-accounting harness** - implemented in PR #7.
7. **R6 - Structured logical-memory substrate contract** - implemented in PR #8.
8. **R7 - Fly-Phi664 structured memory prototype** - implemented in PR #9.
9. **R8 - Q(d,n) binding plus GALAXY/OPT virtualized materialization** - complete in PRs #10-#11.
10. **R9 - Amplitude-memory-wall challenge** - complete in PR #12.
11. **R10 - Noise and decoder modules** - implemented in PR #13 (pending merge).
12. **R11 - Flagship native-ququart experiment**.
13. **R12 - Sonification observer**.
14. **R13 - Compute acceleration**.
15. **R14 - Additional experimental representations**.
16. **R15 - AI observer**.
17. **R16 - Bridge candidate**.

If this summary and ROADMAP.md ever differ, **ROADMAP.md is the numbered source of truth**.

## Current status

**R10: replayable noise and decoder modules (PR #13, pending merge).**

The project now has a representation-independent QEC crate with deterministic seeded Weyl error traces, canonical error-pattern and syndrome identities, a decoder contract that consumes only typed syndrome data, an exact prime-d repetition-X decoder, and a separately built bounded lookup candidate compared exhaustively on the correctable corpus. No state-representation crate is available to the decoder runtime, so oracle rescue is structurally excluded. R11 native-ququart comparison is next after R10 merges.
