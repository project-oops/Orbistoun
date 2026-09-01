# 2026-09-01 - Fault reports carry the faulting instruction (D432)


(worker) The fault report now prints the bytes AT the instruction pointer and a 48-byte window
BEFORE it, read straight from the loaded image inside the handler - page-clamped both ways so the read
cannot fault, allocation-free, one write (D065: executable pages are readable here). `Line` capacity
1024 so the register/frame lines are never the part dropped. This is what a `dump`/`disasm` verb would
have been, but automatic: a SELF/eboot fault, whose `image+0x…` the ELF offsets cannot locate, is now a
disassembly on the page.

Used it on the shared PPSA02664/PPSA25872 wall and it disassembled cleanly: the guest `call`s
`image+0x7b5890`, that returns 0, and the guest dereferences it unchecked - `mov [rax],rax` with rax=0,
initialising a self-referential (empty circular-list) structure through a null pointer. The three
`lea [rip+…]` right before the fault compute exactly the dump's rsi/rdi/rdx, confirming the decode. So
the "just before" memory calls were a red herring; the wall is a guest function returning null.
Honestly ruled out `ORBISTOUN_MAP_SHAPE=reserved-low` (non-zero physical base) - it changed nothing.

The wall is now the precise question "why does image+0x7b5890 return 0" rather than an address, with the
tool to answer it in place. Not drilled further: naming that function or blind-stubbing `_init_env`
(no citable contract) would be the pointer-vs-error guess the finding warns against. Worker tests green
(40+9+1); kernel green (74+33+33); my code clippy-clean (orbistoun-fs escape/socket debt is pre-existing).

