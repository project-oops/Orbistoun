# D391 - What real hardware said, and which of it orbistoun had wrong


**assumed** - 2026-08-30

A parallel effort put obSCEne on a real console and against PS5PCEM, and sent back a list of
measured format facts. Three of them were live defects here. Two more things it named were
already right, which is worth recording as clearly as the failures.

### `PT_DYNAMIC` has no address on a real title

The headline. On a real eboot the dynamic segment carries `vaddr 0` and lives at the **tail of
`PT_SCE_DYNLIBDATA`**, which is *also* at `vaddr 0`:

```text
PT_SCE_DYNLIBDATA  off 0x8c130  filesz 0x3760  vaddr 0   -> ends 0x8f890
PT_DYNAMIC         off 0x8f450  filesz 0x0440  vaddr 0   -> ends 0x8f890
```

`dynamic_bytes` resolved it **by address**, so it asked "which segment covers address zero" -
and two of them do. Which one won was the order they happen to appear in the header table: the
right answer by luck, or the start of the vendor blob, which parses as a dynamic table of
nonsense rather than as an error.

It reads `p_offset` now **in a bare container**, which is where its bytes are and is
unambiguous. The guard was watched failing on the old code first, and failed with exactly the
predicted symptom - offset `0x300`, the vendor segment, instead of `0x380`.

### And then it broke every real title, which is the part worth writing down

The first version applied `p_offset` to *both* paths. In a **wrapped** container `p_offset`
describes the decrypted image while the file holds the wrapper's descriptors, so it routinely
points past end-of-file - and all six titles in `titles/` went from loading to
`no usable dynamic table` in one line.

Two things about that are worse than the bug:

- **The answer was already written down.** `vaddr_to_offset`, the function immediately above,
  has said exactly this since D052: *the headers' own `p_offset` values are not usable in a
  wrapped container.* The change was made without reading the paragraph explaining why it
  would not work.
- **It was verified against the wrong material.** Bare payloads and a synthetic fixture, both
  of which are unwrapped, so both agreed. Six real titles were sitting in the repository the
  whole time and none of them was run until afterwards.

A finding measured on hardware describes *hardware's* file layout. Generalising it to every
container this project reads is an inference, and it needed the same evidence as anything
else. Corrected: bare uses `p_offset`, wrapped goes through the wrapper, and all six titles
read again - `PPSA02664` back to `image+0xafc959` with 23 imports and 222 calls, matching the
record from 2026-08-23 exactly.

### The same coincidence was hiding two more

`info.strtab` is a **virtual address** for an ordinary module and an **offset into
`PT_SCE_DYNLIBDATA`** for a vendor one. D247 established that and gave `table_offset` the job
of knowing the difference - and fixed *one* of the sites that needed it. Two others still
resolved the string table by address, and worked for the same reason the dynamic table did:
the vendor segment sits at address zero, so the two readings coincide until something else
also claims that address.

Fixed at the source: both go through `table_offset`.

### `sceKernelIsStack` knew about one stack

It distinguishes a stack address from a static correctly, and refuses to guess when nothing
told it where the stack is - both right. But only the **main** stack was ever recorded, so a
guest *thread* asking about a local of its own was told no.

That is the wrong answer to the only question the function is ever asked, and it is the same
blind spot the argument dumps had (D387): a span that comes into existence after the run
starts, and a table filled before it. A thread now records its own span in a thread-local,
which is what "the calling thread's stack" means - a registry of every guest stack would answer
yes about another thread's, which is a different question.

### Two that were already right

**Mutex recursion.** `Recursion::Forbidden` is the default, *because it is POSIX's, and because
the alternative turns a real double-lock bug in the guest into silence* - with a test that
watches a non-recursive lock refuse its owner rather than deadlock.

**`sceKernelLoadStartModule` refuses**, and says why: orbistoun places one executable and has
no way to bring another in, so answering a handle would tell a guest a library it is about to
call is present. That is the honest answer to a capability it does not have.

### Still outstanding, named rather than done

- The three `e_type` values, each refused by name - `0xFE00` eboot, `0xFE18` bundled `.prx`,
  `0xFE10` which no console accepts and every emulator does.
- `DT_SCE_ORIGINAL_FILENAME` being required for a shared library.
- Library ids 0-based and module ids 1-based, with an executable declaring no export library.
- The process parameter block: `0x60` bytes, magic `ORBI`, three pointers to counted
  structures, and **libkernel writing into the first at `+0x28`** - a null there is a fault at
  absolute `0x28` before the entry point, which is precisely the kind of silent death worth
  faulting legibly on.
- Resolving modules **by name**, which would let a probe ask whether a library exists rather
  than requiring it.
- The sandbox library prefix being randomised per boot, so no absolute library path can be
  hardcoded anywhere.

### The lesson worth copying, which this repository keeps relearning

> Name the layer that failed, not the structure being sought.

Two of that effort's hours went to `Failed to load SCE_DYNLIBDATA: 5` raised by the block layer
before any tag was read, and to an `e_type` complaint about a value that was correct. This is
the same finding as five separate ones here today - a tool reporting what it *can* see in the
words it uses for a real result. It is cheap when writing the message and expensive for
everyone downstream.

### And the structural recommendation, which is not taken yet

orbistoun still holds its own `dynamic.rs`, `reloc.rs` and `nid`, and the facts above are in
`selfish` with tests. Taking `selfish-elf`/`selfish-container` rather than re-deriving is the
migration this repository's own notes say has not happened, and every one of these three
defects is an argument for it: they were all *already fixed somewhere else*. Not done here
because selfish is being actively changed by the effort that produced this list, and merging
into a moving target is how both copies end up wrong.

