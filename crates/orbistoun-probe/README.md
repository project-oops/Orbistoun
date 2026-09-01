# orbistoun-probe

Reading the records a hardware conformance probe produces.

**Models:** the line protocol, its record types, transcript and corpus parsing, and the
grading that says what a set of records actually establishes.

**Deliberately fakes:** nothing. It does not drive a session, open a socket, or know what
machine it is reading about.

**Design note.** A probe cannot certify its own machine: run inside an emulator it reports
the emulator's version as the platform's. So a `target` arriving on the wire is a **claim**,
not evidence, and nothing is graded above an assumption unless an operator asserts the
hardware on the command line. A corpus of assumptions is recoverable; a corpus of
measurements that were never measured is not.

**Built before there is any hardware.** The protocol ships with captured transcripts whose
stated purpose is that a consumer can be built and tested without hardware attached, and
that is what happens here - every fixture under `tests/fixtures/protocol/` is a real
exchange, and parsing all of them is the conformance test.

Those transcripts are **copied in as data**, never referenced across repositories. A test
that reads a sibling checkout fails for everyone who does not have one.

**Status:** the reading half is done and tested; nothing has been run against real hardware
yet, because there is none. `orbistoun-cli questions --json` is the other end of the same
pipe: the ranked list of what asking would settle.
