# D572 - The unnamed futex had a name in the tree, and the search missed it for a word order

**Status:** measured
**Date:** 2026-09-07

## What D566 asked for

`libkernel_sync_on_address::0xbd04891e6902ce1d` - 78% of every call this project had recorded,
in one title, with no name. D566 hand-tried seven candidates (`sceKernelWaitOnAddress`,
`sceKernelWakeOnAddress`, `sceKernelWaitOnAddressWithTimeout` and four variants), ran the
three-and-a-half-billion-candidate sweep, searched obSCEne for the hash, and asked for a hardware
census of the library.

## Where it was

**In this tree.** `crates/orbistoun-firmware/data/libkernel-vaddrs.txt` - the 12.40 export
layout, read off the real module and resolved to names from obSCEne's mined corpus - carries
`sceKernelSyncOnAddressWait32`, `sceKernelSyncOnAddressWait64` and two aliases under exactly the
library's own spelling. The family is *Sync On Address*, not *Wait On Address*: every hand-tried
candidate had the words in the wrong order, and the grammar had `Sync`, `Address` and `Wait` as
separate words with no pattern that puts them in that order.

Hashed, proved:

| Hash | Name |
|---|---|
| `0xbd04891e6902ce1d` | **`sceKernelSyncOnAddressWait`** |
| `0x90591532c0bf6cab` | **`sceKernelSyncOnAddressWake`** |

Both are imported by PPSA25872's own `libc.prx` and called in every run of it.

## Why obSCEne "did not know"

It did. `data/mined-names.txt` lists `sceKernelSyncOnAddressWait libKernel 0x1dce02691e8904bd` -
**the same eight bytes in the other order.** obSCEne stores the NID as the export table holds it
and this tree prints it as a number, so a search for either spelling in the other repository
finds nothing. D566's "appears nowhere in obSCEne" was true of the string and false of the fact.

The same corpus resolves the two aliases: `_sync_on_address_v1_alias1` shares the wait's NID and
`_sync_on_address_v1_alias2` the wake's, and in the export layout the wait's entry point is the
one `Wait64` uses while `Wait32` has its own - which is what fixes the compare at sixty-four bits
(D573).

obSCEne's own surface note describes the pair as the FreeBSD `_umtx_op` wait and wake, and its
hardware census **could not load the library** - so nothing on hardware has confirmed any of this,
and the knowledge entries say `guest-observed`, not `measured`.

## What was decided

- The two names are declared in a `libkernel_sync_on_address` module of `orbistoun-kernel`, under
  the library name the guest imports them by, so a trace labels them as the guest does.
- `SyncOnAddress` joins the object vocabulary and `Wake` the verbs, with the source written beside
  the word, so the generator can spell the family and the symbol database carries a `generated`
  derivation rather than a hand-placed name.
- **A hash search across the two repositories tries both byte orders.** Not built as a tool -
  it is one `rev` away - but written here because the miss cost a wrong sentence in a decision.

## What this does not establish

**That the family is the platform's only futex.** obSCEne's surface file also lists a
`libkernel_sync_on_address2` with 8-, 16-, 32- and 64-bit sized waits, and the export layout has
two unresolved entries beside the four named ones. PPSA25872 imports the plain pair and nothing
else from either library; the sized variants are declared nowhere here yet.

**Nor why the sweep did not find it once the words were there.** The vocabulary had `Sync` and
`Address` as objects and `Wait` as a verb, and the patterns compose one object with one verb.
`SyncOnAddress` is a compound no pattern generates from parts, which is the ordinary case for
this grammar and the ordinary fix - a word, not a rule.
