# R8B Exact Virtualized Materialization

R8B is the second ordered gate of the Fly-Phi664 Q(d,n) track.

It keeps the R8A state contract fixed and changes only how the exact encoded
state is stored, traversed, materialized, reused, and shared.

The separation remains:

```text
Fly-Phi664 storage substrate
        !=
R8A Q(d,n) encoding contract
        !=
R8B exact virtualized execution/materialization
```

R8B does not introduce a new quantum model, a new basis order, a new amplitude
codec, or an approximation.

## Preserved Gate-A contract

The virtualized path preserves:

- `SystemSpec = Q(d,n)`;
- subsystem-0 least-significant basis order;
- basis-index identity;
- `qsolqec.fly-phi664-qdn.complex64-be.v1` amplitude encoding;
- exact finite IEEE-754 Complex64 payload bits;
- positive-zero elision and signed-zero preservation;
- the exact semantic state-digest domain;
- serial norm-squared semantics;
- exact operation support for Weyl X/Z, Fourier, controlled shift, and swap;
- explicit unsupported status for local permutation and generic local unitary;
- atomic failure behavior;
- no Dense runtime fallback.

Dense remains a dev-test oracle only.

## Signed-zero boundary

Gate A made IEEE-754 bit identity observable.

That matters for optimization. An absent amplitude represents positive complex
zero. Weyl Z multiplies every amplitude by a phase. Even when the mathematical
value remains zero, IEEE-754 multiplication can produce a signed-zero bit
pattern.

Therefore:

- Weyl X, controlled shift, and swap may skip absent positive-zero amplitudes,
  because they are pure permutations of existing amplitude bit patterns;
- Weyl Z may not blanket-skip absent amplitudes, so R8B scans the logical Q(d,n)
  state through bounded worker-local tiles;
- Fourier may skip an entirely empty lane because the frozen DFT loop maps an
  all-positive-zero lane back to positive zero exactly; any lane containing a
  materialized value, including signed zero, is executed normally.

This distinction is deliberate evidence discipline, not an implementation
accident.

## OPT-SET-001: density-adaptive exact pages

R8B partitions the Q(d,n) basis range into configurable pages.

Each page is represented canonically as one of:

```text
Empty
Sparse
Bitmap
Dense
```

Selection depends on configurable local occupancy thresholds.

Sparse pages keep sorted `(offset, payload)` pairs. Bitmap pages store a
bitset plus compact payload sequence. Dense pages store a bitset plus a fixed
payload slot for every local offset.

All variants preserve exact membership, payload bits, and increasing logical
address order.

Thresholds and page spans are target configuration. The repository does not
treat any default value as portable performance authority.

## OPT-REDUCE-001: sound early working-set reduction

Permutation operations transform only currently materialized amplitudes.

The skipped set is exactly the implicit positive-zero set. Because a
permutation changes only address identity and does not perform arithmetic, this
reduction commutes with the operation exactly.

Weyl Z does not use this reduction because the signed-zero contract makes it
unsound.

## OPT-SOA-001 and GALAXY worker-local tiling

The executor creates a reusable worker-local scratch pool.

Each worker owns bounded structure-of-arrays fields for input and output real
and imaginary values.

Scratch is explicitly allocated and first-touched when the executor is created,
then reused across operation dispatches.

The deterministic scratch-capacity model is:

```text
workers * tile_span * 4 * sizeof(f64)
```

This is a working-set bound, not process RSS.

The current R8B dispatcher is serial. Worker count expresses reusable scratch
domains and future parallel partitioning. This PR makes no parallel speedup
claim.

## OPT-INV-001: named invariant reuse

R8B recognizes only bitwise-proven operation identities:

- Weyl X where `shift mod d == 0`;
- controlled shift where `shift mod d == 0`.

Those operations reuse the exact current state without rebuilding pages.

Weyl Z with zero reduced power is deliberately not treated as a bitwise
identity because complex multiplication can change signed-zero bits.

## OPT-INC-001: signature-bound incremental execution

Reusable operation results are keyed by a deterministic signature over:

- semantic input-state digest;
- source/macrograph identity;
- Phi664 geometry identity;
- encoder identity;
- virtualization configuration;
- canonical operation bytes.

Cached states are immutable page snapshots.

For an operation sequence, newly computed cache generations remain pending
until the complete requested sequence succeeds. A failed partial sequence does
not publish any newly computed generation as reusable authoritative state.

The live state is replaced only after complete success.

## OPT-FAN-001: shared immutable tile materialization

`SharedTileMaterializer` binds to one immutable virtualized state snapshot.

A logical tile is generated once and returned as an `Arc<MaterializedTile>`.
Subsequent consumers receive the same immutable tile object rather than
independently reconstructing identical amplitude vectors.

The materializer is state-snapshot-bound, so a tile cannot silently drift to a
new semantic state after validation.

## OPT-COAL-001: duplicate in-flight generation

Each tile has one `OnceLock` generation cell.

Equivalent requests for the same not-yet-generated tile share that cell, so
only one bounded generation is admitted.

The request contract includes:

- static owner identity;
- admission limit;
- cooperative cancellation before/after shared generation;
- deadline checks before/after shared generation;
- deterministic error results;
- retry after failed generation.

Cancellation or deadline expiry for one waiter does not corrupt an immutable
tile successfully generated for other consumers.

## OPT-CONT-001: static partitioned coordination domains

Tile ownership is deterministic:

```text
owner = tile_index mod owner_count
```

A request using the wrong owner fails closed.

R8B does not implement dynamic ownership reassignment, leases, or failover.
Therefore donor-repository fencing/epoch rules for replacement owners are not
claimed here. If dynamic reassignment is introduced later, fencing must be
added before it can become authoritative.

## OPT-POOL-001: persistent reusable scratch

Executor construction creates the complete configured scratch pool once.
Operation dispatches reuse those allocations.

The current implementation is a persistent scratch pool, not yet a persistent
parallel thread pool. Topology-aware parallel promotion requires separate
measurement and equivalence evidence.

## OPT-PRUNE-001: sound Fourier lane pruning

A Fourier lane is skipped only when it contains no materialized amplitude.

Under the frozen loop order, such a lane consists entirely of implicit positive
zero and maps exactly to positive zero.

No heuristic magnitude threshold or near-zero test is used.

## OPT-APPROX-001 remains disabled

R8B has no approximate path.

No tolerance, truncation, quantization, heuristic pruning, or probabilistic
replacement can silently weaken an exact caller.

Any future approximation requires a separate declared error contract and
validation boundary.

## Observation and storage accounting

`VirtualFlyQdnState` implements both the existing Glass Box
`ObservableState` contract and the R6 `ObservableStorage` contract.

The Glass Box snapshot keeps Gate-A semantic identity.

The storage snapshot reports:

- complete Phi664 logical address count;
- materialized amplitude count;
- exact 16-byte materialized payload bytes;
- deterministic representation-owned resident lower bound;
- source identity;
- geometry identity;
- exact storage declaration;
- virtual-page persistence identity;
- deterministic storage digest.

R8B uses global R6 `PhysicalBacking::Sparse` because unmaterialized logical
addresses remain absent. Per-page Sparse/Bitmap/Dense layout is reported
separately by the virtualized state.

## Evidence level and claim boundary

R8B remains E3 oracle-compared.

Tests require bitwise equality with the merged Gate-A adapter and Dense on
tractable fixtures, including:

- sparse permutation reduction;
- Weyl-Z signed-zero behavior;
- Fourier active-lane pruning;
- mixed operation sequences;
- adaptive page forms;
- committed cache reuse;
- invariant reuse;
- failed-sequence atomicity;
- shared immutable materialization;
- ownership, cancellation, and deadline guards;
- logical versus materialized storage accounting.

R8B does not claim:

- a universal memory reduction;
- a universal runtime improvement;
- a process-RSS improvement;
- parallel scaling;
- biological/neural semantics;
- approximation quality.

Those measurements belong to R9 under the common memory-wall benchmark
contract.
