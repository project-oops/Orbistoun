# 607. Two more measured AGC builders wired; the retail wall is now purely the patch family

**2026-09-15** - "blocked on obSCEne" was not the whole truth: some of the cluster was already
measured, and wiring it is real progress

## The correction to myself

I had been reporting PPSA02664/PPSA03416 as blocked on obSCEne. Two of the builders in the cluster
walling them were already measured, from requests that came back days ago, and had simply not been
wired:

- **`sceAgcCbNop`** - measured *whole* (`166-agc/cb-nop`): the packet is `0xffff1000`, four bytes,
  and it takes no arguments, so it is a complete encoder, not a stand-in.
- **`sceAgcDcbAcquireMem`** - header (`0xc0065800`) and extent (32 bytes) measured
  (`166-agc/dcb-acquire-mem`, REQ-...72d7/c74f); it is called three times by the wall title.

Both are now wired, bringing the AGC surface to nineteen.

## AcquireMem is a reservation, and says so

Its argument-to-body mapping is a permutation the sweep summary fingerprints but does not pin -
`dw2` carries arg5 masked, `dw4` arg4, `dw7` arg3, with `dw1`/`dw6` constant - and encoding that
from a summary would be guessing. So it wires on the same terms as the Cx producer (D696): the
measured header and extent, a real cursor, and a **zero body**. That hands the guest a real
reservation where an unwired builder handed it the loud placeholder, which is what clears its
callers' placeholder-as-pointer; the exact body waits on the systematic sweep (REQ-...a70f). The
test asserts exactly this - header present, length right, body all zero, no invented encoding.

`CbNop` needs no such caveat: no arguments, measured whole, asserted to the byte.

## What moved

PPSA02664 now answers two more of its imports - unanswered fell from 32 to 30 - because `CbNop` and
`AcquireMem` are served rather than stubbed. Reach and fault are unchanged; the frontier was
regenerated.

## What the wall now is, exactly

It did **not** move: still `read of 0xa8` inside `memcpy` (`VCRUNTIME140.dll+0x1dc8d`). With
`CbNop` and `AcquireMem` answered, the remaining unimplemented calls on the wall title's secondary
command buffer are:

- `sceAgcSetCxRegIndirectPatchAddRegisters` (32x) and `sceAgcSetUcRegIndirectPatchAddRegisters` -
  the **patch family**, blocked on REQ-...a70f / 9e21.
- `sceAgcDcbPushMarker` / `sceAgcDcbPopMarker` - measured *extent only* (12 bytes), **no header
  dumped**, so they cannot even be reserved without inventing an opcode. Blocked on the same sweep.
- `sceAgcDcbWaitRegMem` - refuses to execute on sentinel arguments (`166-agc/acb-wait-reg-mem`), so
  its body is not measurable this way.

So the wall is now *purely* what a hardware sweep must answer - I have wired everything the resolved
requests measured. That is a sharper statement than "blocked": the cluster is characterised down to
the four functions that genuinely need REQ-...a70f, and nothing measurable remains unwired.

## The routing behaved correctly, which is worth noting

The `read of 0xa8` fault's null base is `r13`/`r14` (=0), not `rax`. Worklog 606's routing checks
the immediately-preceding call's return, which lands in `rax` - so it did **not** fire here, and the
finding fell back to the honest "find where r13 was set to zero" hint rather than blaming an
unrelated last call. The restraint built in yesterday held on the first real fault that could have
tripped it.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,347 pass / 0 fail (the wired-set guard demanded 17 -> 19, and `cb_nop` /
`acquire_mem` each assert their measured bytes; `cb_nop` mutation-checked), worklogs unique,
identity scan clean, frontier regenerated.
