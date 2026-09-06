# 2026-09-03 - (/loop) The vendor `stat` is not the POSIX one under another name

```
181 -> 188 distinct imports (+7)   415,356 -> 415,532 calls
suites 125   tests 2011   clippy/fmt/identity clean
wall unchanged: read of 0xa0 at image+0x1389269
```

Eleventh cron tick. `sceKernelStat` was declared at arity 2 and registered to nothing, while
`stat` - the same call, POSIX-named - had been implemented all along.

## They agree about success and disagree about failure

```text
POSIX  stat           answers -1 on failure
sce*   sceKernelStat  answers 0x8002_00xx, as sceKernelMkdir beside it already does
```

Registering the POSIX function under the vendor name hands a caller `0xffff_ffff_ffff_ffff`
where it tests a negative 32-bit vendor error. **Not the same number**, and invisible until a
guest branches on it (D125). The success path and the structure written are shared, so which
generation of `struct stat` a guest gets stays one decision (D374).

`ENOENT` because that is what the failure is. **Which errno the console answers is unmeasured**,
so the test pins the family and the sign - what a caller branches on - not a code no run has
established.

## What it opened

A/B on one build, `ORBISTOUN_RETURN=sceKernelStat:0x7fff0001` forcing the stub back:

```text
with stat   188 distinct, 415,532 calls
as a stub   181 distinct, 415,359 calls

only once stat answered:
  sceKernelOpen, sceKernelClose, sceKernelPread, sceKernelAllocateDirectMemory,
  sceKernelDeleteSema, and three libSceAmpr command-buffer calls
```

**The guest starts opening and reading files and using the decompression library** - the
asset-loading path, sitting behind one unanswered question about whether a file was there.

## One function stopped being called, and that is the corroboration

Eight gained, one lost - `sceKernelMkdir` - net seven. The guest stops creating a directory
**because `stat` now tells it the directory exists**. Worth more than the eight gained: a wrong
implementation adds calls, but it does not make a guest stop doing something that has become
unnecessary.

## Two gates caught what I did not

- `every_implemented_function_is_written_down` - the knowledge entry was missing.
- `every_implementation_is_also_declared_here_or_says_why_not` - **"sceKernelStat is implemented
  but not declared"**. The body belongs in `metadata`, beside the POSIX form it shares a success
  path with; the *name* belongs to `libkernel_fs`, where it is declared. Registering it beside
  the POSIX ones offered it under the wrong library - a real structural error that nothing in
  the change itself would have surfaced.

## Break watched to fail

```text
("sceKernelStat", stat)  ->  "NOT the POSIX -1 - that is the whole reason this is not the
                              POSIX function under a second name"        FAILED
```

Decision: [D525](../decisions/D525-the-vendor-stat-is-not-the-posix-one-under-another-name.md).
