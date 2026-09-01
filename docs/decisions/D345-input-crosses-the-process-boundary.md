# D345 - Input crosses the process boundary before anything can read it


**decided** · 2026-08-27 · an inconsistency pointed out, and it was right

The event queue was built with no deliverable events: raise, withhold, count, wait for a
measurement. Then the input transport was refused on the grounds that nothing on the far
side could consume it - which is the same shape and the opposite answer. Both are a mechanism
that is ours beside a payload encoding that is a measurement, and mixing the two is what
produces confident wrong answers. Either both are premature or neither is.

Neither. `Request::Input` carries pad state to the worker, `orbistoun_input::latest` holds
it, and `scePadReadStateExt` still refuses to write a structure nobody has measured.

**What it buys before that measurement arrives**, which is the part worth recording because
"it will be useful later" is not a reason:

- **`Focus` stops being a tested function with no observable effect.** What a title may see
  is decided in the window, and nothing downstream can widen it: the shell's own button is
  always stripped, and a title without focus is handed a pad nobody is holding rather than
  the last state it saw, held forever.
- **The transport has a property that can be asserted now** - what the window sent arrives
  intact - and it is asserted through the real protocol loop rather than by round-tripping
  the type, because the interesting failure is a message routed to the wrong place.
- **The gap is counted.** *"N pad update(s) arrived and none reached the guest - no measured
  layout to write one into"* is a transport waiting for something; silence would be one that
  looks broken.

Three details settled along the way. Input is a **level, not a stream**, so the latest state
replaces the previous one and an unchanged pad sends nothing - a queue would replay presses
that finished seconds ago, which is worse than losing them. An **absent port answers neutral**
rather than an error, because a title enumerating four pads with two configured should find
two nobody is holding, which is what a real machine with two controllers looks like. And
`Request` lost its `Eq` derive, which `Event` never had: a stick position is a real number
and there is no total ordering on those.

