# 596. Surface layout gets a roadmap entry, and half of it was never blocked

**2026-09-15** - step 6 of the gap analysis, which asked for the roadmap entry first and was right
to

## The gap, verified

Searching the roadmap for `tiling`, `detile`, `swizzle`, `surface layout` or `block-compress`
returns nothing. G10 covers what a *descriptor contains*; nothing covered how the image it points
at is arranged in guest memory. The gap analysis called this the one gap with no entry under any
name, and that holds.

It was not unknown - it was unfiled. D690 names the missing subsystem exactly - *"a descriptor
decoder, a surface cache, a format table, an upload path"* - and calls it the right eventual
answer while correctly declining to build it blind. A concept that lives only in a decision entry
is a concept nobody planning the next stretch of work will see.

Now **G15**, in the phase-6 table and with a section of its own.

## Writing it split the item in two, which is the useful part

- **Detiling is blocked on a capture.** A descriptor names a tiling mode; the address swizzle that
  mode implies is hardware, and nothing here has measured it. It cannot be written from a
  published document and must not be guessed - G10's own note already says a resource layout
  guessed wrong renders a frame that is *subtly* incorrect, which is the failure this project is
  least able to detect.
- **Block-compressed decode is blocked on nothing, and may not be needed at all.** Vulkan consumes
  BC data natively, so the work is undoing the tiling and handing the blocks over - not
  decompressing them. Whether that is possible is one device feature, `textureCompressionBC`, and
  the backend queried four features and not that one.

## So it was asked

```
NVIDIA GeForce RTX 5070 Ti reports textureCompressionBC = true
```

**A decoder is a fallback, not a requirement.** The eventual upload path undoes the tiling and
hands the compressed blocks over untouched, and a decoder exists only for a device that answers
no. That is a plan changed by a one-line question, asked before there was anything to upload -
which is the whole reason the analysis put a roadmap entry before the work.

Requested the way every other feature here is: only where the device offers it, and reported as
what was **enabled** rather than what the silicon could do, so a device created without it cannot
have a caller believe otherwise (D552's rule).

## A recorder, not a guard, deliberately

`whether_this_device_samples_compressed_textures_is_recorded` prints; it does not assert. That is
not laziness - the test beside it already pins the enabled-versus-offered contract and says why
one feature is enough: *"a list would be a list to maintain, and `tools/validate-device.sh` asks a
validator about all of them at once."* Adding a second assertion of the same rule would grow the
list that reasoning exists to avoid. The subgroup size is reported the same way, for the same
reason.

What it does buy: a run of the suite on a different machine records that machine's answer, so the
day a device says `false` it is in the output rather than a surprise at upload time.

## What was refused

Anything that would have needed the swizzle. No layout model, no detiler, no format table - the
entry says what must be measured first and stops there. A tiling mode's swizzle written from
plausibility would be the exact failure G10 warns about, and it would be invisible until a frame
looked *almost* right.

## Gate state

`cargo fmt --all --check` clean, clippy `--workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,338 pass / 0 fail, worklogs unique, identity scan clean.
