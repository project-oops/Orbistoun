# D191 - The harvest walker ignored the rule written to fix it


**decided** · 2026-08-22

Re-harvesting from a FreeBSD checkout, to close the provenance gap D168 describes, lost 147
names and added six. A harvest that reports success while shrinking is the shape worth
chasing, and it led to a defect older than the exercise.

### Two implementations of one rule, and only one was fixed

`orbistoun_names::harvest::is_version_script` accepts **any** `.map` file. Its doc comment
records why: `libthr` calls its exports `pthread.map`, and a rule that looked for
`Symbol.map` cost every `pthread_*` name (D127).

The `harvest` command does not call it. It tests `file_name() == "Symbol.map"` directly, and
has since before D127 was written. The fix landed in the crate and in an example binary; the
command that anybody actually runs kept the original rule and kept reporting success.

So the walker skipped `lib/libthr/pthread.map`, `lib/libsys/Symbol.sys.map`,
`Symbol.thr.map`, `syscalls.map` and every per-architecture map - ten files of fifty-seven.
Wired to the rule that already existed: **2685 names to 3064**, and the two "lost" entries
are words from the old file's own header.

`lib/libsys` also had to be added to the directory list: FreeBSD moved its system-call stubs
out of `libc`, so `socket` and its neighbours left from under a constant naming `lib/libc`.

### D168 was right about the symptom and wrong about the cause, and so was I

D168 said the harvest was missing a syscall family. True. Its proposed fix - re-harvest from
a fuller checkout - was not the fix: depth and completeness were never the problem. A
directory had been renamed underneath one constant and a filename convention had never been
honoured by another.

**Two predictions made before doing the work, both wrong, both by generalising from the head
of a list.** Reading five names off the top of the audit's output, I said it would turn
"~187 records" green: it turned a handful green, because the unaccounted are 202 vendor
`sce*` names and 17 C++ ABI names, not syscall wrappers. And I said the enlarged vocabulary
would plausibly name several of the corpus's unnamed imports: it names **none** of the
twenty-six.

The work was still worth doing, for a reason neither prediction named - it surfaced a defect
that had been silently degrading every harvest since D127.

