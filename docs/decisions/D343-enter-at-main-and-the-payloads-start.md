# D343 - Enter at `main` and the payloads start working


**decided** · 2026-08-27

Every open-toolchain payload dies inside `__crt_start`, rejecting a handoff structure nothing
here can supply (D306, D308). That looked like the blocking research problem for the whole of
[PAYLOADS.md](../PAYLOADS.md).

It is not, for three of the five. `main` is a real, sized, `GLOBAL FUNC` symbol in klogsrv,
shsrv and ftpsrv, so it can be located by name without deriving anything - and if
`__crt_start` is merely unpacking that structure and calling `main`, entering at `main` skips
the problem outright.

`[entry] at = <image-relative address>` is a diagnostic that does exactly that. One boot:

| payload | entered at | got to | imports called |
|---|---|---|---|
| ftpsrv 0.21.1 | `image+0x5ba0` | `image+0x5c64` | **1** |
| klogsrv 0.9 | `image+0x0` | `image+0xd4` | **2** |
| shsrv 0.20 | `image+0x0` | jumped to `0x0` immediately | 0 |

**The first library calls any of these has ever made in orbistoun**, after a day in which the
answer was uniformly zero. And they are the right ones:

```
klogsrv    signal(0xd)  then  getopt(...)
ftpsrv     getopt(...)
```

`0xd` is `SIGPIPE`. A network server whose first two acts are installing a `SIGPIPE` handler
and parsing its options is a network server behaving exactly as one should - which is worth
more than the count, because it says the code reached is *the code that was meant to run*,
not a plausible-looking accident.

### What it costs, and what it does not claim

**It is a diagnostic, not a claim about how the platform starts a program.** Something did
run `__crt_start` on real hardware and it presumably matters; this only establishes that
whatever it does is not required for `main` to begin working. `Some(0)` is a real request
rather than an absent one - an image's first byte is a legitimate address and two of these
put `main` exactly there, which is why the field is `Option<u64>` and not a sentinel zero.

The executable-segment refusal that guards the declared entry (D010) now guards this too. A
diagnostic that jumps into data produces a fault about itself, which is the failure a
diagnostic must not have - and the third time this session that the instrument needed
guarding before its results could be trusted (D308, D325).

The run says so on stderr when it starts anywhere other than the declared entry, because
every later number in that report is about a program that did not start where its container
says it does.

### Where it stops, and why that is cheap

Both faults are a null read shortly after `getopt`, which is unimplemented and answers a
placeholder. `main` was also handed the entry-argument block in `rdi` rather than a real
`argc`, so it is parsing arguments it never received.

So the front of the queue is now measured rather than guessed: **`signal`, `getopt` and
`optarg`** - three functions, all POSIX, ahead of the thirteen-function universal set that
`PAYLOADS.md` Stage 1 names. `optarg` is a data object and already receives storage (D323).

shsrv is the odd one out: entering at `image+0x0` jumps straight to null, so either its
`main` is not there or address zero is not what its symbol table appeared to say. Untried,
and it does not block the other two.


