# 466. Four numbers

**2026-09-09** - directed, continuing 465

Both my requests came back. One delivered, one `not-possible` with the complete reasoning - and the
`not-possible` one carried the wall's measurement anyway.

## The probe started making real syscalls

The delivered request added a per-leg syscall-route probe. Run locally it reports something nobody
asked for: `gadget = 0x700000008c0a` - **orbistoun's own thunk table**, `getpid` plus ten, the nop
sled D400 laid down for exactly this. The probe's new build hunts for a gadget in title mode and
finds it, so a guest here issues real syscalls for the first time.

The run went backwards - 223 imports to 187, a clean exit replaced by a fault at `0x5e2d` - and the
report said why without being asked: **4,213 calls to syscall 603**, first argument walking
backwards through the image in `0x4000` steps. A program mapping its own address space, never told
where a region ends.

603 is `sceKernelVirtualQuery`, implemented here for months. Answered by name, refused by number
(D641).

## Four numbers, in two steps, each observed first

| number | name | reached |
|---|---|---|
| 603 | `sceKernelVirtualQuery` | 4,213 calls |
| 572 | `sceKernelAllocateDirectMemory` | **only after 603 was bound** |
| 573 | `sceKernelMapDirectMemory` | only after 572 |
| 574 | `sceKernelReleaseDirectMemory` | only after 573 |

The last three did not exist in any trace until the first was answered. Added in two steps rather
than one, because that is what made them observed rather than guessed - which is the bar
`vendor-syscalls.toml` sets for itself. 585 and 586, the current-generation half of obSCEne's
conditional, are deliberately absent: no guest here has asked.

```text
imports  225 distinct (+38), 432605 calls (+427701)
fault the guest called exit   (was 0x5e2d)
verdict  FURTHER  executed code it could not reach before
```

**225 imports, past the previous best of 223.** Against the console's pkg leg: 115 passed-there
failed-here down to **5**, seventeen distinct findings down to **five** - three libSceNet, one
input-SDK poll, and the weak-symbol relocation (D633).

## The AGC wall, bounded

The `not-possible` resolution was right about the legs, and the sweep carried `166-agc/create-shader`
measurements from the payload leg regardless. **Every header shape answers `0x8a6c002f`** - the
well-formed one, the `0xd8` and `0x118` shapes, and PPSA03416's own `0x108` shape that I supplied -
and `out-after` is unchanged poison, so the call writes nothing.

So the header is not the variable. Forcing that measured code into PPSA03416 moved nothing, as
forcing `0x0` did not yesterday. The guest acts on the out-parameter, not the return.

## Surprises

- **Binding one number revealed three more.** The memory walk was a gate: nothing behind it appeared
  in any trace until it opened. A syscall table cannot be filled in by inspection, only by running.
- **Orbistoun's gadget was already right and nobody had noticed it being used.** D400 built the sled
  a year of payloads never exercised, and the first guest to use it did so this morning.
- **The frontier now records a worse run.** 187 imports faulting beats 225 exiting cleanly, because
  the faulting one reached the flip rung. The ladder is working as designed and the summary reads
  backwards; noted, not changed.

## Next

- Two new requests filed: enumerate the kernel export table's NIDs (four unnamed hashes, one
  aborting a title), and dump libc's exported data objects (PPSA21564's zeroed vtables).
- Apply the title-module binding to module relocations - still needs `relocate.rs`, still blocked.
- libSceNet is now three of the five remaining differential findings.
