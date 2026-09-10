# 500. The frontier, fully mapped, and the two things that block it

**2026-09-10** - loop, continuing 499

A tick spent not writing code but establishing, precisely, that there is no accessible code to
write. Worth recording because "the wall did not move" is a different fact from "the wall cannot be
moved from here", and this is the second.

## The conformance frontier is three checks, all blocked

Against the matching hardware leg (`20260909-110725-eboot`, `title/unknown-gpu`), under the
reference profile, orbistoun passes 176 to hardware's 167. The checks where **hardware passes and
orbistoun does not** are down to three, from ten at the start of the session, and each is blocked:

| check | why orbistoun does not pass it | blocked on |
|---|---|---|
| `900-surface/control` | packaged `PPSA99980` carries no weak symbols; its census control is a GLOBAL import orbistoun stubs | **expected** - SELFish D101 decided a built module cannot represent a weak import; payload-only by construction, and it *does* pass on `obscene-payload` |
| `080-video/visual-flip` | no scanout | the GPU arc (obSCEne `8ef4` full captures) |
| `111-modlink/walk` | no `DT_DEBUG` -> `r_debug` link-map in guest memory | the **loader directory**, denied to this session |

The first is not a gap - both sides agreed it. The other two are each blocked on a specific,
external thing.

## The nine where orbistoun is *too* permissive are not tractable either

orbistoun passes nine checks hardware does not. Seven are `hw=skip` - hardware never ran them, so
they are not divergences. The two real ones are `hw=fail`:

- `035-libc/wide-strings`: the console's `wcslen` returns **1** for a four-wide-char string - broken
  per the C spec. orbistoun's is correct. Matching hardware means replicating a failure whose
  mechanism (2-byte vs 4-byte `wchar_t`, or a genuine libc bug) is unmeasured, and hardcoding
  `return 1` without it is inventing a bug - the plausible-output trap, principle 3.
- `107-videodec/library`: hardware fails because the library is absent in that context, not because
  of a bug to copy.

Both need a hardware-mechanism explanation before a fidelity fix would be honest, not code.

## The title walls are the same two blockers

- Four retail titles (three Unity, Earthion) stop at `sceAgcCreateShader` - the GPU wall, obSCEne
  `8ef4`.
- GTA V traps on `int 0x41` after an upstream internal call fails; ASTRO BOT null-derefs after a
  failed lookup. Both are deep traces into the title's own code, and the interrupt path is
  loader/engine territory.

## So the loop is genuinely at a boundary

Every remaining forward move needs one of exactly two things:

1. **obSCEne's full GPU captures** (`8ef4`) - a whole command buffer and a whole shader, which flow
   into the two decoders this session already hardware-anchored (worklog 499). This is the corpus
   frontier.
2. **The loader directory** (`crates/orbistoun-loader/src/`), denied to this session - which
   `111-modlink/walk`, the `unknown-gpu` context label, and the interrupt path all sit behind.

Neither is a matter of effort here. The session's accessible work is done: D667-D675, both GPU
decoders confirmed against real hardware, conformance divergence cut from ten to three-all-blocked,
and the data requests filed. Further ticks are quiet holds until obSCEne delivers `8ef4` or the
loader deny is lifted.

## Next

- On new obSCEne data: wire a full capture into `packet::walk` + `orbistoun-shader` (the decoders
  are ready) - REQ-26aa piece 1, now worth building because the data would exercise it.
- If the loader deny lifts: `DT_DEBUG`/link-map for `111-modlink/walk`, which also fixes the
  context label and unblocks `modvaddr`.
- Otherwise: a deliberate deep trace of GTA V's upstream failure or ASTRO BOT's null, which are the
  only unblocked moves and are multi-hour with uncertain payoff.
