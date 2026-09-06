# 2026-09-03 - (/loop) The instruction bytes answered it, and the tags refused the answer

```
tests   1991  ->  1991   (a diagnosis; the code written for it was removed)
```

The wall is `read of 0x8` at `the title's own modules+0x13dca44`. The report listed six
candidate null registers - `rax`, `rdx`, `r9`, `r10`, `r13`, `r15`, all zero - and no way to
pick. It was also printing the instruction bytes the whole time.

## Decoded, it is one step

```text
bytes   49 8b 4f 08    mov rcx, [r15 + 8]     <- faults, r15 = 0
before  4c 8b 7e 08    mov r15, [rsi + 8]     <- and r15 came from here
```

`rsi` is `0x480001f0c330`, inside `Il2CppUserAssemblies`' own data. So **a global in the module
holds zero**, the function loaded it, and dereferenced it eight bytes in. Not a register the
compiler forgot - a field that should have been filled and was not.

Six guesses became one fact by reading four bytes that were already on screen.

## The obvious cause, which is wrong (D491)

A module's C++ static constructors fill exactly that kind of global. orbistoun runs none:
`init_array` is parsed in `orbistoun-elf` and **consumed nowhere else in the workspace** - grep
returns nothing. And the module's dynamic tags list `DT_INIT`, `DT_INIT_ARRAY`,
`DT_INIT_ARRAYSZ`. That is a complete story, it explains the symptom, and code implementing it
was written and built.

Then the values:

```text
Il2CppUserAssemblies: init=0x10  init_array=0x0  init_arraysz=0x0
PS5Util:              init=0x10  init_array=0x0  init_arraysz=0x0
libc:                 init=0x10  init_array=0x0  init_arraysz=0x0
```

**There is no init array.** A tag dump reporting "INIT_ARRAY, 1 occurrence" means the tag is
*present*, and I read it as meaning it has one. And `DT_INIT` is `0x10` in all three, which is
not an address - `base + 0x10` is the ELF header.

The implementation was removed. Left in, it would have called into a header and reported it as
the module's constructor.

## Twice in one tick, and the same check both times

Yesterday's rule was *ask whether a reported value could have come from the function named* -
that caught D490. Today it caught this in a different dress: **could `base + 0x10` be a
constructor?** It could not, and the probe took one line.

The first version of the report even printed it plainly - `initialisers Il2CppUserAssemblies 1
to run, first 0x480000000010` - three modules, three identical `+0x10`. Identical values across
unrelated modules are the tell.

## What is actually open now

- `[0x480001f0c330 + 8]` is zero when read. Specific, reproducible, and in a module this loader
  placed.
- **What fills it is unestablished.** Not `DT_INIT_ARRAY`; it is empty.
- All 247 of the module's exports are vendor-encoded, so a `module_start` would be a hash, not a
  name. Nothing here has computed that hash to look for it - `orbistoun-cli` has no `nid` verb
  (that is obSCEne's tool), which is the first small thing the next tick needs.
- `DT_INIT = 0x10` across three unrelated modules is a property of the format or the toolchain,
  not of a module. Worth understanding before anything acts on the tag.

The word *Start* in `sceKernelLoadStartModule` is the thread to pull: the platform does
something to a module after placing it, and it is not walking an array that does not exist.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-341 and D466-D491.

**Next**: compute the hash for `module_start` and look for it among the module's 247 exports.
That needs a `nid` verb in `orbistoun-cli`, which is a small thing worth having anyway - the
hash is the project's own and the tool that computes it lives in a sibling repository.
