# 531. The DCB-writer-handle request, and a second witness for the packet walker

**2026-09-14** - fresh obSCEne hardware captures landed (sweep 20260914-000606); acting on the GPU half

A three-leg hardware sweep arrived carrying section `166-agc` in full: the PM4 packet encodings for
the AGC command builders the retail titles wall on. Two things came out of it - one request, one
grounded increment.

## The request (obSCEne mesh)

Filed **REQ-20260913T2346Z-d3cb** into `C:\tmp\Obscene\worklog.md`: the layout of the *DCB writer
handle* the `sceAgcDcb*`/`sceAgcCb*` builders take in arg0. The sweep measured what each builder
**writes** (the packet bytes) and its **size** (the paired `*GetSize`), but not the handle it writes
*through* - where its cursor lives, how it advances, what the constant `0x200060078` return is. That
handle layout is the one thing needed to make the builders *callable*, and it is not in these captures
(the probe holds the handle in-process but did not dump it). Implementing the builder side-effect from a
guessed layout would be an invented struct, which principle 1 forbids - so the builders stay
declared-only until d3cb resolves. This is the same channel the create-shader object model (3c5e) and the
shader-linkage set (e4f1/9a41) came through.

`170-gpu-capture` (reading a live process's command ring) is confirmed **blocked by kernel hardening**, so
that is not a route to a real title command stream either; the builders are the way the stream gets built.

## The grounded increment (orbistoun-gpu)

What the captures *do* fully support: a second, independent witness for the packet walker.
`tests/measured_packets.rs` already held `packet.rs` against four command buffers from the
`run-native-title` capture, and noted its own weakness - "the multi-packet case rests on one capture."
The fresh sweep called the same builders with **different arguments**, so its buffers share the D565
buffers' opcodes and lengths but not their bodies. Added them as `CAPTURES_20260914` with a test that:

- walks each fresh buffer and asserts it consumes exactly (DmaData 28 B, ReleaseMem 32 B, WaitRegMem 56 B);
- decomposes the fresh `sceAgcDcbWaitRegMem` into the same **16 + 28 + 12** three-packet split with opcodes
  `[0x79, 0x3c, 0x79]` - from argument bytes that are all zeros where the first run carried live addresses.

That closes the single-capture gap: the length rule is now shown to be argument-independent, witnessed
twice across two hardware runs. The walker feeds the `capture_shaders` pipeline (worklog 530), so this is
the shader-address extraction's foundation getting firmer, not just a report detail.

## State

`orbistoun-gpu` green (31 unit + measured_packets 5 + pipeline 17 + capture 3 + others); fmt- and
clippy-clean on the touched file; identity guard clean. Nothing committed. The AGC Dcb builders remain
declared-only, now with a specific, acceptance-testable request (d3cb) standing between them and being
implementable - not a guess away from it.
