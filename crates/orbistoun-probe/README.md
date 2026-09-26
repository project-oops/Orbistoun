# orbistoun-probe

Reading the records a hardware conformance probe produces.

It holds the line protocol, its record types, transcript and corpus parsing, and the grading
that says what a set of records establishes. It does not drive a session, open a socket, or
know what machine it is reading about; `orbistoun-cli session`, `ask` and `probe` use it, and
`orbistoun-cli questions --json` is the other end of the same pipe: the ranked list of what
asking would settle.

## Rules

- **A reported target is a claim, not evidence.** A probe cannot certify its own machine: run
  inside an emulator, it reports the emulator's version as the platform's. Nothing is graded
  above an assumption unless an operator asserts the hardware on the command line. A corpus of
  assumptions is recoverable; a corpus of measurements that were never measured is not.
- **Fixtures are real exchanges.** Every fixture under `tests/fixtures/protocol/` is a captured
  transcript, and parsing all of them is the conformance test, so the consumer is built and
  tested without hardware attached.
- **Transcripts are copied in as data**, never referenced across repositories. A test that
  reads a sibling checkout fails for everyone who does not have one.
