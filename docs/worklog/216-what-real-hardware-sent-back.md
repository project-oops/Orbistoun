# 2026-08-30 - What real hardware sent back


A parallel effort put obSCEne on a console and against PS5PCEM and returned a list of measured
format facts. Three were live defects here (D391).

**`PT_DYNAMIC` has no address on a real title.** It carries `vaddr 0` and sits at the tail of
`PT_SCE_DYNLIBDATA`, which is *also* at `vaddr 0`. Resolving it by address asks which segment
covers address zero, and two of them do - so the answer was header order: right by luck, or the
start of the vendor blob, which parses as a dynamic table of nonsense rather than as an error.
It reads `p_offset` now. The guard was watched failing on the old code first and failed with
exactly the predicted symptom.

**The same coincidence was hiding two more.** D247 established that `strtab` is an address for
an ordinary module and an offset into the vendor segment for a title, gave `table_offset` the
job of knowing the difference - and fixed one of the sites. Two others still resolved by
address and worked because the vendor segment sits at address zero.

**`sceKernelIsStack` knew about one stack.** It tells a stack address from a static correctly,
but only the main stack was recorded, so a guest thread asking about its own local was told no.
Same blind spot the argument dumps had this morning, in a different subsystem: a span that
comes into existence after the run starts and a table filled before it.

Two things it named were already right, which is worth writing down as clearly as the failures:
mutex recursion defaults to forbidden with a test, and `sceKernelLoadStartModule` refuses
honestly rather than answering a handle for a library that is not there.

The lesson attached to the list is one this repository keeps relearning: **name the layer that
failed, not the structure being sought.** Two of their hours went to a block-layer error
reported as a missing vendor table, and to an `e_type` complaint about a value that was
correct. That is the same finding as five separate ones here today.

Not taken: the migration to `selfish-elf`/`selfish-container`. Every one of these three defects
is an argument for it - they were all already fixed somewhere else - but selfish is being
actively changed by the effort that produced the list, and merging into a moving target is how
both copies end up wrong.

