# D156 - Nothing reachable from a guest call may panic


**decided** · 2026-08-20

A guest-facing implementation runs on a frame the guest entered through an
`extern "sysv64"` boundary. **Unwinding across that is undefined behaviour**, and it does
not present as a panic message - it presents as an unattributable fault somewhere in host
code, with a host instruction pointer the fault reporter cannot name and no relation to
the cause.

So the ordinary Rust reflex of letting arithmetic panic on overflow is wrong here, and
`u64::next_multiple_of` is the specific trap: it panics, it is the natural thing to write
when rounding a length up to an alignment, and **a guest is entitled to pass any value at
all** - including the all-ones word that some callers use to mean "no preference".

Every guest-reachable rounding is now checked, with `checked_next_multiple_of` existing
only to make the reason unmissable at each call site. Tests pass hostile values -
`u64::MAX` as a length, as an address hint, and `1 << 63` as an alignment - and assert a
refusal rather than a crash.

Worth stating as a rule rather than as a fix, because it applies to every implementation
this project will ever add and the failure mode is one of the least attributable available:
an index, a slice, an unwrap, a division. The stub layer's whole purpose is to answer
honestly, and a process that dies inside it answers nothing at all.

