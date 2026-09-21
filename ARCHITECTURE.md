# QSOLQEC Architecture

## 1. Design goal

QSOLQEC is an experimental research workbench. The architecture should allow a new representation, decoder, noise model, observer, analysis method, sonifier, or compute backend to be added without redesigning the core runtime.

The core therefore owns only:

- experiment identity;
- module descriptors;
- capability declarations;
- typed input/output declarations;
- experiment-graph validation;
- runtime event boundaries;
- artifact handoff.

Quantum-specific mathematics belongs in modules.

## 2. Authority boundary

```text
QSOLQEC
experimental, permissive
    |
    | E6 candidate export
    v
QSOL-QEC-BRIDGE
translation, reproduction, conformance
    |
    | integration artifact
    v
QSOLKCB/QEC
canonical, strict
```

There is no direct promotion path from QSOLQEC into QEC.

## 3. Modules

A module advertises:

- stable module ID;
- implementation version;
- capabilities;
- data kinds consumed;
- data kinds produced;
- experimental flag;
- maturity level.

Initial capability vocabulary:

- StateRepresentation
- OperationExecution
- NoiseModel
- Measurement
- Decoder
- Observer
- Sonifier
- Analyzer
- ComputeBackend
- Compressor
- Oracle

The vocabulary can grow, but the runtime must not infer capabilities from module names.

## 4. Typed scientific pipeline

Modules connect through declared data kinds rather than hidden shared state.

Examples:

```text
state source
    |
    v
QuditState
    |
    v
operation executor
    |
    v
StateTransition
    |
    v
observer
    |
    v
Observation
```

or:

```text
Syndrome -> Decoder -> Correction
```

or:

```text
QuditState -> Compressor -> EncodedState -> Decompressor -> QuditState
                                                        |
                                                        v
                                                     Oracle
```

The first runtime validates graph compatibility. It does not yet execute arbitrary pipelines.

## 5. Q(d,n) model and basis ordering

The first mathematical primitive is:

```text
Q(d,n)
```

where `d` is local state dimension and `n` is subsystem count.

The dense reference size is:

```text
d^n
```

complex amplitudes.

Initial dimensions of interest are d=2, d=3, and d=4, but the core type is intentionally general.

R1 freezes subsystem 0 as the least-significant base-`d` digit. For digits `[q0,q1,...]`:

```text
index = q0 + q1*d + q2*d^2 + ...
```

The normative conversion helpers are `SystemSpec::basis_index` and `SystemSpec::basis_digits`.

See [docs/QUDIT_DENSE_ORACLE.md](docs/QUDIT_DENSE_ORACLE.md).

## 6. Oracle separation

An oracle exists to judge a candidate representation or decoder where exact calculation remains tractable.

It must not silently rescue the candidate execution path.

```text
candidate execution ------> candidate result
          |
          | same declared experiment
          v
exact oracle -------------> reference result
          |
          v
comparison
```

If a candidate needs oracle intervention to produce its result, that must be a different experiment.

The R1 dense oracle is intentionally exponential. Its purpose is reference truth on tractable systems, not scalability.

## 7. Numerical contracts

Floating-point reproducibility and deterministic experiment identity are different concepts.

The project may require byte-stable identity for:

- experiment specification;
- module configuration;
- seeds;
- input fixtures;
- source revision;
- claim boundary.

Numerical results may require a declared comparison such as:

- exact equality;
- absolute tolerance;
- relative tolerance;
- fidelity;
- distribution distance.

The R1 dense oracle uses serial fixed-order `f64` accumulation for its norm reference. It does not silently normalize supplied amplitudes.

An optimized CPU/GPU backend must not be declared incorrect merely because floating-point reduction order changes last-bit results, but neither may it choose its own acceptance rule after seeing the output.

## 8. Compute backends

Compute acceleration is a module concern.

Examples may include:

- scalar CPU;
- SIMD CPU;
- threaded CPU;
- CUDA;
- multi-GPU;
- distributed compute.

The compute backend accelerates a declared mathematical representation. It does not define mathematical truth.

The long-term pattern should be:

```text
serial reference
      |
      +--> optimized CPU
      +--> SIMD
      +--> GPU
      +--> distributed
```

with bounded comparison against the reference.

## 9. Observation and sonification

The Glass Box is an observer layer around execution.

It may record:

- operation identity;
- representation identity;
- norm/fidelity metrics;
- representation size;
- timing;
- memory;
- approximation declaration;
- decoder output;
- noise event;
- analysis result.

Sonification is initially an observer:

```text
experiment event -> deterministic audio mapping
```

It has no correction authority.

A future simulated phononic-control module would be a separate declared model:

```text
modeled mechanical control -> modeled quantum state
```

Sonification and phononic control must not be conflated.

## 10. Dynamic loading

The architecture is modular, but the initial implementation does not use runtime-loaded shared libraries.

Initial modules are ordinary Rust crates registered at compile time. Dynamic loading can be considered later if a concrete research need justifies the ABI and platform complexity.

## 11. Experimental freedom

The project explicitly permits modules that are:

- speculative;
- approximate;
- unstable;
- slow;
- incomplete;
- falsified by later experiments.

The requirement is not that every experiment succeeds. The requirement is that the experiment declares what happened.
