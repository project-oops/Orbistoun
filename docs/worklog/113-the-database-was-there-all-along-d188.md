# 2026-08-21 - The database was there all along (D188)


Chasing the `tlsf_create` wall named `memalign` by hash from the repository's own word list -
and then a check turned up that `symbols/generated.json` already held it. And `printf`. And
405 others.

Nothing loaded it. `--symbols-db` was opt-in and defaulted to no database at all, so every
run in this session reported hashes the tree could already name, and advised extending the
vocabulary to find them. **A twenty-minute name search was run to discover `printf`, which
was sitting in the committed database the whole time.**

Worse than a missing feature, because the findings are what this project is for and they
were confidently recommending the wrong next action - a positive false claim rather than an
absence a reader might notice.

The first fix went in the CLI and did nothing: the worker loads names from a path and the
CLI passes the path, not the loaded database. Principle 13 again - a shim resolving a
default the layer below ignores. Corrected where the naming actually happens, so the GUI
gets it too.

Also, the stack poison built earlier got its first use and returned a clean **negative**:
eight megabytes of `0xCC` instead of zeros produced an identical run - same imports, same
calls, same fault. The guest is not reading unwritten stack on this path, so the D171
explanation for `tlsf_create` is eliminated rather than argued about. The condition line
labelled the run, so the "same" verdict could not be misread as a comparison of equals.


