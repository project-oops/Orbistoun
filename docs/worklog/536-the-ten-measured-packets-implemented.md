# 536. The ten measured packets, implemented as encoders - and why they are not yet wired

**2026-09-14** - `packet::build` gains ten builders, every dword of them measured

## What landed

`crates/orbistoun-gpu/src/packet.rs` gains a `build::measured` module - ten PM4 opcodes, each one
**read off a packet the console's own library wrote** rather than transcribed from a document - and
the builders that emit them:

`event_write`, `set_index_count`, `set_num_instances`, `draw_index_auto`, `set_index_base`,
`set_context_register`, `set_uconfig_register`, `set_sh_register_range`, `draw_index_2`.

`tests/measured_builders.rs` replays obSCEne's exact arguments through each one and asserts the same
bytes come out. Twelve tests, and they are the other half of `measured_packets.rs`: that file asks
whether the *walker* reads a real buffer correctly, this asks whether what orbistoun *emits* is what
the console emitted.

Each case asserts three independent things - the header, the length against obSCEne's separately
measured byte advance, and a clean single-packet walk by `packet::walk`. The walker is not consulted
to build the packet, so it is a genuine second opinion on the length rule.

## The guard was made to fail

Changing `EVENT_WRITE` from `0x46` to `0x47` fails exactly one test and reverting restores green.
Worth doing rather than assuming: a table of constants asserted against constants derived from the
same table is a tautology, and this one is not.

## Why `implementations()` is unchanged, which is the important part

The encodings are no longer the blocker; the **writer handle** is. Every `sceAgcDcb*` builder takes
a DCB in arg0 and writes through its cursor, and that cursor's offset within the handle is
unmeasured. A handler wired today could build exactly the right packet and would still have to
invent a struct to place it - which is what principle 1 forbids and what D671 already refused for
the pad layout.

So the split is deliberate and is the shape principle 8 asks for: a **pure encoder** that is fully
grounded, and no effectful wrapper until the thing it would write through is measured. The handle
layout is already requested (obSCEne `REQ-20260913T2346Z-d3cb`); when it lands, wiring these is
small and the packets will not need revisiting.

The stale sentence in `agc.rs` - "builders stay declared-only until a capture grounds each one's
encoding" - was corrected in place, because a reader hitting it today would conclude the captures
are still missing when the blocker has moved.

## Honesty about coverage

- **`draw_index_2` is 3 of 5 body dwords measured.** obSCEne read positions 1, 2 and 3 (address low,
  address high, index count) and did not read 0 and 4. Those two are placed by the published PM4
  field order and are named parameters - `max_size` and `initiator` - specifically so a caller
  supplies them rather than inheriting a zero somebody invented. The *size* is measured and is not
  in doubt.
- **`event_write` is the short form only.** A longer address-carrying form exists; this is not it,
  and the doc comment says so.
- **Every builder here is one argument set.** A builder that agrees on one call is not verified, and
  this sweep proved that matters: `sceAgcDcbWaitRegMem` writes a different extent for different
  arguments, which is why it is absent from this list.

## Surprises

**Two long heredocs were truncated mid-string**, surfacing as an unbalanced-quote error rather than
a length error. Both times the fix was to split into shorter commands. Worth remembering as a shape:
a shell error that names a quote may be complaining about a length.

**Seven clippy errors in `agc.rs`/`agc_driver.rs` are not from this work** - `unsafe` blocks missing
safety comments, in another session's uncommitted changes. They will fail the static gate, which
denies `undocumented_unsafe_blocks`. Flagged rather than fixed: editing another session's live file
is how the lost update earlier today happened.

## Addendum: the static gate was already red, and is now green

The seven `unsafe` blocks flagged above were fixed rather than left, on request. They were all in
`create_shader`'s pointer relocation and in `peek_u64`, and each now carries the invariant that makes
it sound rather than a restatement of what it does - the header object's measured `0x130`-byte
extent for the direct field accesses, and for the sub-table walk the fact that the offset is the
guest object's **own** relative offset, rejected unless non-zero and under `0x1000`, at an entry
`i * 8 < 40` inside the region hardware itself dereferences at the same site.

Four further warnings would have failed CI's `-D warnings` and are fixed too: a doc line that wrapped
onto a leading `-` and so read as a Markdown list item (`agc_driver.rs`), a `| (0 << 17)` that
clippy correctly calls a no-op (the field it stood for is zero, now said in a comment instead), two
`const` items declared after statements, and two match arms with identical bodies merged into one.

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` now exits 0, `cargo fmt --check` is
clean, and all 74 tests pass. None of these were this work's own defects - they came in with another
session's uncommitted changes - but a red gate belongs to whoever is looking at it.
