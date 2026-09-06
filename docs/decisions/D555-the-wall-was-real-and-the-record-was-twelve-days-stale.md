# D555 - The wall was real, the record was twelve days stale, and the library was never empty

**decided** - 2026-09-04

Two findings, and the first one retracts a decision I wrote six ticks ago.

## D548 is wrong. Retracted.

D548 said the title library held only guest output and that `./bin/orbistoun run` could not be
turned in this session. **There are seventy-one guest binaries**, and there always were. I ran
`orbistoun-cli paths`, read

```text
titles      %APPDATA%\OOPS\titles
library     %APPDATA%\OOPS\titles
```

and took it as the whole answer. That directory is the **overlay** root - `titles_dir()` is
documented as *"the root every title's own data lives under"*, deliberately away from the module
so *"a guest able to write into it would be editing its own evidence (D250, D251)"*. It contained
`fs/` trees and nothing runnable, and `paths` prints its own *"(not a folder - nothing will be
found)"* warning only when the path is absent - so a real directory that structurally cannot hold
titles produced no warning at all.

The library was in the working directory the whole time.

**The defect that made it possible is real and worth fixing**: with no `config.toml`,
`library.root` defaults under the data root, which is the same place as the overlays. Out of the
box, "where do I look for titles" answers with a directory that cannot contain any. The comment
directly above that code says resolving it centrally was so that question would have *"a single
answer - which is exactly what it did not have (D228)"*. It has a single answer now and the
answer was wrong.

The user has since moved the library to the data root deliberately, so every OOPS tool shares
it. That works: `eboot.bin` and `fs/` coexist in a title's directory and the overlay never
touches the image.

**The lesson is the one D547 already recorded and I repeated at larger scale**: I established
what one path contained and concluded what *every* path contained. Ask whether the tool has a
seam before concluding it cannot be used.

## The wall moved. A long way.

```text
[status] before   image+0xafc959    23 imports        222 calls   2026-08-23
[status] now      image+0xf56e09   197 imports    420,611 calls   2026-09-04
```

Measured, not inferred: a run with `learned.toml` set aside entirely, so **zero overrides** - the
tool recorded it to the honest slot itself and printed *"explicit stub answers went from 1 to 0,
so this verdict measures a settings change"*.

So the famous wall - `read of 0x50 at image+0xf56e09` - **is the honest wall**, and has been for
some time. The `[experiment]` slot was never carrying it: the one override is a single
`sceKernelReserveVirtualRange` measurement learned from the guest on 2026-08-26, worth **thirty
calls** out of four hundred and twenty thousand.

That also means the honest slot has been *unreachable* since that measurement was learned:
`propped_up()` is `overrides > 0`, and a learned fact counts. Every run for twelve days went to
`[experiment]` and `[status]` froze at 222 calls - which is what I then quoted back as the state.
**A record that cannot be written is worse than no record**, because it reads as a measurement.

And the credit belongs to this session's implementation work: the fixed-base heap (D513), the
unrun initialiser arrays (D514, D515), the flip-pending lie (D516), `sceKernelStat` and
`sceKernelPread` (D525, D526). 222 calls to 420,611 is what those bought, and nothing said so
because nothing could run.

## VINTRP translates

The last capture-free instruction family. `v_interp_p1_f32` and `v_interp_p2_f32` are a guest
pair computing one interpolated attribute; SPIR-V has no such pair, because a fragment `Input`
variable *is* the interpolated value. **Both halves therefore answer the whole value** - treating
`p2` as a no-op on the grounds that its destination already holds `p1`'s result breaks the moment
they do not share one, which `unreached.s` does on purpose.

Recorded as **assumed**, with the edge named: a shader using `p1`'s intermediate for anything but
feeding `p2` gets a different number, and both `vsrc` operands - the guest's barycentric I and J -
are ignored, because the host interpolates with its own.

`v_interp_mov_f32` is **refused**: its second operand selects between a constant term and two
deltas, and this repository has no citable encoding for which value names which. Translating it
as the attribute's value would be right for one of three and silently wrong for the others.

`crates/orbistoun-gpu-vulkan/tests/translated_interpolation.rs` checks it through the oracle
D554 built - equal corners, so the expected colour is exact whatever the driver's sample
positions - against the hand-written passthrough shader.

### The bug it caught, which is the reason the oracle exists

The first run came back **all zeros**. The input variables were declared *after* the header, so
they never reached the entry point's interface. I had written a comment three ticks earlier
predicting that *"a module that omits one is rejected by a driver rather than misbehaving"* - and
this driver did not reject it. It answered zeros.

That is exactly what a translator verified against its own output cannot find, and it was found
in the first thirty seconds of running a translated module against a hand-written one.
