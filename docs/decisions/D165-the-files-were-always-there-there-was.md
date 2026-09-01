# D165 - The files were always there; there was nothing to hand them over


**decided** · 2026-08-20

The wall at `image+0xecda` was `fopen`. The guest opens `/app0/game.bin` and
`/app0/Textures/*.gnf` - **and every one of those files was sitting in the title's own
directory the whole time.** Nothing was missing except a filesystem.

### The return value could not be made safe

`fopen` was not declared anywhere, so it fell to the default stub and answered
`0x7FFF0001`. The guest carried that through `fseek`, `ftell`, `fread` and `fclose`, then
sized an allocation from what `ftell` gave back and asked the memory subsystem for **two
gigabytes**.

Declaring it with `returns = "pointer"` made it answer null instead, per D125. The guest
then read offset four of the null and faulted immediately - same address, `read of 0x4`,
with `rdi` zero. It does not check.

So there is no stub value that works here, which is worth stating because the instinct is
to keep tuning the stub. The guest dereferences what it is handed; only a real handle backed
by real memory will do. That is the third subsystem to reach this conclusion (D151, D159),
and it should now be the default assumption rather than a discovery.

### `/app0` is the title's directory, and it is read-only

One mount serves every path observed. Read-only deliberately: **a guest writing through
`/app0` would be writing into the user's own dumped title.** Save data will need a writable
mount, and it belongs in a sandbox under orbistoun's data directory, not there.

Escaping a mount is refused by walking components rather than resolving and checking after
- a guest chooses these strings, and `/app0/../../../etc/passwd` is a path like any other.
`..` is refused outright rather than cancelled against what precedes it, because cancelling
is only sound when nothing in the path is a symbolic link, and a guest supplies these.

### What it bought

| | before | after |
|---|---|---|
| distinct imports | 41 | **47** |
| calls | 790 | **932** |
| files opened | 0 | **10** - every one it asked for |
| largest allocation | 2 GiB from a nonsense `ftell` | 128 MiB from a real one |

The allocation figure is the one worth noticing: the same code path, asking for a sensible
amount, because the number it was computing from finally meant something.

### And then `_Znwm`

The next fault was `write to 0x7fff0019` - `0x7FFF0001 + 0x18`, the same error code used as
a pointer by something else. The unnamed import was proposed against candidate names and
the hash confirmed **`_Znwm`**: C++ `operator new`, allocating a thirty-two byte object the
guest then writes a field into.

Implemented as the heap it already is - a program mixing `new` and `malloc` must see one
heap, or it frees pointers the other never issued. **A real `operator new` throws on
failure and this answers null**, which is a stated gap: throwing needs an exception runtime
that does not exist, and inventing an unwind would be worse than a caller checking a
pointer it did not expect to check.

