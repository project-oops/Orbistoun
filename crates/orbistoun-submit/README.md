# orbistoun-submit

What one machine has to contribute, gathered into one directory.

The loop does not need this repository. The guest itself is an oracle, expensive per query and
per person, and somebody running a binary against a title nobody here owns is consulting the
same oracle. `orbistoun-cli submit export` gathers what their machine found; `submit check`
checks what somebody sent.

A submission carries two kinds of claim:

- **Measurements** - what a function must answer, and what that rests on.
- **Title results** - how far one title got, and under which policy.

## Rules

- **Re-derive, never trust.** A receiving machine checks a submission by re-deriving it locally
  and comparing. A claim this machine never measured is reported as unmeasured, not as a
  contradiction; an accepted claim enters as `assumed` until somebody who owns the title
  confirms it.
- **Claims, not behaviour.** The crate has no path to the emulator, the loader or a model
  runtime, so a bundle cannot carry behaviour. A source change is a proposal, which nothing
  here can check, and is not part of a submission.
