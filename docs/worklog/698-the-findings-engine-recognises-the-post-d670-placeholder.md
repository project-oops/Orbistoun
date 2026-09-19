# 698. The findings engine recognises the placeholder D670 moved

**2026-09-19** — inbox `-ed20`: since D670 an unimplemented call answers `0xF7FF_0001`, but the
findings engine's `looks_like_placeholder` still gated on the pre-D670 `0x7FFF_0000` window. So the
three sites that rest on it - a fault shape, a placeholder in an argument register, a placeholder
faulting address - were **silent on exactly the wall this corpus produces**. The `0xf7ff0001`-into-
`memcpy` wall of worklogs 594/616 was found by hand; the worker's own fault label already used
`placeholder_named`, so a report's fault line and its findings disagreed.

## The fix

- **Derived from the core, not restated.** `diagnose.rs`'s `PLACEHOLDER_LOW` and the service's
  `PLACEHOLDER_PREFIX` are now `orbistoun_core::PLACEHOLDER_BASE` (0xF7FF_0000), and `PLACEHOLDER_HIGH`
  / `PLACEHOLDER_TAGGED_HIGH` derive from it. When D670 moved the base, these went blind precisely
  because they carried the old literal; deriving from the one definition means the next move takes the
  detector with it. (`orbistoun-report` gains a dependency on `orbistoun-core` for `PLACEHOLDER_BASE`
  and `GuestError`.)
- **Sign-extension folded.** A negative code widened `int`→`long` sign-extends to
  `0xFFFF_FFFF_F7FF_0001`; a guest that widened a refusal before using it faults there. A new
  `low32_if_sign_extended` folds the all-ones top half before the range test, the same normalisation
  `placeholder_named` does - a case that only exists since the high bit went on (D670).
- **Tagging keeps the high bit.** The service tags stubs `PLACEHOLDER_PREFIX | (0x10 + slot)`; moving
  the prefix onto `PLACEHOLDER_BASE` means a tagged placeholder is now negative too, so a guest's own
  `rc < 0` catches it - the behaviour D670 removed for the untagged code, now removed for the tagged.

## Made to fail first

- `the_post_d670_placeholder_is_recognised_in_a_register_and_as_a_fault`: a trace whose tail hands
  `GuestError::Unimplemented.as_raw()` in **rdx** (not rdi - `error_used_as_pointer` reads every
  argument) yields a `Gap::ErrorUsedAsPointer`, and a fault at that address is classified as a
  placeholder. **Watched failing against the current code first** (it did, at the rdx assertion),
  then passing.
- `every_tag_the_service_produces_is_negative_and_recognised`: across the slot range the tags span,
  every `PLACEHOLDER_BASE | (0x10 + slot)` has bit 31 set and is recognised by `looks_like_placeholder`
  - a tag that lost the high bit is exactly the regression D670 removed.
- The five existing placeholder tests were re-pointed from the `0x7FFF` literal to
  `GuestError::Unimplemented.as_raw()` / `PLACEHOLDER_BASE`. PPSA28061's **measured** stale registers
  `0x7fff0201` / `0x7fffbe01` are kept as-is: they now fall *outside* the placeholder block entirely,
  so D670 made them unmistakable-for-a-tag rather than the near-miss the old positive range risked -
  and the in-range-but-uncalled rejection is carried by `base | 0x999` instead.

Stale comments corrected in `orbistoun-env` and `orbistoun-service` and the `diagnose.rs` docs (every
one that still said a stub answers `0x7fff_0001`).

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): `orbistoun-report`/`-service`/`-env`
clippy `-D warnings` clean, workspace tests pass (100 report tests, two new), prose exit 0,
`status --check` exit 0 (no count moved), knowledge-audit 27, doc gate clean, identity scan clean.
Corpus unchanged. No commit.
