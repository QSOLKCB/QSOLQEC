# R7 Fly-Phi664 Structured Memory Prototype

R7 is the first concrete implementation of the R6 structured logical-storage contract.

It is deliberately **not** a quantum-state representation.

## Source boundary

The built-in source descriptor is pinned to the public MaleCNS v1.0 release:

- dataset: `male-cns:v1.0`;
- release date: 2026-06-08;
- node identity field: `bodyId`;
- node source: the versioned v1.0 body-annotation Feather export;
- edge source: the versioned v1.0 connection-weight Feather export;
- edge interpretation: directed `body_pre -> body_post`, with published minconf-0.5 synapse count as weight;
- licence: CC-BY.

R7 does not copy the multi-gigabyte connectome tables into QSOLQEC.

Instead, callers supply the exact set of `bodyId` values selected from the pinned release. The prototype sorts those IDs in ascending order, rejects duplicates, hashes the exact canonical ID set, and folds that digest into the R6 `StorageSourceIdentity`.

Therefore:

```text
MaleCNS release identity
+ exact bodyId set
+ projection rules
=
R7 macrograph source identity
```

The historical roadmap value of 166,691 neurons remains planning context only. It is not used as an allocation bound or silently treated as the node count of an arbitrary export. The supplied and hashed node set is authoritative for each R7 store.

## Macrograph use

MaleCNS contributes a **macro-address topology identity**.

R7 pins the edge source and its interpretation but does not materialize or execute biological adjacency. No claim is made that Fly-Phi664 simulates fly neural dynamics, an effectome, cognition, or biological memory.

The macro node exposed by the address API is the real source `bodyId`.

Internally, body IDs are sorted ascending and assigned a deterministic rank solely for packed-address calculation.

## Phi664 fibre geometry

Every macro node owns three independent finite fibres:

```text
F27   = 3 x 3 x 3 = 27
N125  = 5 x 5 x 5 = 125
R512  = 8 x 8 x 8 = 512

Phi664 = F27 disjoint-union N125 disjoint-union R512

27 + 125 + 512 = 664
```

These are disjoint namespaces, not a Cartesian product.

Offsets inside one macro node are frozen as:

```text
F27   0 .. 26
N125 27 .. 151
R512 152 .. 663
```

Within each fibre, local coordinates use x-major ordering:

```text
local = x * side^2 + y * side + z
```

The complete packed address is:

```text
packed = macro_rank * 664 + fibre_offset + local
```

Every arithmetic step is checked through the R6 geometry/address contract.

## Geometry identity

The R7 geometry identity binds:

- exact R6 source identity;
- exact macro-node count;
- ascending-`bodyId` rank convention;
- F27/N125/R512 dimensions;
- disjoint-union offsets;
- local x/y/z ordering.

Changing the node set therefore changes both source identity and geometry identity.

An address constructed for one geometry cannot be packed by another geometry.

## Sparse physical store

`Phi664Store` is initially sparse.

Payload semantics are deliberately opaque byte strings. R7 does not assign F27, N125, or R512 permanent quantum meanings.

The store exposes:

- exact payload identity independent of address identity;
- write/read/remove by structured address;
- bounded materialization windows;
- R6 `ObservableStorage` snapshots;
- deterministic storage digest.

Unwritten logical addresses consume no payload entry.

## Bounded materialization

The store is created with an explicit maximum materialization-window length.

A materialization request is rejected before vector allocation if it exceeds that limit.

A window may cross fibre or macro-node boundaries because the packed namespace is contiguous and the decoder is deterministic.

This permits experiments over bounded regions without constructing the complete logical namespace.

## Storage accounting

The R7 snapshot keeps the R6 distinctions intact:

```text
logical address count
!=
materialized address count
!=
materialized payload bytes
!=
tracked resident bytes
!=
process RSS
```

The initial tracked resident metric is a deterministic lower bound:

```text
8 bytes * number of frozen body IDs
+
materialized payload bytes
```

It excludes allocator and tree overhead.

It must not be substituted for process RSS. R5 remains responsible for process-level measurement.

## Claim boundary

R7 does not implement:

- `SystemSpec = Q(d,n)`;
- basis-state or amplitude semantics;
- norm or fidelity;
- quantum operations;
- Glass Box `ObservableState`;
- connectome dynamics;
- neural activation;
- biological memory semantics.

Those remain outside R7.

R8 is the first roadmap rung permitted to define a Q(d,n)-bound adapter.

## Deterministic evidence

Tests cover:

- the 27 + 125 + 512 disjoint sum;
- source-version pinning;
- order-independent body-ID canonicalization;
- duplicate body-ID rejection;
- source/geometry identity changes when node membership changes;
- exact fibre-boundary pack/unpack round trips;
- unknown-node and coordinate rejection;
- geometry-crossing rejection;
- bounded materialization across a fibre boundary;
- sparse logical-vs-materialized accounting;
- payload/address identity separation;
- storage-observation identity changes and restoration after exact payload removal.
