# 2026-08-29 - ftpsrv talks, and asks to be root


The measured list said two functions. `sscanf` and `strftime` are written, and `ftpsrv` now
imports nothing this project does not implement.

It does not serve. It runs, prints its own diagnostics in its own words, and stops:

```text
main-prospero.c:49:malloc: error 0 (orbistoun has no message table)
Unable to change AuthID
```

**It is trying to become root**, through the `KERNEL_ADDRESS_*` globals a loader supplies on
real hardware. There is no kernel here to have them. That is a stated wall rather than a
mystery, which is the difference that matters: `klogsrv` listens because a log server needs a
socket; `ftpsrv` needs a filesystem it is allowed to read, and asks for it the way a
jailbroken console lets it (D382).

Also answered `sysctl kern.proc.proc` with the truth - there are no processes - which is what
both payloads are really asking when they look for an earlier copy of themselves, and which
sidesteps `struct kinfo_proc` and its layout question entirely.

### The check that was too clever

Yesterday's `%s` guard followed a pointer only inside a published range. Those ranges are the
*guest's*, and a `%s` argument is very often a pointer into memory this project handed the
guest - a `strerror` buffer, a `getifaddrs` block. So `ftpsrv` printed a perfectly good error
message with `(unmapped)` where the reason should have been.

Back to the narrow rule. The wrong version was visible in a guest's own output for about an
hour, which is the cheapest way to find out, and only possible because a payload had got far
enough to print.

