# D467 - A packed half widens to a float through a real 16-bit float type, not a hand-rolled unpack

**assumed** - 2026-09-02 (user-directed /loop: the GPU shader-translation frontier)

FLOAT16 is the first packed buffer kind that is genuinely a float in memory rather than an integer
normalised into one. A 16-bit IEEE half has to become a 32-bit float. There were three ways to do it,
and the choice matters because a conversion that is wrong only for subnormals or infinities is wrong
*silently* - the one failure mode [principle 3](../../CLAUDE.md) singles out.

**Chosen: narrow the field to a 16-bit unsigned, bitcast it to a 16-bit float, widen with
`OpFConvert`.** Three instructions (`UConvert` u32->u16, `Bitcast` u16->f16, `FConvert` f16->f32, then
a bitcast back to the register's bits). The conversion itself is the driver's own IEEE path, so
subnormals, infinities and NaNs are its problem, correctly, and not a constant this project would have
to get right without a way to notice when it did not.

The cost is two capabilities - `Float16` and `Int16` - and two extra types (`OpTypeFloat 16`,
`OpTypeInt 16`) declared in every module, whether or not it loads a half. That makes every emitted
module require a device offering those features. This is the **assumed** part: the development GPU
offers both (the FLOAT16 test executed on it), and for a research emulator targeting modern hardware
the requirement is acceptable. If a target ever lacks them, the declaration can be made conditional on
a half actually appearing - lazy emission needs a cached type id, which is the only reason it was not
just done now.

**Rejected: `OpExtInstImport "GLSL.std.450"` + `UnpackHalf2x16`.** Also correct-by-driver, and the
route SRGB's `pow` *will* need. But it wanted real new infrastructure: a header slot inserted between
`OpExtension` and `OpMemoryModel` (renumbering the builder's ordered header, which every module shares
- the highest-blast-radius change on the table), an `OpExtInst` emit path and its `SHAPES` rows, plus
a `vec2<f32>` type for the unpack result and an `OpCompositeExtract` to take the `.x`. More surface, and
more of it shared, for no better an answer than `OpFConvert` gives. When SRGB forces the ext-inst
plumbing in, FLOAT16 can move onto it if there is any reason to; there is not likely to be.

**Rejected: manual bit-unpack (the "magic multiply").** The field's sign, exponent and mantissa
reassembled into a float with shifts and one float multiply. It is elegant and needs no new
infrastructure at all - but its subnormal path depends on subnormals *not* being flushed in the
intermediate, and this project does not declare `DenormPreserve`. So a subnormal half would silently
read back as zero. Exactly the silent-wrong the whole packed path is built to avoid, and the reason a
correct-by-construction route won even though it costs two capabilities.

**Scope.** Admitted at width 16 only. The narrower packed floats - the 11- and 10-bit channels of a
format like R11G11B10 - are not IEEE halves and decode differently; the refusal checks every component
is 16 bits before admitting the kind, and leaves the rest a loud gap. See [D466](D466-packed-typed-buffer-formats-by-kind.md)
for the packed path this extends, and worklog 296.
