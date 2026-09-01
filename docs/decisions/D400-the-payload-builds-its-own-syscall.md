# D400 - The payload builds its own syscall entry, and this hands it something with no inside


**measured** - 2026-08-30

Past the entry-argument wall (D399), `elfldr` faults reading `0x2001` inside an unrelated stub.
That looked like a bad handle being dereferenced. It is not: it is a **jump landing in the
middle of the thunk table**, and the mechanism is legible in the payload's own instructions.

### What is measured

There is a trampoline that shifts every argument down one register - `rsi` to `rdi`, `rdx` to
`rsi`, `rcx` to `rdx`, `r8` to `r10`, `r9` to `r8`, the stack slot to `r9` - and then calls
through one global. That is the shape of a **syscall shim**: argument zero is the number, and
`r10` rather than `rcx` is where the fourth argument belongs, which is the kernel convention and
not the C one.

That global is built like this:

```
4ab2:  mov  %rax,0x5cb20     ; a pointer the runtime has just obtained
4ae9:  mov  0x5cb20,%rax
4af5:  add  $0xa,%rax        ; ten bytes into it
4af9:  mov  %rax,0x5cb20     ; and that is the syscall entry from here on
```

So the payload does not ask for a syscall gadget. It takes a pointer it already has, **skips ten
bytes into it**, and calls that for every system call it will ever make. On the target that is a
function whose eleventh byte begins a `syscall` instruction - the ordinary trick of reaching the
instruction inside a wrapper rather than the wrapper's prologue.

### Why it fails here

Every address this project hands a guest is a **thunk**, and a thunk has no inside. Ten bytes
into one is either the middle of its own body or the start of the next one, so the payload's
first system call enters an arbitrary stub with a syscall number in `rdi`. The stub it happened
to land in reads its first argument as a pointer, and that argument was `0x2001` - a module
handle, which is why the fault looked like a handle problem.

This is the same class as D378, where the runtime wanted a syscall gadget in a named global and
got one. The difference is that this payload never names anything: it **derives** the entry from
a pointer, so there is no name to serve.

### Which pointer it is, settled by watching the write

The disassembly could not say: the instruction reads the handoff's field zero, and a comparison
against a freshly resolved address sits between the read and the store. Either answer fitted,
and they want different fixes - so it was left open rather than guessed at, and a watchpoint on
the global answered it in one run.

Three writes arrive, in order:

| written by | value |
|---|---|
| `image+0x4743` | `0x0` - the runtime clearing its own `.bss` |
| a **host** address, inside this project's own resolver | `0x7000000015a0` |
| `image+0x4b00` | `0x7000000015aa` |

The second is this project writing an answer into the guest's out-parameter, and the value is
the thunk for **`getpid`**. The third is the payload's own `add $0xa`. So the pointer is a
**resolved symbol**, not the resolver, and the offset is applied to whatever address this
project answered a name with.

That is the harder of the two answers. It is not one field to fill: it is a property of every
thunk a guest might resolve, because the guest picks the name.

### Why it lands in a *particular* wrong function

A stub is twenty-three bytes of instructions in a thirty-two byte slot:

```
[0..2]    mov r10, imm64      the index the dispatcher switches on
[2..10]   the index
[10..12]  mov r11, imm64      <- ten bytes in is exactly here
[12..20]  the dispatch trampoline
[20..23]  jmp r11
```

Ten bytes in is **already a valid instruction boundary**, and what sits there is
`mov r11, trampoline; jmp r11`. So the payload does not jump into the middle of an instruction
and crash on garbage - it jumps into the dispatcher correctly, having skipped only the part that
loads the index. The dispatcher then switches on whatever `r10` happened to hold.

That is worth knowing because it changes what the failure is. It is not corruption and it is not
random: it is a **well-formed call to an arbitrary function**, chosen by a stale register. The
stub it reached read its first argument as a pointer and that argument was `0x2001`, which is
how a module handle ended up looking like the cause twice over.

It also means a guest could get *anywhere* this way, quietly, if the stale index happened to
name something that returned plausibly instead of faulting.

### What that means for the fix

A thunk must have something usable ten bytes in. On the target, `getpid` is a wrapper whose
`syscall` instruction sits at that offset, and the payload knows it - which is why it resolves
an ordinary function rather than asking for a gadget.

This project cannot answer with a real `syscall`: guest code runs natively on the host, so the
instruction would trap to the **host's** kernel rather than to this one. What it can do is place
its own gadget - the one D378 already builds, which calls the dispatcher and returns - at that
offset in every thunk, so the resolve-and-offset convention lands somewhere real whichever name
a guest picks.

### Built, and what it changed

A stub is now sixty-four bytes: a short jump over a **landing zone**, the zone a sled of
one-byte `nop`s falling into `mov r11, gadget; jmp r11`, then the dispatch path. Entering at any
offset from two to sixteen slides into the gadget, so eight, ten and twelve all arrive - the
number is not this payload's ten, because the offset belongs to whichever C library build a
guest was written against and serving exactly one of them would work once and mislead after.

`jmp` and not `call` into the gadget: the guest reached the stub by calling, so its return
address is already on the stack and the gadget's own `ret` takes it home.

With it, `elfldr` resolves `getpid`, builds its entry from that thunk plus ten, and **makes three
real system calls through it**:

```
0    649  nothing here implements it
1     20  getpid
2     20  getpid
```

Two of them are served. That is the whole path working end to end - resolve, offset, land,
dispatch, return - where before it was a jump into an arbitrary stub.

The payload then stops at its own `ud2`, the trap it places after a call that must not return:
it reads a status through handoff field five, calls its terminal exit, and this project's stub
returns from it. So the next wall is **syscall 649**, which is a named thing to implement rather
than a mystery, and after it a notion of a call that does not come back.

The fallback path this decision started from is never taken: handle 1 succeeds, so `0x2001` is
only ever reached when it fails. It was a red herring three times over - first as a supposed
constant, then as a handle being dereferenced, and finally as a number that simply never comes
up on the path a working run takes.

