# Prime-Dimensional Stabilizer Representation

## Purpose

R4 introduces QSOLQEC's first non-dense state representation.

`PrimeStabilizerState` stores a stabilizer generator tableau rather than one complex amplitude for every computational-basis state.

The initial representation is deliberately restricted to **prime local dimensions**.

That means R4 supports examples such as:

```text
d = 2  qubits
d = 3  qutrits
d = 5  prime qudits
...
```

and deliberately rejects:

```text
d = 4  ququarts
```

This is not because ququart stabilizer formalisms are impossible. It is because the first implementation uses arithmetic modulo a prime, where every nonzero element belongs to a field. QSOLQEC will not silently treat composite `Z_4` arithmetic as though it were the same mathematical object.

## Representation

For `n` subsystems, the initial zero state is represented by `n` commuting stabilizer generators.

Each generator stores:

```text
phase
x[0..n]
z[0..n]
```

and represents:

```text
omega^phase X^x Z^z
```

under the R2 Weyl convention.

The initial `|0...0>` state is stabilized by:

```text
Z_0
Z_1
...
Z_(n-1)
```

Clifford operations update these generators algebraically rather than updating `d^n` amplitudes.

## Exact supported operations

R4 declares exact support for:

- Weyl `X_d`;
- Weyl `Z_d`;
- Fourier `F_d`;
- generalized controlled shift;
- subsystem SWAP.

These operations preserve the stabilizer representation under the frozen QSOLQEC conventions.

## Explicitly unsupported operations

R4 returns `Unsupported` for:

- arbitrary local permutations;
- arbitrary local unitaries.

The stabilizer module does not fall back to the dense oracle.

An unsupported operation means the candidate representation cannot execute that experiment under its current contract.

## Operation support contract

R4 adds the representation-facing support vocabulary:

```text
Exact
Approximate
Unsupported
```

The prime stabilizer module currently returns only `Exact` or `Unsupported`.

The `Approximate` value is reserved for later compressed representations.

Approximation details still belong in the validated Glass Box approximation declaration.

## Oracle comparison

The dense engine remains the small-system oracle.

The stabilizer crate uses dense only as a **dev-test dependency**.

Candidate execution itself has no dense dependency path.

For tractable qubit, qutrit, and d=5 fixtures:

1. the dense oracle and stabilizer candidate begin in the same state;
2. both receive the same supported operation sequence;
3. each stabilizer generator is applied independently to the final dense state;
4. the test verifies that every generator leaves the dense state invariant.

This checks the candidate tableau against the oracle without using the oracle to perform candidate execution.

## Representation size

The dense oracle stores:

```text
d^n complex amplitudes
```

The initial stabilizer tableau stores approximately:

```text
n * (2n + 1)
```

integer scalars.

So the first alternate representation already exhibits the central phenomenon QSOLQEC wants to study:

> A restricted quantum state family can have an exponentially large dense description while admitting a much smaller exact classical representation.

This is not a universal classical simulation result. It applies to the declared stabilizer domain and supported operation family.

## Glass Box

`PrimeStabilizerState` implements the same `ObservableState` interface as the dense oracle.

Its snapshot declares:

```text
representation = prime-stabilizer
approximation = Exact
norm_squared = 1
logical_bytes = tableau payload size
```

The state digest binds the ordered generator tableau.

The digest is representation-specific; it is not claimed to be a universal canonical hash shared by every mathematically equivalent stabilizer generating set.

## Why this matters

R4 is the first point where QSOLQEC has two genuinely different ways to represent the same modeled quantum state:

```text
DenseState
    vs
PrimeStabilizerState
```

Dense remains general but exponential.

Prime stabilizer is restricted but compact.

R5 can now benchmark these representations under controlled workloads instead of discussing representation advantage only in theory.
