# D511 - Two formatter bugs in one run, both in the padding nobody tested

**measured** - 2026-09-03 (twenty-three `sprintf` cases against glibc 2.39)

`sprintf` had no differential coverage. Twenty-three cases later, two conformance bugs, both
in the same three lines:

```text
sprintf/width-zero-negative: buffer is 30302d3432…, expected 2d30303432…
sprintf/precision:           returned 0x2, glibc returned 0x3
```

## `%05d` of -42 rendered `00-42`

The padding was applied uniformly after rendering, so a negative number got its zero fill in
front of its sign. ISO C 7.21.6.1 puts the `0` flag's fill **after** any sign, which is what
makes `-0042` and not `00-42`.

Nothing here could have noticed. The string is the right length, the digits are right, and it
is only wrong in the order of two characters - a guest parsing it back gets a number it cannot
read, somewhere with no connection to the formatting.

## `%.3d` of 42 rendered `42`

Precision was read by `read_specifier`, applied to `%s` - where it truncates - and to nothing
else. For `d i o u x X` it is the **minimum number of digits**, zero-filled on the left, so `42`
must render as `042` and the return must be 3.

Two further rules came with it and are implemented rather than noticed later: **a specified
precision makes the `0` flag ignored**, and a precision of zero with a value of zero renders no
characters at all.

## Why the cases found it and the previous ones did not

`snprintf` has had cases since the differential was built, and every one of them formats `%d`
with no flags. **The interesting failures in a formatter are not in any one conversion, they
are in the specifier parser** - a width read as a precision, a `0` dropped for a
left-justified field, a `%%` that consumes an argument.

So the format string is a case *input* here rather than a family of helpers, and the twenty-
three cases sweep the conversions and the flags rather than the values. That shape is the
finding, more than either bug.

Both were watched failing, separately, by reverting each half of the fix on its own - a single
break would have left the other half unproven.

## And the dispatch was consolidated because it tripped the lint twice in a day

`run` in the differential test grew an `if` block per shape and hit the 100-line ceiling twice
while these cases were being added. It is one `match` naming which replay each function takes,
so adding a shape is a line rather than a block, and a reader asking how `memcpy` is replayed
has a list rather than a chain to walk.
