# 786. PPSA28061's GetRegisterDefaults2 return structure is decoded (records at +0x30, count at +0x38); a count-zero struct lets the guest skip the register search gracefully, a testable path to move the wall

**2026-09-21** — worklog 785 committed the loop to synthesising `sceAgcGetRegisterDefaults2` for
PPSA28061 and named step 1 as reading the call site to fix the structure the title needs. This did that.
The structure is small and, importantly, the title has a graceful empty path - so there is a way to move
the wall that does not depend on data orbistoun does not have.

## The return contract, from the call site

Disassembling `image+0x10b9e0` in PPSA28061's eboot:

```text
0x10b9e0  push rbp; mov rbp,rsp
0x10b9e4  call sceAgcGetRegisterDefaults2        ; -> rax = struct*
0x10b9e9  mov  ecx, [rax+0x38]                   ; count : u32     <-- the fault (rax = placeholder)
0x10b9ec  test rcx, rcx
0x10b9ef  je   0x10ba2b                          ; count == 0 -> skip the whole search
0x10b9f9  mov  rdx, [rax+0x30]                   ; records : u64*
0x10ba10  cmp  dword [rdx + rsi], 0xe24f806d     ; find the record whose id == 0xe24f806d
```

`rcx*4` then `rdi = rcx*12` fixes the record stride at **12 bytes**, keyed on a `u32` id at offset 0. So
`sceAgcGetRegisterDefaults2` returns a pointer to a struct `{ ...; records* @ +0x30; u32 count @ +0x38 }`,
and the records are `{ id:u32, value:u64 }` (12 bytes). The title looks up **one** register default by id
`0xe24f806d`.

## Two ways to answer it, and which is available

- **Full/faithful:** a records table for register-set `arg0 = 0xd` containing `0xe24f806d`'s default.
  That id is not in orbistoun (`registers.rs` is packet *decode*, not reset *defaults*), and it is an
  AGC-specific register key, not a raw Mesa offset - so the table is not sourceable in-tree today, and
  obSCEne cannot call the function (a non-export, worklog 769). Real data is a later step.
- **Empty, and honestly minimal:** return a struct whose **`count` (+0x38) is 0**. The title's own code
  then takes the `je 0x10ba2b` branch and skips the register search entirely - a path the title is built
  to handle, not garbage. It applies no override for `0xe24f806d`, which is a real (if incomplete)
  answer, marked `assumed`, distinct from the placeholder that crashes.

The empty answer is the honest floor: it stops the placeholder-as-pointer fault with a value the title
accepts, and it measures whether the register default actually matters early or the run walks on without
it. That is the D226 second observation the fix needs anyway.

## Next

Implement `sceAgcGetRegisterDefaults2` in `orbistoun-gpu` to return a pointer to a small static struct
with `count = 0` (and a null/empty records pointer), using the same data-return path ASTRO BOT's run
noted ("imports named data and were given storage rather than a stub"). Record it `known_by = assumed`
with the call-site structure as its justification, wire it into `implementations()` and the knowledge
file, and run PPSA28061: if the wall moves, the register defaults are not gating early execution and the
next fault names what is; if it faults straight back needing `0xe24f806d`, that pins the data to source
next. Either outcome is progress the placeholder could not give.

## Gate state

No code changed - a synthesis step that decodes `sceAgcGetRegisterDefaults2`'s return structure from
PPSA28061's call site (`records @ +0x30`, `count @ +0x38`, 12-byte id/value records, lookup of
`0xe24f806d`) and identifies a `count = 0` struct as the honest, testable minimal answer. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
