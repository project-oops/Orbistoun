# D250 - A guest gets writable storage, and it is never the title's own directory


**decided** · 2026-08-25 · the conformance probe asked for it in its first few calls

`open` was read-only, and said so deliberately: *"a guest that could write through this would
be writing into the user's own title directory. Adding write access is a decision with
consequences, not an oversight to be corrected silently."* The comment was right to refuse,
and it named the actual objection - a guest able to edit `/app0` is editing the material
being measured.

The probe opens `/data/obscene-report.txt` within its first few calls. `/data` is storage the
console gives an application, separate from its read-only title, and orbistoun had nothing
there at all.

So the answer is **where**, not whether. `/data` resolves into storage the installation owns,
`/app0` stays read-only, and `mount::is_writable` is the whole of the distinction. Write
intent in the open flags is honoured under the first and ignored under the second, so a guest
that asked for more than it needed is not stopped.

