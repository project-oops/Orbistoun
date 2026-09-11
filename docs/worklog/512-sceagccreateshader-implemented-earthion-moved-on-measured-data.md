# 512. sceAgcCreateShader implemented from the measured model - Earthion moved, for real

**2026-09-11** - operator lifted the "A" hold and said build it (1 then 2 then 3)

The first orbistoun GPU-HLE implementation grounded end to end in hardware measurement, and the
first retail title advanced on it rather than on a diagnostic prop.

## What landed

`crates/orbistoun-gpu/src/agc.rs` gains `sceAgcCreateShader` (registered via `agc::implementations()`,
wired in `orbistoun-service/src/symbols.rs`). It writes exactly the 3c5e object model and nothing
invented:

- `*out = header` - the shader object **is** the guest-supplied header region (guest-adjacent, no
  allocation; `shader-obj-ptr == arg1`, distance 0)
- `object+0x10 = bytecode` (arg2 pointer), `object+0x30 = 0`, `object+0x50 = 0` - the three fields the
  guest reads at the D556 fault site
- returns `0`; a null `out`/`header` gets the wrapper's measured `0x8a6c000a` rather than a fault

The other bytes of the `0x130`-byte object stay as the guest left them - nothing measured says what
they hold, and inventing them is what principle 3 forbids. Two unit tests pin the fill and the null
refusal; `orbistoun-gpu` is green (19 lib + 4 vocabulary), and the `packet.rs` decoder tests still
pass.

## The wall moved, measured, not propped

PPSA28061 (Earthion), honest run, no diagnostics:

| | before | after |
|---|---|---|
| calls | 334 | **396 (+62)** |
| distinct imports | 26 | 27 |
| verdict | abort at create-shader placeholder | **FURTHER** |

The trace now shows `sceAgcCreateShader -> 0x0` succeeding, and the guest runs 62 calls further into
its own startup before the next wall. This is the create-shader wall genuinely retired - the win
worklogs 505-508 were held for, now banked on measured data.

## The next links, exactly as b7e2 predicted

Earthion now aborts after this chain:

```
sceAgcCreateShader                                    -> 0x0          (implemented)
sceAgcDriverRegisterOwner (0x600000800e8c)            -> 0xf7ff0001   (placeholder)
sceKernelMapperGetParam   (0x600000800e20)            -> 0xf7ff0001   (placeholder)
libc::abort
```

So the AGC-driver **context** is the remaining wall, one coherent problem as 509 called it:
`sceAgcDriverRegisterOwner` sets it up, `sceKernelMapperGetParam` queries it (b7e2 case b: the mapper
is AGC-linked, resolvable only once the driver owner is registered). Also newly reached and
unimplemented: `sceAgcDriverQueryResourceRegistrationUserMemoryRequirements` (a size query) and an
unnamed `libSceAgc::0x53bbd82b51d172db`.

None of these has a measured behaviour yet - 3c5e gave the shader object, e4f1 gave the *later*
interpolant/prim/link functions, but the **owner registration and the mapper fill** are unmeasured.
So the honest next step is the 3c5e move again, for the next links: filed to obSCEne.

## Method note

This is the loop working as intended: implement the one precisely-measured call, run, read the next
call the guest makes, and let *that* say what to measure next - rather than implementing the whole
e4f1 list blind against argument layouts a run has not confirmed. The interpolant/prim/link functions
(e4f1) sit behind the owner/mapper context, so the guest has not reached them yet; they are built when
it does.

## Next

- Filed the owner/mapper/resource-query behaviours to obSCEne (the next 3c5e-style measurement).
- Then tasks 2 (clear the red gates: prose + inspect_test) and 3 (trace writeout), per the operator.
- When owner+mapper land, Earthion should reach the e4f1 interpolant/prim/link calls, and the AGC
  context closes as one pass.
