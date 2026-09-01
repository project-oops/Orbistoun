# D216 - A pattern's size is computed once, not per candidate


**decided** · 2026-08-24

`Pattern::len` multiplied the lengths of every vocabulary in the pattern, every time it
was asked. The search asks it for every pattern, for every candidate: once to locate
which pattern a global index falls in, and again inside `write_at` to bounds-check the
index the caller had just checked against that same number.

Over a 2.6-billion-candidate run against one module, that is roughly thirty dependent
heap loads per candidate - `parts` is a `Vec<Vec<String>>`, so each length lives in a
separate allocation - to rediscover eight numbers fixed at the moment the grammar was
parsed.

The number now lives in the struct, computed in `Pattern::new`. `parts` is private, which
is what makes the cached value trustworthy: nothing can change the vocabularies without
going through the constructor.

### What it was worth

Measured on this project's own workload - `names` against `libSceNpCppWebApi.prx`, 2.6
billion candidates, sixteen threads - as candidates per *process-CPU-second* rather than
per wall-second. Five runs per variant:

| | candidates/CPU-second | |
|---|---|---|
| before | 2,427,396 | |
| after | 2,717,500 | **+11.9%** |

Wall-clock was useless here and worth recording as a trap: on a machine sharing sixteen
threads with other work, the *same binary* varied 48% run to run, which is ten times the
effect being measured. Two early A/B rounds looked clean and consistent, and pointed the
wrong way. CPU-time normalisation fixes this because a descheduled process accrues no CPU
time - contention changes how long a run takes, not what it costs.

### What was tried and rejected

A bitmap prefilter in front of `Targets::wants`, on the reasoning that a NID is already a
uniformly distributed hash and `HashSet` was running SipHash over it for every candidate.

It works - +5.6% on its own - and it is not worth having. A probe that reduced `wants` to
a single integer comparison, keeping the SHA-1 alive so it could not be optimised away,
measured +7.4%: that is the ceiling on *any* lookup optimisation, and the prefilter was
already close to it. Stacked on top of this decision it added +0.8%, inside the noise.

Both were relieving the same stall. While the core waited on the pointer chase this
decision removes, the hash lookup's work was hiding in the shadow of that latency for
free. Remove the chase and the lookup stops being on the critical path. Anyone tempted by
a faster hasher, a perfect-hash table or a Bloom filter here should read that ceiling
first: there is at most 7% in it, and this decision has already taken most of it.

### What actually dominates

SHA-1, at roughly nine tenths of the per-candidate cost. `sha1 0.10` dispatches to a
SHA-NI backend at runtime where the CPU has one, and the development machine - a Coffee
Lake part - does not, so it takes the software path. The remaining multiples are in
hardware with SHA extensions, or in multi-buffer SIMD, not in this file.

