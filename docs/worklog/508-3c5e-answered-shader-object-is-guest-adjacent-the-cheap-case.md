# 508. 3c5e answered: the shader object is guest-adjacent - the cheap case, held ready

**2026-09-11** - loop tick; obSCEne answered REQ-...0930Z-3c5e within the hour

Both open orbistoun requests came back RESOLVED, and obSCEne's `20260911-002219-eboot` sweep carries
exactly the create-shader object-memory model 3c5e asked for. It resolves the hold's central
question - cheap vs new-subsystem - firmly to **cheap**.

## The measured model

```
arg0-addr        0x7eeffb4a8      (the out-slot; pointer-to-pointer)
arg1-addr        0x7eeffb4c0      (the header the guest supplies)
arg2-addr        0x6d2500         (the bytecode)
shader-obj-ptr   0x7eeffb4c0      == arg1        dist-from-arg1 = 0x0
                                  == arg0 + 0x18 dist-from-arg0 = 0x18
field-0x10       0x6d2500         == arg2        f10-dist-from-arg2 = 0x0
field-0x30       0x0
field-0x50       0x0
hdr-changed      0x2c bytes changed within a 0x130-byte object
```

**The shader object IS arg1** - the guest-supplied header region, not a driver-heap allocation. The
call writes `*arg0 = arg1`, then within that region sets `+0x10 = arg2` (a pointer to the passed
bytecode), `+0x30 = 0`, `+0x50 = 0`. Every value the write needs - `arg1`, `arg2`, `0` - is an
argument the handler already receives. So there is **no allocation and no new mechanism**: it is the
same "fill a region you were handed" pattern the Gnm builders and pad-reads already use, and the
current `args -> rax` `GuestFn` contract does it. Nothing in the denied loader/mem is touched.

## The implementation, specified and held ready

In `orbistoun-gpu` (`agc.rs` gains an `implementations()`, wired through `lib.rs`), a
`sceAgcCreateShader(args)` handler:

1. write `arg1` (u64) at `arg0`               - `*arg0 = arg1`, the object pointer
2. write `arg2` (u64) at `arg1 + 0x10`        - the bytecode pointer the guest reads
3. write `0` (u64) at `arg1 + 0x30`           - measured
4. write `0` (dword) at `arg1 + 0x50`         - measured
5. return `0`

All via the identity mapping (`with_exposed_provenance_mut`), the way `write_dwords` already does,
with a `SAFETY` note that arg1 is a guest-owned region of at least `0x130` bytes (the object extent
the guest allocates for the header). The other ~`0x14` of the `0x2c` changed bytes are unmeasured and
stay honest-unknown - only the three fields obSCEne measured are written. A guest that later reads an
unmeasured field faults honestly, and that names the next measurement.

## Why it is not written yet

The operator chose **B then A**: file 3c5e (done), then hold until the oops-sdk/obSCEne 3D work
settles. It has not - the operator's own obSCEne request is actively measuring `sceAgcCreateShader`
**stage differentiation (VS/PS/CS)**, `sceAgcDriverCreateQueue`, `SET_CONTEXT_REG` and hardware
draws, with sweeps landing seconds apart. So create-shader itself is still under investigation and its
object layout may refine per shader stage. The precondition for implementing is not met, so the
implementation is specified and held, not applied. (Two `cli learn` attempts to record this on the
knowledge entry were denied this tick - taken as "do not mutate knowledge during the hold"; captured
here instead.)

## Also this tick

- REQ-...1332Z-b7e2 (mapper-param) shows RESOLVED in the inbox - worth reading its resolution on the
  next active pass, though the mapper wall was the return-code-vs-fill finding of worklog 505.

## Next

- When the operator's stage-differentiation work settles (or on their go): apply the five-line
  handler above, then run PPSA28061/PPSA03416 to see the shader wall move past `sceAgcCreateShader`
  for the first time on measured data - the session's biggest available win, now cheap.
- Until then: hold. The model is banked; nothing is gained by building on a layout the operator is
  still actively measuring per stage.
