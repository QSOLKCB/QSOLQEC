# R10 Noise and Decoder Modules

R10 adds the first representation-independent quantum-error-correction path to
QSOLQEC.

The implementation lives in:

```text
crates/qsolqec-qec
```

Its first exact code family is deliberately narrow:

```text
prime-d odd-length repetition code
generalized X-shift errors only
```

The purpose of R10 is to freeze replay, syndrome, correction, and decoder
contracts before expanding to more general stabilizer or QLDPC decoders.

## Architectural boundary

The R10 crate depends only on:

- `qsolqec-core`;
- `qsolqec-glassbox`;
- `qsolqec-module-api`;
- `qsolqec-ops`.

It does **not** depend on:

- `qsolqec-dense`;
- `qsolqec-stabilizer`;
- `qsolqec-fly-qdn`;
- `qsolqec-memorywall`.

That dependency boundary is intentional.

The decoder cannot silently inspect a state representation, call Dense as an
oracle, or rescue a failed candidate with another representation.

R10 decoders consume:

```text
DataKind::Syndrome
```

and produce:

```text
DataKind::Correction
```

They do not consume `QuditState` or `EncodedState`.

A future structured-memory decoder input must therefore introduce an explicit
adapter/contract rather than acquiring state access implicitly.

## Replayable Weyl noise

`WeylNoiseSpec` defines:

- explicit 64-bit seed;
- X-error threshold in integer parts per million;
- Z-error threshold in integer parts per million.

The permitted threshold range is:

```text
0 ..= 1_000_000
```

The R10 sampler uses a frozen SplitMix64 implementation.

Every subsystem consumes exactly four PRNG words:

1. X decision;
2. X magnitude;
3. Z decision;
4. Z magnitude.

The fixed consumption schedule matters: changing one rate cannot move the RNG
position used by later subsystems.

Magnitude selection performs the modulo operation in `u64` before converting
to `usize`, avoiding architecture-width-dependent replay results between
32-bit and 64-bit Rust targets.

The threshold model is a deterministic synthetic noise specification. A PPM
setting is not evidence that the model reproduces calibrated hardware noise.

## Error patterns

A sampled or manually supplied `ErrorPattern` binds:

- `SystemSpec`;
- source noise-spec digest;
- canonical ordered Weyl-error events;
- deterministic error-pattern digest.

Events are canonicalized by subsystem target.

Duplicate target events fail closed.

An event records:

```text
(target, x_shift, z_power)
```

with both exponents required to lie in `0 .. d`, and an event with both
exponents zero is rejected.

### Replay order

When converted to the common R2 operation stream, each event replays in the
frozen order:

```text
Weyl X
then
Weyl Z
```

This ordering is part of the R10 contract because X and Z do not generally
commute as concrete operators.

## Repetition-code contract

`RepetitionCodeSpec` requires:

- prime local dimension `d`;
- odd code length;
- length at least 3.

The first R10 code space is the generalized repetition family. Its declared
X-error correctable radius is:

```text
t = (n - 1) / 2
```

R10 deliberately rejects composite local dimensions in this decoder family.
This keeps the first decoder aligned with the repository's existing
prime-dimensional stabilizer discipline and avoids conflating native Z4
arithmetic with GF(4) formalisms.

## Syndrome data

R10 adds the concrete typed `Syndrome` object and the module API gains:

```text
DataKind::ErrorPattern
```

The decoder routing kinds `DataKind::Syndrome` and
`DataKind::Correction` already existed in the foundational module API.

For X-shift vector:

```text
e = [e0, e1, ..., e(n-1)]
```

the frozen R10 syndrome convention is:

```text
s_i = e_i - e_(i+1) mod d
```

The syndrome stores its code identity, values, and canonical digest.

### Measurement boundary

`Syndrome::from_error_pattern` derives syndrome from the **declared error
trace**.

It is not a physical syndrome-extraction circuit and is not evidence about
measurement noise.

The initial repetition-X contract rejects any error event with nonzero Z power.
It does not silently discard an unsupported Z component.

## Exact decoder

`ExactRepetitionXDecoder` is the R10 tractable reference decoder.

For one syndrome it enumerates the `d` error representatives compatible with
the frozen syndrome equations by varying the first error symbol.

For each representative it computes Hamming weight.

It accepts only a:

```text
unique minimum-weight representative
whose weight <= t
```

and returns the inverse X-shift vector as the correction.

The decoder fails closed on:

- code mismatch;
- ambiguous minimum-weight syndrome;
- minimum representative outside the correctable radius;
- malformed syndrome data.

No state-vector oracle participates in this decision.

## Correction contract

A `Correction` binds:

- repetition-code identity;
- source syndrome digest;
- decoder ID/version;
- X-shift correction vector;
- deterministic correction digest.

It can be converted to the common R2 `WeylX` operation stream.

`cancels_x_error` validates the supplied error vector before testing exact
component-wise cancellation.

## Lookup candidate decoder

`LookupRepetitionXDecoder` is a separately constructed candidate.

It enumerates the complete declared correctable corpus:

```text
sum_(w=0..t) C(n,w) * (d-1)^w
```

before building a deterministic syndrome-to-correction table.

Construction requires an explicit maximum-entry budget.

If the complete table would exceed the budget, construction fails **before**
enumeration.

Any syndrome collision among distinct correctable corrections also fails
closed.

The lookup decoder never calls the exact decoder while decoding.

## Candidate comparison

`compare_decoders_on_correctable_errors` compares two decoder modules over
the complete correctable corpus, subject to an explicit case budget.

The comparison records:

- total cases;
- matching corrections;
- mismatches;
- reference failures;
- candidate failures;
- first mismatch syndrome digest;
- deterministic comparison digest.

Comparison is observational.

It does not replace a candidate result with the reference result and therefore
does not provide oracle rescue.

The R10 regression corpus includes complete comparison for the prime-qutrit
length-5 code:

```text
d = 3
n = 5
t = 2
cases = 1 + 5*2 + 10*4 = 51
```

The lookup candidate matches the algebraic exact decoder on all 51 correctable
patterns.

## Module descriptors

R10 exports three descriptors.

### Replayable noise

```text
id: replayable-weyl-noise
capability: NoiseModel
produces: ErrorPattern, OperationStream
maturity: E2 deterministic fixture
```

### Exact repetition-X decoder

```text
id: repetition-x-exact
capability: Decoder
consumes: Syndrome
produces: Correction
maturity: E2 deterministic fixture
```

### Lookup repetition-X candidate

```text
id: repetition-x-lookup
capability: Decoder
consumes: Syndrome
produces: Correction
maturity: E3 oracle-compared
```

For the lookup candidate, E3 records exhaustive comparison against the exact
algebraic reference over the declared tractable corpus. It does not mean Dense
is used at runtime.

## Donor boundary

QSOLKCB/QEC informed the engineering discipline used here:

- deterministic seeded behavior;
- stable ordering;
- replayable artifacts;
- decoder-core separation;
- comparisons that observe rather than modify a candidate.

No QEC decoder implementation was copied into R10.

QSOLQEC keeps its own smaller typed Rust contracts and claim boundaries.

## Claim boundary

R10 establishes:

- deterministic replayable synthetic Weyl error traces;
- typed error-pattern, syndrome, and correction data;
- a decoder module contract;
- an exact tractable prime-d repetition-X decoder;
- a separately implemented bounded lookup candidate;
- deterministic candidate/reference comparison.

R10 does **not** establish:

- arbitrary stabilizer-code decoding;
- arbitrary Pauli/Weyl error correction;
- Z-error correction by the repetition-X decoder;
- composite-d stabilizer decoding;
- physical noise or measurement fidelity;
- fault-tolerant syndrome extraction;
- decoder performance at large code distance;
- direct structured-memory decoder access;
- a quantum-hardware result.

Those require later explicit contracts and evidence.
