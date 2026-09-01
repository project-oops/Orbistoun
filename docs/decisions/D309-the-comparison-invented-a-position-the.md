# D309 - The comparison invented a position the display half refuses to invent


**decided** · 2026-08-27 · D301's outstanding item, and worse than D301 described it

`fault: None` means a run did not fault. `compare` scored that as an ordering anyway:

```rust
(Some(_), None) => Some(Greater),   // this run faulted, the last did not
(None, Some(_)) => Some(Less),      // this run did not, the last did
```

So a run that **stopped faulting** - the wall the previous run hit now gone - reported
`BACK`, *reaching less of the interface than it did*. And a run that **started faulting** on
the same imports reported `FURTHER`, *executed code it could not reach before*. Both
sentences are false, and `FURTHER` is the one word this project steers by.

Thirty lines above, `describe_end` already says a missing fault "is not the same as saying
it died at address zero". The two halves of one file disagreed about the same absent value,
and **neither arm had a test** - so nobody had ever watched them decide anything.

A missing fault is not a position in either direction: ordering it against a real one
compares a number with its absence. Both arms now fall through to "does not compare", and
the interface count decides alone - which still measures something.

**And the saturation is now reported.** D301 said a run that ends without a fault "has
stopped measuring progress and started measuring nothing; reporting `FURTHER` from it reads
as confirmation", and left it unfixed. `Progress::ended_without_a_fault` carries it and the
verdict line says so, next to the existing intervention caveat - because both are the same
warning: *this number is not what you think it is*.

**What this cost is unknowable, which is the point.** Every previous run comparison where a
fault appeared or vanished was graded by a fabricated ordering, and the log has no record of
which those were. A verdict that is wrong in a recorded way can be re-read later; one that
was never asserted cannot.

