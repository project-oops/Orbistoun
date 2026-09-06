# 2026-09-03 - (/loop) The worker links the title, and the verdict turns out to be noisy

```
tests   1984  ->  1984   (a wiring change; the slice's own tests landed in 334-336)
```

Worklog 336 linked the title's own modules and left the CLI as the only caller. This wires it
into the worker, so a run actually gets them.

```text
reached ContainerParsed
reached ImportsResolved
reached Mapped
reached Linked
reached Entered
```

The guest gets far enough to print its own launcher banner:

```text
Argument Count = 1Arg 0 = /app0/titles/PPSA02664-app0/eboot.bin
********** LAUNCHER CONTROL ************************************
	File System Root = NONE SPECIFIED
**********/LAUNCHER CONTROL ************************************
TODO:static void LocalFileSystemPS5::SetupArchive(const char *, const char *)
```

## What the wiring changed in the code

`what_imports_resolve_to` built a table for one module. It is now `link_the_title`, which calls
`link_title_modules` - so the executable and every module the title ships share **one** stub
table, the executable at offset 0 (D484). Its imports keep the slot numbers every trace and
every measurement already refer to, and the resolver for it needs no offset at all.

Two things fell out that are worth naming:

- **`install_data_symbols` moved inside the link** and the worker's own call was removed. It is
  a `OnceLock`, so leaving both would have read as though the second did something.
- **`module: &str` was deleted as a parameter.** It was `path.display().to_string()`, and once
  `path` had to be passed too, two parameters that must agree is one more thing that can
  disagree. Clippy's arity warning is what prompted looking; the redundancy was the real find.

## Then the measurement, which did not say what the first run said

The first run reported `FURTHER +1 distinct import` and that was nearly the write-up. **It was
noise.** Seven runs of identical code alternate between two states:

```text
68 distinct, 10905 calls      verdict BACK
69 distinct, 10902 calls      verdict FURTHER
                              verdict same
```

All three verdicts, same binary, same limit, stable fault address. D487 records it: the one
measure of progress this project has is not reproducible at the granularity it reports, and a
one-import delta is not a signal.

## Interleaved, the linking does show a real effect

Three runs of each path, alternated rather than batched:

| | calls | distinct imports |
|---|---|---|
| single-module | 10884, 10887, 10884 | 69 every time |
| title modules linked | 10902, 10905, 10902, 10905 | 68 or 69 |

**+18 calls against a ±3 spread** - outside the noise, and a real effect. The distinct-import
count shows no gain and sometimes one fewer.

The two numbers disagree because they measure different things: the guest does measurably more
work, and does not reach a wider part of the platform interface. The headline verdict keys on
the second, which is why it reads `same` or `BACK` for a change that plainly did something.

## The reservation warning is not new

```text
orbistoun: a reservation failed: base=0x6b0000000000 len=0x440000 - conflict
```

`0x6b00_0000_0000` is `POLICY_REGION_BASE`, which is in code touched this week, so it was worth
not assuming. **The A/B settled it**: the message appears identically on the single-module path.
Pre-existing, and left alone.

## What is still not true

The guest crashes in the same place it did before, at `0x7fff0001`, flagged by the reporter as
orbistoun's own code rather than the guest's. Nothing here got past that. The title's modules
are linked and reachable; the guest has not been shown to execute any of them.

## State

`cargo test --workspace` green - **117 suites, 1984 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-337 and D466-D487.

**Next**: diagnose the nondeterminism - it is the thing standing between every future change
and a trustworthy verdict, and the guest spawning threads is the first place to look. Then the
fault at `0x7fff0001`, which the reporter already says is orbistoun's own.
