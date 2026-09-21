# 754. The oracle consulted — a related but unsolved Unity shader fault, and what it is worth

**2026-09-21** — 753 named the reference emulator as the one unused source and set the rule for it:
oracle, never transcribed. This tick consults it and reports the result honestly. prosper has notes on
the Unity shader path, they are about a **different fault it has not itself solved**, and the boundary
means what transfers is a checkable Unity fact, not prosper's investigation. The oracle did not hand
over PPSA02664's misread input; it pointed at where such inputs live.

## What was consulted, and the discipline

prosper (credited in `ACKNOWLEDGEMENTS.md`, cited already across `libSceAgc.toml`) documents the Unity
2022.3 shader serialization format, cross-checked against AssetRipper. Reading a *format description* is
the sanctioned oracle use — it predicts what correctly-formed data looks like, the same way its AGC
notes predict a probe's return. No source was taken; nothing of prosper's own fault analysis is carried
into orbistoun as a finding here. What follows is the one thing that is both in-boundary and useful: a
Unity-format fact, and a hypothesis for orbistoun to verify on its own.

## The referenceable fact, and why it bears on this wall

Unity's shader serialization is **version-sensitive**: fields changed width and alignment between engine
versions (a shader-requirements field widened to 64-bit in the 2022.x line; odd-length `u16[]` arrays
need a 4-byte align that earlier readers skipped). A reader working to a 2021.2-shaped model **survives
every simpler shader** — empty fields read as zero — and desyncs only on the first shader that populates
the newer fields, from which point every subsequent field is read at the wrong cursor. The desync is
cumulative and silent until a count or pointer comes out wrong.

That shape rhymes with PPSA02664's wall exactly: a header whose group-pointer relocation comes out
**three of five**, a count that is wrong by a fixed amount, on a Unity 2022.3 title. It is a hypothesis,
not a transfer — the fault prosper documents is a *deserialization* crash on a different shader and path,
and prosper has **not** solved it (its own notes end at an undone live-cursor trace). So the reference
has no answer to lift even if lifting were allowed. What it gives is a direction orbistoun can test with
its own instruments: **does orbistoun feed the Unity reader a value that flips its shader parse to the
wrong version-shape?** — a klog/version/size answer, checkable against the measured 2022.3 format.

## Why this is a lead and not a fix

The parse runs entirely in the guest (Unity's code), so orbistoun cannot correct it directly; it can
only correct an *input* the parser reads. Finding that input is the same hard problem 748-753 circled,
now with a sharper frame: not "which byte of the descriptor is wrong" but "which value orbistoun answers
makes a 2022.3 parser behave like a 2021.2 one". That is an orbistoun-side, measurable question — a
version query, a file size, a keyword count the HLE returns — and it does not need the inlined loader's
code or a construction watchpoint. It does need identifying which of the title's reads feeds the shader
parser, which is the next tick's work.

## Honest close

The oracle was consulted within the boundary and gave a frame, not an answer — fitting, since it has not
solved its own version of this. PPSA02664's create_shader correction (751) stands; its inlined-loader
wall is now framed as a probable Unity-version parse desync driven by an orbistoun-answered value, to be
found by orbistoun's own trace of what the shader reader consumes.

## Gate state

No code changed — an oracle doc read (no source taken, prosper cited by name) and analysis.
`./bin/orbistoun check` unchanged from 753 (green but for the same three generated-doc drifts from a
prior session's uncommitted `compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No
commit.
