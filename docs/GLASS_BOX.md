# Glass Box Observation Boundary

## Purpose

R3 introduces the first executable Glass Box around QSOLQEC kernels.

The Glass Box observes an operation before and after execution. It does not live inside the mathematical kernel and it does not decide whether the kernel result is correct.

```text
state
  |
  v
Glass Box BEFORE event
  |
  v
existing operation kernel
  |
  v
Glass Box AFTER event
  |
  v
observation receipt
```

## Representation-neutral observation

The `ObservableState` trait exposes only an observation snapshot:

- representation ID and version;
- `Q(d,n)` system geometry;
- approximation declaration;
- semantic state digest;
- norm squared;
- logical representation bytes.

The Glass Box does not need access to internal amplitude, tensor, tableau, GPU, or later representation storage.

R3 wires the dense oracle into this trait first.

## Dense state digest

The dense representation computes its semantic state digest from:

- schema identity;
- local dimension;
- subsystem count;
- frozen basis order;
- every complex amplitude as IEEE-754 bit patterns.

The digest is SHA-256 and is fed incrementally. Observation does not build a second amplitude-sized canonical buffer, so enabling the Glass Box does not duplicate the dense state merely to hash it.

This digest identifies the represented dense state. It does not claim cryptographic proof of physical quantum state.

## Pre/post events

Every observed operation produces two typed events:

1. `Before` — state snapshot before execution.
2. `After` — state snapshot after execution, elapsed time, outcome class, and optional human diagnostic.

Event sequence numbers are monotonic within one Glass Box instance.

A failed kernel call is still observable. The Glass Box records the post-call state and failure class; it does not hide, repair, retry, or reinterpret the error.

## Numerical contract

R3 records an explicit numerical contract with each receipt.

Initial contracts support:

- exact IEEE-754 bit comparison;
- absolute amplitude tolerance under IEEE-754 `f64`.

Semantically equivalent zero tolerances are canonicalized: `-0.0` and `+0.0` produce the same contract and artifact identity.

Normalization policy is `ObserveOnly`: the observer records the norm and never silently normalizes the state.

The numerical contract is evidence metadata. It does not let a backend choose a tolerance after seeing a result.

## Approximation declaration

Each observed representation declares either:

```text
Exact
```

or a validated:

```text
Approximate(method, optional declared absolute error)
```

Approximate metadata can only be constructed through its validating constructor; external representations cannot directly populate unchecked method/error fields. Signed zero error bounds are canonicalized to `+0.0`.

The dense oracle declares `Exact`.

Future compressed or approximate representations can use the same receipt type without being mislabeled as exact.

## Timing and memory

R3 records:

- wall-clock elapsed nanoseconds for the wrapped operation;
- logical representation bytes reported by the state snapshot.

Logical bytes are not process RSS, allocator overhead, GPU residency, or total system memory. Those may become additional observations later.

Timing is an observation, not a benchmark claim.

## Artifact identity

Each receipt receives:

```text
sha256:<digest>
```

over deterministic semantic content:

- receipt schema;
- canonical operation bytes;
- numerical contract;
- pre-state snapshot;
- post-state snapshot;
- success/failure outcome class.

The artifact identity deliberately excludes:

- wall-clock elapsed time;
- event sequence numbers;
- human diagnostic strings.

Therefore two semantically identical executions can retain the same artifact identity even if they ran at different speeds or occurred at different positions in an observation stream.

## Operation identity

R3 adds a canonical byte encoding to the representation-independent `Operation` contract.

The encoding includes all operation parameters, including local-unitary matrix entries as IEEE-754 bit patterns. The Glass Box hashes those bytes independently to produce an operation ID.

## Authority boundary

The Glass Box is an observer.

It must not:

- normalize a state;
- change an operation;
- retry failed execution;
- promote an approximate result to exact;
- declare a decoder correct;
- turn timing into a performance claim;
- turn a receipt into QEC authority.

A later QSOL-QEC-BRIDGE candidate may use Glass Box artifacts as input evidence, but bridge conformance remains a separate process.
