# 614. The Cx-indirect patch answers its measured 0x0, and two Unity titles reach 222 imports

**2026-09-15** - the producer skeleton (D696) moved PPSA02664's wall one call downstream to the patch,
whose own placeholder was the new leak; the measured return clears it

## What the run said, and how the taxonomy named it

Mining the bus (REQ-...c7d1 resolved: PPSA02664's render-state sub-object is guest-internal, not an
AGC object - verified against the real rows, though the resolution cited the wrong line numbers) sent
me back to PPSA02664. Its wall had **moved** off the c7d1 site (`image+0x3f258`) to a host fault, and
the fault report named it exactly - the host-module renderer (worklog 594) and the emulator-bug
framing both firing:

```
! the guest faulted at VCRUNTIME140.dll+0x1dc8d, read of 0xa8 ...
  >> EMULATOR BUG: the fault is in orbistoun's OWN code ... the host stack:
     11: orbistoun_libc::memcpy
     12: orbistoun_thunk::dispatch::on_guest_call
just before: libc::memcpy(...)                                     <- faulted
just before: sceAgcSetCxRegIndirectPatchAddRegisters(...) -> 0xf7ff0001
```

So the guest called `sceAgcSetCxRegIndirectPatchAddRegisters`, it answered the **unimplemented
placeholder**, and the guest carried that into a `memcpy` and faulted in host code on it. This is the
*same* placeholder-as-pointer leak D696 fixed for the **producer** (`sceAgcDcbSetCxRegistersIndirect`)
- wiring the producer skeleton gave the guest a real cursor and moved it one call along, to the patch,
whose own return was the next placeholder.

## The fix: a measured return, not an invented one

`166-agc/patch-cx-registers-indirect` measured this patch returns **`0x0`** with the cursor unchanged
(REQ-...4386; I verified the `patch|rc|0x0` row exists in three sweeps - 125124, 152001, 174357 -
rather than trusting the resolution's line numbers, which were wrong here too). So orbistoun was
answering a *wrong* value: the placeholder where hardware returns `0x0`. Implemented to answer the
measured `0x0` and **write nothing** - the guest `memcpy`s each eight-byte register entry into the
packet itself (knowledge file), and the in-place amendment is an argument mapping no sweep has pinned,
so returning the measured success without inventing a body is the honest half, the same discipline as
the producer skeleton it completes.

The one-bit oracle justified it before the edit (`ORBISTOUN_RETURN=...:0x0` -> FURTHER, +1 import),
and the implemented version confirms it: PPSA02664 **220 -> 222 imports**, stubs answered dropping
74 -> 42. And it is not one title: PPSA03416, in the same Unity/AGC cluster, also advanced 220 -> 222.
The two furthest Unity titles moved together, from one measured return.

## The wall after it, named for the follow-up

The `memcpy` still faults at the same site, because the patch was one of a **cluster** of unimplemented
AGC calls the command-buffer loop reaches, each still answering the placeholder:
`sceAgcDcbWaitRegMem`, `sceAgcDmaDataPatchSetDstAddressOrOffset`, and an unnamed hash
`0x7d86501b8094ef57`. Only the Cx patch's return is measured (`rc 0x0`); the rest of the `sceAgc*Patch*`
family has **no measured return** in any sweep. So the honest state is: one patch implemented from its
measurement, the family's returns filed back to obSCEne (the a70f patch section, extended to ask for
each patch's `rc`, not only its body amendment).

## Made to fail

- `the_cx_indirect_patch_answers_the_measured_success_not_a_placeholder` (gpu) - it returns `0x0` and
  specifically not the placeholder the guest reads as a pointer.
- `the_wired_set_is_the_size_the_module_documentation_claims` - 22 -> 23, the guard against the module
  prose drifting from the built slice.
- `every_implemented_function_is_written_down` (service) - the knowledge entry was `guest-observed`
  and stale ("never measured", describing the pre-D696 producer); rewritten to `measured` with the
  `rc 0x0` rows and the note that the producer now hands a real address, so the wall is the patch's
  own return.
- `the_frontier_matches_what_is_committed` (overrides) - the run recorded a new best for PPSA02664 and
  PPSA03416 (220 -> 222); regenerated the golden, and the diff is the two titles advancing together,
  which is the point of that test.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,356 pass / 0 fail, worklogs unique, identity scan clean. The frontier golden
was regenerated for the two improved records and its diff read before committing to it.
