# D427 - The GPU command builders and the packet walker share one packet format

**Status:** decided
**Date:** 2026-09-01

The GPU command builders, which write command packets into a caller's buffer, and the packet
walker, which decodes a submitted buffer, are built against one shared packet-header format, so a
packet built here walks back to the packet it stood for.

**Why:** The GPU command-stream translation this project exists to do needs both directions - a
guest hands the loader packets to interpret, and the loader must in places hand a guest packets
back (a default hardware-state block, a dispatch). One shared format keeps them from drifting
apart and lets a test pin the round trip directly, rather than trusting two independent encodings
to agree.

**Rejected:** a separate ad hoc encoder for the write side - nothing then confirms it agrees with
what the reader expects.
