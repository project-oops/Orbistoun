# 446. A conflict nobody stepped past

**2026-09-08** - directed, continuing 445

## What was done

**The refusal record found two bugs in itself.** Its first output showed two entries sharing one
call ordinal, one claiming a guest *stack* address as a mapping base - the wrapper was recording
`args[0]`, which is the `void **` the answer goes back through, and a note at both the wrapper and
the failing branch inside recorded one refusal twice. Both fixed; neither was visible until
somebody read the record (D604).

**Then it showed the real thing.** A refusal with base zero - the guest asking for thirty-two
mebibytes *anywhere* and being told `NoMemory`. `sceKernelReserveVirtualRange` has retried past a
conflict sixteen times since it was written; `sceKernelMapNamedDirectMemory` had the same hazard
and no mitigation.

| | before | after |
|---|--:|--:|
| refusals per run | 2 to 4 | **0** |
| distinct imports, four runs | 192, 192, 193, 193 | **193, 193, 193, 193** |

**So the drift was never a guest branch.** It was how many mappings orbistoun declined, with the
guest doing less afterwards as the consequence. D602 chased it as a branch and D603 corrected the
measurement; this is the cause.

## New hardware arrived mid-iteration

The reports directory has been replaced: six files timestamped this morning, three contexts -
`title/unknown-gpu`, `payload/unknown-gpu`, and a new one, **`title/ps4-bc`** with `libSceGnm`
mapped and `libSceAgc` absent.

`orbistoun-cli probe` reads them. **`--is-target` is needed or everything grades as `assumed`** -
without it a real console run reports `measured 0`, which is the tool being careful and is easy to
mistake for the run having established nothing.

The payload run carries a **`140-oracle/kexport-table` of 2,443 NID-to-vaddr pairs**. orbistoun's
own firmware table holds 1,867 exports from a module *file*; this is a live console's kernel
export table. `orbistoun_nid::encode_nid` already turns a hash into the base64 spelling the report
uses, so cross-referencing it against the 8,329 hashes on `wanted.txt` is tractable - and is a
piece of work rather than a tail-end one.

## Surprises

- **The instrumentation's own bugs were only findable by reading its output.** Both were mine,
  both were in code written an hour earlier, and both looked correct.
- **Four numbers changed together**: refusals to zero, imports to stable, and the two entries that
  had chased the drift as something else are now corrected rather than merely caveated.

## Next

- Cross-reference the console's kernel export table against the unnamed hashes. That is the
  largest naming input this project has been offered.
- The mapping sequence, still varying where the import count no longer does.
- The clean title's wall.
