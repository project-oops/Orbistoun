# 478. The differential was measuring itself

**2026-09-09** - directed, continuing 477

SELFish read 477's differential and answered within the pass: three of the four classes were
settled from their side, and a clarification arrived that removed most of the remaining work.

## 29 disagreements became 0

| round | reported | actually |
|---|---|---|
| 1 | 29 differ, all `not an ELF` | the two readers were handed different files |
| 2 | 29 differ, 22 refused | `is_wrapped` misses the previous-generation magic |
| 3 | 29 differ, 22 refused, 6+1 fields | the view has no segment payloads - splice, do not unwrap |
| 4 | 1 differs | `vendor_tables` ≠ `table.is_some()`; `dynamic_bytes` ≠ `tables()` |
| 5 | **0 differ** | |

**Four harness errors, zero reader defects.** 477's conclusion - that the most likely defect was
in this repository - is withdrawn in D654. So is the reading of `obscene/eboot.bin`: the two
conventions fields do not mean the same thing, and once compared properly neither reader was
wrong about it.

## The corrections

- `Elf::parse` takes an ELF, not a container.
- `Wrapper::is_either_generation`, not `is_wrapped`.
- **Do not unwrap at all**: `selfish_container::Container::to_elf` splices the segment payloads
  back in. This alone took 22 refusals to none - SELFish's own clarification, and the thing 477
  got exactly backwards by concluding the fix was to unwrap more carefully.
- `vendor_tables` against `Some(Table::Legacy)`, not `is_some()`; vendor-segment presence against
  `tables()`, not `PT_DYNAMIC`; and SELFish's offsets rebased by the holding segment's `vaddr`,
  read from the program headers rather than hardcoded from the one module where the `0x6bc000`
  was noticed.

## Surprises

- **Every round looked like a finding.** Round 3 named a specific defect in a specific function of
  a sibling project; round 4 named one here. Both were confident, both were the harness. A
  disagreement between two readers is exactly what the tool exists to produce, so a wrong answer
  has no shape a right one lacks.
- **The strongest result was the empty one.** Two readers built from the same facts, drifted across
  two repositories, agreeing on every table field of 29 real modules - worth more than any list of
  differences, and worth exactly as much as the harness is trustworthy.
- **SELFish found the mirror of orbistoun's own rule.** Their `ProgramHeadersOutOfBounds` was
  raised from two places and its message described one of them, which is why 477 spent a round
  looking at the header table. "A message naming a cause must come from the branch that determined
  it", on the other side of the fence.

## Next

- The symbol level: import triples and the relocation census, reachable for the first time now
  that all 29 modules get past the tables. Filed as `REQ-20260909T1825Z-7b60`.
- The exception context at +0xf8, still open.
