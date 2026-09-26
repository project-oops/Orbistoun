# orbistoun-elf

Vendor ELF and PRX container parsing.

It reads the ELF64 file header and program header table, the container wrapper, the dynamic
segment and the relocation tables. `is_vendor_segment` detects vendor `p_type` values;
`dynamic::DynamicInfo` reads the dynamic table, and `dynamic::imports_from_symbols` turns the
symbol and string tables into the import list the loader resolves against.

## Rules

- Every parse produces a typed value or names what it could not handle. An empty import list
  would read as "this title needs nothing", which is never true, so no function returns one
  to mean failure.
- No `unsafe`. Every structure is read through `zerocopy`, which validates size and alignment
  before returning a typed value. The input is arbitrary bytes from outside the project.
- Bounds are enforced, not trusted: an absurd symbol count is an error, not an allocation.
- Vendor `p_type` values are recognised by range, not by individual constant. A wrong
  constant that silently mis-parses is worse than an explicit "not handled".
