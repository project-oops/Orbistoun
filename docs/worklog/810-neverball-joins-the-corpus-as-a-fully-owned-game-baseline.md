# 810. Neverball (`NVRB00001`) joins the corpus as the second fully-owned baseline — a real game that boots in-game on hardware — and its first run reaches `flipped` with 7,242 frames

**2026-09-24** — the cube (`GLCB00001`) proved the rendering path can be driven one honest function at
a time against a guest whose every line is our own. Neverball is the same kind of guest at game scale:
the upstream open-source game ported onto oops-sdk and oops-gl, which boots and plays on the console.
Everything beneath it is our source, and the console image is known-good, so every wall it hits is
orbistoun's (D708) and framebuffer-diffing against the hardware image is a usable correctness signal.

## How it came in

The package is assembled the way the port's own `make package` assembles it, without touching the
port: the built title directory (`eboot.bin`, `sce_module/libc.prx`, `sce_sys/`) plus the compiled
upstream data tree as `data/` — 407 compiled levels, fonts, textures and music, 257 MB in all. It lives
in the data directory's title library beside `GLCB00001`, as the cube does. It is not a `corpus/sources.toml`
asset: `corpus sync` copies named files one at a time, and a data tree of this size is not a list of
assets. The eboot in the library is byte-identical to the one the port built.

## The first run

```
recorded [experiment] flipped - 49 imports, 20000000 calls, 96% standing
outcome: spent its call budget
```

The fixes the cube drove through the display path (the buffer attributes, `RegisterBuffers2`'s struct
array, the queue handle) serve this guest too, so Neverball creates its window, initialises the hardware GL pipeline and flips 7,242 frames. What
it shows, in the order the loop should take it:

- **Its own data is not found.** `fs_load found nothing` at `ttf/DejaVuSans-Bold.ttf`, and the `classic`
  theme fails to open — yet both are present under `/app0/data/`. The port finds its data by walking out
  from `argv[0]` (`/app0/neverball`), so some path, stat or directory call on that walk answers wrongly.
  Without data it has no fonts, no theme and no levels, so this comes first.
- **The same submit wall as the cube.** *The driver refused the command buffer*: the submit hash
  `0x145f597e80e9876f` has no name, so the hardware clear self-test fails and drawing stops.
- **A stubbed event pump spends the budget.** `sceSystemServiceReceiveEvent` has no name in the recorded
  library and answers the placeholder 978,709 times — 4% of the run, and all but 14 of the calls that land on a stub.
- **Keyboard init is unimplemented** (`sceKeyboardInit` and friends answer the placeholder); the game
  carries on without it.

## Gate state

No source change: the run wrote `compat/NVRB00001.toml` and `docs/titles/NVRB00001.md`, and the
generated `PROJECT_STATUS.md` and `COMPATIBILITY.md` are regenerated with it. `./bin/orbistoun check`
green, worklog index regenerated, identity scan clean. No commit.
