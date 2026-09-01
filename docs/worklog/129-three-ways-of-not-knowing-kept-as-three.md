# Three ways of not knowing, kept as three


The network handover gained a `sysinfo` block: the target's account of itself - memory,
VRAM, generation, firmware - one record per field, shaped `field | state | value`. Parsed,
displayed, and kept away from grading.

**The state is the record.** All three states can carry the value `unknown` and they are
three different findings: `absent` is a platform gap, `unconfirmed` is the probe's own
unfinished wiring, and `known` is a real reading. Only one of them is anybody's bug, and a
consumer that collapsed them would show one blank where there are three.

Modelled as an enum with a fourth variant, `Unrecognised`, that resolves to none of the
others. A state this version has never seen cannot be read as `absent` - that blames the
platform for something it may well do - nor as `known`, which would treat unknown confidence
as a reading. It stays unrecognised and the decision surfaces.

**And none of it reaches a grade.** Inside an emulator every field answers as that emulator
chooses, so `memory|known|441M` is that emulator's number wearing the target's badge - the
self-reported-firmware trap one layer along. The separation is structural: nothing in
`Origin` is reachable from a record, so it cannot be wired up by accident later.

### Surprises

**The document does the thing it warns against, once.** It argues at length that collapsing
the three states throws away the only distinction worth showing - then says `generation`
reads `unknown` both when *neither* graphics driver resolves and when *both* do. Those are
not the same fact: both-resolved is a positive fingerprint of a stub-everything loader, which
is a real finding about what is on the other end, and neither-resolved is an absence. They
produce identical output today. Raised upstream.

**`sysinfo|listening|0.0.0.0:9803` is not the target's self-report.** It is the server's own
state, sitting in a record kind framed as "what this machine is". A display rendering the
block faithfully puts a bind address next to VRAM. Minor, and it follows from the framing
rather than from a mistake.

**Parsed it and nearly left it unreachable again.** The records were read and tested with
nothing displaying them - the third time this session a library has been finished without a
caller. Their own checklist says to *show* it. Caught before logging this time rather than
by running the tool a day later.

