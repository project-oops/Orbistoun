# D691 - A window length that is not a power of two is refused

**assumed** - 2026-09-15

`Window` now carries the length it spans rather than leaving every reader to reach for
`MEMORY_WORDS` independently (worklog 569). Once a length is a value rather than a constant, the
question is which values are allowed, and the answer here is narrower than it looks: only powers
of two, and never zero.

That is not a tidiness rule. Two pieces of emitted SPIR-V read the same length in two different
ways:

- `word_index` turns a guest address into an array index by **masking** with `words - 1`.
- `address_within_window` decides whether the address is in range by **comparing** against
  `words`.

A mask and a comparison describe the same set of addresses only when the length is a power of
two. At 96 words, address 100 is out of range by the comparison, but masks to index 4 - so a
store the guard exists to reject would land on the fifth word of the window instead. The bounds
check and the addressing would be quietly describing different windows, which is the exact
failure `a_store_past_the_memory_window_does_not_wrap_onto_the_start` was written to prevent, let
back in through the other door.

Zero is refused separately and for a simpler reason: `words - 1` underflows, and a window of no
words describes nothing a shader could address.

## Why at construction rather than at use

The alternative was to keep `Window` a plain pair and check the length where the SPIR-V is
emitted. Rejected: the two readers are in different functions and neither owns the invariant, so
a check in one of them is a check somebody adding a third reader will not know about.
`Window::spanning` returning `Option` puts the refusal at the only place both readers pass
through, and makes an unusable window unconstructable rather than merely unused.

The cost is that `spanning` is fallible where a struct literal was not. `Window::at` stays
infallible for the default length, so nothing that worked before now handles an error.

## What would overturn this

Hardware evidence that a guest addresses a window whose length is not a power of two. The
addressing would then have to become a real modulo rather than a mask, at a cost per access, and
this refusal would go with it. Nothing measured so far asks for that - the default length is the
only one any translated shader has used - so the restriction is free today and is written down
here so it is not mistaken for an accident later.
