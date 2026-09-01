# 2026-08-21 - The abort-at-53 is one bug, in two Unity titles


Narrowed, not solved. What is now certain:

**It is one bug, not two.** PPSA02664 and PPSA03416 produce the *identical* call sequence,
from the identical call sites, with the identical import set. Not merely the same engine -
the same runtime build. Anything that fixes one fixes both.

**They are Unity titles.** The rodata the guest points into is Unity's mono internal-call
registration blob (`UnityEngine.Audio.AudioClipPlayable::...` and neighbours). The abort
happens during C++ static initialisation, before any of the engine proper runs.

**Return values are ruled out.** With `default_return = "ok"` - every unimplemented function
reporting success - the guest reaches *exactly* the same 19 imports and 53 calls and aborts
in the same place. Whatever it is checking, it is not a status code. That is the most useful
thing established today about this wall, because it eliminates the entire class of fix that
had been assumed.

By elimination the cause is a **side effect that never happened**: memory a stub should have
written and did not. The same shape as D171, where an unwritten out-pointer had no signature
in a trace because nothing was written to recognise.

**The shape of the failure.** Four unnamed `libkernel` functions each take the same stack
buffer `0x600000800db8` in sequence, interleaved with a `libc` call carrying a fixed rodata
pointer; the whole block runs twice, once per static object. Then a fifth unnamed
`libkernel` function takes a *different* stack buffer and the guest aborts from inside the
C++ runtime region.

Also worth recording as a caution: the fixed pointer the guest passes resolves *mid-string*
in the rodata blob, so it is not a string argument however much the surrounding text invites
that reading. Noted before it became a confident wrong conclusion.

Next: name those five functions. The generator is the sanctioned route - proposing candidates
from what the context suggests is exactly the recalled-knowledge risk `known_by` exists to
catch (D180).

