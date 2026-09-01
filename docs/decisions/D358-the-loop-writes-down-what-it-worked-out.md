# D358 - The loop writes down what it worked out


**decided** · 2026-08-29 · the answer from D357 existed only as terminal output

D357 had the loop settle a question open since D218 - *the guest walks the map by feeding back
each region's end* - and then print it. `CLAUDE.md` is explicit that anything existing only in
a conversation is already lost, and this project had just spent a day applying that rule to
measurements (D355). It applies to answers too.

An answered question is now a **proposal**: a patch against the entry that asked it, in
`patches/`, inert until somebody promotes it. `known_by = "measured"` rather than
`guest-observed`, and the distinction is real - the guest was not merely watched, it was put
in a situation built to separate two readings and its answer was arithmetic.

Re-running does not re-propose. A settled question producing a fresh patch every turn is how
`patches/` becomes a directory nobody reads.

### Two bugs, both of the same kind: applies is not valid

**The first patch inserted a key the entry already had.**

```
duplicate key `edge_cases` in table `function`
```

`git apply` accepted it without complaint and the result was a file the tool could no longer
read. A key that already exists has to be *joined*, not added again - so `key_line_of` finds
the entry's own list, scoped to that entry because searching the file would find whichever
came first and put one function's answer into another's entry: a patch that applies, parses,
and lies.

**The second left the line ending in a space.** The array is multi-line, so joining onto
`edge_cases = [` produced a trailing separator. `git apply` warned; nothing failed. Trimmed.

Both are the shape D328 already recorded: *`git apply` checks that a patch fits the text.
Nothing in it checks that the result means anything.* Three times now a generated patch has
applied cleanly and been wrong - the invented `found_by`, the duplicate key, the trailing
space - and each was caught by a checker that understood the format rather than by review.

### The cycle, end to end

```
turn → answers the question → writes a proposal → applies → the file parses
     → the tool reads it back → the next turn sees it settled and proposes nothing
```

Worth noting what caught the last step: `knows` did not show the answer after the patch
applied, because knowledge files are `include_str!`-ed and the running binary held the old
copy. That is D260, unchanged and still the first thing to suspect when a data change appears
not to have landed.

### What has not changed

Three of the four questions on that entry are still open, and the one settled was settled
because its discriminator was arithmetic. The others - what the second argument means, what
the terminal return code is - are not that shape, and D357's boundary still holds.

