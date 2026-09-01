# D432 - The fault report carries the faulting instruction, and what the shared wall actually is


**measured** - 2026-09-01 (user-directed)

A fault in a SELF/eboot guest was the one wall that could not be turned into an instruction by hand:
`image+0x…` names an offset the ELF `p_offset` fields do not locate (only the loader's wrapper decode
does), so there was no way to read the faulting instruction short of decoding the wrapper. The fault
report already knew where the guest faulted; now it prints **the bytes at the instruction pointer, and
the window of bytes before it**, read straight from the loaded image inside the handler. Two facts made
this safe rather than a second fault: executable pages here are always `PAGE_EXECUTE_READ` (execute
implies read - D065), and each read is clamped to the single page holding the IP (the end of it for the
instruction, the start of it for the lead-up), so it can never reach an unmapped neighbour. No
allocation, fixed stack buffers, one write - the async-safety the handler already keeps. `Line`'s
capacity rose to 1024 so the two hex runs cannot push the register and frame lines out.

**What it found, immediately.** The shared wall PPSA02664 and PPSA25872 both hit disassembles cleanly:

```text
mov edx, 0x10 ; mov ecx, 0xc ; xor r8d, r8d ; mov dword [rsp], 0
call image+0x7b5890          ; returns rax
lea rsi/rdi/rdx, [rip+…]      ; the three exactly match the fault's rsi/rdi/rdx
mov [rax], rax               ; FAULT - rax is 0, the value the call returned
mov [rax+8], rax ; mov [rax+0x10], rax ; mov word [rax+0x18], 1
```

So it is **not** the memory-management calls that were "just before" it: the guest calls a function at
`image+0x7b5890`, that function returns `0`, and the guest dereferences the result without checking -
initialising a self-referential structure (`[P]=P; [P+8]=P` - an empty circular-list sentinel, the
shape a `std::mutex`/list/once head constructs to) through a null `this`. The register triple confirms
the decode: the three `lea [rip+…]` compute exactly `rsi`, `rdi`, `rdx` as the dump shows them. And it
ruled a hypothesis out honestly: `ORBISTOUN_MAP_SHAPE=reserved-low` (a non-zero physical base, the
shape that exists because "real hardware does not hand a guest physical zero", D083/D218) changed
nothing, so the null is not the direct-memory pool starting at zero.

The wall is now a precise question - *why does `image+0x7b5890` return zero* - rather than an address,
and the tool to answer it exists. Not chased further here: naming that function or tracing its own
dependency (the top unimplemented imports `_init_env` and `__cxa_decrement_exception_refcount` are
candidates) is the next drill, and blind-stubbing `_init_env` without its contract is exactly the
pointer-versus-error-code guess the finding warns against.

**And the furthest title's wall, from the same capability: the thread pointer is never installed.**
PPSA28061 (56 imports, the furthest measured) faults at `image+0x43c4`, which disassembles to
`mov rax, fs:[0]` - a read through the `fs` segment base, the thread-self pointer at the top of the
TLS block - and the read is *of `0x0`*, so the `fs` base is zero. The mechanism to set it exists and
is tested (`orbistoun_abi::thread_pointer::install`, the D061 layout work), but it "sat finished and
unused" because no title examined had ever declared a thread-local: **PPSA28061 is the title that
needs it**, and the run path installs no guest `fs` base, so its first `fs:`-relative access reads the
host's (zero on Windows, which keeps its TEB in `gs`). This is the honest-failure case the module was
written for - not silently running with a wrong pointer. Wiring it is the next subsystem: a per-thread
TLS block (with the `.tdata` image copied and the self-pointer written at `[TP]`), the `fs` base
installed and read back, on the main thread at entry and on every spawn (`thread.rs`'s spawn body
enters the guest with no install today). Recorded rather than half-built: a wrong `fs` base on the host
thread destabilises every run, so this wants a clean start with its own tests, not the tail of a long
session. The `_sceUltUlthreadCreate` calls in the same run (libSceUlt user-level threads, unimplemented)
are a *separate* threading subsystem behind it, not the cause of this fault.

