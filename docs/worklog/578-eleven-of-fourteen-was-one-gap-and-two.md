# 578. Eleven of fourteen was one gap and two refusals, and the tool could not say which

**2026-09-15** - orbistoun-cli, orbistoun-translate and orbistoun-gpu-vulkan, after worklog 577

Every instruction in the corpus translates - 184 of 184, no blockers - and three of the fourteen
shaders still translate at no stage. Worklog 577 left that as the open question.

The census could not answer it. It printed `11 of 14` and a blocker list ranked by instruction,
which explains a shader refused for an unsupported instruction and says **nothing** about one
refused for anything else. That was fine while instructions were the constraint and became
useless the moment they stopped being.

So the tool was fixed rather than worked around, and the answer fell out immediately.

## 1. The answer

```
shaders that translate at no stage
  arith
    Compute: the division pre-scale writes a destination this translator does not know
             as a lane mask
  sampling
    Compute: only a fragment module samples a texture here
    Fragment: this shader reads more than one texture and a pipeline binds one (D690)
  unreached
    Compute: this shader reads one attribute both interpolated and flat, and a host input
             is one or the other; refused rather than picking one
```

**Two of the three are decisions, not gaps.** The sampling fixture uses more than one texture and
D690 refuses that on purpose - a translation that picked whichever was bound would draw a frame
that looks right and is not. The `unreached` fixture reads one attribute two ways and the host
input can only be one of them, so it is refused rather than guessed at.

Only `arith` is a genuine gap: a division pre-scale writing its flag somewhere the translator
does not recognise as a lane mask.

So `11 of 14` is one missing capability and two working guards. A number that reads as "three
things left to build" was three-quarters a report of the project doing what it decided to do.

## 2. Reporting only the last refusal is worse than reporting none

The first version printed one reason per shader - the last stage tried. That is actively
misleading, and the sampling fixture shows why: the mesh stage refuses it for *being* a mesh
module, which says nothing at all about why the fragment stage, the one that could have run it,
turned it down. The real reason was one stage earlier and invisible.

Every distinct refusal is printed now, with stages that agree collapsed so a shader refused
identically everywhere still reads as one line.

## 3. A third way to bake a newline into a message

The `unreached` refusal came out of the tool like this:

```
  ... both interpolated and flat, and a
                             host input is one or the other
```

Twenty-nine spaces in the middle of a sentence - the same damage worklog 567 found and repaired,
arriving a third way. Not a line-continued literal, and not one `cargo fmt` collapsed: a literal
that simply **contains a newline**, with no backslash anywhere. The `prose` gate cannot see it and
neither could the space-run search that found the others, because the run is not on one line.

A detector for it is straightforward once stated: with line continuations gone from the tree, a
scan that tracks whether it is inside a literal can say when a line break happens inside one, and
the damage case is the one where the next line is deep indentation followed by a lowercase word -
a sentence carrying on rather than a new line of output.

Five sites, all repaired: the interpolation refusal, three assertions and prints in the device
feature test, and one comparison message. A sixth was an escaped `\n` followed by fourteen spaces
inside a lint's `reason`, which is the same thing written deliberately.

**Three variants of one defect now.** The gate catches the form that has not been collapsed yet;
everything already damaged needs a different search each time, and this is the second worklog to
say so.

## 4. Files

- `crates/orbistoun-cli/src/main.rs` - `translates` returns every distinct refusal, and the
  census prints a section naming the shaders that translate at no stage.
- `crates/orbistoun-translate/src/wavefront.rs`, `src/model.rs` - two repaired messages.
- `crates/orbistoun-gpu-vulkan/tests/features.rs`,
  `crates/orbistoun-gpu-vulkan/tests/translated_interpolation.rs` - four more.

## Next

1. The division pre-scale's flag destination, which is the one real gap in the corpus.
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
3. A translated levelled sample on a device: it translates, and nothing has drawn with it.
