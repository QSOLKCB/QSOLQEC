# R8A Fly-Phi664 Q(d,n) Binding

R8A is the state-binding gate for the Fly-Phi664 research track.

It introduces qsolqec-fly-qdn, the first component allowed to interpret the
neutral R7 storage substrate as an explicit Q(d,n) quantum/qudit state.

The separation remains strict:

    Fly-Phi664 storage substrate
            !=
    Q(d,n) encoding adapter
            !=
    optimized virtualized materialization

The underlying qsolqec-fly-phi664 crate remains representation-neutral and
continues to expose the R6 storage-observation contract only.

## Gate-A baseline

R8A is intentionally a correctness-first baseline rather than a memory-wall
result.

System geometry is the existing SystemSpec = Q(d,n), using the frozen basis
order in qsolqec-core:

    subsystem 0 is the least-significant base-d digit

For a basis index i in 0..d^n, the adapter maps:

    basis index i -> Phi664 packed logical address i

The mapping therefore follows the already-frozen Phi664 packed ordering across
F27, N125, R512, then successive ascending-bodyId macro nodes.

The adapter rejects a Q(d,n) state before encoding if d^n exceeds the exact
logical address count exposed by the supplied Phi664 geometry.

## Exact amplitude payload codec

Encoding ID:

    qsolqec.fly-phi664-qdn.complex64-be.v1

Each amplitude is represented by the exact IEEE-754 bits of:

    real f64 || imaginary f64

using big-endian byte order for a 16-byte payload.

Positive complex zero (+0.0,+0.0) is implicit and may be absent from the sparse
store. Every other finite bit pattern is materialized, including signed zero,
so reconstruction does not silently canonicalize IEEE-754 state.

Non-finite input amplitudes are rejected, matching the existing DenseState
construction boundary.

The codec is exact. R8A declares no approximation.

## Reconstruction contract

reconstruct() returns the complete d^n Complex64 vector in the frozen QSOLQEC
basis order.

An absent payload within the Q(d,n) basis range decodes as positive complex
zero.

A present payload must be exactly 16 bytes and decode to finite f64
components. Malformed or non-finite stored payloads fail closed.

Full reconstruction allocates d^n Complex64 values. This is deliberate in
Gate A. It establishes the reference representation contract that Gate B must
preserve while removing unnecessary materialization.

## Operation contract

R8A validates every Operation against SystemSpec before mutation.

Exact support:

- Weyl X
- Weyl Z
- Fourier
- controlled shift
- swap

Unsupported:

- local permutation
- generic local unitary

The supported subset matches the existing prime-stabilizer Clifford-style
surface used by the current benchmark track.

Execution does not call DenseState. The adapter reconstructs its own amplitude
vector, executes the frozen R2 scalar semantics locally, and re-encodes only
after the entire validated operation sequence succeeds.

An unsupported or structurally invalid operation leaves the encoded state
unchanged.

## Glass Box state semantics

Only the Q(d,n)-bound adapter implements ObservableState.

Representation ID:

    fly-phi664-qdn-baseline

The Glass Box state snapshot declares:

- the bound SystemSpec;
- exact approximation status;
- a semantic state digest;
- serial norm-squared;
- logical Q(d,n) state bytes as 16 * d^n.

The semantic state digest domain is:

    qsolqec.fly-phi664-qdn.semantic-state.v1

It binds the SystemSpec, frozen basis-order marker, encoding ID, and exact
ordered amplitude bits.

It deliberately does not bind the MaleCNS bodyId set or Phi664 geometry
identity. Those remain storage facts in the independent R6 StorageSnapshot.
The same Q(d,n) state encoded onto two different valid Phi664 macrographs has
the same semantic state digest but different storage geometry/source identity.

## Norm and oracle comparison

Norm-squared is derived by the same fixed serial fold over the exact amplitude
vector used during encoding.

For tractable oracle fixtures, compare_amplitudes() reconstructs the candidate
and reports:

- exact IEEE-754 bit equality;
- maximum componentwise complex absolute error;
- whether that error is within a caller-declared finite non-negative tolerance.

Dense is a dev-test oracle only. qsolqec-fly-qdn has no runtime dependency on
qsolqec-dense.

## Storage accounting

R8A keeps two observation domains separate.

The quantum-state snapshot reports logical Q(d,n) state bytes:

    16 * d^n

The underlying Phi664 StorageSnapshot continues to report:

- complete logical Phi664 address count;
- materialized sparse address count;
- materialized payload bytes;
- tracked resident lower bound;
- source and geometry identity;
- storage digest.

A sparse basis state may therefore materialize one 16-byte payload while still
having a logical Q(d,n) state size of 16 * d^n. This is not yet a memory-saving
claim.

## Failure boundaries

R8A fails rather than silently changing semantics when:

- d^n overflows the platform address space;
- the supplied amplitude count does not equal d^n;
- an amplitude is non-finite;
- d^n exceeds available Phi664 logical addresses;
- reconstruction allocation fails;
- a stored amplitude payload is malformed;
- an operation is invalid or unsupported;
- index arithmetic overflows;
- the underlying R6/R7 storage contract rejects an address or payload action.

## Evidence level

The adapter is marked E3 oracle-compared because deterministic tractable tests
compare its supported operation sequence against DenseState.

Tests also cover:

- signed-zero-preserving exact reconstruction;
- sparse positive-zero elision;
- basis-index mapping across F27/N125/R512 boundaries;
- insufficient Phi664 namespace rejection;
- semantic-state/storage-identity separation;
- unsupported-operation atomicity;
- Glass Box observation of the bound state.

## Gate B next

R8 Gate B may now optimize materialization, but it must preserve this Gate-A
contract or explicitly declare a bounded approximation.

The first optimization work should target the existing R8 mechanisms in order,
starting with density-adaptive physical storage and early working-set
reduction, while keeping Dense oracle comparison on tractable fixtures.
