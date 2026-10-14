# D832 - The loop resolves waits, and stages the patches that move a wall

**Status:** decided
**Date:** 2026-10-14

A run whose time goes to one waiting call - a yield, a sleep, a poll, a wait that times out and
is retried - has a wall that is not the call's answer. The loop treats it as a waiting thread and
resolves it in four mechanical steps, none of which decodes guest code (D276):

1. **The wait.** The run names the thread, the call site and the call, and what the thread's
   callee-saved registers held at it. Those registers hold the objects the waiting loop reads.
2. **The word.** A snapshot of those objects, taken at successive iterations of the wait, names
   the words that never change. Read watchpoints on them name the word the loop reads. The words a
   stopped GPU queue never wrote - its held `RELEASE_MEM` and `WRITE_DATA` destinations - are
   watched after them, since a thread waiting on the GPU polls a fence in no object it holds.
3. **The writer.** A write watchpoint on that word says whether anything writes it, and which
   thread. A word nothing writes is a completion that never came.
4. **The cause.** The word's object is joined to the run's calls: an import handed that object,
   a callback in it, or a pointer into it is the call that was to complete it. Unimplemented,
   placeholder and refused calls rank first.

A wait that moves under `ORBISTOUN_CLOCK=host` and not under the logical clock is reported as a
clock finding: the waiting thread's logical time does not advance, whatever it waits on.

A patch the loop proposes for a wall - a policy entry (D296) - is trialled on the title it came
from and then run across the corpus. One that moves its title `FURTHER` and moves no other title
`BACK` is staged: written into the tracked knowledge file as `known_by = guest-observed`, with the
run that moved it as its evidence, ready for the gate. A census row asking the hardware the same
question is filed with it, and a measurement that contradicts it retires it. A patch that fails
either check stays a candidate in `learned.toml`, which nothing reads by default.

Every diagnostic these steps use is a command or a report section of orbistoun's own, reused by
every title. A question answered once with a script is answered next time with a command.

**Why:** a dominant waiting call ranks first under "the guest is repeating it", and its
suggestion - vary the answer - is wrong: a yield's answer is not what the guest waits for.
SuperTuxKart's menus ignored the pad because a 0.25 s input timer never ran out on a thread whose
logical clock did not advance; Grand Theft Auto V's main thread yields until a completion
callback sets a byte. Both were found by hand, with scripts that recovered symbols, read memory
at the end of a run and searched for stores, and each step was mechanical. A proven patch that
waits for a person to copy it out of a candidate file is a wall moved once and forgotten; staging
it, labelled for what it is and with its measurement requested, keeps it.

**Amends:** D322 - a policy patch that passes its trial and the corpus check is staged without
waiting for a person; it is committed through the gate like any other change. Rust is still
written and merged by a person.

**Rejected:**
- Decoding the waiting loop to name the word it reads: faster, and it would find writers that
  have not run, but it puts a guest-code decoder in the tree, which D276 and the provenance rules
  refuse.
- Staging on `FURTHER` alone: a stub answering success moves walls; the corpus check is what
  catches a patch that helps one title by breaking the assumption another rests on.
- Recording a staged patch as `measured`: the guest accepting an answer is not the hardware
  giving it.
