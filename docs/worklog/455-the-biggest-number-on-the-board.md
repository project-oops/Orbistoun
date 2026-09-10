# 455. The biggest number on the board

**2026-09-08** - directed, continuing 454

Bus idle a fourth pass. So: `worklist`, which is the tool that exists for "what next" and which
this session had not run.

## One import is 87.6% of every call this project has ever recorded

```text
      CALLS  SHARE  MODULES  IMPORT
   19689012  87.6%        1  PS5Util::0xf948d02a4f9f5ace
     595200   2.6%        6  libkernel::sceKernelDlsym
```

Nineteen and a half million calls, one guest, no name - PPSA25872, at **2% standing**, the worst in
the corpus. The report calls the shape correctly: *a guest that keeps asking the same question has
not accepted the answer.*

`PS5Util` is not a platform library. It is `/app0/Media/Modules/PS5Util.prx`, a file the **game
ships**, and orbistoun places, relocates and starts it. So there is nothing to implement: the code
the guest wants is code the guest brought, mapped in this process.

Cross-checking `exports` against `imports`:

| module | eboot imports | module exports | matched |
|---|---|---|---|
| `PS5Util.prx` | 2 | 7 | **2** |
| `Il2cppUserAssemblies.prx` | 4 | 279 | **4** |

**Six for six.** Every import the executable makes from a module the title ships is exported by
that module - `0xf948d02a4f9f5ace` is `PS5Util + 0x2a0` - and all six are answered with the
`Unimplemented` placeholder instead (D630).

## Left as a diagnosis, and why

Resolving an import to a placed module's export is loader work, and this session's brief confines
the work to user-space library and ABI-stub implementations. The diagnosis is complete enough to
act on without more investigation - module present, placed, started; NID in its export table at a
known offset; no measurement, name or capability missing.

And there is a real question under it that stops "just link them" from being the obvious answer:
the same title ships `libc.prx`, and **186** of the executable's imports come from it. For those,
orbistoun's own implementation is almost certainly better than the game's copy, and D005's whole
architecture is that an import is intercepted. So the rule has to separate a library this project
implements from a module only the game could have written. `PS5Util` and `Il2cppUserAssemblies` are
plainly the second kind; `libc.prx` is plainly the first.

## Surprises

- **The corpus's biggest number was never looked at.** 87.6% of all recorded calls, sitting at the
  top of a ranked list that names itself "what to implement next", and it turned out to need no
  implementation at all.
- **The spin's arguments say it is a list operation.** `arg5` points at a structure whose `+0x10`
  field holds its own address - an empty intrusive list, `next = self` - and `arg0` is a host-side
  address outside every region this run gave the guest. Recorded rather than read into.

## Next

- The six title-module imports, when loader work is in scope. This is the largest single wall
  measured anywhere in the corpus.
- The unapplied relocation behind `obs_sink_open` (D628) - the same class of thing, one layer down.
- The declined-syscall casualties, waiting on `REQ-20260908T1620Z-4c1e`.
- `sceAgcCreateShader`, waiting on `REQ-20260908T1621Z-7a5d`.
