# 2026-08-27 - The BSD harvest was names-only, and the work outgrew it


Asked whether we were missing information from the FreeBSD harvest. We were, and
`docs/REFERENCES.md` said so all along: *"Symbol names, and nothing else… no constants."*
Right for the naming loop, outgrown by implementing (D352).

It had already cost three answers this session: `SIGPIPE` recovered from the guest's own
call argument, a `sysctl` MIB left unnamed, `errno` left unset. Sockets would have been far
worse - `AF_INET` and `SOCK_STREAM` are not optional to know.

Sparse checkout widened from `lib/` to include `sys/sys`, `sys/netinet`, `include`: **22 MB
to 25 MB**. 903 constants harvested into `abi-constants.toml`, read by `abi_constant` rather
than retyped.

### The satisfying part

Four constants confirmed measurements taken before the harvest existed, by two routes that
never touched: `SIGPIPE` 13 against the `0xd` klogsrv passed, and `[1, 14, 8, 0]` against
`CTL_KERN` / `KERN_PROC` / `KERN_PROC_PROC` - the last commented *"only return procs"*. So
the MIB is **enumerate all processes**, from a caller the symbol table calls `find_pid`.
D350's open question closed itself.

And the guest confirmed the whole path end to end: `main.c:278:sysctl: error 0` became
**`error 2`**.

### Kept

`SOL_SOCKET` is `0xffff` here and `1` on several other platforms, which is exactly the value
recall gets wrong. There is a test pinning it for that reason - not because the number is
interesting, but because it is the one a table built from memory would differ on first.


