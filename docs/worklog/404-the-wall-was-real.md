# 2026-09-04 - (/loop) The wall was real, the record was twelve days stale, VINTRP translates

```
[status] PPSA02664-app0   image+0xafc959 / 222 calls  ->  image+0xf56e09 / 420,611 calls
shader breadth 120/127 -> 124/127; v_interp_p1/p2 translate, v_interp_mov refused
D548 RETRACTED - there are 71 guest binaries and there always were
```

Forty-first cron tick, and the user corrected me.

## D548 is wrong

I said the title library held only guest output. **There are seventy-one guest binaries.** I ran
`orbistoun-cli paths`, read `library C:\…\AppData\Roaming\OOPS\titles`, and took it as the whole
answer. That is the **overlay** root - `titles_dir()` is documented as where a title's own *data*
lives, kept away from the module because "a guest able to write into it would be editing its own
evidence (D250)". The library was in the working directory the whole time.

The defect that allowed it is real: with no `config.toml`, `library.root` defaults under the data
root, so out of the box "where do I look for titles" answers with the directory that structurally
cannot hold any - and `paths` prints its *"nothing will be found"* warning only when the path is
absent. The user has since moved the library there deliberately, which works: `eboot.bin` and
`fs/` coexist and the overlay never touches the image.

Same lesson as D547, repeated at larger scale: I established what one path held and concluded what
every path held.

## The wall moved, a long way

```text
[status] before   image+0xafc959    23 imports        222 calls   2026-08-23
[status] now      image+0xf56e09   197 imports    420,611 calls   2026-09-04
```

Measured with `learned.toml` set aside, so **zero overrides** - the tool recorded it to the honest
slot itself and said *"explicit stub answers went from 1 to 0, so this verdict measures a settings
change"*.

So `read of 0x50 at image+0xf56e09` **is the honest wall**. The `[experiment]` slot was never
carrying it: the single override is a `sceKernelReserveVirtualRange` measurement learned from the
guest, worth **thirty calls out of four hundred and twenty thousand**.

And the honest slot had been *unreachable* since that was learned - `propped_up()` is
`overrides > 0`, and a learned fact counts - so `[status]` froze at 222 calls for twelve days and
I quoted it back as the state. **A record that cannot be written reads as a measurement.**

The 222 → 420,611 is this session's implementation work: D513, D514/D515, D516, D525/D526.

## VINTRP translates

Both halves of the guest's pair answer the whole interpolated value - SPIR-V's `Input` variable
*is* the result, and treating `p2` as a no-op breaks when the pair does not share a destination,
which `unreached.s` does on purpose. **Assumed**, with the edge named: the barycentric `vsrc`
operands are ignored because the host interpolates with its own.

`v_interp_mov_f32` is **refused** - its parameter operand picks between a constant term and two
deltas and nothing here can cite which is which.

### The bug the oracle caught in thirty seconds

The first translated run came back **all zeros**: the input variables were declared after the
header, so they never entered the entry point's interface. Three ticks ago I wrote a comment
predicting a driver would *reject* such a module. This one answered zeros instead. That is exactly
what a translator checked against its own output cannot find.

The `orbistoun-overrides --test frontier` failure was the gate working: the committed
`COMPATIBILITY.md` no longer matched the improved record. Regenerated with `compat markdown`.

Decision: [D555](../decisions/D555-the-wall-was-real-and-the-record-was-twelve-days-stale.md).
