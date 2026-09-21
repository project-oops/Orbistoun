# 752. The probe question that checks the create_shader fix

**2026-09-21** — 751 widened `create_shader`'s relocation to the full group-pointer array on a sound
inference (an offset in a pointer slot must be relocated) guarded so it cannot misfire. Sound is not
measured, and this project's rule is that silicon settles a claim about silicon. This tick writes the
probe question that would settle it — the agent's job per THE_LOOP.md, which reserves *running* the
probe for a person — and states plainly where the loop stands on PPSA02664.

## The probe question

**Does `sceAgcCreateShader` relocate the group-pointer slots at `+0x18` and `+0x38` when they hold
relative offsets?**

Concretely, for a person driving obSCEne on hardware (the shape of `166-agc/create-shader`, which
already measured the middle three):

- Build a header object whose group array `+0x18..+0x38` is fully populated with small relative offsets
  — e.g. `+0x18=0xa8`, `+0x20=0x70`, `+0x28=0x38`, `+0x30=0x60`, `+0x38=0x58` — with `+0x08` a sub-table
  offset and `+0x10` a bytecode pointer, as the existing check does.
- Call `sceAgcCreateShader(out, header, bytecode, flags)` and read the header back.
- **The question is `+0x18` and `+0x38`**: does the call rewrite them to `header+0xa8` and `header+0x58`
  (relocated, as 751 now does), or leave them as `0xa8`/`0x58` (the pre-751 three-of-five)?

If hardware relocates them, 751 is confirmed and the `create_shader` list is correct at five. If it does
not, 751 is wrong and reverts to `[0x20, 0x28, 0x30]` — and the truth about PPSA02664's header would
then be stranger than "an incomplete relocation set", because its `+0x18` genuinely needs to be a
pointer for the workload walk to survive. Either answer is worth having; the guard means 751 is safe to
carry until it arrives.

## Where the loop stands on PPSA02664

The wall is diagnosed to the byte (748): a Unity-inlined shader loader relocates three of five group
pointers, leaving `+0x18`'s `0xa8` as the address the `memcpy` dies reading. Everything reachable
without hardware or new infrastructure has been reached:

- **Tooling** built: copy operands and the null-source finding (740, 741), the indirect peek that reads
  ASLR'd heap objects (743), the capstone disassembly of guest code (745), the frame walk to the
  descriptor's stable slot (747).
- **Ruled out**: the phantom (746), the AGC patch family (742), `sceAgcCreateShader` on this path (it is
  never called — Unity inlines it, 750), asset truncation (the offsets are all present, 750).
- **Blocked**: the construction watchpoint (allocation is non-deterministic, no determinism control,
  749), the magic scan (the loader copies rather than compares the magic, 749), and reaching the loader
  at all (it is guest-internal, and principle 7 forbids the hook that would catch it).

What is left needs one of two things this loop cannot do in a tick: a **hardware probe** (a person's
job, and the question above is ready for it), or **deterministic guest execution** so a watchpoint can
be armed on the construction (a real capability, but a build, and thread scheduling may resist it). The
honest state is that PPSA02664's own wall is parked on those, while the general `create_shader`
correction it motivated is landed and tested.

## Gate state

No code changed — a workflow check (THE_LOOP.md), a search, and this write. `./bin/orbistoun check`
unchanged from 751 (green but for the same three generated-doc drifts from a prior session's uncommitted
`compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No commit.
