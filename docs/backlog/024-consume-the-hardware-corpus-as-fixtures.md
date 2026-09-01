# Consume the hardware corpus as fixtures and as behaviour


The reason to own the hardware. A remote probe answers "what does this return for these
arguments" as a *query* rather than a research project, and what comes back is the only
ground truth this project has for anything above the loader.

**This is the half that makes the emulator better**, which is why it comes before the
responder below. A recorded answer becomes two things: a test fixture, and the value an
HLE function actually returns. Neither needs a socket, the hardware, or a driver - the
records are files, and files are what CI can depend on.

**Blocked on obSCEne's record format existing**, and on nothing else. It needs no hardware
here: a captured session is enough to build and test against, which is the whole reason
to ask for captured exchanges alongside the spec.

**A record is one observation, not a rule.** A function returning zero for one set of
arguments does not mean it always does, and the corpus has to carry input, output,
firmware and the part that produced it so a later disagreement is legible rather than
mysterious. Anything from a stand-in is `assumed` - see below.

