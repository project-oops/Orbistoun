# 426. The wall was the crash reporter, and the file path it died on has a name now

**2026-09-07** - directed, continuing 425

## The fault everyone has been reading is a symptom

PPSA03416's wall has been recorded as `image+0x1389269`, a read of `0xa0`, since 2026-09-04.
The report's own hint names the base: `r14` is zero and the access is `r14+0xa0`. The
instruction immediately before the fault is `mov r14, [rip+0x6a73a7]`, so the null is a
**global** at `image+0x1a30610`, not a return value.

A write watchpoint settles what happens to it:

```text
orbistoun: watching 0x400001a30610+8 write
orbistoun: watchpoint 0x400001a30610 (image+0x1a30610) touched after the access at
           0x400000afff02 (image+0xafff02); it now holds 0x0
```

**One write, before the guest's first line of output, and it writes zero.** Nothing ever gives
it a value. So the fault is a singleton that was never constructed, read as `[p+0xa0] -
[p+0x98]` - the `end - begin` of a container inside it.

And the frame it faults in is the giveaway. The call path is `0x137d34d <- 0xf17458 <-
0xf269e7`, while `sceSystemServiceReportAbnormalTermination` was called from `0xf1689f` and
the six `strlen` calls that follow it come from `0xf168b5` through `0xf17b16` - one
contiguous run through the same function. **The guest is inside its own abnormal-termination
reporter when it dies.** It had already given up; the null global only decided how.

## What it gave up on: the platform's asynchronous file path

Two lines the guest prints, in its own words:

```text
submitCommandBufferAndGetResult error=2147418113
waitCommandBufferCompletion error=2147418113
```

`2147418113` is `0x7fff0001` - **orbistoun's own `Unimplemented` placeholder**, handed back to
the guest and printed by it. Both lines sit between `LocalFileSystemPS5::SetupArchive` and
`LocalFileSystemPS5::Enumerate`, so this is the title's *filesystem* layer, not its graphics
layer.

The title imports four functions for it, and every one is declared and unimplemented:

| import | called | 
|---|---|
| `libkernel::sceKernelAprResolveFilepathsToIdsAndFileSizes` | yes |
| `libkernel::sceKernelAprSubmitCommandBufferAndGetResult` | yes |
| `libSceAmpr::sceAmprAprCommandBufferConstructor` | not reached |
| `libSceAmpr::sceAmprAprCommandBufferReadFile` | not reached |

**This is the answer to 425's open question.** Five files opened, one read of zero bytes: the
guest opens them by name and then reads them through a command buffer, which nothing here
serves. It never needed `read`.

## The second error line named a function

Forcing the submit call to answer zero removed the first line and left the second, so the two
came from different places. Three unnamed imports were each forced to a distinct value in one
run to find which:

```text
waitCommandBufferCompletion error=4369        # 0x1111 - libkernel::0x23020f8e2805acae
```

That hash is on `symbols/wanted.txt`, one of 8,329 the project cannot name. The guest's own
wrapper is called `waitCommandBufferCompletion`, and its sibling wrapper
`submitCommandBufferAndGetResult` is the platform name `sceKernelAprSubmitCommandBufferAndGetResult`
with `sceKernelApr` prefixed and nothing else changed. Applying the same transformation and
trying the obvious truncations:

```text
0xce925bf6d363cd2e  sceKernelAprWaitCommandBufferCompletion
0x23020f8e2805acae  sceKernelAprWaitCommandBuffer     <-- the import
```

**`sceKernelAprWaitCommandBuffer`, proved by the hash agreeing.** Nothing was consulted; the
candidate came from a string the guest printed and the naming pattern of the function beside
it. The control in the same command re-derived
`sceKernelAprSubmitCommandBufferAndGetResult` to the hash the import table already carries.

## Forcing the returns is not enough, exactly as the loop says it would not be

With all three answering zero, both complaints disappear and **the wall does not move**: same
fault, same address, `files 1 reads, 0 KiB`. The guest wants the command buffer *executed*,
not a success code - the two-dimensional case THE_LOOP.md describes, where the return and the
out-parameter have to move together (D283, D286).

So the intervention is a diagnosis rather than a fix, and it is recorded as one.

## Surprises

- **`libSceAmpr` is filed under `orbistoun-gpu`**, "by name association with the graphics
  submission path - a placement worth revisiting if it turns out to belong elsewhere". It has.
  The title reaches it from `LocalFileSystemPS5`, and the two `libkernel` halves of the same
  subsystem live nowhere near it.
- **The name was unreachable by the generator, and cheaply reachable by hand.** `Command`,
  `Buffer`, `Wait` and `Submit` are all in the vocabulary; `Apr` is not, and a five-part shape
  for `sce`+`Kernel`+`Apr`+verb+noun+noun does not exist. The database already holds thirteen
  `sceKernelApr*` names from string harvesting, and no `Wait` among them.
- **`scePthreadSelf` answers a different value on every run** - `0x23b5e3ade80` one run,
  `0x22841ab0610` the next. A host address reaching the guest, against D181/D238's requirement
  that two runs of one build behave identically. Not investigated here.

## Next

- Whether the near-miss transformation generalises: take an identifier-shaped string the guest
  printed, prefix each `sce<Module>` the title imports from, and try dropping trailing words.
  It would have found this name mechanically. That is a new source rather than a new word, so
  it is a decision rather than an assumption.
- `sceKernelAprWaitCommandBuffer` has no home yet. `symbols/generated.json` is generated and
  `symbols-audit` re-derives every entry, so landing a name found this way needs either the
  source above or an accounted place on the ceiling.
- The Apr subsystem itself, which is step 18 and the real work: resolve paths to ids and
  sizes, construct a command buffer, read files into it, submit, wait.
