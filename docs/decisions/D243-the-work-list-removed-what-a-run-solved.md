# D243 - The work list removed what a run solved, not what anything could name


**decided** · 2026-08-25 · found by comparing two committed files that both claim to be generated

`symbols/wanted.txt` says of itself that entries *"disappear as the vocabulary grows to
explain them"*. 116 of its 3829 entries were hashes `symbols/generated.json` could already
name, and 15 more were names a `guest_module!` declares.

The rule was `for solved in found { wanted.remove(...) }` - only what **this run** solved.
Its comment claimed to also drop *"hashes carried over from an earlier run that a later
vocabulary finally explained"*, and could not: a named hash is excluded from every later
search, because being named is exactly what excludes it, so it is never solved again, so it
is never removed. Once on the list, permanently on the list.

Two changes:

- `Service::is_named` extracts the condition `unnamed_imports` already used - **both**
  sources, the registry and the symbol database, which is why the fix removed 131 rather
  than 116.
- `wanted_now` is a pure function taking that as a predicate, so the rule is testable
  without a corpus, a database and a search. The bug was invisible while it was one
  function with the file writing, which is principle 8's shape argument arriving as a bug
  report rather than as a preference.

The rule is now "cannot be named now", not "was never solved". Those read alike. Pinned by
`a_hash_something_can_already_name_leaves_the_work_list`, confirmed to fail against the old
rule before being kept.

