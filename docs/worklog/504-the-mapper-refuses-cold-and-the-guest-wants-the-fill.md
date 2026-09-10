# 504. The mapper refuses cold, and the guest wants the fill not the code

**2026-09-10** - loop, continuing 503 with obSCEne's measurement of `sceKernelMapperGetParam`

503 filed `REQ-...1332Z-b7e2` — dump what `sceKernelMapperGetParam` fills, because it is the wall
behind the shader and it is libkernel, not GPU. obSCEne answered within the hour, and the answer
sharpens the wall rather than removing it.

## What obSCEne measured

`20260910-154133-payload.obs.log`, new section `137-kernelcall/mapper-param`:

```
sceKernelMapperGetParam  address 0x800031d60   (libkernel+0x31d60, matches D642)
                         rc      0x80020006
param filled-bytes 0     38 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

So a **cold call** — a bare `0x38`-size-prefixed 56-byte buffer, exactly the shape D643 says the
guest passes — **refuses with `0x80020006`** (kernel error family `0x8002`, errno 6, ENXIO "no such
device / not set up") and **fills nothing**. The buffer comes back as just the caller's size-prefix.

## The experiment the measurement enabled, and what it ruled out

The hypothesis worth testing: the title ships and runs, so if hardware returns `0x80020006` here,
the title must *handle* that error — and orbistoun aborts only because it returns the unrecognised
`0xf7ff0001` placeholder instead of the real code. Tested it with the measured value:

| return | PPSA28061 |
|---|---|
| `0xf7ff0001` placeholder | abort at 334 calls |
| `0x80020006` **measured** | abort at 334 calls — verdict **BACK** |
| `0x0` success (prop) | 956 calls |

**Returning the real error does not move the wall.** The guest aborts on `0x80020006` just as it does
on the placeholder. So the title is **not** getting this error on hardware — it gets a success with a
filled struct. obSCEne's cold call does not reproduce the title's conditions, the same way its
synthetic shader header got `0x8a6c002f` instead of a real object (D556/D565). The lever is the
**fill**, not the code — recorded on the `sceKernelMapperGetParam` entry, `known_by = measured`.

This is worth as much as a wall-move: a measured value ruled out a plausible fix that would otherwise
have looked reasonable, and pinned that a successful fill is the only thing that advances here.

## The fork that decides tractability

What warms the mapper so `GetParam` succeeds? The abort chain is
`sceAgcDriverRegisterDefaultOwner → sceAgcCreateShader → sceKernelMapperGetParam → abort`, three
consecutive startup calls. So "the mapper" is one of:

- **(a) a plain kernel object** warmed by some libkernel setup obSCEne can call before `GetParam` —
  then a warm-then-dump of the filled 56 bytes settles it, and it stays the tractable non-GPU piece
  503 hoped for; or
- **(b) the GPU/AGC memory mapper** established by the adjacent `sceAgcDriverRegisterDefaultOwner` —
  then a successful fill needs libSceAgc loaded, folding this back into the `8ef4`/`7a5d` GPU-leg
  block.

Which one it is is the whole question, and it is obSCEne's to answer (it can try warming the mapper;
orbistoun cannot see what libkernel state the call reads). Updated b7e2 with exactly this fork.

## Honest status of the "not GPU-blocked" claim from 503

Softened, not withdrawn. The mapper-param wall is *measurable* in the sense that obSCEne can and did
call it — and that measurement was worth having. But a successful *fill* may still route through the
GPU library. 503's real, surviving point stands: the walls are a chain of out-parameter structs,
propping moves the guest mechanically (334→956 confirmed), and chasing each one on measured data —
even to a negative result like this — is the honest way forward, versus declaring the whole chain
GPU-blocked from the shader alone.

## Next

- Await obSCEne's (a)/(b) determination on b7e2.
- If (a): implement `sceKernelMapperGetParam` to return 0x0 and write the dumped 56 bytes; re-run
  Earthion for the first *informative* wall past 334.
- If (b): the mapper joins the GPU-leg ask, and the tractable-kernel-piece hope was wrong — which is
  itself worth knowing and stops me re-proposing it.
