# 499. The corpus is GPU-bound, and a real header agreed

**2026-09-10** - loop, continuing 497

Re-swept every title after D670-D676. The walls have settled into one shape, and it is graphics.

## Where every title stops now

| title | wall | why |
|---|---|---|
| PPSA02664, PPSA03416, PPSA25872 | `sceCommonDialogInitialize` fails, guest exits | D670 honest failure; the call behind it is `sceAgcCreateShader` |
| PPSA28061 (Earthion) | `abort`, right after `sceAgcCreateShader -> 0xf7ff0001` | gives up on shader creation |
| PPSA04263 (GTA V) | `image+0x2bfab2f` | not yet diagnosed |
| PPSA21564 (ASTRO BOT) | title's own modules+0x7af792 | faults in its own code |
| obscene, obscene-payload | run to completion | the probe, not a retail title |

**Every retail title that gets past initialisation stops at `sceAgcCreateShader`.** It returns the
unimplemented placeholder, and the guest either checks it and gives up or dereferences it and
faults. The GPU layer is the corpus-wide frontier now, and it cannot be reverse-engineered from
self-generated fixtures - it needs real command streams and real shader bytecode.

## obSCEne started producing exactly that

`reports/hardware/payload-klog.obs.log` carries a new section, `170-gpu-capture`: it walked the
running compositor's (`AgcCompositor.elf`) GPU memory and captured a live PM4 command header, a
command-stream window, and an RDNA2 shader window ending in `s_endpgm`. Proof-of-concept sized -
64-byte windows, not whole buffers - but real.

**The header paid for itself immediately.** obSCEne's own C reader decoded `0xc0599328` as `it_op
0x93`, `payload_dwords 0x59`. Fed to orbistoun's packet decoder - transcribed from AMD docs and,
per its own `vocabulary` suite, never once checked against live memory - the two **agree exactly**:
type 3, opcode 0x93, 90 body dwords. That is now a committed cross-implementation test,
`measured_packets.rs::a_live_memory_pm4_header_decodes_as_obscenes_own_reader_did`. Two independent
readers agreeing on live-hardware bytes is the strongest confirmation this layer can get.

Opcode `0x93` is named by neither reader - an AGC opcode with no reference to hand - so it stays a
number. Inventing a name would be cosmetic and is the kind of plausible output the file's own note
warns off.

## Stuck, and the request that says on what

The header is four dwords of a ninety-dword packet, and the shader window is 16 bytes of a
~0x4a000-byte shader. The confirmation is real but the *body* - the register writes, the shader
addresses, the ISA the shader is made of - is what nothing here can reach any other way, and it is
precisely what `sceAgcCreateShader` reverse-engineering needs.

So the GPU wall is blocked on data, not on effort, and the honest move is to say so. Filed obSCEne
`REQ-20260910T1220Z-8ef4`: extend the capture to a *whole* command buffer and a *whole* shader from
one frame. The proof-of-concept works; what is missing is size.

## And the shader decoder too (a second tick, same theme)

With the GPU wall still blocked on data - obSCEne's `8ef4`, `a3f7`, `d1c4` all still open, no new
hardware sweep - the productive move was the shader-side analog of the header check. obSCEne's
capture reports `endpgm-word 0xbf810000` (its own scan for the program terminator) and a 16-byte
window of the live shader. orbistoun's RDNA2 decoder agrees `0xbf810000` is SOPP opcode 1
`s_endpgm`, and decodes the window's two complete instructions - an SMEM load and an SOPP
`s_waitcnt` - from bytes read out of a running compositor rather than a fixture. Pinned as
`orbistoun-shader/tests/measured_shader.rs`.

So **both halves of the GPU-analysis stack are now hardware-anchored for the first time**: the PM4
packet decoder and the RDNA2 shader decoder, each confirmed against bytes obSCEne captured from
live memory. Neither had ever been checked against real hardware before this pair of ticks. When a
full command buffer and a full shader land (`8ef4`), they flow into decoders already known-good on
their first bytes.

## Consumed this pass

- **SELFish `5d20` (decided):** a module built by their or obSCEne's toolchain cannot represent a
  weak undefined import - it is rejected at build, not stripped - so packaged `900-surface/control`
  is payload-only by construction, confirmed both sides. They flagged the real unmeasured hinge:
  what a *real loader* does with a GLOBAL import whose NID no loaded library exports (null, which
  would read absent, or fault). orbistoun stubs it; that may not match hardware, and it is a
  hardware question, not a binding one.
- **Prosperous `3b7f` (would adopt):** yes to a shared `oops-fetch` for the policy, with two
  invariants relayed into oops-libs `9c22` - refuse-before-fetch on a missing digest, and
  verification inside the policy not the caller's closure.
- **obSCEne `e5d9` (delivered):** `obs_run_context` now keys delivery on the bootstrap path, so
  orbistoun stops being misclassified as a payload.

## Next

- A full command buffer or shader from obSCEne (`8ef4`) turns `packet::walk`, `registers` and
  `orbistoun-shader` from self-tested into hardware-verified in one step.
- GTA V's `image+0x2bfab2f` is now diagnosed and it is **not a shim**: the faulting instruction is
  `int 0x41`, a software interrupt orbistoun does not intercept - the guest has gone under the
  library boundary (D378), and it trapped *because* an upstream call returned failure. Moving it is
  engine-level interrupt/trap dispatch, or tracing back to the upstream failure, not an import.
- ASTRO BOT's fault is diagnosed too and is also deep: `cmp [rdi+0x38], rcx` with `rdi` ~null,
  after a run of `strcmp` lookups - a not-found lookup returned null and the title's own code read
  field 0x38 of it without a check. The D670 shape (a bad value used later) but inside the title,
  so moving it means tracing which value became null, not writing an import.

**So the frontier is genuine.** Every furthest-reaching title needs either GPU work (blocked on
obSCEne's `8ef4` full captures) or engine-level trap handling. There is no quick shim that moves a
wall from here - which is itself the finding, and the reason this pass filed a data request rather
than forcing an implementation.
