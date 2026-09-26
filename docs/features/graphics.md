# Graphics

A guest draws by building command buffers in the vendor command-stream format and submitting
them. Orbistoun decodes those buffers, translates the guest's shaders to SPIR-V, and executes
the result on a Vulkan device. The frame a guest flips is shown in the window.

## The pipeline

| Stage | Crate | Does |
|---|---|---|
| decode | `orbistoun-gpu` | walks the command packets, keeps the register file, assembles pipelines. It has no dependency on a host graphics API. |
| shaders | `orbistoun-shader`, `orbistoun-translate`, `orbistoun-spirv` | decode the vendor shader bytecode and translate it to SPIR-V |
| execution | `orbistoun-gpu-vulkan` | makes resources resident, runs dispatches and draws, and reads results back. The only crate that names a graphics API. |

A guest's geometry stage translates to a mesh shader, so a device without mesh shading refuses
a draw that needs one rather than issuing it. A command the backend cannot perform is refused
by name, so an unimplemented command is never mistaken for a rendering fault. Translated
shaders are checked by executing them on a real device and comparing the values they produce.

## In the window

While a title runs, the right side of the window shows the last frame it flipped, as large as
fits with its aspect kept, on black. Until the first frame arrives the window says the title
is starting. In the shell view the frame fills the window.

### Performance overlay

`F3` shows and hides an overlay over the running title's picture, updated about once a
second:

| Line | Shows |
|---|---|
| fps | frames flipped per second |
| submissions/s, draws/s | how many command buffers and draws those frames took |
| gpu busy | the share of the second the device was working |
| shares | each part of presenting a frame as a percentage of the second, the largest highlighted, with the remainder (the guest's own time and anything unmeasured) listed as such |

### Renderer

The window itself draws through `wgpu`, and reports which backend and adapter it got: printed
to the terminal at startup as `orbistoun: renderer: <backend> - <adapter>`, and shown in the
shell view's footer.

## Diagnostics

| Variable | Does |
|---|---|
| `ORBISTOUN_TARGET_WRITEBACK=flip\|submit` | write a drawn target back at the flip (default) or after every submission |
| `ORBISTOUN_TRACE_SUBMITS=1` | print a line for every submission whose draws ran; refusals are printed regardless |
| `ORBISTOUN_PERF_DETAIL=1` | print each submission span's time once a second, beside the performance phases |
| `ORBISTOUN_PROFILE=<n>` | sample the guest's main and device threads and print where they spend their time |
| `ORBISTOUN_FLIP_TO_ALL=1` | post a flip completion to every queue |

```bash
orbistoun-cli shaders <directory> --top 10   # rank the instructions that block translation across a directory of shader binaries
```
