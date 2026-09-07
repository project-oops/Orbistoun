# D064 - A worker that dies without a verdict gets a postmortem

**decided** · 2026-08-19

Guest code faulting is the expected outcome for a long time yet, so the exit status of a
dead worker is a primary diagnostic rather than an edge case. The parent now synthesises
a verdict when the stream closes without one, naming the fault and reporting the
furthest phase already announced - "died having entered the guest" and "died while
parsing" are different problems.

This is why the entry phase is written and flushed **before** the jump. A phase recorded
only afterwards is lost when the process dies, leaving the parent unable to distinguish
"never entered" from "entered and died" - the single most useful thing it could know.

Fault names are a table, not a number: "access violation" says the guest dereferenced
something unmapped, while "breakpoint" says execution reached stub padding, which is a
different bug entirely.

**The breakpoint half of that was superseded on 2026-09-07 by D576.** The message named stub
padding for every breakpoint, from a table keyed on the exception code with no address in it,
so it asserted a cause nothing had checked. It is decided by the address against the stub
span now, and says which of the three things it determined.

**First real result.** All four commercial executables place, link, protect, enter, and
then fault with an access violation. Identical across all four, which makes it a
systematic missing piece rather than a per-title quirk - most likely the absent thread
pointer or the absent process stack image.

