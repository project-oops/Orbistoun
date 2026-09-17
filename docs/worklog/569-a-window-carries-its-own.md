# 569. a window carries its own length

A `Window` named a base address and nothing else, while the memory it described
was always `MEMORY_WORDS` long. Two separate places then had to agree about that
length by coincidence: `declare_counts` sized the SPIR-V array from the constant,
and `address_within_window` bounds-checked against the same constant reached
independently. Nothing connected them, so a window of any other length was not
expressible and a mismatch between the two would have been silent.

`Window` now holds the length it spans.

- `Window::at(base)` keeps the old meaning - the default length, unchanged.
- `Window::spanning(base, words)` is the new one, and it returns `Option`.
- `Window::words()` reads it back.
- `declare_counts` takes `memory_words` and the call site passes `window.words()`,
  so the declared array and the bounds check are now the same number by
  construction rather than by agreement.

## Why `spanning` refuses

It returns `None` for zero words, and for any length that is not a power of two.
That is not tidiness - the two readers disagree at every other length:

- `word_index` masks the address with `words - 1`.
- `address_within_window` compares against `words`.

A mask and a comparison only describe the same set when the length is a power of
two. At, say, 96 words, address 100 fails the comparison but masks to index 4 -
the wrap the guard exists to prevent, arriving through the other door. Refusing
the length at construction is the only place the two can be made to agree once.

Zero is refused separately: `words - 1` underflows, and a window of no words
describes nothing a shader could address.

## What was made to fail

Five tests, each mutation-checked - the guard was inverted and the assertion
watched to fail before it was believed:

- `a_window_length_that_would_alias_is_refused` - 96 words, the aliasing case.
- `a_window_of_no_words_is_refused` - zero.
- `a_power_of_two_window_keeps_its_length_and_base` - both fields survive.
- `the_default_window_is_the_length_it_always_was` - `at()` did not change.
- `a_widened_window_reaches_the_declared_buffer` - a longer window actually
  reaches further into the emitted array, which is the point of the change.

`cargo test -p orbistoun-translate`: 18 + 2 + 114 pass, 0 fail.

## Gate state, honestly

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets -D warnings`
were both clean when run. `cargo test --workspace` did **not** compile, for a
reason that is not this unit: `console_fragment_module` in
`orbistoun-gpu-vulkan/tests/console_fragment.rs` had just gained a `window`
parameter without its two call sites being updated (lines 116 and 230). That is
another unit's edit caught part-way through, and the `Window` it should be passed
is the open question of worklog 563 rather than a mechanical default - so it was
left alone rather than guessed.

## Surprise worth keeping

`cargo clippy --workspace --all-targets` and the same command with `-- -D warnings`
are **different cache fingerprints**. The plain form replayed cached success from
an older tree while the `-D warnings` form rebuilt and failed. Roughly ten minutes
went into chasing errors that appeared and vanished between runs before that was
the explanation. When two clippy invocations disagree, the one that rebuilt is the
one telling the truth; alternating the two forms is what makes a gate look flaky.
