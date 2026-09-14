# 556. Modules declare the sixteen-bit capabilities only where they use them

**2026-09-14** - orbistoun-translate, after worklog 554

Worklog 554 ended with a device asking for `shaderInt16` and `shaderFloat16` because every
module this project emits declared those capabilities. That was the cheap way to silence two
validation errors; the better answer is that a module should declare what it uses. It does now.

## What was wrong with declaring them always

A capability is a claim about what the module needs. Declaring `Float16` and `Int16` in every
module says every module needs sixteen-bit arithmetic, which:

- makes every module invalid on a device that does not offer those features, whether or not it
  contains anything sixteen-bit - and plenty of hardware does not offer them;
- forces the session to request both features for the sake of modules that do not use them;
- and hid the fault, because on a device that supports both, nothing complains until somebody
  runs a validator. Which is how it survived every compute dispatch the suite has ever run.

Exactly one path reaches those types: a typed buffer load whose format carries a half channel,
where the field is narrowed to sixteen bits, read as a half and widened by the driver's own
conversion.

## The fix is the one the module already used for its extended instructions

`glsl_set` imports `GLSL.std.450` on first use and caches the id, so a module that uses no
extended instruction emits no import. The sixteen-bit types now work the same way: `f16_type`
and `u16_type` declare the type **and its capability** the first time something asks, and cache.

That works because of how the builder is arranged rather than by luck. Its header is kept in
ordered slots by opcode, so a capability declared late still lands in the capability section
ahead of the memory model; and its declaration section preserves order, so appending a leaf
type at the end depends on nothing. Both properties were already there, both were written down
where they are enforced, and neither had to change.

The two accessors take `&mut self` now, which is the whole ripple.

## What it is worth

The GL cube's translated pixel shader went from 72,840 to 72,796 bytes - two capability
instructions and two type declarations, gone from a module that never touched a half. More to
the point, that module no longer requires two device features to load, and neither does any
compute dispatch in the suite.

The session still asks for both features where the device offers them, because the modules that
*do* read halves still need them, and a device without them still cannot run those. The comment
that justified the request says which modules those are rather than claiming it is all of them.

## The test, both ways round

`the_sixteen_bit_capabilities_are_declared_only_where_used` translates two programs and scans
the emitted words for `OpCapability`: a move and a terminator, which must declare neither, and
a typed load of a `BUF_FMT_16_16_FLOAT` channel, which must declare both.

The positive half is not decoration. A module that needs a half and omits the capability is
invalid in the other direction, and lazy declaration is precisely the change that could get
that wrong - so the test that proves the laziness works is the same test that proves it has not
gone too far. It needs no device: it reads the words.

## A note on the suite

`cargo test --workspace` does not build at the moment, and not because of anything here: the
sibling `selfish` repository is mid-edit in another session and orbistoun builds against it by
path. The six crates this unit touches - translate, shader, gpu, gpu-vulkan, spirv, gen - pass
at 436, and `tools/validate-device.sh` reports no validation errors.

## Files

- `crates/orbistoun-translate/src/model.rs` - the two accessors, now `&mut self`, with why.
- `crates/orbistoun-translate/src/{predicated,wavefront}.rs` - the types held as `Option`,
  declared with their capability on first use, gone from the headers and constructors.
- `crates/orbistoun-translate/tests/execute.rs` - the test, and a word scanner for capabilities.
- `crates/orbistoun-gpu-vulkan/src/compute.rs` - the comment that said "every module".

## Next

Unchanged from 554: the image subsystem for record B's textured shader, then the mesh stage
(D688), after which the console's own geometry could be drawn and its frame hash becomes a
question worth asking.
