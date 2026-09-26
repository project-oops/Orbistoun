<p align="center">
  <img src="assets/logo.png" alt="Orbistoun" width="200">
</p>

# Orbistoun

Orbistoun is a clean-room high-level emulator (HLE) for Orbis-generation and
Prospero-generation software, written in Rust. Guest and host share the x86-64 architecture, so
guest code executes natively with no interpreter or recompiler. Orbistoun reimplements the layer
beneath it: guest memory mapping, dynamic linker relocation of NID-hashed imports, threads and
synchronisation, and translation of the guest's GPU command streams and shader bytecode to Vulkan
and SPIR-V.

- [docs/features/user-guide.md](docs/features/user-guide.md) - running titles from the GUI and
  the command line.
- [docs/README.md](docs/README.md) - the technical reference index.

## Building

Orbistoun is one member of the [OOPS collection](../README.md). From this directory:

```bash
./bin/orbistoun doctor --fix   # check the toolchain; --fix installs the optional gate tools
./bin/orbistoun check          # build, lint and run the test suite
```

[docs/BUILDING.md](docs/BUILDING.md) lists what `bin/orbistoun` needs and every verb it takes.

## Running a title

```bash
./bin/orbistoun run GLCB00001
```

`run` builds `orbistoun-cli`, resolves the title id in the title library
(`orbistoun-cli paths` prints where it is), runs the guest until it exits, faults or reaches the
time limit, and prints the verdict against the previous run (`FURTHER`, `same` or `BACK`)
followed by ranked findings. Arguments after `--` go to `orbistoun-cli run`.

The desktop shell is `orbistoun-gui`; [docs/features/](docs/features/README.md) describes each
screen beside its command-line equivalent.

## Querying what Orbistoun knows

`./bin/orbistoun cli <args>` runs `orbistoun-cli`.

| Command | Purpose |
|---|---|
| `orbistoun-cli symbols` | every system-library function Orbistoun declares |
| `orbistoun-cli questions` | open questions, ranked by how often guests call them |
| `orbistoun-cli worklist` | what to implement next, ranked across all recorded runs |
| `orbistoun-cli knows <symbol>` | the evidence and citation behind one function |
| `orbistoun-cli compat list` | how far each recorded title reaches |

[docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) is the generated status page, and
[COMPATIBILITY.md](COMPATIBILITY.md) the generated per-title table.

## The loop

```
Guest executable (oops-apps homebrew or a retail title)
         |
         v
+----------------------------------------+
| Orbistoun: native execution,           |
| HLE library calls, GPU translation     |
+-------------------+--------------------+
                    |
          [fault / stub / limit]
                    |
                    v
+----------------------------------------+
| orbistoun-turn: mechanical findings    |
| - snapshot unwritten struct memory     |
| - watchpoints on empty slots           |
| - trace diff: FURTHER / same / BACK    |
+-------------------+--------------------+
                    |
          [unmeasured question]
                    |
                    v
  a person runs an obSCEne probe on the
  hardware through Prosperous
```

1. **Execution.** Orbistoun maps the guest into memory, resolves every import statically and
   jumps to the entry point.
2. **Mechanical findings.** When a guest faults, `orbistoun-turn` runs the steps that need no
   person - argument sweeps, and watchpoints naming the register or unwritten field behind the
   fault - and ranks what to try next.
3. **Hardware measurement.** An unmeasured function or structure is a person's step: an
   [obSCEne](../obscene/) probe run on the hardware through [Prosperous](../prosperous/).
   `orbistoun-cli probe` reads the resulting transcript and reports what it establishes; it
   opens no connection and dispatches nothing. An implementation written from that measurement
   is tagged `known_by = "measured"`.
4. **Verification.** The title runs again. `FURTHER` keeps the change; `BACK` rejects it.

[docs/THE_LOOP.md](docs/THE_LOOP.md) specifies the loop in full.

## Layout

The workspace crates are under `crates/`; [docs/CRATES.md](docs/CRATES.md) says what each one
is for. A few of them:

```
crates/
  orbistoun-elf       ELF and container parsing
  orbistoun-nid       NID hashing and symbol-name resolution
  orbistoun-loader    parse, reserve, resolve, relocate, TLS, entry
  orbistoun-hle       module registry, guest_module!, stub policy
  orbistoun-gpu       command-stream translation
  orbistoun-shader    guest shader bytecode decoding
  orbistoun-turn      mechanical findings
  orbistoun-report    traces, the progress verdict, ranked findings
  orbistoun-cli       the orbistoun-cli binary
```

## Related projects

- [OOPS](../README.md) - the collection and its shared build instructions.
- [The OOPS loop](../docs/THE_LOOP.md) - how the collection's projects feed each other.
- [obSCEne](../obscene/) - the hardware conformance probe.
- [Prosperous](../prosperous/) - remote hardware management: deploying payloads, streaming logs.
- [SELFish](../selfish/) - the platform container formats: building and unpacking.
- [oops-apps](../oops-apps/) - homebrew titles used as test guests.
