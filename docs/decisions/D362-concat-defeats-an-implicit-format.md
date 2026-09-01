# D362 - `concat!` defeats an implicit format capture


**decided** · 2026-08-29

The prose gate's advice is *"use `concat!` of one-line literals instead"*, and applying it
mechanically to the five files still carrying line-continued literals broke the build in
three places:

```
error: there is no argument named `says`
error: there is no argument named `hole`
error: there is no argument named `serves`
```

**An implicit capture needs a literal, and `concat!` is a macro call.** `format!("{says}")`
finds `says` in scope; `format!(concat!("{says}"))` does not, because by the time `format!`
sees anything the argument list is already fixed and the braces are just text.

It fails loudly, which is the saving grace - but only at the site being converted. A
conversion done in bulk without building would have been three quiet breakages in files
nobody was reading.

The fix is a **named argument**: `format!(concat!("...{says}..."), says = says)`. That keeps
both the capture and the one-line-literal rule, and it says out loud what was implicit.

Worth writing down because the advice is given by a gate that cannot know about it: the gate
sees a backslash and says what to do, and what to do is right in every case except this one.

### Why the tree had five such files at once

They were written by a session working in parallel and were never its own to finish. With
the tree quiet, they were fixed here rather than left - a gate that has been red for a day
stops being read, which costs more than the files did.

