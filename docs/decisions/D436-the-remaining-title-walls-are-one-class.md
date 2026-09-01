# D436 - The remaining title walls are one class: a guest allocation returning null, and it needs hardware


**measured** - 2026-09-01 (user-directed, /loop)

Disassembled the walls the local (non-online) titles hit, and they are the same shape - a guest memory
allocation returning null during memory setup, after the identical
`sceKernelVirtualQuery(0x720000240000)` → `sceKernelAllocateMainDirectMemory(0x4000)` →
`sceKernelMapDirectMemory` sequence.

- **PPSA25872, `image+0x7b5890`**: a container allocator. It loads a global pool pointer
  (`mov r9,[rip+…]; mov rax,[r9]`), computes `SIZE_MAX - size`, and `cmp rdx,rsi; jae …` - a max-size /
  overflow check. When the requested size (`rsi`, computed by the caller upstream) exceeds the max it
  returns `0`, and the caller (`image+0x1668a51`) writes through the null with `mov [rax],rax`.
- **PPSA02664 and PPSA03416 share `image+0xafcc08`** (a *shared* wall, so higher-leverage): a C++
  virtual call `call [rax+0x10]` returns `0`; the guest takes the zero branch (`xor r12d,r12d`), zeroes
  an XMM register and stores it - `vmovdqu [r12],xmm0` with `r12=0`, the null write. An allocator's
  virtual `allocate` answering null.

Both are the guest's *own* allocator deciding it has no memory, or too little, from what the
direct-memory and `VirtualQuery` calls told it. The fix is not another missing function - every call in
that sequence is implemented - it is getting the **memory-subsystem semantics exactly right**: what
`VirtualQuery` writes into the rest of its info struct beyond start/end, what map shape and sizes the
guest's allocator reads back, what `AllocateMainDirectMemory` reports. Those are precisely what a
hardware conformance probe measures, and guessing them is the pointer-versus-error class this project
refuses. So these walls need **more data from obscene**, which is the boundary the loop was told to run
to.

The `names` search (1604s, 3.8B candidates) also established that PPSA28061's online blocker
(`libSceNpCppWebApi::0xa9721c01ca796f63`) cannot be named from local inputs - it needs an external
symbol source, and behind it a network the offline emulator does not have. PPSA04263 spends its whole
run walking the memory map (D083), which is the same map-shape question in another guise. So every
remaining wall across the corpus now needs either obscene hardware data (the memory subsystem) or an
external source (the online symbols) - the loop's stop condition, reached honestly rather than by
running out of steam.

