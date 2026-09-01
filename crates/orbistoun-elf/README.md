# orbistoun-elf

Vendor ELF and PRX container parsing.

**Models:** the ELF64 file header and program header table, the container wrapper, the
dynamic segment, and the relocation tables. Vendor `p_type` values are detected via
`is_vendor_segment`; `dynamic::DynamicInfo` reads the dynamic table, and
`dynamic::imports_from_symbols` turns the symbol and string tables into the import list a
loader resolves against.

**Deliberately fakes:** nothing. Every parse either produces a typed value or names what
it could not handle. An empty import list would read as "this title needs nothing", which
is never true, so nothing here ever returns one to mean failure.

**Design note.** Zero `unsafe`. Every structure read goes through `zerocopy`, which
validates size and alignment before returning a typed value. Parsing hostile bytes
is the last place that should contain hand-rolled pointer casts, and it does not.

Individual vendor `p_type` constants are not asserted - only the range. A wrong
constant that silently mis-parses is worse than an explicit "not handled yet", and
the two cost the same to write.

**Status:** done for everything the loader needs. Standard ELF parsing is tested against
truncation, bad magic, and wrong class; the dynamic segment and relocations carry every
commercial executable in the local corpus from bytes to an entry point.

Bounds are enforced rather than trusted - an absurd symbol count is an error, not an
allocation. That matters because the input is arbitrary bytes from outside the project.
