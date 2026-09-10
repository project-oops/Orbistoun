# D653 - A second reader over one corpus

**Status:** measured
**Date:** 2026-09-09

## Why a sibling project's parser earns a dev-dependency

A parser has no oracle. Hostile bytes produce a plausible answer whatever it does, and the two
places this project can check itself - a guest that runs, and a test with a fixture somebody wrote -
both only catch what somebody already thought of. `selfish-elf` was built *from* `orbistoun-elf`
and both have moved since, which makes it the one instrument neither project has alone: the same
knowledge, written twice, drifted independently.

The evidence it works is not speculative. obSCEne's migration to those crates found four defects
this way, two in SELFish and two in obSCEne, and none was visible to either project on its own
(SELFish `REQ-20260909T1452Z-b91d`).

**A dev-dependency, and a differential rather than a migration.** Orbistoun's reader is the one
that runs a guest; nothing here replaces it. The test reports disagreements and asserts nothing
about which side is right, because deciding that needs a person and a third source. It skips when
the corpus is absent - installed titles, not repository fixtures - and prints what it looked for,
because a silent pass is indistinguishable from agreement.

## The harness was wrong twice before either reader was

The first run reported all 29 modules disagreeing: `not an ELF: begins 54 14 f5 ee`. Orbistoun's
`Container::parse` unwraps the signed container on the way in and `selfish_elf::Elf::parse` expects
the ELF itself, so the two were reading **different files**. The second run still had seven,
because `Wrapper::is_wrapped` misses the previous-generation magic and `is_either_generation` is
the one to ask.

Neither is a defect in either reader; both are facts about their entry points, and both would have
been read as parsing bugs by anyone wiring these together without checking. *A differential
measures the harness until it doesn't* - which is the same rule as a guard nobody has made fail.

## What it found, once it was measuring the readers

29 modules, 29 disagreeing, in three classes:

- **22: SELFish refuses the inner ELF** - "the program header table runs past the end of the file",
  every `sce_module/*.prx` and one eboot. The inner ELF of a signed container is a *view*: its
  program headers describe segments whose bytes live in the container's own segment list, so the
  table legitimately points past the end of the inner slice. Orbistoun tolerates it; SELFish does
  not. **Which is right is not settled**, and orbistoun's tolerance has never been tested against a
  deliberately truncated file - so it may be the lax one, and that would be a defect here.
- **6: orbistoun finds a vendor dynamic table where SELFish finds none** - every remaining eboot.
  The shape of the `vendor_segment` defect their request already described, on six real modules.
- **1: `obscene/eboot.bin`, six fields, and every address differs by exactly `0x6bc000`.**

## The constant is the finding

```text
strtab   orbistoun 0x6bc018   selfish 0x18
symtab   orbistoun 0x758230   selfish 0x9c230
hash     orbistoun 0x9df588   selfish 0x323588
rela     orbistoun 0x8297b0   selfish 0x16d7b0
jmprel   orbistoun 0x828400   selfish 0x16c400
```

Not a parse difference - a **rebasing** difference, and it is this project's own D247: a standard
tag holds a virtual address, a vendor tag holds an offset into the vendor data segment, and reading
one as the other lands somewhere plausible and wrong. One reader rebased and the other did not.

The differential prints the file's own tags beside the disagreement, so nobody has to re-derive
them: it carries **both** standard tags (1-12, 20, 23) and vendor tags (0x61000007 … 0x61000049).
That is exactly what `Table::Current` describes - so SELFish's `Some(_)` may be the correct call
and **orbistoun's `vendor_tables = false` the wrong one**. Printing the evidence was the difference
between a report the other project can act on and one it would have to reproduce first.

## What is deliberately not in it

The per-import `(nid, library_id, module_id)` triples and the relocation census, both asked for.
The table-level disagreements stop 28 of 29 modules before there is a symbol table both readers
agree exists, so comparing triples would have been comparing one reader against nothing. It is a
follow-up once the table classes settle, filed anew rather than reopened.
