# D370 - A constant one crate hardcodes is checked by the crate that holds the table


**decided** - 2026-08-29

`access` compares a guest's mode against `W_OK`, and `orbistoun-fs` is where that comparison
lives. It cannot read the harvested ABI table: that table is `orbistoun-libc`'s, and `libc`
depends on `fs`, not the other way round.

Three ways out, and only one of them is honest:

- **Hardcode it and say nothing.** How a constant comes to be a different platform's number
  and nobody notices. `SOL_SOCKET` is `0xffff` here and `1` on several others; there is a test
  about that for a reason.
- **Move the table.** A whole subsystem's dependency graph rearranged for one number.
- **Name the constant where it is used, and test it where the table is.** `orbistoun-fs`
  exports `W_OK`, and `orbistoun-libc` holds a test asserting it equals what the harvest read
  out of `sys/sys/unistd.h`.

The third costs one `pub const` and one test, and it fails loudly the day the two disagree.
The general rule: **a constant may be written wherever it is used; it may not go unchecked.**
Whichever crate can reach the harvested table owns the check.

