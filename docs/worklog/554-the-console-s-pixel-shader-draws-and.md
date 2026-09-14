# 554. The console's pixel shader draws, and five validation errors nobody was watching

**2026-09-14** - orbistoun-gpu-vulkan, after worklog 552, and D689

Worklog 552 ended with a module: the GL cube's untextured pixel shader, run on a console,
translated here, and accepted by `spirv-val`. The obvious next question was whether it runs.
It does - every pixel of an eight by five attachment comes back the colour the shader
computed, over a clear it never writes.

The useful part of this unit is what happened in between.

## 1. It drew, and it was not correct

The first draw produced the right picture immediately. Under the Khronos validation layer the
same draw produced **five errors**, and the picture was the NVIDIA driver being tolerant of a
pipeline it was entitled to refuse.

| what the layer said | what it was |
|---|---|
| `Float16` declared, `shaderFloat16` never enabled | every module this project emits declares it |
| `Int16` declared, `shaderInt16` never enabled | likewise |
| the layout does not declare set 0 binding 1 | the draw path used an empty pipeline layout |
| a fragment stage writes a storage buffer without `fragmentStoresAndAtomics` | the guest's shader writes its canary |
| the pipeline statically uses set 0 and nothing was bound | no descriptor set existed to bind |

The first two are not about this shader or this path. Both models declare 16-bit types in their
headers, so **every compute dispatch in the test suite has been running on capabilities the
device was never asked for**, since the day there was a device. Nothing failed, because nothing
was watching.

That is the shape of fault this project already has a rule about, one level up from the
translator: a report that says more than its measurement supports. Here the report was a
passing test, and what it measured was "this driver did not object".

## 2. What changed

**The device asks for what the modules declare**: `shaderInt16`, `shaderFloat16` and
`fragmentStoresAndAtomics`, each where the physical device offers it. D552 had recorded that
the fragment path was designed not to need the third, which was true of the hand-written
modules and is not true of the console's. D689 supersedes that half and says why the
alternatives are worse.

**The draw path binds the two storage buffers** a translated module declares - a set layout, a
pool, a set, and a bind before the draw, the shape the compute path already had. The window
sizes are parameters with a default, because they belong to the module being run.

The capability report's rule is unchanged and its test is stronger. It used to assert that the
report claims no fragment stores, which pinned an answer; it now asserts the report matches
what the session enabled, which pins the rule. The report already derived that from the request
rather than hard-coding it, so it updated itself - that was somebody's foresight paying off.

## 3. The guard, and the one thing it is meant to say

`tools/validate-device.sh` runs the device tests under the validation layer and fails if
anything reports an error. It is not in `check`: CI has no device, and the layer is somebody's
own SDK install.

One test is skipped by name, `a_malformed_module_is_rejected_rather_than_run`, which hands the
driver a deliberate nonsense opcode and asserts it is refused. It reports a validation error
every time, correctly. Skipping it by name rather than filtering the message is the point -
a filter that matched on text would quietly swallow a real error that happened to look similar.

Every other device test, including the four that were already passing, now runs with the layer
silent.

## 4. What the draw does and does not say

The console drew this shader over its own vertex program, which does not translate yet (D688),
so the geometry here is the oracle's hand-written triangle and the corners are all one colour.
What is asserted is that the shader runs, interpolates what it was given, and exports it.

**These are not the console's pixels**, and the oracle record's frame hash is a different claim
that needs the other half of the pipeline. Said plainly in the test, because a translated shader
drawing *a* correct picture is exactly the kind of result that gets remembered as more than it
was.

## 5. One failure I could not reproduce

Twice during this unit a `cargo test --workspace` run ended with one failure and about a third
of the tests reported, in both cases when two workspace runs were invoked from one command
line. Five standalone runs afterwards were clean at 2,292 passing, as were four consecutive
runs of the device tests alone.

Written down rather than passed over, because the change in this unit adds Vulkan objects to a
path that previously allocated none, and "it went away" is not a diagnosis. What is known: no
captured log contains it, and it has not appeared in any single run before or since.

## 6. Files

- `crates/orbistoun-gpu-vulkan/src/compute.rs` - the three features, requested where offered.
- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - the storage buffers, the descriptor set,
  the bind, the release; `draw_with_windows` for a caller that knows its module's windows.
- `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the console's shader on a device.
- `crates/orbistoun-gpu-vulkan/tests/features.rs` - the report's rule, restated.
- `tools/validate-device.sh`, `docs/decisions/D689-…`.

## 7. Next

1. The image subsystem, for record B's textured shader.
2. The mesh stage (D688), after which the console's *own* geometry could be drawn and the frame
   hash becomes a question worth asking.
3. Whether the modules should declare 16-bit capabilities at all when nothing uses them. The
   device asks for them now, which closes the error; declaring less would be better still.
