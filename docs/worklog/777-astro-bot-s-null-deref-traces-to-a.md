# 777. ASTRO BOT's null deref traces to a specific eboot global (image+0xe553bd0) that should hold a constructed object but is null, loaded and passed through a thunk to the checking module fn

**2026-09-21** — worklog 776 picked ASTRO BOT (PPSA21564) as the grind because it is native and dies on a
single null. This tick disassembled the fault and traced the null one clean step upstream, to a named
global. Native code and working string-refs made it a real step rather than the tooling wall the Unity
titles hit - the null now has an address to watch.

## The faulting instruction

`ORBISTOUN_PEEK` at `0x4800007af792` and capstone decode:

```text
0x7af780  xor  eax, eax
0x7af782  cmp  byte [rip+0x18915f], 0        ; a debug/guard flag
0x7af789  mov  ecx, 0x58585858               ; 'XXXX' poison sentinel
0x7af78e  cmove rcx, rax                      ; flag==0 -> rcx=0, else rcx=sentinel
0x7af792  cmp  qword [rdi+0x38], rcx   <-- fault: rdi=0x0, reads [0x38]
```

So the faulting routine is a small inlined **validity check** - "is this object's `+0x38` field the poison
sentinel?" - and it is handed a **null object** (`rdi=0`). The routine is right to assume non-null; the
caller passed the null.

## The null is a named global, reached through a thunk

The stack return address (`rdi` unpushed at the fault, so `[rsp]`) is `image+0x27dfbf`, in the **eboot**.
Disassembling there:

```text
0x27df96  mov rbx, rdi
0x27df99  mov rdi, [rip+0xe2d5c30]     ; rdi = *(global image+0xe553bd0)
...
0x27dfba  call 0x74973b0               ; -> a thunk that tail-jumps into the module check
```

The eboot loads `rdi` from the global at **`image+0xe553bd0`**, then (on the `edx > 0x10` path) calls a
thunk at `image+0x74973b0`, which tail-jumps into the module routine above - so `[rsp]` is the eboot
return address and the module frame pushed nothing before faulting. The value in that global is what
`rdi` carries, and at the fault it is **null**.

That global sits right beside `rbx` at the fault (`image+0xe553e80`), so `image+0xe553xxx` is a **table of
object pointers**, and the entry at `+0xe553bd0` is the one left null. An object that should live there
was never constructed (or its pointer never stored) - the same shape as PPSA04263's null-vtable, but here
in native code with the slot's address in hand.

## Next

A **write-watchpoint on `image+0xe553bd0`** decides it: if some init writes a constructed object's address
there, that write is late or gated and the question is why; if it is **never written**, the construction
that should populate the table entry does not run, and finding that constructor (native, so string-refs
and a `lea`-to-the-global search both work) is the fix path. Either way the null now has one address to
watch rather than a whole call graph to climb - the payoff of picking the native title.

## Gate state

No code changed - a disassembly that decodes ASTRO BOT's fault as a null-object validity check and traces
the null to the eboot global `image+0xe553bd0` (a table-of-pointers slot), loaded and passed through a
thunk into the module. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No
commit.
