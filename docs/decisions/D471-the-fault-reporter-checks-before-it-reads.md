# D471 - The fault reporter checks the page before reading it, because an execute fault puts `rip` in the hole

**measured** - 2026-09-02 (user-directed /loop; the crash [D470](D470-a-run-that-wrote-no-trace-reports-nothing.md) could see but not explain)

## The assumption that was false

`instruction_bytes` and `bytes_before` copy the machine code around the faulting instruction into the
report - this project reads raw bytes rather than disassembling, because the bytes are a fact and a
disassembly of vendor code is not (principle 1). Both carried this `SAFETY` note:

> `ip` is the instruction pointer of the guest that just faulted, so the page holding it is mapped and
> readable

**True of a data fault, false of an execute one.** When a guest *jumps* to an address that is not
code, `rip` **is** the unmapped address. Reading the instruction at it faults again - and a fault
inside a fault handler is not reported, it ends the process. So the first fault was never recorded,
no trace was written, and the run reported the previous run's file (D470).

The probe caught it exactly:

```
fault-probe: code=0xc0000005 at=0x7fff0001 rip=0x7fff0001 - before emit   <- guest jumped to a placeholder
fault-probe: code=0xc0000005 at=0x7fff0000 rip=0x7ffa932bcc54             <- the reporter, faulting
```

`0x7fff0000` is one byte below `0x7fff0001`: it is `bytes_before` reading backwards from the
placeholder the guest had jumped to. The second `rip` is in a system DLL - the `memcpy` doing the
read. The handler never returned, so nothing was recorded.

## The change

A `readable(address, len)` helper using `VirtualQuery`, which describes a mapping without
dereferencing it and allocates nothing - both properties this path needs. It requires `MEM_COMMIT`,
rejects `PAGE_NOACCESS` and `PAGE_GUARD` (a guard page is committed and still faults, which is the
point of it), and requires the whole window to sit inside one region, because the next one may be
unmapped. Both byte windows now ask before reading and answer zero bytes when the memory is not
there.

The reports lose nothing they used to have: a fault whose `rip` is real still gets its bytes. What
changes is that a fault whose `rip` is *not* real now gets reported at all.

## Why it had never fired

Every fault this project had seen was a **data** fault - the guest executing real code that touched a
bad address - and for those the assumption holds. The execute fault only appeared once `_Getpctype`
was implemented (D468) and the guest ran far enough to reach a different failure: it asks an
unimplemented function for an address, is handed a placeholder, and **calls** it. A new *kind* of
fault found a hole in the reporter that a hundred runs of the old kind never touched.

That is the general shape worth keeping: the invariant was written down honestly in a `SAFETY` note,
and it was still wrong, because it was stated about "the faulting instruction" when it was only ever
checked against one class of fault. A stated invariant is a claim about every case, not the observed
ones.

## What it made visible

The first honest measurement of D468's effect on PPSA02664, against the last recorded pre-ctype
state:

| | imports | calls | fault |
|---|---|---|---|
| before ctype (01:50) | 39 | 1,544 | `image+0xb14be3`, dereferencing `_Getpctype`'s placeholder |
| after ctype | **68** | **10,905** | `0x7fff0001`, an **instruction fetch** |

Seven times the calls and twenty-nine more distinct imports. The ctype table was a real advance and
was reported as `same - nothing moved` for a whole morning.

The new wall is legible: `Il2CppUserAssemblies::0x6f8b9da539afc9af`, called **222 times**, whose
`arg0` points at `"il2cpp_init"` (and `"il2cpp_monitor_pulse"` after it in the same string table).
It is a name resolver; it answers the placeholder, and the guest calls what it was given. Naming and
implementing it is the next step, and the capture's 5,400 kernel exports may name it.
