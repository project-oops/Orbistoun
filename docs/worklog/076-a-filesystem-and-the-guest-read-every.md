# A filesystem, and the guest read every file it asked for


`fopen` was the wall. The guest wanted `/app0/game.bin` and ten `.gnf` textures - all of
them sitting in the title directory already. The files were never missing; there was
nothing to hand them over (D165).

Worth recording how the diagnosis went, because the tooling built earlier today did all of
it: the ordered call tail showed `fopen` followed by `fseek(0x7fff0001)`, which is our own
error code being carried around as a `FILE *`. Then reading the path string straight out of
the module showed exactly which files it wanted.

**No stub value works here.** Unimplemented, `fopen` answered an error code and the guest
carried it through four more calls, sizing a two gigabyte allocation from the nonsense
`ftell` returned. Declared as pointer-returning it answered null, and the guest read offset
four of the null and faulted at the identical address. It does not check. Third subsystem
to land on this - handles must be backed by real memory - and it should stop being a
discovery each time.

`orbistoun-fs` now has a mount table and an open-file table. `/app0` maps to the title's
directory, **read-only**, because a guest writing through it would be writing into the
user's own dumped title. Escapes are refused by walking components rather than resolving
first: a guest chooses these strings.

| | before | after |
|---|---|---|
| imports | 41 | 47 |
| calls | 790 | 932 |
| files opened | 0 | 10 |
| largest allocation | 2 GiB, from garbage | 128 MiB, from a real `ftell` |

Then the next fault named its own cause - `write to 0x7fff0019`, our error code plus 0x18.
Candidate names against the hash confirmed `_Znwm`, C++ `operator new`. Wired to the
existing heap; a program mixing `new` and `malloc` must see one heap.

Now at `sceSysmoduleLoadModule` returning an error the guest dereferences. Same pattern
again, different function.

