# 2026-09-02 - (/loop) Bulk port batch 10: `getsockopt`, and what "missing" actually means

```
documented   715 needed, 267 missing   ->   715 needed, 266 missing
```

One function. The useful part of this tick is what nearly happened instead.

## I almost rewrote something that already existed

The plan said "`getsockopt`/`setsockopt`", carried as a pair through three ticks of loop prompts.
I wrote both - a per-option table, refusals by name, the lot - and the build refused it:
`setsockopt` **is defined multiple times**.

It already existed, and with better reasoning than mine. Its doc comment explains that a server's
first act is `setsockopt(SO_REUSEADDR)` and **failing it stops the server**, so refusing outright
would end every payload before `bind`; that `SO_REUSEADDR` is what the host's listener does by
default, so honouring it changes nothing; and that the call is therefore "accepted, recorded as
not applied, and the knowledge file says which". My version would have replaced a considered
position with a fresh one that had not met the same evidence.

**Where the pairing came from is worth naming**: I put it in my own loop prompt, and each
subsequent tick copied it forward without re-checking. A carried-forward plan is not a
measurement, and the gap report never said `setsockopt` was missing - only `getsockopt`.

## And "missing" is narrower than "unwritten"

`setsockopt` is implemented, declared and delegated, so it counts as present. But the same report
counts a function as **missing** when it resolves to nothing, whatever exists in the source - so
code that is written but unreachable reads as absent, which is right for the guest and wrong as a
guide to what to type. Worth remembering before writing anything the report calls missing: check
whether it exists and is merely unwired.

## What actually went in

`getsockopt`, in `orbistoun-fs/src/socket.rs`, with a per-option table:

- `SO_ERROR` - and **reading it clears it**, which is the half a naive version drops. A caller
  polls it precisely to consume the pending error; one that keeps answering the same error shows
  a socket that never recovers.
- `IP_TTL`.
- Everything else is **refused by name**, once per distinct option, so a trace says which option
  a guest wanted rather than printing two integers. Names come from the harvested constants
  (D350/D351), which also bounds what can be recognised: `TCP_NODELAY` is in `netinet/tcp.h`,
  outside what the harvest reads, so it is refused as unknown rather than guessed at.

Declared **partial** (D474), listing exactly that - the count is now 4.

`getsockopt` takes the opposite tack to `setsockopt` deliberately, and the comment says why: a
server *setting* an option exits if the call fails, whereas a server *reading* one gets a value
it will act on. A wrong answer is worse than a refusal in one direction and better in the other.

## State

clippy `--tests` clean, fmt clean, fs/posix/hle tests pass, nothing committed.

**Next**: 266 left. Still queued and still worth the user's call rather than mine: the **TitleOwn
loader**, and the **obSCEne differential** - which remains the only check for "implemented but
wrong", and roughly 90 functions have now been written without it.
