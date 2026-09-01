# Per-area coverage, and what a skip is not


`sectiontally` is read now, paired with `section` by identifier, and `orbistoun probe`
reports each area of the platform with how much of it came out green.

The point is that one total cannot say what is *understood*. Ninety passes spread thinly
across every area and ninety concentrated in one are the same number and completely
different situations, and only the second means a subsystem can be relied on. The section's
`purpose` line - the probe's own words about what it establishes - is carried rather than
summarised.

On the real report:

    areas     4 of 8 wholly green
      + 010-kernel           Kernel core            3 pass
        035-libc             C runtime              5 pass, 1 partial, 12 fail, 2 skip
        040-file             Filesystem             0 pass

### Surprises

**The area view tells the story of that run in three lines.** Boot, kernel, memory and
threads are wholly green; the C runtime is badly broken; and the filesystem section reports
**no passes at all** - because the check that never concluded is inside it. A single total
would have shown neither where the strength is nor where the run stopped.

**A skip is not green, and saying so took a deliberate decision.** A skip is a check that
did not run, so the section did not establish what it claims to. Rounding one up is how a
subsystem gets relied on for something nobody tested - the same error as reading a death as
a return value, one level up. It stays in the denominator.

**A half-reported section still appears.** A section with no tally, and a tally naming no
section, are incomplete reports rather than absent ones. Dropping either would shrink the
denominator, which flatters the result in the one direction nobody should be flattered.

**Two clippy rules asked for real improvements rather than appeasement this time.**
`parse_record` had grown past a hundred lines, and the right split was to lift out the
outcome parsing - which carries the rule this whole crate is shaped around, so a rule worth
stating turned out to be worth being able to find. And `pass` is now printed even at zero,
because a section reporting no passes is the interesting case and omitting the number would
make it look like a section with no checks rather than one where nothing worked.

