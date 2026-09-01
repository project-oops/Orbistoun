# D178 - The mapping was the witness, not the culprit (resolves D157)


**decided** · 2026-08-20

Resolves D157, and supersedes it. `sceKernelMapNamedDirectMemory` was parked as an unexplained
regression; it was never the bug. Guest calls had been arriving on a misaligned stack since
the project began, every earlier import was small enough that the compiler never needed a
sixteen-byte stack access, and this was the first one that did.

With the entry convention corrected the mapping is on by default and is worth +8 imports
and +427 calls. The switch stays, because turning a subsystem off is a useful thing to be
able to do while bisecting.

The reason for parking it holds up in hindsight, and that is the part worth keeping: the
alternative on the table was forcing alignment in the trampoline, which would have made the
symptom vanish and left the real fault - affecting every run this project has ever made -
permanently hidden.

