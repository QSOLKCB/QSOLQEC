# R9 Amplitude-Memory-Wall Challenge

R9 turns the R6-R8 structured-memory work into a bounded experiment under the
existing R5 memory-wall harness.

The primary question is:

> Can the exact Fly-Phi664 Q(d,n) representation preserve the declared workload
> while materializing and retaining less memory than the explicit DenseState?

A negative result is valid evidence. R9 does not assume that virtualized
materialization beats Dense.

## Common representations

The memory-wall CLI now accepts:

```text
dense
stabilizer
fly-phi664
```

The deterministic workload remains:

```text
qsolqec.memorywall.clifford-ring.v1
```

All three representations receive the same operation stream for a given
Q(d,n) and round count.

Dense and PrimeStabilizer preserve their existing R5 behavior. The Fly
candidate uses the exact R8B virtualized representation and operation contract.

## Receipt schema

R9 promotes the common receipt schema to:

```text
qsolqec.memorywall.receipt.v2
```

and the sweep schema to:

```text
qsolqec.memorywall.sweep.v2
```

The existing R5 memory/timing fields remain. R9 adds an optional
`structured_candidate` section that is populated only for the Fly-Phi664
candidate.

Dense and PrimeStabilizer therefore do not receive invented structured-memory
measurements.

## Fly source identity

The Fly candidate always uses the pinned MaleCNS v1.0 source descriptor from
R7.

Two body-ID input modes are supported.

### Built-in conformance fixture

When no body-ID file is supplied, the runtime uses the two real body IDs in the
R7 provenance fixture:

```text
12781
556329
```

This exposes:

```text
2 * 664 = 1328
```

logical Phi664 addresses.

It is deliberately small. It exists for CI, contract tests, and tractable
local examples, not as whole-connectome evidence.

For qubits, that fixture can hold Q(2,10) because 1024 <= 1328, while Q(2,11)
requires 2048 addresses and terminates with the structured
`logical-namespace-insufficient` outcome.

### Explicit body-ID list

For a larger experiment:

```bash
cargo run --release --locked -q -p qsolqec-memorywall -- run \
  --representation fly-phi664 \
  --dimension 2 --subsystems 20 --rounds 8 \
  --fly-body-ids /path/to/bodyids.txt
```

The file format is one unsigned `bodyId` per line. Blank lines and lines
beginning with `#` are ignored.

The host pathname is not scientific identity. The adapter canonicalizes the
body-ID set through the R7 codec, and receipts bind:

- macro-node count;
- canonical body-ID digest;
- MaleCNS source identity digest;
- Phi664 geometry digest;
- exact logical namespace address count.

Duplicate body IDs still fail closed under the R7 contract.

## Virtualization configuration

R9 exposes the main Gate-B configuration knobs:

```text
--fly-page-span N
--fly-tile-span N
--fly-sparse-max N
--fly-bitmap-max N
--fly-scratch-domains N
--fly-owner-count N
--fly-cache-states N
--fly-max-in-flight N
```

The defaults are only an initial benchmark configuration. They are not promoted
as portable optima.

Every value above is bound into the Fly experiment identity.

## Logical scale versus resident scale

For Fly-Phi664, the Glass Box logical state remains:

```text
16 * d^n bytes
```

because the exact semantic state is still a Complex64 amplitude for every
computational-basis index.

R9 does not claim that sparse storage changes that logical requirement.

Instead it measures whether the exact representation avoids resident
materialization of all those amplitudes.

The distinction remains:

```text
logical Q(d,n) bytes
!=
Phi664 logical namespace
!=
materialized amplitude payload
!=
adaptive page working set
!=
process RSS
```

## Structured candidate measurements

The Fly receipt records:

- `macro_node_count`;
- `body_ids_digest`;
- `source_identity_digest`;
- `geometry_digest`;
- `logical_namespace_addresses`;
- `materialized_address_count`;
- `materialized_page_count`;
- Sparse / Bitmap / Dense page counts;
- `tracked_state_resident_bytes`;
- `worker_scratch_capacity_bytes`;
- `peak_tracked_active_bytes`;
- configured scratch and owner domains;
- final cache-state count;
- cache hits and misses;
- invariant reuse count;
- reused and recomputed generation counts;
- worker dispatches;
- scanned and soundly skipped addresses;
- executed and pruned Fourier lanes.

### Tracked active bytes

`peak_tracked_active_bytes` is the high-water mark sampled by the exact R8B
executor for:

```text
active virtualized state tracked bytes
+
preallocated worker-local SoA scratch capacity
```

It is a deterministic representation metric.

It is **not** process RSS and is not claimed to include every allocation,
container node, cached generation, allocator structure, or temporary object.

The common `resident_working_set_bytes` field carries this tracked high-water
value for Fly-Phi664.

### Peak process RSS

Linux `VmHWM`, when available, remains the broader mandatory memory evidence:

```text
peak_process_rss_bytes
```

It includes the process-level consequences of indexes, caches, maps, allocator
overhead, page tables visible through RSS accounting, temporary candidate
states, and other resident structures.

Candidate RSS is frozen before Dense oracle verification starts, so the oracle
cannot contaminate the candidate peak measurement.

## Reuse and recomputation

For the R8B operation-generation cache:

```text
recomputed_generations = cache_misses
reused_generations = cache_hits + invariant_reuses
```

The receipt also preserves those components separately.

This is operation-generation reuse evidence, not a claim that every reused
byte or tile avoided all CPU work.

## Oracle agreement

Within `--oracle-limit-mib`, the Fly candidate executes first and freezes its
memory/timing measurements.

Only then does the runtime construct DenseState and run the same workload.

R9 compares reconstructed Fly amplitudes against Dense amplitudes.

The exact R8 contract expects zero numerical difference, so the R9 Fly oracle
comparison uses:

```text
tolerance = 0.0
```

A nonzero maximum error is recorded as a mismatch.

If Dense exceeds the configured oracle limit or cannot be constructed,
agreement is recorded as unavailable rather than assumed.

## Fresh-process rule

The existing R5 sweep rule remains mandatory.

Each representation / d / n point executes in a fresh child process because
Linux `VmHWM` is cumulative over process lifetime.

A killed or crashed child becomes a `child_failures` record and does not
erase earlier completed points.

## Examples

Small conformance run using the built-in two-bodyId fixture:

```bash
cargo run --locked -q -p qsolqec-memorywall -- run \
  --representation fly-phi664 \
  --dimension 2 --subsystems 8 --rounds 4 \
  --oracle-limit-mib 16
```

Common overlapping sweep:

```bash
cargo run --release --locked -q -p qsolqec-memorywall -- sweep \
  --representations dense,stabilizer,fly-phi664 \
  --dimension 2 \
  --start-n 4 --end-n 10 --step 2 --rounds 8 \
  --max-logical-mib 64 \
  --oracle-limit-mib 16 \
  --output r9-overlap.json
```

A larger Fly-only sweep can use a larger exact body-ID list:

```bash
cargo run --release --locked -q -p qsolqec-memorywall -- sweep \
  --representations fly-phi664 \
  --dimension 2 \
  --start-n 10 --end-n 26 --step 2 --rounds 8 \
  --fly-body-ids /path/to/bodyids.txt \
  --oracle-limit-mib 16 \
  --output r9-fly.json
```

The requested end point is not evidence that the source geometry can reach it.
The exact supplied body-ID membership determines the namespace ceiling and the
runtime fails closed when Q(d,n) exceeds it.

## Logical budget warning

`--max-logical-mib` remains the R5 logical-representation preflight limit.

For Fly-Phi664, logical state bytes are still `16 * d^n`, so this flag can
stop the Fly candidate even when its sparse resident materialization would be
smaller.

That is intentional: R9 does not redefine an existing R5 resource-limit field.

Use an explicitly chosen logical study horizon for each experiment and retain
that value in the receipt.

## Claim boundary

R9 receipts answer:

> What happened for this source revision, Q(d,n), operation stream, body-ID
> membership, virtualization configuration, resource budget, and host?

They do not establish:

- universal Fly-Phi664 superiority;
- a universal memory saving;
- a universal runtime saving;
- correctness beyond the declared exact/oracle or independent evidence;
- biological cognition or neural-dynamics relevance;
- an approximation result.

A sparse tracked working set with a high process RSS is not a memory win.

A candidate that becomes dense, hits its Phi664 namespace ceiling, runs slower
than Dense, or loses practical memory advantage is still a valid R9 result.
