# 516. The mapper abort branch, disassembled: a hard must-succeed gate reading four filled qwords

**2026-09-12** - operator asked to disassemble `game.bin` at the mapper abort branch (the D677 next step)

Worklog 515 / D677 located PPSA28061's abort in the engine module at `0x480000...`, `0x4d` bytes after
the `sceKernelMapperGetParam` call, and named reading that branch as the next move. Done. The branch is
now read and verified, and it answers the question the measured returns could not.

## How the bytes were obtained

`game.bin` is encrypted (`orbistoun cli inspect` reports "not an ELF"; first bytes are high-entropy),
and no loadable ELF module's executable segment reaches offset `0xa1c760` (eboot exec ends `0x13f6fc`,
libSceNpCppWebApi `0x440a1e`, libc `0x11d99e`). So the module at `0x480000...` cannot be inspected as a
file. Instead the executing bytes were read straight from guest memory with
`ORBISTOUN_WATCH=0x480000a1c750+0x70` - the region "did not change while the guest ran" (confirming it is
code), and the snapshot gave the instruction stream. Verified with capstone (x86-64) rather than trusted
to a hand decode, because a wrong decode here is the same plausible-output failure principle 3 forbids.

## The branch

```x86asm
0x480000a1c753:  mov   qword ptr [rsp], 0x38     ; size prefix = 56, the D643 struct shape
0x480000a1c75b:  call  0x480000a26340            ; sceKernelMapperGetParam(&struct)
0x480000a1c760:  test  eax, eax
0x480000a1c762:  jne   0x480000a1c7a8            ; rc != 0  ->  abort, no fallback
0x480000a1c764:  mov   rax, [rsp + 0x08] ; mov [rbx], rax
0x480000a1c76c:  mov   rax, [rsp + 0x10] ; mov [r12], rax
0x480000a1c775:  mov   rax, [rsp + 0x18] ; mov [r15], rax
0x480000a1c77d:  mov   rax, [rsp + 0x20] ; mov [r14], rax
0x480000a1c785:  mov   rax, [r13] ; cmp rax, [rsp + 0x40] ; jne 0x...a1c7a1  ; stack canary
0x480000a1c790:  xor   eax, eax ; lea rsp,[rbp-0x28] ; pop rbx/r12/r13/r14/r15/rbp ; ret  ; return 0
0x480000a1c7a1:  call  0x480000a26080 ; ud2       ; __stack_chk_fail
0x480000a1c7a8:  call  0x480000a26230 ; ud2       ; abort
```

## What it means

The engine's wrapper around the mapper is a **hard must-succeed gate**:

1. It writes the 56-byte size prefix (`0x38`) into the stack struct and calls `sceKernelMapperGetParam`
   with a pointer to it (the `arg0 = 0x600000800e20` the trace shows).
2. `test eax,eax; jne abort` - **any non-zero return aborts immediately.** There is no error branch, no
   default, no fallback path. `0x80020006` (the measured retail return, 7b3c) takes this jump.
3. On success (`rc == 0`) it reads **four qwords the mapper must have filled** - struct `+0x08`, `+0x10`,
   `+0x18`, `+0x20` - and stores them into four caller out-pointers (`rbx`, `r12`, `r15`, `r14`). So the
   struct's payload after the size prefix is four load-bearing qwords the engine hands onward.

This confirms D677 and closes the "is it faithful or divergent" question at the guest level: orbistoun
returning the measured `0x80020006` produces *exactly the abort the guest's own code dictates* for a
non-zero return. The behaviour is faithful; the wall is the engine's requirement, not an orbistoun bug.

And it hardens the refusal to fake `0x0`. Faking success would clear the `jne`, but then the engine
distributes **four unmeasured qwords** (whatever happens to sit at `rsp+0x08..+0x20`) into `rbx/r12/r15/
r14` and uses them - pointers or handles by their later use. That is D670's pattern precisely: a refusal
the guest cannot see is a success, and the lie surfaces as a fault further in. So `0x0` is not one wrong
value, it is five: the code and the four qwords behind it, none measured.

## What this makes the open question

The wall is now specified exactly, which is what a2f9 needed. To pass it honestly orbistoun needs, from a
retail measurement of a *successful* `sceKernelMapperGetParam`: (a) confirmation it can return `0` at all
on retail, and (b) the four qwords it writes at struct `+0x08/+0x10/+0x18/+0x20` (the size prefix at
`+0x00` is `0x38`, already known). A `not-possible` - "on retail it only ever returns `0x80020006`" - is
equally an answer: it means this engine path is unreachable on stock retail firmware, consistent with the
non-stock boot (faked libraries, a separate encrypted `game.bin`) worklog 515 noted, and the frontier
floor of 22/391 is then faithful and final for this dump. Either way the next datum is a retail struct
dump, not another guess. a2f9 updated with the offsets.

## State

- No code change - this is measurement. Worklog 515's commit (647952e) stands as the faithful behaviour.
- `ORBISTOUN_WATCH` is the right tool for "what are the bytes at this guest address" when the backing
  file is encrypted or synthesised; recorded here because it is not obvious from the env list.
