# 421. The futex had a name, and the wait now waits

**2026-09-07** - directed

## What was done

**Named the function that was 78% of every call this project had recorded** (D566), by hash:
`0xbd04891e6902ce1d` is `sceKernelSyncOnAddressWait`, and `0x90591532c0bf6cab` beside it is
`sceKernelSyncOnAddressWake`. The family is *Sync On Address*; D566's seven hand-tried candidates
all had the words the other way round. The names were in this tree the whole time, in the 12.40
export layout `orbistoun-firmware` carries, and in obSCEne's mined corpus under the byte-reversed
NID - which is why a search for the hash as this tree prints it found nothing there (D572).

**Modelled both** in `orbistoun-kernel`, on FreeBSD's `_umtx_op(2)`: the wait compares a 64-bit
word under the queue's lock and sleeps only while it matches, the wake ends up to *n* sleeps and is
not remembered when nobody is asleep. Eight tests pin the contract; the negative one was watched
failing with its guard removed (D573).

| PPSA25872, 12 s | placeholder | modelled |
|---|--:|--:|
| calls | 2,580,950 | **307,226** |
| on stubs | 88% | **0%** |
| the wait | 2,273,750 | **26** |
| the wake | 13 | 13 |
| verdict | | `same` |

The spin is gone and the wall is where it was, because the wall was never the wait: the main
thread dies in a path walk under `/app0/Media`, reading above the top of its stack, and that
fault now sits at the top of the report instead of behind two million calls of noise.

**What the arguments said**, read from the JSON with all six recorded (D570): thirteen distinct
addresses at a stride of `0x150`, every wait `(address, 0, 0, 0, 0, leftover)`, one call site,
thirteen threads created, thirteen wakes with `(address, 1, 1, 1, 0x7ffffffe, 0)` on words `0x30`
below the waited ones. Twenty-six waits and thirteen wakes afterwards is consistent with each
thread waiting once on the word the main thread wakes and once, forever, on a word nothing sets -
an inference, because a trace does not say which thread called.

**Three tools reported more than they measured** and are fixed (D574): the launcher ran nothing
for a title it could not find and printed a report over zero traces; the tail line's `x48` hid
that the forty-eight calls were on thirteen addresses; and `decide` counted `## D` headings in an
index that has none. The launcher now resolves titles from wherever the built tool keeps its
library, and every corpus-walking verb goes through one root rule.

Knowledge recorded with `learn` for both, `guest-observed`, under a new
`libkernel_sync_on_address` file. `SyncOnAddress` and `Wake` joined the vocabulary with their
source beside them.

## The premise ranking, regenerated

The trace corpus on this machine was **empty at session start** - the traces directory had been
created that minute, and `worklist` and `questions --premises` ranked 765 questions by zero calls.
Seven runs later the ranking means something again. What a console day buys now, by calls
resting on it:

| calls | functions | premise |
|--:|--:|---|
| 159,982 | 14 | modelled on the POSIX call of the same shape, never verified against the target |
| 147,178 | 28 | which errno values the target returns, and when, is unverified |
| 73,529 | 1 | `sceKernelDebugOutText`: that both channels are the operator's log |
| 35,468 | 1 | `sceKernelDlsym`: what handle `1` selects, and that the third argument is the out-pointer |
| 22,561 | 1 | `scePthreadGetthreadid`: FreeBSD's id namespace and width, inferred from the name |

None of the next functions needing semantics are sync primitives any more. PPSA25872's remaining
unimplemented calls are nine, called once each - the app-content, common-dialog, user-service and
mouse initialisers, and one unnamed function in the title's own `PS5Util` module.

## Surprises

**The records are ahead of the machine.** With the code unchanged since 2026-09-04, three titles
reach less than their records: PPSA02664 119 imports against 197, PPSA03416 119 against 187,
PPSA25872 121 against 129. All three crash in their own modules **reading a few kilobytes above
the top of the main stack** - `0x6000008023e0`, `0x600000802068` - which is one signature, not
three. The guest's first argument carrying a host path was found and ruled out by a relative-path
run (D573). The cache directories - traces, filesystem, logs - were empty when the session began,
and that was not tested. Open.

**The identity guard caught a fourth tool.** The name sweep writes the module path it was given
into `symbols/generated.json`, and a sweep pointed at the tool's library in the platform data
directory wrote an absolute host path into a committed file. `scan.sh --cached` refused it; the
sweep was re-run through the launcher, which resolves `titles/` first, and the record is
`titles/obscene/eboot.bin` again. Backlog 036: the record should be relative to the corpus root.

**The corpus directory the launcher walks was empty**, and the tool's library is elsewhere; the
two had drifted without either saying so. A sibling repository holds an untracked directory named
after a host home path, created the evening before - a mangled path from some tool, harmless
while untracked, and worth deleting before an `add -A` finds it.

**`./bin/orbistoun check` was red before today, four ways.** The numbers block in `README.md` and
`docs/PROJECT_STATUS.md` had drifted to 374 declared / 332 implemented against the tool's 944 / 675,
and its title table was from before 2026-09-04 - `status --write` brought both current. The
`decisions` gate counts `## D` headings in an index that has none since the split, so it reports
every historical duplicate as resolved and fails; the `prose` gate finds four line-continued
literals in files this session did not touch; and `cargo doc` refuses public docs in `orbistoun-video`
and `orbistoun-libc` that link private items - two more of the same in `orbistoun-kernel` are fixed
here because the file was open. All three fail on the committed tree.

**`imports` and the trace disagree about a library's name.** The survey prints `libkernel` for
what the trace labels `libkernel_sync_on_address`; the trace reads the import-library table and
the survey does not. Not chased.

## Next

- The wall: why three titles read above the stack top, and whether an empty cache is the cause.
- The guest's first argument (D574): what a console puts there, then make the worker put that.
- The sized variants obSCEne lists under `libkernel_sync_on_address2`, when a title imports one.
