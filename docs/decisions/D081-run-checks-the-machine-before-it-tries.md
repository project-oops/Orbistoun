# D081 - `run` checks the machine before it tries anything

**decided** · 2026-08-19 · at the user's direction

`doctor` is now a dependency of the commands that need a toolchain, not a thing to
remember to run. A missing requirement surfaces as one clear line before any work starts,
rather than as a build error three steps in that a reader has to trace back.

**Silent when it passes.** A full report before every debug run is noise, and noise
before every run is noise people learn to scroll past - at which point the check may as
well not be there. It speaks only when something is actually wrong, and then it says
what and where to go next.

**One list, two consumers.** The requirements live in a single function that both
`doctor` and the preflight read, so the verbose report and the blocking check cannot come
to different conclusions about whether this machine is usable. Two copies of that list
would eventually disagree, and the disagreement would be discovered by somebody whose
build was refused for a reason `doctor` said was fine.

`doctor` prints the exact install command for anything absent rather than naming the
tool and leaving the reader to work it out, and `doctor --fix` runs it.

**`--fix` covers the optional tools and the git hook, never the toolchain.** Installing
Rust is a machine-wide decision that belongs to the person, not to a script they ran to
ask a question. The distinction is between finishing a setup somebody already chose and
making that choice for them.

