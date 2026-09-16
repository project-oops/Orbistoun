# 625. Inferring import signatures from how the guest calls its own imports

**2026-09-16** - a black-box characterisation of every called import's arity and argument kinds,
accumulated allocation-free on the call path and surfaced on the finding that says "implement this"

## The question this answers

The firmware behind a vendor stub is unreadable (XOM), so an unimplemented import's **signature** -
how many arguments it takes and what kind each is - is not knowable from the binary. But the guest's
own calls carry it: a register that is always a pointer into a guest region is a pointer, one that is
always small is a scalar or a size, one that is never anything but zero is unused, and one that is
sometimes a pointer and sometimes zero is an optional pointer. This is inference from observed
behaviour - the only signature source a black box allows - and it is exactly the starting point for
implementing a function correctly, or for designing an obSCEne probe that exercises it with the right
shapes.

It is a **lower bound, not a proof** (principle 3): a real argument that the guest always happened to
pass as zero reads as unused, and the black box cannot rule that out. The report says "the guest
called it as …", never "its signature is …".

## What was built

A per-import argument-shape accumulator in `orbistoun-thunk`, parallel to the existing call counters:

- `classify_arg(v) -> u8` (pure, `const`): one value into one shape bit - `ZERO` (0),
  `SCALAR` (< `0x1_0000`), `POINTER` (inside the guest address space `0x4000…`..`0x8000…`), or
  `OTHER` (a large non-address). Pure by design so the boundaries are pinned without a running guest.
- `SHAPES: OnceLock<Box<[AtomicU8]>>`, `count * 6` slots, allocated once in `prepare_counters`
  beside `COUNTS` - never on the call path.
- On the recording hot path, at the existing counter-increment site: `fetch_add` hands back the
  count *before* this call, so the first `SHAPE_SAMPLE_LIMIT` (16) calls of each import OR their
  argument categories into its slots and the millions after them pay only the increment. The shape
  of a slot is settled in far fewer than sixteen calls; the seventeenth adds nothing (principle 9).
- `arg_shapes() -> Vec<[u8; 6]>` reads it back, mirroring `call_counts()`.
- `describe_shape(&[u8]) -> String` renders `(ptr, u32, ptr?)`: arity is the count up to the last
  slot that ever carried anything (trailing untouched registers are arguments the guest never passed,
  so they are dropped); `?` marks a slot that was also zero on some call; an always-zero argument
  renders `0` and is *within* arity, distinct from an unwritten slot.

Surfaced where it pays off: `CalledImport` gained a `shape` field (serialised, skip-if-empty), and
`orbistoun-report`'s `unimplemented` and `unnamed` findings add "the guest called it as (…)" to their
evidence. So the finding that already says "`sceAgcCreateShader` was called 40 times and nothing
implements it" now also says how the guest called it - the work item carries its own starting point.
An unnamed hash gains an arity constraint: a name candidate whose arity disagrees with how the guest
actually called it is wrong before the NID is even computed.

## Why the thunk owns the rendering

`describe_shape` and the `SHAPE_*` bit meanings live in `orbistoun-thunk`, the crate that produces the
bits; `orbistoun-report` stores the finished `String` and `orbistoun-report`'s findings render it.
This keeps the bit vocabulary in one place and avoids a new crate dependency (principle 13 - the
report is not a shim holding logic the thunk should).

## Where it does *not* leak host semantics

The classification boundaries are guest-address-space facts (image base, pool, stack all in
`0x4000…`..`0x8000…`), not host ones. A host pointer appearing as a guest argument would be a bug the
existing envelope check (D615) already catches; here it would read as `POINTER` or `OTHER`, which is a
faithful description of the value, not a wrong answer.

## Made to fail

- `an_argument_is_classified_by_where_its_value_falls` (thunk) - each boundary pinned: `0`→zero,
  `0xffff`→scalar, `0x1_0000`→other, image base→pointer, one past the top→other. Asserts the
  category, not that *some* bit is set.
- `shapes_or_together_across_calls_into_an_optional_pointer` (thunk) - a slot seen as both NULL and a
  pointer reads as `ZERO|POINTER`, the optional-pointer inference.
- `a_shape_renders_as_a_signature_with_arity_and_nullable_marks` (thunk) - exact string: arity
  dropping trailing registers, `?` on a nullable slot, `0` for an always-zero argument, `u64` for a
  large non-address.
- `an_inferred_signature_is_carried_into_the_unimplemented_finding` (report) - the signature reaches
  the finding's evidence, and an import with **no** shape is not given an invented one.

Confirmed the clippy `multiple_unsafe_ops_per_block` deny caught the first hot-path draft (one block
doing `.add` and `.read`); split into two `// SAFETY:` blocks like the RING store beside it.

## What it is for next

The unimplemented findings now rank with their arities attached. The immediate use is on the AGC and
vendor imports the Unity titles call: an inferred `(ptr, u32, …)` on an unimplemented `sceAgc*`
narrows what to implement and what an obSCEne probe should pass, without a single hardware round-trip.
The `int 0x41` work is unaffected by this (it is an il2cpp assertion, worklog 620, not a missing
import), but the next unimplemented vendor call the boot reaches will arrive with its shape already
measured.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,373 pass / 0 fail, worklogs unique, identity scan clean.
