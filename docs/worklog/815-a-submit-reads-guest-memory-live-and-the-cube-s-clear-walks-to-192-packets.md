# 815. A submit reads guest memory as the run stands, not as it stood at entry — and the cube's clear self-test walks from 0 packets to 192

**2026-09-24** — worklog 814 found the cube's first submission walking to zero packets and suspected
the region list. Confirmed: `set_guest_regions` is called once, before the guest is entered, from the
direct-memory map at that moment; the SDK's `oops_mem_alloc` maps the GL context's fence, command
buffer and render targets afterwards (`sceKernelMapDirectMemory`, `sceKernelBatchMap`), so the command
buffer the guest handed over lay in no region the submit knew of and was refused (5bff).

## The fix: a live readability check, installed by the worker

- **`orbistoun_kernel::is_guest_readable(base, len)`** answers from the kernel's tables as they stand
  when asked: one region wholly covering the range, among the runtime mappings (and then only if its
  protection allows reads), the regions the worker noted (image, TLS) and the stacks — the same places
  `region_containing` already consults for `sceKernelVirtualQuery` (D446).
- **`orbistoun_gpu::agc_driver::install_region_lookup`** takes that check as a plain `fn`, because
  `orbistoun-gpu` does not depend on the kernel crate (principle 12). `MappedRegions::contains` accepts
  a range the entry list covers *or* the live check vouches for. The entry list stays; it is what the
  crate's tests register.
- The worker installs it right after setting the entry list.

A kernel test, `readability_is_answered_from_the_tables_as_they_stand`, pins the live property: a
region noted after the first question is found by the next one, the last word of a region is readable,
a range running off its end is not, and an empty range is not a read.

## What the cube's submission is now

```
orbistoun: a submission reached the NVIDIA GeForce RTX 5070 Ti backend: 4 command(s) driven, 0 refused, in 153 ms
  ! the guest submitted a command buffer: 192 packets, 0 draws, 2 shader candidates
      170 register writes extracted from the stream
      2 of the addresses named resolved to a region the guest was given, 0 did not (D101)
```

The GL context's hardware clear self-test is now a real stream to orbistoun: 192 packets, 170
register writes, and both shader addresses it names resolve into guest memory. Four render commands
reach the backend and none is refused. **It records no draw**, so however the self-test clears its
target — a draw the walk does not recognise, or a fill by another packet — is not yet one; which
packets make up those 192, and what the clear is, is the next unit. After that come the stream's two
closing `RELEASE_MEM`s, whose fence writes D705 lets land only once the work before them has executed.

The guest itself is unchanged — it still waits on its fence (verdict `same`, 14 imports).

## Gate state

`crates/orbistoun-kernel/src/lib.rs` (`is_guest_readable`, test), `crates/orbistoun-gpu/src/agc_driver.rs`
(`install_region_lookup`, the lookup in `contains`), `crates/orbistoun-worker/src/lib.rs` (the install).
`orbistoun-gpu` 92 and the kernel test pass. `./bin/orbistoun check` green, worklog index regenerated,
identity scan clean. No commit.
