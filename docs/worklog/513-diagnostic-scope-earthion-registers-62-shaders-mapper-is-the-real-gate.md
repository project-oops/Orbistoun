# 513. Diagnostic scope: Earthion registers 62 shaders; the mapper is the real gate

**2026-09-11** - operator asked whether we can iterate now or need more data; answered by iterating

With `sceAgcCreateShader` implemented (worklog 512), a prop of the two calls Earthion then stalls on
- `sceAgcDriverRegisterOwner` and `sceKernelMapperGetParam`, both to `0x0` - scoped the rest of the
AGC setup chain without waiting for any new capture.

## What the prop revealed

`ORBISTOUN_RETURN=sceAgcDriverRegisterOwner:0x0,sceKernelMapperGetParam:0x0` on PPSA28061:

| | honest (create-shader only) | + owner/mapper propped |
|---|---|---|
| calls | 396 | **1080 (+684)** |
| distinct imports | 27 | **61 (+34)** |
| verdict | abort after mapper placeholder | FURTHER (diagnostic) |

Past the mapper, the guest does real work: **`sceAgcDriverRegisterResource` called 62 times**,
nothing implementing it. Its arguments name what it is:
- `arg4` = a shader **name** string: `"Agc::Toolkit::cs_set_uint_c"`, `"Agc::Toolkit::cs_set_uint4_c"`,
  ... - the AGC toolkit's built-in compute shaders.
- `arg2` = **RDNA2 bytecode** (`02 00 a0 bf 09 0c 04 7e ...`) - the same compute ISA obSCEne captured
  and orbistoun's shader decoder already handles (worklog 510).

So the setup chain is: `create-shader -> RegisterOwner -> mapper -> RegisterResource x62 -> ... ->
abort` (in a `printf`/`memmove` diagnostic, further in). The guest is registering the toolkit's
shader library before it can draw.

## The honest split: iterate vs need data

- **Diagnostic iteration works now.** The prop is the sanctioned 1-bit oracle (return Ok, does it
  proceed?), and it mapped the chain and named the next real function. This is legitimate progress on
  data in hand.
- **`RegisterOwner` and `RegisterResource` are AGC-driver-specific and return-checked** - propping 0
  advances the guest, so they are implementable as `guest-observed` once reached.
- **The mapper is the real gate, and it genuinely needs `7b3c`.** `sceKernelMapperGetParam` is a
  *general libkernel* call whose filled struct the guest reads (b7e2 case b, worklog 505). Blanket
  returning 0 without the real fill is the plausible-output failure principle 3 forbids, and it is a
  general kernel function, so a fake fill would leak into every guest that calls it, not just Earthion.
  Its measured post-registration fill is exactly `7b3c` item 3.

So: **we can iterate now to scope and to implement the AGC-driver-specific calls; advancing Earthion
on a faithful build past the mapper needs the one `7b3c` measurement.** Not a blanket "need more data",
and not a blanket "no data needed" - one specific measurement gates the real advance, and everything
around it is reachable.

## Consequence for the request

`RegisterResource` is now central (62 calls, the shader-library registration), not a footnote. Folding
it into `7b3c`: obSCEne should measure its return and whether it writes an out-parameter (it takes a
name + bytecode; it likely returns a handle/status). With owner + mapper + resource measured, the
whole setup chain implements as one pass and Earthion reaches the draw path.

## Next

- Extend `7b3c` to cover `sceAgcDriverRegisterResource` (name+bytecode -> ?).
- When the mapper fill lands: implement owner (guest-observed 0), mapper (measured fill), resource
  (measured), and re-run - Earthion should pass the 62 registrations and reach whatever draws.
- The e4f1 interpolant/prim/link functions are still further on, past the registration cluster.
