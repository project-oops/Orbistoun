# D642 - Two tables that answer each other

**Status:** measured
**Date:** 2026-09-09

## A hash 3.9 billion candidates could not reach

PPSA28061 calls `libkernel::0x04df812afad225d7`, is handed orbistoun's placeholder, and calls
`abort` seventy-seven bytes later - a check-and-give-up whose only blocker was the name (D636). The
generated search had already failed on it, and failed again when re-run against this corpus:

```text
generated names: 3911843959 candidates across 11 patterns, 32 threads
generated names: 3911843959 tried in 267.4s (14631099/s), 0 named
```

Both words it needs - `Mapper` and `Param` - are in the vocabulary, and the grammar reaches longer
siblings like `sceKernelMapperGetUsageStatsData`. The shape it needs is simply not one of the
eleven swept, and no amount of running fixes that.

## Neither table answers alone

`REQ-20260909T1050Z-3b7c` asked obSCEne to **enumerate** its kernel export table rather than look
names up in it, and the enumeration arrived in the same sweep: 2,443 entries, hash to address.
Orbistoun already parsed the section and reported `2231 named by this project, 212 not`.

That summary cannot answer the only question worth asking - *is the hash a guest actually calls
one of the 212?* - so the probe now crosses the two sets:

```text
of 7 hash(es) a guest here calls and nothing can name, this table holds 1
  libkernel::0x04df812afad225d7  at 0x800031d60, with no named hash at that address to derive from
```

**Address `0x800031d60`** is libkernel + `0x31d60`. This project's own firmware layout has held a
name at that offset all along:

```text
sceKernelMapperGetParam 0x31d60
```

And the hash agrees:

```text
0x04df812afad225d7  sceKernelMapperGetParam
```

Which is the only proof this project accepts. **A name is confirmed by the hash agreeing, never by
consulting a table** - and here two tables were consulted to produce a *candidate*, which the hash
then confirmed.

## The mechanism is new, so it gets a variant

`StaticSource` is closed on purpose: *"A new mechanism adds a variant here; it does not add a new
sentence."* This is a new mechanism - a console's export table gives hash-to-address, a firmware
layout gives address-to-name, and neither answers "what is this hash called" alone. `FirmwareLayout`
records it, and the doc says which two sources met.

It reaches what generation cannot, and it is bounded in a way generation is not: it can only name
hashes the platform exports and the layout covers.

## What it moved

```text
just before: libkernel::sceKernelMapperGetParam(0x600000800e20) -> 0x7fff0001 from 0x480000a1c760
just before: libc::abort(0xbe9c0)
```

PPSA28061's wall is now a named function taking one pointer, answering a placeholder, followed by
`abort`. It is not implemented and the run is unchanged - but it has stopped being a hash, which is
the difference between something to guess at and something to write.

Six of the seven remaining unnamed hashes are untouched: three are `PS5Util`, the game's own module,
which no vendor table will ever hold (D631), and three are in libraries this enumeration does not
cover.
