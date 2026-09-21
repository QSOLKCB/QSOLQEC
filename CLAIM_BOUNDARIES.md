# QSOLQEC Claim Boundaries

QSOLQEC is intentionally experimental, but experimental freedom does not remove the need for clear claim boundaries.

## Core boundary

> **Simulation is evidence about the declared model, not evidence about physical quantum hardware.**

## Classical emulation and quantum advantage

QSOLQEC may investigate workloads, representations, or algorithms associated with quantum computation.

A classical implementation that reproduces a workload efficiently is a classical result. It does not establish quantum advantage.

If an efficient classical method reproduces a task previously argued to require quantum computation, the appropriate result is that the classical baseline for that task has improved or that the proposed quantum separation has weakened.

## Hardware

Without execution on physical quantum hardware, QSOLQEC must not claim:

- physical coherence;
- physical error rates;
- hardware fault tolerance;
- physical phonon coupling;
- real-time hardware feedback;
- physical logical-qubit performance;
- quantum advantage.

A hardware adapter may be added in the future, but it is not required by the architecture and is not on the initial critical path.

## Qudit representations

A mathematical state space of equal dimension does not make two physical or computational decompositions identical.

For example:

```text
one d=4 subsystem
```

and

```text
two d=2 subsystems
```

both have four complex basis amplitudes, but their locality, operator structure, and experimental interpretation may differ.

Experiments must declare whether they use native-qudit, packed-qubit, logical-symbol, or another representation.

## Approximation

Approximate modules must declare that they are approximate.

If an error bound is known, it should be fixed before execution.

A downstream module must not silently convert an approximate artifact into an exact claim.

## Oracle

An oracle is a reference evaluator, not a hidden fallback.

Oracle intervention in a candidate's execution path must be declared as part of that candidate algorithm.

## Error correction

A decoder benchmark demonstrates behavior only for the declared:

- code;
- syndrome model;
- noise model;
- parameter range;
- representation;
- implementation;
- compute environment.

It does not establish physical QEC performance unless the experiment contains the required physical evidence.

## Sonification

Sonification maps experiment events or model state into audio.

Audio may be useful for observation, anomaly detection, education, or later controlled studies. It is not correction merely because the source data came from a QEC experiment.

A simulated acoustic or phononic control model is a different module and must be described as simulation.

## AI

AI output is an experimental hypothesis or recommendation unless a specific experiment defines and validates a stronger role.

A language model must not silently become:

- decoder truth;
- oracle truth;
- physical evidence;
- promotion authority.

## Promotion to QEC

Nothing in QSOLQEC is part of QSOLKCB/QEC merely because both repositories are under QSOLKCB.

Potential integration must go through QSOL-QEC-BRIDGE and then through QEC's own current acceptance process.
