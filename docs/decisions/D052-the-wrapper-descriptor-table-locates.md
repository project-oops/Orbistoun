# D052 - The wrapper descriptor table locates segment data, not the ELF headers

**decided** · 2026-08-19 · observed and verified against real material

The inner ELF's program headers describe a **virtual** layout. Their `p_offset` values
routinely point past the end of the container - on one 74 KB module, eleven of
fourteen headers did. They are not file offsets and must never be used as such.

The wrapper's descriptor table is what actually locates the bytes:

- A descriptor with the **`0x800`** flag bit carries program-header data. The others
  are small paired blocks (0x20-0x60 bytes, almost certainly digests).
- A data-bearing descriptor names its program header in **flags >> 20**.
- Its `stored_size` equals that header's `p_filesz`.

**Verified**: across an executable and three modules, every data-bearing descriptor
matched its program header's size exactly, and the Rust implementation independently
reproduces the mapping the analysis found - `[0, 1, 3, 7, 8, 11]` on both.

**Headers absent from the mapping are normal, not missing.** Several describe regions
*inside* another header's data. `PT_DYNAMIC` is the case that matters: it has no
descriptor of its own and lives inside the data of program header 8, reachable by
offsetting within that descriptor's bytes. Doing so yields 51 dynamic entries - a mix
of standard `DT_NEEDED` and vendor tags - which is the path to the import table.

**The index bits are only meaningful on data-bearing descriptors.** On the paired
metadata blocks the same bits hold something else, observed as the *wrapper-table*
index of the descriptor they accompany. Reading one as a program-header index is a
mistake the API documents against.

### Two corrections this forced

**D012's vendor `p_type` range was too narrow.** It asserted
`0x61000000..=0x61FFFFFF`; a single ordinary module carries vendor segments at
`0x61000002` **and** `0x6fffff00`/`0x6fffff01`. The narrow range saw one of three.
Replaced by the full OS-specific range minus the GNU extensions, which are ordinary
and would otherwise be miscounted as unhandled vendor data.

**The header's size field is not the file length.** Initially implemented as a
truncation check; on real material it is consistently *smaller* than the file by a
variable amount (38 bytes on a module, 10,670 on an executable), so it measures some
region. What region is not established, so nothing is inferred from it - it is
reported and left alone (D010).

