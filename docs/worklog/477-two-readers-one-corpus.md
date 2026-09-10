# 477. Two readers, one corpus

**2026-09-09** - directed, continuing 476

`REQ-20260909T1720Z-4e17` - the exception context PPSA25872's handler dereferences - is still open,
so the signal wall is waiting on hardware. This pass paid the debt owed to SELFish since 474: the
two-reader differential, claimed three passes ago.

## The harness was wrong twice before either reader was

First run: all 29 modules disagreeing, `not an ELF: begins 54 14 f5 ee`. Orbistoun's
`Container::parse` unwraps the signed container on the way in; `selfish_elf::Elf::parse` expects
the ELF itself. The two were reading **different files**. Second run: still seven, because
`Wrapper::is_wrapped` misses the previous-generation magic and `is_either_generation` is the one to
ask.

Neither is a parsing defect. Both are facts about the two crates' entry points that anyone wiring
them together would first read as parsing bugs (D653).

## Three classes, once it was measuring the readers

- **22** - SELFish refuses the inner ELF: "the program header table runs past the end of the file".
  Every `sce_module/*.prx`. The inner ELF of a signed container is a view whose segments live in
  the container's own list, so the table legitimately points past the slice. Orbistoun tolerates
  it. **Which is right is unsettled**, and orbistoun's tolerance has never been tested against a
  deliberately truncated file - so this may be a defect here.
- **6** - orbistoun finds a vendor dynamic table where SELFish finds none. The defect shape their
  request already described, on six real eboots.
- **1** - `obscene/eboot.bin`, six fields, **every address differing by exactly `0x6bc000`**.

## The constant is the finding

A rebasing difference, not a parse difference - D247's own distinction between a standard tag
holding a virtual address and a vendor tag holding an offset into the vendor data segment. The
differential prints the file's tags beside the disagreement: it carries **both** standard (1-12,
20, 23) and vendor (0x61000007 … 0x61000049), which is what `Table::Current` describes. So
SELFish's reading may be the right one and orbistoun's `vendor_tables = false` the wrong one -
taken away to investigate.

## Surprises

- **The differential measured the harness until it didn't.** Two rounds of all-29 failures, both
  mine. Same rule as a guard nobody has made fail: a comparison is not comparing anything until
  somebody has checked what it is comparing.
- **Printing the file's own tags changed what the report is worth.** Without them it says "two
  readers differ"; with them it says which one the bytes support. Four extra lines.
- **The most likely defect the differential found is in this repository**, not the sibling one.

## Next

- `vendor_tables = false` on a file carrying vendor tags - orbistoun's side of (4).
- Whether orbistoun's tolerance of a program header table past the end of the slice is right, or
  is a bound it should be checking. Untested either way.
- The exception context at +0xf8, still open.
