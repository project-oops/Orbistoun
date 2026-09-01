# 2026-09-01 - (/loop) Fault reports name the null base register automatically

The user asked: when a null-deref happens, can orbistoun do something useful automatically - watchpoint the
value, dump the callstack, name the culprit - rather than leaving a human to correlate? A corpus sweep first:
every title now walls on a guest dereference of a zero/wrong value it did not check (tlsf pool on
02664/03416/25872 per D449; a null base + offset on 04263; the assert-abort on 21564; GPU on 28061). None is
a missing HLE call, so the leverage is the report doing the origin-finding, not another shim.

The dump already had all sixteen registers (D230), but which one was the null pointer was left to the reader.
Added `null_base_registers` in `orbistoun-report::diagnose`: on a null-ish fault it names the register at or
below the null page whose value plus a field offset is the fault address, e.g.
`>> the null base is likely r12 (=0x0) - the access is r12 + 0x10, so find where r12 was set to zero`.
Exactly-zero bases win over near-null coincidences (04263's r14=0x10 was the stored value, not the base), and
it self-gates on faults far from zero. Three tests lock it (culprit named, coincidence excluded,
not-null-ish names nothing). Recorded D457; with D456 the report now says whose bug it is, what instruction
faulted, and which register was the bad pointer.

Deliberately still a follow-up: *where* the register was set to zero. The user's watchpoint idea (re-run with
a hardware watch on the register's source) is the next, bigger step - this names the value so that step has
something to watch. fmt/clippy/tests pass (orbistoun-report, 65). Additive diagnostic; no emulator behaviour
change.
