# R6 Structured Logical-Storage Contract

R6 defines the neutral storage boundary that the later Fly-Phi664 experiment must implement before any Q(d,n) binding is attempted.

The contract lives in `qsolqec-storage`.

## Claim boundary

The storage layer is not a quantum-state representation.

It therefore does not require or expose:

- `SystemSpec = Q(d,n)`;
- basis ordering;
- norm or fidelity;
- quantum operations;
- Glass Box `ObservableState`;
- dense-oracle comparison.

Those semantics belong to a later adapter once a concrete storage substrate is explicitly bound to a quantum/qudit representation.

## Stable identity

R6 separates three identities:

```text
geometry identity
!=
logical address identity
!=
payload identity
```

`StorageGeometryIdentity` hashes a named/versioned canonical geometry description **together with its total logical address count**. Namespace size is therefore part of immutable geometry identity rather than a caller-supplied parameter.

`PackedAddress` binds a checked numeric logical index to that geometry digest. The same numeric index under a different geometry is therefore a different address.

`PayloadIdentity` hashes named/versioned payload bytes independently of the address at which those bytes happen to be materialized.

Source/macrograph provenance is represented separately by `StorageSourceIdentity`.

## Deterministic packing

Geometry implementations use the `LogicalAddressCodec` trait.

They must provide:

- immutable geometry identity that already binds the total logical address count;
- deterministic structured-address to packed-address conversion;
- deterministic packed-address to structured-address conversion;
- checked failure rather than wrap or live-address reuse.

R6 provides checked mixed-radix helpers for simple geometries. Axis 0 is explicitly the most-significant axis. Packing first validates that the **entire radix product** is representable, so no coordinate-dependent prefix of an overflowing namespace can be accepted. Fly-Phi664 is free to implement a different codec because its fibre geometry is not required to be a single rectangular radix product.

## Materialization

`MaterializationWindow` describes a bounded contiguous packed-address region.

Construction rejects:

- zero-length windows;
- starts outside the declared logical namespace;
- end arithmetic overflow;
- windows extending past the logical address count.

A large logical namespace therefore does not imply a large resident allocation.

## Physical backing

The initial backing declaration is intentionally small:

```text
Sparse
Dense
```

This describes the physical storage choice, not the logical namespace.

Later adaptive/hybrid policies may extend the contract only when an implementation needs them.

## Deterministic ownership

`TileOwnershipPolicy` freezes a simple deterministic partition rule:

```text
tile_index = packed_index / tile_span
owner      = tile_index mod owner_count
```

The rule is checked for nonzero tile span and owner count. It establishes an unambiguous ownership primitive without claiming any specific worker count is optimal.

## Persistence boundary

`PersistenceBoundary` carries a named format and nonzero version.

Persistence version is part of storage-observation artifact identity. A format/version change therefore cannot silently masquerade as the same deterministic storage artifact.

## Exact versus approximate storage

`StorageExactness` is either:

- `Exact`; or
- `Approximate { method, error_contract }`.

Approximate storage must name both the method and its declared error contract. R6 does not prescribe a particular metric.

## Typed storage observation

`ObservableStorage` is deliberately distinct from Glass Box `ObservableState`.

Its `StorageSnapshot` reports:

- geometry identity;
- source/macrograph identity;
- logical address count;
- materialized address count;
- materialized payload bytes;
- resident storage working-set bytes;
- sparse/dense backing;
- exact/approximate declaration;
- persistence boundary;
- deterministic storage digest.

`observe_storage` emits schema:

```text
qsolqec.storage.observation.v1
```

and a deterministic SHA-256 artifact identity over those semantic fields. Externally supplied storage digests must use canonical lowercase `sha256:` hexadecimal form; alternate hex casing is rejected rather than producing a second artifact identity for the same digest value.

The observation contains no wall-clock value and no quantum-state fields.

## Relationship to R5

R5 measures process-level benchmark facts such as RSS and timings.

R6 describes representation-owned storage facts.

The distinction remains:

```text
logical address space
!=
materialized payload
!=
resident storage working set
!=
process RSS
```

R7 may feed R6 storage facts into later R5-style benchmark receipts, but one metric must not be substituted for another.

## Next rung

R7 implements Fly-Phi664 as the first concrete structured-storage candidate using this contract.

At R7 it remains a storage substrate only. It must not implement Glass Box `ObservableState` merely to participate in quantum-state tooling.
