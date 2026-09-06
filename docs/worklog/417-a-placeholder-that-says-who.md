# 417. A placeholder that says who

**2026-09-04** - directed, while the Agc and futex asks are out

## What was built

`ORBISTOUN_TAG_PLACEHOLDERS` - an opt-in diagnostic under which each unimplemented function
answers `0x7fff_0000 | (0x10 + its stub slot)` instead of the shared `0x7fff_0001`. Measured
working:

```text
sceCommonDialogInitialize                 -> 0x7fff0091
sceAgcDriverRegisterDefaultOwner          -> 0x7fff021a
sceAgcCreateShader                        -> 0x7fff0225
libkernel::0x04df812afad225d7             -> 0x7fffbeac
```

**The code asked for this itself.** `error_used_as_pointer`'s action reads *"find what answered
with that code just before"* - a person's search, when D299 had already ruled that a finding
sending a reader looking must carry what they are to look at. With a tag there is nothing to look
for; the value is the answer, and the finding now names the function outright.

It cost four gigabytes to not have. A work-area sizer answered the placeholder and PPSA28061
handed it to `malloc` twice, and all the report could offer was the three calls before it (D564).

## Opt-in, and why that is not timidity

**Sixty-seven documents cite `0x7fff0001`**, and four decisions rest on *seeing* it: D516, D523,
D524 and D564 each fix an arity by spotting orbistoun's own placeholder left in a register.
Changing the value everywhere would make all of that stale at a stroke - the exact failure this
project spends most of its discipline avoiding. Opt-in invalidates nothing.

## Three gates caught three different mistakes

**The address-map gate.** I named the constant `PLACEHOLDER_BASE`, and `*_BASE: u64` means an
*address* base in this tree - `docs/ADDRESS_MAP.md` is gated against every one of them (D513). It
demanded this be documented as somewhere memory is mapped, which it is not. **The gate was right
and the name was wrong**; renamed to `PLACEHOLDER_PREFIX`.

**Clippy's missing-docs.** Inserting before a `pub const` anchor stole `TITLE_ENTRY_FILE`'s doc
comment. Third time today, and caught by a lint every time rather than by me.

**My own test.** The first attempt to demonstrate the 4 GiB bug under tagging set
`ORBISTOUN_RETURN` *and* enabled tagging - and the override quietly beat the tag, so the run showed
an untagged `0x7fff0001` and looked like a broken feature. The precedence
(`overridden.or(tagged).or(declared)`) is right: a person's explicit choice wins. The test was
wrong.

## Guards

Four, each watched failing: the tag floor removed so fixed codes attribute to slot 0; the floor off
by one; a tag for an uncalled slot resolving to the nearest name instead of nothing; and the
detector keeping its old `0x7fff_0010` upper bound, which would have made it **blind exactly when
asked to say more**.

## What is not claimed

**That the tag names the right function.** The service tags with a global stub index and the trace
records the same one - true today, asserted nowhere, and nothing would notice if one started
counting differently. The symptom would be a confident finding naming the wrong import, which is
worse than the vague one it replaced. Written into the guard.

**Nor that it would have caught the four gigabytes.** That case cannot be run any more: the sizers
are implemented, so no placeholder reaches `malloc`. The mechanism is proved by unit test and by
the tagged returns above - not by the bug it arrived too late for.
