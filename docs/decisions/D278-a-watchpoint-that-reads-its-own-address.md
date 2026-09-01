# D278 - A watchpoint that reads its own address traps itself


**decided** · 2026-08-25 · found by arming one and watching the run go quiet

The first watchpoint armed against a real title reported nothing at all - no hits, and not
even the fault the run had produced every other time. Armed on an address the guest never
touches, everything worked and the summary said `never touched`. So arming was sound and
**trapping** was not.

The handler reads the watched word, because *what the instruction saw* is half the answer.
With a read-or-write watchpoint the debug register is still live while the handler runs, and
x86 sets the resume flag only for instruction breakpoints - so that read is itself a watched
access, which traps, which runs the handler, which reads the word again. The process dies on
a stack that unwound nothing, with nothing said.

Worth stating plainly because the failure **looks like a result**: a diagnostic that reports
no hits and no fault reads as "the guest never touched it and never got that far", and both
halves of that are false.

The fix is a re-entrancy flag rather than disarming and re-arming around the read. Clearing
the control register from inside the handler means writing it back afterwards, and a path
that can fail halfway leaves a run with watchpoints that silently stopped working - the same
class of wrong answer one level down. A flag cannot half-succeed, and the nested trap is
still **ours**, so it resumes the guest rather than falling through to the next handler.

Two smaller things fall out and both are kept: the debug-status register is cleared on every
path that claims a trap, not only the recording one, so a swallowed nested trap cannot leave
a stale bit that attributes the next access to the wrong watchpoint; and the firing test
uses a read-or-write watchpoint rather than a write-only one, because write-only is the case
that would have passed while the hazard was still there.

