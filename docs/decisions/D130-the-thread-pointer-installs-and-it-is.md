# D130 - The thread pointer installs, and it is checked rather than assumed

**decided** · 2026-08-20

The last outstanding piece of phase 4. Guest code reads thread-local variables through the
`fs` segment base; until that points at a real block, every `fs:`-relative access returns
whatever the host left there - not zero, not the guest's, and undetectable from inside the
guest.

**Verified working on this machine**: `wrfsbase` accepted, and the base reads back exactly.

Three things about how, each of which could have been done worse:

- **Emitted as raw bytes, not by mnemonic.** The intrinsic is unstable on this toolchain,
  and enabling the assembler feature to spell `wrfsbase` would let the compiler emit it
  anywhere in the translation unit. The encoding is fixed, so the bytes depend on nothing.
- **`CPUID` is necessary and not sufficient.** It says the processor *has* the feature;
  the operating system must also have enabled it, which user code cannot observe.
  Executing it when disabled raises an illegal-instruction fault - loud, and named by the
  fault reporter.
- **Every install is read back.** The write is one instruction with no result, so the only
  way to know it took is to look. A silent success that left the base unchanged would give
  a guest plausible, wrong thread-locals with nothing able to attribute the failure.

Where it cannot be done, `install` returns `Unsupported` and says which reason. Running a
guest anyway with a wrong thread pointer is precisely the plausible-wrong-answer that
principle 3 exists to prevent.

Not yet wired into the loader: no executable examined declares a single thread-local
relocation, so nothing needs it until threading - which is the next thing that will.

