# D237 - Address translation was only ever implemented for signed containers


**Status:** decided (2026-08-25)

`Container::vaddr_to_offset` opened with

```rust
let Some(wrapper) = self.wrapper else { return Ok(None); };
```

so **every address of every unwrapped module resolved to nothing**. Not the ones it could
not find - all of them, including an address squarely inside a `PT_LOAD` whose `p_offset`
was valid and whose bytes were present in the file.

It surfaced from outside. obSCEne ran its own module under orbistoun and reported: two
segments placed, then *no `PT_DYNAMIC` segment, or its address could not be located* - about
a module whose `PT_DYNAMIC` sits at `0x67ec10`, inside a `PT_LOAD` spanning
`0x5e8000..0x69ed10` that orbistoun had just mapped successfully. Their own reading was
right: it was the second clause of that message, not the first.

### The comment was true and had stopped being the whole truth

> The headers' own `p_offset` values are not usable - they routinely point past
> end-of-file.

True **of a wrapped container**, where the program headers describe the decrypted image
while the file holds the wrapper's descriptor table (D052). In an unwrapped container the
offsets are the only thing there is, and they are authoritative. The function was written
when the only input was a SELF, the early return was correct then, and nothing said so when
bare ELF became an input the loader accepts and maps.

### `mapped segments []` was not the bug, and looked exactly like it

The inspector reported zero wrapper-located segments on a module with six program headers,
which reads as the parser failing. It is correct and documented: a bare ELF has no
descriptor table, so there is nothing to locate anything through. The defect was one
function further on, and a test now pins the correct behaviour so the next reader does not
re-investigate it.

### A `PT_LOAD` wins a tie, and that is a preference rather than a proof

Two headers can cover one address. A vendor segment carrying dynamic data is commonly
declared at virtual address zero - its contents are addressed as offsets into itself - and a
module whose first `PT_LOAD` also starts at zero then has two headers over the same low
range. An address in the image means the image, so `PT_LOAD` is preferred and anything else
is a fallback.

**Stated as a limit rather than dressed up.** An address that is really an offset into a
vendor segment will resolve against the `PT_LOAD` and return the wrong bytes. The wrapper
path does not have this problem because a descriptor table says which header owns which run
of file. Telling them apart without one needs the vendor segment's own conventions, which is
separate work - so it is written down here and in the source, not guessed at.

### What it unblocks, and what it does not

The dynamic table is reachable now, which is the wall obSCEne hit. Whether their module gets
all the way to imports depends on whether its `DT_SCE_*` addresses resolve, and that is the
ambiguity above - so this is honestly described as *further*, not as *works*. A guest that
can be run as often as we like, whose source we have, and which announces every call before
making it is worth the next hour regardless of where it stops next.

