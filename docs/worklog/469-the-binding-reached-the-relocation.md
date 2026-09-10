# 469. The binding reached the relocation

**2026-09-09** - directed, continuing 468

`crates/orbistoun-loader/` is still denied to this session - the rule matches the path *and* the
text, so even a grep naming `orbistoun_loader::relocate` is refused. The work finished anyway, by
arranging things so the unreadable part did not need reading.

## Two faults, one behind the other

D640 fixed the first: `bind_to_title_modules` read the executable alone, so a module importing from
a **sibling** module was never a candidate. That made the binding correct and changed nothing,
because it was never applied.

**The order.** It was computed *after* the module relocation loop. A relocation that has run cannot
be told which slot should have pointed into a placed module. Moved above the loop, from the same
`placed`, `modules` and `slots` already in scope there.

**The composition.** Modules relocated through the offsetting resolver and never through the
binding one. Wrapping raises a question - which sees the shifted index? - that the denied file
answers.

## Sidestepped, not guessed

One map **per module, keyed by that module's own symbol index**, with the binding *outside* the
offsetting. The binding resolver then never sees a shifted index, and the offsetting one shifts only
what falls through to the shared stub table. Neither needs to know what the other does with the
number, so the ordering question does not arise (D644).

What made the shape safe to assume without reading it: the executable's path has worked this way
since it was written - a map keyed by symbol index, which for module zero is also the slot.

## What it moved

```text
PPSA25872-app0   before:  141 imports  20000000 calls    2% standing
PPSA25872-app0   after:   141 imports    310987 calls  100% standing
```

**19,689,013 calls gone.** They were the guest asking one function the same question until its
budget ran out - 87.6% of every call this project had ever recorded. It calls that function inside
the module the title ships now, so orbistoun never sees it. Eight calls land on stubs out of three
hundred and ten thousand.

PPSA02664 gained an import (199) and PPSA03416 gained one (198, with its fault moved to the same
`image+0x39f7c` the other two stop at). Nothing lost one.

## Surprises

- **The guard I built for this stayed silent both times.** `bound_yet_called` fires on "bound and
  still called through a stub", which is precisely what composing the resolvers backwards would
  have produced. It was silent before because nothing was bound at those indices and silent after
  because the bindings took effect - and it would have shouted in between.
- **The deny rule matches text, not just paths.** A `grep` mentioning the module path was refused
  even though the file it searched was a different crate. Worth knowing before assuming a tool
  failure is a code problem.

## Next

- The AGC contract - now where PPSA02664, PPSA03416 and PPSA28061 all stop, and unreachable from
  any obSCEne leg (D641).
- `sceKernelMapperGetParam`'s fifty-six bytes (D643).
- The libc data-object request, still open, still PPSA21564's wall.
