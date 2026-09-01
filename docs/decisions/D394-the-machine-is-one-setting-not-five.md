# D394 - The machine is one setting, not five constants


**assumed** - 2026-08-30

Fixing the above left five functions each returning a hardcoded constant: retail yes, devkit
no, test kit no, faster revision no, development mode no. Five places to be inconsistent, and
no place that says what was intended.

They are answers to one question. A guest asks them separately and they are **not
independent**: exactly one kind can be true, and a machine that says yes to two is not a
machine - which is precisely how the bug above stayed invisible.

So the machine is the setting:

```toml
[machine]
generation = "ps5"   # ps4 | ps5
kind = "cex"         # cex | dex | tex
revision = "base"    # base | pro
```

and the answers are derived from it. A guest that takes a devkit path takes it because somebody
chose a devkit, and the run says so before it starts: `presenting a ps5/dex/pro machine`.

### Where it lives, and why there

With the rest of what the console is **set to**, not with the installation's own configuration,
because it travels: a title's behaviour on a retail PS5 is a fact about that pairing rather than
about whose computer it ran on (D326).

The types themselves are in `orbistoun-core`, because two layers that cannot see each other both
need them - the shell stores it, the kernel answers a guest from it - and a domain type shared by
every layer is what that crate is for. It is published downwards the way the stack span and the
module list are: told, never re-derived (D275).

### Confirmed by something other than its own log line

Switching to `dex`/`pro` changes what the probe *measures*: `015-sync/machine-kind` goes from
`pass 0x1` to `pass 0x2`. It is detecting a different machine, rather than this project
believing it presented one.

### What is still one machine when it should be a setting

- **The system software version.** `sceKernelGetSystemSwVersion` is imported and unimplemented,
  and titles branch hard on it. The biggest one left.
- **Direct memory size**, which differs on development hardware, and which `kind` should decide.
- **Region**, which nothing here models at all.
- **The console generation's *surface***. `generation` picks what the machine says it is; it does
  not yet pick which symbols exist, so the probe still reports *both generations' drivers
  resolve*. That is the same root as D392 and wants the same lever.

