# QSOLQEC Roadmap

QSOLQEC is an experimental compute laboratory for one central question:

> **How much quantum or qudit computation can be reproduced, compressed, decoded, or otherwise made tractable by deterministic classical computation before the representation hits the exponential wall?**

The roadmap is deliberately modular. A representation, decoder, observer, storage substrate, or compute backend may advance without forcing unrelated modules to move with it.

The project is permissive about experiments and strict about evidence:

> **Modules may be speculative. Experiments must declare what they actually demonstrate.**

## Roadmap discipline

Every new rung should preserve these rules:

1. **Dense remains the tractable small-system oracle.**
2. **Alternate representations do not silently fall back to dense.**
3. **Unsupported means unsupported.**
4. **Approximation must be explicitly declared and bounded.**
5. **Logical representation size and resident memory are different measurements.**
6. **Optimized paths remain subordinate to a reference or conformance contract.**
7. **A donor architecture contributes a mechanism, not automatic authority.**
8. **A result that fails, scales badly, or loses to a baseline is still valid evidence.**
9. **QSOLQEC does not promote directly into QEC.**
10. **QSOL-QEC-BRIDGE remains the only intended promotion/conformance boundary.**
11. **Every benchmark receipt must bind to the exact source revision used to build the executable, with a public locator that an independent reproducer can retrieve.**

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

---

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

---

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

---

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

---

## R3 - Glass Box

**Complete: PR #4**

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

---

## R4 - First alternate representation

**Complete: PR #5**

Initial alternate representation: exact prime-dimensional stabilizer/tableau.

- Add shared `Exact / Approximate / Unsupported` operation-support vocabulary.
- Support prime local dimensions, including d=2 and d=3.
- Deliberately reject composite d=4 in the first stabilizer formalism.
- Exact support for Weyl X/Z, Fourier, controlled shift, and SWAP.
- Explicitly reject arbitrary local permutation and local-unitary operations.
- Use the same Glass Box `ObservableState` contract as dense.
- Keep dense as a dev-test oracle dependency only.
- Compare final stabilizer generators against dense oracle states on tractable fixtures.
- Record O(n^2)-style tableau payload bytes against O(d^n) dense storage.
- Use checked sizing and fallible allocation for stabilizer storage.
- Mark the module E3 only after oracle-comparison tests pass.

R4 establishes the first concrete example of an exponentially large dense state space admitting a much smaller exact classical representation over a restricted mathematical domain.

---

# Next execution track

## R5 - Representation benchmark and memory-accounting harness

**Complete: PR #7**

R5 creates the measuring instrument required before more exotic representations are introduced.

Compare representations under the same declared workload using:

- wall-clock runtime;
- logical representation bytes;
- peak resident memory / RSS where measurable;
- working-set / resident tile bytes;
- allocation count or allocation events where measurable;
- materialization count;
- oracle agreement;
- operation support class;
- maximum feasible `n`;
- local dimension `d`;
- operation family;
- worker count;
- compute backend;
- host/environment identity.

Required distinctions:

```text
logical address space
!=
materialized representation
!=
resident working set
!=
process RSS
```

Initial comparison matrix:

```text
DenseState
vs
PrimeStabilizerState
```

R5 supplies deterministic benchmark specifications and machine-readable receipts through the `qsolqec-memorywall` runtime.

The initial runtime provides:

- `probe` host receipts;
- `run` single-point execution;
- `sweep` execution with one fresh child process per measured point;
- deterministic `clifford-ring-v1` workloads;
- logical-byte preflight limits before state construction;
- Linux current/peak RSS where available;
- separate construction, execution, snapshot, and oracle-verification timing;
- representation-independent workload identity;
- explicit exact/approximate/unsupported operation-support class;
- build-time source SHA binding plus public GitHub commit locator;
- separate host identity;
- optional NVIDIA inventory without implying GPU execution;
- Dense self-reference and tractable Stabilizer-vs-Dense oracle agreement.

Allocation count and representation-specific resident tile bytes remain explicitly unavailable (`null`) until later observers/materialized representations provide those measurements. Missing measurements are not inferred.

Benchmark observations are not correctness proofs.

No future representation should claim a memory or performance advantage outside this measurement contract or a later explicitly versioned successor.

---

# Memory-wall research track

## R6 - Structured logical-memory substrate contract

**Complete: PR #8**

R6 defines a representation-independent contract for very large logical address spaces whose complete contents do not need to be resident simultaneously.

The primary distinction is:

> **Logical scale does not imply resident-memory scale.**

Required concepts:

- stable logical address identity;
- deterministic address packing/encoding;
- immutable geometry identity;
- payload identity separate from address identity;
- bounded materialization windows;
- sparse and dense physical backing choices;
- deterministic tile ownership;
- explicit persistence/version boundaries;
- exact versus approximate storage declaration;
- failure on address-space exhaustion or unsupported allocation rather than silent wrap/reuse;
- a **typed storage-observation contract** distinct from quantum-state observation.

The storage-observation contract should describe storage facts such as:

```text
storage geometry identity
macrograph/source identity
logical address count
materialized address/page count
materialized payload bytes
resident tile/working-set bytes
storage digest
exact/approximate storage declaration
```

It must not require a `SystemSpec`, quantum norm, fidelity, or other state semantics that a neutral storage substrate does not yet possess.

The existing Glass Box `ObservableState` contract remains reserved for representations that have an explicit Q(d,n) state interpretation.

Candidate logical address form:

```text
(macro_node, fibre_id, x, y, z)
```

R6 is a storage contract. It is not yet a connectome-specific implementation.

The R6 implementation supplies:

- the `qsolqec-storage` contract crate;
- stable geometry and source identities derived from canonical bytes, with logical namespace size bound into geometry identity;
- geometry-bound packed logical addresses with fail-closed exhaustion checks;
- checked mixed-radix helpers that reject an unrepresentable full radix namespace before packing any coordinate;
- bounded materialization windows that reject arithmetic wrap and range overflow;
- explicit sparse/dense physical backing declarations;
- deterministic tile ownership from tile span and owner count;
- explicit persistence format/version boundaries;
- exact versus named/contract-bounded approximate storage declarations;
- payload identities independent of logical address identity;
- a typed `ObservableStorage` / `StorageSnapshot` contract;
- deterministic storage-observation artifact identity under schema `qsolqec.storage.observation.v1`, with canonical lowercase SHA-256 digest input;
- deterministic fixtures for round-tripping, exhaustion, materialization bounds, ownership, payload/address separation, and observation identity.

The contract deliberately contains no `SystemSpec`, norm, fidelity, or Glass Box `ObservableState` requirement. Those remain reserved for a later Q(d,n)-bound adapter.

See `docs/STRUCTURED_STORAGE_CONTRACT.md`.

---

## R7 - Fly-Phi664 structured memory prototype

**Implemented in PR #9 (pending merge)**

R7 introduces the first flagship structured-memory candidate for the amplitude-memory problem.

### Macro topology

Use a frozen, provenance-bound MaleCNS/FlyWire-derived graph as a **macro-address topology**.

The connectome is used as a structural donor:

```text
connectome
!=
effectome
!=
neural dynamics
!=
biological cognition
```

QSOLQEC does **not** claim that biological neurons contain these fibres, that the connectome is a digital brain, or that this storage architecture models fruit-fly cognition.

The source dataset version, node identities, edge interpretation, and any graph projection must be explicitly pinned before experimental evidence is accepted.

### Local fibre bundle

Each macro node receives three independent finite storage fibres:

```text
F27   = 3 x 3 x 3   = 27 positions
N125  = 5 x 5 x 5   = 125 positions
R512  = 8 x 8 x 8   = 512 positions
```

Define:

```text
Phi664 = F27 disjoint-union N125 disjoint-union R512
```

with:

```text
27 + 125 + 512 = 664 logical positions per macro node
```

The fibres are **independent address spaces**, not a Cartesian multiplication of 27 x 125 x 512.

### Fibre intent

- **F27** - ternary / Sierpinski-inspired local address geometry descended conceptually from ETQ fibre work.
- **N125** - neutral-atom-inspired 5x5x5 cubic address geometry; storage only, not a claim of neutral-atom hardware or quantum dynamics.
- **R512** - 8x8x8 cube address geometry; storage first, with reversible permutation experiments allowed later as a separate operation contract.

The base fibres are neutral storage resources. Their payload semantics are not permanently assigned in advance.

### Planning scale

Using the current MaleCNS planning node count of 166,691 macro nodes:

```text
166,691 x 664 = 110,682,824
```

nominal logical fibre addresses are available.

This number describes the logical namespace only. It is **not** a requirement to allocate 110,682,824 resident objects.

The pinned source manifest, not this planning estimate, becomes authoritative when the prototype is implemented.

The R7 implementation therefore does **not** use 166,691 as an allocation bound. It accepts the exact `bodyId` set selected from the pinned MaleCNS v1.0 release, sorts it canonically, rejects duplicates, hashes the complete ID set, and makes that digest plus the versioned source/projection rules authoritative for source and geometry identity.

### Candidate representation

Possible crate/module direction:

```text
qsolqec-structured-memory
or
qsolqec-fly-phi664
```

A first implementation should be able to:

- map a canonical logical address to a stable packed identifier;
- round-trip packed IDs back to structured coordinates;
- validate fibre bounds;
- bind every address to the pinned macrograph identity;
- expose logical size separately from resident/materialized size;
- materialize bounded regions without constructing the full logical store;
- emit the R6 typed storage snapshot/artifact without pretending the neutral store is already a Q(d,n) state representation.

At R7, Fly-Phi664 is therefore a **storage substrate**, not yet a quantum-state representation. It must not implement `ObservableState` merely to obtain Glass Box compatibility.

The R7 implementation supplies:

- `qsolqec-fly-phi664` as the first concrete R6 substrate;
- a MaleCNS v1.0 source descriptor with versioned node and edge exports;
- real source `bodyId` values as the public macro-node identity;
- canonical ascending-`bodyId` rank only for packed-address calculation;
- an exact digest over the complete supplied macro-node ID set;
- source identity that changes when macro-node membership changes;
- F27, N125, and R512 as independent 3x3x3, 5x5x5, and 8x8x8 fibres;
- stable disjoint-union offsets 0, 27, and 152;
- checked structured-address ↔ packed-address round trips;
- unknown-node, fibre-bound, geometry-crossing, and address-space failure behavior;
- exact sparse opaque-byte payload storage;
- bounded materialization windows that may cross fibre boundaries without constructing the full namespace;
- deterministic logical/materialized/tracked-resident accounting;
- R6 `ObservableStorage` snapshots and storage-artifact identity;
- a tiny real-ID provenance fixture that is explicitly not a whole-connectome evidence claim.

The edge export and its directed `body_pre -> body_post` interpretation are provenance-bound in R7, but biological adjacency is not materialized or executed by this storage prototype.

See `docs/FLY_PHI664.md`.

---

## R8 - Q(d,n) binding plus GALAXY/OPT virtualized materialization

R8 has two ordered gates. The storage substrate must pass the state-binding gate before it can participate in QSOLQEC representation/oracle comparisons, and the baseline bound representation must exist before optimization mechanisms can claim parity.

### Gate A - explicit Q(d,n) encoding and operation contract

**Implemented in the PR #10 (pending merge).**

Define a representation adapter that binds neutral Fly-Phi664 storage to a declared quantum/qudit model.

The adapter contract must state, at minimum:

- the `SystemSpec = Q(d,n)` it represents;
- the frozen basis-order convention it uses;
- the payload codec from Q(d,n) state information into logical fibre addresses;
- whether the codec is exact or approximate;
- the inverse/reconstruction contract where reconstruction is claimed;
- which observables may be decoded without full reconstruction;
- the supported `Operation` subset and how each operation transforms the encoded representation;
- the explicit `Exact / Approximate / Unsupported` support class;
- the semantic state digest domain;
- norm semantics, including whether norm is reconstructed, derived, or unavailable;
- comparison/fidelity semantics against Dense where the oracle is tractable;
- failure behavior when required state information cannot be represented or recovered.

Only this **Q(d,n)-bound adapter** may implement the existing Glass Box `ObservableState` contract.

The underlying Fly-Phi664 store remains representation-neutral and continues to expose only the R6 storage-observation contract.

This preserves the separation:

```text
Fly-Phi664 storage substrate
        !=
Q(d,n) encoding adapter
        !=
quantum-state semantics
```

R9 may compare Fly-Phi664 against Dense/Stabilizer only after this adapter exists and deterministic tractable fixtures demonstrate that the declared workload can be encoded, operated on, and compared under the same experiment contract.

The R8A implementation supplies:

- `qsolqec-fly-qdn` as the explicit state-binding adapter;
- exact basis-index -> same-numbered Phi664 packed-address mapping;
- a 16-byte big-endian IEEE-754 Complex64 payload codec;
- exact reconstruction with positive-zero elision and signed-zero preservation;
- fail-closed rejection when `d^n` exceeds the supplied Phi664 namespace;
- exact support for Weyl X/Z, Fourier, controlled shift, and swap;
- explicit unsupported status for local permutation and generic local unitary;
- atomic validated operation sequences with no Dense runtime fallback;
- a semantic state digest domain separated from storage source/geometry identity;
- serial norm-squared and explicit amplitude-comparison semantics;
- Glass Box `ObservableState` only at the bound-adapter layer;
- Dense as a dev-test oracle only, reaching E3 oracle-compared maturity;
- documentation in `docs/FLY_PHI664_QDN.md`.

This is deliberately a full-reconstruction baseline. It does not claim a memory-wall advantage. Gate B must preserve the Gate-A semantics while reducing materialization.

### Gate B - GALAXY/OPT virtualized materialization

After a baseline bound adapter exists, apply previously extracted optimization contracts to make Fly-Phi664 sparse, hierarchical, and incrementally materialized rather than a giant flat allocation.

Every optimized path must preserve the Gate-A representation contract or explicitly declare a bounded approximation.

The initial donor mechanisms are:

### OPT-SET-001 - density-adaptive compact representation

Choose physical storage locally according to occupancy:

```text
empty
-> sparse
-> compact bitmap/set
-> dense local page
```

while preserving the same logical address semantics.

Sparse-to-dense and dense-to-sparse transitions must preserve exact membership/payload semantics and version compatibility.

### OPT-REDUCE-001 - early working-set reduction

Determine which logical regions can affect the current operation before expensive materialization.

Do not materialize irrelevant fibres merely because they exist in the logical namespace.

Any reduction that crosses an operation stage must preserve the declared semantics; heuristic irrelevance is not proof of irrelevance.

### OPT-SOA-001 - worker-local SoA tiling

Adopt the GALAXY-derived rule:

> **Working storage should scale with workers x tile capacity, not with total logical population.**

Materialize bounded worker-local tiles containing only hot fields.

Reuse tile scratch rather than converting the complete logical representation.

### OPT-INV-001 - invariant-driven reuse

If a named invariant proves that a previously materialized region is unchanged by the current effective operation/state transition, reuse the exact prior result.

Similarity is insufficient; reuse requires a tested/proven equivalence predicate.

### OPT-INC-001 - signature-bound incremental execution

Bind each reusable materialization to the complete effective identity required to reproduce it, potentially including:

- source state identity;
- operation-sequence identity;
- encoder version;
- macrograph/source dataset identity;
- fibre geometry identity;
- tile/region identity;
- relevant configuration.

Changed identity invalidates the materialization.

Failed or partial work must never become reusable authoritative state.

### OPT-FAN-001 - shared materialization fan-out

Materialize a deterministic tile/region once and allow multiple read-only consumers to reuse the exact validated artifact:

```text
executor
observer
benchmark
decoder
sonifier
```

must not independently reconstruct identical materializations when one immutable/versioned instance can be shared safely.

### OPT-COAL-001 - duplicate in-flight work coalescing

Equivalent simultaneous requests for the same not-yet-materialized region may share one bounded generation, with explicit cancellation, ownership, error, deadline, and admission semantics.

### OPT-CONT-001 - partitioned coordination domains

Partition ownership by stable macrograph/fibre regions rather than coordinating all storage through one global hotspot.

Preserve global identity and ownership invariants. Finite packed address spaces must fail closed on exhaustion rather than wrap or reuse live identities.

### OPT-POOL-001 - persistent topology-aware worker pools

Reuse workers, tile buffers, indexes, and scratch across repeated dispatches.

Worker count, topology policy, and tile size remain environment-specific measurements rather than portable constants.

### OPT-APPROX-001 - contract-bounded approximation

Approximation is a later opt-in path only.

If a fibre or payload representation becomes approximate, it must declare:

- error/degradation metric;
- bound or statistical contract;
- composition horizon;
- validation method;
- exact/reference fallback where practical.

Exact callers must never be silently weakened.

### OPT-PRUNE-001 - sound bound-driven pruning

A logical graph/fibre region may be skipped only when a sound bound proves it cannot affect the declared result.

A heuristic may prioritize work, but it may not masquerade as an exact pruning proof.

All OPT mechanisms are imported as **patterns and contracts**, not universal parameter values. Thresholds, tile sizes, worker caps, cache keys, and budgets must be re-measured in QSOLQEC.

---

## R9 - Amplitude-memory-wall challenge

R9 turns the storage work into a bounded scientific experiment.

Compare:

```text
DenseState
PrimeStabilizerState
Fly-Phi664 / structured-memory candidate
and later additional representations
```

under common R5 benchmark contracts.

Primary question:

> **Can a structured logical representation retain the information required by a declared quantum/qudit workload while using substantially less resident memory than the explicit dense amplitude vector?**

Required measurements:

- logical namespace size;
- actual persisted/materialized payload size;
- peak resident tile/working-set bytes;
- **peak process RSS**, including indexes, caches, allocator overhead, page maps, and other representation-owned resident structures where the platform exposes them;
- number of materialized addresses/tiles;
- number of reused versus recomputed regions;
- operation runtime;
- fidelity/agreement where a dense oracle exists;
- exact/approximate/unsupported classification;
- failure point as `n` grows.

The experiment must not count unmaterialized logical addresses as resident-memory savings unless the required information remains reproducible under the declared contract.

A candidate also cannot claim a resident-memory win merely because its active tiles are small: peak process RSS must remain part of the comparison so occupancy indexes, caches, allocator overhead, page maps, and other resident structures cannot hide outside the working-set metric.

### Beyond the dense-oracle limit

When dense execution becomes infeasible, the candidate does not become correct by default.

Evidence beyond the dense limit may use independently testable constraints such as:

- reversible identities;
- known algebraic invariants;
- analytically solvable circuits;
- exact stabilizer-domain cases;
- projected tractable subsystems;
- metamorphic tests;
- cross-representation agreement.

Claims must narrow as oracle coverage disappears.

A negative result is valid: if Fly-Phi664 merely relocates exponential information, or runtime/precision explodes instead of memory, record that result.

---

# Quantum error-correction and qudit track

## R10 - Noise and decoder modules

- Replayable error/noise specifications.
- Syndrome data kind.
- Decoder module contract.
- Exact decoder where tractable.
- Candidate decoder comparisons.
- No oracle rescue unless explicitly part of the candidate algorithm.
- Permit structured-memory representations as decoder inputs only under explicit contracts.

---

## R11 - Flagship native-ququart experiment

Compare:

```text
native Q(4,n)
vs
packed Q(2,2n)
```

under explicitly equivalent workloads where equivalence can be defined.

Measure:

- runtime;
- logical representation size;
- resident memory;
- representation growth;
- numerical agreement;
- operation locality/cost;
- decoder behavior.

Do not conflate Z4 generalized Weyl arithmetic with GF(4) stabilizer formalisms.

---

# Observation and compute track

## R12 - Sonification observer

- Deterministic event-to-audio mapping.
- No correction authority.
- Preserve provenance to the source event.
- Optionally test whether audio improves human anomaly detection versus logs alone.

Historical QC64 audio work may inform interface ideas, with original authorship preserved.

---

## R13 - Compute acceleration

Potential backends:

- scalar reference;
- threaded CPU;
- SIMD;
- GPU;
- multi-GPU;
- QSOL-MESH distributed compute.

Compute acceleration does not define mathematical truth.

Every optimized backend remains subordinate to a declared numerical contract and a reference/conformance path.

GALAXY-derived worker-local tiling and persistent-worker patterns may be reused where measured evidence supports them.

---

# Broader experimental track

## R14 - Additional experimental representations

Potential candidates include:

- sparse amplitudes;
- tensor networks / MPS;
- decision diagrams;
- Pauli/Weyl expansions;
- spectral representations;
- low-rank models;
- learned compression;
- graph-indexed stores other than MaleCNS;
- alternate finite fibre bundles;
- hybrid exact/approximate representations;
- new QSOL representations.

Every candidate should use the R5 benchmark contract and declare whether its gain comes from:

```text
mathematical restriction
compression
sparsity
lazy materialization
reuse
approximation
compute/memory trade
or a combination
```

Failure to outperform a baseline is a valid result.

---

## R15 - AI observer

Only after deterministic experiment evidence exists:

- receipt interpretation;
- anomaly classification;
- hypothesis suggestion;
- strategy recommendation;
- representation-selection suggestions.

Compare AI, deterministic heuristics, and human observers on the same hidden corpus.

AI has no truth, oracle, decoder, or promotion authority.

---

## R16 - Bridge candidate

A mature module may be exported to QSOL-QEC-BRIDGE only when there is a bounded integration reason.

Before export, the candidate should have:

- stable source identity;
- explicit claim boundary;
- deterministic fixtures;
- reproducible receipts;
- supported/unsupported domain definition;
- approximation declaration where applicable;
- independent reproduction evidence appropriate to maturity;
- a reason QEC would benefit from the integration.

The bridge, not QSOLQEC, owns translation and conformance with QEC.

Bridge acceptance does not imply QEC acceptance.

---

# Donor lineage for the memory-wall track

The structured-memory experiment deliberately combines mechanisms from several prior programs without merging their histories or authorities:

```text
ETQ
  finite fibres / structured local state
          |
          v
MaleCNS/FlyWire
  frozen complex macrograph
          |
          v
GALAXY
  huge logical population
  != bounded resident population
          |
          v
OPT
  adaptive representation
  bounded tiling
  reuse
  incremental materialization
  shared artifacts
  partitioned coordination
          |
          v
Fly-Phi664
  experimental structured memory candidate
```

This donor lineage is conceptual and architectural.

The source graph, fibre geometry, optimization contracts, encoding, payload, and experimental evidence remain separately identified.

## Claim boundary for Fly-Phi664

Fly-Phi664 is intended to test a storage hypothesis:

> **Can a large provenance-bound graph carrying finite structured fibres provide a useful logical memory substrate for quantum/qudit representations while keeping resident working memory bounded?**

It does **not** claim:

- that fruit-fly neurons naturally implement F27, N125, or R512;
- that the connectome itself is memory;
- that the architecture simulates biological cognition;
- that Sierpinski, neutral-atom-inspired, or cube geometry is intrinsically quantum;
- that a large logical namespace creates physical storage capacity for free;
- that arbitrary quantum states can be losslessly compressed into polynomial memory;
- that the amplitude wall has been solved merely because logical addresses are sparse or lazily materialized.

The experiment succeeds only to the extent that measurable workload behavior survives under explicit memory, correctness, and approximation contracts.
