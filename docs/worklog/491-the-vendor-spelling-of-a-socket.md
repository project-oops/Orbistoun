# 491. The vendor spelling of a socket, and the flag that hung the guest

**2026-09-10** - loop, continuing 490

Every `102-net` conformance check now passes, and each one matches the value hardware
answered rather than merely reporting success. 191 checks passing became 196 across the
whole sweep, with nothing regressing.

## Six functions were implemented and unreachable

obSCEne under orbistoun answered `0x7fff0001` to `sceNetSocket`, `sceNetBind`,
`sceNetListen`, `sceNetSetsockopt`, `sceNetRecv` and `sceNetSend`. All six have had working
bodies in `orbistoun-fs` for months, under their POSIX names. A **payload** reaching the
network worked; a **title** hit stubs.

The vendor spellings now live in `orbistoun-net`, which is the crate that declares
`libSceNet` - D525's rule, applied. It takes a dependency on `orbistoun-fs` for the bodies,
which stay where the descriptor table is.

The two spellings disagree about exactly one thing, so that one thing travels: each body
answers `Answer = Result<u64, u32>` - a value, or the POSIX errno that stopped it - and each
crate encodes it its own way. POSIX gets `-1`, `libSceNet` gets `0x8041_0100 | errno` (D667).

## Three measurements that guessing would have got wrong

**`sceNetSocket` takes a name before the address family.** Every other call in the library
sits at the POSIX argument positions; this one is shifted by a register. Sharing the body
unshifted reads a `const char *` where `AF_INET` belongs and refuses every socket. Asserted
from both sides now, because only the pair says which way round it is.

**`setsockopt(SOL_SOCKET, 0x1200, &1, 4)` is how this platform turns non-blocking on**, and
it is applied rather than accepted. `0x1200` is in no harvested header; hardware answered
`0x0` to it, `-1` to `fcntl(F_SETFL, O_NONBLOCK)`, and refused the `0x1100` obSCEne offers as
a fallback. The shim had been accepting it and doing nothing - the plausible answer.

**`ENOTCONN` is now a measured entry** in `orbistoun-core`'s errno table. `sceNetRecv` on a
listener answers `0x8041_0139` and on an empty connected socket `0x8041_0123`; orbistoun
answers the same two codes for the same two conditions.

## Surprises

**A wrong answer costs one check; a block costs the rest of the run.** The first run with the
vendor names bound got *worse* - 222 imports and 18,383 calls against 245 and 450,179, the
guest silent for nine seconds. `102-net/recv-would-block` reads a plain blocking socket with
`MSG_DONTWAIT`; the shim ignored flags, so the read waited and the time limit ended the run
with eleven sections unrun. Ignoring a flag had looked like a fidelity question and was
actually a liveness one.

**Restoring the mode turned up the same defect already shipped.** `MSG_DONTWAIT` means making
the host socket non-blocking for one call and putting it back, which needs the mode
remembered because the host has a setter and no getter. Adding that memory made visible that
the readiness peek behind `select` sets non-blocking, probes, and restores `false`
*unconditionally* - handing a guest's non-blocking socket back blocking. Found sideways.

**Twelve knowledge files were in no build.** `EMBEDDED` in `orbistoun-hle` is a hand-kept list
of `include_str!` calls and it had drifted by half - `libSceNet.toml` among them, carrying the
D627 error-base measurement that nothing loaded. `orbistoun-cli learn` was writing behaviour
the emulator could not see while the accounting reported it as recorded. Registered, plus a
test that reads the directory and names any file the list does not carry; written first, and
it named all twelve. Recorded behaviours went 714 to 749 as a result (D668).

**A table can be read and not returned.** The first attempt put the vendor names in
`orbistoun_fs::socket::implementations()` and nothing changed:
`orbistoun_posix::implementations()` consumes that table as a *lookup source* for its
`DELEGATED` pairs and returns only POSIX names. Entries added there are visible to the lookup
and invisible to the dispatcher.

## One divergence this opened

Hardware answers **NULL** to `dlsym` for these names; orbistoun now resolves them, because
implementing a name is what makes it resolvable here. `102-net/resolve` passes either way, so
it is not a failed check - but it is a real difference in what a guest can discover about
itself, and it is on the record rather than left to be rediscovered.

## The mesh

SELFish resolved the `split-decisions.sh` request and reported a residual bug with the fix:
the reader took any vocabulary word from lines 2-6, so an entry with no status line inherited
whatever word its opening paragraph used. Fixed as asked - status now comes from a line that
*declares* one, either labelled anywhere or an emphasised vocabulary word in the header.

Orbistoun's own index did not move: 678 entries, zero status changes. The other three trees
were **not** run against, and each has a request in its inbox to re-run and read its own diff.
obSCEne's moves the most - around seventy rows, most of them entries whose real
`Status: bug`/`Status: evidence` lines the old vocabulary filter had been discarding - and two
of its entries open `**partly superseded by D023**`, which the old reader called `superseded`
(over-claiming) and the new one calls `unrecorded` (under-claiming). That one is a word in the
entry, not a pattern, so it went back to them as a question.

## Next

- Nine of the thirteen context-matched conformance divergences remain. The four memory ones
  and `035-libc/wide-strings` are orbistoun *passing* where hardware fails, which is a
  different shape of problem and probably a different fix.
- `045-disc/open` is expected and `900-surface/control` is not looked at yet.
- The two shell surfaces - a package manager over `packages/`, and a payload lister - are
  still unbuilt.
- The corpus fetch path still uses `repo`/`tag`/`path` rather than walking the origin list
  D664 built.
