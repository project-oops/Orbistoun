# D204 - Typed and untyped buffer accesses share one body, and unmeasured formats are refused


**Status:** decided (2026-08-21)

`buffer_access` is the whole of both. A typed access differs from an untyped one in exactly
two things - whether a format was checked, and how many components move - and shares the
descriptor, the addressing equation and both bounds checks.

Splitting them would have meant two copies of the addressing, which is the part of this
where being wrong is silent: a wrong address produces a shader that runs and reads the
wrong memory, and nothing in the pipeline notices.

### What is translated

Formats whose components are all thirty-two bits. Those move whole words unchanged, so the
work is an untyped access repeated per component and the format contributes only a count.
`a_single_channel_typed_access_is_an_untyped_one_with_a_format` pins that claim directly -
if the two ever disagree, the addressing has been duplicated somewhere.

### What is refused, and why refusing is the point

Three things, each by name:

- **A format needing conversion.** A normalised eight-bit component becomes a float by
  dividing by 255; a half-precision one needs a real conversion. None of that is written,
  and treating the word as the value would produce a shader that compiles, runs, draws, and
  is wrong only in its pixels - the failure this project has no cheap way to detect.
- **A format whose component count disagrees with the channel count.** The hardware permits
  it. What it does then - padding the missing channels, discarding the extra - was never
  measured here. Refusing an unmeasured rule costs one shader; guessing it costs the
  ability to trust every shader that used one.
- **A code with no meaning**, whether the explicitly invalid one or a reserved one. The
  nearest real format would render.

All three have tests, and those tests need no GPU - the refusal happens during translation.
That matters more than it sounds: the honest-failure paths are the ones most likely to rot,
because nothing exercises them by accident.

### The bounds check steps with the components

Each component's offset is checked, not just the first. A four-channel access at the very
end of a buffer would otherwise pass a check on its first word and read three past the end
- which is precisely the case the check exists for.

