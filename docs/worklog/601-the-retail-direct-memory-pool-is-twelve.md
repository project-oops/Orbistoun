# 601. The retail direct-memory pool is twelve gibibytes, and it moves Grand Theft Auto

**2026-09-15** - the request I filed for PPSA04263's memory wall came back, and it moved the wall
while refuting my own hypothesis

## The wall, and what worklog 574 got wrong

PPSA04263 asks `sceKernelAllocateMainDirectMemory` for `0x1_20F0_0000` (4.51 GiB) after already
taking ~4 GiB, and orbistoun refused it against a five-gibibyte pool - a `NoMemory` fault the guest
died on. Worklog 574 offered three explanations and led with the wrong one: that `Main` direct
memory might be a **separate account** from direct memory. `REQ-...5d1c` settled it.

**It is one pool, and the pool is twelve gibibytes on retail.** Verified in the raw log
(`20260915-125124-eboot.obs.log`, which the sweep produced and I read directly rather than trusting
the resolution summary):

- `OBS|res|020-memory/direct-size|pass|0x300000000` - `sceKernelGetDirectMemorySize` answers 12 GiB.
- `020-memory/allocations-do-not-overlap`: two allocations at `0x2230000` and `0x2234000` - disjoint,
  from one pool handing out sequential blocks.

So `Main` and plain direct memory are not two accounts; they draw from one twelve-gibibyte range,
and PPSA04263's 8.5 GiB of total demand fits it with room to spare.

**A caveat on provenance.** The resolution *summary* described a tidy five-step sequence with
specific addresses (`0x2400000`, `0x42400000`); those exact values are not in the log, which shows
`020-memory/allocate` skipped for lack of a syscall route and `allocate-main` at `0x2230000`. The
headline - 12 GiB, disjoint - is directly measured and is what this rests on; the summary's
per-step addresses are not, and are not relied upon.

## It moves the wall, exactly to the record

With the pool at 12 GiB, PPSA04263 gives **70 imports, 30,261 calls, `image+0x196b91a`** - the
2026-09-07 record to the digit, reproduced with no override where it previously died at 49 imports
in the allocator. The `NoMemory` fault is gone; the guest advances the 21 imports the record always
claimed.

## Why a setting, not a flipped constant

D398 measured this same call at **five** gibibytes, on a conformance/homebrew run, and that
measurement was real - it was even validated to matter (changing the size moved a guest's next
query, exactly tracking). So there are two hardware measurements of `sceKernelGetDirectMemorySize`,
5 GiB and 12 GiB, and they are not in conflict: **a retail title and a homebrew payload are handed
different budgets by the platform's sandbox**, the same way their filesystem and network reach
differ. A single compiled constant cannot be right for both.

`Settings::pool_bytes` now holds it, defaulting to the retail 12 GiB, with
`HOMEBREW_DIRECT_MEMORY_SIZE` (5 GiB) kept for a homebrew leg to configure. The default is retail
because the corpus and every memory wall in it are retail. The pool the guest allocates from and
the size the query reports are the **same field**, so they cannot describe different machines -
D398's invariant, now structural rather than a coincidence of two constants.

## Made to fail

`the_default_pool_is_the_retail_size_and_homebrew_is_a_distinct_measured_size` pins that the default
is 12 GiB and the homebrew figure is a genuinely different 5 GiB. Reverting the constant to 5 GiB
fails it with the retail-size assertion - which is the regression that would silently re-wall
PPSA04263. `the_reported_size_matches_the_model_being_walked` now reads both the report and the pool
from the one setting, so the D398 invariant holds by construction.

## What did not break, which is the interesting part

Defaulting every guest to 12 GiB changed **no** test but the frontier - obSCEne under orbistoun now
sees 12 GiB where hardware-homebrew saw 5, and nothing asserted otherwise, so the homebrew value is
a configurable correctness refinement rather than a regression. PPSA04263 re-ranks with one more
import answered (63 -> 64); the frontier was regenerated.

## What was refused

Nothing here - the value is measured, verified in the raw log, and proven to move the wall to the
recorded position. What was *not* done: building per-title sandbox-privilege detection to choose 5
vs 12 automatically. The setting exists; wiring a homebrew leg to select 5 GiB is a smaller follow-up
that the corpus does not currently need, since no homebrew guest in it allocates near either ceiling.

## Gate state

fmt clean, clippy `--workspace --all-targets -D warnings` clean, `cargo test --workspace` 2,341
pass / 0 fail, worklogs unique, identity scan clean, frontier regenerated.
