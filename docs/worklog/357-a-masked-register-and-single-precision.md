# 2026-09-03 - (/loop) A masked register, and the width where a shim shows itself

```
CLAIMED        76  ->  77        OUTSTANDING   68  ->  67
differential  235  ->  247 cases
tests               1996
```

## The raw MXCSR was claimable after all

It sat outstanding because it mixes two things: `0x9fe0` is the four configuration fields plus
bit 5, the sticky precision flag the console's own startup arithmetic had already set. Orbistoun
installs `0x9fc0` and deliberately does not reproduce that bit (D486), so the numbers differ.

**The split is published, so the whole value is claimable against it:**

```text
0x9fe0 & !0x3f == 0x9fc0 == GUEST_MXCSR
```

Bits 0-5 are the six exception flags - sticky state a program accumulates - and every other bit
is configuration. That is Intel's definition of the register, not a partition invented to make
two numbers meet. D508.

**Two assertions, and the second is not decoration.** `raw & !STATUS == live` also passes if
orbistoun set a configuration bit the console clears, because masking hides which side a
difference is on. So the test also pins `raw & STATUS == 0x20` - the precision flag is the only
bit the console carries and orbistoun does not. Watched failing by moving `GUEST_MXCSR` to
`0x9f80`, a configuration bit rather than a status one, which is the break a single masked
assertion is weakest against.

## `strtof`, and a wire that would have compared the wrong function

**235 -> 247 cases.** `strtof` is the same shape as `strtod` at single precision and shares its
replay path. Worth its own cases rather than assumed to follow the double: **an implementation
that parses in double and narrows is not the same function** - two roundings, and a value near a
float halfway lands on the wrong side. `16777217` is the case that shows it, and orbistoun
answers `0x4b800000` like glibc. So do the subnormal (`1e-40`, `ERANGE`), the overflow (`1e39`
to `+inf`) and the underflow.

Sharing the path had a trap in it: `run_strtod` began
`float_implementation_named("strtod")` with the name hard-coded. Every `strtof` case would have
been run against orbistoun's **`strtod`** and compared to glibc's `strtof` - and `1.5` and `42`
would have agreed, so it would have looked fine and failed only where the widths differ. Taken
from the case instead.

Watched failing on the double-rounding case, which is the one that would have caught the wire
too.

## State

`cargo test --workspace` green - 119 suites, **1996 tests**, 0 failures. Differential **247
cases**, all agreeing. clippy `--tests` clean, fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-357 and D466-D508.

**Next**: `strtok_r` (needs a `saveptr` through the `tokenise` shape), `sprintf`/`vsnprintf`,
and the 13 imports that name data rather than a function.
