# D491 - The null is a module global, and it is not a missing constructor

**measured** - 2026-09-03 (instruction bytes, then the dynamic tags that killed the obvious answer)

PPSA02664 dies at `the title's own modules+0x13dca44` reading `0x8`. This records what the
faulting instruction actually is, and kills the explanation that fits it best.

## The instruction, decoded rather than guessed

The report prints the bytes at the fault and the sixteen before it. Decoded:

```text
bytes   49 8b 4f 08        mov rcx, [r15 + 8]     <- faults, r15 = 0
before  4c 8b 7e 08        mov r15, [rsi + 8]     <- r15 came from here
```

So the null is not a register the compiler forgot to set. **`r15` was loaded from `[rsi + 8]`,
and that memory held zero.** `rsi` is `0x480001f0c330` - inside `Il2CppUserAssemblies`' own data.

The report had offered six candidate null registers, all zero, and no way to choose between
them. The instruction bytes decide it in one step, and they were already being printed.

## So a global in the module is null, and the obvious reason is wrong

A module's C++ static constructors would fill exactly that kind of global, and orbistoun runs
nothing of the sort: `init_array` is parsed in `orbistoun-elf` and **consumed nowhere** in the
workspace. `Il2CppUserAssemblies.prx` lists `DT_INIT`, `DT_INIT_ARRAY` and `DT_INIT_ARRAYSZ`
among its dynamic tags. The story writes itself.

It is not true. Read directly:

```text
Il2CppUserAssemblies: init=0x10  init_array=0x0  init_arraysz=0x0
PS5Util:              init=0x10  init_array=0x0  init_arraysz=0x0
libc:                 init=0x10  init_array=0x0  init_arraysz=0x0
```

**There is no init array.** The tag is present and empty - which is what a dynamic-tag dump
showing "INIT_ARRAY, 1 occurrence" means, and reading that as "has an init array" was the
mistake. And `DT_INIT` is `0x10` in all three, which is not a virtual address: a call to
`base + 0x10` lands in the ELF header.

An implementation that treated those as call targets was written and then removed. It would
have jumped into a header and reported it as the module's constructor.

## What that leaves

- **The null is a real, specific fact**: `[0x480001f0c330 + 8]` is zero when the guest reads it,
  and the address is in a module orbistoun placed and relocated.
- **What is supposed to fill it is unestablished.** Not `DT_INIT_ARRAY`, which is empty. All 247
  of the module's exports are vendor-encoded, so a `module_start` would appear as a hash rather
  than a name, and nothing here has yet computed that hash to look for it.
- **`DT_INIT = 0x10` is unexplained.** Identical across three unrelated modules, so it is a
  property of the format or the toolchain rather than of any one module - worth knowing before
  anything acts on the tag.

The name `sceKernelLoadStartModule` carries the word *Start*, which is the thread to pull next:
the platform's own loader does something to a module after placing it, and whatever that is, it
is not walking a `DT_INIT_ARRAY` that does not exist.

## Why this is a decision and not a note

Because the hypothesis it kills is the one anybody would reach for next, it fits every symptom,
and the code implementing it builds and runs. The only thing wrong with it is that the tag it
depends on is zero - which takes one probe to check and would otherwise have been discovered
after a module's constructor was reported as jumping into an ELF header.
