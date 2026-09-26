# compat/

What each title needs, and what it last did. One TOML file per title, named by its title id and
tracked by git.

Nothing here is guest material. A record holds a title identifier, settings, and numbers read off
a run - never guest bytes, never a path into a title library. Guest material lives in the title
library outside the repository; what was learned from it lives here, so a finding travels when
the title cannot.

## Record format

A record holds what Orbistoun sets for a title and what Orbistoun got from it. Every section is
optional.

```toml
[title]                  # read from the title's own param.json; absent for a bare payload
id = "GLCB00001"
name = "GL1 Cube"
content_version = "01.00"
master_version = "01.00"

[hardware]               # what the real hardware does with it, attested from outside
does = "renders"
attested_by = "operator" # or an obSCEne probe id
on = "2026-09-26"
note = "..."

[compat.direct_memory_alignment]   # a deviation Orbistoun applies, written by a person
value = 4096
kind = "workaround"                # quirk, workaround or unsupported
reason = "..."                     # mandatory

[settings]               # ordinary preferences scoped to this title
# key = value

[status]                 # the best unaided run - written by `compat record`, never by hand
reach = "entered"
outcome = "image+0x43c4"
imports = 47
calls = 933
standing = 85
default_return = "unimplemented"
unanswered = 9
limit_seconds = 40
build = "v0.1.0 - <commit>"
measured_on = "2026-09-26"

[experiment]             # the best helped run, same fields as [status]
overrides = 2
propping = 2
frames = 975
```

### Compatibility kinds

| `kind` | Meaning |
|---|---|
| `quirk` | the title does something out of spec that the hardware tolerates; permanent |
| `workaround` | masks a defect in Orbistoun; removed when the defect is fixed |
| `unsupported` | a capability Orbistoun lacks; removed when it exists |

A key names the behaviour, never the title, so a second title needing the same thing adds a line
rather than a code path.

### Run fields

| Field | Meaning |
|---|---|
| `reach` | the furthest rung: `rejected`, `parsed`, `linked`, `entered`, `exited`, `flipped`, `presented` |
| `outcome` | how the run ended: the fault site, the guest's own exit, or the limit |
| `imports` | distinct imports the guest called |
| `calls` | total calls through any import |
| `standing` | percentage of calls that reached an implementation rather than a placeholder |
| `default_return` | what unimplemented functions answered |
| `overrides` | functions handed a specific answer instead of the default |
| `propping` | how many of those answers rest on nothing measured |
| `frames` | frames the guest handed to the output layer |
| `unanswered` | distinct imports called that had nothing behind them; absent when not measured |
| `limit_seconds` | the wall-clock limit the run was given |
| `build`, `measured_on` | the build and the day that produced it |
| `notes` | anything the numbers do not say |

The `[status]` and `[experiment]` sections are derived from a trace, never typed. A run whose
default return is not `unimplemented`, or that has any `propping`, is a helped run and records in
`[experiment]`; every other run records in `[status]`. The two answer different questions - how
far the emulator takes the title, and how far it could if the propped answers were real - so
neither overwrites the other.

A `[hardware]` attestation says whether the title is sound on the hardware. Without one, a fault
is treated as Orbistoun's gap (D708).

## Recording a result

```
orbistoun-cli run <path-to-eboot.bin>
orbistoun-cli compat record <path-to-eboot.bin>
```

`record` refuses to replace a better entry in the same section; `--force` records anyway, for a
deliberate correction. `--note <text>` adds a note.

`orbistoun-cli compat markdown` renders every record into `COMPATIBILITY.md` and `docs/titles/`;
`--check` fails if either is out of date.

## Contributed entries

A record carries the build and the time limit that produced it, because a result measured with
another machine's build and settings is not comparable to a local one.
