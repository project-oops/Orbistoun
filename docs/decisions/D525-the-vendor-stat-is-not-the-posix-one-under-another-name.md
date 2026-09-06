# D525 - The vendor `stat` is not the POSIX one under another name, and it opened eight functions

**guest-observed** - 2026-09-03 (A/B against the same build, the stub forced back)

`sceKernelStat` was declared at arity 2 and registered to nothing, while `stat` - the same
call, POSIX-named - had been implemented all along. Closing that gap took the guest from 181
distinct imports to 188.

## They agree about success and disagree about failure

That is the entire difference, and it is the reason this is code rather than a second entry in
a table:

```text
POSIX  stat        answers -1  on failure
sce*   sceKernelStat  answers 0x8002_00xx, as sceKernelMkdir beside it already does
```

Registering the POSIX function under the vendor name hands a caller `0xffff_ffff_ffff_ffff`
where it tests for a negative 32-bit vendor error. Those are not the same number, and the
difference is invisible until a guest branches on it (D125).

The success path and the structure written are **shared**, so which generation of `struct stat`
a guest is given stays one decision (D374) rather than two that can drift apart.

`ENOENT` for a path nothing answers, because that is what the failure is: `facts_of` returns
nothing only when neither a host file nor a mount point is behind the name. **Which errno the
console answers is unmeasured**, so the test pins the family and the sign - what a caller
branches on - rather than a specific code no run has established.

## What it opened

A/B on the same build, with `ORBISTOUN_RETURN=sceKernelStat:0x7fff0001` forcing the stub back:

```text
with stat   188 distinct, 415,532 calls
as a stub   181 distinct, 415,359 calls

reached only once stat answered:
  sceKernelOpen, sceKernelClose, sceKernelPread, sceKernelAllocateDirectMemory,
  sceKernelDeleteSema,
  sceAmprAprCommandBufferConstructor, sceAmprCommandBufferConstructor,
  sceAmprCommandBufferSetBuffer
```

**The guest starts opening and reading files, and starts using the decompression library.** That
is the asset-loading path, which had been sitting behind one unanswered question about whether a
file was there.

## And one function stopped being called, which is the corroboration

Eight gained, one lost - `sceKernelMkdir`. Net seven, which is what the report says.

The guest stops creating a directory **because `stat` now tells it the directory already
exists**. That is a change in the right direction for the right reason, and it is worth more
than the eight gained: a wrong implementation adds calls, but it does not make a guest stop
doing something that had become unnecessary.

## Two gates caught what I did not

Both on their first run after the change, which is what they are for:

- `every_implemented_function_is_written_down` - the knowledge entry was missing.
- `every_implementation_is_also_declared_here_or_says_why_not` - **`sceKernelStat is implemented
  but not declared`**. The body belongs in `metadata`, beside the POSIX form it shares its
  success path with; the *name* belongs to `libkernel_fs`, which is where it is declared.
  Registering it beside the POSIX ones offered it under the wrong library.

That second one is a genuine structural error - the function would have been reachable under a
library that does not declare it - and nothing in the change itself would have surfaced it.

## What did not change

The wall. 188 distinct now, but still `read of 0xa0` at `image+0x1389269`. More of the interface
is reached before it, which is what `FURTHER` means and all it means.
