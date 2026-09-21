# Generalized Qudit Operations

## Purpose

R2 freezes the first representation-independent operation contract and adds exact scalar execution to the dense oracle.

The guiding rule is:

> **Local operations are applied locally. QSOLQEC does not construct a global d^n x d^n matrix merely to act on one or two subsystems.**

The operation specification lives in `qsolqec-ops`. The dense reference implementation lives in `qsolqec-dense`. Future representations can implement the same semantics without inheriting dense storage.

## Frozen conventions

Let:

```text
omega = exp(2*pi*i/d)
```

### Weyl X

```text
X_d(s) |j> = |j + s mod d>
```

`shift` is reduced modulo `d`.

### Weyl Z

```text
Z_d(p) |j> = omega^(p*j) |j>
```

`power` is reduced modulo `d`.

For unit shift/power:

```text
Z_d X_d = omega X_d Z_d
```

under the conventions above.

### Fourier transform

QSOLQEC uses the normalized **positive-exponent** discrete Fourier convention:

```text
F_d |j> = 1/sqrt(d) * sum_k omega^(j*k) |k>
```

This convention gives `F_d^4 = I`.

The sign convention is frozen here so later modules cannot silently choose a different transform and still call it the same operation.

### Controlled shift

R2 uses the generalized SUM-style controlled shift:

```text
CS_s |c,t> = |c, t + c*s mod d>
```

This is not a binary-only "if control == 1" rule. The control digit itself multiplies the declared shift.

### Swap

```text
SWAP |a,b> = |b,a>
```

### Local permutation

A local permutation supplies:

```text
map[input_digit] = output_digit
```

The map must be a complete bijection of `0..d`.

### Generic local unitary

The fallback accepts a row-major `d x d` complex matrix acting on one subsystem.

The matrix is rejected unless:

- its dimension is at least 2;
- it contains exactly `d^2` entries;
- every entry is finite;
- `U†U` is identity within the fixed construction tolerance `1e-12`.

That tolerance validates the supplied operation definition. It is not a general experiment-result tolerance.

## Dense execution

The dense executor uses the R1 least-significant-subsystem basis convention.

Single-subsystem transforms work on strided local lanes of length `d`.

Two-subsystem permutations calculate destination indices directly and use a dense scratch state. They do not construct global unitary matrices.

R2 is a scalar correctness implementation. Allocation reduction, SIMD, threading, and GPU execution remain later compute-backend work.

## Structural atomicity

`DenseState::apply_operations` validates the entire operation sequence before applying the first operation.

Therefore a structurally invalid later operation does not leave a partially mutated state.

This does not make arbitrary numerical execution transactional; it freezes the structural contract needed before R3 adds observation events.

## Algebraic tests

R2 checks:

- `X_d^d = I` for d=2,3,4;
- `Z_d^d = I` for d=2,3,4;
- `Z_d X_d = omega X_d Z_d`;
- `F_d^4 = I`;
- controlled-shift semantics;
- `SWAP^2 = I`;
- local permutation semantics;
- generic local-unitary execution;
- norm preservation across a representative mixed sequence;
- validation-before-mutation for operation batches.

These are software-model checks. They do not establish physical qudit behavior.
