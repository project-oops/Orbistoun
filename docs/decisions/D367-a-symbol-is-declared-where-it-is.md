# D367 - A symbol is declared where it is imported, not where its code lives


**decided** - 2026-08-29

`clock_gettime` and `gettimeofday` are C library functions and were written as C library
functions, in `orbistoun-libc`. Declaring them there broke the audit: both were already
declared in `libScePosix`, because that is where a title was measured importing them.

The rule the audit enforces is one declaration per symbol, and it is right. A symbol declared
twice is two libraries claiming one function, and the trace then labels a call with whichever
declaration was found first.

### Which of the two declarations is true

The measured one. `libScePosix` is where a real title asked for these names; nothing here has
observed a `libc` exporting them on this platform, and inventing a second export to make the
code tidy would be inventing a fact about the target.

So the declaration stayed in `libScePosix` and the implementation stayed in `orbistoun-libc`,
bound by a delegation entry whose two halves are the same name. That reads oddly and is
correct: these have no vendor-named twin, so the POSIX name *is* the implementation's name.
The delegation still earns its place, because it is what binds a declaration in one crate to
code in another - and the existing test refuses a delegation that names nothing.

### The general shape

**Where a symbol is declared is a claim about the target. Where its code lives is a claim
about this repository.** They are answerable separately, they were conflated because they
usually coincide, and the audit is what noticed.

