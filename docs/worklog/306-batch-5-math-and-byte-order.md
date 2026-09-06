# 2026-09-02 - (/loop) Bulk port batch 5: the single-precision math family and byte order

```
documented   714 needed, 395 missing   ->   714 needed, 374 missing
```

Twenty-one entries. The simple ones are the standard's function under Rust's name for it; the
six written by hand are the ones where that correspondence does not exist, and they are where
the specification says something a plausible implementation would get wrong.

## Math (ISO/IEC 9899 7.12), in `orbistoun-libc/src/math.rs`

By macro: `acosf`, `asinf`, `atanf`, `cbrtf`, `exp2f`, `log10f`, `log2f`, `tanhf`, `hypotf`,
`exp2`. By hand: `nearbyintf`, `frexp`, `modf`, `modff`, `ldexp`, `ldexpf`, `sincos`.

Four details worth having written down:

- **`nearbyintf` is ties-to-even, not `round`.** `round` is ties-away-from-zero and the two
  disagree on exactly the halfway cases - the ones a casual test never uses. Tested at 0.5, 1.5,
  2.5 and -0.5.
- **`frexp` must answer zero, infinity and NaN unchanged with `*exp = 0`.** A version computing
  the exponent from a logarithm answers negative infinity for zero and stores nonsense in the
  caller's `int`.
- **`modf` gives both parts the argument's sign.** `modf(-3.5)` is `-0.5` and `-3.0`; flooring
  gives `0.5` and `-4.0`, which reassembles to the same number and is still wrong.
- **`ldexp` splits its scaling.** `2^n` is representable only inside the exponent's range, so a
  single `x * 2f64.powi(n)` answers infinity for large `n` even when the product would be
  finite - and cannot reach a subnormal going the other way. Multiplying by an exact power of
  two is itself exact, so splitting keeps one rounding at the end. Tested at 2^1023, past it,
  and down to the smallest subnormal (which needs the split to reach at all).

`ldexp`, `frexp`, `modf` and `sincos` take **integer** arguments - an exponent or an
out-parameter - which arrive in integer registers. Reading them from the float registers would
read whatever the caller last put there.

`sincos` is a BSD extension rather than ISO C, cited to FreeBSD `sincos(3)`; the target's C
library is FreeBSD-derived, which is why it is imported at all.

## Byte order, in `orbistoun-fs/src/socket.rs`

`htonl`, `htons`, `ntohl`, `ntohs` (POSIX.1-2008), beside the sockets that use them, with
delegation rows in `orbistoun-posix`. Exact, and the two tests that matter say why: **only the
low half of the register is meaningful** (the argument is a `uint32_t` in a 64-bit register
whose top half is the caller's leftovers - reading it would answer a different number each call
for the same input), and each conversion is its own inverse, which is why network order needs
one operation under four names rather than two operations.

Written as a byte swap rather than a no-op, with a comment saying so: a no-op is what it would
be on a big-endian host, and that is exactly the assumption that would be wrong there.

## Housekeeping

Cleared five clippy warnings that were **not** from this batch - `unnecessary qualification` in
`orbistoun-fs/src/lib.rs`, left by an earlier session's test-isolation work in this same
uncommitted tree. They would have failed CI.

clippy `--tests` clean across libc/fs/posix, fmt clean, tests pass, nothing committed.

**Next**: the 259 genuinely-absent POSIX names, which is the largest remaining block. Also
worth adding: a machine-checkable **partial** marker. The knowledge files carry `known_by`,
`returns`, `arity`, `cites` and `edge_cases` but nothing that says "implemented, but not
completely", so a deliberately partial function like `getopt` (which handles only the
no-arguments case and says so in prose) is indistinguishable from a finished one in
`--static-gap`. That is a real hole in the measurement, not just in the code.
