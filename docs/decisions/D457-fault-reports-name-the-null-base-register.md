# D457 - Fault reports name the null base register, not just dump sixteen


**measured** - 2026-09-01 (user-directed /loop; the user asked for something that, when this class of fault happens, does the origin-finding automatically instead of leaving it to a reader)

A corpus sweep found every title now walls the same way: a guest dereference of a value that was zero
(or wrong) and unchecked - PPSA02664/03416/25872 on the tlsf pool (D449), PPSA04263 on a null base plus a
field offset, PPSA21564 on its own assert-abort. None is a missing HLE function; each is a guest-computed
value the load-transformed eboot hides from file-RE (D455). So the leverage is not another shim - it is making
the fault report do the origin-finding.

The register dump already carries all sixteen (D230), but *which* one was the null pointer was left for a
reader to find by matching values against the fault address - the manual step that turned PPSA04263's
`mov [r12+0x10], r14d` into a correlation exercise. The report now does it: on a null-ish fault it names the
register at or below the null page whose value plus a field-sized offset is the fault address -
`>> the null base is likely r12 (=0x0) - the access is r12 + 0x10, so find where r12 was set to zero`.

Two things keep it honest rather than noisy:

- **Exactly-zero wins.** A register holding the *stored value* can match the same arithmetic by coincidence
  (PPSA04263's `r14 = 0x10` matched the address with a zero offset). A textbook null dereference has a base
  of exactly zero, so when any register is zero those are named and the near-null coincidences are dropped.
- **It self-gates.** A fault far from zero names nothing: the base must be below the null page and the offset
  within a field of it. Three tests lock this - the culprit named, the coincidence excluded, and a
  not-null-ish fault naming nothing (the guard made to fail before being trusted, per principle 3).

With D456 (privileged instructions named, emulator-vs-guest called out) this closes most of the gap between
"here is a fault" and "here is the function to fix": the report now says whose bug it is, what instruction
faulted, and which register was the bad pointer.

**What is deliberately still a follow-up.** The register is named; *where it was set to zero* is not yet
answered automatically. The user's larger idea - re-run with a watchpoint on that register's source and catch
the write of zero - is the next step and a bigger one (it needs guest re-execution under a hardware
watchpoint, or backward dataflow over the captured instruction window). This decision is the foundation it
builds on: you cannot watch the origin of a value until you have named the value.

`fmt`/`clippy`/`cargo test` pass for `orbistoun-report` (65 tests). Pure additive diagnostic - no behaviour
change to the emulator itself.
