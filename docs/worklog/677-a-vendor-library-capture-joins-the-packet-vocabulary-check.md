# 677. A vendor-library capture joins the packet-vocabulary check

**2026-09-17** — inbox `-2f28`. `tests/vocabulary.rs` checks `data/packets.toml` - transcribed, and its
own comment calls it the least certain thing in the crate - against captured builder calls. Both
captures it had were the GL cube's frame 30, written by oops-sdk's own GL path. A first-party writer's
expectations come from the same understanding that built the table, so they check the decode against
itself. A **vendor** library call that states its own register and value is a stronger check.

## The capture

`sceAgcDcbSetCxRegisterDirect(dcb, (0x12345678 << 32) | 0x200)` writes one `SET_CONTEXT_REG` packet -
`0xc0016900 0x200 0x12345678`, twelve bytes - and `tests/measured_builders.rs` already measures those
bytes beside the builder. Turned that measured pair into a third capture:
- `tests/captures/agc-set-cx-register-direct.hex`: the three dwords, little-endian, in the `.hex`
  text form (`read_words` parses eight-hex-digit words; the provenance guard refuses a `.bin`).
- `tests/captures/agc-set-cx-register-direct.toml`: `call = "sceAgcDcbSetCxRegisterDirect"` and one
  `[[register]]` expectation.

`vocabulary.rs` walks the bytes through `register_writes` against `data/packets.toml` and checks the
decoded write equals the expectation.

## The register the decode produces is `0xA200`, not the offset

`-2f28` phrased the expectation as "register `0x200`". `0x200` is the offset the call's packed argument
carries; `register_writes` decodes a `SET_CONTEXT_REG` to the *full* index, context base `0xA000` plus
the offset, `0xA200` - the same convention the GL cube captures use (`0xA318`, `0xA3B0`, ...), and the
same one worklog 675's pipeline test relies on. So the expectation is `register = 0xA200`, with the
offset noted in the capture's comment. Written the other way, `check()` would have looked for a write
to `0x200`, found the decode produced `0xA200`, and failed - which is the honest signal that `0x200`
was the offset, not the decoded register.

## Result

`every_capture_agrees_with_the_register_vocabulary` now reports **"27 expectation(s) verified across 3
capture(s)"** - three, not two, with the new one's `0xA200 = 0x12345678` holding. The acceptance is
met.

## Gate state

`orbistoun-gpu` vocabulary test passes (3 captures), full gpu suite green (10 binaries); `./bin/orbistoun
prose` exit 0; identity scan clean (the `.hex` is text, not a console dump). No commit.
