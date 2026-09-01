# 2026-08-19 - Concept intake and roadmap resequence


Documentation only; no code touched.

**Done.** Fourteen decisions captured from a planning conversation (D028-D041),
covering portable mode, configuration formats, the GUI, process architecture, and two
standing directives. Roadmap resequenced to add phase 0c (structural seams), phase 2b
(GUI shell and library), a stretch section, and a fuller phase 4. CLAUDE.md gained
principles 11-13. `ACKNOWLEDGEMENTS.md` created.

**The consequential one is D032** - the guest executes in a **child process**, not
in the shim. The deciding argument is address space: the guest demands fixed
addresses, and in a shim's process those compete with the UI toolkit, the graphics
driver, loaded DLLs, and ASLR, so a load failure would be *nondeterministic*. Thread
reclamation, clean restart, and fault-handler ownership all compound it. Cost is
deferred to phase 6, where output produced in the worker has to reach a window in the
shim.

**Surprises.**
- An initial in-process recommendation was a shortcut, caught only because the
  standing "no urgency, highest-payoff path" directive was stated explicitly. That
  directive is now D028 and CLAUDE.md principle 11 precisely because it changed an
  answer once already.
- The first thing that writes to disk is **GUI settings at phase 2b**, not traces at
  phase 4 - so `orbistoun-paths` had to move earlier than first planned. Worth
  re-checking that assumption whenever a new write appears.
- Batching decision-logging until "the end of planning" was the wrong call. Fifteen
  decisions accumulated in conversation before a prompt to verify caught it. Log as
  you go; the batch never feels urgent until it is lost.

**Next.** Unchanged - phase 0, with 0b and 0c available in parallel.

