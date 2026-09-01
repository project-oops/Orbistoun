# D374 - The checkout is a newer FreeBSD than the target


**decided** - 2026-08-29

Every constant and structure this project reads comes out of one FreeBSD checkout, and that
has been fine for nine harvests. `stat` and `dirent` are where it stops being fine.

**The checkout is FreeBSD 15. The target's user space is a much older generation.** These are
exactly the two structures that changed shape in between:

```text
struct stat     st_dev 32-bit to 64-bit, st_nlink 16-bit to 64-bit, fields reordered
struct dirent   d_fileno 32-bit to 64-bit, and d_off added
```

Writing the modern layout for an older guest is not a call that fails. It is a file server
reporting the wrong size for every file, and nothing anywhere saying so - the exact failure
this project refuses on principle, arrived at by being careful about everything except which
release the header came from.

### Both layouts are in the same header

`sys/sys/stat.h` carries `struct freebsd11_stat` beside `struct stat`, and
`sys/sys/dirent.h` carries `struct freebsd11_dirent` beside `struct dirent`. So neither
layout is a guess about what a structure *is* - both are citable from the one checkout - and
the only open question is **which one this target uses**.

That is a hypothesis, the guest is the only oracle for it, and a hypothesis compiled in is
one nobody can test. So it is `ORBISTOUN_STAT_LAYOUT`, defaulting to the older, because the
target's user space predates the change - a reason rather than a measurement, which is why
the other is one environment variable away.

### The wider point, which applies to every harvest already done

The constants file already says *"these are FreeBSD's numbers, not the target's"*. That
caveat was about the **target being a fork**. This is a second axis nobody had written down:
the target is also a fork of a **particular release**, and a number that is stable across
releases and a structure that is not are different kinds of borrowing from the same source.

Error numbers, signal numbers and socket constants have not moved in decades. Structure
layouts have. The harvest is as citable as it ever was; what changed is knowing which parts
of it carry a second question.

### And C octal is not TOML octal

`S_IFDIR` is `0040000` in the header, and a leading zero is how C says octal. TOML rejects a
leading zero outright, so the first file mode harvested made **the whole table unparseable** -
and it surfaced as every constant in every section going missing at once, which reads like a
build problem rather than like one number.

The generator emits TOML's `0o` form now. Same number, same base, still comparable against the
header by eye - which is why it is not simply converted to decimal.

