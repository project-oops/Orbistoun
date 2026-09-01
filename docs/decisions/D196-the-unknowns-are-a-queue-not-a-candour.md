# D196 - The unknowns are a queue, not a candour exercise


**decided** · 2026-08-24

Sixty-seven `assumptions` lines sit across the knowledge files. Individually each is an
honest admission; collectively they were **unreadable as a body of work** - scattered per
function, in no order, with no way to ask "what is worth measuring first".

`orbistoun-cli questions` gathers them and ranks them by **how often a guest actually calls
the function**. Alphabetical is the same as unordered, and the difference is stark: the top
entry is `sceKernelDirectMemoryQuery` at 87.6 million calls, and the tail is functions
nothing has ever reached.

### Why this is the handoff and not a report

A conformance probe on real hardware can answer these, and until now there was nothing for
it to work from. The choice was between a probe author reading the knowledge files by hand
and a probe consuming a ranked queue; only one of those survives contact with a thousand
exports.

`--json` exists for that consumer. `orbistoun-probe` already reads the probe's own protocol
and maps a hardware answer to `Oracle::Measured` - the return path was built before the
outbound one, which is the right way round but left the loop open at the near end.

### Each entry carries its shape, and that is the point

`returns` and `arity` were recorded for trace fidelity. They are also **the dispatch key for
a property**: everything answering a handle can be asked the same questions, and so can
everything answering a count. A probe generates tests from the shape without knowing the
function, which is what makes broad coverage tractable - a dozen property templates over
sixty-six functions rather than sixty-six bespoke tests.

Four kinds are recorded across the base: 39 `status`, 16 `pointer`, 9 `count`, 2 `handle`.
Fifteen entries have **no** recorded kind, and the queue prints `shape unrecorded` for them
rather than omitting them - a function no property can dispatch on is itself a gap, and
hiding it would make the coverage look better than it is.

### What it is not

Not a claim that these are all the unknowns. It is every unknown somebody **wrote down**,
which is a strictly smaller set - and the number is expected to rise as more is noticed
(D180). A queue that only falls is measuring candour rather than knowledge.

