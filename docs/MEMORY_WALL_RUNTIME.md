# R5 Memory-Wall Runtime

R5 supplies the common measuring instrument required before QSOLQEC introduces more exotic representations.

The runtime is intentionally provider-neutral. The same binary and experiment specification can be run on a local workstation, a Vast instance, qBraid Lab compute, or another Linux host.

## Crate

qsolqec-memorywall

The initial executable supports:

~~~text
probe
run
sweep
~~~

## Measurement boundary

R5 freezes this distinction:

~~~text
logical representation bytes
!=
materialized payload bytes
!=
resident working-set bytes
!=
process RSS
~~~

For Dense, PrimeStabilizer, and the R9 Fly-Phi664 virtualized candidate, the current implementation can report:

- estimated logical bytes before allocation;
- final logical representation bytes from ObservableState;
- materialized payload bytes;
- Linux process RSS where /proc/self/status is available;
- Linux process peak RSS (VmHWM) where available;
- construction time;
- operation execution time;
- final snapshot time;
- oracle-verification time separately;
- materialization count;
- allocation count as unavailable (null) until an allocation observer exists.

For `fly-phi664`, receipt schema `qsolqec.memorywall.receipt.v2` also carries an optional `structured_candidate` section with exact MaleCNS/Phi664 source and geometry identities, logical namespace size, materialized addresses/pages, adaptive-page mix, tracked active working-set high water, scratch capacity, reuse/recompute counters, and pruning/scan counters. Dense and Stabilizer leave this section null rather than fabricating candidate-specific measurements.

See [AMPLITUDE_MEMORY_WALL_CHALLENGE.md](AMPLITUDE_MEMORY_WALL_CHALLENGE.md) for the R9 measurement semantics.

A missing measurement is serialized as null. The runtime does not invent a value.

## Fresh-process rule

Linux VmHWM is cumulative over the life of one process.

Therefore sweep launches every (representation, d, n) point as a fresh child process.

This prevents a prior Dense allocation from contaminating a later Stabilizer peak-RSS measurement.

## Deterministic workload

The initial workload is:

~~~text
qsolqec.memorywall.clifford-ring.v1
~~~

For each round it deterministically walks subsystem targets and emits only the operation family already supported exactly by the prime stabilizer candidate:

- Fourier;
- controlled shift when n > 1;
- Weyl Z;
- Weyl X;
- SWAP when n > 1.

The workload ID hashes the workload schema, Q(d,n), round count, and canonical R2 operation bytes.

The workload ID does not include the representation. Dense, Stabilizer, and Fly-Phi664 therefore receive the same declared operation stream.

## Public source identity

R5 enforces the invariant:

> **Every benchmark receipt must bind to the exact source revision used to build the executable, with a public locator that an independent reproducer can retrieve.**

The memory-wall crate's build script requires a **clean Git checkout**, resolves the Git commit SHA while the binary is built, and embeds that 40-hex revision into the executable. Runtime working-directory Git state is never used as benchmark provenance.

The build script fails closed on **any dirty checkout**, including untracked files. The workspace `Cargo.lock` is committed and benchmark builds use `--locked`, so dependency versions are part of the retrievable source identity rather than an untracked local input.

The build script also registers the tracked repository files plus Git HEAD/ref/index metadata as Cargo rerun inputs, so reusing one target directory across commit A -> commit B forces provenance resolution to run again. Before embedding the SHA, it performs a public Git fetch of that exact revision from `https://github.com/QSOLKCB/QSOLQEC.git`; a clean local-only or fork-only commit is rejected if the canonical public repository cannot serve it.

Receipts expose both:

- `source_revision`;
- `source_revision_url`, pointing at the corresponding public QSOLQEC commit.

Source-archive builds without Git metadata are deliberately rejected for benchmark receipts because the runtime cannot prove that arbitrary archive bytes match the claimed public commit.

An already-built executable cannot be relabeled by changing its runtime environment.

## Experiment identity

The experiment identity binds:

- representation identity;
- d;
- n;
- round count;
- workload identity;
- logical-memory budget;
- dense-oracle logical limit;
- compute backend.

Those resource limits are also serialized directly into every receipt as `max_logical_bytes` and `oracle_logical_limit_bytes`; consumers never have to reverse-engineer them from the opaque experiment hash.

Host identity is deliberately separate, so the same experiment can be executed on different machines.

## Oracle agreement

Dense is the reference representation and records self-reference.

For PrimeStabilizer and Fly-Phi664, the runtime freezes candidate timing and candidate RSS before dense verification begins.

If the corresponding dense state is below the configured oracle logical-byte limit, the runtime performs representation-appropriate exact verification:

- PrimeStabilizer applies each final stabilizer generator to Dense and records maximum generator/norm disagreement under the existing 1e-10 tolerance.
- Fly-Phi664 reconstructs the exact candidate amplitudes after candidate measurements are frozen and compares them to Dense with tolerance 0.0.

If Dense is too large or cannot be constructed, oracle agreement is recorded as unavailable rather than silently assumed.

## Resource preflight

--max-logical-mib is a logical-representation preflight budget.

Example:

    cargo run --locked -q -p qsolqec-memorywall -- run \
      --representation dense \
      --dimension 2 \
      --subsystems 24 \
      --rounds 8 \
      --max-logical-mib 512

If the estimated representation exceeds the declared budget, the run emits a machine-readable logical-budget-exceeded receipt before constructing the state.

This is not yet a hard operating-system RSS cgroup/rlimit. R5 records process RSS; later runtime hardening may add platform-specific hard resident-memory enforcement.

## Local examples

Probe the host:

    cargo run --locked -q -p qsolqec-memorywall -- probe

Run one dense point:

    cargo run --locked -q -p qsolqec-memorywall -- run \
      --representation dense \
      --dimension 2 \
      --subsystems 16 \
      --rounds 16 \
      --max-logical-mib 1024

Run the same workload through the stabilizer representation:

    cargo run --locked -q -p qsolqec-memorywall -- run \
      --representation stabilizer \
      --dimension 2 \
      --subsystems 16 \
      --rounds 16 \
      --oracle-limit-mib 16

Sweep all current representations:

    cargo run --locked -q -p qsolqec-memorywall -- sweep \
      --representations dense,stabilizer,fly-phi664 \
      --dimension 2 \
      --start-n 4 \
      --end-n 28 \
      --step 2 \
      --rounds 16 \
      --max-logical-mib 2048 \
      --oracle-limit-mib 16 \
      --output memorywall-local.json

A representation sweep stops after its first structured non-success outcome.

If a child process is terminated before it can emit a receipt—for example by a cgroup or kernel OOM kill—the parent preserves every completed point, records the attempted terminal point in `child_failures` with exit-code/signal evidence, and continues with the next representation. It does not discard the completed sweep evidence.

## Cloud use

R5 does not embed Vast, qBraid, SSH, scheduler, billing, or provider credentials.

The intended workflow is:

~~~text
same repository SHA
same memory-wall binary
same experiment arguments
different host
        |
        v
machine-readable receipts
~~~

For a remote Linux host:

    git clone <repository>
    cd QSOLQEC
    git checkout <exact-sha>

    cargo run --release --locked -q -p qsolqec-memorywall -- probe
    cargo run --release --locked -q -p qsolqec-memorywall -- sweep ...

Benchmark binaries must be built from the clean public Git checkout. This keeps the embedded source identity independently retrievable and prevents source archives or dirty worktrees from being mislabeled as a public commit.

## NVIDIA metadata

If nvidia-smi is available, host probing records GPU name, total advertised GPU memory, and driver version.

R5's initial compute backend remains scalar-cpu.

Recording a GPU does not imply the workload executed on it. GPU execution remains a later compute-backend phase.

## Claim boundary

R5 answers:

> How did this representation behave under this workload, resource budget, source revision, and host?

It does not answer:

> Which representation is universally best?

Benchmark receipts are observations, not correctness proofs. Correctness remains tied to the declared representation contract and oracle/conformance evidence.
