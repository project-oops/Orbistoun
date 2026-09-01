# compat/

**What each title needs, and what it last did.** One file per title, tracked by git.

Nothing here is guest material. A file holds a title *identifier*, settings, and numbers
read off a run - never bytes, never a path into anybody's library. `titles/` holds the
material and is never tracked; this directory holds everything we learned from it and is
always tracked. That split is what lets a finding travel when the title cannot.

## One file, two directions

A file describes what orbistoun **sets** for a title and what orbistoun **got**:

```toml
[status]                 # what we got - written by `compat record`, never by hand
reach = "entered"
outcome = "image+0x43c4"
imports = 47
calls = 933
standing = 85            # % of calls that reached an implementation
default_return = "unimplemented"

[compat.direct_memory_alignment]   # what we set - written by a person, with a reason
value = 4096
kind = "workaround"
reason = "..."
```

They live together because they are keyed by the same title and edited in the same
session. Two files would immediately disagree about which was current.

The `[status]` half is **derived from a trace, never typed**. A hand-written grade drifts
the moment somebody is optimistic, and nothing can check it afterwards.

## Recording a result

```
orbistoun-cli run titles/<title>/eboot.bin
orbistoun-cli compat record titles/<title>/eboot.bin
```

`record` refuses to overwrite a better entry, and refuses to claim an improvement produced
by loosening the stub policy - a run where unimplemented functions report success reaches
further by construction and means less. See docs/DECISIONS.md D181 and D182.

## Contributed entries

An entry carries the build and the limit that produced it, because a result measured on
somebody else's machine with somebody else's settings is not comparable to yours and the
file has to be able to say so.
