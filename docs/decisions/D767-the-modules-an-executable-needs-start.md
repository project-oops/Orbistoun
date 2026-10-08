# D767 - The modules an executable needs start before its entry

**Status:** decided
**Date:** 2026-10-08
**known_by:** published (the System V ABI's initialisation order), and assumed for the reload answer

Before the executable's entry, the modules it lists in `DT_NEEDED` that the title ships are
started: their `DT_INIT` and `DT_INIT_ARRAY` run. Each module starts after the shipped modules it
needs itself, depth first in `DT_NEEDED` order; a cycle starts each member once. A needed name
matches a shipped module by its stem, case-insensitively. A module also needs every shipped module
its imports were bound into, whether or not it names it: PPSA28061's `libSceNpCppWebApi` names the
system's `libSceLibcInternal`, and its heap calls bind into the title's own `libc.prx` instead. A shipped module nothing needs is not
started until the guest starts it (D515).

A module started this way has a handle. A later `sceKernelLoadStartModule` of its path answers that
handle and runs nothing again.

**Why:** the title's own `libc.prx` sets up its heap in an initialiser, and the platform's runtime
linker runs it before the executable's code. Orbistoun started a module only when the guest named
it, and nothing names libc. Its heap stayed zero, and the first allocation inside it answered null.
PPSA02664 lists il2cpp first and libc last, and il2cpp needs libc, so the order matters: libc
starts first. Started this way, the title runs as far as `ORBISTOUN_START_MODULES=all` took it:
1.475M calls, into level loading. That diagnostic also ran il2cpp's constructors twice, because
the title loads and starts il2cpp by path as well.

**Assumed:** that loading and starting a module already started answers its handle without
starting it again. The runtime linker counts references to a loaded object rather than loading it
twice, and the other reading, running constructors twice, corrupts static state.

**Rejected:**
- Starting every placed module, as the diagnostic does: a plugin the title loads later would start
  before the title's own setup.
- Placement order: it is the order files were found, not the order the modules depend on each
  other.
