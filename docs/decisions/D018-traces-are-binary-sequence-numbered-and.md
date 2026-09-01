# D018 - Traces are binary, sequence-numbered, and call-site attributed

**decided** · 2026-08-19

Text logging does not survive emulator call volume. Every event carries a global
monotonic sequence number (so a multi-threaded trace can be totally ordered after
the fact) and the guest return address (because "which call site" is the useful
question - the same stub called from two places is usually two different bugs).
Recording must not allocate.

