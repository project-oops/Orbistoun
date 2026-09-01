# A record becomes a knowledge entry, graded, or it does not become one at all


`Finding::knowledge()` renders a probe result as a `FunctionKnowledge` entry, and
`orbistoun probe <path> --as-knowledge` prints what would be written. Printed rather than
written: a corpus is evidence, and evidence being read is not evidence being believed.
Merging stays a separate deliberate act.

The conversion is held to the knowledge base's **own** validator - `provenance_faults()`,
the same function that fails the build for a hand-written entry - so a generated entry
meets exactly the standard a person's would.

End to end, from a session transcript:

    [[function]]
    name = "sceKernelOpen"
    edge_cases = ["returns 0x80020002 (pass)"]
    known_by = "measured"
    cites = "conformance probe, check 010-fs/open-missing on console firmware 13.520.001"

The identical records with `target|deck` produce `known_by = "assumed"`, no citation, and
the origin moved into an assumption reading *"observed on deck, which is not the target"*.

### Surprises

**A `assumed` grade may not carry a citation, and that rule bites exactly where it should.**
The knowledge base refuses a citation beside a guess, because one reads as evidence at a
glance. So a demoted stand-in measurement cannot cite the run it came from - even though
that run is known precisely. The information goes into the note and an explicit assumption
instead, where it reads as *why this is not settled* rather than as the authority for it.
Being able to say where a guess came from is useful; saying it in the field reserved for
established facts is how a guess becomes one.

**A report has no session, and therefore cannot be graded at all.** A committed report
carries no `hello` and no `part` - negotiation is a protocol thing. Its `build` record names
the *binary kind*, `module`, `payload` or `host`, not the machine. So nothing in a report
says which hardware produced it, and `--as-knowledge` refuses rather than inventing an
origin or omitting one. Found by running it and getting silence, which is its own small
lesson: printing nothing was indistinguishable from having nothing to say.

**The heredoc mangled Rust string continuations for the fourth time today.** Same trap,
same fix, and at this point the rule is simply: no multi-line Rust strings through a
heredoc, use the Write tool.

