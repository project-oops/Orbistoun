# 467. Two tables that answer each other

**2026-09-09** - directed, continuing 466

The kexport enumeration I filed at 10:50 arrived in the sweep before the request was marked
resolved. It named the function a title dies on.

## The search cannot reach it, and did not have to

`libkernel::0x04df812afad225d7` is what PPSA28061 calls before `abort`. Re-running the generated
search against the whole corpus:

```text
generated names: 3911843959 candidates across 11 patterns, 32 threads
generated names: 3911843959 tried in 267.4s (14631099/s), 0 named
```

Both words it needs are in the vocabulary and the grammar reaches longer siblings, so the shape is
simply not one of the eleven swept.

The enumeration gives hash-to-address for 2,443 entries. Orbistoun already parsed it and said
`2231 named by this project, 212 not` - a summary that cannot answer *is the hash a guest actually
calls among them*. It crosses the two sets now:

```text
of 7 hash(es) a guest here calls and nothing can name, this table holds 1
  libkernel::0x04df812afad225d7  at 0x800031d60, with no named hash at that address to derive from
```

`0x800031d60` is libkernel + `0x31d60`, and this project's own firmware layout has had a name there
all along: `sceKernelMapperGetParam`. The hash agrees exactly, which is the only proof accepted here
(D642).

```text
just before: libkernel::sceKernelMapperGetParam(0x600000800e20) -> 0x7fff0001 from 0x480000a1c760
just before: libc::abort(0xbe9c0)
```

## A new mechanism, so a new variant

`StaticSource` is closed on purpose - *"a new mechanism adds a variant here; it does not add a new
sentence"* - and this is one: a console export table gives hash-to-address, a firmware layout gives
address-to-name, and neither answers the question alone. `FirmwareLayout` records it.

## Also this pass

- **86 then 35 new measurements** accounted for across two sweeps, grouped by family with a
  completion condition each; four that stopped being constant left the lists, one of them from
  `CLAIMED` rather than the tuple lists, which the block remover does not reach.
- The frontier and status regenerated; `symbols/generated.json` is 30,184 names and the audit
  accounts for all of them.

## Surprises

- **The answer was in two files this project already had.** The firmware layout has named that
  offset since it was written; the export table arrived this morning. Neither was a new fact - the
  *pairing* was.
- **A summary hid it.** "212 not named" was true, printed, and useless; the intersection with what
  a guest calls is one line and it is the whole finding.
- **Four self-inflicted doc-comment splits** in one session, every one from inserting a function
  above an existing one. Worth a habit: insert after the closing brace, never before a comment.

## Next

- Implement `sceKernelMapperGetParam` - one pointer argument, and the guest aborts on the answer.
  Nothing measured says what it should return, so it needs a knowledge entry first.
- The libc data-object request (`REQ-…-8e4a`) is still open; it is PPSA21564's wall.
- The title-module binding still needs `relocate.rs`.
