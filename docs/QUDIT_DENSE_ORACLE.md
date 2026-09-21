# Native Qudit Dense Oracle

## Purpose

R1 introduces the exact dense reference representation for small QSOLQEC systems.

The dense oracle is deliberately simple. It stores one complex `f64` amplitude for every computational-basis state in `Q(d,n)`, so its storage requirement is exactly:

```text
d^n complex amplitudes
```

It is not intended to solve the scaling problem. It exists so later compressed, approximate, stabilizer, tensor, GPU, decoder, or other experimental modules have a small-system reference to compare against.

## Frozen basis ordering

QSOLQEC freezes subsystem 0 as the least-significant base-`d` digit.

For subsystem digits:

```text
[q0, q1, q2, ...]
```

the flat dense index is:

```text
q0 + q1*d + q2*d^2 + ...
```

Examples:

```text
Q(2,2): [1,1] -> 3
Q(3,2): [2,1] -> 5
Q(4,2): [3,2] -> 11
```

The `SystemSpec::basis_index` and `SystemSpec::basis_digits` methods are the normative conversion helpers.

Changing this ordering later is an experiment-format change, not an implementation detail.

## Dense-state behavior

R1 supports:

- all-zero basis-state construction;
- arbitrary computational-basis-state construction;
- explicit amplitude construction;
- serial fixed-order norm calculation;
- raw Born-weight extraction;
- single-basis probability lookup.

R1 does not support gates, time evolution, noise, measurement sampling, normalization, or decoding.

## No silent normalization

`DenseState::from_amplitudes` checks:

- dense state size;
- amplitude count;
- finite real and imaginary components.

It does **not** normalize the supplied vector.

That is deliberate. If an upstream module produces a state with norm 2, the oracle reports norm 2. Silently repairing it would hide the defect the oracle is supposed to expose.

## Numerical role

The R1 oracle uses a serial, fixed-order `f64` sum for `norm_squared()`.

This does not claim exact real-number arithmetic. It defines the initial scalar reference implementation against which later optimized implementations can be compared under an explicit numerical contract.

## Deterministic fixtures

R1 includes machine-checked fixtures for:

- `Q(2,2)`;
- `Q(3,2)`;
- `Q(4,2)`.

Each fixture binds:

- local dimension;
- subsystem count;
- subsystem basis digits;
- flat basis index;
- dense state length;
- exact basis-state probability vector.

These fixtures establish basis/index semantics before R2 adds operations.
