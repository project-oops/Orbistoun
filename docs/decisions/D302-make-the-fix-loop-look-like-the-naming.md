# D302 - Make the fix loop look like the naming loop: oracle first, generator second


**decided** · 2026-08-26 · because this project already solved this problem once

The naming loop brute-forces billions of candidates and has never produced a wrong name. Not
because the generator is clever - it is a dumb enumeration over a grammar - but because **the
oracle cannot be fooled**. A candidate either hashes to the import or it does not, and nothing
wrong survives contact with that.

The fix loop has the balance backwards. Its generator is careful - rules derived from
measurement, each one argued for - and its oracle is `FURTHER`, which answers "did the guest
get past something" and saturates the moment a run stops faulting (D301). A careful generator
guarded by a weak oracle is exactly the arrangement that produces confident wrong answers,
which is what principle 3 is about.

**So the order of work is oracle first.** `orbistoun-probe` already parses what the conformance
probe emits, and the probe grades **515 checks against a spec**, each announced by name.
`037-math/sqrt` passing means sqrt is *correct* - not that the guest survived it. That is a
fitness function, and it exists.

```
score = probe()
for each candidate:
    apply, score again
    keep if   nothing went pass -> fail
    and       something went fail -> pass
    else revert
```

Once that is the gate, widening the generator becomes **safe rather than reckless**, and the
generator can get dumber rather than smarter: enumerate the answer shapes a stub can take and
let the probe decide. That is the naming loop's shape, and it is the only arrangement this
project has ever been willing to put a machine inside.

**Two limits, and only one of them is permanent.** A function no check exercises has no oracle
and stays `assumed` - but that set shrinks every time obSCEne gains a check, and obSCEne is
ours. Real logic - a pseudo-random sequence, formatted output - is not expressible as data at
any vocabulary size and needs code, reviewed. The probe is also what says *which* functions
those are, rather than it being a matter of opinion.

