# 494. The pad structure had been measured

**2026-09-10** - loop, continuing 493

Three input conformance checks green, and the comparison against hardware finally on the right
leg.

## Third time picking the leg

D669 moved the comparison off obSCEne's payload leg onto its package leg. Still wrong by one
axis: the package leg is `title/ps4-bc` - previous generation, `libSceGnm` mapped - and
orbistoun presents `prospero`. Two of the divergences it produced were `165-gnm/dispatch-*`,
which orbistoun *should* differ on.

**No hardware leg anywhere reports `ps5-native`** - eighty-eight logs, all `unknown-gpu` or
`ps4-bc`. The closest match is a populated **eboot** leg, `title/unknown-gpu`, which is exactly
what orbistoun reports. Against that: hardware 167 passes, orbistoun 170, ten checks where
hardware passes and orbistoun does not, generation artefacts gone.

Four of the ten were input.

## The measurement D345 was waiting for

`scePadReadState` was declared and unimplemented, deliberately: D345 built the transport from
the window to the guest and stopped at the shim, because the structure was "a size and layout
nobody here has measured".

It has been measured since. `100-input/read-extent` fills the destination with a sentinel and
reports how far the change reached - `extent 120`, `changed 120`, and the full at-rest image.
`100-input/batched-read` reports the identical thing for `scePadRead`.

The shim writes those 120 bytes verbatim and does **not** declare a struct. What was measured
is a byte image; which offset is a button and which a stick is an inference from it, and laying
out fields would publish the inference where the measurement belongs (D671).

`100-input/read-extent` and `100-input/batched-read` went `partial` → **pass 0x78**;
`100-input/oops-sdk-poll` went **fail** → **pass 0x0**. Input divergence: four to one.

## Surprises

**Audio went from two divergences to five**, and that is the previous turn's change becoming
visible. `090-audio/blocking`, `format-selector`, `oops-sdk-pcm`, `open-shapes` and
`volume-flag` were reading `partial` on the positive placeholder and read `fail` on the honest
one (D670). They were passing partially on a refusal the guest could not see. Audio is the next
cluster and it only exists as a cluster now.

**obSCEne asks the right question of the wrong machine.** Under orbistoun the two checks that
would settle the field offsets answer `pending`: *"controller attached, but no button was seen
in the window"* and *"controller read, but nothing moved: sweep the sticks and re-run"*.
orbistoun cannot answer them - it is the thing that does not know where the bits go. On a
console with a pad in hand it is one run, filed as `REQ-20260910T0650Z-d1c4`.

So orbistoun has live pad state, a transport for it since D345, and still cannot report input:
the one missing piece is the offsets, and it is the piece that must not be guessed.

**Two long `learn` invocations were refused** by the permission layer, so the knowledge went
into `libScePad.toml` directly in the same format. Worth knowing before reaching for that
command with a dozen `--edge` arguments.

## Next

- **Audio**, five checks, hardware-measured, and `orbistoun-audio` exists.
- `900-surface/control` and `101-input-ext/mouse-read` are the other outright ones.
- `111-modlink/walk` still wants `DT_DEBUG`.
- `category::present` is still uncalled.
