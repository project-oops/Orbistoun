# D054 - Distribution formats stop at the door; mounts are ordered

**decided** · 2026-08-19 · observed from real material

**orbistoun never reads a distribution container.** Package files and disc images are
both encrypted, both need keys to open, and both are ruled out by D014. On console
they are decrypted and installed; what the guest actually sees is a directory tree
mounted at `/app0`. That tree - executable, bundled modules, `sce_sys/`, assets - is
the only form orbistoun accepts, and it is what `titles/` holds.

So the packaging question has no emulator-side answer: the two formats converge on the
same directory tree before anything reaches us.

**What is a real emulator concern is mount layering.** A patch installs *over* the
base, and the runtime sees a merged view in which patch files shadow base files. The
title inspected here happens to be add-only - its `update/` carries archives and DLC
packs but no executable - but a patch **can** carry a new executable, and then the
patch's copy is the one that runs. Loading the base executable when a patch supersedes
it would mean emulating a version nobody plays, and the symptom would be baffling:
correct-looking behaviour that does not match any real installation.

`orbistoun-fs` therefore needs **ordered mounts**, not a single root directory, and
title identity (D048) must hash whichever executable actually wins the merge.

