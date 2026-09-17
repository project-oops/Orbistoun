# Memory

Host x86-64 register dumps and guest virtual memory layout.

Because Orbistoun runs guest code natively without binary translation, guest virtual addresses map directly to reserved ranges in the host address space.

---

## GUI: Memory & Register State

Open the **Memory** tab from the main emulation window (or press `Ctrl+M`).

```text
+-------------------------------------------------------------------------------+
|  Memory & Register State                                         [_][O][X]    |
+-------------------------------------------------------------------------------+
| RAX: 0000000000000000  RBX: 0000000800402000  RCX: 0000000000000038           |
| RDX: 00007fffffffe120  RSI: 00007fffffffe100  RDI: 0000000800400000           |
| RSP: 00007fffffffe0c0  RBP: 00007fffffffe0f0  R8 : 0000000000000000           |
| RIP: 0000000000401140 (gl-cube.elf: main + 0x140)                             |
|-------------------------------------------------------------------------------|
| Virtual Memory Range:                                                         |
|   0x0000000000400000 - 0x0000000000600000 : Main Executable (RX)              |
|   0x0000000800000000 - 0x0000000880000000 : Direct Memory / AGC Ring Buffer    |
|   0x00007fffff800000 - 0x00007ffffffff000 : Main Thread Stack (RW)            |
+-------------------------------------------------------------------------------+
```

*(Screenshot placeholder: Memory & Register State)*

### GUI Controls:
- **General Purpose Registers**: Real-time snapshot of the active thread's x86-64 context (`RAX`, `RBX`, `RCX`, etc.).
- **Memory Map**: Lists active direct memory allocations (`sceKernelAllocateDirectMemory`), mapped executable segments, and stack allocations.
- **RIP Tracker**: Shows current instruction pointer symbol resolution.

---

## CLI: Memory Inspection

To print memory maps at execution exit:

```bash
OOPS_LOG=orbistoun_mem=debug orbistoun-cli run build/title/GLCB00001
```

